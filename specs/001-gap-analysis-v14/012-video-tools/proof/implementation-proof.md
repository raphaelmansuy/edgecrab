# 012 Video Tools — Implementation Proof

Shipped:

- `crates/edgecrab-tools/src/tools/video/analyze.rs` — `video_analyze`
- `crates/edgecrab-tools/src/tools/video/generate.rs` — `video_generate`, `video_status`
- Opt-in toolset `video` (`VIDEO_TOOLS` in `toolsets.rs`)

Acceptance:

- Tools register via inventory and appear when `enabled_toolsets` includes `video`.
- `video_generate` returns non-blocking `job_id`; `video_status` polls mock store.
- Path jail + SSRF applied on local/URL analyze inputs.
