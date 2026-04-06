//! # transcribe — Speech-to-text transcription
//!
//! WHY transcription: Hermes-agent provides `transcribe_audio` for
//! converting audio files to text. This enables processing voice messages
//! from gateway platforms (Telegram, Discord, WhatsApp, Slack, Signal).
//!
//! Achieves full parity with hermes-agent's transcription_tools.py:
//!
//! ```text
//!   transcribe_audio("/path/to/audio.ogg")
//!       │
//!       ├── Local whisper CLI  (default — free, no API key)
//!       │   ├── configurable command template (EDGECRAB_LOCAL_STT_COMMAND)
//!       │   ├── configurable model / language
//!       │   ├── ffmpeg conversion for non-WAV formats
//!       │   └── binary discovery in /opt/homebrew/bin, /usr/local/bin, PATH
//!       ├── Groq Whisper API  (GROQ_API_KEY — free tier, fast)
//!       └── OpenAI Whisper API (OPENAI_API_KEY — paid)
//! ```
//!
//! Supports formats: mp3, mp4, mpeg, mpga, m4a, wav, webm, ogg

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

/// Maximum audio file size (25 MB — Whisper API limit).
const MAX_FILE_SIZE: usize = 25 * 1024 * 1024;

const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "mp4", "mpeg", "mpga", "m4a", "wav", "webm", "ogg"];

/// Formats that the whisper CLI handles natively without ffmpeg conversion.
const LOCAL_NATIVE_FORMATS: &[&str] = &["wav", "aiff", "aif"];

/// Common binary directories (macOS Homebrew, Linux local).
const COMMON_BIN_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

/// Default local model name (matches hermes DEFAULT_LOCAL_MODEL).
const DEFAULT_LOCAL_MODEL: &str = "base";

/// Default local language.
const DEFAULT_LOCAL_LANGUAGE: &str = "en";

/// Known OpenAI-only model names (auto-corrected for local).
const OPENAI_MODELS: &[&str] = &["whisper-1", "gpt-4o-mini-transcribe", "gpt-4o-transcribe"];
/// Known Groq-only model names (auto-corrected for local).
const GROQ_MODELS: &[&str] = &[
    "whisper-large-v3",
    "whisper-large-v3-turbo",
    "distil-whisper-large-v3-en",
];

// ---------------------------------------------------------------------------
// Backend detection
// ---------------------------------------------------------------------------

/// Detect transcription backend.  Priority: local > groq > openai (matches hermes).
enum SttBackend {
    /// Local whisper CLI (free, no keys)
    LocalCommand { whisper_bin: String },
    /// Groq API (free-tier, fast)
    Groq { api_key: String },
    /// OpenAI Whisper API (paid)
    OpenAi { api_key: String },
    /// Nothing available
    None,
}

/// Find a binary by name, checking common Homebrew/local prefixes then PATH.
fn find_binary(name: &str) -> Option<String> {
    for dir in COMMON_BIN_DIRS {
        let candidate = PathBuf::from(dir).join(name);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    // Check PATH via `which`
    which::which(name)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Find the whisper binary.  Checks env-configured command first, then
/// `EDGECRAB_LOCAL_STT_COMMAND`, then common paths + PATH.
fn find_whisper_binary() -> Option<String> {
    // User-configured custom command template
    if let Ok(cmd) = std::env::var("EDGECRAB_LOCAL_STT_COMMAND") {
        let cmd = cmd.trim().to_string();
        if !cmd.is_empty() {
            return Some(cmd);
        }
    }
    find_binary("whisper")
}

fn find_ffmpeg_binary() -> Option<String> {
    find_binary("ffmpeg")
}

fn detect_backend() -> SttBackend {
    // Prefer local (free, no network) — matches hermes default
    if let Some(whisper_bin) = find_whisper_binary() {
        return SttBackend::LocalCommand { whisper_bin };
    }

    // Groq (free tier, very fast)
    if let Ok(key) = std::env::var("GROQ_API_KEY") {
        if !key.is_empty() {
            return SttBackend::Groq { api_key: key };
        }
    }

    // OpenAI (paid)
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        if !key.is_empty() {
            return SttBackend::OpenAi { api_key: key };
        }
    }

    SttBackend::None
}

fn is_supported_format(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Normalize model name for local use — if caller passed an API-only model,
/// fall back to the default local model (matches hermes `_normalize_local_command_model`).
fn normalize_local_model(model: &str) -> &str {
    if model.is_empty() || OPENAI_MODELS.contains(&model) || GROQ_MODELS.contains(&model) {
        DEFAULT_LOCAL_MODEL
    } else {
        model
    }
}

// ---------------------------------------------------------------------------
// Provider: local whisper CLI
// ---------------------------------------------------------------------------

/// Convert non-native audio to WAV via ffmpeg (matches hermes `_prepare_local_audio`).
async fn prepare_local_audio(file_path: &Path, work_dir: &Path) -> Result<PathBuf, ToolError> {
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // WAV/AIFF are native — no conversion needed
    if LOCAL_NATIVE_FORMATS.contains(&ext.as_str()) {
        return Ok(file_path.to_path_buf());
    }

    let ffmpeg = find_ffmpeg_binary().ok_or_else(|| ToolError::ExecutionFailed {
        tool: "transcribe_audio".into(),
        message: "Local STT requires ffmpeg for non-WAV audio, but ffmpeg was not found. \
                  Install ffmpeg or use a cloud provider (groq/openai)."
            .into(),
    })?;

    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    let converted = work_dir.join(format!("{stem}.wav"));

    let output = tokio::process::Command::new(&ffmpeg)
        .arg("-y")
        .arg("-i")
        .arg(file_path.to_string_lossy().as_ref())
        .arg(converted.to_string_lossy().as_ref())
        .output()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!("Failed to run ffmpeg: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!("ffmpeg conversion failed: {stderr}"),
        });
    }

    Ok(converted)
}

/// Transcribe via local whisper CLI with full hermes parity:
/// - Configurable model and language
/// - ffmpeg conversion for non-native formats
/// - Temp directory for output with proper cleanup
/// - Custom command template support
async fn transcribe_local(
    file_path: &Path,
    whisper_bin: &str,
    model: &str,
    language: &str,
) -> Result<String, ToolError> {
    let model = normalize_local_model(model);

    // Create a dedicated temp dir for this transcription
    let tmp_dir = tempfile::tempdir().map_err(|e| ToolError::ExecutionFailed {
        tool: "transcribe_audio".into(),
        message: format!("Failed to create temp directory: {e}"),
    })?;

    // Prepare audio (convert via ffmpeg if needed)
    let input_path = prepare_local_audio(file_path, tmp_dir.path()).await?;

    // Check if it's a custom command template or a binary path
    let is_template = whisper_bin.contains("{input_path}") || whisper_bin.contains("{output_dir}");

    let output = if is_template {
        // Expand template placeholders (matches hermes _transcribe_local_command)
        let expanded = whisper_bin
            .replace("{input_path}", &input_path.to_string_lossy())
            .replace("{output_dir}", &tmp_dir.path().to_string_lossy())
            .replace("{language}", language)
            .replace("{model}", model);

        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&expanded)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "transcribe_audio".into(),
                message: format!("Failed to run local STT command: {e}"),
            })?
    } else {
        // Standard whisper CLI invocation
        tokio::process::Command::new(whisper_bin)
            .arg(input_path.to_string_lossy().as_ref())
            .arg("--model")
            .arg(model)
            .arg("--language")
            .arg(language)
            .arg("--output_format")
            .arg("txt")
            .arg("--output_dir")
            .arg(tmp_dir.path().to_string_lossy().as_ref())
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "transcribe_audio".into(),
                message: format!("Failed to run whisper: {e}"),
            })?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!("Local STT failed: {stderr}"),
        });
    }

    // Read transcript from .txt output files (whisper outputs <stem>.txt)
    let mut txt_files: Vec<_> = std::fs::read_dir(tmp_dir.path())
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!("Failed to read output directory: {e}"),
        })?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|e| e == "txt")
                .unwrap_or(false)
        })
        .collect();
    txt_files.sort_by_key(|e| e.file_name());

    if let Some(txt_entry) = txt_files.first() {
        let text = tokio::fs::read_to_string(txt_entry.path())
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "transcribe_audio".into(),
                message: format!("Failed to read whisper output: {e}"),
            })?;
        Ok(text.trim().to_string())
    } else {
        // Fallback: whisper may have printed to stdout
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().to_string())
    }
    // tmp_dir is dropped here → auto-cleanup
}

// ---------------------------------------------------------------------------
// Provider: Groq Whisper API (free tier)
// ---------------------------------------------------------------------------

async fn transcribe_groq(
    file_path: &Path,
    api_key: &str,
    model: &str,
) -> Result<String, ToolError> {
    let model = if model.is_empty() || LOCAL_NATIVE_FORMATS.contains(&model) {
        std::env::var("STT_GROQ_MODEL").unwrap_or_else(|_| "whisper-large-v3-turbo".into())
    } else {
        // Auto-correct OpenAI-only models for Groq
        if OPENAI_MODELS.contains(&model) {
            "whisper-large-v3-turbo".into()
        } else {
            model.to_string()
        }
    };
    let base_url =
        std::env::var("GROQ_BASE_URL").unwrap_or_else(|_| "https://api.groq.com/openai/v1".into());

    transcribe_openai_compatible(file_path, api_key, &base_url, &model).await
}

// ---------------------------------------------------------------------------
// Provider: OpenAI Whisper API (paid)
// ---------------------------------------------------------------------------

async fn transcribe_openai(
    file_path: &Path,
    api_key: &str,
    model: &str,
) -> Result<String, ToolError> {
    let model = if model.is_empty() {
        std::env::var("STT_OPENAI_MODEL").unwrap_or_else(|_| "whisper-1".into())
    } else {
        // Auto-correct Groq-only models for OpenAI
        if GROQ_MODELS.contains(&model) {
            "whisper-1".into()
        } else {
            model.to_string()
        }
    };
    let base_url =
        std::env::var("STT_OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into());

    transcribe_openai_compatible(file_path, api_key, &base_url, &model).await
}

// ---------------------------------------------------------------------------
// Shared OpenAI-compatible API implementation
// ---------------------------------------------------------------------------

async fn transcribe_openai_compatible(
    file_path: &Path,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<String, ToolError> {
    let file_bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!("Failed to read audio file: {e}"),
        })?;

    if file_bytes.len() > MAX_FILE_SIZE {
        return Err(ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!(
                "Audio file too large: {:.1} MB (max {} MB)",
                file_bytes.len() as f64 / (1024.0 * 1024.0),
                MAX_FILE_SIZE / (1024 * 1024)
            ),
        });
    }

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.mp3")
        .to_string();

    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(filename)
        .mime_str("application/octet-stream")
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!("Failed to create form part: {e}"),
        })?;

    let form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .text("response_format", "text".to_string())
        .part("file", file_part);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!("HTTP client error: {e}"),
        })?;

    let url = format!("{}/audio/transcriptions", base_url.trim_end_matches('/'));

    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!("Transcription API error: {e}"),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            tool: "transcribe_audio".into(),
            message: format!("Transcription API returned {status}: {body}"),
        });
    }

    let text = resp.text().await.map_err(|e| ToolError::ExecutionFailed {
        tool: "transcribe_audio".into(),
        message: format!("Failed to read transcript response: {e}"),
    })?;

    Ok(text.trim().to_string())
}

// ─── TranscribeAudioTool ──────────────────────────────────────────────

pub struct TranscribeAudioTool;

#[derive(Deserialize)]
struct TranscribeArgs {
    file_path: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

#[async_trait]
impl ToolHandler for TranscribeAudioTool {
    fn name(&self) -> &'static str {
        "transcribe_audio"
    }

    fn toolset(&self) -> &'static str {
        "media"
    }

    fn emoji(&self) -> &'static str {
        "🎤"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "transcribe_audio".into(),
            description:
                "Transcribe speech from an audio file to text. Uses local whisper (free, default), \
                 Groq Whisper (free tier), or OpenAI Whisper. \
                 Auto-converts non-WAV formats via ffmpeg for local mode. \
                 Supports: mp3, mp4, mpeg, mpga, m4a, wav, webm, ogg."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the audio file to transcribe"
                    },
                    "provider": {
                        "type": "string",
                        "enum": ["local", "groq", "openai"],
                        "description": "Force a specific transcription provider (default: auto-detect, prefers local)"
                    },
                    "model": {
                        "type": "string",
                        "description": "Model name (local: 'tiny'|'base'|'small'|'medium'|'large', groq: 'whisper-large-v3-turbo', openai: 'whisper-1')"
                    },
                    "language": {
                        "type": "string",
                        "description": "Language code for transcription (default: 'en'). Only used with local provider."
                    }
                },
                "required": ["file_path"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        !matches!(detect_backend(), SttBackend::None)
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let args: TranscribeArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "transcribe_audio".into(),
                message: e.to_string(),
            })?;

        let path = PathBuf::from(&args.file_path);
        let abs_path = if path.is_absolute() {
            path
        } else {
            ctx.cwd.join(&path)
        };

        // Validate file exists
        if !abs_path.exists() {
            return Err(ToolError::ExecutionFailed {
                tool: "transcribe_audio".into(),
                message: format!("Audio file not found: {}", abs_path.display()),
            });
        }

        // Validate format
        if !is_supported_format(&abs_path) {
            return Err(ToolError::InvalidArgs {
                tool: "transcribe_audio".into(),
                message: format!(
                    "Unsupported audio format. Supported: {}",
                    SUPPORTED_EXTENSIONS.join(", ")
                ),
            });
        }

        let model = args
            .model
            .as_deref()
            .or(ctx.config.stt_whisper_model.as_deref())
            .unwrap_or("");
        let language = args.language.as_deref().unwrap_or(DEFAULT_LOCAL_LANGUAGE);

        // Select backend
        let backend = if let Some(ref provider) = args.provider {
            match provider.as_str() {
                "local" => {
                    if let Some(whisper_bin) = find_whisper_binary() {
                        SttBackend::LocalCommand { whisper_bin }
                    } else {
                        return Err(ToolError::Unavailable {
                            tool: "transcribe_audio".into(),
                            reason: "Local whisper not available. Install whisper CLI \
                                     or set EDGECRAB_LOCAL_STT_COMMAND."
                                .into(),
                        });
                    }
                }
                "groq" => {
                    let key =
                        std::env::var("GROQ_API_KEY").map_err(|_| ToolError::Unavailable {
                            tool: "transcribe_audio".into(),
                            reason: "GROQ_API_KEY not set".into(),
                        })?;
                    SttBackend::Groq { api_key: key }
                }
                "openai" => {
                    let key = std::env::var("VOICE_TOOLS_OPENAI_KEY")
                        .or_else(|_| std::env::var("OPENAI_API_KEY"))
                        .map_err(|_| ToolError::Unavailable {
                            tool: "transcribe_audio".into(),
                            reason: "OPENAI_API_KEY not set".into(),
                        })?;
                    SttBackend::OpenAi { api_key: key }
                }
                other => {
                    return Err(ToolError::InvalidArgs {
                        tool: "transcribe_audio".into(),
                        message: format!("Unknown provider '{other}'. Use: local, groq, openai"),
                    });
                }
            }
        } else {
            match ctx.config.stt_provider.as_deref() {
                Some("local") => {
                    if let Some(whisper_bin) = find_whisper_binary() {
                        SttBackend::LocalCommand { whisper_bin }
                    } else {
                        SttBackend::None
                    }
                }
                Some("groq") => std::env::var("GROQ_API_KEY")
                    .map(|api_key| SttBackend::Groq { api_key })
                    .unwrap_or(SttBackend::None),
                Some("openai") => std::env::var("VOICE_TOOLS_OPENAI_KEY")
                    .or_else(|_| std::env::var("OPENAI_API_KEY"))
                    .map(|api_key| SttBackend::OpenAi { api_key })
                    .unwrap_or(SttBackend::None),
                Some(_) | None => detect_backend(),
            }
        };

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let (transcript, provider_name) = match backend {
            SttBackend::LocalCommand { whisper_bin } => {
                let t = transcribe_local(&abs_path, &whisper_bin, model, language).await?;
                (t, "local")
            }
            SttBackend::Groq { api_key } => {
                let t = transcribe_groq(&abs_path, &api_key, model).await?;
                (t, "groq")
            }
            SttBackend::OpenAi { api_key } => {
                let t = transcribe_openai(&abs_path, &api_key, model).await?;
                (t, "openai")
            }
            SttBackend::None => {
                return Err(ToolError::Unavailable {
                    tool: "transcribe_audio".into(),
                    reason: "No transcription backend available. Install whisper CLI, \
                             set GROQ_API_KEY, or set OPENAI_API_KEY."
                        .into(),
                });
            }
        };

        if transcript.is_empty() {
            Ok(format!(
                "(No speech detected in audio file) [provider: {provider_name}]"
            ))
        } else {
            Ok(format!("Transcript (via {provider_name}):\n{transcript}"))
        }
    }
}

inventory::submit!(&TranscribeAudioTool as &dyn ToolHandler);

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_valid() {
        let schema = TranscribeAudioTool.schema();
        assert_eq!(schema.name, "transcribe_audio");
        let required = schema.parameters["required"].as_array().expect("array");
        assert!(required.iter().any(|v| v == "file_path"));
        // Verify model and language params exist
        let props = schema.parameters["properties"].as_object().expect("object");
        assert!(props.contains_key("model"));
        assert!(props.contains_key("language"));
        assert!(props.contains_key("provider"));
    }

    #[test]
    fn format_detection() {
        assert!(is_supported_format(Path::new("audio.mp3")));
        assert!(is_supported_format(Path::new("voice.ogg")));
        assert!(is_supported_format(Path::new("recording.wav")));
        assert!(is_supported_format(Path::new("clip.webm")));
        assert!(is_supported_format(Path::new("voice.m4a")));
        assert!(is_supported_format(Path::new("file.mpga")));
        assert!(is_supported_format(Path::new("VIDEO.MP4")));
        assert!(!is_supported_format(Path::new("document.pdf")));
        assert!(!is_supported_format(Path::new("image.png")));
        assert!(!is_supported_format(Path::new("archive.zip")));
    }

    #[test]
    fn tool_metadata() {
        assert_eq!(TranscribeAudioTool.name(), "transcribe_audio");
        assert_eq!(TranscribeAudioTool.toolset(), "media");
        assert_eq!(TranscribeAudioTool.emoji(), "🎤");
    }

    #[test]
    fn normalize_model_for_local() {
        assert_eq!(normalize_local_model("base"), "base");
        assert_eq!(normalize_local_model("large"), "large");
        assert_eq!(normalize_local_model("tiny"), "tiny");
        // API-only models should fall back to default
        assert_eq!(normalize_local_model("whisper-1"), DEFAULT_LOCAL_MODEL);
        assert_eq!(
            normalize_local_model("whisper-large-v3-turbo"),
            DEFAULT_LOCAL_MODEL
        );
        assert_eq!(normalize_local_model(""), DEFAULT_LOCAL_MODEL);
    }

    #[test]
    fn native_format_check() {
        // WAV/AIFF don't need ffmpeg conversion
        assert!(LOCAL_NATIVE_FORMATS.contains(&"wav"));
        assert!(LOCAL_NATIVE_FORMATS.contains(&"aiff"));
        assert!(!LOCAL_NATIVE_FORMATS.contains(&"mp3"));
        assert!(!LOCAL_NATIVE_FORMATS.contains(&"ogg"));
    }
}
