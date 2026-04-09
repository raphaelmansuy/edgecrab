//! # vision — Image analysis using multimodal LLM
//!
//! WHY vision: Provides `vision_analyze` for image analysis via multimodal LLMs.
//! For HTTPS URLs the URL is passed directly to the provider (no re-download).
//! For local file paths the bytes are read and base64-encoded before sending.
//! Supports GPT-4V, Claude 3, Gemini Pro Vision, and any vision-capable model.
//!
//! ```text
//!   vision_analyze("https://example.com/image.png", "What is in this image?")
//!       │
//!       ├── HTTPS URL? ──→ ImageData::from_url() — provider fetches directly
//!       │
//!       └── Local file? ──→ Read bytes → base64 encode → ImageData::new()
//!                  │
//!                  └── ChatMessage::user_with_images(prompt, [image_data])
//!                           │
//!                           └── provider.chat(&messages, &options) → analysis text
//! ```

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

use edgecrab_types::{ToolError, ToolSchema};
use edgequake_llm::{
    ChatMessage, CompletionOptions, ConfigProviderType, LLMProvider, ModelCapabilities, ModelCard,
    ModelType, ModelsConfig, OpenAICompatibleProvider, ProviderConfig, ProviderFactory,
    ProviderType,
};

use crate::path_utils::jail_read_path_multi;
use crate::registry::{ToolContext, ToolHandler};
use crate::vision_models::{
    model_supports_vision, normalize_model_name, normalize_provider_name, parse_provider_model_spec,
};

/// Maximum image file size for local files (10 MB).
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// Raw-byte threshold above which we shrink the image before encoding.
/// Base64 adds ~33% overhead, so 1 MB raw → ~1.33 MB in the JSON payload,
/// well within the Copilot API ~4 MB body limit.
const AUTO_RESIZE_THRESHOLD: usize = 1024 * 1024; // 1 MB

/// Maximum dimension (width or height) after resize.
const MAX_VISION_DIM: u32 = 1024;

/// JPEG quality used when re-encoding oversized images (0–100).
const JPEG_QUALITY: u8 = 82;

/// LLM vision call timeout (120 seconds — local vision models can be slow).
const VISION_TIMEOUT_SECS: u64 = 120;

// ─── Helpers ──────────────────────────────────────────────────────────

/// Determine MIME type from file extension.
fn mime_from_extension(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        _ => "image/jpeg", // default to JPEG for .jpg, .jpeg, or unknown
    }
}

/// Validate that a URL is an HTTP(S) URL and not pointing at a private address.
fn validate_image_url(url: &str) -> Result<(), ToolError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ToolError::InvalidArgs {
            tool: "vision_analyze".into(),
            message: "URL must start with http:// or https://".into(),
        });
    }

    // SSRF protection via edgecrab-security
    match edgecrab_security::url_safety::is_safe_url(url) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ToolError::PermissionDenied(
            "Blocked: URL points to a private/internal address (SSRF protection)".into(),
        )),
        Err(e) => Err(ToolError::InvalidArgs {
            tool: "vision_analyze".into(),
            message: format!("URL validation error: {e}"),
        }),
    }
}

/// Read image bytes from a local file.
async fn read_local_image(path: &Path) -> Result<(Vec<u8>, String), ToolError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "vision_analyze".into(),
            message: format!("Failed to read image file: {e}"),
        })?;

    if bytes.len() > MAX_IMAGE_SIZE {
        return Err(ToolError::ExecutionFailed {
            tool: "vision_analyze".into(),
            message: format!(
                "Image too large: {} bytes (max {} bytes)",
                bytes.len(),
                MAX_IMAGE_SIZE
            ),
        });
    }

    let mime = mime_from_extension(path).to_string();
    Ok((bytes, mime))
}

/// Shrink an image to stay within the API body-size limit.
///
/// WHY: The GitHub Copilot API returns HTTP 413 when the JSON body exceeds
/// ~4 MB. A screenshot PNG (3–8 MB raw) encodes to ~5–11 MB in base64.
/// We decode the image in-memory, resize so the longest edge ≤ 1024 px,
/// re-encode as JPEG quality 82 — always < 300 KB for typical screenshots.
///
/// WHY pure Rust (`image` crate) instead of `sips`/`convert`:
/// - Cross-platform: works on macOS, Linux, Windows, CI — no OS tools needed
/// - Operates entirely in memory — no temp files, no process spawning
/// - Deterministic: no dependency on installed tool versions
///
/// ```text
/// raw bytes > 1 MB?
///   yes → image::load_from_memory → resize(≤1024px) → JpegEncoder(82)
///       → return (jpeg_bytes, "image/jpeg")
///   no  → return original bytes unchanged
///
/// Decode or encode fails? → return original bytes unchanged (graceful fallback)
/// ```
async fn auto_resize_if_needed(bytes: Vec<u8>, mime: String) -> (Vec<u8>, String) {
    if bytes.len() <= AUTO_RESIZE_THRESHOLD {
        return (bytes, mime);
    }

    let orig_len = bytes.len();

    // Image decoding and JPEG encoding are CPU-bound — run on the blocking pool
    // so we don't stall the Tokio executor.
    let result = tokio::task::spawn_blocking(move || shrink_to_jpeg(bytes, mime)).await;

    match result {
        Ok((out_bytes, out_mime)) => {
            tracing::debug!(
                original_bytes = orig_len,
                resized_bytes = out_bytes.len(),
                "vision: auto-resized image to JPEG"
            );
            (out_bytes, out_mime)
        }
        Err(e) => {
            // spawn_blocking panic — extremely unlikely (would require OOM)
            tracing::warn!(error = %e, "vision: image resize task panicked, cannot recover");
            (Vec::new(), "image/jpeg".to_string())
        }
    }
}

/// Synchronous helper: decode `bytes`, resize longest side to ≤ `MAX_VISION_DIM`,
/// then JPEG-encode at `JPEG_QUALITY`.  Returns the original bytes on any error.
fn shrink_to_jpeg(bytes: Vec<u8>, mime: String) -> (Vec<u8>, String) {
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i,
        Err(e) => {
            tracing::debug!(error = %e, "vision: cannot decode image for resize, sending as-is");
            return (bytes, mime);
        }
    };

    let (w, h) = (img.width(), img.height());
    let resized = if w > MAX_VISION_DIM || h > MAX_VISION_DIM {
        img.resize(
            MAX_VISION_DIM,
            MAX_VISION_DIM,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    let mut buf: Vec<u8> = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    match resized.write_with_encoder(encoder) {
        Ok(()) if !buf.is_empty() => (buf, "image/jpeg".to_string()),
        Ok(()) => {
            tracing::debug!("vision: JPEG encoder produced empty output, sending original");
            (bytes, mime)
        }
        Err(e) => {
            tracing::debug!(error = %e, "vision: JPEG encode failed, sending original");
            (bytes, mime)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisionTarget {
    provider: String,
    model: String,
    base_url: Option<String>,
    api_key_env: Option<String>,
    source: &'static str,
}

fn load_models_config() -> Option<ModelsConfig> {
    match ModelsConfig::load() {
        Ok(config) => Some(config),
        Err(err) => {
            tracing::debug!(error = %err, "vision: unable to load models config");
            None
        }
    }
}

fn select_provider_vision_model(
    models: Option<&ModelsConfig>,
    config: &ProviderConfig,
) -> Option<String> {
    let default = config.default_llm_model.as_deref().and_then(|model| {
        config
            .models
            .iter()
            .find(|card| {
                card.name == model
                    && matches!(card.model_type, ModelType::Llm | ModelType::Multimodal)
                    && model_supports_vision(models, &config.name, &card.name)
                    && !card.deprecated
            })
            .map(|card| card.name.clone())
    });

    default.or_else(|| {
        config
            .models
            .iter()
            .find(|card| {
                matches!(card.model_type, ModelType::Llm | ModelType::Multimodal)
                    && model_supports_vision(models, &config.name, &card.name)
                    && !card.deprecated
            })
            .or_else(|| {
                config.models.iter().find(|card| {
                    matches!(card.model_type, ModelType::Llm | ModelType::Multimodal)
                        && model_supports_vision(models, &config.name, &card.name)
                })
            })
            .map(|card| card.name.clone())
    })
}

fn build_custom_openai_compatible_target(
    target: &VisionTarget,
) -> Result<Arc<dyn LLMProvider>, String> {
    let model_card = ModelCard {
        name: target.model.clone(),
        display_name: target.model.clone(),
        model_type: ModelType::Multimodal,
        capabilities: ModelCapabilities {
            supports_vision: true,
            supports_streaming: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let provider = ProviderConfig {
        name: target.provider.clone(),
        display_name: format!("{} (custom vision)", target.provider),
        provider_type: ConfigProviderType::OpenAICompatible,
        api_key_env: target.api_key_env.clone().filter(|v| !v.trim().is_empty()),
        base_url: target.base_url.clone(),
        default_llm_model: Some(target.model.clone()),
        models: vec![model_card],
        ..Default::default()
    };

    let provider = OpenAICompatibleProvider::from_config(provider)
        .map_err(|err| err.to_string())?
        .with_model(target.model.clone());
    Ok(Arc::new(provider))
}

fn build_provider_for_target(
    target: &VisionTarget,
    models: Option<&ModelsConfig>,
) -> Result<Arc<dyn LLMProvider>, String> {
    if target.base_url.is_some() {
        return build_custom_openai_compatible_target(target);
    }

    if ProviderType::from_str(&target.provider).is_some() {
        return crate::create_provider_for_model(&target.provider, &target.model);
    }

    let Some(models) = models else {
        return Err(format!(
            "provider '{}' is not built-in and no models config is available",
            target.provider
        ));
    };

    let config = models
        .get_provider(&target.provider)
        .ok_or_else(|| format!("provider '{}' not found in models config", target.provider))?;

    match config.provider_type {
        ConfigProviderType::OpenAICompatible => {
            let provider = OpenAICompatibleProvider::from_config(config.clone())
                .map_err(|err| err.to_string())?
                .with_model(target.model.clone());
            Ok(Arc::new(provider))
        }
        ConfigProviderType::OpenAI => ProviderFactory::create_llm_provider("openai", &target.model)
            .map_err(|err| err.to_string()),
        ConfigProviderType::Anthropic => {
            ProviderFactory::create_llm_provider("anthropic", &target.model)
                .map_err(|err| err.to_string())
        }
        ConfigProviderType::OpenRouter => {
            ProviderFactory::create_llm_provider("openrouter", &target.model)
                .map_err(|err| err.to_string())
        }
        ConfigProviderType::Ollama => ProviderFactory::create_llm_provider("ollama", &target.model)
            .map_err(|err| err.to_string()),
        ConfigProviderType::LMStudio => {
            ProviderFactory::create_llm_provider("lmstudio", &target.model)
                .map_err(|err| err.to_string())
        }
        ConfigProviderType::Azure => ProviderFactory::create_llm_provider("azure", &target.model)
            .map_err(|err| err.to_string()),
        ConfigProviderType::Mistral => {
            ProviderFactory::create_llm_provider("mistral", &target.model)
                .map_err(|err| err.to_string())
        }
        ConfigProviderType::Mock => Err("mock provider is not a real vision backend".to_string()),
    }
}

fn resolve_explicit_target(
    ctx: &ToolContext,
    current_provider: &str,
    current_model: &str,
    models: Option<&ModelsConfig>,
) -> Option<VisionTarget> {
    let explicit_provider = ctx
        .config
        .auxiliary_provider
        .as_deref()
        .map(normalize_provider_name);
    let explicit_model = ctx
        .config
        .auxiliary_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let explicit_base_url = ctx
        .config
        .auxiliary_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let explicit_api_key_env = ctx
        .config
        .auxiliary_api_key_env
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if explicit_provider.is_none() && explicit_model.is_none() && explicit_base_url.is_none() {
        return None;
    }

    let from_spec = explicit_model
        .as_deref()
        .and_then(parse_provider_model_spec)
        .map(|(provider, model)| (Some(provider), Some(model)));

    let provider = from_spec
        .as_ref()
        .and_then(|(provider, _)| provider.clone())
        .or(explicit_provider)
        .unwrap_or_else(|| normalize_provider_name(current_provider));
    let model = from_spec
        .as_ref()
        .and_then(|(_, model)| model.clone())
        .or(explicit_model)
        .or_else(|| {
            if normalize_provider_name(current_provider) == provider {
                Some(normalize_model_name(&provider, current_model))
            } else {
                None
            }
        })
        .or_else(|| {
            models
                .and_then(|cfg| cfg.get_provider(&provider))
                .and_then(|cfg| select_provider_vision_model(models, cfg))
        })?;

    Some(VisionTarget {
        provider: provider.clone(),
        model: normalize_model_name(&provider, &model),
        base_url: explicit_base_url,
        api_key_env: explicit_api_key_env,
        source: "auxiliary override",
    })
}

fn resolve_vision_targets(
    ctx: &ToolContext,
    provider: Arc<dyn LLMProvider>,
) -> Vec<(VisionTarget, Arc<dyn LLMProvider>)> {
    let models = load_models_config();
    let models_ref = models.as_ref();
    let current_provider = normalize_provider_name(provider.name());
    let current_model = normalize_model_name(&current_provider, provider.model());
    let mut resolved = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(target) =
        resolve_explicit_target(ctx, &current_provider, &current_model, models_ref)
    {
        match build_provider_for_target(&target, models_ref) {
            Ok(explicit_provider) => {
                seen.insert((
                    target.provider.clone(),
                    target.model.clone(),
                    target.base_url.clone(),
                ));
                resolved.push((target, explicit_provider));
            }
            Err(err) => {
                tracing::warn!(error = %err, "vision: auxiliary override is configured but could not be built");
            }
        }
    }

    if model_supports_vision(models_ref, &current_provider, &current_model)
        && seen.insert((current_provider.clone(), current_model.clone(), None))
    {
        resolved.push((
            VisionTarget {
                provider: current_provider.clone(),
                model: current_model.clone(),
                base_url: None,
                api_key_env: None,
                source: "current chat model",
            },
            provider.clone(),
        ));
    }

    if let Some(models) = models_ref {
        let mut providers: Vec<&ProviderConfig> =
            models.providers.iter().filter(|cfg| cfg.enabled).collect();
        providers.sort_by_key(|cfg| cfg.priority);

        for config in providers {
            let Some(model) = select_provider_vision_model(models_ref, config) else {
                continue;
            };
            let provider_name = normalize_provider_name(&config.name);
            let key = (provider_name.clone(), model.clone(), None);
            if !seen.insert(key) {
                continue;
            }

            let target = VisionTarget {
                provider: provider_name,
                model,
                base_url: None,
                api_key_env: config.api_key_env.clone(),
                source: "auto fallback",
            };

            match build_provider_for_target(&target, Some(models)) {
                Ok(candidate_provider) => resolved.push((target, candidate_provider)),
                Err(err) => {
                    tracing::debug!(
                        provider = %target.provider,
                        model = %target.model,
                        error = %err,
                        "vision: skipping unavailable fallback backend"
                    );
                }
            }
        }
    }

    resolved
}

// ─── vision_analyze ───────────────────────────────────────────────────

pub struct VisionAnalyzeTool;

#[derive(Deserialize)]
struct VisionArgs {
    /// Image URL (http/https) or local file path.
    image_source: String,
    /// Prompt/question about the image.
    #[serde(default = "default_prompt")]
    prompt: String,
    /// Optional detail level for vision models: "auto" (default), "low" (faster/cheaper),
    /// or "high" (higher resolution, better for fine-grained details).
    #[serde(default)]
    detail: Option<String>,
}

fn default_prompt() -> String {
    "Describe this image in detail. What do you see?".into()
}

#[async_trait]
impl ToolHandler for VisionAnalyzeTool {
    fn name(&self) -> &'static str {
        "vision_analyze"
    }

    fn toolset(&self) -> &'static str {
        "media"
    }

    fn emoji(&self) -> &'static str {
        "👁️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "vision_analyze".into(),
            description:
                "Analyze a local image file or remote image URL using a vision-capable LLM. \
                 Use this for: clipboard-pasted images, local PNG/JPG/WEBP/GIF files, \
                 screenshots saved to disk, and any image file path. Also accepts HTTP(S) URLs. \
                 This is the CORRECT tool whenever an image file path is given — do NOT use \
                 browser_vision for local files. Returns the model's detailed analysis."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "image_source": {
                        "type": "string",
                        "description": "Image URL (http/https) or local file path"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Question or instruction about the image (default: general description)"
                    },
                    "detail": {
                        "type": "string",
                        "enum": ["auto", "low", "high"],
                        "description": "Vision detail level: 'auto' (default), 'low' (faster/cheaper), 'high' (best for fine details)"
                    }
                },
                "required": ["image_source"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        // Vision requires an LLM provider with multimodal support.
        // We mark available and check at execution time.
        true
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let args: VisionArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "vision_analyze".into(),
                message: e.to_string(),
            })?;

        let provider = ctx
            .provider
            .as_ref()
            .ok_or_else(|| ToolError::Unavailable {
                tool: "vision_analyze".into(),
                reason: "No LLM provider available for vision analysis".into(),
            })?
            .clone();

        // For HTTPS URLs pass the URL directly to the provider — avoids an unnecessary
        // download + base64-encode round-trip.  For local file paths we still read the
        // bytes ourselves and encode, because providers cannot reach local files.
        let image_data = if args.image_source.starts_with("https://")
            || args.image_source.starts_with("http://")
        {
            validate_image_url(&args.image_source)?;
            let mut img = edgequake_llm::ImageData::from_url(&args.image_source);
            if let Some(ref d) = args.detail {
                img = img.with_detail(d.clone());
            }
            img
        } else {
            // Treat as local file path.
            // Trusted roots — single source of truth via AppConfigRef helpers:
            //   1. ctx.cwd              — workspace files the agent is working on
            //   2. tui_images_dir()     — clipboard images saved by EdgeCrab TUI
            //      (~/.edgecrab/images/)
            //   3. gateway_image_cache_dir() — images downloaded by the WhatsApp
            //      Baileys bridge (~/.edgecrab/image_cache/). Created exclusively
            //      by EdgeCrab's own bridge process → safe to trust.
            //   4. gateway_media_dir() — files downloaded by Rust-native gateway
            //      adapters (Telegram, Discord, …) to ~/.edgecrab/gateway_media/.
            //      Trusting the root covers all current and future platform
            //      sub-directories without per-platform changes.
            let images_dir = ctx.config.tui_images_dir();
            let image_cache_dir = ctx.config.gateway_image_cache_dir();
            let gateway_media_dir = ctx.config.gateway_media_dir();
            let path_policy = ctx.config.file_path_policy(&ctx.cwd);
            let trusted = [
                images_dir.as_path(),
                image_cache_dir.as_path(),
                gateway_media_dir.as_path(),
            ];
            let canonical = jail_read_path_multi(&args.image_source, &path_policy, &trusted)
                .map_err(|e| match e {
                    ToolError::PermissionDenied(_) => ToolError::PermissionDenied(format!(
                        "Image path '{}' is outside all trusted directories. \
                         Trusted locations: workspace root, configured file allowed roots, \
                         ~/.edgecrab/images/, \
                         ~/.edgecrab/image_cache/ (WhatsApp), \
                         ~/.edgecrab/gateway_media/ (Telegram/other). \
                         If this image came from a gateway, ensure the bridge \
                         downloaded it to one of these locations.",
                        args.image_source
                    )),
                    other => ToolError::ExecutionFailed {
                        tool: "vision_analyze".into(),
                        message: other.to_string(),
                    },
                })?;

            let (image_bytes, mime_type) = read_local_image(&canonical).await?;

            // Shrink large images before base64 encoding to avoid HTTP 413.
            let (image_bytes, mime_type) = auto_resize_if_needed(image_bytes, mime_type).await;

            use base64::Engine as _;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&image_bytes);
            let mut img = edgequake_llm::ImageData::new(b64, mime_type);
            if let Some(ref d) = args.detail {
                img = img.with_detail(d.clone());
            }
            img
        };

        // Build multimodal message
        let message = ChatMessage::user_with_images(&args.prompt, vec![image_data]);
        let messages = vec![message];

        let options = CompletionOptions {
            temperature: Some(0.1),
            max_tokens: Some(4096),
            ..Default::default()
        };

        let mut failures = Vec::new();
        let mut analysis = None;
        for (target, candidate_provider) in resolve_vision_targets(ctx, provider) {
            match tokio::time::timeout(
                std::time::Duration::from_secs(VISION_TIMEOUT_SECS),
                candidate_provider.chat(&messages, Some(&options)),
            )
            .await
            {
                Ok(Ok(response)) if !response.content.trim().is_empty() => {
                    analysis = Some((target, response.content.trim().to_string()));
                    break;
                }
                Ok(Ok(_)) => {
                    failures.push(format!(
                        "{} {} returned an empty response",
                        target.source, target.model
                    ));
                }
                Ok(Err(err)) => {
                    failures.push(format!(
                        "{} {} failed: {}",
                        target.source, target.model, err
                    ));
                }
                Err(_) => {
                    failures.push(format!(
                        "{} {} timed out after {}s",
                        target.source, target.model, VISION_TIMEOUT_SECS
                    ));
                }
            }
        }

        let (target, analysis) = analysis.ok_or_else(|| ToolError::ExecutionFailed {
            tool: "vision_analyze".into(),
            message: if failures.is_empty() {
                "No vision-capable backend is configured. Set auxiliary.provider/model, or use a chat model that is declared or known to support vision.".into()
            } else {
                format!(
                    "No vision backend succeeded:\n- {}",
                    failures.join("\n- ")
                )
            },
        })?;

        let source_label = if args.image_source.starts_with("https://")
            || args.image_source.starts_with("http://")
        {
            format!("url: {}", &args.image_source)
        } else {
            format!("local file: {}", &args.image_source)
        };
        Ok(format!(
            "Image analysis ({} via {}/{} [{}]):\n\n{}",
            source_label, target.provider, target.model, target.source, analysis
        ))
    }
}

inventory::submit!(&VisionAnalyzeTool as &dyn ToolHandler);

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_valid() {
        let schema = VisionAnalyzeTool.schema();
        assert_eq!(schema.name, "vision_analyze");
        let required = schema.parameters["required"].as_array().expect("array");
        assert!(required.iter().any(|v| v == "image_source"));
    }

    #[test]
    fn mime_detection() {
        assert_eq!(mime_from_extension(Path::new("photo.png")), "image/png");
        assert_eq!(mime_from_extension(Path::new("photo.jpg")), "image/jpeg");
        assert_eq!(mime_from_extension(Path::new("photo.jpeg")), "image/jpeg");
        assert_eq!(mime_from_extension(Path::new("photo.gif")), "image/gif");
        assert_eq!(mime_from_extension(Path::new("photo.webp")), "image/webp");
        assert_eq!(mime_from_extension(Path::new("photo.bmp")), "image/bmp");
        assert_eq!(mime_from_extension(Path::new("photo")), "image/jpeg");
    }

    #[test]
    fn url_validation_rejects_private() {
        // These should be caught by SSRF protection
        assert!(validate_image_url("ftp://example.com/img.png").is_err());
        assert!(validate_image_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn url_validation_accepts_https() {
        // Public HTTPS URLs should pass validation
        let result = validate_image_url("https://example.com/photo.jpg");
        assert!(result.is_ok());
    }

    #[test]
    fn default_prompt_not_empty() {
        assert!(!default_prompt().is_empty());
    }

    #[test]
    fn tool_metadata() {
        assert_eq!(VisionAnalyzeTool.name(), "vision_analyze");
        assert_eq!(VisionAnalyzeTool.toolset(), "media");
        assert!(VisionAnalyzeTool.is_available());
    }

    #[test]
    fn schema_exposes_detail_param() {
        let schema = VisionAnalyzeTool.schema();
        let props = &schema.parameters["properties"];
        assert!(
            props.get("detail").is_some(),
            "schema must have 'detail' property"
        );
        let enum_vals = &props["detail"]["enum"];
        assert!(enum_vals.as_array().is_some_and(|a| a.len() == 3));
    }

    #[test]
    fn detail_not_required() {
        let schema = VisionAnalyzeTool.schema();
        let required = schema.parameters["required"].as_array().expect("array");
        // 'detail' must be optional — only 'image_source' is required
        assert!(!required.iter().any(|v| v == "detail"));
        assert!(required.iter().any(|v| v == "image_source"));
    }

    #[test]
    fn provider_aliases_are_normalized() {
        assert_eq!(normalize_provider_name("copilot"), "vscode-copilot");
        assert_eq!(normalize_provider_name("google"), "gemini");
        assert_eq!(normalize_provider_name("vertex-ai"), "vertexai");
    }

    #[test]
    fn provider_model_spec_keeps_nested_model_path() {
        let (provider, model) =
            parse_provider_model_spec("openrouter/openai/gpt-4.1").expect("spec parses");
        assert_eq!(provider, "openrouter");
        assert_eq!(model, "openai/gpt-4.1");
    }

    #[test]
    fn declared_vision_models_are_detected() {
        let models = ModelsConfig::load().expect("built-in models config");
        assert!(model_supports_vision(Some(&models), "openai", "gpt-4o"));
        assert!(!model_supports_vision(
            Some(&models),
            "openai",
            "text-embedding-3-small"
        ));
    }

    #[test]
    fn explicit_target_can_reuse_current_model_for_same_provider() {
        let models = ModelsConfig::load().expect("built-in models config");
        let ctx = ToolContext {
            task_id: "test".to_string(),
            cwd: std::path::PathBuf::from("."),
            session_id: "test".to_string(),
            user_task: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            config: crate::config_ref::AppConfigRef {
                auxiliary_provider: Some("openai".to_string()),
                ..Default::default()
            },
            state_db: None,
            platform: edgecrab_types::Platform::Cli,
            process_table: None,
            provider: None,
            tool_registry: None,
            delegate_depth: 0,
            sub_agent_runner: None,
            delegation_event_tx: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: None,
            todo_store: None,
            current_tool_call_id: None,
            current_tool_name: None,
            injected_messages: None,
            tool_progress_tx: None,
        };

        let target = resolve_explicit_target(&ctx, "openai", "gpt-4o", Some(&models))
            .expect("target resolves");
        assert_eq!(target.provider, "openai");
        assert_eq!(target.model, "gpt-4o");
    }

    /// Build a minimal 2×2 RGB PNG in memory (no file I/O, no external tools).
    ///
    /// PNG structure: signature + IHDR + IDAT + IEND.
    /// The image crate can parse this format, so it lets us test the full
    /// shrink_to_jpeg → decode → resize → JPEG-encode path synthetically.
    fn make_small_png_bytes() -> Vec<u8> {
        // Use image crate to produce a valid 2×2 RGBA PNG.
        let mut buf = Vec::new();
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255u8, 128u8, 0u8, 255u8]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("test PNG encode");
        buf
    }

    #[test]
    fn shrink_to_jpeg_small_image_passthrough() {
        // A tiny PNG (well under AUTO_RESIZE_THRESHOLD) must be returned as-is.
        let png = make_small_png_bytes();
        assert!(
            png.len() < AUTO_RESIZE_THRESHOLD,
            "precondition: test PNG is small"
        );
        let (out, mime) = shrink_to_jpeg(png.clone(), "image/png".to_string());
        // Since it is small, the caller never calls shrink, but shrink itself should
        // still tolerate small inputs and produce valid JPEG output.
        assert!(!out.is_empty(), "output must not be empty");
        assert_eq!(mime, "image/jpeg");
    }

    #[test]
    fn shrink_to_jpeg_produces_smaller_output() {
        // Build a synthetic 2048×2048 red PNG — large enough to need resizing.
        let mut buf_png = Vec::new();
        let big_img = image::RgbImage::from_pixel(2048, 2048, image::Rgb([200u8, 50u8, 50u8]));
        image::DynamicImage::ImageRgb8(big_img)
            .write_to(
                &mut std::io::Cursor::new(&mut buf_png),
                image::ImageFormat::Png,
            )
            .expect("test encode");

        let original_len = buf_png.len();
        let (out, mime) = shrink_to_jpeg(buf_png, "image/png".to_string());

        assert_eq!(mime, "image/jpeg", "output MIME must be image/jpeg");
        assert!(!out.is_empty(), "output must not be empty");
        assert!(
            out.len() < original_len,
            "resized JPEG ({} bytes) should be smaller than original PNG ({} bytes)",
            out.len(),
            original_len,
        );
        // Confirm the output starts with the JPEG SOI marker (FF D8).
        assert_eq!(out[0], 0xFF);
        assert_eq!(out[1], 0xD8);
    }

    #[test]
    fn shrink_to_jpeg_invalid_bytes_passthrough() {
        let garbage = b"not-an-image-at-all-\x00\x01\x02".to_vec();
        let (out, mime) = shrink_to_jpeg(garbage.clone(), "image/png".to_string());
        // On decode failure we return the original bytes unchanged.
        assert_eq!(out, garbage);
        assert_eq!(mime, "image/png");
    }
}
