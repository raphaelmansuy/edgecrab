# Mission Steering — First Principles

## First Principle 1: Loop Safety

> *"Never mutate shared conversation state from an external thread without a
> synchronisation primitive."*

The ReAct loop runs in an `async` Tokio context. The TUI event loop runs in
a separate Tokio task. Any steering message must cross this boundary safely.

**Choice:** `tokio::sync::mpsc::UnboundedSender<SteeringEvent>` held by the TUI.
The conversation loop holds the `Receiver` and polls it at safe checkpoints.

**Why unbounded:** Steering is rare and bounded by human typing speed. An
unbounded channel eliminates backpressure concerns without memory risk.

---

## First Principle 2: Injection Point Discipline

> *"Inject only at message boundaries, never mid-message."*

The LLM API requires alternating user/assistant messages. Injecting a user
message while the assistant is mid-streaming would corrupt the turn structure.

**Safe injection points:**
```
user message ──→ [API CALL] ──→ assistant response
                                     │
                             has tool_calls?
                                  YES ─→ dispatch tools ──→ [INJECT HERE] ──→ loop
                                  NO  ─→ final response
```

The only safe injection point is **between tool dispatch completion and the next
API call**. This is where we check the steering channel.

**Why not during streaming?** Mid-stream injection would:
1. Create an orphaned assistant message (no matching tool results)
2. Corrupt Anthropic's message alternation requirement
3. Potentially break the streaming decoder

---

## First Principle 3: Cache Preservation

> *"The system prompt is sacred — never rebuild it for a steer."*

Anthropic charges ~1.25× write cost to cache the system prompt. After the
first cache write, subsequent turns pay only 10% of the read cost. Rebuilding
the system prompt on every steer would make each steer cost 5–10× more.

**Rule:** Steer messages are injected as **user-role messages** into the conversation
history. They never touch the system prompt. The system prompt cache remains
valid for the lifetime of the session.

**Steer message format:**
```
{"role": "user", "content": "[⛵ STEER] <user's steering text>"}
```

The `[⛵ STEER]` tag makes steers searchable in session history and signals to
the LLM (and to our compression logic) that this is human guidance, not a task.

---

## First Principle 4: Non-Destructive by Default

> *"Prefer soft operations. Hard-stop is the last resort."*

Steering kind hierarchy (from least to most destructive):

```
HINT      → append user context, continue loop unchanged
REDIRECT  → append user context, suggest loop should change direction
STOP      → graceful: finish current tool, then exit loop with partial result
ABORT     → immediate: cancel token, destroy current tool, exit now
```

Ctrl+C maps to ABORT. The new steer shortcut defaults to HINT/REDIRECT.
The user gets to choose by how they phrase their steer.

---

## First Principle 5: Child Agent Propagation

> *"A steer to the parent is a steer to all children."*

When the agent delegates to a sub-agent (via `delegate_task`), the steer must
reach the sub-agent. This is achieved by:
1. Storing the steer in the parent's pending buffer
2. On the next turn boundary, serialising the steer into the sub-agent's
   injected context (same mechanism used for parent injection)

---

## First Principle 6: Idempotent Drain

> *"Multiple steers in the queue become one clear signal."*

If the user sends 3 quick steers while the agent is in a long tool execution,
the loop should not execute 3 separate injection turns. Instead, drain all
pending steers and combine them into one user message:

```
[⛵ STEER] (1/3) Focus on auth
[⛵ STEER] (2/3) Skip DB migration
[⛵ STEER] (3/3) Use JWT not sessions
```

This preserves all user guidance while minimising API call overhead.

---

## First Principle 7: UX Immediacy

> *"The user must see acknowledgement within 100ms of sending a steer."*

Even if the agent is mid-tool-call and won't process the steer for 10 seconds,
the TUI must immediately show: `⛵ 1 steer pending`.

This prevents the user from sending duplicate steers out of uncertainty.

---

## Design Invariants Summary

| # | Invariant |
|---|-----------|
| I1 | Steer channel is `UnboundedSender` — never blocks the TUI |
| I2 | Steer injection only at tool boundaries, never mid-stream |
| I3 | System prompt never rebuilt for steers (cache preserved) |
| I4 | Default kind is HINT (non-destructive) |
| I5 | Multiple pending steers are drained and merged |
| I6 | TUI shows pending steer count within 1 render frame |
| I7 | Steer messages tagged `[⛵ STEER]` for history search |
| I8 | Hard cancel (Ctrl+C) always works independently |
