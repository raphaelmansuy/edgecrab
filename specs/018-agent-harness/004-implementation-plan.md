# 004 — Implementation Plan (W1–W4)

## Sequence

```text
W0 specs (this tree) → W1 evidence+contracts → W2 cache/disclosure
                              ↓
                         W3 dispatch extract
                              ↓
                         W4 trust/surface honesty
```

## W1 — Unified evidence + GoalContract

- `GoalContract` in `edgecrab-types`
- Persist `contract_json` on `session_goals`
- `goal_judge` requires evidence when `verification` set
- `VerificationSummary` carries coding + VisualUx + contract signals
- `LifecycleEvent::PreVerify` emit (context only)

## W2 — Cache + indexed disclosure

- Hot set = 5; `write_file` hot
- Expand `input_examples` via materialize / tool_search only
- Doctor cache SLO surface
- CI: no `cached_system_prompt` mutation from materialize path

## W3 — Loop physics

- `turn_dispatch_policy` compositor owns storm → port → nav → theater → spill → guardrail
  (P1: body moved out of `turn_dispatch` rename facade)
- Move assess/reopen scaffolding into prologue/epilogue
- Shrink `conversation.rs` incrementally (non-goal for close-lies pass)

## W4 — Trust + surface

- Keep port-scoped preview (no Hermes private-URL breadth)
- `deny_default` threaded into `wrap_command` (default false = soft; doctor warns)
- Surface `ExitReason` / `user_summary` on TUI + `--json-stream` done
- Scorecard: sandbox not **L** while soft default remains

## Gates

See [proof/](./proof/) and `.github/workflows/harness-benchmark.yml` dry-solid checks.
