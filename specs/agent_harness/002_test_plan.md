# Agent Harness Test Plan

Status: proposed
Related ADR: [001_adr_unified_agent_harness.md](./001_adr_unified_agent_harness.md)

## Objective

Verify that EdgeCrab reports progress and completion truthfully across CLI, gateway, ACP, and subagent paths.

---

## Test categories

## 1. Progress transport parity

### Goal

Ensure all frontends observe the same core lifecycle.

### Cases

- tool start emits a running event
- tool progress emits an update event
- tool completion emits a finished event
- subagent start / update / finish are surfaced consistently
- stream finalization emits one authoritative final event

### Evidence

- no duplicate finish event
- no missing finish event
- no success event on error path

---

## 2. Completion assessor correctness

### Goal: completion assessor correctness

Ensure terminal state is classified correctly.

### Cases: completion assessor correctness

1. **Normal success**
   - no pending tasks
   - no approval/clarification pending
   - final response present
   - expected outcome: `Completed`

2. **Budget exhausted**
   - iteration limit hit
   - fallback summary returned
   - expected outcome: `BudgetExhausted`
   - must not be marked complete

3. **Interrupted**
   - cancel token fired
   - expected outcome: `Interrupted`

4. **Needs clarification**
   - clarify request still open
   - expected outcome: `NeedsUserInput`

5. **Awaiting approval**
   - dangerous command approval outstanding
   - expected outcome: `Blocked`

6. **Unfinished todo items**
   - active task ledger entries remain
   - expected outcome: `Incomplete` or `Blocked`

---

## 3. TaskLedger integration

### Goal: task ledger integration

Ensure task tracking influences completion semantics.

### Cases: task ledger integration

- all tasks completed -> eligible for completion
- one task still in progress -> not eligible
- one task blocked -> final state is blocked
- all tasks cancelled -> not treated as successful completion unless response explicitly explains cancellation outcome

---

## 4. Hook semantics

### Goal: hook semantics

Verify hook events are unambiguous.

### Cases: hook semantics

- response delivered triggers `agent:response_delivered`
- completed run triggers `agent:task_completed`
- blocked run triggers `agent:task_blocked`
- interrupted run triggers `agent:run_finished` with interrupted reason

### Assertion

No consumer should need to guess whether “done” meant transport done or task done.

---

## 5. Regression cases from this study

### Regression A — false completion on budget exhaustion

**Setup:** force iteration limit exhaustion and allow fallback text generation.

**Expected:**

- non-empty fallback response may be shown to user
- run outcome is still `BudgetExhausted`
- session and hook metadata must not claim success

### Regression B — transport done but work blocked

**Setup:** stream ends after approval request or clarification request.

**Expected:**

- transport completion event occurs
- run outcome is `Blocked` or `NeedsUserInput`
- no “task completed” hook fires

### Regression C — subagent parity

**Setup:** delegated task succeeds and another delegated task blocks.

**Expected:**

- both child outcomes are normalized
- parent summary aggregates them correctly
- parent does not report success if one required child remains blocked

---

## Suggested implementation tests

### Unit tests

- `completion_assessor_returns_budget_exhausted_when_fallback_text_exists`
- `completion_assessor_rejects_completed_when_active_todos_exist`
- `completion_assessor_returns_blocked_for_pending_approval`
- `run_outcome_preserves_exit_reason`

### Integration tests

- CLI receives progress + final outcome
- gateway receives progress + final outcome
- ACP receives progress + final outcome
- subagent outcomes roll up into parent outcome

### Golden output tests

Check user-visible status summaries such as:

- Completed — request satisfied and verified
- Blocked — waiting for approval
- Needs input — clarification requested
- Stopped — iteration budget exhausted

---

## Success metric

A run is only reported as complete when the harness has evidence that the requested task is actually complete, not merely because the model stopped talking.
