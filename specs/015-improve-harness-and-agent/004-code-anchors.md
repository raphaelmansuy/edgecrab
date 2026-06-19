# 004 — Code Anchors (Code Is Law)

Current EdgeCrab implementation map for harness behavior. **No line numbers** — paths drift; grep symbols when needed.

---

## Loop orchestration

| Symbol / behavior | File | Notes |
|-------------------|------|-------|
| `execute_loop` | `crates/edgecrab-core/src/conversation.rs` | Main ReAct loop; **large** — extraction target |
| `dispatch_single_tool` / parallel JoinSet | same | Parallel path overlap via `claimed_paths` |
| `check_tool_argument_budget` | same ~5090 | Pre-dispatch reject; logs `rejecting tool call` |
| `maybe_spill` post-tool | same ~5799 | Per-result spill |
| `enforce_turn_budget` | same ~5409 | Turn-level spill |
| Non-streaming wait heartbeats | same ~4167 | `provider_llm` tracing |
| `sanitize_orphaned_tool_results` | same ~1587 | API hygiene |

---

## Completion & outcome

| Symbol | File | Notes |
|--------|------|-------|
| `CompletionPolicy` | `crates/edgecrab-core/src/completion_assessor.rs` | `DefaultCompletionPolicy` |
| `RunOutcome` | `crates/edgecrab-types` | `CompletionDecision` + `ExitReason` |
| `HarnessSnapshot` | `crates/edgecrab-tools` | Mutation debt · oracle failures |
| `assess_completion` | completion_assessor | Used at turn end |

**Gap:** Gateway/TUI may not surface full `RunOutcome` text uniformly — verify `app.rs` + `event_processor.rs`.

---

## Mutation & argument budget (DRY)

| Symbol | File | Notes |
|--------|------|-------|
| `check_tool_argument_budget` | `crates/edgecrab-tools/src/mutation_turn_policy.rs` | Single geometry formula |
| `LOCAL_TOOL_TURN_ABS_MAX_TOKENS` | same | 8192 → ~27852 B |
| `output_token_budget_for_tool_turn` | same | Shared with provider `max_tokens` |
| `recovery_catalog::tool_argument_budget_exceeded` | `crates/edgecrab-tools/src/recovery_catalog.rs` | LLM-facing error |

---

## Spill & result shaping

| Symbol | File | Notes |
|--------|------|-------|
| `maybe_spill` | `crates/edgecrab-tools/src/artifact_spill.rs` | Writes `.edgecrab-artifacts/{session}/` |
| `SpillOutcome` | same | Inline vs Spilled stub |
| `tool_result_spill` | `crates/edgecrab-core/src/tool_result_spill.rs` | Conversation integration |
| `summarize_tool_result_for_history` | `crates/edgecrab-core/src/tool_result_summary.rs` | `read ?` when args missing |
| `summarize_tool_result_preview` | same | TUI `[spilled — use read_file on artifact]` |
| `prune_tool_outputs` | `crates/edgecrab-core/src/compression.rs` | Structural pre-pass |

**Artifact root:** `{cwd}/.edgecrab-artifacts/{session_id}/` (workspace-relative in games003).

---

## Tool wire & schema mode

| Symbol | File | Notes |
|--------|------|-------|
| `TOOL_SEARCH_NAME` | `crates/edgecrab-tools/src/tool_schema_index.rs` | `tool_search` |
| `partition_schemas` | same | wire vs deferred |
| `is_deferred_not_on_wire` | same | Blocks dispatch until materialized |
| `schema_mode` | `crates/edgecrab-tools/src/schema_mode.rs` | compact/full/indexed |
| `CORE_TOOLS` / `INDEXED_HOT_TOOLS` | `crates/edgecrab-tools/src/toolsets.rs` | 56 core names |
| `build_wire_llm_definitions` | `crates/edgecrab-tools/src/registry.rs` | Per-iteration wire set |

---

## Provider policy (local + Copilot)

| Symbol | File | Notes |
|--------|------|-------|
| `local_provider_policy` | `crates/edgecrab-core/src/local_provider_policy.rs` | LM Studio / Ollama |
| `should_use_non_streaming_tool_turn` | same | Provider-specific |
| Non-streaming liveness strings | `crates/edgecrab-tools/src/tool_progress_tail.rs` | Copilot vs LM Studio |

---

## Progress transport

| Symbol | File | Notes |
|--------|------|-------|
| `StreamEvent` | `crates/edgecrab-core/src/agent.rs` | Token · ToolExec · ToolProgress · Done |
| `TurnActivityState` | `crates/edgecrab-cli/src/turn_activity.rs` | Shelf phases |
| `tool_display` | `crates/edgecrab-cli/src/tool_display.rs` | Column budgets · previews |
| `activity_shelf` | `crates/edgecrab-cli/src/activity_shelf.rs` | Live turn UI |
| Gateway mapping | `crates/edgecrab-gateway/src/event_processor.rs` | Delivery |

Contract matrix: [002-terminal-ux-ui/004](../002-terminal-ux-ui/004-stream-event-contract.md).

---

## Observability

| Symbol | File | Notes |
|--------|------|-------|
| `TARGET_HARNESS` | `crates/edgecrab-core/src/observability.rs` | `harness: api iteration` |
| `stream_observability` | `crates/edgecrab-core/src/stream_observability.rs` | tool start/complete |
| `otel_metrics` | `crates/edgecrab-core/src/otel_metrics.rs` | Metrics export |
| `apply_runtime_from_config` | observability.rs | Env layering |

---

## Security (do not bypass)

| Layer | Crate |
|-------|-------|
| Path jail | `edgecrab-security/src/path_safety.rs` |
| SSRF | `edgecrab-security/src/ssrf.rs` |
| Command scan | `edgecrab-security/src/command_scan.rs` |
| Read-before-write | `edgecrab-tools/src/read_tracker.rs` |

---

## Tests (harness law encoded)

| Test file | Covers |
|-----------|--------|
| `crates/edgecrab-core/tests/local_harness_geometry_e2e.rs` | LH geometry · max_arg |
| `crates/edgecrab-core/tests/local_prefill_prune_e2e.rs` | Prefill prune |
| `crates/edgecrab-core/tests/observability_e2e.rs` | Harness logging |
| `crates/edgecrab-tools/tests/provider_tracing_e2e.rs` | Provider spans |

---

## Symbol grep cheatsheet

```bash
# From repo root
rg "check_tool_argument_budget|maybe_spill|assess_completion" crates/
rg "nonstreaming_wait|composing" crates/edgecrab-tools/src/tool_progress_tail.rs
rg "RunOutcome|ExitReason" crates/edgecrab-types
```
