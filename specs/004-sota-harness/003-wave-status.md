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

## Wave 2 — Economics + trust

| ID   | Workstream                           | Status | Proof / notes                       |
| ------| --------------------------------------| --------| -------------------------------------|
| W2-a | Finish 009 pluggable providers       | 🟡      | `list_plugin_provider_aliases` stub |
| W2-b | 029 Pareto / smart router polish     | ✅      | `SmartRoutingStats` + doctor        |
| W2-c | Token-efficiency UX                  | 🟡      | doctor SLO + `/context budget`      |
| W2-d | Optional OS sandbox (Seatbelt/bwrap) | ✅      | `os_sandbox.rs`                     |

## Wave 3 — Surfaces

| ID | Workstream | Status | Proof / notes |
|----|------------|--------|---------------|
| W3-a | Headless JSON stream + GH Action | ✅ | `--json-stream` + `headless-smoke.yml` |
| W3-b | Kanban UI depth | 🟡 | `edgecrab kanban list` + dashboard HTML |
| W3-c | Gateway Tier B hygiene | 🟡 | `circuit_breaker.rs` stub |

## Out of scope (do not dilute)

Spotify/Feishu doc tools, Electron desktop, Codex app-server clone,
full Hermes 100+ Python plugin port.
