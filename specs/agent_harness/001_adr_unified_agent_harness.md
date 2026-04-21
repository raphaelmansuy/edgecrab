# ADR-001: Unified Progress and Completion Harness for EdgeCrab

Status: proposed
Date: 2026-04-19

## Context

EdgeCrab already has a strong typed event transport for user-visible progress.

Evidence from the current implementation:

- the loop emits structured progress events from [../../crates/edgecrab-core/src/conversation.rs](../../crates/edgecrab-core/src/conversation.rs)
- the gateway maps those events to delivery behavior in [../../crates/edgecrab-gateway/src/event_processor.rs](../../crates/edgecrab-gateway/src/event_processor.rs)
- session-level planning survives compression via [../../crates/edgecrab-tools/src/tools/todo.rs](../../crates/edgecrab-tools/src/tools/todo.rs)

This is already better separated than Hermes Agent, where callbacks, CLI printing, and loop logic are more tightly coupled.

However, the study also found a critical semantic gap:

EdgeCrab is good at knowing when a **stream** is done, but not yet rigorous enough at knowing when a **task** is truly done.

Today, the meaning of “done” is spread across multiple surfaces:

- `StreamEvent::Done` means stream transport is complete
- gateway `agent:done` means a response was delivered
- a non-empty `final_response` often implies the loop is complete
- the session-level `completed` flag is currently derived too loosely

These are related, but they are not equivalent.

That ambiguity makes it easy to misreport completion for partial, budget-exhausted, or blocked work.

---

## Problem statement

From first principles, an agent harness must answer all of the following explicitly:

1. What is the agent doing right now?
2. Is the agent still making forward progress?
3. What work remains?
4. Has the requested task actually been completed?
5. If the run stopped, why did it stop?

The current EdgeCrab harness answers question 1 well, but only partially answers questions 2–5.

### Specific weaknesses discovered

1. **Transport done is conflated with task done.**
2. **Completion is inferred from response presence rather than verified evidence.**
3. **Todo state exists, but it is not authoritative for completion.**
4. **Gateway, TUI, ACP, and hooks do not share a single terminal-state contract.**
5. **Subagents and top-level agents do not expose the exact same run-outcome semantics.**

---

## Decision

EdgeCrab will introduce a **Unified Agent Harness** with four dedicated responsibilities:

1. **LoopEngine** — runs the ReAct/tool loop.
2. **ProgressBus** — emits normalized progress/state events.
3. **TaskLedger** — tracks explicit work items and their status.
4. **CompletionAssessor** — decides whether the task is complete, blocked, needs input, or failed.

### Core rule

EdgeCrab must no longer treat “non-empty final response” as sufficient proof of task completion.

Instead, task completion must be decided by an explicit **completion policy** that inspects:

- terminal stop reason
- todo/task state
- pending approvals or clarifications
- unfinished background work or child tasks
- verification evidence when the request involved execution, file changes, or testing

---

## Why this is the right abstraction

### First-principles rationale

A trustworthy agent must separate:

- **communication state** — what the user sees
- **execution state** — what the loop is doing
- **task state** — what work remains
- **terminal state** — why the run stopped

If these are merged, the system will inevitably misclassify partial work as success.

### DRY rationale

A single run-outcome contract removes repeated, slightly different notions of done across:

- CLI
- gateway
- ACP
- plugins/hooks
- session persistence
- future SDK integrations

### SOLID rationale

- **SRP:** progress emission, task tracking, and completion assessment become separate units.
- **OCP:** new UIs subscribe to the same event contract without changing loop logic.
- **LSP:** parent agents and subagents return the same outcome model.
- **ISP:** UIs depend only on a `ProgressSink`, not the entire runtime.
- **DIP:** the loop depends on `CompletionPolicy` and `ProgressSink` traits, not specific concrete consumers.

---

## Proposed architecture

## 1. ProgressBus

Keep the existing event-driven model, but normalize it into a single harness event surface.

```rust
pub enum HarnessEvent {
    RunStarted { session_id: String },
    StepStarted { kind: StepKind, label: String },
    StepProgress { kind: StepKind, label: String, detail: String },
    StepFinished { kind: StepKind, label: String, ok: bool, evidence: Option<String> },
    TaskLedgerUpdated { active: usize, completed: usize, blocked: usize },
    NeedsUserInput { question: String },
    NeedsApproval { action: String },
    CompletionCandidate { summary: String },
    RunFinished { outcome: RunOutcome },
}
```

### Notes

- This reuses the spirit of the current `StreamEvent` model rather than replacing it.
- Existing `StreamEvent` values can be adapted into `HarnessEvent` through a thin mapping layer.
- `RunFinished` becomes the single authoritative terminal event.

---

## 2. TaskLedger

Promote the existing todo store into a first-class harness concept.

The ledger should track:

- task id
- title
- owner (main agent or subagent)
- status: not-started / in-progress / completed / blocked / cancelled
- evidence summary
- last update time

### Key rule

The ledger is informative today. It should become **advisory for completion** tomorrow.

That means:

- if active tasks remain, the run should not report completed
- if all tasks are terminal but verification is missing, the run should become `NeedsVerification`
- if a task is blocked on user input or approval, the outcome should say so explicitly

---

## 3. CompletionAssessor

Add a dedicated completion policy component.

```rust
pub trait CompletionPolicy: Send + Sync {
    fn assess(&self, ctx: &CompletionContext) -> CompletionDecision;
}

pub struct CompletionContext {
    pub final_response: String,
    pub interrupted: bool,
    pub budget_exhausted: bool,
    pub pending_approval: bool,
    pub pending_clarification: bool,
    pub active_todos: usize,
    pub blocked_todos: usize,
    pub child_runs_in_flight: usize,
    pub verification: VerificationSummary,
}
```

### Decision enum

```rust
pub enum CompletionDecision {
    Completed,
    NeedsUserInput,
    Blocked,
    BudgetExhausted,
    Interrupted,
    Failed,
    Incomplete,
}
```

### Policy rules

- **Completed** only if there is a final response and no pending work, no pending approval, and no verification debt.
- **NeedsUserInput** if the run stopped waiting on clarification.
- **Blocked** if approvals or external dependencies remain unresolved.
- **BudgetExhausted** if the loop hit its cap before satisfying completion criteria.
- **Interrupted** if the user or runtime cancelled execution.
- **Incomplete** if the model stopped talking but the task ledger still shows unfinished work.

---

## 4. RunOutcome contract

All caller-facing surfaces should consume the same terminal result type.

```rust
pub struct RunOutcome {
    pub state: CompletionDecision,
    pub exit_reason: ExitReason,
    pub user_summary: String,
    pub evidence: Vec<String>,
    pub verification: VerificationSummary,
    pub active_tasks: usize,
    pub blocked_tasks: usize,
}
```

### ExitReason must be explicit

```rust
pub enum ExitReason {
    ModelReturnedFinalText,
    NoMoreToolCalls,
    BudgetExhausted,
    Interrupted,
    AwaitingClarification,
    AwaitingApproval,
    ToolFailure,
    ModelError,
}
```

This is the strongest lesson from Claude Code: terminal reasons should be first-class, not implicit.

---

## Dedicated done tool: decision

## We should not add a naive “done” tool that ends the run immediately

That would let the model self-certify completion too early.

## We should add a **structured status signal** instead

Recommended tool:

```rust
report_task_status(
  status: in_progress | blocked | completed,
  summary: String,
  evidence: String[],
  remaining_steps: String[]
)
```

### Important guardrail

Calling `report_task_status(status=completed)` does **not** terminate the run by itself.

It only creates a **completion candidate**. The harness still runs the `CompletionAssessor` before emitting the final outcome.

This preserves safety while giving the model an explicit way to tell the system what it believes just happened.

---

## Hook changes

Split the current semantics into clearer lifecycle hooks:

- `agent:response_delivered`
- `agent:run_finished`
- `agent:task_completed`
- `agent:task_blocked`

### Why

Today, `agent:done` is too ambiguous.

A plugin should be able to react differently when:

- the answer was delivered,
- the run finished due to cancellation,
- the task truly completed,
- the task needs user input.

---

## UI and operator behavior

### CLI / TUI

The TUI should continue showing:

- current tool / subagent activity
- context pressure
- explicit completion banner with terminal reason

Example terminal summaries:

- Completed — request satisfied and verified
- Blocked — waiting for your approval
- Needs input — clarification requested
- Stopped — iteration budget exhausted before completion

### Gateway / messaging

The gateway should avoid emitting a success-looking “done” message when the outcome is actually blocked or incomplete.

Instead, it should send the terminal status summary derived from `RunOutcome`.

### ACP / SDK

ACP clients should receive the same normalized finish event and should not need to infer outcome from raw token transport.

---

## Migration plan

### Phase 1 — Types only

- add `ExitReason`, `CompletionDecision`, and `RunOutcome`
- keep existing behavior, but populate these fields conservatively
- emit `RunFinished`

### Phase 2 — Assessment wiring

- implement `CompletionAssessor`
- stop computing `completed` from final response presence alone
- treat budget-exhausted fallback as `BudgetExhausted`, not success

### Phase 3 — TaskLedger integration

- wire `TodoStore` into the assessor
- include blocked / active counts in final outcomes
- expose structured task summaries in UI and hooks

### Phase 4 — Optional model status tool

- add `report_task_status`
- store evidence and remaining steps
- keep harness-side verification authoritative

---

## Consequences

### Positive

- clearer user trust model
- fewer false-positive completions
- cleaner plugin semantics
- stronger parity between main agent, subagents, CLI, gateway, and ACP
- better basis for future SDK exposure

### Negative / costs

- more explicit state to maintain
- some existing hooks and tests will need updates
- slight increase in loop bookkeeping

These costs are acceptable because correctness and operator trust are more important than shaving a few lines from the runtime.

---

## Non-goals

This ADR does **not** propose:

- rewriting the existing event bus
- removing streaming or subagent support
- turning the harness into a workflow engine
- forcing all tasks to use a todo list

The goal is semantic clarity, not unnecessary complexity.

---

## Acceptance criteria

This ADR is considered implemented when all of the following are true:

1. A stream finishing no longer implies task completion.
2. Budget exhaustion cannot produce `completed = true`.
3. Top-level and subagent runs emit the same terminal-state contract.
4. UI surfaces can show terminal reasons without custom inference.
5. Hooks distinguish response delivery from verified task completion.
6. Todo/task state influences completion assessment.

---

## Recommended next step

Implement Phase 1 and Phase 2 first.

That yields the highest trust improvement with the lowest architectural risk.
