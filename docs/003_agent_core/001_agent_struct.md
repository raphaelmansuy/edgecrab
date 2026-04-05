# Agent Structure

Verified against `crates/edgecrab-core/src/agent.rs`.

`Agent` is the session-scoped runtime object. It owns the mutable parts of a conversation and the shared resources that tools need while that conversation is active.

## High-level shape

```text
Agent
  -> config (RwLock<AgentConfig>)
  -> provider (RwLock<Arc<dyn LLMProvider>>)
  -> tool registry
  -> optional SessionDb
  -> optional gateway sender
  -> ProcessTable
  -> session state
  -> iteration budget
  -> cancellation tokens
  -> TodoStore
```

## Why the struct looks this way

- `RwLock` is used for config and provider because `/model` and related runtime changes can swap them during the lifetime of the agent.
- `ProcessTable` and `TodoStore` are owned by the agent so they survive multiple tool calls and context compression inside one session.
- `gateway_sender` is optional because plain CLI and cron runs do not need cross-platform delivery.

## `AgentConfig` highlights

Important fields in the current struct:

- model selection and max iteration control
- enabled and disabled toolsets
- platform and API mode
- save-skip flags such as `save_trajectories`, `skip_context_files`, `skip_memory`
- delegation limits
- browser, checkpoint, auxiliary, terminal backend, and filesystem policy settings

## Session state

`SessionState` keeps:

- the current `session_id`
- message history
- cached system prompt
- token and tool-call counters
- per-session usage totals

The cached system prompt is deliberate. It prevents rebuilding prompt context on every turn.
