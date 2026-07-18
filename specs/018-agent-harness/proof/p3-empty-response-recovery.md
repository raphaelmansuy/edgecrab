# Proof: P3 empty-response / half-open navigate recovery

**Date:** 2026-07-18  
**Status:** landed (session forensics game005)  
**Session seed:** `a63fd17c` — `net::ERR_EMPTY_RESPONSE` ×5 with no recovery JSON

## Law

1. Loopback `browser_navigate` failures whose CDP `errorText` (or navigate Err)
   matches connection-refused **or** empty-response / connection-reset / closed
   **must** attach `recovery_catalog::browser_navigate_no_server` (or port-heal)
   when no verified listening port is known — or when the recorded port is stale.
2. Bare `ExecutionFailed` without `recovery` for these classes is a harness bug.

## Anchors

| Piece | Location |
|-------|----------|
| Match helpers | `edgecrab_tools::tools::browser` — `is_loopback_nav_server_failure` |
| Recovery | `recovery_catalog::browser_navigate_no_server` / `browser_navigate_port_heal` |
| Session seed | `specs/018-agent-harness/005-session-forensics-game005-2026-07-18.md` |

## Verify

```bash
cargo test -p edgecrab-tools --lib empty_response
cargo test -p edgecrab-tools --lib loopback_nav_server_failure
```
