# 003 — Wave Status

Update statuses as work ships (`○` open · `🟡` partial · `✅` done).

## Wave 1 — SOTA core

| ID | Workstream | Status | Proof / notes |
|----|------------|--------|---------------|
| W1-0 | Living scoreboard (`specs/004-sota-harness/`) | ✅ | This tree |
| W1-d | Promptware / brainworm (gap 031) | ✅ | `threat_patterns` + proof |
| W1-a | Lifecycle hook events in core loop | ✅ | `lifecycle_hooks.rs` |
| W1-b | `edgecrab install` skill-pack UX | ✅ | `Command::Install` |
| W1-c | Parallel worktree subagent orchestrator | ✅ | `isolated_worktree` + `delegate_task` |
| W1-e | Public harness benchmark CI | ✅ | `harness-benchmark.yml` |
| W1-f | Tool-call closure on PartialAbort | ✅ | `proof/tool-call-closure-invariant.md` — `ExitReason::InvalidToolBudget` |

## Wave 2 — Economics + trust

| ID   | Workstream                           | Status | Proof / notes                       |
| ------| --------------------------------------| --------| -------------------------------------|
| W2-a | Finish 009 pluggable providers       | 🟡      | `list_plugin_provider_aliases` stub |
| W2-b | 029 Pareto / smart router polish     | ✅      | `SmartRoutingStats` + doctor        |
| W2-c | Token-efficiency UX                  | 🟡      | doctor SLO + `/context budget`; schema floor: `proof/indexed-schema-disclosure.md` + `proof/tool-progressive-load.md` + `proof/progressive-disclosure-decisions.md` + `proof/create-path-disclosure.md` (hot `write_file`; game001 thrash closed) + `proof/visual-preview-lifecycle.md` (game002 serve→perceive; storm exempt + no port shopping) |
| W2-d | Optional OS sandbox (Seatbelt/bwrap) | ✅      | `os_sandbox.rs`                     |
| W2-e | Harness-run contracts + verify-on-stop (018 Jul) | ✅ | `contract_verify` + `HarnessConfig.verify_on_stop`; proof `../018-agent-harness/proof/p2-harness-run-contract.md` |
| W2-f | Local harness scorecard CI | ✅ | `harness-benchmark.yml` `LOCAL_HARNESS_SCORECARD` |

## Wave 3 — Surfaces

| ID | Workstream | Status | Proof / notes |
|----|------------|--------|---------------|
| W3-a | Headless JSON stream + GH Action | ✅ | `--json-stream` + `headless-smoke.yml` |
| W3-b | Kanban UI depth | 🟡 | `edgecrab kanban list` + dashboard HTML |
| W3-c | Gateway Tier B hygiene | 🟡 | `circuit_breaker.rs` stub |

## Local harness scorecard (July 2026)

CI job prints `LOCAL_HARNESS_SCORECARD passed=N failed=M total=T` aggregating:

- `dry_solid_harness_gates`
- `harness_games003_replay`
- `visual_preview_lifecycle_e2e`
- `goals_ralph_loop` (`contract_*`)
- `wave_a_*` / `wave_b_*` / `contract_verify` unit tests

Public Terminal-Bench/Harbor remains deferred until a held-out set exists.
Sandbox Trust stays **P** (`deny_default` soft default + doctor probe).

## Out of scope (do not dilute)

Spotify/Feishu doc tools, Electron desktop, Codex app-server clone,
full Hermes 100+ Python plugin port.
