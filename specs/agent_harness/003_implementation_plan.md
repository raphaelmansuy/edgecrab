# Agent Harness Implementation Plan

Status: planned
Date: 2026-04-19
Related: [001_adr_unified_agent_harness.md](./001_adr_unified_agent_harness.md), [002_test_plan.md](./002_test_plan.md)

## Objective

Implement the Unified Agent Harness in EdgeCrab so that transport completion is no longer conflated with task completion.

This implementation targets the highest-value phases first:

1. explicit terminal-state types,
2. authoritative completion assessment,
3. task-ledger influence on completion,
4. streaming and hook parity,
5. structured model-side task-status signaling.

---

## Current code findings

The current runtime already has strong building blocks:

- typed stream transport in `edgecrab-core`
- gateway event processing and delivery separation
- a session-scoped todo store that survives compression
- subagent progress events and structured results

The main semantic gap is at the end of the loop:

- a non-empty response can still look like success even when the run stopped for a non-success reason
- `StreamEvent::Done` currently means transport is finished, not that the user’s task is verified complete
- gateway `agent:done` is too ambiguous for plugins and operators
- todo state does not currently influence the final completion decision

---

## Implementation scope

### Phase A — shared terminal-state contract

Add shared harness types:

- `CompletionDecision`
- `ExitReason`
- `VerificationSummary`
- `RunOutcome`

These become the single source of truth for the end state of a run.

### Phase B — completion assessor in the core loop

Add a dedicated `CompletionAssessor` that evaluates:

- interruption state
- budget exhaustion
- pending user-input/approval markers
- active and blocked todo counts
- verification evidence debt

Rules:

- `Completed` requires no unresolved work and no verification debt
- `BudgetExhausted` must never look completed
- `NeedsUserInput` and `Blocked` must be explicit
- an empty-or-partial stop with active work remains `Incomplete`

### Phase C — transport and subagent parity

Wire the same outcome model into:

- `ConversationResult`
- streaming events via `RunFinished`
- subagent results
- delegation progress finish status

### Phase D — hook clarity

Split hook semantics so plugins can distinguish:

- response delivered
- run finished
- task completed
- task blocked

Backward compatibility is preserved by continuing to emit `agent:done` as a compatibility alias for `agent:run_finished` during this release.

### Phase E — structured task-status tool and prompt guidance

Add a safe tool:

- `report_task_status(status, summary, evidence, remaining_steps)`

Guardrail:

- this tool never terminates execution by itself
- it only records a completion candidate for the harness to assess

Prompt guidance will explicitly instruct the model to use this tool for major milestones rather than self-certifying completion in prose.

---

## File-level plan

### Core runtime

- `crates/edgecrab-types/src/lib.rs`
- `crates/edgecrab-core/src/agent.rs`
- `crates/edgecrab-core/src/conversation.rs`
- `crates/edgecrab-core/src/sub_agent_runner.rs`
- `crates/edgecrab-core/src/lib.rs`

### Tooling

- `crates/edgecrab-tools/src/registry.rs`
- `crates/edgecrab-tools/src/toolsets.rs`
- `crates/edgecrab-tools/src/tools/todo.rs`
- `crates/edgecrab-tools/src/tools/mod.rs`
- `crates/edgecrab-tools/src/tools/report_task_status.rs`

### Gateway / integration

- `crates/edgecrab-gateway/src/event_processor.rs`
- `crates/edgecrab-gateway/src/run.rs`

---

## Edge cases and roadblocks

### 1. Budget exhaustion with fallback text

**Risk:** fallback text can appear user-friendly and accidentally get treated as a success.

**Mitigation:** set `exit_reason=BudgetExhausted` and `state=BudgetExhausted` even if fallback text exists.

### 2. Pending approval or clarification

**Risk:** the stream can finish after surfacing an interaction request.

**Mitigation:** detect these markers and classify the run as `NeedsUserInput` or `Blocked`, not completed.

### 3. Active todo items after the model stops

**Risk:** the response can sound final while the task ledger still shows in-progress work.

**Mitigation:** make task counts part of the completion context and final outcome.

### 4. Subagent mismatch

**Risk:** child runs can report success loosely while the parent uses stronger semantics.

**Mitigation:** propagate the same completion contract through `SubAgentResult` and delegation finish events.

### 5. Prompt-cache regression

**Risk:** mid-session prompt mutation would break Anthropic caching efficiency.

**Mitigation:** keep outcome assessment runtime-side; only static tool guidance is added to the system prompt.

### 6. Hook compatibility

**Risk:** external scripts may depend on `agent:done`.

**Mitigation:** keep `agent:done` as a compatibility event this release while also emitting the new explicit hooks.

---

## Verification plan

### Unit tests

- assessor returns `BudgetExhausted` when fallback text exists
- assessor rejects completion when active todos remain
- assessor returns `Blocked` for blocked todos or pending approval
- assessor returns `NeedsUserInput` for clarify markers
- run outcome preserves explicit exit reason

### Integration tests

- streaming path emits `RunFinished` before transport `Done`
- gateway surfaces blocked/input-needed statuses accurately
- subagent outcome rolls up without false completion
- compatibility hook plus new explicit hooks both fire as expected

### Release gates

Before considering the feature complete:

- `cargo fmt --all --check`
- `cargo clippy --workspace -- -D warnings`
- targeted harness tests pass
- `cargo test --workspace`
- `cargo build --workspace --release`

---

## Success definition

This plan is complete when EdgeCrab can truthfully answer all of the following without heuristic guessing:

1. What happened?
2. Why did the run stop?
3. Is any required work still pending?
4. Was the user’s task actually completed?
