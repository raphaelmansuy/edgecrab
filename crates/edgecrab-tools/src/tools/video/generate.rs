//! `video_generate` / `video_status` — async job-based video generation (gap 012).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

#[derive(Clone)]
struct VideoJob {
    id: String,
    prompt: String,
    status: String,
    created_at: u64,
    output_path: Option<String>,
}

fn job_store() -> &'static Mutex<Vec<VideoJob>> {
    static STORE: OnceLock<Mutex<Vec<VideoJob>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

pub struct VideoGenerateTool;

#[derive(Debug, Deserialize)]
struct GenerateArgs {
    prompt: String,
    #[serde(default)]
    image_url: Option<String>,
    #[serde(default)]
    backend: Option<String>,
}

#[async_trait]
impl ToolHandler for VideoGenerateTool {
    fn name(&self) -> &'static str {
        "video_generate"
    }
    fn toolset(&self) -> &'static str {
        "video"
    }
    fn emoji(&self) -> &'static str {
        "🎥"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "video_generate".into(),
            description:
                "Submit a text-to-video (or image-to-video) job. Returns job_id immediately; \
                 poll with video_status. Opt-in toolset `video`. Backends: mock (default), \
                 or provider keys when configured (OPENAI_API_KEY / RUNWAY_API_KEY / etc.)."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string" },
                    "image_url": { "type": "string", "description": "Optional image for image-to-video" },
                    "backend": { "type": "string", "description": "mock|sora|runway|veo (default mock)" }
                },
                "required": ["prompt"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: GenerateArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "video_generate".into(),
                message: e.to_string(),
            })?;
        if args.prompt.trim().is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "video_generate".into(),
                message: "prompt must not be empty".into(),
            });
        }
        let backend = args
            .backend
            .as_deref()
            .unwrap_or("mock")
            .to_ascii_lowercase();
        if backend != "mock" {
            // Provider wiring lands behind the same job API; refuse until keys are present.
            let has_key = match backend.as_str() {
                "sora" | "openai" => std::env::var("OPENAI_API_KEY").is_ok(),
                "runway" => std::env::var("RUNWAY_API_KEY").is_ok(),
                "veo" | "google" => {
                    std::env::var("GEMINI_API_KEY").is_ok()
                        || std::env::var("GOOGLE_CLOUD_PROJECT").is_ok()
                }
                _ => false,
            };
            if !has_key {
                return Err(ToolError::ExecutionFailed {
                    tool: "video_generate".into(),
                    message: format!(
                        "backend '{backend}' needs API credentials; use backend=mock for dry-run jobs"
                    ),
                });
            }
            return Err(ToolError::ExecutionFailed {
                tool: "video_generate".into(),
                message: format!(
                    "backend '{backend}' is recognized but live provider submit is not enabled in this build; use mock"
                ),
            });
        }

        let id = format!("vid_{}", uuid::Uuid::new_v4().simple());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let out = ctx
            .cwd
            .join(".edgecrab")
            .join("artifacts")
            .join(&ctx.session_id)
            .join(format!("{id}.mp4"));
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Mock: write a tiny placeholder so status can report a path.
        let _ = std::fs::write(&out, b"EDGECRAB_MOCK_VIDEO");

        let job = VideoJob {
            id: id.clone(),
            prompt: args.prompt.clone(),
            status: "completed".into(),
            created_at: now,
            output_path: Some(out.display().to_string()),
        };
        if let Ok(mut store) = job_store().lock() {
            store.push(job);
            if store.len() > 64 {
                let drain = store.len() - 64;
                store.drain(0..drain);
            }
        }

        Ok(json!({
            "success": true,
            "job_id": id,
            "status": "completed",
            "backend": "mock",
            "prompt": args.prompt,
            "image_url": args.image_url,
            "output_path": out.display().to_string(),
            "note": "Non-blocking job API. Poll video_status for long-running providers."
        })
        .to_string())
    }
}

inventory::submit!(&VideoGenerateTool as &dyn ToolHandler);

pub struct VideoStatusTool;

#[derive(Debug, Deserialize)]
struct StatusArgs {
    job_id: String,
}

#[async_trait]
impl ToolHandler for VideoStatusTool {
    fn name(&self) -> &'static str {
        "video_status"
    }
    fn toolset(&self) -> &'static str {
        "video"
    }
    fn emoji(&self) -> &'static str {
        "⏳"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "video_status".into(),
            description: "Check status of a video_generate job_id.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "job_id": { "type": "string" }
                },
                "required": ["job_id"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: StatusArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "video_status".into(),
                message: e.to_string(),
            })?;
        let store = job_store().lock().map_err(|_| ToolError::ExecutionFailed {
            tool: "video_status".into(),
            message: "job store lock poisoned".into(),
        })?;
        let Some(job) = store.iter().find(|j| j.id == args.job_id) else {
            return Err(ToolError::NotFound(format!(
                "unknown video job_id {}",
                args.job_id
            )));
        };
        Ok(json!({
            "success": true,
            "job_id": job.id,
            "status": job.status,
            "prompt": job.prompt,
            "created_at": job.created_at,
            "output_path": job.output_path,
        })
        .to_string())
    }
}

inventory::submit!(&VideoStatusTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_generate_and_status() {
        let ctx = ToolContext::test_context();
        let out = VideoGenerateTool
            .execute(json!({"prompt": "a crab dancing"}), &ctx)
            .await
            .expect("generate");
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        let job_id = v["job_id"].as_str().expect("job_id").to_string();
        let status = VideoStatusTool
            .execute(json!({"job_id": job_id}), &ctx)
            .await
            .expect("status");
        assert!(status.contains("completed"));
    }
}
