//! # tts — Text-to-speech conversion
//!
//! WHY TTS: Voice output is a key feature of hermes-agent for accessibility
//! and hands-free workflows. EdgeCrab supports three backends:
//!
//! ```text
//!   text_to_speech("Hello world")
//!       │
//!       ├──→ ElevenLabs API (if ELEVENLABS_API_KEY is set)
//!       │         └──→ POST /v1/text-to-speech/{voice} → save to file
//!       │
//!       ├──→ OpenAI TTS API (if OPENAI_API_KEY is set)
//!       │         └──→ POST /v1/audio/speech → save to file
//!       │
//!       └──→ edge-tts (free, no key) — default fallback
//!                 └──→ subprocess: edge-tts --text "..." -o output.mp3
//! ```
//!
//! Provider can be forced via `tts.provider` in config.yaml or auto-detected.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

/// Default voice for edge-tts (Microsoft Edge neural voices).
const DEFAULT_EDGE_TTS_VOICE: &str = "en-US-AriaNeural";

/// Default voice for OpenAI TTS.
const DEFAULT_OPENAI_VOICE: &str = "alloy";

/// Default voice ID for ElevenLabs.
const DEFAULT_ELEVENLABS_VOICE_ID: &str = "21m00Tcm4TlvDq8ikWAM"; // Rachel

/// Check if the ElevenLabs API key is available for TTS.
fn elevenlabs_tts_available() -> bool {
    std::env::var("ELEVENLABS_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

/// Check if the edge-tts Python package is available.
fn edge_tts_available() -> bool {
    std::process::Command::new("edge-tts")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if the OpenAI API key is available for TTS.
fn openai_tts_available() -> bool {
    std::env::var("OPENAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

/// Determine the TTS backend to use.
enum TtsBackend {
    ElevenLabs,
    OpenAi,
    EdgeTts,
    None,
}

fn detect_backend() -> TtsBackend {
    // Check if user has configured a preferred provider via env
    if let Ok(pref) = std::env::var("EDGECRAB_TTS_PROVIDER") {
        match pref.to_lowercase().as_str() {
            "elevenlabs" | "eleven" if elevenlabs_tts_available() => return TtsBackend::ElevenLabs,
            "openai" if openai_tts_available() => return TtsBackend::OpenAi,
            "edge-tts" | "edge" if edge_tts_available() => return TtsBackend::EdgeTts,
            _ => {} // fall through to auto-detect
        }
    }
    // Auto-detect: prefer edge-tts (free), then elevenlabs, then openai
    if edge_tts_available() {
        return TtsBackend::EdgeTts;
    }
    if elevenlabs_tts_available() {
        return TtsBackend::ElevenLabs;
    }
    if openai_tts_available() {
        return TtsBackend::OpenAi;
    }
    TtsBackend::None
}

/// Generate speech using edge-tts subprocess.
async fn tts_edge(text: &str, voice: &str, output_path: &Path) -> Result<String, ToolError> {
    let output = tokio::process::Command::new("edge-tts")
        .args([
            "--text",
            text,
            "--voice",
            voice,
            "--write-media",
            &output_path.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("Failed to run edge-tts: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("edge-tts failed: {stderr}"),
        });
    }

    Ok(output_path.to_string_lossy().into_owned())
}

/// Generate speech using OpenAI TTS API.
async fn tts_openai(text: &str, voice: &str, output_path: &Path) -> Result<String, ToolError> {
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| ToolError::Unavailable {
        tool: "text_to_speech".into(),
        reason: "OPENAI_API_KEY not set".into(),
    })?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("HTTP client error: {e}"),
        })?;

    let resp = client
        .post("https://api.openai.com/v1/audio/speech")
        .bearer_auth(&api_key)
        .json(&json!({
            "model": "tts-1",
            "input": text,
            "voice": voice,
            "response_format": "mp3"
        }))
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("OpenAI TTS API error: {e}"),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("OpenAI TTS API returned {status}: {body}"),
        });
    }

    let bytes = resp.bytes().await.map_err(|e| ToolError::ExecutionFailed {
        tool: "text_to_speech".into(),
        message: format!("Failed to read audio response: {e}"),
    })?;

    tokio::fs::write(output_path, &bytes)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("Failed to write audio file: {e}"),
        })?;

    Ok(output_path.to_string_lossy().into_owned())
}

/// Generate speech using ElevenLabs TTS API.
async fn tts_elevenlabs(
    text: &str,
    voice_id: &str,
    output_path: &Path,
) -> Result<String, ToolError> {
    let api_key = std::env::var("ELEVENLABS_API_KEY").map_err(|_| ToolError::Unavailable {
        tool: "text_to_speech".into(),
        reason: "ELEVENLABS_API_KEY not set".into(),
    })?;

    let model_id =
        std::env::var("ELEVENLABS_MODEL_ID").unwrap_or_else(|_| "eleven_turbo_v2".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("HTTP client error: {e}"),
        })?;

    let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{voice_id}");

    let resp = client
        .post(&url)
        .header("xi-api-key", &api_key)
        .header("Content-Type", "application/json")
        .header("Accept", "audio/mpeg")
        .json(&json!({
            "text": text,
            "model_id": model_id,
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75
            }
        }))
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("ElevenLabs TTS API error: {e}"),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("ElevenLabs TTS API returned {status}: {body}"),
        });
    }

    let bytes = resp.bytes().await.map_err(|e| ToolError::ExecutionFailed {
        tool: "text_to_speech".into(),
        message: format!("Failed to read audio response: {e}"),
    })?;

    tokio::fs::write(output_path, &bytes)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "text_to_speech".into(),
            message: format!("Failed to write audio file: {e}"),
        })?;

    Ok(output_path.to_string_lossy().into_owned())
}

// ─── text_to_speech ────────────────────────────────────────────

pub struct TextToSpeechTool;

#[derive(Deserialize)]
struct TtsArgs {
    /// Text to convert to speech.
    text: String,
    /// Voice name (backend-dependent). Defaults to a sensible voice per backend.
    #[serde(default)]
    voice: Option<String>,
    /// Output file path. If omitted, saves to a temp file.
    #[serde(default)]
    output_path: Option<String>,
}

#[async_trait]
impl ToolHandler for TextToSpeechTool {
    fn name(&self) -> &'static str {
        "text_to_speech"
    }

    fn toolset(&self) -> &'static str {
        "media"
    }

    fn emoji(&self) -> &'static str {
        "🔊"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "text_to_speech".into(),
            description: "Convert text to speech audio. Uses edge-tts (free), OpenAI TTS API, \
                 or ElevenLabs API. Returns the path to the generated audio file."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to convert to speech"
                    },
                    "voice": {
                        "type": "string",
                        "description": "Voice name (e.g. 'en-US-AriaNeural' for edge-tts, 'alloy' for OpenAI)"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Output file path for the audio. Defaults to a temp file."
                    }
                },
                "required": ["text"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        edge_tts_available() || openai_tts_available() || elevenlabs_tts_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let args: TtsArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "text_to_speech".into(),
            message: e.to_string(),
        })?;

        if args.text.trim().is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "text_to_speech".into(),
                message: "Text cannot be empty".into(),
            });
        }

        // Determine output path
        let output_path = match args.output_path {
            Some(ref p) => PathBuf::from(p),
            None => {
                let tmp_dir = std::env::temp_dir().join("edgecrab_tts");
                tokio::fs::create_dir_all(&tmp_dir).await.map_err(|e| {
                    ToolError::ExecutionFailed {
                        tool: "text_to_speech".into(),
                        message: format!("Failed to create temp dir: {e}"),
                    }
                })?;
                let id = uuid::Uuid::new_v4();
                tmp_dir.join(format!("speech_{id}.mp3"))
            }
        };

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let backend = detect_backend();
        let result = match backend {
            TtsBackend::EdgeTts => {
                let voice = args.voice.as_deref().unwrap_or(DEFAULT_EDGE_TTS_VOICE);
                tts_edge(&args.text, voice, &output_path).await?
            }
            TtsBackend::ElevenLabs => {
                let voice_id = args.voice.as_deref().unwrap_or(DEFAULT_ELEVENLABS_VOICE_ID);
                tts_elevenlabs(&args.text, voice_id, &output_path).await?
            }
            TtsBackend::OpenAi => {
                let voice = args.voice.as_deref().unwrap_or(DEFAULT_OPENAI_VOICE);
                tts_openai(&args.text, voice, &output_path).await?
            }
            TtsBackend::None => {
                return Err(ToolError::Unavailable {
                    tool: "text_to_speech".into(),
                    reason: "No TTS backend available. Install edge-tts (pip install edge-tts), \
                             set OPENAI_API_KEY, or set ELEVENLABS_API_KEY."
                        .into(),
                });
            }
        };

        Ok(format!("Audio saved to: {result}"))
    }
}

inventory::submit!(&TextToSpeechTool as &dyn ToolHandler);

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tts_schema_valid() {
        let schema = TextToSpeechTool.schema();
        assert_eq!(schema.name, "text_to_speech");
        let required = schema.parameters["required"].as_array().expect("array");
        assert!(required.iter().any(|v| v == "text"));
    }

    #[test]
    fn tts_toolset() {
        assert_eq!(TextToSpeechTool.toolset(), "media");
    }

    #[test]
    fn tts_emoji() {
        assert_eq!(TextToSpeechTool.emoji(), "🔊");
    }

    #[test]
    fn detect_backend_returns_something() {
        // In CI neither may be available, but the function should not panic
        let _backend = detect_backend();
    }

    #[tokio::test]
    async fn tts_rejects_empty_text() {
        let ctx = ToolContext::test_context();
        let result = TextToSpeechTool.execute(json!({"text": "  "}), &ctx).await;
        assert!(result.is_err());
        let err = result.expect_err("empty text");
        assert!(err.to_string().contains("empty"));
    }

    #[tokio::test]
    async fn tts_rejects_missing_text() {
        let ctx = ToolContext::test_context();
        let result = TextToSpeechTool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tts_cancelled() {
        let ctx = ToolContext::test_context();
        ctx.cancel.cancel();
        let result = TextToSpeechTool
            .execute(json!({"text": "hello"}), &ctx)
            .await;
        assert!(result.is_err());
        assert!(
            result
                .expect_err("cancelled")
                .to_string()
                .contains("Cancelled")
        );
    }

    #[test]
    fn default_voices_are_not_empty() {
        assert!(!DEFAULT_EDGE_TTS_VOICE.is_empty());
        assert!(!DEFAULT_OPENAI_VOICE.is_empty());
    }
}
