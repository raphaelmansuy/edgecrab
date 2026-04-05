# Task Log — 2026-03-29-14-38 — clarify tool TUI wiring

## Actions
- Added `ClarifyRequest { question, response_tx: oneshot::Sender<String> }` struct to `edgecrab-tools/src/registry.rs`
- Added `clarify_tx: Option<UnboundedSender<ClarifyRequest>>` field to `ToolContext` and `test_context()` builder
- Rewrote `clarify.rs`: if `ctx.clarify_tx` is set, sends request and awaits answer (300s timeout); falls back to `[CLARIFY]` marker otherwise
- Added `Clarify { question, response_tx }` variant to `StreamEvent` in `agent.rs` (also replaced `#[derive(Debug)]` with manual `impl Debug`)
- Added `clarify_tx` field to `DispatchContext` in `conversation.rs`
- Updated `build_tool_context` signature to accept `clarify_tx` param; threaded into all call sites
- In `execute_loop`: create `mpsc::unbounded_channel::<ClarifyRequest>()`, spawn forwarder task → `StreamEvent::Clarify`
- Updated parallel tool spawn in `process_response` to clone `clarify_tx` into `DispatchContext`
- Updated `dispatch_single_tool` to pass `dctx.clarify_tx.clone()` to `build_tool_context`
- Fixed `delegate_task.rs` child context: added `clarify_tx: None`
- Added `AgentResponse::Clarify { question, response_tx }` variant to `app.rs`
- Added `clarify_pending_tx: Option<oneshot::Sender<String>>` field to `App` struct
- `check_responses()`: handles `AgentResponse::Clarify` → displays question, stores `response_tx`
- `process_input()`: if `clarify_pending_tx.is_some()`, routes user input to answer channel instead of new prompt
- Updated `e2e_copilot.rs` test to handle new `StreamEvent::Clarify` variant (ignore arm)

## Decisions
- Non-streaming paths (gateway, tests, batch) receive `None` for `clarify_tx` → tool falls back to `[CLARIFY] marker` string (safe degradation)
- 300-second timeout on clarify wait — unblocks if user closes TUI without answering
- Sub-agent children always get `clarify_tx: None` — interactive Q&A only at top-level

## Next steps
- Add `vision_analyze` tool (stub with OpenAI vision API)
- Run full `cargo test` on every future change to catch regressions early

## Lessons/Insights
- `StreamEvent` was `#[derive(Debug)]` which prevented adding `oneshot::Sender<String>` (no `Debug` impl); manual `impl Debug` resolves this cleanly
- All `ToolContext` struct literal initializations must be updated when new fields are added — use `cargo build` immediately to catch all sites
