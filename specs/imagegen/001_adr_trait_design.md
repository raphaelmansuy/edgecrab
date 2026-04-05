# ADR-001: ImageGenProvider Trait Design

**Spec ID:** IMAGEGEN-001  
**Type:** Architecture Decision Record  
**Status:** Accepted  
**Date:** 2026-04-04  
**Deciders:** Raphael Mansuy  

---

## Context

We need a provider abstraction for AI image generation. The existing
`LLMProvider` and `EmbeddingProvider` traits in `edgequake-llm` establish the
pattern. We must decide:

1. Where to locate the trait (same crate vs new crate)
2. What the trait contract looks like
3. How to model the request / response types
4. How to handle provider-specific parameters without breaking the abstraction

---

## Decision Drivers

- **Minimise coupling**: image-gen is a distinct capability, not a sub-mode of text-gen
- **Extensibility**: new providers (DALL-E, Stability, Midjourney) must be addable
  without breaking existing callers
- **Type safety**: prevent mixing Vertex-specific params with FAL-specific params
- **Async-first**: all I/O must be `async`
- **Testability**: a `MockImageGenProvider` must satisfy the trait

---

## Option A — Extend `LLMProvider` with `generate_image()`

Add `async fn generate_image(&self, req: &ImageGenRequest) ...` as an optional
method with a default implementation that returns `Err(NotSupported)`.

```
  LLMProvider
    ├── complete()
    ├── chat()
    └── generate_image()   <- NEW optional fn
```

### Pros
- Single trait, simple factory
- Existing providers auto-get the method (returns error)

### Cons
- **Violates ISP** (Interface Segregation Principle): text providers must now carry
  dead image-gen methods
- `CompletionOptions` and `ImageGenOptions` are unrelated — shared struct is wrong
- Error surfacing becomes ambiguous (`LlmError` vs image-gen error)
- Harder to mock: `MockLLMProvider` must handle two concerns

**Decision: REJECTED**

---

## Option B — Separate `ImageGenProvider` Trait in `edgequake-llm`

Define a new trait in the existing `edgequake-llm` crate alongside `LLMProvider`
and `EmbeddingProvider`.

```
  edgequake-llm/src/
    traits.rs          LLMProvider, EmbeddingProvider
    imagegen/
      mod.rs           pub use
      traits.rs        ImageGenProvider trait
      types.rs         ImageGenRequest, ImageGenResponse, ImageGenOptions
      error.rs         ImageGenError
      providers/
        vertexai.rs    VertexAIImageGen
        fal.rs         FalImageGen
        mock.rs        MockImageGen
```

### Pros
- Strict separation of concerns — no ISP violation
- Own error type (`ImageGenError`) does not pollute `LlmError`
- Factory can return `Arc<dyn ImageGenProvider>` independently
- Providers can implement *only* `ImageGenProvider` (no `chat()` required)
- Clean extension path: add `ImageEditProvider`, `VideoGenProvider` etc.

### Cons
- Slightly more files
- Separate factory logic for image-gen providers

**Decision: ACCEPTED**

---

## Option C — New crate `edgequake-imagegen`

Full separate crate.

### Pros
- Hard dependency boundary

### Cons
- Workspace overhead
- Versioning complexity (must bump two crates)
- Circular dependency risk: `edgecrab-tools` already depends on `edgequake-llm`
- No benefit given edgequake-llm already has sub-modules per domain

**Decision: REJECTED** (premature crate split; revisit if trait surface >3 impls)

---

## Chosen Design (Option B)

### Trait Definition

```rust
// edgequake-llm/src/imagegen/traits.rs

use async_trait::async_trait;
use crate::imagegen::types::{ImageGenRequest, ImageGenResponse};
use crate::imagegen::error::ImageGenError;

/// Provider-agnostic image generation capability.
///
/// WHY a separate trait from LLMProvider:
///   - Image synthesis has different semantics (no token budget, no tool calls)
///   - Output type is fundamentally different (pixels, not text)
///   - Different billing (per-image / per-megapixel, not per-token)
///   - Allows providers to implement *only* what they support
#[async_trait]
pub trait ImageGenProvider: Send + Sync {
    /// Short identifier for logging / metrics ("vertexai-imagen", "fal-flux").
    fn name(&self) -> &str;

    /// Default model for this provider.
    fn default_model(&self) -> &str;

    /// Generate one or more images from the given request.
    async fn generate(
        &self,
        request: &ImageGenRequest,
    ) -> Result<ImageGenResponse, ImageGenError>;

    /// Optional: list models this provider exposes.
    /// Default: returns a slice with just `default_model()`.
    fn available_models(&self) -> Vec<&str> {
        vec![self.default_model()]
    }
}
```

### Request Type

```
  ImageGenRequest
  +-------------------------------------------------+
  | prompt:          String          (required)      |
  | model:           Option<String>  (provider dflt) |
  | options:         ImageGenOptions                 |
  +-------------------------------------------------+

  ImageGenOptions
  +-------------------------------------------------+
  | count:              u8         (1)               |
  | width:              Option<u32>                  |
  | height:             Option<u32>                  |
  | aspect_ratio:       Option<AspectRatio>          |
  | seed:               Option<u64>                  |
  | negative_prompt:    Option<String>               |
  | guidance_scale:     Option<f32>                  |
  | output_format:      ImageFormat (Png, Jpeg, Webp)|
  | safety_level:       SafetyLevel                  |
  | enhance_prompt:     bool       (false)           |
  | resolution:         Option<ImageResolution>      | <- Gemini image_size
  | enable_web_search:  Option<bool>                 | <- Gemini web grounding
  | thinking_level:     Option<ThinkingLevel>        | <- Gemini thinking
  | reference_images:   Vec<String>                  | <- up to 14 URLs (edit)
  | extra:              HashMap<String,JsonValue>    | <- escape hatch
  +-------------------------------------------------+

  ImageResolution  (Gemini / Nano Banana image_size)
  +-------------------------------------------------+
  | Half   = "512"   (0.5K) — only Gemini 3.1 Flash |
  | OneK   = "1K"    (default for Gemini models)     |
  | TwoK   = "2K"    (1.5x billing multiplier)       |
  | FourK  = "4K"    (2x billing multiplier)         |
  +-------------------------------------------------+

  ThinkingLevel  (Gemini reasoning control)
  +-------------------------------------------------+
  | Minimal = "minimal"  (lowest latency; default)  |
  | High    = "high"     (better quality; +cost)     |
  +-------------------------------------------------+
```

### Response Type

```
  ImageGenResponse
  +-------------------------------------------------+
  | images:          Vec<GeneratedImage>             |
  | provider:        String                          |
  | model:           String                          |
  | latency_ms:      u64                             |
  | enhanced_prompt: Option<String>                  |
  +-------------------------------------------------+

  GeneratedImage
  +-------------------------------------------------+
  | data:            ImageData                       |
  | width:           u32                             |
  | height:          u32                             |
  | mime_type:       String                          |
  | seed:            Option<u64>                     |
  +-------------------------------------------------+

  ImageData (enum)
  +-------------------------------------------------+
  | Bytes(Vec<u8>)   -- Vertex AI (base64 decoded)  |
  | Url(String)      -- FAL (CDN URL)               |
  +-------------------------------------------------+
```

### AspectRatio Enum

All ratios supported by any provider in the system:

```
  AspectRatio
    Auto        = "auto"  — model decides (Gemini / Nano Banana only)
    Square      = "1:1"   (1024x1024)
    SquareHd    = "1:1"   (2048x2048 high-quality)
    Landscape43 = "4:3"   (1024x768)
    Landscape169= "16:9"  (1280x720)
    Ultrawide   = "21:9"  (2560x1080)
    Portrait43  = "3:4"   (768x1024)
    Portrait169 = "9:16"  (720x1280)
    Frame54     = "5:4"   (1280x1024)
    Frame45     = "4:5"   (1024x1280)
    Print32     = "3:2"   (1152x768)
    Print23     = "2:3"   (768x1152)
    Extreme41   = "4:1"   (ultra-wide banner) — Gemini 3.1 Flash Image only
    Extreme14   = "1:4"   (ultra-tall banner) — Gemini 3.1 Flash Image only
    Extreme81   = "8:1"   (panoramic strip)   — Gemini 3.1 Flash Image only
    Extreme18   = "1:8"   (vertical strip)    — Gemini 3.1 Flash Image only
```

Provider availability matrix:

```
  Ratio        Vertex Imagen   FAL FLUX   Gemini API   FAL Nano Banana
  ─────────    ─────────────   ────────   ──────────   ───────────────
  auto         No              No         Yes          Yes
  1:1          Yes             Yes        Yes          Yes
  4:3          Yes             Yes        Yes          Yes
  16:9         Yes             Yes        Yes          Yes
  21:9         No              No         Yes          Yes
  3:4          Yes             Yes        Yes          Yes
  9:16         Yes             Yes        Yes          Yes
  5:4          Yes             No         Yes          Yes
  4:5          No              No         Yes          Yes
  3:2          Yes             No         Yes          Yes
  2:3          No              No         Yes          Yes
  4:1          No              No         Yes*         Yes*
  1:4          No              No         Yes*         Yes*
  8:1          No              No         Yes*         Yes*
  1:8          No              No         Yes*         Yes*

  * Only Gemini 3.1 Flash Image Preview (Nano Banana 2)
```

---

## Consequences

### Positive

- Tool layer (`generate_image` tool) talks only to `Arc<dyn ImageGenProvider>`,
  never to concrete types
- Provider implementations are individually testable with `MockImageGenProvider`
- `extra: HashMap<String, JsonValue>` in `ImageGenOptions` allows each provider
  to accept its specific knobs without breaking the common contract
- Clear upgrade path: add `async fn poll_async()` for queue-based providers
  without breaking the sync `generate()` path
- `ImageResolution` and `ThinkingLevel` are first-class fields — Gemini's unique
  capabilities are not hidden in the untyped `extra` map
- `reference_images: Vec<String>` enables image editing on Gemini models including
  the FAL nano-banana-2/edit endpoint (up to 14 URLs / base64 data URIs)
- Three concrete providers sharing the same trait: `VertexAIImageGen`,
  `FalImageGen` (FLUX), `GeminiImageGen` (Nano Banana — native API or FAL route)

### Negative / Risks

- `ImageData::Url` vs `ImageData::Bytes` forces callers to match — accepted
  because callers need to know where the bytes are to store/serve them
- `extra` fields are untyped; provider docs must be clear about accepted keys
- `ImageResolution` and `ThinkingLevel` fields are silently ignored by Vertex AI
  Imagen and FLUX providers — callers should consult provider docs

### Neutral

- `AsyncImageGenProvider` (queue support) deferred to a later ADR; FAL's
  queue is wrapped synchronously in the initial implementation
- `GeminiImageGen` supports two backends (native Gemini API vs FAL nano-banana-2);
  selected by factory env detection — see IMAGEGEN-006
