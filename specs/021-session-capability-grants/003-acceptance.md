# 003 — Acceptance

| ID | Scenario | Pass |
|----|----------|------|
| G1 | First loopback navigate blocked | Approval overlay appears (preview copy) |
| G2 | User Session | Same URL navigate succeeds without config persist |
| G3 | User Always | `security.preview.enabled` persisted |
| G4 | User Deny | No identical navigate spam; suppress_retry |
| G5 | Identical fails in transcript | Collapsed `×N (same failure)` |
| G6 | Node engine mismatch | Structured recovery with `needs_user_preference` / clarify hint — no silent nvm |

## Unit tests (canonical names)

| Acceptance name | Crate / module |
|-----------------|----------------|
| `session_preview_grants_allow_loopback_url` | `edgecrab-security` `url_safety::tests` |
| `once_preview_grant_consumes_on_first_check` | `edgecrab-security` `url_safety::tests` (G2 Once) |
| `preview_grant_payload_parsed_from_recovery` | `edgecrab-tools` `preview_grant::tests` |
| `browser_navigate_blocked_includes_request_user_grant` | `edgecrab-tools` `recovery_catalog::tests` (G1) |
| `collapse_identical_tool_failures` | `edgecrab-cli` `tool_display::tests` (G5) |
| `node_engine_mismatch_recovery_marks_preference` | `edgecrab-tools` `recovery_catalog::tests` (G6) |

Compat alias: `ha16_browser_blocked_includes_preview_hint` → calls `browser_navigate_blocked_includes_request_user_grant`.

## Manual / dogfood (G1–G4 live)

See [proof/g1-g6-session-capability.md](proof/g1-g6-session-capability.md). Unit tests cover grant store + recovery typing; live TUI still required to prove overlay → Session → green navigate.
