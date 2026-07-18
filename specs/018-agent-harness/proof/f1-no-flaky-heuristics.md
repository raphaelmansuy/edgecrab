# Proof: F1–F4 no flaky heuristics

**Date:** 2026-07-18  
**Status:** landed  
**Law:** Harness Complete / Block / storm / heal / contract decisions use typed facts only.

## Controllers

| Concern | Typed fact | Must not use |
|---------|------------|--------------|
| Approval → Blocked | `CompletionContext.pending_approval` | `"approval required"` prose |
| Visual evidence | `StructuredBrowserResult` | loader / spinner / ERR prose |
| Theater writes | JSON `path` basename | scanning tool-result body |
| Status evidence | `evidence[]` array | `summary` cheerleading |
| Contract | terminal_result / structured ok / harness backend | free-text needle on tool prose |
| Guardrail failure | `parse_tool_error_payload` / exit_code / structured ok | `"error"` / `"failed"` substrings |
| Port heal | parsed argv port + TCP probe | `unwrap_or(8000)` |
| Preview ready | TCP listen | English `"Serving HTTP"` |
| Nav server failure | `net::ERR_*` allowlist | Chrome UI copy |

## Verify

```bash
# Unit gates
cargo test -p edgecrab-core --lib approval_prose_alone
cargo test -p edgecrab-core --lib report_task_status_summary_alone
cargo test -p edgecrab-core --lib contract_evidence
cargo test -p edgecrab-tools --lib classify_tool_failure
cargo test -p edgecrab-tools --lib infer_http_server_port
cargo test -p edgecrab-tools --lib structured_browser_vision
cargo test -p edgecrab-tools --lib loopback_nav_server_failure

# CI grep gate (also in harness-benchmark.yml)
bash scripts/check-no-flaky-heuristics.sh
```

## Banned reintroduction

See [`scripts/check-no-flaky-heuristics.sh`](../../../scripts/check-no-flaky-heuristics.sh).
