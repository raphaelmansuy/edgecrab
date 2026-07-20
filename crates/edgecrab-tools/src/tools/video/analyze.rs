//! `video_analyze` — sample frames with ffmpeg for vision follow-up.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

use edgecrab_security::url_safety::is_safe_url;
use edgecrab_types::{ToolError, ToolSchema};

use crate::path_utils::jail_read_path;
use crate::registry::{ToolContext, ToolHandler};

pub struct VideoAnalyzeTool;

#[derive(Debug, Deserialize)]
struct AnalyzeArgs {
    /// Local path or http(s) URL.
    path: String,
    #[serde(default = "default_frames")]
    max_frames: u32,
    #[serde(default)]
    question: Option<String>,
}

fn default_frames() -> u32 {
    8
}

fn ffmpeg_bin() -> String {
    std::env::var("EDGECRAB_FFMPEG").unwrap_or_else(|_| "ffmpeg".into())
}

fn sample_frames(
    ffmpeg: &str,
    input: &Path,
    out_dir: &Path,
    max_frames: u32,
) -> Result<Vec<PathBuf>, ToolError> {
    std::fs::create_dir_all(out_dir).map_err(|e| ToolError::ExecutionFailed {
        tool: "video_analyze".into(),
        message: format!("temp dir: {e}"),
    })?;
    let pattern = out_dir.join("frame_%03d.jpg");
    let fps = (max_frames.clamp(1, 32) as f64 / 30.0).max(0.1);
    let status = Command::new(ffmpeg)
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input)
        .arg("-vf")
        .arg(format!("fps={fps}"))
        .arg("-frames:v")
        .arg(max_frames.clamp(1, 32).to_string())
        .arg("-y")
        .arg(&pattern)
        .status()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "video_analyze".into(),
            message: format!(
                "ffmpeg failed to start ({ffmpeg}): {e}. Install ffmpeg or set EDGECRAB_FFMPEG."
            ),
        })?;
    if !status.success() {
        return Err(ToolError::ExecutionFailed {
            tool: "video_analyze".into(),
            message: format!("ffmpeg exited with {status}"),
        });
    }
    let mut frames = Vec::new();
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("jpg") {
                frames.push(p);
            }
        }
    }
    frames.sort();
    if frames.is_empty() {
        return Err(ToolError::ExecutionFailed {
            tool: "video_analyze".into(),
            message: "ffmpeg produced no frames".into(),
        });
    }
    Ok(frames)
}

#[async_trait]
impl ToolHandler for VideoAnalyzeTool {
    fn name(&self) -> &'static str {
        "video_analyze"
    }
    fn toolset(&self) -> &'static str {
        "video"
    }
    fn emoji(&self) -> &'static str {
        "🎬"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "video_analyze".into(),
            description: "Sample frames from a local video (or URL) with ffmpeg and return \
                 frame paths for vision_analyze. Opt-in toolset `video`."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Local video path or http(s) URL" },
                    "max_frames": { "type": "integer", "minimum": 1, "maximum": 32, "default": 8 },
                    "question": { "type": "string", "description": "Optional analysis question" }
                },
                "required": ["path"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: AnalyzeArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "video_analyze".into(),
                message: e.to_string(),
            })?;

        let temp = tempfile::TempDir::new().map_err(|e| ToolError::ExecutionFailed {
            tool: "video_analyze".into(),
            message: e.to_string(),
        })?;

        let local_path = if args.path.starts_with("http://") || args.path.starts_with("https://") {
            match is_safe_url(&args.path) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(ToolError::ExecutionFailed {
                        tool: "video_analyze".into(),
                        message: "URL blocked by SSRF policy".into(),
                    });
                }
                Err(e) => {
                    return Err(ToolError::ExecutionFailed {
                        tool: "video_analyze".into(),
                        message: format!("URL validation failed: {e}"),
                    });
                }
            }
            let dest = temp.path().join("input.bin");
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .map_err(|e| ToolError::ExecutionFailed {
                    tool: "video_analyze".into(),
                    message: e.to_string(),
                })?;
            let bytes = client
                .get(&args.path)
                .send()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool: "video_analyze".into(),
                    message: format!("download failed: {e}"),
                })?
                .bytes()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool: "video_analyze".into(),
                    message: format!("download body: {e}"),
                })?;
            if bytes.len() > 500 * 1024 * 1024 {
                return Err(ToolError::ExecutionFailed {
                    tool: "video_analyze".into(),
                    message: "video exceeds 500 MB limit".into(),
                });
            }
            std::fs::write(&dest, &bytes).map_err(|e| ToolError::ExecutionFailed {
                tool: "video_analyze".into(),
                message: e.to_string(),
            })?;
            dest
        } else {
            let policy = ctx.config.file_path_policy(&ctx.cwd);
            jail_read_path(&args.path, &policy)?
        };

        let frames_dir = temp.path().join("frames");
        let frames = sample_frames(&ffmpeg_bin(), &local_path, &frames_dir, args.max_frames)?;

        let persist = ctx
            .cwd
            .join(".edgecrab")
            .join("artifacts")
            .join(&ctx.session_id)
            .join("video_frames");
        std::fs::create_dir_all(&persist).map_err(|e| ToolError::ExecutionFailed {
            tool: "video_analyze".into(),
            message: e.to_string(),
        })?;
        let mut persisted = Vec::new();
        for (i, src) in frames.iter().enumerate() {
            let dest = persist.join(format!("frame_{i:03}.jpg"));
            std::fs::copy(src, &dest).map_err(|e| ToolError::ExecutionFailed {
                tool: "video_analyze".into(),
                message: e.to_string(),
            })?;
            persisted.push(dest.display().to_string());
        }

        let question = args
            .question
            .unwrap_or_else(|| "Describe key visual events in order.".into());

        Ok(json!({
            "success": true,
            "frame_count": persisted.len(),
            "frames": persisted,
            "question": question,
            "hint": "Pass frames to vision_analyze for multimodal description."
        })
        .to_string())
    }
}

inventory::submit!(&VideoAnalyzeTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_ok() {
        assert_eq!(VideoAnalyzeTool.schema().name, "video_analyze");
    }
}
