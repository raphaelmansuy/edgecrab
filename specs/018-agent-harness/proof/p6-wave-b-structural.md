# Proof: P6 Wave B–D structural controllers

**Date:** 2026-07-18  
**Status:** landed  
**Baselines:** sessions `a63fd17c` / `0f6fc6d6` are **pre-fix** (see [005-session-forensics](../005-session-forensics-game005-2026-07-18.md))

## Laws

1. **TCP listen probe** owns session HTTP port recording — not spawn text, not bare log lines alone.
2. **Structured browser results** — navigate/snapshot/vision emit JSON with `ok`, `final_url`, `error_text`, `is_chrome_error`, `node_count`.
3. **VisualUx assess** reads structured fields only — no prose heuristics (loader/spinner/ERR_* substrings).
4. **TaskClass** uses path/artifact signals — not vibe adjectives (`beautiful` / `amazing`).
5. **PreVerify** scripts with non-zero exit force `NeedsVerification`.
6. **Message timestamps** use per-message `created_at` across mid-turn checkpoints.
7. **web_extract** chrome-error URLs are non-success; `parse_tool_error_payload` unwraps delimiter envelopes.

## Verify

```bash
cargo test -p edgecrab-tools --lib probe_loopback
cargo test -p edgecrab-tools --lib structured_browser
cargo test -p edgecrab-core --lib navigate_alone
cargo test -p edgecrab-core --lib chrome_error
cargo test -p edgecrab-core --lib pre_verify_deny
cargo test -p edgecrab-state --lib message_timestamp
cargo test -p edgecrab-core --test visual_preview_lifecycle_e2e
```
