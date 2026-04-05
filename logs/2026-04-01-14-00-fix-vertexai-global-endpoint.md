# Task Log — 2026-04-01 — Fix VertexAI Global Endpoint URL Bug

## Actions
- Analysed 404 error: URL in error was `locations/global/...` — routing was CORRECT but host was `global-aiplatform.googleapis.com` (invalid)
- Traced root cause to `edgequake-llm` v0.3.0 `build_url()` using `https://{region}-aiplatform.googleapis.com` — with region=global produces invalid host
- Copied `edgequake-llm` 0.3.0 from cargo registry to `vendor/edgequake-llm`
- Added `GeminiProvider::vertex_host(region)` helper: returns `aiplatform.googleapis.com` for `global`, `{region}-aiplatform.googleapis.com` otherwise
- Patched 3 URL construction sites: `build_url`, `list_models`, cache URL
- Added 3 new test cases: `test_build_url_vertex_ai_global_region`, `test_vertex_host_global`, `test_vertex_host_regional`
- Added `[patch.crates-io]` to workspace `Cargo.toml` pointing to `vendor/edgequake-llm`
- Added `vendor/edgequake-llm` to `workspace.exclude`

## Decisions
- Vendor+patch over fork or Cargo.lock hack — cleanest, zero-dependency approach
- `vertex_host()` as reusable helper (DRY) rather than inline ternary in each URL format string
- Kept `GOOGLE_CLOUD_REGION=global` auto-set in `main.rs` — still needed for the library to know the region

## Next steps
- Run `edgecrab` with `vertexai/gemini-3-flash-preview` to confirm end-to-end
- Consider upstreaming the fix to `edgequake-llm` on GitHub

## Lessons/insights
- The 404 "not found" from Vertex AI with `locations/global` in the URL was a **host** problem, not a path problem. `global-aiplatform.googleapis.com` doesn't exist; the correct global endpoint is `aiplatform.googleapis.com`.
- Official docs use env var `GOOGLE_CLOUD_LOCATION` but the library uses `GOOGLE_CLOUD_REGION` — different names, same purpose.
