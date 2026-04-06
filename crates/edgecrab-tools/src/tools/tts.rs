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
fn elevenlabs_tts_available(api_key_env: &str) -> bool {
    std::env::var(api_key_env)
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

fn configured_elevenlabs_api_key_env(ctx: &ToolContext) -> &str {
    ctx.config
        .tts_elevenlabs_api_key_env
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("ELEVENLABS_API_KEY")
}

fn detect_backend(ctx: &ToolContext) -> TtsBackend {
    let elevenlabs_api_key_env = configured_elevenlabs_api_key_env(ctx);
    // Check if user has configured a preferred provider via env
    if let Ok(pref) = std::env::var("EDGECRAB_TTS_PROVIDER") {
        match pref.to_lowercase().as_str() {
            "elevenlabs" | "eleven" if elevenlabs_tts_available(elevenlabs_api_key_env) => {
                return TtsBackend::ElevenLabs;
            }
            "openai" if openai_tts_available() => return TtsBackend::OpenAi,
            "edge-tts" | "edge" if edge_tts_available() => return TtsBackend::EdgeTts,
            _ => {} // fall through to auto-detect
        }
    }
    if let Some(pref) = ctx.config.tts_provider.as_deref() {
        match pref.to_ascii_lowercase().as_str() {
            "elevenlabs" | "eleven" if elevenlabs_tts_available(elevenlabs_api_key_env) => {
                return TtsBackend::ElevenLabs;
            }
            "openai" if openai_tts_available() => return TtsBackend::OpenAi,
            "edge-tts" | "edge" if edge_tts_available() => return TtsBackend::EdgeTts,
            _ => {}
        }
    }
    // Auto-detect: prefer edge-tts (free), then elevenlabs, then openai
    if edge_tts_available() {
        return TtsBackend::EdgeTts;
    }
    if elevenlabs_tts_available(elevenlabs_api_key_env) {
        return TtsBackend::ElevenLabs;
    }
    if openai_tts_available() {
        return TtsBackend::OpenAi;
    }
    TtsBackend::None
}

/// Generate speech using edge-tts subprocess.
async fn tts_edge(
    text: &str,
    voice: &str,
    rate: Option<&str>,
    output_path: &Path,
) -> Result<String, ToolError> {
    let mut cmd = tokio::process::Command::new("edge-tts");
    cmd.args([
        "--text",
        text,
        "--voice",
        voice,
        "--write-media",
        &output_path.to_string_lossy(),
    ]);
    if let Some(rate) = rate.filter(|value| !value.trim().is_empty()) {
        cmd.args(["--rate", rate]);
    }
    let output = cmd.output().await.map_err(|e| ToolError::ExecutionFailed {
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
async fn tts_openai(
    text: &str,
    voice: &str,
    model: Option<&str>,
    output_path: &Path,
) -> Result<String, ToolError> {
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
            "model": model.filter(|value| !value.trim().is_empty()).unwrap_or("tts-1"),
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
    api_key_env: &str,
    voice_id: &str,
    model_id_override: Option<&str>,
    output_path: &Path,
) -> Result<String, ToolError> {
    let api_key = std::env::var(api_key_env).map_err(|_| ToolError::Unavailable {
        tool: "text_to_speech".into(),
        reason: format!("{api_key_env} not set"),
    })?;

    let model_id = model_id_override
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("ELEVENLABS_MODEL_ID").ok())
        .unwrap_or_else(|| "eleven_turbo_v2".to_string());

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
    /// Optional provider override: edge-tts, openai, elevenlabs.
    #[serde(default)]
    provider: Option<String>,
    /// Optional model override for provider-specific backends.
    #[serde(default)]
    model: Option<String>,
    /// Optional speech rate override (edge-tts only).
    #[serde(default)]
    rate: Option<String>,
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
                 or ElevenLabs API. Returns the generated file path and a MEDIA: hint \
                 so the caller can deliver the audio natively."
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
                    "provider": {
                        "type": "string",
                        "description": "Optional backend override. One of: 'edge-tts', 'openai', 'elevenlabs'."
                    },
                    "model": {
                        "type": "string",
                        "description": "Optional provider-specific model override (for example 'tts-1-hd' or an ElevenLabs model id)."
                    },
                    "rate": {
                        "type": "string",
                        "description": "Optional speech rate override for edge-tts, such as '+10%' or '-5%'."
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
        edge_tts_available()
            || openai_tts_available()
            || elevenlabs_tts_available("ELEVENLABS_API_KEY")
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

        let backend = if let Some(provider) = args.provider.as_deref() {
            match provider.to_ascii_lowercase().as_str() {
                "edge-tts" | "edge" if edge_tts_available() => TtsBackend::EdgeTts,
                "openai" if openai_tts_available() => TtsBackend::OpenAi,
                "elevenlabs" | "eleven"
                    if elevenlabs_tts_available(configured_elevenlabs_api_key_env(ctx)) =>
                {
                    TtsBackend::ElevenLabs
                }
                "edge-tts" | "edge" | "openai" | "elevenlabs" | "eleven" => TtsBackend::None,
                other => {
                    return Err(ToolError::InvalidArgs {
                        tool: "text_to_speech".into(),
                        message: format!(
                            "Unknown provider '{other}'. Use: edge-tts, openai, elevenlabs"
                        ),
                    });
                }
            }
        } else {
            detect_backend(ctx)
        };
        let result = match backend {
            TtsBackend::EdgeTts => {
                let voice = args
                    .voice
                    .as_deref()
                    .or(ctx.config.tts_voice.as_deref())
                    .unwrap_or(DEFAULT_EDGE_TTS_VOICE);
                let rate = args.rate.as_deref().or(ctx.config.tts_rate.as_deref());
                tts_edge(&args.text, voice, rate, &output_path).await?
            }
            TtsBackend::ElevenLabs => {
                let voice_id = args
                    .voice
                    .as_deref()
                    .or(ctx.config.tts_elevenlabs_voice_id.as_deref())
                    .or(ctx.config.tts_voice.as_deref())
                    .unwrap_or(DEFAULT_ELEVENLABS_VOICE_ID);
                let model = args
                    .model
                    .as_deref()
                    .or(ctx.config.tts_elevenlabs_model_id.as_deref());
                tts_elevenlabs(
                    &args.text,
                    configured_elevenlabs_api_key_env(ctx),
                    voice_id,
                    model,
                    &output_path,
                )
                .await?
            }
            TtsBackend::OpenAi => {
                let voice = args
                    .voice
                    .as_deref()
                    .or(ctx.config.tts_voice.as_deref())
                    .unwrap_or(DEFAULT_OPENAI_VOICE);
                let model = args.model.as_deref().or(ctx.config.tts_model.as_deref());
                tts_openai(&args.text, voice, model, &output_path).await?
            }
            TtsBackend::None => {
                return Err(ToolError::Unavailable {
                    tool: "text_to_speech".into(),
                    reason: "No TTS backend available. Install edge-tts (pip install edge-tts), \
                             set OPENAI_API_KEY, or configure ElevenLabs credentials."
                        .into(),
                });
            }
        };

        Ok(format!(
            "Audio saved to: {result}\nUse MEDIA:{result} to send it natively."
        ))
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
        let ctx = ToolContext::test_context();
        let _backend = detect_backend(&ctx);
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
