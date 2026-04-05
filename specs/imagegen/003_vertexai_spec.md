# Vertex AI Imagen Provider Specification

**Spec ID:** IMAGEGEN-003  
**Status:** Draft  
**Date:** 2026-04-04  

---

## 1. Overview

The **VertexAI Imagen provider** calls the Vertex AI REST prediction API to
generate images using Google's Imagen 3 and Imagen 4 model family.

```
  +-----------------------------------------------------------------+
  |  VertexAIImageGen                                               |
  |                                                                 |
  |  +------------------+     POST /v1/projects/{proj}/...          |
  |  | ImageGenRequest  +---> /publishers/google/models/{m}:predict  |
  |  +------------------+     Bearer <access_token>                 |
  |                                                                 |
  |             Response: predictions[].bytesBase64Encoded          |
  |                    +-------------------------------+            |
  |                    | ImageGenResponse              |            |
  |                    |  images[].data = Bytes(Vec<u8>)|           |
  |                    +-------------------------------+            |
  +-----------------------------------------------------------------+
```

---

## 2. API Reference

### 2.1 Endpoint

```
POST https://{REGION}-aiplatform.googleapis.com/v1/projects/{PROJECT_ID}
     /locations/{REGION}/publishers/google/models/{MODEL}:predict
```

| Variable     | Source                                         | Default        |
|--------------|------------------------------------------------|----------------|
| `REGION`     | `GOOGLE_CLOUD_REGION` env var                  | `us-central1`  |
| `PROJECT_ID` | `GOOGLE_CLOUD_PROJECT` env var                 | (required)     |
| `MODEL`      | `model` field in request, or `default_model()` | see §2.3       |

### 2.2 Authentication

```
Authorization: Bearer {ACCESS_TOKEN}
Content-Type: application/json
```

Token acquisition order:
1. `GOOGLE_ACCESS_TOKEN` env var (static token — useful in CI)
2. `gcloud auth print-access-token` subprocess (dev machines)
3. Service account key via `GOOGLE_APPLICATION_CREDENTIALS` (server deployments)

```
  Token fetch logic:
  +-[ env GOOGLE_ACCESS_TOKEN ]----> use directly
  |
  +-[ env GOOGLE_APPLICATION_CREDENTIALS ]
  |     |
  |     v
  |   Read service account JSON
  |   Sign JWT (RS256)
  |   POST https://oauth2.googleapis.com/token
  |   -> Access token (1 h TTL)
  |
  +-[ gcloud CLI fallback ]---------> run subprocess
```

Token caching: refresh when `expiry - now < 5 min`. Use `RwLock<TokenCache>` for
thread-safe refresh.

### 2.3 Supported Models

| Model ID                          | Tier      | Size        | Notes                    |
|-----------------------------------|-----------|-------------|--------------------------|
| `imagen-4.0-ultra-generate-001`   | Imagen 4  | Ultra       | Highest quality, slowest |
| `imagen-4.0-generate-001`         | Imagen 4  | Standard    | Best quality/speed ratio |
| `imagen-4.0-fast-generate-001`    | Imagen 4  | Fast        | Fastest, good quality    |
| `imagen-3.0-generate-002`         | Imagen 3  | Standard    | Proven, stable           |
| `imagen-3.0-generate-001`         | Imagen 3  | Standard    | Older revision           |
| `imagen-3.0-fast-generate-001`    | Imagen 3  | Fast        | Economy tier             |

Default: `imagen-4.0-generate-001`

---

## 3. Request Mapping

### 3.1 Wire Format

```json
{
  "instances": [
    {
      "prompt": "{prompt}"
    }
  ],
  "parameters": {
    "sampleCount":      1,
    "aspectRatio":      "1:1",
    "negativePrompt":   "",
    "seed":             42,
    "guidanceScale":    7,
    "sampleImageStyle": "photograph",
    "enhancePrompt":    false,
    "addWatermark":     false,
    "safetySetting":    "block_medium_and_above",
    "personGeneration": "allow_adult",
    "language":         "auto",
    "outputOptions": {
      "mimeType": "image/jpeg"
    }
  }
}
```

### 3.2 `ImageGenOptions` → Parameters Mapping

```
  ImageGenOptions field         Vertex AI parameter
  ─────────────────────────     ─────────────────────────────────────
  count                    -->  parameters.sampleCount    (1–4)
  aspect_ratio             -->  parameters.aspectRatio    (named string)
  width + height           -->  NOT supported (use aspect_ratio instead)
  seed                     -->  parameters.seed
  negative_prompt          -->  parameters.negativePrompt
  guidance_scale           -->  parameters.guidanceScale (float, 0–30)
  output_format (Png)      -->  parameters.outputOptions.mimeType = "image/png"
  output_format (Jpeg)     -->  parameters.outputOptions.mimeType = "image/jpeg"
  enhance_prompt           -->  parameters.enhancePrompt
  safety_level             -->  parameters.safetySetting (see §3.3)
  extra["style"]           -->  parameters.sampleImageStyle
  extra["watermark"]       -->  parameters.addWatermark (bool)
  extra["person_gen"]      -->  parameters.personGeneration
  extra["language"]        -->  parameters.language
```

### 3.3 Safety Level Mapping

```
  ImageGenOptions.safety_level    Vertex AI safetySetting
  ──────────────────────────      ──────────────────────────────────
  SafetyLevel::BlockNone          "block_none"
  SafetyLevel::BlockLow           "block_low_and_above"
  SafetyLevel::BlockMedium        "block_medium_and_above"   (default)
  SafetyLevel::BlockHigh          "block_only_high"
```

### 3.4 Aspect Ratio Mapping

```
  AspectRatio::Square       -->  "1:1"
  AspectRatio::Landscape43  -->  "4:3"
  AspectRatio::Landscape169 -->  "16:9"
  AspectRatio::Portrait43   -->  "3:4"
  AspectRatio::Portrait169  -->  "9:16"
  AspectRatio::Frame54      -->  "5:4"
  AspectRatio::Print32      -->  "3:2"
```

---

## 4. Response Parsing

### 4.1 Wire Response

```json
{
  "predictions": [
    {
      "bytesBase64Encoded": "<BASE64_JPEG_BYTES>",
      "mimeType": "image/jpeg",
      "prompt": "<ENHANCED_PROMPT>"
    }
  ]
}
```

When `enhancePrompt=false`, the `prompt` field is absent.

### 4.2 Response Mapping

```
  predictions[i].bytesBase64Encoded  ->  base64::decode() ->  ImageData::Bytes(Vec<u8>)
  predictions[i].mimeType            ->  GeneratedImage.mime_type
  predictions[i].prompt              ->  ImageGenResponse.enhanced_prompt (first item only)

  GeneratedImage.width / height:
    - Vertex AI does NOT return pixel dimensions in the response
    - Derive from sampleImageSize param if set, else use aspect_ratio defaults
    - Imagen 4 default: 1024x1024 for 1:1, 1280x720 for 16:9, etc.
```

---

## 5. Error Handling

```
  HTTP 400  -> ImageGenError::InvalidRequest (bad prompt, unsupported param)
  HTTP 401  -> ImageGenError::AuthError (bad/expired token)
  HTTP 403  -> ImageGenError::PermissionDenied (project not enabled)
  HTTP 429  -> ImageGenError::RateLimited { retry_after: Option<Duration> }
  HTTP 5xx  -> ImageGenError::ProviderError { status, body } (retry eligible)

  Vertex-specific error bodies:
  { "error": { "code": 400, "message": "...", "status": "INVALID_ARGUMENT" } }
  { "error": { "code": 429, "message": "Resource exhausted", "status": "RESOURCE_EXHAUSTED" } }

  Content policy block (embedded in 200 OK):
  { "predictions": [], "metadata": { "rai_info": [...] } }
   -> ImageGenError::ContentFiltered { reason: String }
```

---

## 6. Configuration

### 6.1 Environment Variables

| Variable                          | Required | Description                          |
|-----------------------------------|----------|--------------------------------------|
| `GOOGLE_CLOUD_PROJECT`            | Yes      | GCP project ID                       |
| `GOOGLE_CLOUD_REGION`             | No       | Vertex AI region (default us-central1)|
| `GOOGLE_ACCESS_TOKEN`             | No*      | Short-lived OAuth2 access token       |
| `GOOGLE_APPLICATION_CREDENTIALS`  | No*      | Path to service account JSON         |
| `EDGECRAB_IMAGEGEN_MODEL`         | No       | Override default model               |
| `EDGECRAB_IMAGEGEN_TIMEOUT_SECS`  | No       | HTTP timeout (default 120)           |

*At least one auth method required.

### 6.2 `VertexAIImageGenConfig` Struct

```rust
#[derive(Debug, Clone)]
pub struct VertexAIImageGenConfig {
    pub project_id: String,
    pub region: String,               // default: "us-central1"
    pub model: String,                // default: "imagen-4.0-generate-001"
    pub timeout_secs: u64,            // default: 120
    pub http_client: Option<Client>,  // injectable for testing
}

impl VertexAIImageGenConfig {
    pub fn from_env() -> Result<Self, ImageGenError> { ... }
}
```

---

## 7. Struct Layout

```
  edgequake-llm/src/imagegen/providers/vertexai.rs

  VertexAIImageGen
  ├── config:     VertexAIImageGenConfig
  ├── client:     reqwest::Client
  └── token:      RwLock<AccessTokenCache>

  AccessTokenCache
  ├── token:      String
  └── expires_at: Instant

  impl ImageGenProvider for VertexAIImageGen {
      fn name()          -> "vertexai-imagen"
      fn default_model() -> "imagen-4.0-generate-001"
      async fn generate(req) -> Result<ImageGenResponse, ImageGenError>
  }

  Private helpers:
    async fn build_request_body(&self, req) -> serde_json::Value
    async fn get_access_token(&self)        -> Result<String, ImageGenError>
    fn endpoint_url(&self, model: &str)     -> String
    fn parse_response(body: Value)          -> Result<ImageGenResponse, ImageGenError>
```

---

## 8. Sequence Diagram

```
  generate_image tool          VertexAIImageGen       Vertex AI REST
       |                             |                      |
       |--- generate(req) ---------->|                      |
       |                             |                      |
       |                   get_access_token()               |
       |                   (cache hit or refresh)           |
       |                             |                      |
       |                   build_request_body(req)          |
       |                             |                      |
       |                             |--- POST /v1/projects/|
       |                             |    .../models/img:   |
       |                             |    predict --------->|
       |                             |    Bearer {token}    |
       |                             |                      |
       |                             |<-- 200 OK {predictions}
       |                             |                      |
       |                   parse_response()                 |
       |                   base64::decode(each prediction)  |
       |                             |                      |
       |<-- Ok(ImageGenResponse) ----|                      |
       |    images: [ImageData::Bytes(...)]                  |
```

---

## 9. Testing Strategy

| Test                               | Method                           |
|------------------------------------|----------------------------------|
| Token refresh on expiry            | `MockServer` + mock token endpoint|
| Request body serialisation         | `serde_json::assert_eq!`         |
| Base64 decode correctness          | Fixture .png round-trip          |
| 429 rate-limit error mapping       | `MockServer` returning 429       |
| Content filter (empty predictions) | `MockServer` returning `{predictions:[]}`|
| Auth error (401)                   | `MockServer` returning 401       |
| Full round-trip with real API      | Integration test, `#[ignore]`    |
