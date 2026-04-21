---
title: Mission Steering
description: Guide to EdgeCrab's live mission steering — inject hints, redirects, or stop signals into a running agent loop via Ctrl+S in the TUI or the steering API.
sidebar:
  order: 4
---

Mission Steering lets you guide an **already-running agent** without cancelling
it.  Instead of waiting for the current turn to finish (or pressing `Ctrl+C`),
you inject a typed steer signal that the agent picks up at the next tool-result
boundary.

---

## Why steering?

The ReAct loop can run for many iterations and many seconds.  During that time
you may realize the agent is:

- heading in a direction that is technically correct but not what you wanted
  (**Hint** — subtle correction, no hard pivot)
- going down a wrong path entirely (**Redirect** — explicit new direction)
- doing something you want stopped immediately (**Stop** — clean exit after the
  current tool finishes)

Steering lets you communicate those corrections _inline_ without destroying the
conversation context or losing the work already done.

---

## TUI Usage

### Opening the overlay

Press **`Ctrl+S`** while the agent is processing.  A compact floating panel
appears at the bottom of the screen:

```
┌─────────────── ⛵ Mission Steer ───────────────────────┐
│  kind:  HINT   REDIRECT   STOP      (Tab=cycle)        │
│                                                         │
│  > type your steer here…                               │
│                                                         │
│  Enter send   Tab kind   Esc cancel                    │
└─────────────────────────────────────────────────────────┘
```

### Cycling the steer kind

Press **`Tab`** to cycle between the three kinds:

| Kind       | Colour  | Effect |
|------------|---------|--------|
| `HINT`     | green   | Gentle nudge; agent stays on course but considers the hint |
| `REDIRECT` | amber   | Explicit new direction; agent pivots |
| `STOP`     | red     | Request clean exit after the current tool finishes |

### Sending the steer

Press **`Enter`** to send.  The overlay closes and a confirmation appears in
the output stream:

```
⛵ Steer queued (HINT/focus only on the auth layer)
```

The status bar shows the pending count in amber:

```
⛵ 1 pending
```

When the agent picks it up the status bar flashes green:

```
⛵ applied
```

### Pressing Ctrl+S while idle

If the agent is **not** currently processing, pressing `Ctrl+S` opens the
overlay but sends the steer as a new user message prefixed with
`[⛵ STEER/<kind>]` when you press Enter.  The agent receives it as an
ordinary turn.

### Closing without sending

Press **`Esc`** or **`Ctrl+S`** again to close the overlay without sending.

---

## Status Bar Indicators

The status bar reflects steering state:

| State | Display |
|-------|---------|
| Steer sent, not yet picked up | `⛵ N pending` (amber background) |
| Steer picked up by agent | `⛵ applied` (green, fades after 4 s) |
| No pending steers | *(nothing)* |

The right-side hint area shows **`^S=steer`** while the agent is running.

---

## API Usage

Steers can be sent programmatically via the steering channel exposed by
`Agent::steer_sender()`:

```rust
use edgecrab_core::{SteeringEvent, SteeringKind};

// Obtain the sender once after building the agent.
let steer_tx = agent.steer_sender();

// Send a hint (clone the sender for each send).
steer_tx.send(SteeringEvent::new(
    SteeringKind::Hint,
    "focus on the auth module only",
))?;

// Redirect mid-turn.
steer_tx.send(SteeringEvent::new(
    SteeringKind::Redirect,
    "use an async approach instead",
))?;

// Ask the agent to stop cleanly.
steer_tx.send(SteeringEvent::new(
    SteeringKind::Stop,
    "stop after this tool call",
))?;
```

`SteeringSender` is a cheap clone (`UnboundedSender`) — store one per session
and share clones across threads.

### Steers are injected after each tool result

```
while api_call_count < max_iterations {
    response = provider.chat(model, messages, tools).await?;
    if response.has_tool_calls() {
        for call in response.tool_calls {
            result = registry.dispatch(call).await;
            messages.push(tool_result(call.id, result));
        }
        // ← Steers injected HERE as [⛵ STEER] user messages ←
        drain_pending_steers(&steer_rx, &mut messages);
    } else {
        return final_response;
    }
}
```

`STOP` steers also cancel any long-running tool via the shared cancellation
token.

---

## Gateway `second_message_mode`

In messaging platform deployments (Telegram, Slack, etc.) you can configure
how a **second message** that arrives while the agent is already running is
handled.  Set `second_message_mode` in `gateway:` in `config.yaml`:

```yaml
gateway:
  second_message_mode: steer   # queue | steer | interrupt
```

| Mode        | Behaviour |
|-------------|-----------|
| `queue`     | The new message is queued and replayed after the current turn finishes. *(default)* |
| `steer`     | The new message is injected as a `Redirect` steer into the running agent. The user receives `⛵ Steering...` as an acknowledgement. |
| `interrupt` | The running agent is cancelled immediately and a new turn starts with the new message. |

### Example: Telegram mid-turn redirect

```
User: summarise the last 100 commits
Bot:  [agent starts reading git log...]
User: actually just focus on the last 7 days
Bot:  ⛵ Steering the agent with your new message...
      [agent pivots without the user needing to wait]
```

---

## Edge Cases

| Scenario | Outcome |
|----------|---------|
| Steer channel closed (session ended) | CLI: falls back to sending as new message.  Gateway: falls back to `queue`. |
| `STOP` steer | Agent exits cleanly after the _current_ tool returns.  Active background processes receive a cancellation signal. |
| Multiple steers in flight | All are injected in order at the next tool boundary.  The pending count in the status bar reflects the queue depth. |
| Steer arrives when no tool calls are pending | Injected before the _next_ LLM call as context. |

---

## Architecture Overview

```
                 Ctrl+S (TUI)
                      │
                      ▼
             ┌─────────────────┐
             │ SteeringOverlay │  (render_steering_overlay)
             │ (steering_kind, │
             │  textarea)      │
             └────────┬────────┘
                      │ send_steer_from_overlay()
                      ▼
             ┌─────────────────┐
             │  SteeringSender │  agent.steer_sender()
             │  (unbounded tx) │
             └────────┬────────┘
                      │  SteeringEvent { kind, message }
                      ▼
         ┌────────────────────────────┐
         │    ReAct Loop              │
         │    (conversation.rs)       │
         │                            │
         │   …tool result returned…   │
         │       ↓                    │
         │   drain_pending_steers()   │
         │       ↓                    │
         │   inject as [⛵ STEER]     │
         │   user message             │
         └────────────────────────────┘
```

---

## Related

- [React Loop](react-loop) — detailed walkthrough of `execute_loop()`
- [TUI Interface](tui) — all keyboard shortcuts and status bar details
- [Memory](memory) — persistent agent memory, distinct from transient steers
