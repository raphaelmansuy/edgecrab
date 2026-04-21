# Mission Steering — Problem Analysis & Cross-Reference Study

## 1. Problem Statement

When an AI agent is running a multi-step ReAct loop (reasoning → tool calls →
reasoning…), the user may observe that:

- The agent is **on the wrong track** ("focus on X, not Y")
- The agent **missed context** they forgot to include
- The agent is **stuck** in a sub-optimal approach
- The agent should **stop a specific sub-task** but continue the session
- The **mission has changed** mid-flight

Currently in EdgeCrab the only option is **Ctrl+C** (hard cancel), which:
1. Destroys the entire ongoing conversation turn
2. Loses all tool results accumulated so far
3. Requires the user to re-state their full intent
4. Invalidates the Anthropic prompt cache (expensive)

**Goal:** Provide a *soft* steering primitive that injects user guidance at the
next tool boundary, preserving conversation state and cache validity.

---

## 2. Cross-Reference Study

### 2.1 Hermes Agent (Python)

```
tools/interrupt.py
run_agent.py:
  _interrupt_requested: bool
  _interrupt_message: Optional[str]
  busy_input_mode: "interrupt" | "queue"
  _pending_input: Queue
  _interrupt_queue: Queue

def interrupt(self, message=None):
    self._interrupt_requested = True
    self._interrupt_message = message
    _set_interrupt(True)            # global threading.Event
    # propagate to child agents
```

**Key insight:** Hermes has two modes:
- `interrupt` mode: Enter while running → stop the loop, new message is the *replacement* prompt
- `queue` mode: Enter while running → queue the message for the *next* turn (soft steer)

The interrupt signal is a global `threading.Event` polled by long-running tools.
When the loop exits with `interrupted=True`, `_interrupt_message` is included in
the result so the CLI can immediately re-invoke with that message.

**Missing:** No concept of *injecting* the hint INTO the running conversation's
message list while continuing execution. It always stops and restarts.

### 2.2 Claude Code Analysis (TypeScript)

```typescript
// query.ts
toolUseContext.abortController.signal.aborted
toolUseContext.abortController.signal.reason !== 'interrupt'

// RemoteSessionManager.ts
cancelSession(): void {
    this.websocket?.sendControlRequest({ subtype: 'interrupt' })
}

// messages.ts
createUserInterruptionMessage({ toolUse: false })
// Injects "[Request interrupted by user]" into message history
```

**Key insight:** Claude Code has a more sophisticated abort:
- `reason === 'interrupt'` (user-initiated): skip the `createUserInterruptionMessage`
  because the queued user message provides context
- `reason !== 'interrupt'` (submit-interrupt): inject placeholder then follow with user msg

The **submit-interrupt** pattern is the key: user types new message while agent
runs → abort signal fires with `reason = 'interrupt'` → the queued message IS the
continuation. No interruption placeholder needed.

**Missing:** No "non-aborting hint injection" — you still kill the current turn.

### 2.3 EdgeCrab (Rust, current state)

```rust
// agent.rs
pub fn interrupt(&self) {
    self.cancel.lock().cancel();  // CancellationToken — one-way latch
}

// conversation.rs - cancellation gate
if cancel.is_cancelled() {
    interrupted = true;
    break;
}

// injected_messages: Option<Arc<tokio::sync::Mutex<Vec<Message>>>>
// Exists in ToolContext but never populated from outside
```

**Key insight:** EdgeCrab already has:
1. `CancellationToken` for hard-stop
2. `injected_messages` arc-mutex in ToolContext (populated as `None` currently)
3. Tool-boundary checks for cancellation

**Missing:**
- A steering channel (mpsc sender for soft hints)
- Steering message injection at tool boundaries
- TUI keybinding for steering input
- Visual feedback when a steer is pending or was applied

---

## 3. Comparative Analysis

```
Feature                  Hermes      Claude Code    EdgeCrab (now)
─────────────────────────────────────────────────────────────────
Hard cancel              ✓           ✓              ✓ (Ctrl+C)
Queue next message       ✓ (/queue)  ✓              ✓ (/queue)
Inject hint mid-loop     ✗           ✗              ✗
Submit-interrupt         ✓ (bim)     ✓              ✗
Steer without abort      ✗           ✗              ✗  ← TARGET
TUI steer overlay        ✗           ✗              ✗  ← TARGET
Child agent propagation  ✓           partial        ✓ (cancel only)
Cache preservation       ✗ (breaks)  ✗ (breaks)     ✗ (breaks)  ← IMPROVE
```

---

## 4. Target Behaviour

After implementation:

```
User presses Ctrl+S while agent is running
    ↓
Steering overlay opens (non-blocking)
    ↓
User types: "focus on authentication, skip the DB migration"
    ↓
Sends with Enter — overlay closes
    ↓
At the NEXT tool-dispatch boundary:
  1. Pending steer is drained from steering channel
  2. Injected as a user message: "[STEER] focus on authentication..."
  3. Agent loop continues — LLM sees the steer on the next API call
    ↓
TUI shows "⛵ 1 steer applied" in the status bar briefly
```

Hard-cancel (Ctrl+C) remains unchanged and always works.

---

## 5. Requirements

### Functional
- R1: User can inject a hint while the loop is running
- R2: Hint is injected at the next tool-dispatch boundary (not mid-stream)
- R3: Multiple concurrent steers are merged/queued
- R4: Steer does NOT cancel the current tool execution (non-destructive)
- R5: Hard cancel (Ctrl+C) continues to work independently
- R6: Steer is visible in the TUI (pending + applied states)
- R7: Steer is visible in conversation history (tagged message)
- R8: Child agents (delegate_task) receive propagated steers

### Non-Functional
- NF1: Steer injection MUST preserve Anthropic prompt cache (no system prompt rebuild)
- NF2: Steer channel is lock-free (no blocking the conversation loop)
- NF3: TUI overlay must not block rendering (≤1 frame delay)
- NF4: Steer message is sanitized (injection scan before appending)
- NF5: Zero overhead when no steer is pending (hot path)
