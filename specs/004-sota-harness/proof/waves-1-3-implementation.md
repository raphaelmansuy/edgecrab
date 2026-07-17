# Implementation proof — SOTA harness Waves 1–3

## Wave 1

| Item | Proof |
|------|-------|
| Lifecycle hooks | `crates/edgecrab-core/src/lifecycle_hooks.rs` + wiring in `conversation.rs`, `Agent::emit_lifecycle` |
| `edgecrab install` | `cli_args.rs` `Command::Install`, `main.rs::run_install` |
| Parallel worktrees | `crates/edgecrab-tools/src/isolated_worktree.rs`, `delegate_task` + `sub_agent_runner` |
| Benchmark CI | `.github/workflows/harness-benchmark.yml`, `specs/004-sota-harness/proof/benchmark-ci.md` |

## Wave 2

| Item | Proof |
|------|-------|
| Plugin provider aliases | `model_router::list_plugin_provider_aliases` |
| Pareto / smart routing stats | `SmartRoutingStats`, `SessionSnapshot.routing_savings_note`, doctor check |
| OS sandbox | `crates/edgecrab-security/src/os_sandbox.rs`, `terminal.rs` + `AppConfigRef.os_sandbox_mode` |

## Wave 3

| Item | Proof |
|------|-------|
| Headless NDJSON | `--json-stream` in `cli_args.rs`, `main.rs` quiet path |
| Headless CI | `.github/workflows/headless-smoke.yml` |
| Kanban CLI | `edgecrab kanban list` |
| Gateway circuit breaker | `crates/edgecrab-gateway/src/circuit_breaker.rs` |
