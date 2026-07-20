# 025 — Harness Balance: Reopen Cap + Prebuilt Demo Latch

**Status:** Implemented (2026-07-20)  
**Session:** `50d96e9d` (SimCity / `openrouter/tencent/hy3:free`)  
**Authority:** AE1–AE3 · L1–L8 · Hermes `verification_stop` max_attempts=2  
**Related:** [017](017-session-forensics-2026-07-19.md) · [019](019-non-flaky-harness-improvement-plan.md) · [022](022-session-roadblock-4f94111e-harness-deadlock.md)

---

## Verdict

EdgeCrab over-indexed on “never stop without evidence” while under-wiring Artifact latch for **already-built** demos. Model tried to finish → `[system: do not stop yet]` + stale `enable security.preview` debt → thrash until interrupt. `priming` is AwaitingFirstToken UX, not a stuck ReAct phase.

## Laws (L1–L8)

| ID | Law |
|----|-----|
| L1 | Model proposes; harness judges |
| L2 | Evidence must be load-bearing |
| L3 | Reopen is scarce (max 2) |
| L4 | Allowed-action invariant / escalate |
| L5 | Create ≠ verify budgets |
| L6 | Strip stale debt text |
| L7 | Operator sovereignty + session close |
| L8 | No mid-budget model nagging (default) |

## Code

| Module | Change |
|--------|--------|
| `evidence_latch.rs` | `seed_artifact_from_demo_dir` / known dirs; oracle_ok completes visual |
| `completion_reopen.rs` | `CompletionReopenGate` + `decide_completion_reopen` |
| `completion_assessor.rs` | `visual_ux_debt_reason` · `visual_product_oracle_ok` |
| `turn_dispatch.rs` | seed on `read_file`; oracle on terminal smoketest |
| `conversation.rs` | wire reopen gate; preserve `started_at`; budget pressure opt-in |
| `config.rs` | `max_completion_reopens=2`, `inject_budget_pressure=false` |
| `session_db.rs` | `close_stale_open_sessions` |
| `runtime.rs` | reap zombies on DB open (2h) |

## Tests

- Unit: `bal_025_seed_artifact_from_existing_demo_dir`, `completion_reopen::*`
- E2E: `nf_e8_prebuilt_demo_snapshot_stops_reopen`, `nf_e9_reopen_cap_ends_turn`

## Config

```yaml
harness:
  max_completion_reopens: 2
  inject_budget_pressure: false   # L8 — opt-in only
```
