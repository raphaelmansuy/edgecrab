# 011 — Hermes Parity Map (Borrow List)

Concrete patterns from `hermes-agent` to close EdgeCrab harness gaps surfaced by homelab `games003` sessions.  
**Rule:** adopt mechanism, not file layout — one owner module per row (DRY with spec 008).

Cross-ref: [006-comparator-hermes-claude-pi.md](./006-comparator-hermes-claude-pi.md) · [008-improvement-plan.md](./008-improvement-plan.md).

---

## Map

| EdgeCrab gap (games003) | Hermes anchor | EdgeCrab target owner | Plan item |
|-------------------------|---------------|----------------------|-----------|
| Spill stub opaque | `tools/tool_result_storage.py` — `maybe_persist_tool_result`, `_build_persisted_message`, `<persisted-output>` tags | `artifact_spill.rs` | P0.1 ✅ |
| Turn budget blow-up | `enforce_turn_budget()` + `BudgetConfig` per-tool thresholds | `artifact_spill` + `read_tracker` | P2.5 |
| `read_file` spill loops | Pin `read_file` threshold = ∞ in budget config | `artifact_spill` / config | P2.5 |
| Localhost preview blocked | `url_safety._global_allow_private_urls` (global) + `browser_tool._navigation_session_key` (local sidecar) | `url_safety.rs` + `browser.rs` | P0.4 ✅ (port-scoped > global) |
| Dev server wrong port | `tui_gateway/server.py` `preview.restart` — inspect port owner, MIME recovery | `doctor.rs` + new `preview_hint` | P1.7 |
| `completed` ambiguous | `conversation_loop._turn_exit_reason` taxonomy | `completion_assessor.rs` | P1.1 ✅ |
| Turn explainer on partial exit | `run_agent._format_turn_completion_explanation()` | `turn_completion.rs` | P1.1 ✅ |
| Partial stream tool JSON | `PARTIAL_STREAM_STUB_ID`, `_get_continuation_prompt()` branches | `conversation.rs` | P1.6 |
| Copilot text-block tools | `copilot_acp_client._extract_tool_calls_from_text()` | provider layer (document only) | P1.4 ✅ |
| Todo lost after compress | `conversation_compression` todo_snapshot synthetic user msg | `compression.rs` | P1.5 |
| Memory 2200 cap hard fail | `memory_tool` char limits + `_drift_error()` + `.bak` | `tools/memory.rs` | P0.7 |
| Heredoc in terminal | `terminal_tool` schema + `approval.DANGEROUS_PATTERNS` for `<<` | `terminal.rs` + `recovery_catalog` | P0.8 |
| Spill via stdin not argv | `tool_result_storage._write_to_sandbox` stdin pipe | remote backends only today | P2.7 (defer) |
| Visual verify routing | `vision_routing.should_route_capture_through_aux_vision()` | `task_class` + `vision.rs` | P1.8 |
| Background job blindness | `process_registry.notify_on_complete` | `process_table` + TUI | P2.6 |
| Coding verify targets | `coding_context._VERIFY_TARGETS` (test/lint from Makefile) | `task_class` footer | P1.8 |

---

## Five high-leverage borrows (prioritized)

1. **Three-layer spill stack** — persist → turn budget → actionable stub (Hermes `tool_result_storage`); EdgeCrab has layer 1 partial; add aggregate turn cap + read_file pin.
2. **Continuation prompts by failure class** — network stall vs length cap vs dropped tool args (Hermes `conversation_loop._get_continuation_prompt`).
3. **Todo snapshot on compress** — only `pending`/`in_progress`, capped chars (Hermes `TodoStore.format_for_injection`).
4. **Memory limit recovery** — return `used_chars`, `max_chars`, suggest `session_search` / prune entries (Hermes `memory_tool` errors).
5. **Preview.restart semantics** — “verify URL serves intended app, not gateway UI” before claiming visual success (Hermes TUI gateway prompt).

---

## Explicit non-borrows

| Hermes pattern | Why not |
|----------------|---------|
| Global `allow_private_urls` | Too broad; EdgeCrab uses port allowlist (`security.preview`) |
| Auto-inject spill into context | Blows turn budget — recipe only (ADR, battle test S22) |
| Profile config fully isolated | **Revisit** — caused E15; merge unset security keys from global ([013 rank 1](./013-impact-ranked-backlog.md)) |
| Disable SSRF for dev | Security regression |
| Haiku-specific guardrails | Provider-agnostic harness required |
