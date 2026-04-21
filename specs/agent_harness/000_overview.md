# Agent Harness Study — EdgeCrab vs Hermes Agent vs Claude Code

Date: 2026-04-19
Status: draft study
Owner: architecture

## Goal

Study three agent runtimes through three practical questions:

1. How does the runtime communicate progress to the user while the loop is running?
2. Does the runtime expose a dedicated task-done signal/tool/hook?
3. How does the runtime decide that work is done and stop safely?

The outcome of this study is an ADR recommendation for improving the EdgeCrab harness.

---

## Evidence base

### EdgeCrab

- Progress/event transport: [../../crates/edgecrab-gateway/src/event_processor.rs](../../crates/edgecrab-gateway/src/event_processor.rs)
- Core loop and completion logic: [../../crates/edgecrab-core/src/conversation.rs](../../crates/edgecrab-core/src/conversation.rs)
- Agent surface and session todo store: [../../crates/edgecrab-core/src/agent.rs](../../crates/edgecrab-core/src/agent.rs)
- Todo persistence tool: [../../crates/edgecrab-tools/src/tools/todo.rs](../../crates/edgecrab-tools/src/tools/todo.rs)

### Hermes Agent

- Main loop and callbacks: [../../../hermes-agent/run_agent.py](../../../hermes-agent/run_agent.py)
- Gateway stream consumer: [../../../hermes-agent/gateway/stream_consumer.py](../../../hermes-agent/gateway/stream_consumer.py)

### Claude Code analysis repo

- Main query loop: [../../../claude-code-analysis/src/query.ts](../../../claude-code-analysis/src/query.ts)
- Stop hooks and TaskCompleted hooks: [../../../claude-code-analysis/src/query/stopHooks.ts](../../../claude-code-analysis/src/query/stopHooks.ts)
- Task lifecycle model: [../../../claude-code-analysis/src/Task.ts](../../../claude-code-analysis/src/Task.ts)

---

## Comparative findings

| Question | EdgeCrab | Hermes Agent | Claude Code |
| --- | --- | --- | --- |
| Progress communication | Strong typed event bus: reasoning, tool start, tool progress, tool done, subagent start/finish, context pressure, stream done | Rich callback surface and CLI spinner; also gateway stream consumer for progressive editing | Generator-driven progress messages, tool summaries, hook progress, and task lifecycle updates |
| Dedicated task-done tool | No dedicated done tool; there is a transport-level `StreamEvent::Done` and a gateway `agent:done` hook after delivery | No dedicated done tool; done is inferred from final assistant response | No dedicated done tool, but it has explicit task statuses plus `TaskCompleted` hooks |
| Stop / done assessment | Mostly “no more tool calls” plus non-empty final response; task completion semantics are weak | Mostly “no more tool calls” plus final response presence and iteration limit checks | Strongest of the three: explicit terminal reasons and hook-based stop/continuation control |

---

## Answer to the study questions

## 1. How do they communicate loop progress?

### EdgeCrab progress model

EdgeCrab already has the cleanest low-level architecture for progress transport.

It uses a typed stream event model that distinguishes:

- reasoning updates
- tool execution start
- tool progress
- tool completion
- subagent start / reasoning / tool execution / finish
- context-pressure warnings
- final stream completion

This is strong from a first-principles standpoint because transport concerns are separated from the core loop. The gateway event processor and TUI only subscribe to events; they do not own the loop itself.

### Hermes progress model

Hermes exposes progress via a broad callback interface and user-facing spinner/status printing.

This is flexible but less cohesive. The loop, UI side effects, and completion logic live together inside one large runtime, which makes the system harder to reason about and evolve.

### Claude Code progress model

Claude Code uses a highly structured async generator model. Progress is streamed as message updates, progress messages, tool summaries, and hook events.

This model is excellent for UI composition because the consumer can render granular updates without peeking into internal loop state.

---

## 2. Do they have a task-done tool?

### EdgeCrab done signaling

No dedicated task-done tool. It has:

- `manage_todo_list` for planning/tracking
- `StreamEvent::Done` for transport completion
- `agent:done` hook in the gateway after delivery

Important: those do **not** mean “the user’s request has been fully completed and verified.” They mainly mean “the run/stream finished” or “the answer was delivered.”

### Hermes done signaling

No dedicated done tool. It also has a todo store, but completion is still inferred rather than explicitly assessed.

### Claude Code done signaling

No user-facing done tool either. Instead, it uses:

- explicit task states
- stop hooks
- `TaskCompleted` hooks
- a terminal reason enum returned by the loop

This is closer to the right abstraction: a completion contract rather than a naive done button.

---

## 3. How do they stop and decide work is done?

### EdgeCrab stop semantics

The loop stops when:

- the model returns a final text response with no more tool work
- the loop is interrupted
- the budget is exhausted
- an error aborts execution

However, current completion semantics are too loose. In the current code, `completed` is derived from:

- not interrupted
- non-empty final response

That means a budget-exhausted fallback message can still look “completed” at the session end hook level.

### Hermes stop semantics

Hermes behaves similarly. It is better instrumented than a minimal loop, but completion is still largely inferred from the presence of final output and iteration status.

### Claude Code stop semantics

Claude Code has the strongest completion contract. It returns explicit terminal reasons such as:

- completed
- stop hook prevented continuation
- prompt too long
- aborted streaming
- model error
- blocking limit

That is the key insight from this study.

---

## First-principles analysis

If we strip the problem down to first principles, an agent harness must answer four separate questions:

1. What is happening now?
2. What remains to be done?
3. What evidence proves the task is done?
4. Why did the run stop?

A robust harness must therefore separate four concepts that are currently too easy to conflate:

- stream finished
- model produced text
- task is actually complete
- run stopped for a known reason

That separation is the main architectural improvement EdgeCrab should make.

---

## DRY and SOLID assessment

### DRY

EdgeCrab already avoids some duplication by using a shared event model. That is good.

But completion meaning is still repeated in several places with slightly different interpretations:

- the loop returns a final response
- the session hook receives `completed`
- the gateway emits `agent:done`
- the UI receives `StreamEvent::Done`

Those are related, but they are not the same concept.

### SOLID

- **Single Responsibility:** progress transport is well separated, but completion assessment is not yet isolated.
- **Open/Closed:** event-based progress is extensible; completion policy is not.
- **Liskov Substitution:** main agents and subagents should produce the same run outcome contract.
- **Interface Segregation:** UI layers should depend on a tiny progress sink interface, not full loop internals.
- **Dependency Inversion:** the core loop should depend on `CompletionPolicy` and `ProgressSink` traits, not concrete gateway/TUI logic.

---

## Main conclusion

EdgeCrab is already strong on **progress transport**.

The biggest gap is **task completion semantics**.

Therefore the recommendation is:

1. keep the typed event bus,
2. formalize a unified run outcome contract,
3. add a real completion assessor,
4. distinguish response delivery from verified task completion.

The detailed decision is captured in [001_adr_unified_agent_harness.md](./001_adr_unified_agent_harness.md).
