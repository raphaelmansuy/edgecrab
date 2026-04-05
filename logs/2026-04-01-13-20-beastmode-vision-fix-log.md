# Vision Analysis Fix — Task Log
Date: 2026-04-01 13:20

## Root Cause Analysis

### Primary Bug: Missing `copilot-vision-request` header

**The `vision_analyze` tool fails silently (empty response / JSON decode error) when using the `copilot/gpt-4.1` provider.**

#### Flow trace

```
vision_analyze(image_path) 
  → read file → base64 encode → ImageData::new(b64, "image/png")
  → ChatMessage::user_with_images(prompt, [image_data])
  → provider.chat(&messages, options)
    → VsCodeCopilotProvider::convert_messages (builds multipart content correctly ✓)
    → VsCodeCopilotClient::chat_completion(request)
      → build_headers()
        → vision_enabled = false  ← ROOT CAUSE
        → copilot-vision-request header NEVER SENT
  → GitHub Copilot API: receives image parts BUT no vision intent header
  → Returns empty content (or rejects the vision request)
  → response.content.trim().is_empty() → ToolError
```

#### What `vision_enabled` controls

In `edgequake_llm::VsCodeCopilotClient::build_headers()`:
```rust
if self.vision_enabled {
    headers.insert("copilot-vision-request", "true".parse().unwrap());
}
```

The `copilot-vision-request: true` header signals to the GitHub Copilot API that  
the request contains multimodal/vision content. Without it, the API ignores image  
parts and returns empty content.

#### Where the bug lives

Every call to `VsCodeCopilotProvider::new()` in the edgecrab codebase was missing  
`.with_vision(true)`, causing `supports_vision = false` → `vision_enabled = false` on the HTTP client:

| File | Line | Context |
|---|---|---|
| `edgecrab-cli/src/main.rs` | ~83 | Initial provider creation on startup |
| `edgecrab-cli/src/app.rs` | ~2919 | /model switch command |
| `edgecrab-core/src/conversation.rs` | ~396 | Smart routing cheap model |
| `edgecrab-core/src/conversation.rs` | ~655 | Fallback provider |
| `edgecrab-core/src/sub_agent_runner.rs` | ~127 | Sub-agent delegation |

### Secondary Bug (in external edgequake-llm 0.3.0): URL image handling

In `edgequake_llm::VsCodeCopilotProvider::convert_messages()`:
```rust
// BUG: format always produces "data:<mime>;base64,<data>"
let data_uri = format!("data:{};base64,{}", img.mime_type, img.data);
```

For URL images (`img.mime_type == "url"`, `img.data == "https://..."`) this produces:
`data:url;base64,https://example.com/photo.jpg` → **invalid**

Should use `img.to_api_url()` which dispatches correctly:
- URL images → returns the URL directly
- Base64 images → returns `data:{mime};base64,{data}`

**This does NOT affect the current bug** (clipboard images are always base64, not URL).  
It would affect `vision_analyze` on HTTPS URLs. This is an upstream edgequake-llm bug.

## Fix Applied

Added `.with_vision(true)` to the builder chain at all 5 provider-creation sites:

```rust
// Before (broken):
VsCodeCopilotProvider::new()
    .model(model_name)
    .build()

// After (fixed):
VsCodeCopilotProvider::new()
    .model(model_name)
    .with_vision(true)  // sends copilot-vision-request: true header
    .build()
```

## Verification

- `cargo check`: clean build ✓
- `cargo test`: 998 tests pass, 0 failed ✓

## Actions
- Fixed 5 VsCodeCopilotProvider::new() call sites to include `.with_vision(true)`

## Decisions
- Always enable vision for all copilot models — modern GPT-4.x models all support vision; the header is harmless for text-only requests

## Next Steps
- Test `vision_analyze` with a real clipboard image using `copilot/gpt-4.1`
- Consider upstreaming the URL image fix to edgequake-llm (secondary bug)

## Lessons
- When creating LLM providers, builder options like `with_vision()` MUST be set explicitly — they default to disabled
- The `copilot-vision-request: true` header is required for GitHub Copilot API to accept multimodal requests
