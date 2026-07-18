# 002 — Stale Claims (Jun 016 → July code)

Baseline: [`../016-harness-assessment/006-brutal-verdict.md`](../016-harness-assessment/006-brutal-verdict.md) (2026-06-19).

| Jun 016 claim | July 2026 code status | Anchor |
|---------------|------------------------|--------|
| VERIFY not loop physics | **Improved** — provisional assess + `should_reopen_loop` + VisualUx `NeedsVerification` | `turn_epilogue.rs`, `completion_assessor.rs` |
| `hard_stop_enabled: false` | **Fixed** — `HarnessConfig.guardrails_hard_stop` default **true** | `config.rs`, `harness_loop_policy.rs` |
| Mid-loop empty harness | **Fixed** — `build_turn_harness_snapshot`; ban `HarnessSnapshot::default()` at assess | `turn_epilogue.rs` HA-45 |
| Doctor JSONL blind | **Improved** — HA-44 tracing JSONL parse (`tool_starts`) | `harness_analyzer.rs` |
| Preview ops broken | **Improved** — `PreviewConfig` enabled + ports + inheritance/session fallback | `config.rs`, `task_class.rs` |
| Monolith `conversation.rs` | **Still open** — large LOC; prologue/epilogue/dispatch extracts in progress | W3 |
| No public benchmarks | **Unchanged** — local CI replay only | `harness-benchmark.yml` |
| W1 “unified ledger” end-to-end | **Partial → fixed (P0)** — enrich was dead until close-lies; now assess folds `GoalContract` via tool-result evidence | [proof/p0-one-assess-contract.md](./proof/p0-one-assess-contract.md) |
| `deny_default` sandbox | **Partial → fixed (P0)** — field was unused; now wired into `wrap_command` + doctor soft warn. Not meter L while default is soft | [proof/w4-trust-surface.md](./proof/w4-trust-surface.md) |
| `turn_dispatch_policy` ownership | **Partial → fixed (P1)** — was rename facade; body now in policy module | [proof/w3-dispatch-policy.md](./proof/w3-dispatch-policy.md) |

Net grade move: tool OS **A** (unchanged); task harness VisualUx **C− → B/B+**.
Sandbox scorecard: do **not** claim **L** until `deny_default: true` is the measured default.
