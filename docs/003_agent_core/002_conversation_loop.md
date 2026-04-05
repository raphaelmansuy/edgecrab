# Conversation Loop

Verified against `crates/edgecrab-core/src/conversation.rs`.

`Agent::execute_loop` is the core of the runtime. It resets budget and cancellation state for the turn, expands context, calls the provider, dispatches tools, and persists the result.

## Loop outline

```text
+-----------------------------+
| start turn                  |
+-----------------------------+
               |
               v
+-----------------------------+
| snapshot config and provider|
+-----------------------------+
               |
               v
+-----------------------------+
| resolve cwd and toolsets    |
+-----------------------------+
               |
               v
+-----------------------------+
| expand @context references  |
+-----------------------------+
               |
               v
+-----------------------------+
| build or reuse prompt       |
+-----------------------------+
               |
               v
+-----------------------------+
| maybe route to cheap model  |
+-----------------------------+
               |
               v
+-----------------------------+
| call provider               |
+-----------------------------+
               |
      +--------+--------+
      |                 |
      v                 v
+----------------+  +----------------+
| tool calls     |  | final text     |
+----------------+  +----------------+
      |                 |
      v                 v
+----------------+  +----------------+
| dispatch tools |  | persist and    |
| append results |  | return         |
+----------------+  +----------------+
      |
      v
    repeat
```

## Things the loop does beyond "call model, run tools"

- Builds `ToolContext` with session id, cwd, process table, provider handle, delegation runner, and optional gateway sender.
- Applies context compression when thresholds are exceeded.
- Converts tool failures into model-visible tool result payloads instead of crashing the turn.
- Tracks usage and estimated cost.
- Emits stream events for frontends.
- Triggers end-of-session reflection when enough tool calls were used.

## Termination conditions

- model returns plain assistant text
- iteration budget is exhausted
- user cancellation is observed
- unrecoverable provider or runtime failure occurs

## Practical invariant

The conversation history always stays in OpenAI-style message form:

- `system`
- `user`
- `assistant`
- `tool`

That matters because compression, persistence, and tool-call recovery all rely on that shape.
