# Agent Struct 🦀

> **Verified against:** `crates/edgecrab-core/src/agent.rs`

---

## Why `Agent` is the shape it is

Every design decision in the `Agent` struct answers the question:
*"what survives across multiple turns of a conversation?"*

- Config and provider are behind `RwLock` because the `/model` command
  can swap them mid-session without ending the conversation.
- `ProcessTable` and `TodoStore` are owned by the agent so that a
  background process started in turn 3 is still trackable in turn 10.
- `state_db` is optional because tests and cron runs should not require
  a SQLite database to function.
- `budget` uses an atomic because it is checked on every iteration and
  must not become a contention bottleneck.

---

## Struct fields

```rust
// crates/edgecrab-core/src/agent.rs
pub struct Agent {
    // Hot-swappable: the /model command writes these under a lock
    pub(crate) config:          RwLock<AgentConfig>,
    pub(crate) provider:        RwLock<Arc<dyn LLMProvider>>,
    pub(crate) gateway_sender:  RwLock<Option<Arc<dyn GatewaySender>>>,

    // Conversation history, token counters, cached system prompt
    pub(crate) session:         RwLock<SessionState>,

    // Optional — tests and cron can skip SQLite
    pub(crate) state_db:        Option<Arc<SessionDb>>,

    // Read-only after build(); safe to access without a lock
    pub(crate) tool_registry:   Option<Arc<ToolRegistry>>,

    // Background processes survive across multiple tool calls
    pub(crate) process_table:   Arc<ProcessTable>,

    // Lock-free iteration budget (AtomicU32 internally)
    pub(crate) budget:          Arc<IterationBudget>,

    // Per-turn cancellation (reset on new_session)
    pub(crate) cancel:          Mutex<CancellationToken>,

    // Background GC task lifetime — cancelled on Drop
    pub(crate) gc_cancel:       CancellationToken,

    // Session-scoped todo list shared with tools
    pub(crate) todo_store:      Arc<TodoStore>,
}
```

---

## `AgentConfig` key fields

```
  ┌──────────────────────────────────────────────────────────┐
  │  AgentConfig (default values from code)                  │
  │                                                          │
  │  model                  "anthropic/claude-opus-4.6"      │
  │  max_iterations         90                               │
  │  streaming              true                             │
  │  platform               Platform::Cli                    │
  │  delegation_enabled     true                             │
  │  delegation_max_subagents  3                             │
  │  delegation_max_iterations 50                            │
  │  checkpoints_enabled    true                             │
  │  checkpoints_max_snapshots 50                            │
  │  terminal_backend       BackendKind::Local               │
  │                                                          │
  │  enabled_toolsets       Vec<String>  (empty = all)       │
  │  disabled_toolsets      Vec<String>                      │
  │  file_allowed_roots     Vec<PathBuf>                     │
  │  path_restrictions      Vec<PathBuf>                     │
  └──────────────────────────────────────────────────────────┘
```

---

## `SessionState` — the mutable conversation

```rust
pub struct SessionState {
    pub session_id:                Option<String>,
    pub messages:                  Vec<Message>,
    pub cached_system_prompt:      Option<String>,
    pub user_turn_count:           u32,
    pub api_call_count:            u32,
    pub session_input_tokens:      u64,
    pub session_output_tokens:     u64,
    pub session_cache_read_tokens: u64,
    pub session_cache_write_tokens: u64,
    pub session_reasoning_tokens:  u64,
    pub session_tool_call_count:   u32,
}
```

`cached_system_prompt` is the performance-critical field. `PromptBuilder`
is called once per session (or when `invalidate_system_prompt()` is called
explicitly). Between calls, the system prompt is reused verbatim.

---

## `AgentBuilder` — constructing an `Agent`

```rust
// Minimum viable builder:
let agent = AgentBuilder::new("anthropic/claude-sonnet-4-20250514")
    .provider(Arc::new(my_provider))
    .build()?;

// Full builder for production gateway use:
let agent = AgentBuilder::new(config.model.name.as_str())
    .from_config(&app_config)
    .provider(Arc::clone(&provider))
    .state_db(Arc::clone(&session_db))
    .tools(Arc::clone(&tool_registry))
    .platform(Platform::Telegram)
    .session_id(session_id.clone())
    .origin_chat(platform_str, chat_id)
    .streaming(true)
    .build()?;
```

`build()` returns `Err(AgentError::Config("no provider set"))` if
`.provider()` was never called — the only mandatory field.

---

## `IterationBudget`

```
  AgentConfig::max_iterations = 90  (default)
        │
        ▼
  IterationBudget::new(90)
    remaining = AtomicU32(90)
        │
  each iteration:
        ▼
  budget.try_consume()  → CAS decrement
    ├── true  → continue loop
    └── false → AgentError::BudgetExhausted { used: 90, max: 90 }
                ConversationResult::budget_exhausted = true
```

---

## `StreamEvent` — what frontends receive

`Agent::chat_streaming()` sends these events over
`tokio::sync::mpsc::UnboundedSender<StreamEvent>`:

```
  Client (TUI / gateway / ACP)               Agent task
        │                                         │
        │◄── StreamEvent::Token("Hello ")         │
        │◄── StreamEvent::Reasoning("let me ...")  │  (thinking models)
        │◄── StreamEvent::ToolExec { name, args } │
        │◄── StreamEvent::ToolDone { name, dur.. }│
        │◄── StreamEvent::ContextPressure { .. }  │  (compression warning)
        │◄── StreamEvent::Clarify { question, tx }│  (agent asks user)
        │◄── StreamEvent::Approval { command, tx }│  (dangerous shell cmd)
        │◄── StreamEvent::Done                    │
```

`Clarify` and `Approval` carry a `oneshot::Sender<String>` (or
`oneshot::Sender<ApprovalChoice>`) — the frontend sends the user's
response back through the channel and the loop resumes.

---

## `ApprovalChoice`

```rust
pub enum ApprovalChoice {
    Once,     // approve just this execution
    Session,  // approve all identical commands for this session
    Always,   // add to permanent allowlist (~/.edgecrab/approval.json)
    Deny,     // block the command; model sees a PermissionDenied error
}
```

---

## Public API reference

`Agent`'s most-used methods:

| Method | What it does |
|---|---|
| `chat(&str)` | Single-turn, returns full response string |
| `chat_in_cwd(&str, &Path)` | Single-turn with explicit working directory |
| `chat_streaming(&str, tx)` | Streaming turn; sends `StreamEvent` to `tx` |
| `run_conversation(user, sys, history)` | Supply your own history and system prompt |
| `fork_isolated(opts)` | Clone agent with isolated session for sub-agent delegation |
| `interrupt()` | Signal cooperative cancellation |
| `new_session()` | Clear history and session ID, retain config and provider |
| `swap_model(model, provider)` | Hot-swap model/provider without losing history |
| `force_compress()` | Trigger compression immediately |
| `undo_last_turn()` | Remove last assistant+user turn pair from history |
| `restore_session(&str)` | Load session from SQLite into memory |
| `session_snapshot()` | Copy current session for checkpointing |

---

## Lifecycle diagram

```
  AgentBuilder::build()
        │
        ▼
  Agent created
   ├── gc background task spawned (with gc_cancel)
   │
   ├── chat() / chat_streaming()
   │       │
   │       ▼
   │   execute_loop()  [see Conversation Loop doc]
   │       │
   │       ▼
   │   ConversationResult returned
   │
   ├── new_session()  → clear messages, reset session_id
   │
   └── Agent::drop()  → cancel gc_cancel → GC task stops
```

---

## Tips

> **Tip: `fork_isolated()` creates a sub-agent that shares the tool registry
> and state_db but has its own message history and cancellation token.**
> Use this for `delegate_task` — the sub-agent can run 50 iterations
> independently and return a `SubAgentResult` to the parent.

> **Tip: Calling `invalidate_system_prompt()` forces the next turn to rebuild
> from scratch.** Do this after `/memory` writes or skill installs so the new
> content is reflected immediately.

> **Tip: `session_snapshot()` returns a cloneable struct suitable for storing
> as a checkpoint.** Pair with `restore_session()` to implement undo-like rollback.

---

## FAQ

**Q: Is one `Agent` per user or one global `Agent`?**
One per logical session. The gateway creates one `Agent` per `(platform, user_id)`
pair. The CLI creates one per interactive session. They share the same
`ToolRegistry` and `SessionDb` (behind `Arc`), but have independent conversation
histories.

**Q: Why is `state_db` optional?**
Tests call `AgentBuilder::new(..).provider(..).build()` — no database needed.
Cron runs typically also skip persistence. Only gateway and CLI sessions that
need session history pass `.state_db()`.

**Q: What does `Drop` do on `Agent`?**
It cancels `gc_cancel`, which signals the background garbage-collection task
(which prunes old process handles and expired session data) to stop gracefully.

---

## Cross-references

- Conversation loop that `execute_loop()` implements → [Conversation Loop](./002_conversation_loop.md)
- System prompt assembly → [Prompt Builder](./003_prompt_builder.md)
- Concurrency details for `RwLock` usage → [Concurrency Model](../002_architecture/003_concurrency_model.md)
- `ToolContext` passed to tools → [Tools Runtime](../004_tools_system/004_tools_runtime.md)
