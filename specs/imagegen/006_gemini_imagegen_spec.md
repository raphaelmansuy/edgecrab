# Gemini Image Generation Provider Specification

**Spec ID:** IMAGEGEN-006  
**Status:** Draft  
**Date:** 2026-04-04  
**Models:** Nano Banana 2 (Gemini 3.1 Flash Image Preview), Nano Banana Pro (Gemini 3 Pro Image Preview), Nano Banana v1 (Gemini 2.5 Flash Image)

---

## 1. Overview

> **"Nano Banana"** is Google's commercial name for its Gemini-architecture native
> image generation capability. Unlike Imagen (a specialised diffusion model), Nano
> Banana uses Gemini's multimodal reasoning before and during generation, enabling
> *conversational editing, accurate text rendering, web-search grounding, multi-image
> reference composition, and chain-of-thought quality control*.

The `GeminiImageGenProvider` is the **recommended default** image-gen provider for
EdgeCrab when a `GEMINI_API_KEY` or Vertex AI credentials are available. 

```
  +-------------------------------------------------------------------+
  |  GeminiImageGenProvider                                           |
  |                                                                   |
  |  +-----------------+    POST /v1beta/models/{model}:generateContent|
  |  | ImageGenRequest +---> Authorization: Bearer {token_or_key}     |
  |  +-----------------+    Content-Type: application/json            |
  |         ^               x-goog-api-key: {GEMINI_API_KEY}         |
  |         |                                                          |
  |         |     inline_data.data (base64) in response               |
  |  +------+----------+                                              |
  |  | parse_response()|                                              |
  |  +-----------------+    ImageData::Bytes(decoded_png)             |
  |                                                                   |
  |     OR (Vertex AI backend)                                        |
  |                                                                   |
  |  POST https://{REGION}-aiplatform.googleapis.com/...             |
  |       Authorization: Bearer {ACCESS_TOKEN}                       |
  +-------------------------------------------------------------------+
```

---

## 2. Model Family

| Model ID                         | Nano Banana Name  | Best for                             |
|----------------------------------|-------------------|--------------------------------------|
| `gemini-3.1-flash-image-preview` | Nano Banana 2     | Default — speed + quality balance    |
| `gemini-3-pro-image-preview`     | Nano Banana Pro   | Professional quality, complex prompts|
| `gemini-2.5-flash-image`         | Nano Banana v1    | High-volume, low-latency, budget     |

All three models:
- Embed a **SynthID watermark** in every generated image (cannot be disabled)
- Support the `generateContent` endpoint (Gemini API and Vertex AI)
- Return images as base64 `inline_data` in the response

---

## 3. API Reference

### 3.1 Gemini API (ai.google.dev) — Recommended

**Endpoint:**
```
POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent
```

**Auth:** `x-goog-api-key: {GEMINI_API_KEY}` header  
OR `Authorization: Bearer {token}` (for Vertex AI — see §3.2)

**Base URL:** `https://generativelanguage.googleapis.com`

### 3.2 Vertex AI Backend (Alternative)

**Endpoint:**
```
POST https://{REGION}-aiplatform.googleapis.com/v1/projects/{PROJECT_ID}/locations/{REGION}/publishers/google/models/{MODEL_ID}:generateContent
```

**Auth:** `Authorization: Bearer {ACCESS_TOKEN}` (short-lived, from gcloud ADC)

The model IDs are the same (`gemini-3.1-flash-image-preview`, etc.) on both backends.

---

## 4. Request Format

### 4.1 Minimal Text-to-Image Request

```json
{
  "contents": [
    {
      "parts": [
        { "text": "A photorealistic sunset over alpine mountains, golden hour" }
      ]
    }
  ],
  "generationConfig": {
    "responseModalities": ["IMAGE"],
    "imageConfig": {
      "aspectRatio": "16:9",
      "imageSize": "1K"
    }
  }
}
```

### 4.2 Text-to-Image with Thinking + Web Search

```json
{
  "contents": [
    {
      "parts": [
        { "text": "Visualize today's weather in Tokyo as a stylized infographic" }
      ]
    }
  ],
  "tools": [
    {
      "googleSearch": {}
    }
  ],
  "generationConfig": {
    "responseModalities": ["TEXT", "IMAGE"],
    "imageConfig": {
      "aspectRatio": "16:9",
      "imageSize": "2K"
    },
    "thinkingConfig": {
      "thinkingLevel": "High",
      "includeThoughts": false
    }
  }
}
```

### 4.3 Image Editing with Reference Images

```json
{
  "contents": [
    {
      "parts": [
        {
          "inlineData": {
            "mimeType": "image/png",
            "data": "<base64_encoded_reference_image>"
          }
        },
        {
          "text": "Change the car in this photo to a red Ferrari"
        }
      ]
    }
  ],
  "generationConfig": {
    "responseModalities": ["IMAGE"],
    "imageConfig": {
      "aspectRatio": "3:2",
      "imageSize": "1K"
    }
  }
}
```

> For **multiple reference images** (up to 14), add each as a separate
> `inlineData` part before the text instruction. Parts are processed together
> by Gemini's multimodal context.

### 4.4 Text + Image Interleaved Output

```json
{
  "contents": [
    {
      "parts": [
        { "text": "Generate an illustrated step-by-step recipe for paella" }
      ]
    }
  ],
  "generationConfig": {
    "responseModalities": ["TEXT", "IMAGE"]
  }
}
```

---

## 5. Response Format

### 5.1 Image-Only Response (`responseModalities: ["IMAGE"]`)

```json
{
  "candidates": [
    {
      "content": {
        "parts": [
          {
            "inlineData": {
              "mimeType": "image/png",
              "data": "<base64_encoded_image>"
            },
            "thoughtSignature": "<Signature_A>"
          }
        ],
        "role": "model"
      },
      "finishReason": "STOP"
    }
  ],
  "usageMetadata": {
    "promptTokenCount": 12,
    "candidatesTokenCount": 0,
    "totalTokenCount": 12
  }
}
```

### 5.2 Text + Image Interleaved Response

```json
{
  "candidates": [
    {
      "content": {
        "parts": [
          { "text": "Here is step 1 of the recipe:\n\n### Step 1: The Sofrito\n\n" },
          {
            "inlineData": {
              "mimeType": "image/png",
              "data": "<base64_image_step1>",
              "thoughtSignature": "<Signature_B>"
            }
          },
          { "text": "\n\n### Step 2: Adding the Rice\n\n" },
          {
            "inlineData": {
              "mimeType": "image/png",
              "data": "<base64_image_step2>",
              "thoughtSignature": "<Signature_C>"
            }
          }
        ]
      }
    }
  ]
}
```

### 5.3 Response Parsing Rules

```
  For each candidate.content.parts:
    if part.thought == true  → skip (thinking intermediate, not billable output)
    if part.inline_data exists AND mime_type starts with "image/" →
      decode part.inline_data.data (base64) → ImageData::Bytes(Vec<u8>)
      create GeneratedImage {
        data:      ImageData::Bytes(decoded),
        width:     0,   // not returned; must infer from bytes or image_config
        height:    0,   // same — decode image header for actual dims
        mime_type: part.inline_data.mime_type,
        seed:      None,  // Gemini does not return seed
      }
    if part.text exists → append to ImageGenResponse.enhanced_prompt
```

---

## 6. `ImageGenOptions` → Gemini Parameter Mapping

```
  ImageGenOptions field    Gemini generationConfig / tool
  ─────────────────────    ──────────────────────────────────────────────────
  aspect_ratio         --> imageConfig.aspectRatio (ratio string, "auto" if Auto)
  resolution           --> imageConfig.imageSize ("512"/"1K"/"2K"/"4K")
  output_format        --> (post-process decode; specify mime_type for inline_data)
  count (num_images)   --> (Gemini does not reliably respect; generate in loop if needed)
  seed                 --> (NOT SUPPORTED by Gemini image models; ignored)
  negative_prompt      --> (NOT SUPPORTED; model uses reasoning instead)
  guidance_scale       --> (NOT APPLICABLE; not a diffusion model)
  enhance_prompt       --> (NOT SUPPORTED; Gemini always uses full reasoning)
  thinking_level       --> thinkingConfig.thinkingLevel ("minimal" | "High")
  enable_web_search    --> tools: [{ "googleSearch": {} }]
  reference_images     --> additional inlineData parts in contents[0].parts
  safety_level         --> (Gemini API safety settings — mapped separately)
```

### Safety Settings Mapping

```
  SafetyLevel::BlockNone   --> No safety settings override (most permissive API)
  SafetyLevel::BlockLow    --> threshold: BLOCK_ONLY_HIGH on all categories
  SafetyLevel::BlockMedium --> threshold: BLOCK_MEDIUM_AND_ABOVE (default)
  SafetyLevel::BlockHigh   --> threshold: BLOCK_LOW_AND_ABOVE
```

```json
"safetySettings": [
  { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_MEDIUM_AND_ABOVE" },
  { "category": "HARM_CATEGORY_HATE_SPEECH",       "threshold": "BLOCK_MEDIUM_AND_ABOVE" },
  { "category": "HARM_CATEGORY_HARASSMENT",        "threshold": "BLOCK_MEDIUM_AND_ABOVE" },
  { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_MEDIUM_AND_ABOVE" }
]
```

---

## 7. Aspect Ratio → Gemini String

```
  AspectRatio::Auto        → "auto"      (Gemini matches input or defaults to 1:1)
  AspectRatio::Square      → "1:1"
  AspectRatio::SquareHd    → "1:1"       (use ImageResolution::TwoK or FourK)
  AspectRatio::Landscape43 → "4:3"
  AspectRatio::Landscape169→ "16:9"
  AspectRatio::Ultrawide   → "21:9"
  AspectRatio::Portrait43  → "3:4"
  AspectRatio::Portrait169 → "9:16"
  AspectRatio::Frame54     → "5:4"
  AspectRatio::Frame45     → "4:5"
  AspectRatio::Print32     → "3:2"
  AspectRatio::Print23     → "2:3"
  AspectRatio::Extreme41   → "4:1"       (NB 2 only; error on Pro / v1)
  AspectRatio::Extreme14   → "1:4"       (NB 2 only)
  AspectRatio::Extreme81   → "8:1"       (NB 2 only)
  AspectRatio::Extreme18   → "1:8"       (NB 2 only)
```

> **Validation:** `GeminiImageGenProvider::build_request()` returns
> `ImageGenError::InvalidRequest` if an extreme ratio is requested with
> `gemini-3-pro-image-preview` or `gemini-2.5-flash-image`. Extreme ratios
> are only valid for `gemini-3.1-flash-image-preview` (Nano Banana 2).

---

## 8. Resolution → Pixel Dimensions

From the official Gemini API docs (3.1 Flash Image Preview):

| Ratio    | 512 (0.5K)  | 1K    | 2K    | 4K    |
|----------|-------------|-------|-------|-------|
| 1:1      | 512×512     | 1024×1024 | 2048×2048 | 4096×4096 |
| 16:9     | 688×384     | 1376×768  | 2752×1536 | 5504×3072 |
| 9:16     | 384×688     | 768×1376  | 1536×2752 | 3072×5504 |
| 3:2      | 632×424     | 1264×848  | 2528×1696 | 5056×3392 |
| 4:3      | 600×448     | 1200×896  | 2400×1792 | 4800×3584 |
| 21:9     | 792×168     | 1584×672  | 3168×1344 | 6336×2688 |

> `512` (0.5K) is only available for `gemini-3.1-flash-image-preview`.
> Pro and v1 only support `1K`, `2K`, `4K`.

---

## 9. Error Handling

```
  HTTP 400  → ImageGenError::InvalidRequest
  HTTP 401  → ImageGenError::AuthError
  HTTP 403  → ImageGenError::PermissionDenied
  HTTP 429  → ImageGenError::RateLimited { retry_after: response header }
  HTTP 5xx  → ImageGenError::ProviderError { status, message }

  finishReason == "SAFETY" → ImageGenError::ContentFiltered { reason: "Gemini safety filter" }
  finishReason == "OTHER"  → ImageGenError::ProviderError

  No image parts in response (text only) →
    ImageGenError::ProviderError { message: "No image in Gemini response" }
```

---

## 10. Struct Layout

```
  edgequake-llm/src/imagegen/providers/gemini.rs

  GeminiImageGenBackend (enum)
    GoogleAI { api_key: String }              // ai.google.dev — recommended
    VertexAI { project_id, region, token }    // Vertex AI backend

  GeminiImageGenConfig
  ├── backend:          GeminiImageGenBackend
  ├── default_model:    String                // gemini-3.1-flash-image-preview
  ├── timeout_secs:     u64                   // 120

  GeminiImageGenProvider
  ├── config: GeminiImageGenConfig
  └── client: reqwest::Client

  impl ImageGenProvider for GeminiImageGenProvider {
    fn name()           -> "gemini-imagegen"
    fn default_model()  -> "gemini-3.1-flash-image-preview"
    fn available_models() -> [
      "gemini-3.1-flash-image-preview",
      "gemini-3-pro-image-preview",
      "gemini-2.5-flash-image"
    ]
    async fn generate(req) -> Result<ImageGenResponse, ImageGenError>
  }

  Private helpers:
    fn base_url(&self, model: &str) -> String
    fn auth_headers(&self) -> reqwest::header::HeaderMap
    fn build_request(&self, req: &ImageGenRequest) -> Result<Value, ImageGenError>
    fn parse_response(&self, value: Value) -> Result<ImageGenResponse, ImageGenError>
    fn validate_model_constraints(&self, model: &str, options: &ImageGenOptions)
      -> Result<(), ImageGenError>
      // Checks extreme ratios not requested for non-NB2 models
      // Checks 0.5K resolution not requested for Pro / v1
```

---

## 11. Configuration

### 11.1 Environment Variables

| Variable                          | Required   | Description                            |
|-----------------------------------|------------|----------------------------------------|
| `GEMINI_API_KEY`                  | Yes (GoogleAI) | Gemini API key from aistudio.google.com|
| `GOOGLE_ACCESS_TOKEN`             | Yes (Vertex)   | Short-lived Bearer token (gcloud ADC)  |
| `GOOGLE_CLOUD_PROJECT`            | Vertex     | GCP project ID                         |
| `GOOGLE_CLOUD_REGION`             | No         | Vertex AI region (default `us-central1`)|
| `EDGECRAB_GEMINI_IMAGEGEN_MODEL`  | No         | Override default model                 |
| `EDGECRAB_IMAGEGEN_PROVIDER`      | No         | Set to `gemini` to select this provider|

### 11.2 Factory Detection Order

```
  EDGECRAB_IMAGEGEN_PROVIDER=gemini           → GeminiImageGenProvider (GoogleAI backend)
  GEMINI_API_KEY set                          → GeminiImageGenProvider (GoogleAI backend)
  GOOGLE_ACCESS_TOKEN + GOOGLE_CLOUD_PROJECT  → GeminiImageGenProvider (Vertex AI backend)
  FAL_KEY + EDGECRAB_FAL_MODEL=fal-ai/nano-*  → NanoBananaImageGen (FAL partner route)
  FAL_KEY (other model)                       → FalImageGen (FLUX)
  VERTEX_AI_* vars (Imagen)                   → VertexAIImageGen
```

---

## 12. Sequence Diagram (GoogleAI backend)

```
  generate_image tool    GeminiImageGenProvider     Gemini API
       |                          |                      |
       |--- generate(req) ------->|                      |
       |                          |                      |
       |              validate_model_constraints()       |
       |              build_request(req)                 |
       |                          |                      |
       |                          |--- POST /generateContent-->
       |                          |    x-goog-api-key    |
       |                          |    { contents, ... } |
       |                          |                      |
       |                          |<-- 200 {candidates}--|
       |                          |                      |
       |              parse_response()                   |
       |              for each part:                     |
       |                if inline_data → decode base64   |
       |                if text → enhanced_prompt        |
       |                          |                      |
       |<-- Ok(ImageGenResponse) -|                      |
       |    images: [ImageData::Bytes(Vec<u8>)]          |
```

---

## 13. Testing Strategy

| Test                                         | Method                                       |
|----------------------------------------------|----------------------------------------------|
| Request body (text-to-image, NB 2, 1K, 16:9) | `serde_json::assert_eq!` against fixture     |
| Request body with thinking + web search       | Fixture: `thinkingConfig`, `tools`           |
| Request body with 3 reference images          | Fixture: 3 `inlineData` parts                |
| Extreme ratio 4:1 accepted (NB 2 model)       | No error returned                            |
| Extreme ratio 4:1 rejected (Pro model)        | `ImageGenError::InvalidRequest`              |
| 0.5K resolution accepted (NB 2)               | No error                                     |
| 0.5K resolution rejected (Pro)               | `ImageGenError::InvalidRequest`              |
| Response parsing: image part → Bytes          | Decode base64, check mime_type               |
| Response parsing: thought parts skipped       | `part.thought == true` filtered out          |
| Response parsing: text parts → enhanced_prompt| Joined into `ImageGenResponse.enhanced_prompt`|
| `finishReason == "SAFETY"` → ContentFiltered  | Unit test                                    |
| HTTP 429 → RateLimited                        | Mock returning 429 with Retry-After          |
| GoogleAI backend → correct URL + header       | Unit test on `base_url()` + `auth_headers()` |
| VertexAI backend → correct URL + Bearer       | Unit test                                    |
| Round-trip with real Gemini API key           | Integration test, `#[ignore]`                |

---

## 14. Nano Banana Capability Matrix

```
  Feature                        NB 2 (3.1 Flash)  NB Pro (3 Pro)   NB v1 (2.5 Flash)
  ─────────────────────────────  ────────────────  ─────────────    ──────────────────
  Max resolution                 4K                4K               1K (1024px)
  0.5K (512px) resolution        Yes               No               No
  Extreme ratios (4:1, 8:1...)   Yes               No               No
  Default thinking               No (optional)     Always on        No
  Thinking level control         minimal / high    N/A              N/A
  Web search grounding           Web + Image       Web only         Basic
  Image search grounding (new)   Yes               No               No
  Max reference images           14 (10 obj+4 char)14 (6 obj+5 char)~3
  Character consistency          Up to 4           Up to 5          Limited
  Text rendering quality         High              Highest          Medium
  Pricing (1K)                   $0.08/img         $0.15/img        ~$0.04/img
  Speed (p50)                    3–6 s             10–20 s          1–3 s
```
