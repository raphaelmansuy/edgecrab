//! # advanced — Advanced tool implementations
//!
//! Provides real implementations for media-generation tools when API keys
//! are present, plus stub implementations for tools requiring additional
//! external infrastructure.
//!
//! ## Image Generation
//!
//! `GenerateImageTool` prefers the shared `edgequake-llm` image-generation
//! providers so Gemini and Vertex AI stay in lockstep with the upstream crate:
//! 1. **Gemini Image** (`GEMINI_API_KEY`)
//! 2. **Vertex AI Gemini Image** (`GOOGLE_CLOUD_PROJECT`)
//! 3. **Vertex AI Imagen** (`GOOGLE_CLOUD_PROJECT`, explicit provider/model)
//! 4. **FAL.ai** (`FAL_KEY`)
//! 5. **OpenAI DALL-E 3** (`OPENAI_API_KEY`) — retained as a legacy fallback
//!
//! Generated images are saved to `~/.edgecrab/generated/` and returned as
//! `MEDIA:/absolute/path` so gateway platforms can deliver them natively.

use async_trait::async_trait;
use edgequake_llm::{
    AspectRatio, FalImageGen, GeminiImageGenProvider, ImageFormat, ImageGenData, ImageGenOptions,
    ImageGenProvider, ImageGenRequest, ImageResolution, ThinkingLevel, VertexAIImageGen,
};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

use edgecrab_types::{Platform, ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

// ─── GenerateImageTool ────────────────────────────────────────────────

pub struct GenerateImageTool;

#[derive(Debug, Deserialize)]
struct GenerateImageArgs {
    prompt: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    aspect_ratio: Option<String>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    count: Option<u8>,
    #[serde(default)]
    reference_images: Vec<String>,
}

impl GenerateImageTool {
    fn has_backend() -> bool {
        std::env::var("GEMINI_API_KEY").is_ok()
            || std::env::var("GOOGLE_CLOUD_PROJECT").is_ok()
            || std::env::var("FAL_KEY").is_ok()
            || std::env::var("OPENAI_API_KEY").is_ok()
    }

    fn select_provider(
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<SelectedImageProvider, ToolError> {
        let requested = provider.map(|value| value.trim().to_ascii_lowercase());
        match requested.as_deref() {
            Some("gemini") | Some("google") => Ok(SelectedImageProvider::Gemini(
                GeminiImageGenProvider::from_env()
                    .map_err(|e| imagegen_error("generate_image", &e.to_string()))?,
            )),
            Some("vertex") | Some("vertexai") | Some("vertex-gemini") => {
                Ok(SelectedImageProvider::VertexGemini(
                    GeminiImageGenProvider::from_env_vertex_ai()
                        .map_err(|e| imagegen_error("generate_image", &e.to_string()))?,
                ))
            }
            Some("imagen") | Some("vertex-imagen") => Ok(SelectedImageProvider::VertexImagen(
                VertexAIImageGen::from_env()
                    .map_err(|e| imagegen_error("generate_image", &e.to_string()))?,
            )),
            Some("fal") => Ok(SelectedImageProvider::Fal(
                FalImageGen::from_env()
                    .map_err(|e| imagegen_error("generate_image", &e.to_string()))?,
            )),
            Some("openai") => Ok(SelectedImageProvider::OpenAi),
            Some("auto") | None => {
                if model.is_some_and(|value| value.starts_with("imagen-")) {
                    return Ok(SelectedImageProvider::VertexImagen(
                        VertexAIImageGen::from_env()
                            .map_err(|e| imagegen_error("generate_image", &e.to_string()))?,
                    ));
                }
                if std::env::var("GEMINI_API_KEY").is_ok() {
                    return Ok(SelectedImageProvider::Gemini(
                        GeminiImageGenProvider::from_env()
                            .map_err(|e| imagegen_error("generate_image", &e.to_string()))?,
                    ));
                }
                if std::env::var("GOOGLE_CLOUD_PROJECT").is_ok() {
                    return Ok(SelectedImageProvider::VertexGemini(
                        GeminiImageGenProvider::from_env_vertex_ai()
                            .map_err(|e| imagegen_error("generate_image", &e.to_string()))?,
                    ));
                }
                if std::env::var("FAL_KEY").is_ok() {
                    return Ok(SelectedImageProvider::Fal(
                        FalImageGen::from_env()
                            .map_err(|e| imagegen_error("generate_image", &e.to_string()))?,
                    ));
                }
                if std::env::var("OPENAI_API_KEY").is_ok() {
                    return Ok(SelectedImageProvider::OpenAi);
                }
                Err(ToolError::Unavailable {
                    tool: "generate_image".into(),
                    reason: "Set GEMINI_API_KEY, GOOGLE_CLOUD_PROJECT, FAL_KEY, or OPENAI_API_KEY to enable image generation."
                        .into(),
                })
            }
            Some(other) => Err(ToolError::InvalidArgs {
                tool: "generate_image".into(),
                message: format!(
                    "Unknown provider '{other}'. Use: auto, gemini, vertexai, imagen, fal, openai"
                ),
            }),
        }
    }

    fn configured_provider<'a>(
        args_provider: Option<&'a str>,
        ctx: &'a ToolContext,
    ) -> Option<&'a str> {
        match args_provider {
            Some(value) if !value.trim().is_empty() => Some(value),
            _ => ctx
                .config
                .image_provider
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
        }
    }

    fn configured_model<'a>(args_model: Option<&'a str>, ctx: &'a ToolContext) -> Option<&'a str> {
        match args_model {
            Some(value) if !value.trim().is_empty() => Some(value),
            _ => ctx
                .config
                .image_model
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
        }
    }
}

enum SelectedImageProvider {
    Gemini(GeminiImageGenProvider),
    VertexGemini(GeminiImageGenProvider),
    VertexImagen(VertexAIImageGen),
    Fal(FalImageGen),
    OpenAi,
}

impl SelectedImageProvider {
    fn name(&self) -> &'static str {
        match self {
            Self::Gemini(_) => "gemini-image",
            Self::VertexGemini(_) => "vertexai-gemini-image",
            Self::VertexImagen(_) => "vertexai-imagen",
            Self::Fal(_) => "fal",
            Self::OpenAi => "openai",
        }
    }

    fn with_model(self, model: Option<&str>) -> Self {
        let Some(model) = model.filter(|value| !value.trim().is_empty()) else {
            return self;
        };
        match self {
            Self::Gemini(provider) => Self::Gemini(provider.with_model(model)),
            Self::VertexGemini(provider) => Self::VertexGemini(provider.with_model(model)),
            Self::VertexImagen(provider) => Self::VertexImagen(provider.with_model(model)),
            Self::Fal(provider) => Self::Fal(provider.with_model(model)),
            Self::OpenAi => Self::OpenAi,
        }
    }

    fn model_label<'a>(&'a self, request: &'a ImageGenRequest) -> &'a str {
        request.model.as_deref().unwrap_or(match self {
            Self::Gemini(provider) => provider.default_model(),
            Self::VertexGemini(provider) => provider.default_model(),
            Self::VertexImagen(provider) => provider.default_model(),
            Self::Fal(provider) => provider.default_model(),
            Self::OpenAi => "dall-e-3",
        })
    }

    async fn generate(
        &self,
        request: &ImageGenRequest,
    ) -> Result<edgequake_llm::ImageGenResponse, ToolError> {
        match self {
            Self::Gemini(provider) => provider
                .generate(request)
                .await
                .map_err(|e| imagegen_error("generate_image", &e.to_string())),
            Self::VertexGemini(provider) => provider
                .generate(request)
                .await
                .map_err(|e| imagegen_error("generate_image", &e.to_string())),
            Self::VertexImagen(provider) => provider
                .generate(request)
                .await
                .map_err(|e| imagegen_error("generate_image", &e.to_string())),
            Self::Fal(provider) => provider
                .generate(request)
                .await
                .map_err(|e| imagegen_error("generate_image", &e.to_string())),
            Self::OpenAi => {
                let mut images = Vec::new();
                let count = request.options.count_or_default().max(1);
                for _ in 0..count {
                    let path = tempfile::NamedTempFile::new()
                        .map_err(|e| ToolError::Other(format!("temp image file error: {e}")))?;
                    generate_openai(
                        &request.prompt,
                        request.options.aspect_ratio_or_default(),
                        request
                            .options
                            .extra
                            .get("quality")
                            .and_then(|value| value.as_str())
                            .unwrap_or("standard"),
                        path.path(),
                    )
                    .await?;
                    let bytes = std::fs::read(path.path())
                        .map_err(|e| ToolError::Other(format!("image read failed: {e}")))?;
                    let (width, height) = request
                        .options
                        .aspect_ratio_or_default()
                        .default_dimensions();
                    images.push(edgequake_llm::GeneratedImage {
                        data: ImageGenData::Bytes(bytes),
                        width,
                        height,
                        mime_type: "image/png".into(),
                        seed: None,
                    });
                }
                Ok(edgequake_llm::ImageGenResponse {
                    images,
                    provider: "openai".into(),
                    model: "dall-e-3".into(),
                    latency_ms: 0,
                    enhanced_prompt: None,
                })
            }
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
        Self::has_backend()
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "generate_image".into(),
            description: "Generate one or more images from a text prompt using Gemini Image, Vertex AI Gemini Image, Vertex Imagen, FAL.ai, or OpenAI DALL-E 3. Respects the configured default image provider/model unless the tool call overrides them. Returns saved file paths that can be referenced as MEDIA:/absolute/path.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Detailed description of the image to generate"
                    },
                    "provider": {
                        "type": "string",
                        "description": "Optional backend override: auto, gemini, vertexai, imagen, fal, openai"
                    },
                    "model": {
                        "type": "string",
                        "description": "Optional model override, for example 'gemini-2.5-flash-image', 'gemini-3.1-flash-image-preview', or 'imagen-4.0-generate-001'"
                    },
                    "size": {
                        "type": "string",
                        "description": "Backward-compatible size preset: 'square', 'landscape', or 'portrait'. Default: square",
                        "enum": ["square", "landscape", "portrait"]
                    },
                    "aspect_ratio": {
                        "type": "string",
                        "description": "Optional aspect ratio override: square, landscape, portrait, landscape_4_3, portrait_4_3, ultrawide, auto"
                    },
                    "quality": {
                        "type": "string",
                        "description": "Quality hint: 'standard' or 'hd'. Used by providers that expose a quality/resolution knob.",
                        "enum": ["standard", "hd"]
                    },
                    "format": {
                        "type": "string",
                        "description": "Output format: png, jpeg, or webp",
                        "enum": ["png", "jpeg", "webp"]
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of images to generate (provider limits apply). Default: 1",
                        "minimum": 1,
                        "maximum": 4
                    },
                    "reference_images": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional local paths or URLs to reference images for providers that support image-conditioned generation."
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
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: GenerateImageArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "generate_image".into(),
                message: e.to_string(),
            })?;
        if args.prompt.trim().is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "generate_image".into(),
                message: "prompt is required".into(),
            });
        }

        // Ensure output directory exists
        let out_dir = ctx.config.edgecrab_home.join("generated");
        tokio::fs::create_dir_all(&out_dir)
            .await
            .map_err(|e| ToolError::Other(format!("Cannot create output dir: {e}")))?;

        let aspect_ratio = parse_aspect_ratio(args.size.as_deref(), args.aspect_ratio.as_deref())?;
        let output_format = parse_output_format(args.format.as_deref());
        let resolution = match args.quality.as_deref() {
            Some("hd") => Some(ImageResolution::TwoK),
            _ => None,
        };
        let thinking_level = match args.quality.as_deref() {
            Some("hd") => Some(ThinkingLevel::High),
            _ => None,
        };
        let mut options = ImageGenOptions {
            count: args.count,
            aspect_ratio: Some(aspect_ratio),
            output_format: Some(output_format),
            resolution,
            thinking_level,
            reference_images: args.reference_images.clone(),
            ..Default::default()
        };
        if let Some(quality) = args.quality.as_deref() {
            options.extra.insert("quality".into(), json!(quality));
            options.enhance_prompt = Some(quality == "hd");
        }
        let preferred_provider = Self::configured_provider(args.provider.as_deref(), ctx);
        let preferred_model = Self::configured_model(args.model.as_deref(), ctx);
        let mut request = ImageGenRequest::new(args.prompt.clone()).with_options(options);
        if let Some(model) = preferred_model {
            request = request.with_model(model.to_string());
        }
        let provider =
            Self::select_provider(preferred_provider, preferred_model)?.with_model(preferred_model);
        let model_label = provider.model_label(&request).to_string();
        let response = provider.generate(&request).await?;
        let file_paths = persist_generated_images(&out_dir, response.images, output_format).await?;

        Ok(json!({
            "status": "ok",
            "provider": provider.name(),
            "model": model_label,
            "files": file_paths,
            "instruction": "Reference the chosen file in your final response as MEDIA:/absolute/path to send it natively."
        })
        .to_string())
    }
}

inventory::submit!(&GenerateImageTool as &dyn ToolHandler);

// ─── OpenAI DALL-E 3 backend ──────────────────────────────────────────

/// Generate an image via OpenAI DALL-E 3 API and save to `out_path`.
async fn generate_openai(
    prompt: &str,
    aspect_ratio: AspectRatio,
    quality: &str,
    out_path: &Path,
) -> Result<(), ToolError> {
    let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    let client = reqwest::Client::new();

    let dall_e_size = match aspect_ratio {
        AspectRatio::Portrait169 | AspectRatio::Portrait43 | AspectRatio::Frame45 => "1024x1792",
        AspectRatio::Landscape169
        | AspectRatio::Landscape43
        | AspectRatio::Frame54
        | AspectRatio::Print32
        | AspectRatio::Ultrawide => "1792x1024",
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
async fn download_image(url: &str, out_path: &Path) -> Result<(), ToolError> {
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

fn imagegen_error(tool: &str, message: &str) -> ToolError {
    ToolError::ExecutionFailed {
        tool: tool.into(),
        message: message.to_string(),
    }
}

fn parse_aspect_ratio(
    size: Option<&str>,
    aspect_ratio: Option<&str>,
) -> Result<AspectRatio, ToolError> {
    if let Some(value) = aspect_ratio {
        return match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(AspectRatio::Auto),
            "square" => Ok(AspectRatio::Square),
            "landscape" | "landscape_16_9" => Ok(AspectRatio::Landscape169),
            "landscape_4_3" => Ok(AspectRatio::Landscape43),
            "portrait" | "portrait_16_9" => Ok(AspectRatio::Portrait169),
            "portrait_4_3" => Ok(AspectRatio::Portrait43),
            "ultrawide" => Ok(AspectRatio::Ultrawide),
            other => Err(ToolError::InvalidArgs {
                tool: "generate_image".into(),
                message: format!("Unsupported aspect_ratio '{other}'"),
            }),
        };
    }

    Ok(match size.unwrap_or("square") {
        "landscape" => AspectRatio::Landscape169,
        "portrait" => AspectRatio::Portrait169,
        _ => AspectRatio::Square,
    })
}

fn parse_output_format(format: Option<&str>) -> ImageFormat {
    match format.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if value == "jpeg" || value == "jpg" => ImageFormat::Jpeg,
        Some(value) if value == "webp" => ImageFormat::Webp,
        _ => ImageFormat::Png,
    }
}

fn extension_for_mime(mime_type: &str, fallback: ImageFormat) -> &'static str {
    match mime_type {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/png" => "png",
        _ => fallback.extension(),
    }
}

async fn persist_generated_images(
    out_dir: &Path,
    images: Vec<edgequake_llm::GeneratedImage>,
    fallback_format: ImageFormat,
) -> Result<Vec<String>, ToolError> {
    let mut paths = Vec::with_capacity(images.len());
    for image in images {
        let ext = extension_for_mime(&image.mime_type, fallback_format);
        let file_name = format!("{}.{}", uuid::Uuid::new_v4().simple(), ext);
        let out_path = out_dir.join(file_name);
        match image.data {
            ImageGenData::Bytes(bytes) => {
                tokio::fs::write(&out_path, &bytes)
                    .await
                    .map_err(|e| ToolError::Other(format!("Image write failed: {e}")))?;
            }
            ImageGenData::Url(url) => download_image(&url, &out_path).await?,
        }
        paths.push(out_path.to_string_lossy().to_string());
    }
    Ok(paths)
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
        assert_eq!(
            GenerateImageTool.is_available(),
            GenerateImageTool::has_backend()
        );
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
        if GenerateImageTool::has_backend() {
            return; // Skip in CI with live keys
        }
        let ctx = ToolContext::test_context();
        let result = GenerateImageTool
            .execute(json!({"prompt": "a cat"}), &ctx)
            .await;
        assert!(matches!(result, Err(ToolError::Unavailable { .. })));
    }

    #[test]
    fn aspect_ratio_parses_legacy_size() {
        assert_eq!(
            parse_aspect_ratio(Some("landscape"), None).expect("ratio"),
            AspectRatio::Landscape169
        );
        assert_eq!(
            parse_aspect_ratio(Some("portrait"), None).expect("ratio"),
            AspectRatio::Portrait169
        );
    }

    #[test]
    fn parse_output_format_defaults_to_png() {
        assert_eq!(parse_output_format(None), ImageFormat::Png);
        assert_eq!(parse_output_format(Some("jpeg")), ImageFormat::Jpeg);
        assert_eq!(parse_output_format(Some("webp")), ImageFormat::Webp);
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
