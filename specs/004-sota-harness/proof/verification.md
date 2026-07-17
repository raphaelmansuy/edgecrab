# Verification — DRY / SOLID / E2E / First Principles

Date: 2026-07-17  
Status: **verified** (local pass)

## First principles (still hold)

| Meter | Invariant | Evidence |
|-------|-----------|----------|
| Task success | Single ReAct orchestrator | `conversation.rs` `execute_loop` unchanged |
| Horizon | Goals/steers/footers → messages, not system | No Wave change to cache zones |
| Cost / turn | 3-tier cache + smart routing stats | `context_cache_efficiency_e2e` + `SmartRoutingStats` |
| Prefill | Local stale-stream policy preserved | Prior Sprint G |
| Trust | Unified threat SoT + tool delimiters + OS sandbox | `threat_patterns`, `prepare_tool_result_body`, `os_sandbox` |
| Surface | One binary: TUI + gateway + ACP + headless | `--json-stream`, hooks, install |

**Cache law:** Lifecycle hooks only `emit_global(...)` at turn/tool/compress boundaries; they never assign `session.cached_system_prompt`. Goals/steers still inject into `messages`.

## DRY / SOLID fixes in this verification pass

| Issue | Fix |
|-------|-----|
| Duplicate `hooks_dir` (core vs gateway) | `lifecycle_hooks::hooks_home()` is SoT; gateway delegates |
| Tool-result wrap logic duplicated | `prepare_tool_result_body` + `tool_output_delimiters_enabled` |
| OS sandbox foreground-only | Same helper on background terminal path |
| Dead `Off` match arm | Collapsed in `wrap_command` |
| `ScanContext::ToolOutput` unwired | Scanned in `prepare_tool_result_body` (warn, never suppress) |

Remaining debt (documented, not blocking): skills_guard / plugins/guard local Severity enums; CLI vs tools git worktree primitives; Seatbelt allow-default profile.

## E2E results (this pass)

| Suite | Result |
|-------|--------|
| `edgecrab-core --test sota_harness_wave_e2e` (8) | pass |
| `edgecrab-security --lib threat_patterns` | pass |
| CLI: `parse_top_level_install_skill_pack`, `parse_json_stream_global_flag`, `json_stream_emits_done_and_token_kinds` | pass |

```bash
cargo test -p edgecrab-core --test sota_harness_wave_e2e
cargo test -p edgecrab-security --lib threat_patterns
cargo test -p edgecrab-cli --bin edgecrab -- \
  parse_top_level_install_skill_pack parse_json_stream_global_flag json_stream_emits_done_and_token_kinds
```

CI: `.github/workflows/harness-benchmark.yml` includes `sota_harness_wave_e2e` + `harness_failover_matrix`.
