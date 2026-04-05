//! # advanced — Advanced tool implementations
//!
//! Provides real implementations for media-generation tools when API keys
//! are present, plus stub implementations for tools requiring additional
//! external infrastructure.
//!
//! ## Image Generation
//!
//! `GenerateImageTool` supports two backends, checked in order:
//! 1. **FAL.ai** (`FAL_KEY` env var) — uses `fal-ai/flux-pro` model
//! 2. **OpenAI DALL-E 3** (`OPENAI_API_KEY` env var) — 1024×1024 by default
//!
//! Generated images are saved to `~/.edgecrab/generated/` and returned as
//! `MEDIA:/absolute/path` so gateway platforms can deliver them natively.

use async_trait::async_trait;
use serde_json::json;

use edgecrab_types::{Platform, ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

// ─── GenerateImageTool ────────────────────────────────────────────────

pub struct GenerateImageTool;

impl GenerateImageTool {
    /// Returns `true` when at least one image-generation API key is set.
    fn backend() -> Option<&'static str> {
        if std::env::var("FAL_KEY").is_ok() {
            Some("fal")
        } else if std::env::var("OPENAI_API_KEY").is_ok() {
            Some("openai")
        } else {
            None
        }
    }
}

#[async_trait]
impl ToolHandler for GenerateImageTool {
    fn name(&self) -> &'static str {
        "generate_image"
    }
    fn toolset(&self) -> &'static str {
        "media"
    }
    fn emoji(&self) -> &'static str {
        "🎨"
    }

    fn is_available(&self) -> bool {
        Self::backend().is_some()
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "generate_image".into(),
            description: "Generate an image from a text description using FAL.ai FLUX or OpenAI DALL-E 3. Returns the file path of the saved image.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Detailed description of the image to generate"
                    },
                    "size": {
                        "type": "string",
                        "description": "Image size: 'square' (1024x1024), 'landscape' (1536x1024), 'portrait' (1024x1536). Default: square",
                        "enum": ["square", "landscape", "portrait"]
                    },
                    "quality": {
                        "type": "string",
                        "description": "Quality: 'standard' or 'hd'. Default: standard (DALL-E only)",
                        "enum": ["standard", "hd"]
                    }
                },
                "required": ["prompt"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let prompt = args["prompt"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "generate_image".into(),
                message: "prompt is required".into(),
            })?;
        let size = args["size"].as_str().unwrap_or("square");
        let quality = args["quality"].as_str().unwrap_or("standard");

        // Ensure output directory exists
        let out_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".edgecrab")
            .join("generated");
        tokio::fs::create_dir_all(&out_dir)
            .await
            .map_err(|e| ToolError::Other(format!("Cannot create output dir: {e}")))?;

        let filename = format!("{}.png", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let out_path = out_dir.join(&filename);

        match Self::backend() {
            Some("fal") => {
                generate_fal(prompt, size, &out_path).await?;
            }
            Some("openai") | Some(_) => {
                generate_openai(prompt, size, quality, &out_path).await?;
            }
            None => {
                return Err(ToolError::Unavailable {
                    tool: "generate_image".into(),
                    reason: "Set FAL_KEY (for FLUX) or OPENAI_API_KEY (for DALL-E 3) to enable."
                        .into(),
                });
            }
        }

        let path_str = out_path.to_string_lossy().to_string();
        Ok(format!("Image generated: MEDIA:{path_str}"))
    }
}

inventory::submit!(&GenerateImageTool as &dyn ToolHandler);

// ─── FAL.ai backend ───────────────────────────────────────────────────

/// Generate an image via FAL.ai FLUX API and save to `out_path`.
async fn generate_fal(
    prompt: &str,
    size: &str,
    out_path: &std::path::Path,
) -> Result<(), ToolError> {
    // FAL.ai image_size presets
    let image_size = match size {
        "landscape" => "landscape_16_9",
        "portrait" => "portrait_16_9",
        _ => "square_hd", // square
    };

    let fal_key = std::env::var("FAL_KEY").unwrap_or_default();
    let client = reqwest::Client::new();

    let body = json!({
        "prompt": prompt,
        "image_size": image_size,
        "num_inference_steps": 28,
        "guidance_scale": 3.5,
        "num_images": 1,
        "enable_safety_checker": false,
        "output_format": "png",
        "sync_mode": true
    });

    let resp = client
        .post("https://fal.run/fal-ai/flux-pro/v1.1")
        .header("Authorization", format!("Key {fal_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| ToolError::Other(format!("FAL request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ToolError::Other(format!("FAL API error {status}: {text}")));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ToolError::Other(format!("FAL response parse error: {e}")))?;

    let image_url = json["images"][0]["url"]
        .as_str()
        .ok_or_else(|| ToolError::Other("FAL response missing image URL".into()))?;

    download_image(image_url, out_path).await
}

// ─── OpenAI DALL-E 3 backend ──────────────────────────────────────────

/// Generate an image via OpenAI DALL-E 3 API and save to `out_path`.
async fn generate_openai(
    prompt: &str,
    size: &str,
    quality: &str,
    out_path: &std::path::Path,
) -> Result<(), ToolError> {
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    let client = reqwest::Client::new();

    let dall_e_size = match size {
        "landscape" => "1792x1024",
        "portrait" => "1024x1792",
        _ => "1024x1024",
    };

    let body = json!({
        "model": "dall-e-3",
        "prompt": prompt,
        "n": 1,
        "size": dall_e_size,
        "quality": quality,
        "response_format": "url"
    });

    let resp = client
        .post("https://api.openai.com/v1/images/generations")
        .bearer_auth(&api_key)
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| ToolError::Other(format!("DALL-E request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ToolError::Other(format!(
            "DALL-E API error {status}: {text}"
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ToolError::Other(format!("DALL-E response parse error: {e}")))?;

    let image_url = json["data"][0]["url"]
        .as_str()
        .ok_or_else(|| ToolError::Other("DALL-E response missing image URL".into()))?;

    download_image(image_url, out_path).await
}

// ─── Shared download helper ───────────────────────────────────────────

/// Download an image URL and write raw bytes to `out_path`.
async fn download_image(url: &str, out_path: &std::path::Path) -> Result<(), ToolError> {
    let resp = reqwest::get(url)
        .await
        .map_err(|e| ToolError::Other(format!("Image download failed: {e}")))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ToolError::Other(format!("Image read failed: {e}")))?;
    tokio::fs::write(out_path, &bytes)
        .await
        .map_err(|e| ToolError::Other(format!("Image write failed: {e}")))
}

// ─── SendMessageTool ──────────────────────────────────────────────────

pub struct SendMessageTool;

#[async_trait]
impl ToolHandler for SendMessageTool {
    fn name(&self) -> &'static str {
        "send_message"
    }

    fn toolset(&self) -> &'static str {
        "messaging"
    }

    fn emoji(&self) -> &'static str {
        "📨"
    }

    fn is_available(&self) -> bool {
        // Available when gateway is running OR when explicitly configured
        true
    }

    fn check_fn(&self, ctx: &ToolContext) -> bool {
        // Expose send_message only when a real outbound gateway transport is
        // available for this session. This keeps CLI/cron schemas honest while
        // still making messaging a core capability in live gateway sessions.
        ctx.platform != Platform::Cron && ctx.gateway_sender.is_some()
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "send_message".into(),
            description:
                "Send a message to a user via a platform channel, or list available targets.\n\n\
                IMPORTANT: When the user asks to send to a specific channel or person, \
                call send_message(action='list') FIRST to see available targets, then \
                send to the correct one. If the user names only a platform \
                (for example 'telegram'), use the platform home channel."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["send", "list"],
                        "description": "Action: 'send' sends a message, 'list' returns available targets."
                    },
                    "target": {
                        "type": "string",
                        "description": "Preferred delivery target. Format: 'platform' (home channel), 'platform:recipient', or 'platform:chat_id:thread_id'. Examples: 'telegram', 'telegram:-1001234567890:17', 'discord:#bot-home', 'email:alice@example.com'."
                    },
                    "platform": {
                        "type": "string",
                        "description": "Backward-compatible platform field when 'target' is not provided."
                    },
                    "recipient": {
                        "type": "string",
                        "description": "Backward-compatible recipient field. Empty or omitted uses the platform home channel."
                    },
                    "message": {
                        "type": "string",
                        "description": "Message content to send"
                    }
                },
                "required": []
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let action = args["action"].as_str().unwrap_or("send");

        let sender = ctx
            .gateway_sender
            .as_ref()
            .ok_or_else(|| ToolError::Unavailable {
                tool: "send_message".into(),
                reason: "Gateway not running — send_message requires an active gateway.".into(),
            })?;

        if action == "list" {
            return match sender.list_targets().await {
                Ok(targets) => Ok(json!({ "targets": targets }).to_string()),
                Err(e) => Err(ToolError::Other(format!("Failed to list targets: {e}"))),
            };
        }

        // action == "send"
        let (platform, recipient) = parse_send_target(&args)?;
        let message = args["message"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "send_message".into(),
                message: "'message' is required when action='send'".into(),
            })?;

        match sender.send_message(platform, recipient, message).await {
            Ok(()) => Ok(json!({
                "status": "sent",
                "platform": platform,
                "recipient": recipient,
            })
            .to_string()),
            Err(e) => Err(ToolError::Other(format!("Send failed: {e}"))),
        }
    }
}

fn parse_send_target(args: &serde_json::Value) -> Result<(&str, &str), ToolError> {
    if let Some(target) = args["target"].as_str() {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "send_message".into(),
                message: "'target' cannot be empty when provided".into(),
            });
        }

        if let Some((platform, recipient)) = trimmed.split_once(':') {
            return Ok((platform.trim(), recipient.trim()));
        }

        return Ok((trimmed, ""));
    }

    let platform = args["platform"]
        .as_str()
        .ok_or_else(|| ToolError::InvalidArgs {
            tool: "send_message".into(),
            message: "Either 'target' or 'platform' is required when action='send'".into(),
        })?;
    let recipient = args["recipient"].as_str().unwrap_or("");
    Ok((platform, recipient))
}

inventory::submit!(&SendMessageTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::GatewaySender;
    use async_trait::async_trait;

    struct MockGatewaySender;

    #[async_trait]
    impl GatewaySender for MockGatewaySender {
        async fn send_message(
            &self,
            _platform: &str,
            _recipient: &str,
            _message: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn list_targets(&self) -> Result<Vec<String>, String> {
            Ok(vec!["telegram".into()])
        }
    }

    #[test]
    fn generate_image_unavailable_without_key() {
        // is_available() / backend() depends on env vars — just verify the logic is consistent
        let has_backend = GenerateImageTool::backend().is_some();
        assert_eq!(GenerateImageTool.is_available(), has_backend);
    }

    #[test]
    fn send_message_is_available() {
        assert!(SendMessageTool.is_available());
    }

    #[test]
    fn send_message_check_fn_requires_gateway_sender() {
        let ctx = ToolContext::test_context();
        assert!(!SendMessageTool.check_fn(&ctx));

        let mut ctx = ToolContext::test_context();
        ctx.gateway_sender = Some(std::sync::Arc::new(MockGatewaySender));
        assert!(SendMessageTool.check_fn(&ctx));
    }

    #[tokio::test]
    async fn send_message_requires_gateway_sender() {
        let ctx = ToolContext::test_context();
        let result = SendMessageTool
            .execute(
                json!({"action": "send", "platform": "telegram", "recipient": "123", "message": "hi"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn generate_image_returns_unavailable_when_no_key() {
        // Only run if no API key is configured
        if GenerateImageTool::backend().is_some() {
            return; // Skip in CI with live keys
        }
        let ctx = ToolContext::test_context();
        let result = GenerateImageTool
            .execute(json!({"prompt": "a cat"}), &ctx)
            .await;
        assert!(matches!(result, Err(ToolError::Unavailable { .. })));
    }

    #[test]
    fn send_message_target_platform_only_uses_home_channel_mode() {
        let args = json!({"action": "send", "target": "telegram"});
        let (platform, recipient) = parse_send_target(&args).expect("target");
        assert_eq!(platform, "telegram");
        assert_eq!(recipient, "");
    }

    #[test]
    fn send_message_target_preserves_thread_suffix() {
        let args = json!({"action": "send", "target": "telegram:-1001234567890:17"});
        let (platform, recipient) = parse_send_target(&args).expect("target");
        assert_eq!(platform, "telegram");
        assert_eq!(recipient, "-1001234567890:17");
    }
}
