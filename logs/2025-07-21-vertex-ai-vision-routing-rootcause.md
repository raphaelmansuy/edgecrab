# Root Cause: Vertex AI Vision Routing Failure

**Date**: 2025-07-21  
**Severity**: High — user-visible quota exhaustion (429) during vision analysis  
**Status**: Fixed

---

## Observed Symptom

```
✅ vision analyze   image_source: ~/.edgecrab/images/...   17.8s
❌ vision analyze   image_source: ~/.edgecrab/images/...    1.4s

LLM API error: API call failed after 0 retries: API error: Stream error: {
  "error": {
    "code": 429,
    "message": "You exceeded your current quota...",
    "status": "RESOURCE_EXHAUSTED"
  }
}
Quota exceeded for metric:
  generativelanguage.googleapis.com/generate_content_free_tier_requests
  limit: 20, model: gemini-2.5-flash
```

The user had `vertexai/gemini-2.5-flash` as the primary model (confirmed in status bar).  
The error came from `generativelanguage.googleapis.com` — the Google AI Studio endpoint — **NOT** Vertex AI (`aiplatform.googleapis.com`).

---

## Root Cause

### First Principles Analysis

The edgecrab agent has two distinct provider layers:

| Layer | Provider | Purpose |
|-------|----------|---------|
| `provider` | Vertex AI (`vertex-ai`) | Tool execution, sub-agents, context compression |
| `effective_provider` | Smart-routed cheap model | Main LLM API calls in conversation loop |

**The bug is in the `effective_provider` creation path** (`conversation.rs`, smart routing section).

### Code Path

When smart routing is configured with `cheap_model = "google/gemini-2.5-flash"`, the code does:

```rust
// conversation.rs ~line 429 (BEFORE fix)
let (prov_name, model_name) = route.model.split_once('/');
// prov_name = "google", model_name = "gemini-2.5-flash"
let canonical = match prov_name {
    "copilot" => "vscode-copilot",
    other => other,  // canonical = "google"
};
ProviderFactory::create_llm_provider(canonical, model_name)
// → create_llm_provider("google", "gemini-2.5-flash")
```

Inside `factory.rs::create_llm_provider`, `"google"` maps to `ProviderType::Gemini`:

```rust
// factory.rs (unchanged/by design)
ProviderType::Gemini => {
    if model.starts_with("vertexai:") {
        // Vertex AI path — NOT taken (no "vertexai:" prefix)
        GeminiProvider::from_env_vertex_ai()?.with_model(actual_model)
    } else {
        // Google AI Studio path — TAKEN
        GeminiProvider::from_env()?.with_model(model)
    }
}
```

And `GeminiProvider::from_env()` tries `GEMINI_API_KEY` FIRST:

```rust
// gemini.rs::from_env()
pub fn from_env() -> Result<Self> {
    if let Ok(api_key) = std::env::var("GEMINI_API_KEY") {
        return Ok(Self::new(api_key));  // ← Google AI Studio endpoint
    }
    Self::from_env_vertex_ai()  // ← only if GEMINI_API_KEY is absent
}
```

Since the user has `GEMINI_API_KEY` set in their environment (typical setup alongside Vertex AI credentials), the smart-routing cheap model is created as a **Google AI Studio provider**, not Vertex AI.

### Why This Bypasses the Vision Tool Fix

The vision tool itself (`vision_analyze`) correctly uses Vertex AI:

```rust
// dispatch_single_tool (conversation.rs)
let ctx = build_tool_context(..., dctx.provider.clone(), ...);
// dctx.provider = Some(provider.clone())  ← PRIMARY Vertex AI provider ✅

// vision.rs
let provider = ctx.provider.as_ref()...;
provider.chat(&messages, Some(&options))  // ← uses Vertex AI API ✅
```

BUT the **main LLM API calls** in the conversation loop use `effective_provider`:

```rust
// conversation.rs ~line 694
let response = api_call_with_retry(
    &effective_provider,  // ← Google AI Studio (free tier, 20 RPM limit) ❌
    ...
)
```

### Quota Mechanics

- **Google AI Studio free tier**: 20 RPM for `gemini-2.5-flash`
- **Vertex AI**: 1000+ QPM for paid accounts, billed per token
- When smart routing fires and routes even a SINGLE "simple" message to Google AI Studio, it consumes one of the 20 allowed requests per minute
- After a 17.8s vision analysis, the second main LLM call hits the 20-RPM Google AI Studio quota
- Error: `"API call failed after 0 retries"` (0 retries because native streaming mode disables retry logic)

### Why Native Streaming Causes Immediate Failure

```rust
// conversation.rs
let native_streaming_active = config.streaming
    && event_tx.is_some()
    && effective_provider.supports_tool_streaming();

// api_call_with_retry
let retry_budget = if use_native_streaming { 0 } else { max_retries };
// → retry_budget = 0 when streaming is active

// Error on failure:
Err(AgentError::Llm(format!(
    "API call failed after {} retries: {}",
    retry_budget,  // = 0
    last_err...
)))
```

Native streaming forces `retry_budget = 0`, so a single 429 from Google AI Studio terminates the session immediately with no retries.

---

## Fix Applied

**File**: `crates/edgecrab-core/src/conversation.rs`

When creating `effective_provider` for smart routing, detect if the primary provider is Vertex AI (`provider.name() == "vertex-ai"`) and the cheap model is a Google/Gemini model without the `vertexai:` prefix. In that case, automatically add the prefix and use `GeminiProvider::from_env_vertex_ai()` instead of `GeminiProvider::from_env()`.

```rust
let is_gemini_canonical =
    matches!(canonical, "google" | "gemini" | "vertex" | "vertexai");
let primary_is_vertex = provider.name() == "vertex-ai";
let already_vertex_prefixed = model_name.starts_with("vertexai:");

let (effective_canonical, effective_model): (&str, Cow<str>) =
    if is_gemini_canonical && primary_is_vertex && !already_vertex_prefixed {
        tracing::warn!(
            cheap_model = %route.model,
            "smart routing: coercing cheap Gemini model to Vertex AI endpoint \
             (primary is vertex-ai; using GEMINI_API_KEY would route to Google \
             AI Studio free tier and exhaust its quota)"
        );
        ("gemini", Cow::Owned(format!("vertexai:{model_name}")))
    } else {
        (canonical, Cow::Borrowed(model_name))
    };

ProviderFactory::create_llm_provider(effective_canonical, &effective_model)
```

---

## Impact Matrix

| Component | Before Fix | After Fix |
|-----------|-----------|-----------|
| Vision tool (`ctx.provider`) | ✅ Vertex AI | ✅ Vertex AI (unchanged) |
| Main LLM call (`effective_provider`) | ❌ Google AI Studio (if cheap_model = "google/...") | ✅ Vertex AI |
| Smart routing for non-Gemini models | ✅ Unaffected | ✅ Unaffected |
| Smart routing for Copilot models | ✅ Unaffected | ✅ Unaffected |
| auto_title_session (uses `effective_provider`) | ❌ Google AI Studio | ✅ Vertex AI |

---

## Recommended User Action

If you want to cheap-route with a Gemini model when your primary is `vertexai/*`:

```yaml
# config.json or similar — BEFORE (routing to Google AI Studio accidentally)
model_config:
  smart_routing:
    enabled: true
    cheap_model: "google/gemini-2.5-flash"  # ← silently used GEMINI_API_KEY

# AFTER FIX — the coercion is automatic, but you can also be explicit:
model_config:
  smart_routing:
    enabled: true
    cheap_model: "vertexai/gemini-2.5-flash"  # ← explicit Vertex AI
```

After the fix, the warning log will appear when the coercion fires:
```
WARN smart routing: coercing cheap Gemini model to Vertex AI endpoint
     (primary is vertex-ai; using GEMINI_API_KEY would route to Google AI Studio free tier...)
```

---

## Root Cause Chain (Summary)

```
User sets primary model: vertexai/gemini-2.5-flash
                               │
                    ┌──────────▼──────────────────────┐
                    │  provider = Vertex AI provider   │
                    │  (self.provider.read().await)    │
                    └──────────┬──────────────────────┘
                               │
         Smart routing config: cheap_model = "google/gemini-2.5-flash"
                               │
                    ┌──────────▼──────────────────────────────────┐
                    │  create_llm_provider("google", "gemini-2.5-flash")  │
                    │    → GeminiProvider::from_env()              │
                    │    → GEMINI_API_KEY found in env             │
                    │    → GeminiEndpoint::GoogleAI { api_key }   │
                    │    = Google AI Studio provider  ❌            │
                    └──────────┬──────────────────────────────────┘
                               │
              effective_provider = Google AI Studio (free tier, 20 RPM)
                               │
              Main LLM API calls use effective_provider
                               │
            ┌──────────────────┼────────────────────────┐
            │                  │                        │
      1st call OK         2nd call hits             vision tool
      (LLM → call          Google AI Studio          uses dctx.provider
      vision_analyze)      quota (20 RPM limit)      = Vertex AI ✅
            │                  │
       vision tool ✅ 17.8s   429 RESOURCE_EXHAUSTED
       (Vertex AI)            "API call failed after 0 retries"
                              (streaming → no retry)
```
