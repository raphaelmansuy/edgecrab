# Task Log — 2025-07-25 — WhatsApp Platform Vision Fix + Full Test Suite

## Actions
- Resumed from summarized context; ran `cargo test` to validate all in-progress gateway/platform changes
- **872 tests passed, 0 failed** across all 21 crates

## Decisions
- No additional code changes needed; all 8 edits from the session were already committed and building cleanly
- Accepted minor text duplication between `app.rs` (CLI) and `platform.rs` (gateway) format strings — cost of new shared crate dependency outweighs DRY benefit for a static string

## Changes Delivered This Session (full picture)
1. `vision.rs`: from_url HTTPS passthrough, detail param, max_tokens 4096, borrow-after-move fix, multi-root jail
2. `path_utils.rs`: jail_read_path_multi — trusts cwd AND ~/.edgecrab/images/
3. `file_read.rs`: redirect image extensions → "use vision_analyze instead"
4. `browser.rs`: browser_vision schema — "ONLY for web pages, NOT local files"
5. `prompt_builder.rs`: VISION_GUIDANCE constant injected when vision_analyze in tool list
6. `app.rs`: handle_paste, is_image_path, Ctrl+Shift+V, format_image_attachment_block()
7. `platform.rs`: format_image_attachment_block(attachments) + 5 tests
8. `run.rs`: single dispatch point — appends *** ATTACHED IMAGES *** block to effective_text for ALL gateway platforms

## Next Steps
- Manual smoke test on WhatsApp platform: paste image → confirm vision_analyze selected, not browser_vision
- Consider consolidating the VISION_GUIDANCE format contract into a shared constant (edgecrab-types) when adding a new platform

## Lessons/Insights
- Single gateway dispatch point (run.rs) is the correct DRY chokepoint — individual platform adapters need no per-image handling
- VISION_GUIDANCE fires ONLY when *** ATTACHED IMAGES text is present in the user message — CLI and gateway now both inject it
