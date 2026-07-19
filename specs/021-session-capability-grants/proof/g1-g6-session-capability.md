# Proof — Session Capability Grants (G1–G6)

**Date:** 2026-07-19  
**Spec:** [../000-overview.md](../000-overview.md), [../004-first-principles.md](../004-first-principles.md)

## Automated coverage

| ID | Claim | Evidence |
|----|-------|----------|
| G1 | SSRF loopback attaches `RequestUserGrant` | `cargo test -p edgecrab-tools --lib browser_navigate_blocked_includes_request_user_grant` |
| G1 payload | Grant host/port/url parseable | `cargo test -p edgecrab-tools --lib preview_grant_payload_parsed_from_recovery` |
| G2 Session | Session grant allows loopback without global preview | `cargo test -p edgecrab-security --lib session_preview_grants_allow_loopback_url` |
| G2 Once | Once grant consumed after first check | `cargo test -p edgecrab-security --lib once_preview_grant_consumes_on_first_check` |
| G5 | Identical fails share fingerprint for ×N collapse | `cargo test -p edgecrab-cli collapse_identical_tool_failures` |
| G6 | Node engine mismatch → clarify preference, no silent nvm | `cargo test -p edgecrab-tools --lib node_engine_mismatch_recovery_marks_preference` |

### G3 / G4 (code path, not full E2E)

| ID | Claim | Where |
|----|-------|-------|
| G3 Always | Persist `security.preview.enabled` + port + hot-apply policy | `preview_grant::persist_preview_enabled` |
| G4 Deny | `PermissionDenied` + `should_suppress_retry` | Deny arm in `preview_grant::request_preview_loopback_grant`; `ToolError::PermissionDenied` suppresses retry |

## Live dogfood checklist (operator)

Run in TUI with preview disabled (`security.preview.enabled: false`):

1. Start a local HTTP server on `127.0.0.1:8000`.
2. Ask agent to `browser_navigate` that URL.
3. **G1:** Approval overlay shows preview copy (“Allow browser access to …”).
4. **G2:** Choose **Session** → navigate succeeds; `config.yaml` unchanged for `security.preview.enabled`.
5. New process, repeat → choose **Always** (**G3**) → `security.preview.enabled: true` persisted.
6. Fresh process with preview off → choose **Deny** (**G4**) → no navigate spam; suppress path.
7. If Node/engine mismatch appears during install (**G6**), agent should clarify toolchain once — not silent `nvm use`.

## Commands used for this proof

```bash
cargo test -p edgecrab-security --lib session_preview_grants_allow_loopback_url
cargo test -p edgecrab-security --lib once_preview_grant_consumes_on_first_check
cargo test -p edgecrab-tools --lib browser_navigate_blocked_includes_request_user_grant
cargo test -p edgecrab-tools --lib preview_grant_payload_parsed_from_recovery
cargo test -p edgecrab-tools --lib node_engine_mismatch_recovery_marks_preference
cargo test -p edgecrab-cli --bin edgecrab collapse_identical_tool_failures
```

## Verdict

Automated layer proves Laws A/B/D/E typing and grant store. Live G1–G4 dogfood remains the product metric check (≤1 user decision from first denial to navigate or Deny).
