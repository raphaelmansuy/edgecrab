# Proof P7 — Document class + bind latch (006 pptx forensics)

**Spec:** [006-session-forensics-pptx-4bcc4a8f](../006-session-forensics-pptx-4bcc4a8f.md)

## Gates

| Gate | Pass condition | Test / check |
|------|----------------|--------------|
| Class | `demo/pptx` alone ≠ VisualUx; landed `.pptx` → Document | `task_class::demo_dir_alone_is_not_visual_ux`, `landed_pptx_is_document` |
| Storm | Document does not arm VisualUx storm / theater blocks | `harness_advisory` VisualUx-only matches |
| Bind | Spawn JSON has `bind_ready`; reuse on same port; navigate → wait_bind | `dev_server` + `browser_navigate_wait_bind` |
| Binary | `read_file` on `.pptx` → structured binary stub | `file_read` media router |
| Skills | `edgecrab_home` / `skills` in file allowed roots | `config_ref::file_path_policy` |
| Deferred | Deferred soft-fail is typed `tool_error` | `deferred_tool_error_response` parses as tool_error |
| Telemetry | `tool_call_count` derived from tool messages on save/restore | `derive_tool_call_count` / `restore_session` |

## Commands

```bash
cargo test -p edgecrab-core --lib task_class
cargo test -p edgecrab-tools --lib dev_server
cargo test -p edgecrab-tools --lib file_read
cargo test -p edgecrab-tools deferred_tool_error
bash scripts/check-no-flaky-heuristics.sh
```
