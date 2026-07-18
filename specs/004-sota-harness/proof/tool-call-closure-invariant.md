# Proof — Tool-Call Closure Invariant

**Date:** 2026-07-17  
**Meter:** Task success + Trust (structured harness facts)  
**Principle:** Completion depends on structured facts, not NLP heuristics.

## Invariant

For every `assistant.tool_calls[i].id`, a matching `Role::Tool` message with that
`tool_call_id` must exist before the ReAct loop exits for any reason.

## Bug (pre-fix)

On unknown-tool strike 3 (`MAX_INVALID_TOOL_RETRIES`), `process_response` returned
`LoopAction::PartialAbort` **after** pushing the assistant message with
`tool_calls` but **before** appending tool results.

Observable failure (MSFT / `quick_stock_quote` session):

| Signal | Pre-fix |
|--------|----------|
| `count_unanswered_tool_calls` | 1 |
| Harness gate | Incomplete — tool call(s) ended without results |
| Operator text | Abort reason + false history-inconsistency warning |

A naive fix that only closed tool results would clear the unanswered gate and
risk assess flipping to **Completed** (non-empty `final_response` + clean
harness).

## Fix

1. **Closure:** append structured `unknown_tool_error_response` for every call in
   the batch *before* returning `PartialAbort` (same path as strikes 1–2).
2. **Typed exit:** `ExitReason::InvalidToolBudget` +
   `CompletionContext.invalid_tool_budget_exhausted` → `CompletionDecision::Failed`
   (assessed before harness-incomplete / Completed).
3. **Operator:** skip pending-tool warning when `exit_reason == InvalidToolBudget`;
   dedupe headline / operator_hint lines in `format_turn_completion_explanation`.
4. **Terminal:** `should_reopen_loop` returns false for `InvalidToolBudget`.

## Outcome contract (MSFT-class path)

| Signal | Post-fix |
|--------|-----------|
| `unanswered_tool_calls` | 0 |
| `CompletionDecision` | `Failed` |
| `ExitReason` | `InvalidToolBudget` |
| Pending-tool warning | absent |

## Tests

- `completion_assessor::invalid_tool_budget_exhausted_is_failed_not_completed`
- `completion_assessor::closed_unknown_tool_results_do_not_trip_unanswered_gate`
- `completion_assessor::ha51_unanswered_tool_calls_incomplete` (orphan still Incomplete)
- `turn_completion::invalid_tool_budget_skips_pending_tool_warning`
- `turn_completion::dedupes_headline_and_operator_hint_when_summary_already_has_them`
- `turn_epilogue::invalid_tool_budget_does_not_reopen_loop`
- `tool_call_pipeline::tcp06_classify_unknown_batch_abort_on_third_strike` (unchanged)

## Follow-up (2026-07-17) — registry dictionary recovery

First principle: the tool registry is truth. Invented names do not get
`RetrySameCall`; they get progressive disclosure via the existing dictionary.

| Fix | Detail |
|-----|--------|
| Discovery | `CallToolFirst(tool_search)` with `query` from invent name tokens |
| Candidates | BM25 over full registry catalog + CORE anchors (`build_registry_catalog`) |
| No second tool | Reuses `tool_search` — no parallel `tool_dictionary` |
| Typed headline | `ExitReason::InvalidToolBudget` → “invalid tool call retry budget exhausted” |
| Operator notice | `format_operator_notice` — TUI must not re-prefix enriched `user_summary` |

E2E: `crates/edgecrab-core/tests/unknown_tool_recovery_e2e.rs`

## Out of scope

No finance tool, no query-type heuristics, no fuzzy-cutoff changes.
