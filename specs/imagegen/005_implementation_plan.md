# Image Generation — Rust Implementation Plan

**Spec ID:** IMAGEGEN-005  
**Status:** Draft  
**Date:** 2026-04-04  

---

## 1. File Layout

```
  edgequake-llm/src/
  └── imagegen/                          NEW module
      ├── mod.rs                         pub use re-exports
      ├── types.rs                       Core types (all providers share)
      ├── error.rs                       ImageGenError enum
      ├── trait.rs                       ImageGenProvider async trait
      ├── factory.rs                     ImageGenFactory::from_env()
      └── providers/
          ├── mod.rs
          ├── vertexai.rs                VertexAIImageGen (Imagen)
          ├── fal.rs                     FalImageGen (FLUX)
          ├── nano_banana.rs             NanoBananaImageGen (Gemini via FAL)
          ├── gemini.rs                  GeminiImageGenProvider (native API)
          └── mock.rs                    MockImageGenProvider

  edgecrab-tools/src/tools/
  └── imagegen.rs                        NEW tool: generate_image
```

---

## 2. Complete Type Definitions

### 2.1 `imagegen/types.rs`

```rust
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// ─── Aspect Ratio ────────────────────────────────────────────────────

/// Named aspect ratio presets shared across providers.
///
/// WHY named presets: Each provider specifies aspect ratio differently
/// (Vertex uses "16:9", FAL uses "landscape_16_9", DALL-E uses "1792x1024").
/// Canonical enum enables provider-level translation without exposing
/// provider strings to callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AspectRatio {
    #[default]
    Auto,          // "auto"  — model decides (Gemini / Nano Banana only)
    Square,        // "1:1"   — 1024x1024
    SquareHd,      // "1:1"   — 2048x2048 or maximum supported
    Landscape43,   // "4:3"   — 1024x768
    Landscape169,  // "16:9"  — 1280x720
    Ultrawide,     // "21:9"  — 2560x1080
    Portrait43,    // "3:4"   — 768x1024
    Portrait169,   // "9:16"  — 720x1280
    Frame54,       // "5:4"   — 1280x1024
    Frame45,       // "4:5"   — 1024x1280
    Print32,       // "3:2"   — 1152x768
    Print23,       // "2:3"   — 768x1152
    // Gemini 3.1 Flash Image only (Nano Banana 2)
    Extreme41,     // "4:1"   — ultra-wide banner
    Extreme14,     // "1:4"   — ultra-tall banner
    Extreme81,     // "8:1"   — panoramic strip
    Extreme18,     // "1:8"   — vertical strip
}

impl AspectRatio {
    /// Resolved default pixel dimensions for this ratio.
    pub fn default_dimensions(&self) -> (u32, u32) {
        match self {
            Self::Auto        => (1024, 1024),  // fallback
            Self::Square      => (1024, 1024),
            Self::SquareHd    => (2048, 2048),
            Self::Landscape43 => (1024, 768),
            Self::Landscape169 => (1280, 720),
            Self::Ultrawide   => (2560, 1080),
            Self::Portrait43  => (768, 1024),
            Self::Portrait169 => (720, 1280),
            Self::Frame54     => (1280, 1024),
            Self::Frame45     => (1024, 1280),
            Self::Print32     => (1152, 768),
            Self::Print23     => (768, 1152),
            Self::Extreme41   => (2048, 512),
            Self::Extreme14   => (512, 2048),
            Self::Extreme81   => (3072, 384),
            Self::Extreme18   => (384, 3072),
        }
    }

    /// Vertex AI aspect ratio string representation.
    pub fn as_vertex_str(&self) -> &'static str {
        match self {
            Self::Auto         => "1:1",  // best effort fallback
            Self::Square       => "1:1",
            Self::SquareHd     => "1:1",
            Self::Landscape43  => "4:3",
            Self::Landscape169 => "16:9",
            Self::Ultrawide    => "16:9", // not supported; approximate
            Self::Portrait43   => "3:4",
            Self::Portrait169  => "9:16",
            Self::Frame54      => "5:4",
            Self::Frame45      => "4:5",
            Self::Print32      => "3:2",
            Self::Print23      => "2:3",
            Self::Extreme41    => "4:1", // Vertex Imagen doesn't support; callers must validate
            Self::Extreme14    => "1:4",
            Self::Extreme81    => "8:1",
            Self::Extreme18    => "1:8",
        }
    }

    /// FAL image_size string for FLUX models.
    pub fn as_fal_str(&self) -> &'static str {
        match self {
            Self::Auto         => "square",     // FLUX has no auto
            Self::Square       => "square",
            Self::SquareHd     => "square_hd",
            Self::Landscape43  => "landscape_4_3",
            Self::Landscape169 => "landscape_16_9",
            Self::Ultrawide    => "landscape_16_9", // approximation
            Self::Portrait43   => "portrait_4_3",
            Self::Portrait169  => "portrait_16_9",
            Self::Frame54      => "landscape_4_3", // approximation
            Self::Frame45      => "portrait_4_3",  // approximation
            Self::Print32      => "landscape_4_3", // approximation
            Self::Print23      => "portrait_4_3",  // approximation
            Self::Extreme41    => "landscape_16_9", // not supported
            Self::Extreme14    => "portrait_16_9",
            Self::Extreme81    => "landscape_16_9",
            Self::Extreme18    => "portrait_16_9",
        }
    }

    /// Gemini API (and Nano Banana / FAL) aspect ratio string.
    /// Returns Option because Auto and standard ratios differ between
    /// model generations.
    pub fn as_gemini_str(&self) -> &'static str {
        match self {
            Self::Auto         => "auto",   // pass as None in imageConfig to let model decide
            Self::Square       => "1:1",
            Self::SquareHd     => "1:1",
            Self::Landscape43  => "4:3",
            Self::Landscape169 => "16:9",
            Self::Ultrawide    => "21:9",
            Self::Portrait43   => "3:4",
            Self::Portrait169  => "9:16",
            Self::Frame54      => "5:4",
            Self::Frame45      => "4:5",
            Self::Print32      => "3:2",
            Self::Print23      => "2:3",
            Self::Extreme41    => "4:1",
            Self::Extreme14    => "1:4",
            Self::Extreme81    => "8:1",
            Self::Extreme18    => "1:8",
        }
    }
}

// ─── Output Format ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    #[default]
    Jpeg,
    Png,
    Webp,  // Supported by Gemini / Nano Banana; ignored by Vertex AI Imagen
}

impl ImageFormat {
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png  => "image/png",
            Self::Webp => "image/webp",
        }
    }
    pub fn extension(&self) -> &'static str {
        match self { Self::Jpeg => "jpg", Self::Png => "png", Self::Webp => "webp" }
    }
}

// ─── Safety Level ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyLevel {
    BlockNone,
    BlockLow,
    #[default]
    BlockMedium,
    BlockHigh,
}

// ─── Request ─────────────────────────────────────────────────────────

/// Options shared across all image generation providers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageGenOptions {
    /// Number of images to generate (default: 1).
    pub count: Option<u8>,

    /// Aspect ratio preset. Takes precedence over width/height.
    pub aspect_ratio: Option<AspectRatio>,

    /// Explicit width in pixels. Ignored if aspect_ratio is set.
    pub width: Option<u32>,

    /// Explicit height in pixels. Ignored if aspect_ratio is set.
    pub height: Option<u32>,

    /// Random seed for deterministic generation.
    pub seed: Option<u64>,

    /// Negative prompt (Vertex AI Imagen only — ignored by Gemini/FLUX).
    pub negative_prompt: Option<String>,

    /// Guidance / CFG scale (Vertex AI Imagen and FLUX only).
    pub guidance_scale: Option<f32>,

    /// Output image format.
    pub output_format: Option<ImageFormat>,

    /// Safety filter level.
    pub safety_level: Option<SafetyLevel>,

    /// Request prompt enhancement (Vertex AI Imagen only).
    pub enhance_prompt: Option<bool>,

    // ── Gemini / Nano Banana specific ──────────────────────────────

    /// Output resolution for Gemini image models (image_size).
    /// "512" | "1K" | "2K" | "4K". Ignored by Vertex Imagen and FLUX.
    pub resolution: Option<ImageResolution>,

    /// Enable Google Search grounding for image generation.
    /// Only supported by Gemini image models (NB 2 / NB Pro).
    pub enable_web_search: Option<bool>,

    /// Reasoning depth for Gemini image models.
    /// Minimal = fastest, High = better quality at higher cost.
    /// Only Nano Banana 2 supports control; NB Pro always thinks.
    pub thinking_level: Option<ThinkingLevel>,

    /// Reference image URLs or base64 data URIs (up to 14).
    /// When non-empty + Gemini/NanoBanana provider → uses edit endpoint.
    /// Ignored by Vertex AI Imagen and FLUX providers.
    pub reference_images: Vec<String>,

    // ── Escape hatch ────────────────────────────────────────────────

    /// Escape hatch for provider-specific parameters.
    /// Example: extra["steps"] = 28 (FAL inference steps)
    ///          extra["style"] = "photograph" (Vertex sampleImageStyle)
    #[serde(default)]
    pub extra: HashMap<String, JsonValue>,
}

// ─── Image Resolution (Gemini) ───────────────────────────────────────

/// Output resolution for Gemini image models.
///
/// Maps to `imageSize` in Gemini API `imageConfig`:
/// `"512"` (0.5K), `"1K"`, `"2K"`, `"4K"`.
///
/// Billing multipliers (Nano Banana 2): 0.5K=×0.75, 1K=×1, 2K=×1.5, 4K=×2
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageResolution {
    Half,    // "512"  — 0.5K; Nano Banana 2 only
    #[default]
    OneK,    // "1K"   — default
    TwoK,    // "2K"
    FourK,   // "4K"
}

impl ImageResolution {
    /// Gemini API `imageSize` string.
    pub fn as_gemini_str(&self) -> &'static str {
        match self {
            Self::Half  => "512",
            Self::OneK  => "1K",
            Self::TwoK  => "2K",
            Self::FourK => "4K",
        }
    }

    /// FAL Nano Banana `resolution` string.
    pub fn as_nano_banana_str(&self) -> &'static str {
        match self {
            Self::Half  => "0.5K",
            Self::OneK  => "1K",
            Self::TwoK  => "2K",
            Self::FourK => "4K",
        }
    }
}

// ─── Thinking Level (Gemini) ─────────────────────────────────────────

/// Reasoning depth for Gemini image models.
///
/// Maps to `thinkingConfig.thinkingLevel` in Gemini API.
/// Ignored silently by Vertex AI Imagen and FLUX providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel {
    Minimal, // lowest latency; model still thinks minimally
    High,    // deeper reasoning before generation; better quality
}

impl ThinkingLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::High    => "High",  // Gemini API uses capitalised "High"
        }
    }
}

/// A complete image generation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenRequest {
    /// Descriptive text prompt.
    pub prompt: String,

    /// Model to use. None means provider default.
    pub model: Option<String>,

    /// Generation options.
    #[serde(default)]
    pub options: ImageGenOptions,
}

impl ImageGenRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            options: ImageGenOptions::default(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_options(mut self, options: ImageGenOptions) -> Self {
        self.options = options;
        self
    }
}

// ─── Response ────────────────────────────────────────────────────────

/// Output data for a single generated image.
#[derive(Debug, Clone)]
pub enum ImageData {
    /// Raw bytes (e.g. Vertex AI base64-decoded response).
    Bytes(Vec<u8>),
    /// CDN URL (e.g. FAL hosted output).
    Url(String),
}

/// Metadata + data for one generated image.
#[derive(Debug, Clone)]
pub struct GeneratedImage {
    pub data: ImageData,
    pub width: u32,
    pub height: u32,
    pub mime_type: String,
    pub seed: Option<u64>,
}

/// Aggregate response from an image generation call.
#[derive(Debug, Clone)]
pub struct ImageGenResponse {
    pub images: Vec<GeneratedImage>,
    pub provider: String,
    pub model: String,
    pub latency_ms: u64,
    /// Enhanced prompt returned by provider (e.g. Vertex AI enhancePrompt).
    pub enhanced_prompt: Option<String>,
}
```

### 2.2 `imagegen/error.rs`

```rust
use thiserror::Error;
use std::time::Duration;

#[derive(Debug, Error)]
pub enum ImageGenError {
    #[error("Image generation provider not configured")]
    NotConfigured,

    #[error("Provider auth error: {0}")]
    AuthError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Content filtered: {reason}")]
    ContentFiltered { reason: String },

    #[error("Rate limited (retry after {retry_after:?})")]
    RateLimited { retry_after: Option<Duration> },

    #[error("Provider error (HTTP {status}): {message}")]
    ProviderError { status: u16, message: String },

    #[error("Request timed out after {elapsed_secs}s")]
    Timeout { elapsed_secs: u64 },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Serialisation error: {0}")]
    Serialisation(#[from] serde_json::Error),

    #[error("Base64 decode error: {0}")]
    Base64(String),
}
```

### 2.3 `imagegen/trait.rs`

```rust
use async_trait::async_trait;
use crate::imagegen::types::{ImageGenRequest, ImageGenResponse};
use crate::imagegen::error::ImageGenError;

#[async_trait]
pub trait ImageGenProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_model(&self) -> &str;

    async fn generate(
        &self,
        request: &ImageGenRequest,
    ) -> Result<ImageGenResponse, ImageGenError>;

    fn available_models(&self) -> Vec<&str> {
        vec![self.default_model()]
    }
}
```

### 2.4 `imagegen/factory.rs`

```rust
use std::sync::Arc;
use crate::imagegen::{ImageGenProvider, ImageGenError};
use crate::imagegen::providers::vertexai::{VertexAIImageGen, VertexAIImageGenConfig};
use crate::imagegen::providers::fal::{FalImageGen, FalImageGenConfig};
use crate::imagegen::providers::nano_banana::{NanoBananaImageGen, NanoBananaConfig};
use crate::imagegen::providers::gemini::{GeminiImageGenProvider, GeminiImageGenConfig};

pub struct ImageGenFactory;

impl ImageGenFactory {
    /// Detect image-gen provider from environment.
    ///
    /// Priority:
    ///   1. EDGECRAB_IMAGEGEN_PROVIDER=gemini       -> GeminiImageGenProvider
    ///   2. EDGECRAB_IMAGEGEN_PROVIDER=nano-banana  -> NanoBananaImageGen (FAL)
    ///   3. EDGECRAB_IMAGEGEN_PROVIDER=fal          -> FalImageGen (FLUX)
    ///   4. EDGECRAB_IMAGEGEN_PROVIDER=vertexai     -> VertexAIImageGen
    ///   5. GEMINI_API_KEY present                  -> GeminiImageGenProvider
    ///   6. FAL_KEY + model starts with nano-banana -> NanoBananaImageGen
    ///   7. FAL_KEY present                         -> FalImageGen (FLUX)
    ///   8. GOOGLE_CLOUD_PROJECT + ADC / token      -> VertexAIImageGen
    ///   9. None                                    -> None (no image-gen)
    pub fn from_env() -> Option<Arc<dyn ImageGenProvider>> {
        if let Ok(pref) = std::env::var("EDGECRAB_IMAGEGEN_PROVIDER") {
            match pref.to_lowercase().as_str() {
                "gemini" => {
                    let cfg = GeminiImageGenConfig::from_env().ok()?;
                    return Some(Arc::new(GeminiImageGenProvider::new(cfg)));
                }
                "nano-banana" | "nanobana" | "nb" => {
                    let cfg = NanoBananaConfig::from_env().ok()?;
                    return Some(Arc::new(NanoBananaImageGen::new(cfg)));
                }
                "fal" => {
                    let cfg = FalImageGenConfig::from_env().ok()?;
                    return Some(Arc::new(FalImageGen::new(cfg)));
                }
                "vertexai" | "vertex" => {
                    let cfg = VertexAIImageGenConfig::from_env().ok()?;
                    return Some(Arc::new(VertexAIImageGen::new(cfg)));
                }
                _ => {}
            }
        }
        // Gemini API key present → native Gemini provider (recommended)
        if std::env::var("GEMINI_API_KEY").is_ok() {
            if let Ok(cfg) = GeminiImageGenConfig::from_env() {
                return Some(Arc::new(GeminiImageGenProvider::new(cfg)));
            }
        }
        // FAL key present → check if nano-banana model requested
        if std::env::var("FAL_KEY").is_ok() {
            let model = std::env::var("EDGECRAB_FAL_MODEL").unwrap_or_default();
            if model.contains("nano-banana") {
                if let Ok(cfg) = NanoBananaConfig::from_env() {
                    return Some(Arc::new(NanoBananaImageGen::new(cfg)));
                }
            } else if let Ok(cfg) = FalImageGenConfig::from_env() {
                return Some(Arc::new(FalImageGen::new(cfg)));
            }
        }
        // Vertex AI
        if std::env::var("GOOGLE_CLOUD_PROJECT").is_ok() {
            if let Ok(cfg) = VertexAIImageGenConfig::from_env() {
                return Some(Arc::new(VertexAIImageGen::new(cfg)));
            }
        }
        None
    }
}
```

---

## 3. `generate_image` Tool (edgecrab-tools)

### 3.1 Tool Skeleton

```rust
// edgecrab-tools/src/tools/imagegen.rs

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use edgequake_llm::imagegen::types::{
    ImageGenRequest, ImageGenOptions, AspectRatio, ImageFormat,
    SafetyLevel, ImageData,
};
use edgecrab_types::{ToolError, ToolSchema};
use crate::registry::{ToolContext, ToolHandler};

pub struct GenerateImageTool;

#[async_trait]
impl ToolHandler for GenerateImageTool {
    fn name(&self) -> &str { "generate_image" }

    fn schema(&self) -> ToolSchema {
        // JSON Schema exposed to the LLM
        ToolSchema {
            name: "generate_image".into(),
            description: "Generate one or more images from a text prompt using \
                          the configured AI image generation provider \
                          (Vertex AI Imagen or FAL/FLUX).".into(),
            parameters: json!({
                "type": "object",
                "required": ["prompt"],
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Text description of the image to generate.",
                        "maxLength": 2000
                    },
                    "model": {
                        "type": "string",
                        "description": "Override the default model for this request."
                    },
                    "count": {
                        "type": "integer",
                        "minimum": 1, "maximum": 4,
                        "description": "Number of images (default 1)."
                    },
                    "aspect_ratio": {
                        "type": "string",
                        "enum": ["1:1","4:3","16:9","3:4","9:16","5:4","3:2"],
                        "description": "Aspect ratio (default 1:1)."
                    },
                    "width":  { "type": "integer" },
                    "height": { "type": "integer" },
                    "seed":   { "type": "integer" },
                    "negative_prompt": { "type": "string" },
                    "guidance_scale":  { "type": "number" },
                    "output_format": {
                        "type": "string", "enum": ["jpeg","png"]
                    },
                    "enhance_prompt": { "type": "boolean" },
                    "safety_level": {
                        "type": "string",
                        "enum": ["block_none","block_low","block_medium","block_high"]
                    }
                }
            }),
        }
    }

    async fn handle(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let image_gen = ctx.image_gen.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed {
                tool: "generate_image".into(),
                message: "No image generation provider configured. \
                         Set FAL_KEY or GOOGLE_CLOUD_PROJECT.".into(),
            }
        })?;

        // Parse args; length validation, SSRF not needed (prompt is text)
        let input: GenerateImageInput = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidArgs {
                tool: "generate_image".into(),
                message: e.to_string(),
            })?;

        // Build request
        let mut opts = ImageGenOptions::default();
        opts.count = input.count;
        opts.aspect_ratio = input.aspect_ratio.and_then(|ar| ar.parse().ok());
        opts.width = input.width;
        opts.height = input.height;
        opts.seed = input.seed;
        opts.negative_prompt = input.negative_prompt;
        opts.guidance_scale = input.guidance_scale;
        opts.output_format = input.output_format;
        opts.enhance_prompt = input.enhance_prompt;
        opts.safety_level = input.safety_level;

        let req = ImageGenRequest {
            prompt: input.prompt,
            model: input.model,
            options: opts,
        };

        let resp = image_gen.generate(&req).await.map_err(|e| {
            ToolError::ExecutionFailed {
                tool: "generate_image".into(),
                message: e.to_string(),
            }
        })?;

        // Save images to cache; build output JSON
        let cache_dir = edgecrab_core::config::gateway_image_cache_dir();
        let mut result_images = Vec::new();

        for img in &resp.images {
            let ext = if img.mime_type == "image/png" { "png" } else { "jpg" };
            let fname = format!("{}.{}", uuid::Uuid::new_v4(), ext);
            let path = cache_dir.join(&fname);

            match &img.data {
                ImageData::Bytes(bytes) => {
                    tokio::fs::write(&path, bytes).await.map_err(|e| {
                        ToolError::ExecutionFailed {
                            tool: "generate_image".into(),
                            message: format!("Failed to save image: {e}"),
                        }
                    })?;
                    result_images.push(json!({
                        "path": path.to_string_lossy(),
                        "width": img.width,
                        "height": img.height,
                        "mime_type": img.mime_type,
                        "seed": img.seed
                    }));
                }
                ImageData::Url(url) => {
                    result_images.push(json!({
                        "url":   url,
                        "width": img.width,
                        "height": img.height,
                        "mime_type": img.mime_type,
                        "seed": img.seed
                    }));
                }
            }
        }

        Ok(json!({
            "provider": resp.provider,
            "model": resp.model,
            "count": resp.images.len(),
            "images": result_images,
            "enhanced_prompt": resp.enhanced_prompt,
            "latency_ms": resp.latency_ms
        }))
    }
}
```

---

## 4. Module Wiring (`imagegen/mod.rs`)

```rust
// edgequake-llm/src/imagegen/mod.rs

pub mod error;
pub mod types;
#[allow(clippy::module_inception)]
pub mod r#trait;
pub mod factory;
pub mod providers;

pub use error::ImageGenError;
pub use types::{
    AspectRatio, GeneratedImage, ImageData, ImageFormat,
    ImageGenOptions, ImageGenRequest, ImageGenResponse, SafetyLevel,
    ImageResolution, ThinkingLevel,
};
pub use r#trait::ImageGenProvider;
pub use factory::ImageGenFactory;
```

```rust
// edgequake-llm/src/lib.rs  (additions)

pub mod imagegen;
pub use imagegen::{
    ImageGenError, ImageGenFactory, ImageGenProvider,
    ImageGenRequest, ImageGenResponse, ImageGenOptions,
    AspectRatio, ImageData, ImageFormat, SafetyLevel,
    ImageResolution, ThinkingLevel,
};
```

---

## 5. Cargo.toml Changes

```toml
# edgequake-llm/Cargo.toml  (additions to [dependencies])
base64 = "0.22"         # already in workspace
tokio-util = "0.7"      # already in workspace
```

No new external dependencies needed. `base64` is already in the workspace
(`edgecrab/Cargo.toml`). `reqwest` is also present.

---

## 6. Implementation Checklist

```
  Phase 1 — Foundation (edgequake-llm)
  [ ] Create src/imagegen/ module skeleton
  [ ] Implement types.rs (all enums + structs incl. ImageResolution, ThinkingLevel)
  [ ] Implement error.rs
  [ ] Implement trait.rs
  [ ] Implement mock.rs (MockImageGenProvider for tests)
  [ ] Add imagegen/mod.rs + re-exports in lib.rs

  Phase 2a — FAL / FLUX provider
  [ ] Implement FalImageGenConfig::from_env()
  [ ] Implement FalImageGen::submit()
  [ ] Implement FalImageGen::poll_until_complete()
  [ ] Implement FalImageGen::parse_result()
  [ ] Implement ImageGenProvider for FalImageGen
  [ ] Unit tests: body serialisation, queue state machine, NSFW filter

  Phase 2b — FAL Nano Banana provider
  [ ] Implement NanoBananaConfig::from_env()
  [ ] Implement NanoBananaImageGen::resolve_endpoint() (T2I vs edit)
  [ ] Implement NanoBananaImageGen::build_body()
  [ ] Implement NanoBananaImageGen::parse_result() (includes description field)
  [ ] Implement ImageGenProvider for NanoBananaImageGen
  [ ] Unit tests: body, edit routing, aspect_ratio auto, safety_tolerance mapping

  Phase 3a — Vertex AI Imagen provider
  [ ] Implement VertexAIImageGenConfig::from_env()
  [ ] Implement AccessTokenCache + refresh logic
  [ ] Implement VertexAIImageGen::build_request_body()
  [ ] Implement VertexAIImageGen::parse_response()
  [ ] Implement ImageGenProvider for VertexAIImageGen
  [ ] Unit tests: request body, base64 decode, error mapping, token refresh

  Phase 3b — Gemini native API provider
  [ ] Implement GeminiImageGenConfig::from_env()
  [ ] Implement GeminiImageGenProvider::build_request() with:
      - inlineData parts for reference_images
      - thinkingConfig when thinking_level set
      - tools:[googleSearch] when enable_web_search
      - imageConfig with aspectRatio + imageSize
  [ ] Implement GeminiImageGenProvider::parse_response()
      - skip thought parts (part.thought == true)
      - decode base64 inline_data
      - collect text parts into enhanced_prompt
  [ ] Implement validate_model_constraints() (extreme ratios, 512 resolution)
  [ ] Implement ImageGenProvider for GeminiImageGenProvider
  [ ] Unit tests: request body fixtures, model constraint validation,
                  thought-skipping, text→enhanced_prompt, auth header modes

  Phase 4 — Factory
  [ ] Implement ImageGenFactory::from_env() (all 4 providers)
  [ ] Add factory tests with env var mocking

  Phase 5 — Tool integration (edgecrab-tools)
  [ ] Add image_gen: Option<Arc<dyn ImageGenProvider>> to ToolContext
  [ ] Update ToolContext construction sites (AgentBuilder, tests)
  [ ] Implement generate_image ToolHandler (add resolution, thinking_level,
      enable_web_search, reference_images to JSON schema + arg parsing)
  [ ] Register tool in toolsets.rs
  [ ] Integration tests with MockImageGenProvider

  Phase 6 — CLI wiring (edgecrab-cli)
  [ ] Call ImageGenFactory::from_env() in agent builder
  [ ] Inject into ToolContext
  [ ] Update config.yaml docs

  Phase 7 — Documentation
  [ ] Add doc-comments to all public types
  [ ] Update README with EDGECRAB_IMAGEGEN_PROVIDER env var
  [ ] Add example in examples/generate_image.rs
  [ ] Add example in examples/edit_image_gemini.rs (multi-reference edit)
```

---

## 7. Data-Flow Diagram (End-to-End)

```
  User / LLM
      |
      |  tool call: generate_image
      |  { "prompt": "a cat on a mountain", "aspect_ratio": "16:9" }
      v
  edgecrab-tools / GenerateImageTool.handle()
      |
      |  ctx.image_gen.generate(ImageGenRequest { ... })
      v
  (if FalImageGen)                    (if VertexAIImageGen)
  FalImageGen::generate()             VertexAIImageGen::generate()
      |                                   |
      | POST queue.fal.run/...            | POST {region}-aiplatform...
      | Key $FAL_KEY                      | Bearer $GOOGLE_ACCESS_TOKEN
      | { prompt, image_size, ... }       | { instances:[{prompt}], params }
      |                                   |
      | poll /status until COMPLETED      | 200 { predictions:[{bytes,mime}] }
      |                                   |
      | GET /requests/{id}                | base64::decode()
      | { images:[{url,w,h}] }            |
      |                                   |
      v                                   v
  ImageGenResponse                    ImageGenResponse
  images[0].data = Url("https://…")  images[0].data = Bytes(Vec<u8>)
      |                                   |
      +-----------------------------------+
      |
      v
  GenerateImageTool saves to
  ~/.edgecrab/image_cache/{uuid}.jpg
      |
      v
  JSON result to LLM:
  {
    "provider": "fal-ai",
    "model": "fal-ai/flux/dev",
    "count": 1,
    "images": [{
      "url": "https://v3.fal.media/...",
      "width": 1280,
      "height": 720,
      "mime_type": "image/jpeg",
      "seed": 42
    }],
    "latency_ms": 2110
  }
```

---

## 8. Environment Variable Summary

| Variable                           | Provider        | Required | Description                               |
|------------------------------------|-----------------|----------|-------------------------------------------|
| `EDGECRAB_IMAGEGEN_PROVIDER`       | any             | No       | Force provider: gemini/nano-banana/fal/vertexai |
| `GEMINI_API_KEY`                   | Gemini (native) | Yes      | Google AI Studio API key                  |
| `EDGECRAB_GEMINI_IMAGEGEN_MODEL`   | Gemini          | No       | Override model (default: nb2)             |
| `FAL_KEY`                          | FAL/NanaBanana  | Yes      | FAL API key                               |
| `EDGECRAB_FAL_MODEL`               | FAL/NanaBanana  | No       | Override endpoint (fal-ai/nano-banana-2)  |
| `EDGECRAB_FAL_TIMEOUT_SECS`        | FAL             | No       | Queue timeout (default 300)               |
| `GOOGLE_CLOUD_PROJECT`             | VertexAI        | Yes      | GCP project ID                            |
| `GOOGLE_CLOUD_REGION`              | VertexAI+Gemini | No       | Region (default us-central1)              |
| `GOOGLE_ACCESS_TOKEN`              | VertexAI        | No*      | Static OAuth2 token                       |
| `GOOGLE_APPLICATION_CREDENTIALS`   | VertexAI        | No*      | Service account JSON path                 |
| `EDGECRAB_IMAGEGEN_MODEL`          | VertexAI        | No       | Override Imagen model                     |
| `EDGECRAB_IMAGEGEN_TIMEOUT_SECS`   | VertexAI/Gemini | No       | HTTP timeout (default 120)                |

*At least one VertexAI auth method required when using Vertex backend.
