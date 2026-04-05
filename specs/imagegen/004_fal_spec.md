# FAL / FLUX Provider Specification

**Spec ID:** IMAGEGEN-004  
**Status:** Draft  
**Date:** 2026-04-04  

---

## 1. Overview

The **FalImageGen provider** calls the [fal.ai](https://fal.ai) REST API to
generate images using FLUX and other diffusion models hosted on the FAL
marketplace.

FAL supports multiple calling modes (direct, queue, stream). This provider
implements the **queue-backed synchronous** mode (`subscribe`) for reliability,
wrapped into `async fn generate()`.

```
  +------------------------------------------------------------------+
  |  FalImageGen                                                     |
  |                                                                  |
  |  +-----------------+     POST https://queue.fal.run/{endpoint}   |
  |  | ImageGenRequest +---> Authorization: Key $FAL_KEY             |
  |  +-----------------+     Content-Type: application/json          |
  |         ^                                                        |
  |         |   poll loop (GET /requests/{id}/status)                |
  |  +------+----------+                                             |
  |  | FalQueuePoller  |    on Completed: GET /requests/{id}         |
  |  +-----------------+     -> { images: [{url, width, height}] }   |
  |                                                                  |
  |                    +------------------------------+              |
  |                    | ImageGenResponse             |              |
  |                    |  images[].data = Url(String) |              |
  |                    +------------------------------+              |
  +------------------------------------------------------------------+
```

---

## 2. API Reference

### 2.1 Base URLs

| Mode            | URL                                       |
|-----------------|-------------------------------------------|
| Direct (sync)   | `https://fal.run/{endpoint}`              |
| Queue (submit)  | `https://queue.fal.run/{endpoint}`        |
| Queue (status)  | `https://queue.fal.run/{endpoint}/requests/{id}/status` |
| Queue (result)  | `https://queue.fal.run/{endpoint}/requests/{id}` |

This provider uses **queue mode** for production reliability.

### 2.2 Authentication

```
Authorization: Key {FAL_KEY}
Content-Type: application/json
```

`FAL_KEY` is read from the `FAL_KEY` environment variable (required).

### 2.3 Supported Models (Endpoint IDs)

| Endpoint ID                       | Model              | Notes                        |
|-----------------------------------|--------------------|------------------------------|
| `fal-ai/flux/dev`                 | FLUX.1 dev (12B)   | Best quality, commercial OK  |
| `fal-ai/flux/schnell`             | FLUX.1 schnell     | Fastest (~1–2 s)             |
| `fal-ai/flux/pro`                 | FLUX.1 pro         | Highest quality (API only)   |
| `fal-ai/stable-diffusion-xl`      | SDXL               | Wider LoRA ecosystem         |
| `fal-ai/aura-flow`                | AuraFlow           | Open-source alternative      |
| `fal-ai/hyper-sdxl`               | Hyper-SDXL         | < 2 s generation             |

Default: `fal-ai/flux/dev`

Override via `EDGECRAB_FAL_MODEL` or the `model` field in `ImageGenRequest`.

---

## 3. Request Mapping

### 3.1 FLUX.1 Wire Format

```json
{
  "prompt":                "a photorealistic sunset over mountains",
  "image_size":            "landscape_16_9",
  "num_inference_steps":   28,
  "seed":                  42,
  "guidance_scale":        3.5,
  "num_images":            1,
  "enable_safety_checker": true,
  "output_format":         "jpeg",
  "acceleration":          "none"
}
```

Custom size (when `width`/`height` provided instead of `aspect_ratio`):
```json
{
  "image_size": { "width": 1280, "height": 720 }
}
```

### 3.2 `ImageGenOptions` → FLUX Parameter Mapping

```
  ImageGenOptions field         FAL / FLUX parameter
  ─────────────────────────     ───────────────────────────────────────
  count                    -->  num_images            (1–N)
  aspect_ratio             -->  image_size (named preset, see §3.3)
  width + height           -->  image_size: { width, height }
  seed                     -->  seed
  negative_prompt          -->  (IGNORED — FLUX does not use neg prompts)
  guidance_scale           -->  guidance_scale        (default 3.5)
  output_format (Png)      -->  output_format = "png"
  output_format (Jpeg)     -->  output_format = "jpeg"
  enhance_prompt           -->  (not natively supported; ignored)
  safety_level (Block*)    -->  enable_safety_checker = true
  safety_level (None)      -->  enable_safety_checker = false
  extra["steps"]           -->  num_inference_steps
  extra["acceleration"]    -->  acceleration ("none"|"regular"|"high")
```

### 3.3 Aspect Ratio → `image_size` Mapping

```
  AspectRatio                FAL image_size preset
  ─────────────────────      ──────────────────────────
  Square      (1:1)     -->  "square"          (512x512)
  Square HD   (1:1)     -->  "square_hd"       (1024x1024)
  Landscape43 (4:3)     -->  "landscape_4_3"   (1024x768) [default]
  Landscape169 (16:9)   -->  "landscape_16_9"  (1280x720)
  Portrait43  (3:4)     -->  "portrait_4_3"    (768x1024)
  Portrait169 (9:16)    -->  "portrait_16_9"   (720x1280)
```

Default image_size: `landscape_4_3` (matches FLUX.1 default).

### 3.4 Inference Steps (default for each model)

```
  fal-ai/flux/dev          28 steps   (quality)
  fal-ai/flux/schnell       4 steps   (speed — designed for 1–4 steps)
  fal-ai/flux/pro          40 steps   (quality)
```

---

## 4. Queue Protocol

```
  Step 1 — Submit
  ─────────────────────────────────────────────────────
  POST https://queue.fal.run/fal-ai/flux/dev
  { ...request body... }

  Response (202 Accepted):
  {
    "request_id": "764cabcf-b745-4b3e-ae38-1200304cf45b",
    "status":     "IN_QUEUE",
    "status_url": "https://queue.fal.run/.../requests/{id}/status",
    "response_url":"https://queue.fal.run/.../requests/{id}"
  }

  Step 2 — Poll status (until Completed or Failed)
  ─────────────────────────────────────────────────────
  GET https://queue.fal.run/.../requests/{request_id}/status

  Responses:
    { "status": "IN_QUEUE",    "queue_position": 2 }
    { "status": "IN_PROGRESS", "logs": [...] }
    { "status": "COMPLETED" }
    { "status": "FAILED",      "error": "..." }

  Poll interval: 500 ms, exponential back-off up to 5 s, timeout 300 s

  Step 3 — Fetch result (when status == "COMPLETED")
  ─────────────────────────────────────────────────────
  GET https://queue.fal.run/.../requests/{request_id}
  Authorization: Key {FAL_KEY}

  Response (200 OK):
  {
    "images": [
      {
        "url":          "https://v3.fal.media/files/koala/abc123.jpeg",
        "width":        1024,
        "height":       768,
        "content_type": "image/jpeg"
      }
    ],
    "seed":   42,
    "prompt": "a photorealistic sunset over mountains",
    "has_nsfw_concepts": [false],
    "timings": { "inference": 2.11 }
  }
```

### Queue State Machine

```
          POST /submit
               |
               v
         [ IN_QUEUE ]
         queue_pos >= 0
               |  (runner picks up)
               v
        [ IN_PROGRESS ]
        logs streaming
               |  (runner finishes)
               v
          [COMPLETED]
               |                   [FAILED]
               |                       |
               v                       v
        GET /requests/{id}     ImageGenError::ProviderError
        parse images[]
               |
               v
        ImageGenResponse
        images[].data = Url(...)
```

---

## 5. Response Parsing

```
  fal_response.images[i].url          -> ImageData::Url(String)
  fal_response.images[i].width        -> GeneratedImage.width
  fal_response.images[i].height       -> GeneratedImage.height
  fal_response.images[i].content_type -> GeneratedImage.mime_type
  fal_response.seed                   -> GeneratedImage.seed (all images same seed)
  fal_response.timings.inference      -> ImageGenResponse.latency_ms (x1000)
```

---

## 6. Error Handling

```
  HTTP 400  -> ImageGenError::InvalidRequest (bad endpoint, bad body)
  HTTP 401  -> ImageGenError::AuthError (FAL_KEY missing or invalid)
  HTTP 404  -> ImageGenError::InvalidRequest (unknown endpoint ID)
  HTTP 429  -> ImageGenError::RateLimited { retry_after: None }
  HTTP 5xx  -> ImageGenError::ProviderError { status, body }

  Queue FAILED:
  { "status": "FAILED", "error": "out of memory" }
  -> ImageGenError::ProviderError { message: "..." }

  NSFW filter triggered:
  has_nsfw_concepts: [true]
  -> ImageGenError::ContentFiltered { reason: "NSFW content detected" }

  Poll timeout (> POLL_TIMEOUT_SECS):
  -> ImageGenError::Timeout { elapsed_secs: u64 }
```

---

## 7. Configuration

### 7.1 Environment Variables

| Variable                     | Required | Description                          |
|------------------------------|----------|--------------------------------------|
| `FAL_KEY`                    | Yes      | FAL API key                          |
| `EDGECRAB_FAL_MODEL`         | No       | Override default endpoint            |
| `EDGECRAB_FAL_TIMEOUT_SECS`  | No       | Total timeout in seconds (default 300)|
| `EDGECRAB_FAL_POLL_INTERVAL` | No       | Initial poll interval ms (default 500)|

### 7.2 `FalImageGenConfig` Struct

```rust
#[derive(Debug, Clone)]
pub struct FalImageGenConfig {
    pub api_key:           String,        // from FAL_KEY
    pub default_endpoint:  String,        // from EDGECRAB_FAL_MODEL
    pub timeout_secs:      u64,           // default: 300
    pub poll_interval_ms:  u64,           // default: 500
    pub max_poll_interval_ms: u64,        // default: 5000
}

impl FalImageGenConfig {
    pub fn from_env() -> Result<Self, ImageGenError> { ... }
}
```

---

## 8. Struct Layout

```
  edgequake-llm/src/imagegen/providers/fal.rs

  FalImageGen
  ├── config: FalImageGenConfig
  └── client: reqwest::Client

  impl ImageGenProvider for FalImageGen {
      fn name()          -> "fal-ai"
      fn default_model() -> "fal-ai/flux/dev"
      async fn generate(req) -> Result<ImageGenResponse, ImageGenError>
  }

  Private helpers:
    async fn submit(&self, endpoint, body) -> Result<String, ImageGenError>
      // returns request_id

    async fn poll_until_complete(&self, endpoint, request_id)
      -> Result<serde_json::Value, ImageGenError>

    fn build_flux_body(&self, req: &ImageGenRequest) -> serde_json::Value
    fn parse_result(&self, body: Value) -> Result<ImageGenResponse, ImageGenError>
    fn base_url(&self) -> &str  // "https://queue.fal.run"
    fn auth_header(&self) -> String  // "Key {api_key}"
```

---

## 9. Sequence Diagram

```
  generate_image tool        FalImageGen            fal.run API
       |                          |                      |
       |--- generate(req) ------->|                      |
       |                          |                      |
       |                  build_flux_body(req)           |
       |                          |                      |
       |                          |--- POST /queue/submit-->
       |                          |    Key {FAL_KEY}     |
       |                          |<-- 202 {request_id}--|
       |                          |                      |
       |                   poll loop                     |
       |                   (until COMPLETED)             |
       |                          |--- GET /status ------>
       |                          |<-- {IN_QUEUE}         |
       |                          |--- GET /status ------>
       |                          |<-- {IN_PROGRESS}      |
       |                          |--- GET /status ------>
       |                          |<-- {COMPLETED}        |
       |                          |                      |
       |                          |--- GET /result ------>
       |                          |<-- {images:[{url}]}  |
       |                          |                      |
       |                  parse_result()                 |
       |                          |                      |
       |<-- Ok(ImageGenResponse) -|                      |
       |    images: [ImageData::Url("https://…")]        |
```

---

## 10. Testing Strategy

| Test                                   | Method                                    |
|----------------------------------------|-------------------------------------------|
| Request body for FLUX dev              | `serde_json::assert_eq!` against fixture  |
| Aspect ratio mapping                   | Unit test each `AspectRatio` variant      |
| Queue polling (COMPLETED after 2 polls)| `MockServer` with state machine           |
| Queue polling timeout                  | `MockServer` returning IN_PROGRESS forever|
| NSFW filter detection                  | Mock result with `has_nsfw_concepts: [true]`|
| FAILED queue state                     | Mock result with `status: FAILED`         |
| Auth error (401)                       | Mock returning 401                        |
| Rate limit (429) + retry               | Retry middleware test                     |
| Round-trip with real FAL API           | Integration test, `#[ignore]`             |

---

## 11. FAL Nano Banana 2 (Gemini 3.1 Flash Image via FAL Partner)

> **What it is:** FAL.ai is an official Google partner distributing Gemini image
> generation models under the "Nano Banana" commercial name. The underlying model is
> identical to calling `gemini-3.1-flash-image-preview` via the native Gemini API —
> same SynthID watermark, same pricing tier, same capabilities. The FAL route is
> useful when the caller already has a FAL key and wants a unified queue-based
> interface across FLUX + Gemini models.
>
> For the **native Gemini API** approach (recommended), see IMAGEGEN-006.

### 11.1 Endpoints

| Endpoint                         | Purpose                          | Notes                    |
|----------------------------------|----------------------------------|--------------------------|
| `fal-ai/nano-banana-2`           | Text-to-image                    | Default NB 2 endpoint    |
| `fal-ai/nano-banana-2/edit`      | Image editing (up to 14 refs)    | `image_urls` required    |
| `fal-ai/nano-banana-pro`         | Pro (Gemini 3 Pro Image)         | $0.15/img, always thinks |
| `fal-ai/nano-banana`             | v1 (Gemini 2.5 Flash Image)      | Older generation         |

Auth and queue protocol are **identical** to FLUX (§2.2 and §4 above).

### 11.2 Nano Banana 2 Wire Format (T2I)

```json
{
  "prompt":              "a photorealistic sunset over mountains",
  "num_images":          1,
  "seed":                42,
  "aspect_ratio":        "16:9",
  "output_format":       "png",
  "safety_tolerance":    "4",
  "resolution":          "1K",
  "limit_generations":   true,
  "enable_web_search":   false,
  "thinking_level":      "minimal"
}
```

All fields except `prompt` are optional.

### 11.3 Nano Banana 2 / Edit Wire Format

```json
{
  "prompt":              "make the man in the photo drive a convertible along the coast",
  "image_urls":          [
    "https://example.com/photo1.png",
    "https://example.com/photo2.png"
  ],
  "num_images":          1,
  "aspect_ratio":        "auto",
  "output_format":       "png",
  "safety_tolerance":    "4",
  "resolution":          "1K",
  "limit_generations":   true
}
```

> `image_urls` is **required** for the edit endpoint. Pass up to 14 URLs or
> base64 `data:image/png;base64,...` strings. There is no mask — editing is
> semantic via natural language.

### 11.4 Parameter Mapping: ImageGenOptions → Nano Banana

```
  ImageGenOptions field         FAL Nano Banana parameter
  ─────────────────────────     ───────────────────────────────────────────────
  count                    -->  num_images             (1–N)
  aspect_ratio             -->  aspect_ratio (ratio string, "auto" if AspectRatio::Auto)
  seed                     -->  seed
  output_format (Png)      -->  output_format = "png"
  output_format (Jpeg)     -->  output_format = "jpeg"
  output_format (Webp)     -->  output_format = "webp"
  safety_level (BlockNone) -->  safety_tolerance = "6"
  safety_level (BlockLow)  -->  safety_tolerance = "5"
  safety_level (BlockMed)  -->  safety_tolerance = "3"     (default 4)
  safety_level (BlockHigh) -->  safety_tolerance = "1"
  resolution               -->  resolution ("512"/"1K"/"2K"/"4K")
  enable_web_search        -->  enable_web_search           (bool)
  thinking_level           -->  thinking_level ("minimal" | "high")
  reference_images         -->  image_urls (list<string>)   → edit endpoint
  negative_prompt          -->  (IGNORED — Gemini doesn't use neg prompts)
  guidance_scale           -->  (IGNORED — multimodal reasoning, not diffusion)
  width, height            -->  (IGNORED — use aspect_ratio + resolution instead)
  enhance_prompt           -->  (IGNORED)
  extra["limit_gen"]       -->  limit_generations           (bool)
```

**Routing rule:** if `reference_images` is non-empty → use `/edit` endpoint;
otherwise → use the T2I endpoint.

### 11.5 Aspect Ratio → Nano Banana String

```
  AspectRatio::Auto        → "auto"    (model decides — Gemini default)
  AspectRatio::Square      → "1:1"
  AspectRatio::Landscape43 → "4:3"
  AspectRatio::Landscape169→ "16:9"
  AspectRatio::Ultrawide   → "21:9"
  AspectRatio::Portrait43  → "3:4"
  AspectRatio::Portrait169 → "9:16"
  AspectRatio::Frame54     → "5:4"
  AspectRatio::Frame45     → "4:5"
  AspectRatio::Print32     → "3:2"
  AspectRatio::Print23     → "2:3"
  AspectRatio::Extreme41   → "4:1"
  AspectRatio::Extreme14   → "1:4"
  AspectRatio::Extreme81   → "8:1"
  AspectRatio::Extreme18   → "1:8"
```

> Extreme ratios (4:1, 1:4, 8:1, 1:8) are **only supported by Nano Banana 2**
> (`gemini-3.1-flash-image-preview`). Attempting them with FLUX or Vertex AI
> Imagen will return `ImageGenError::InvalidRequest`.

### 11.6 Response Parsing

The output schema is identical for T2I and edit:

```json
{
  "images": [
    {
      "url":          "https://v3.fal.media/files/nano-banana/abc123.png",
      "content_type": "image/png",
      "file_name":    "abc123.png",
      "file_size":    204800,
      "width":        1024,
      "height":       1024
    }
  ],
  "description": "A photorealistic sunset painting the sky in vivid orange and purple…"
}
```

Mapping:
```
  output.images[i].url          → ImageData::Url(String)
  output.images[i].width        → GeneratedImage.width
  output.images[i].height       → GeneratedImage.height
  output.images[i].content_type → GeneratedImage.mime_type
  output.description            → ImageGenResponse.enhanced_prompt
  (no seed in output)           → GeneratedImage.seed = None
```

### 11.7 Pricing

| Resolution  | Multiplier | Price / image (NB 2) | Price / image (NB Pro) |
|-------------|:----------:|:--------------------:|:----------------------:|
| 0.5K (512)  |  ×0.75     |  $0.060              | N/A                    |
| 1K (default)|  ×1.00     |  $0.080              | $0.150                 |
| 2K          |  ×1.50     |  $0.120              | $0.225                 |
| 4K          |  ×2.00     |  $0.160              | $0.300                 |
| +web search |  +$0.015   |  +$0.015 per image   | +$0.015 per image      |
| +high think |  +$0.002   |  +$0.002 per image   | (always thinks)        |

### 11.8 Struct Layout (NanoBananaImageGen)

```
  edgequake-llm/src/imagegen/providers/nano_banana.rs

  NanoBananaImageGen
  ├── config: NanoBananaConfig
  │     ├── api_key:           String        // FAL_KEY
  │     ├── default_endpoint:  String        // fal-ai/nano-banana-2
  │     ├── timeout_secs:      u64           // 300
  │     └── poll_interval_ms:  u64           // 500
  └── client: reqwest::Client

  impl ImageGenProvider for NanoBananaImageGen {
      fn name()          -> "fal-nano-banana"
      fn default_model() -> "fal-ai/nano-banana-2"
      fn available_models() -> ["fal-ai/nano-banana-2",
                                 "fal-ai/nano-banana-2/edit",
                                 "fal-ai/nano-banana-pro",
                                 "fal-ai/nano-banana"]
      async fn generate(req) -> Result<ImageGenResponse, ImageGenError>
  }

  Private helpers:
    fn resolve_endpoint(&self, req: &ImageGenRequest) -> String
      // Returns edit endpoint if reference_images non-empty

    fn build_body(&self, req: &ImageGenRequest) -> serde_json::Value
      // Builds nano-banana-specific JSON (not FLUX format)

    async fn submit_and_poll(&self, endpoint, body)
      -> Result<serde_json::Value, ImageGenError>
      // Reuses same queue protocol as FalImageGen

    fn parse_result(&self, value: Value)
      -> Result<ImageGenResponse, ImageGenError>
```

### 11.9 Environment Variables

| Variable                           | Required | Description                              |
|------------------------------------|----------|------------------------------------------|
| `FAL_KEY`                          | Yes      | FAL API key                              |
| `EDGECRAB_FAL_MODEL`               | No       | Override (set to `fal-ai/nano-banana-2`) |
| `EDGECRAB_IMAGEGEN_PROVIDER`       | No       | Set to `nano-banana` to select this      |

Factory detection order (see IMAGEGEN-002):
1. `EDGECRAB_IMAGEGEN_PROVIDER=nano-banana` → `NanoBananaImageGen`
2. `FAL_KEY` set + `EDGECRAB_FAL_MODEL` starts with `fal-ai/nano-banana` → `NanoBananaImageGen`
3. `FAL_KEY` set (model not nano-banana) → `FalImageGen` (FLUX)
4. Vertex AI env vars → `VertexAIImageGen`

### 11.10 Testing Strategy (Nano Banana additions)

| Test                                    | Method                                         |
|-----------------------------------------|------------------------------------------------|
| T2I request body (NB 2)                 | `serde_json::assert_eq!` against fixture       |
| Edit endpoint routing                   | Non-empty `reference_images` → `/edit` URL     |
| Edit request body (`image_urls` field)  | Fixture with 2 reference image URLs            |
| `AspectRatio::Auto` → `"auto"` string   | Unit test                                      |
| Extreme ratio 4:1 correct string        | Unit test                                      |
| `resolution` mapping (0.5K → `"512"`)  | Wait, NB uses `"0.5K"` not `"512"` (FAL)      |
| `safety_tolerance` from `SafetyLevel`   | Each variant yields correct string             |
| `thinking_level` field present          | Fixture comparison                             |
| `description` → `enhanced_prompt`       | Response parsing unit test                     |
| Round-trip with real FAL key            | Integration test, `#[ignore]`                  |
