# ADR-002: Integration with EdgeCrab Tool System

**Spec ID:** IMAGEGEN-002  
**Type:** Architecture Decision Record  
**Status:** Accepted  
**Date:** 2026-04-04  
**Deciders:** Raphael Mansuy  

---

## Context

The `ImageGenProvider` trait lives in `edgequake-llm`. The agent interacts with
capabilities through **tools** registered in `edgecrab-tools`. We must decide:

1. How the tool accesses the provider (constructor injection vs env detection)
2. How the tool is named and what schema it exposes
3. Where generated images are stored and how their paths are returned to the agent
4. How to integrate the provider with the existing `ProviderFactory`

---

## Option A — Image-gen as a new `ToolContext` field

Add `image_gen: Option<Arc<dyn ImageGenProvider>>` to `ToolContext`.

```
  ToolContext
    ├── llm:        Arc<dyn LLMProvider>
    ├── embedder:   Arc<dyn EmbeddingProvider>
    └── image_gen:  Option<Arc<dyn ImageGenProvider>>   <- NEW
```

Tools that need image-gen check `ctx.image_gen` and return a helpful error
if `None`.

### Pros
- Consistent with how LLM / embedder are injected
- Easy to test: inject `MockImageGenProvider`
- Lazy: if no provider configured, image tools simply aren't available

### Cons
- `ToolContext` struct grows; must update all construction sites

**Decision: ACCEPTED**

---

## Option B — Tool detects provider from environment at call time

`generate_image` reads `IMAGEGEN_PROVIDER` env var and constructs the provider
inline on each call.

### Pros
- No `ToolContext` change
- Zero-config for simple scripts

### Cons
- Provider construction cost on every tool call
- Reinitialises HTTP client per-call (connection pools wasted)
- No dependency injection — untestable without real credentials
- Hidden global state

**Decision: REJECTED**

---

## Option C — Separate image-gen tool binary / sub-process

Call an external `edgecrab-imagegen` binary as a child process.

### Cons
- Process spawn overhead (seconds for each image)
- Complex IPC for binary data (base64 pipe or temp files)
- No benefit over in-process Arc<dyn>

**Decision: REJECTED**

---

## Chosen Design — Option A with `ImageGenFactory`

### ToolContext change

```rust
// edgecrab-tools/src/registry.rs  (illustrative)

pub struct ToolContext {
    pub llm:       Arc<dyn LLMProvider>,
    pub embedder:  Arc<dyn EmbeddingProvider>,
    /// Optional image generation provider.
    /// None if IMAGEGEN_PROVIDER is not configured.
    pub image_gen: Option<Arc<dyn ImageGenProvider>>,
    // ...existing fields...
}
```

### ImageGenFactory

```rust
// edgequake-llm/src/imagegen/factory.rs

pub struct ImageGenFactory;

impl ImageGenFactory {
    /// Build from environment variables.
    ///
    /// Provider detection order:
    ///   1. IMAGEGEN_PROVIDER env var (explicit override)
    ///   2. FAL_KEY set              -> FalImageGen
    ///   3. GOOGLE_ACCESS_TOKEN +
    ///      GOOGLE_CLOUD_PROJECT set -> VertexAIImageGen
    ///   4. None                     -> None (graceful absence)
    pub fn from_env() -> Option<Arc<dyn ImageGenProvider>> { ... }
}
```

Detection order rationale:
- FAL wins by default because it requires only one env var and has no GCP setup
- VertexAI is the enterprise path requiring GCP project + auth

### Tool Schema

```
Tool name: generate_image

Input schema:
  {
    "prompt":          string   (required, max 2000 chars)
    "model":           string?  (provider default if omitted)
    "count":           integer? (1, default 1, max 4)
    "aspect_ratio":    string?  ("1:1" | "4:3" | "16:9" | "3:4" | "9:16")
    "width":           integer? (overrides aspect_ratio width)
    "height":          integer? (overrides aspect_ratio height)
    "seed":            integer?
    "negative_prompt": string?
    "guidance_scale":  number?
    "output_format":   string?  ("png" | "jpeg", default "jpeg")
    "enhance_prompt":  boolean? (default false)
    "safety_level":    string?  ("block_none" | "block_low" | "block_medium" |
                                 "block_high", default "block_medium")
  }

Output (returned to agent):
  {
    "provider":  "fal-flux-dev",
    "model":     "fal-ai/flux/dev",
    "count":     1,
    "images": [
      {
        "path":      "/home/user/.edgecrab/image_cache/abc123.jpeg",
        "url":       "https://cdn.fal.ai/...",  // FAL only
        "width":     1024,
        "height":    768,
        "mime_type": "image/jpeg",
        "seed":      42
      }
    ],
    "enhanced_prompt": null,
    "latency_ms": 3120
  }
```

---

## Storage Strategy

```
  File system layout:
  ~/.edgecrab/
  └── image_cache/
      ├── fal/
      │   └── {sha256_of_request}.jpeg
      └── vertexai/
          └── {sha256_of_request}.png

  Cache key = sha256(json_sorted({
    provider, model, prompt, width, height,
    seed, guidance_scale, output_format
  }))
```

- FAL returns a CDN URL → download + store locally
- Vertex AI returns base64 bytes → decode + store locally
- The tool returns *both* the local `path` and the `url` (if available) so the
  agent can chain with `vision_analyze` or attach to gateway messages

---

## ProviderFactory Integration

`ProviderFactory::from_env()` currently returns `(Arc<dyn LLMProvider>, Arc<dyn EmbeddingProvider>)`.  
We extend it with a new method that does not break the existing signature:

```
  ProviderFactory
    ├── from_env() -> (LLM, Embed)         -- unchanged
    └── image_gen_from_env()               -- NEW
              -> Option<Arc<dyn ImageGenProvider>>
```

Callers that want image-gen call `image_gen_from_env()` explicitly. This avoids
forcing GCP credentials on users who only need text generation.

---

## Integration Diagram

```
  +--[ edgecrab-cli / gateway ]--------------------------+
  |                                                      |
  |  ProviderFactory::from_env()          -- LLM         |
  |  ImageGenFactory::from_env()          -- ImageGen    |
  |        |                                             |
  |        v                                             |
  |  ToolContext { llm, embedder, image_gen, ... }       |
  |        |                                             |
  +--------|---------------------------------------------+
           |
           v  (ToolHandler::handle)
  +--[ edgecrab-tools ]----------------------------------+
  |                                                      |
  |   generate_image tool                                |
  |      |                                               |
  |      |  1. validate prompt (SSRF, length)            |
  |      |  2. ctx.image_gen.as_ref()?.generate(req)     |
  |      |  3. save to image_cache_dir                   |
  |      |  4. return JSON summary                       |
  |                                                      |
  +------------------------------------------------------+
```

---

## Consequences

### Positive

- Tool activation is opt-in: users without API keys get a clear
  `"image_gen provider not configured"` error, not a crash
- Cache prevents re-generating the same image (cost saving)
- `MockImageGenProvider` + `ToolContext` injection makes unit tests trivial

### Risks

- `ToolContext` struct change requires updating `AgentBuilder`; this is a
  controlled one-time migration
- CDN URLs from FAL expire; the tool stores bytes locally to avoid expiry issues
