# Mission Steering — Architecture

## 1. Component Overview

```
+------------------------------------------------------------------+
|                         TUI (App)                                |
|  +-------------------+   Ctrl+S   +-------------------------+   |
|  | Main Input Area   |  -------> | Steering Overlay        |   |
|  | (normal prompt)   |           | (floating, non-blocking) |   |
|  +-------------------+           +-------------------------+   |
|                                            |                    |
|  Status Bar: ⛵ 1 steer pending            | send               |
+--------------------------------------------|--------------------|
                                             |
                                             v
                              SteeringChannel (mpsc::UnboundedSender)
                                             |
+--------------------------------------------v--------------------|
|                   execute_loop() [Tokio task]                    |
|                                                                  |
|  loop {                                                          |
|    budget_gate()                                                 |
|    cancel_gate()          ← Ctrl+C still works                   |
|    steer_gate()           ← NEW: drain SteeringChannel           |
|    compression()                                                 |
|    api_call()             ── streaming ──>  TUI StreamEvent      |
|    if tool_calls:                                                |
|      dispatch_tools()                                            |
|      [STEER INJECTION POINT] ← safe boundary after tool results  |
|    else:                                                         |
|      return final_response                                       |
|  }                                                               |
+------------------------------------------------------------------+
```

---

## 2. Steering Types

```rust
/// A user-initiated guidance signal injected into a running agent loop.
///
/// WHY an enum: different steering intents require different injection
/// strategies. HINT appends context; REDIRECT signals course change;
/// STOP is a graceful-exit request (finish current tool, then stop).
pub enum SteeringKind {
    /// Add context or a hint — loop continues unchanged.
    Hint,
    /// Suggest a different approach — strong signal to the LLM.
    Redirect,
    /// Finish the current tool then stop the loop (graceful partial result).
    Stop,
}

pub struct SteeringEvent {
    pub kind: SteeringKind,
    pub message: String,
    pub timestamp: std::time::Instant,
}
```

---

## 3. Steering Channel Lifecycle

```
AgentBuilder::build()
        |
        v
  create channel:
    (steer_tx, steer_rx) = mpsc::unbounded_channel()
        |
        |── steer_rx ──> stored in Agent.steer_rx (Mutex<Option<Receiver>>)
        |
        v
  Agent::steer_sender() -> Option<SteeringChannel>
        |
        v
  TUI App: stores steer_tx as app.steer_tx
        |
        v
  User presses Ctrl+S
        |
        v
  SteeringOverlay opens
        |
        v
  User types + Enter
        |
        v
  steer_tx.send(SteeringEvent { kind: Hint, message: "..." })
        |
        v
  TUI immediately shows: "⛵ 1 steer pending"
        |
        v
  execute_loop reaches next tool boundary
        |
        v
  drain_pending_steers(steer_rx)
    → builds combined steer message
    → appends to session.messages as user-role
        |
        v
  Next API call: LLM sees steering context
        |
        v
  TUI shows: "⛵ steer applied" (briefly, then fades)
```

---

## 4. Injection Point — Detail

```rust
// In process_response() or just before the next API call:

// STEER INJECTION POINT
// Check for pending steers between tool dispatch and next API call.
// WHY here: all tool results are fully committed to session.messages,
// so we maintain strict user/assistant alternation.
if let Some(steer_msg) = drain_pending_steers(steer_rx) {
    tracing::info!(len = steer_msg.len(), "steering message injected");
    session.messages.push(Message::user(&steer_msg));
    // Emit a StreamEvent so the TUI can display the injected steer
    if let Some(tx) = event_tx {
        let _ = tx.send(StreamEvent::SteerApplied { message: steer_msg });
    }
}
```

---

## 5. Message Format in History

```
[⛵ STEER] Focus on authentication — skip the DB migration step.
```

When multiple pending steers are drained:
```
[⛵ STEER] (1) Focus on authentication
[⛵ STEER] (2) Skip DB migration
[⛵ STEER] (3) Use JWT, not sessions
```

---

## 6. StreamEvent Extensions

```rust
pub enum StreamEvent {
    // ... existing variants ...

    /// A steering event was injected into the conversation.
    SteerApplied { message: String },

    /// A steering event is pending (waiting for next tool boundary).
    SteerPending { count: usize },
}
```

---

## 7. Data Flow Diagram

```
User types steer
      |
      v
  [TUI Event Loop]
  handle_key(Ctrl+S) -> open SteeringOverlay
  handle_key(Enter in overlay) -> steer_tx.send(event)
  pending_steer_count += 1
  -> trigger redraw
      |
      |  mpsc channel
      v
  [execute_loop (Tokio)]
  'conversation_loop: loop {
      budget_gate()
      cancel_gate()
      compression()
      api_call() ─── streaming ──> StreamEvent::Token
           |
           v (if tool_calls)
      dispatch_tools() ────────> ToolContext (has cancel, steer_rx)
           |
           v (all tool results collected)
      [STEER CHECK] ──── drain steer_rx ────> None | Some(msg)
                                                        |
                                              append Message::user(msg)
                                              emit StreamEvent::SteerApplied
      loop ──> next api_call (LLM now sees steer)
  }
```

---

## 8. Agent Struct Extension

```rust
pub struct Agent {
    // ... existing fields ...

    /// Steering channel receiver.
    ///
    /// WHY Mutex: the receiver is moved out once into the execute_loop
    /// but must be accessible from the Agent's public API for re-creation.
    /// Using a Mutex<Option<Receiver>> allows execute_loop to take the
    /// receiver for its duration without blocking other Agent methods.
    pub(crate) steer_rx: std::sync::Mutex<
        Option<tokio::sync::mpsc::UnboundedReceiver<SteeringEvent>>
    >,

    /// Cloneable sender end — handed to the TUI or gateway.
    pub(crate) steer_tx: tokio::sync::mpsc::UnboundedSender<SteeringEvent>,
}

impl Agent {
    /// Returns the sender half of the steering channel.
    /// The TUI stores this to enqueue user steers.
    pub fn steer_sender(&self) -> SteeringSender {
        self.steer_tx.clone()
    }
}
```

---

## 9. TUI App Extensions

```rust
pub struct App {
    // ... existing fields ...

    /// Steering sender wired to the current agent.
    steer_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::steering::SteeringEvent>>,

    /// Count of steers pending in the channel (updated optimistically on send).
    pending_steer_count: usize,

    /// Whether the steering overlay is currently shown.
    steering_overlay_active: bool,

    /// Text content of the steering input box.
    steering_input: tui_textarea::TextArea<'static>,
}
```

---

## 10. Security: Injection Scan

All steer messages MUST be scanned before injection:

```rust
fn build_steer_message(events: &[SteeringEvent]) -> Option<String> {
    if events.is_empty() { return None; }

    // Security: scan for prompt injection in user steer text
    let combined = events.iter()
        .enumerate()
        .map(|(i, e)| {
            let prefix = if events.len() == 1 { String::new() }
                         else { format!("({}) ", i + 1) };
            format!("[⛵ STEER] {}{}", prefix, e.message.trim())
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Reuse existing injection scanner
    let threats = crate::prompt_builder::scan_for_injection(&combined);
    if threats.iter().any(|t| t.severity == crate::prompt_builder::InjectionSeverity::High) {
        tracing::warn!(n = threats.len(), "steer message blocked: high-severity injection");
        return Some("[⛵ STEER] [blocked: content flagged by injection scanner]".into());
    }
    Some(combined)
}
```
