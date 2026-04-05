# Image Generation Provider System — Overview

**Spec ID:** IMAGEGEN-000  
**Status:** Draft  
**Date:** 2026-04-04  
**Author:** Raphael Mansuy  

---

## 1. Motivation

EdgeCrab currently supports text-based LLM providers (`LLMProvider`) and embedding
providers (`EmbeddingProvider`). Neither trait models the distinct semantics of
**generative image creation**: prompt-in / image-out, aspect-ratio control, safety
settings, and heterogeneous output delivery (URL vs base64 bytes).

Adding image generation as a bolt-on to `LLMProvider` would pollute its contract.
Image synthesis is a *separate capability* with different:

- authentication models (API key, OAuth2, access token)
- output types (binary image bytes or signed CDN URLs)
- billing units (per-image, per-megapixel, or per-token)
- provider-specific knobs (guidance scale, safety filters, watermarks)

The goal of this spec set is to define a clean, extensible, provider-agnostic trait
and its first three implementations: **Vertex AI Imagen**, **fal.ai (FLUX)**, and
**Gemini Image Generation (Nano Banana)** — the last of which covers both the native
Gemini API (`ai.google.dev`) and FAL.ai's partner endpoints (`fal-ai/nano-banana-2`).

---

## 2. Scope

```
.
+--[ In scope ]----------------------------------------------+
|                                                            |
|  ImageGenProvider trait      (edgequake-llm crate)         |
|  VertexAI Imagen provider    (edgequake-llm crate)         |
|  FAL/FLUX provider           (edgequake-llm crate)         |
|  Gemini ImageGen provider    (edgequake-llm crate)         |
|    - native Gemini API (ai.google.dev)                     |
|    - FAL nano-banana-2 endpoint (partner route)            |
|  generate_image tool         (edgecrab-tools crate)        |
|  Factory / env detection     (edgequake-llm crate)         |
|                                                            |
+--[ Out of scope ]------------------------------------------+
|                                                            |
|  Video generation                                          |
|  LoRA fine-tuning                                          |
|  Local/Stable-Diffusion-WebUI integration                  |
|  Storage backend (use gateway_image_cache_dir)             |
|                                                            |
+------------------------------------------------------------+
```

> **Note on image editing:** FAL's `fal-ai/nano-banana-2/edit` and the Gemini API's
> multi-turn editing are now *in scope* as a second call mode on the
> `GeminiImageGenProvider`. Multi-image input (up to 14 references) is handled via
> the `reference_images` field in `ImageGenOptions`.

Future providers (OpenAI DALL-E 3, Stability AI, Replicate, Midjourney API) can be
added by implementing the trait without touching the rest of the system.

---

## 3. System Context Diagram

```
  +---------------------------------------------------------------+
  |                    edgecrab-tools                             |
  |                                                               |
  |   generate_image tool                                         |
  |   +----------------------------+                              |
  |   | ImageGenInput              |                              |
  |   |  prompt: String            |                              |
  |   |  options: ImageGenOptions  |    Tool call from agent      |
  |   +-----------+----------------+  <---  (JSON args)           |
  |               |                                               |
  +---------------|-----------------------------------------------+
                  | calls
  +---------------|-----------------------------------------------+
  |  edgequake-llm|                                               |
  |               v                                               |
  |   +------------------------------+                            |
  |   |   ImageGenProvider  (trait)  |                            |
  |   |   generate(req) -> resp      |                            |
  |   +-------+----------+----------+                             |
  |           |          |          |                             |
  |      +----+    +-----+    +-----+                             |
  |      v         v               v                              |
  | +---------+ +-------+  +----------------+                     |
  | | VertexAI| | FalAI |  | GeminiImageGen |                     |
  | | Imagen 4| | FLUX  |  | NanaBanana 2   |                     |
  | +---------+ +-------+  +-------+--------+                     |
  |      |          |              |                              |
  +------|----------|--------------|--------------------------+   |
         |          |              |                              |
         v          v              v                              |
  +----------+ +----------+ +-----------+  +-----------+          |
  | Vertex AI| | fal.run  | | Gemini API|  | fal.run   |          |
  | Imagen   | | FLUX     | | ai.google |  | nano-ban. |          |
  | REST     | | /dev     | | .dev      |  | -2        |          |
  +----------+ +----------+ +-----------+  +-----------+          |

  GeminiImageGenProvider supports two backends:
    A) Native Gemini API  -- GEMINI_API_KEY  (recommended)
    B) FAL nano-banana-2  -- FAL_KEY         (partner route)
```

---

## 4. Provider Comparison

| Dimension          | Vertex AI Imagen 4          | FAL / FLUX.1 dev         |
|--------------------|-----------------------------|--------------------------|
| Auth               | Bearer token (gcloud ADC)   | API Key header           |
| Output             | Base64 in response body     | Hosted CDN URL           |
| Models             | imagen-4.0-*-001, 3.0-*     | fal-ai/flux/dev, schnell |
| Aspect ratio       | Named strings (16:9, 1:1…)  | Named preset + WxH       |
| Quantity           | 1–4                         | 1–N                      |
| Safety             | block_medium_and_above etc  | enable_safety_checker    |
| Negative prompt    | Yes                         | No (FLUX doesn't use)    |
| Guidance scale     | Yes (0–30+)                 | Yes (CFG scale, ~3.5)    |
| Latency            | 5–20 s                      | 2–10 s (schnell faster)  |
| Pricing            | per image (~$0.02–0.08)     | per megapixel ($0.025)   |
| Watermark          | Optional (SynthID)          | No native watermark      |
| Queued mode        | None (sync only)            | Queue + polling + WH     |

---

## 5. Document Map

| File                           | Content                                                         |
|--------------------------------|-----------------------------------------------------------------|
| `000_overview.md`              | This file — motivation, scope, context                          |
| `001_adr_trait_design.md`      | ADR: ImageGenProvider trait design                              |
| `002_adr_integration.md`       | ADR: Integration with edgecrab tool system                      |
| `003_vertexai_spec.md`         | Vertex AI Imagen provider full spec                             |
| `004_fal_spec.md`              | FAL/FLUX + FAL Nano Banana provider specs                       |
| `005_implementation_plan.md`   | Rust implementation plan, types, modules, all providers         |
| `006_gemini_imagegen_spec.md`  | Gemini API native provider spec (ai.google.dev + Vertex AI)     |

---

## 6. Cross-Cutting Concerns

### 6.1 Security

- API keys stored in environment variables, never in config files
- Vertex AI access tokens obtained via `gcloud auth application-default login`
  or `GOOGLE_ACCESS_TOKEN` env var; tokens are short-lived (1 h)
- All outbound URLs validated via `edgecrab-security::url_safety::is_safe_url`
  before returning CDN image URLs to the agent
- No user prompt content is logged at INFO level (privacy)

### 6.2 Error handling

All provider errors map to a new `ImageGenError` enum (not `LlmError`) to avoid
conflating image-gen failures with text-gen failures in the agent's retry logic.

### 6.3 Caching

Image binaries are expensive to re-generate. The `generate_image` tool writes
images to `gateway_image_cache_dir()/<sha256>.{png,jpg}`. Cache key is
`sha256(provider_name + model + sorted_params_json)`.

### 6.4 Observability

Each call emits OpenTelemetry-compatible spans:
- `imagegen.generate` (root span)
- `imagegen.provider.request` (HTTP call)
- Attributes: `provider`, `model`, `image_count`, `resolution`, `latency_ms`
