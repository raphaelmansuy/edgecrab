# Agent Architecture & Implementation

**File:** `crates/edgecrab-core/src/agent.rs` (2594 lines)  
**Purpose:** Core entry point for multi-turn conversation execution, model hot-swapping, session management, and dependency injection.

---

## Executive Summary

The `Agent` is the primary interface for running conversations in EdgeCrab. It combines:

- **Builder pattern** for 10+ dependency injection points (provider, tools, state DB, etc.)
- **RwLock-based hot-swap** for real-time model and provider switching
- **Separation of concerns** — conversation lock independent from session state lock to prevent blocking on I/O
- **Atomic iteration budget** (lock-free) to prevent runaway tool loops
- **Session-scoped state** including messages, metrics, compression history, and cancel tokens

The agent does NOT contain the ReAct loop itself — that lives in `conversation.rs`. This file provides the orchestration, configuration, and session management wrapper around it.

---

## Data Structure Overview

```
┌─────────────────────────────────────────────────────────────────┐
│ Agent — Session + Conversation Orchestrator                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  config: RwLock<AgentConfig>  ◄── Hot-swap on /model command   │
│                               ├─ model, max_iterations         │
│                               ├─ toolsets, tool gating         │
│                               ├─ compression policy            │
│                               ├─ provider routing              │
│                               └─ platform (CLI/Telegram/etc)   │
│                                                                 │
│  provider: RwLock<Arc<dyn LLMProvider>>                         │
│                               ◄── Switched by /model, cloned   │
│                               at loop start (in-flight safe)    │
│                                                                 │
│  session: RwLock<SessionState> ◄── Messages, metrics, history  │
│                               ├─ messages: Vec<Message>        │
│                               ├─ compression_summary_prefix    │
│                               ├─ user/api turn counters        │
│                               ├─ token usage (cumulative)      │
│                               ├─ injected_assistant_context    │
│                               └─ preloaded_skills              │
│                                                                 │
│  conversation_lock: Mutex<()> ◄── Serializes turns             │
│  ┌────────────────────────────────────────────────────────────┐│
│  │ WHY separate from session lock:                            ││
│  │ • Old design: session lock guarded both state AND loop     ││
│  │ • Problem: /status, inspection reads blocked for full turn ││
│  │   (prompt assembly + API call + tool execution)            ││
│  │ • Solution: conversation_lock is a thin mutex, session     ││
│  │   lock only protects state reads/writes                    ││
│  └────────────────────────────────────────────────────────────┘│
│                                                                 │
│  tool_registry: RwLock<Option<Arc<ToolRegistry>>>               │
│                 ◄── Hot-swappable tool set                      │
│                                                                 │
│  gateway_sender: RwLock<Option<Arc<dyn GatewaySender>>>         │
│                 ◄── For send_message to external platforms     │
│                 None in CLI/cron; set by gateway runtime       │
│                                                                 │
│  process_table: Arc<ProcessTable> ◄── Shared background procs  │
│                 WHY on Agent: all tools in a session share     │
│                 the same process namespace (Agent lifetime)    │
│                                                                 │
│  budget: Arc<IterationBudget>  ◄── Lock-free atomic counter    │
│          Prevents runaway loops (max 90 by default)            │
│                                                                 │
│  cancel: std::sync::Mutex<CancellationToken>  ◄── Ctrl+C       │
│          Reset on every new turn so Ctrl+C only stops the     │
│          current turn, not all future turns                   │
│          (one-way latch, needs fresh token each turn)         │
│                                                                 │
│  gc_cancel: CancellationToken  ◄── For background GC task      │
│             NOT reset; lives for Agent lifetime               │
│             Cancelled via Drop                                 │
│                                                                 │
│  todo_store: Arc<TodoStore>   ◄── Persistent task list        │
│              Survives context compression                     │
│              Re-injected after compression                    │
│              (Agent lifetime == session lifetime)             │
│                                                                 │
│  state_db: Option<Arc<SessionDb>>  ◄── Session persistence    │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## AgentConfig — Runtime Configuration

```rust
pub struct AgentConfig {
    // Model & iteration control
    pub model: String,                    // "anthropic/claude-opus-4.6"
    pub max_iterations: u32,              // Hard cap on ReAct loop (default 90)
    
    // Tool gating
    pub enabled_toolsets: Vec<String>,    // "CORE_TOOLS", "WEB_TOOLS"
    pub disabled_toolsets: Vec<String>,   // Blacklist
    pub enabled_tools: Vec<String>,       // Individual tool names
    pub disabled_tools: Vec<String>,      // Individual tool blacklist
    
    // Conversation behavior
    pub streaming: bool,                  // Live token streaming (default true)
    pub temperature: Option<f32>,         // LLM temperature override
    pub reasoning_effort: Option<String>, // "low" / "medium" / "high"
    
    // Context & identity
    pub platform: Platform,               // CLI, Telegram, Discord, etc.
    pub api_mode: ApiMode,                // ChatCompletions or other
    pub session_id: Option<String>,       // Persistent session ID
    pub quiet_mode: bool,                 // Suppress progress/status
    
    // System prompt customization
    pub personality_addon: Option<String>,      // From config.display.personality
    pub custom_system_prompt: Option<String>,   // User-provided override
    
    // Feature flags
    pub save_trajectories: bool,          // Save turn transcripts
    pub skip_context_files: bool,         // Skip AGENTS.md/SOUL.md
    pub skip_memory: bool,                // Skip MEMORY.md/USER.md
    
    // Complex routing & delegation
    pub model_config: ModelConfig,        // base_url, api_key_env, smart routing
    pub skills_config: SkillsConfig,      // Disabled skills, platform-specific
    pub plugins_config: PluginsConfig,    // Plugin enable/disable
    pub delegation_enabled: bool,         // Subagent delegation
    pub delegation_model: Option<String>,
    pub delegation_max_subagents: u32,    // Default 3
    pub delegation_max_iterations: u32,   // Default 50
    
    // Platform integration
    pub origin_chat: Option<(String, String)>, // ("telegram", "chat_id")
                                                 // Set by gateway
    
    // Context compression
    pub compression: CompressionConfig,   // Threshold, protect_last_n, etc.
    
    // Backend configuration
    pub terminal_backend: BackendKind,    // Local, Docker, SSH, Modal, etc.
    pub terminal_docker: DockerBackendConfig,
    pub terminal_ssh: SshBackendConfig,
    pub terminal_modal: ModalBackendConfig,
    pub terminal_env_passthrough: Vec<String>,
    
    // File system policy
    pub file_allowed_roots: Vec<PathBuf>,
    pub path_restrictions: Vec<PathBuf>,
    pub result_spill: bool,               // Spillable large results to artifacts
    pub result_spill_threshold: usize,    // Default 16 KB
    
    // Auxiliary services
    pub auxiliary: AuxiliaryConfig,       // Vision, compression provider
    pub moa: MoaConfig,                   // Mixture-of-Agents roster
    pub tts: TtsConfig,                   // Voice output
    pub stt: SttConfig,                   // Voice input
    pub image_generation: ImageGenerationConfig,
    pub lsp: LspConfig,                   // Language server definitions
    pub browser: BrowserConfig,           // Recording, timeouts
    pub checkpoints_enabled: bool,        // Git-like state snapshots
}
```

---

## SessionState — Live Conversation State

```rust
pub struct SessionState {
    pub session_id: Option<String>,
    pub title: Option<String>,
    
    // Core conversation history
    pub messages: Vec<Message>,
    pub compression_summary_prefix: Option<String>,
    
    // Metrics (cumulative across session)
    pub user_turn_count: u32,
    pub api_call_count: u32,
    pub session_input_tokens: u64,
    pub session_output_tokens: u64,
    pub session_cache_read_tokens: u64,
    pub session_cache_write_tokens: u64,
    pub session_reasoning_tokens: u64,
    
    // Last API call metadata
    pub last_prompt_tokens: u64,          // Current context pressure indicator
    
    // Tool streaming state
    pub native_tool_streaming_disabled: bool,  // Set after provider rejection
    pub session_tool_call_count: u32,
    
    // Injected context (survives compression)
    pub injected_assistant_context: Option<String>,
    pub preloaded_skills: Vec<String>,
    
    // Custom system prompt (overlaid after base assembly)
    pub custom_system_prompt: Option<String>,
}
```

---

## IterationBudget — Lock-Free Iteration Counter

```rust
pub struct IterationBudget {
    remaining: AtomicU32,      // Lock-free counter
    max: u32,
}

impl IterationBudget {
    pub fn try_consume(&self) -> bool {
        // CAS loop: only decrement if remaining > 0
        // Prevents runaway tool loops
        // Called at top of execute_loop each iteration
    }
    
    pub fn reset(&self) {
        // Called at new_session() to reset for new turn
    }
}
```

**Why atomic?** The budget is checked on every loop iteration in `conversation.rs`. A `Mutex` would cause contention on the hot path.

---

## Public Interface

### Session Initialization & Lifecycle

#### `chat(&self, message: &str) → Result<String>`
Simplest interface. Runs one message through the full ReAct loop.

```rust
let result = agent.chat("explain this code").await?;
```

#### `chat_in_cwd(&self, message: &str, cwd: &Path) → Result<String>`
Like `chat()`, but sets working directory for tools.

#### `chat_with_origin(&self, message: &str, origin: &str) → Result<String>`
Sets `platform` context before chat (used by gateway to indicate platform source).

```rust
agent.chat_with_origin("hello", "telegram").await?;
// → sets Platform::Telegram in system prompt
```

#### `run_conversation(&self, message, system, history) → ConversationResult`
Full interface. Returns structured result with messages, usage, costs, session ID.

```rust
pub struct ConversationResult {
    pub final_response: String,
    pub messages: Vec<Message>,
    pub session_id: String,
    pub usage: Usage,                // tokens in/out
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost: Cost,                  // USD estimate
    pub turn_count: u32,
    pub api_call_count: u32,
    pub compressed: bool,            // Did context compression happen?
}
```

#### `new_session(&self)`
Resets session ID, clears messages, calls `on_session_reset` hooks, resets budget.

#### `finalize_session(&self)`
Calls `on_session_finalize` hooks before shutdown.

### Model & Provider Control

#### `swap_model(&self, model: String, provider: Arc<dyn LLMProvider>)`
Hot-swap LLM at runtime (used by `/model` command).

**Thread safety:** Clones Arc at loop start, so in-flight conversations unaffected.

#### `model(&self) → String`
Current model name.

### Session Query & Manipulation

#### `session_snapshot(&self) → SessionSnapshot`
Async snapshot of live session state.

```rust
pub struct SessionSnapshot {
    pub user_turn_count: u32,
    pub api_call_count: u32,
    pub session_tokens: u64,
    pub last_prompt_tokens: u64,
    pub cost_usd: f64,
    pub messages_count: usize,
    pub title: Option<String>,
}
```

Used by `/status`, `/cost`, `/history` commands.

#### `system_prompt(&self) → Option<String>`
Cached assembled system prompt for current session.

#### `append_to_system_prompt(&self, note: &str)`
Add runtime notes to system prompt.

#### `invalidate_system_prompt(&self)`
Force rebuild on next turn (e.g., after skill installation).

#### `set_custom_system_prompt(&self, prompt: Option<String>)`
User-provided override prompt (persisted in config).

#### `messages(&self) → Vec<Message>`
Full conversation history.

#### `undo_last_turn(&self) → usize`
Pop last message and assistant response; return new message count.

#### `force_compress(&self)`
Manually trigger context compression.

### Session Persistence

#### `restore_session(&self, id: &str) → Result<usize, AgentError>`
Restore session from state DB; return message count.

#### `list_sessions(db, limit) → Vec<SessionRow>`
List recent sessions (stateless helper).

#### `delete_session(db, id) → Result<(), AgentError>`
Remove session from DB.

#### `rename_session(db, id, title) → Result<(), AgentError>`
Update session title.

### Tools & Skills

#### `tool_names(&self) → Vec<String>`
List available tool names (respecting enabled/disabled policy).

#### `toolset_summary(&self) → Vec<(String, usize)>`
Toolsets with tool counts.

#### `tool_inventory(&self) → Vec<ToolInventoryEntry>`
Rich metadata for UI selectors (name, description, emoji, category, etc.).

#### `set_tool_registry(&self, registry: Arc<ToolRegistry>)`
Hot-swap tool set.

#### `preloaded_skills(&self) → Vec<String>`
Skills injected at prompt assembly.

#### `set_preloaded_skills(&self, skills: Vec<String>)`
Update skill injection list.

### Execution Control

#### `interrupt(&self)`
Trigger Ctrl+C on current turn (sets cancel token).

#### `is_cancelled(&self) → bool`
Check if Ctrl+C was pressed.

#### `fork_isolated(&self, options) → Result<Agent, AgentError>`
Clone agent with fresh isolated session (same config, different session ID).

```rust
pub struct IsolatedAgentOptions {
    pub session_id: Option<String>,
    pub platform: Option<Platform>,
    pub quiet_mode: Option<bool>,
    pub origin_chat: Option<(String, String)>,
}
```

Used by subagent delegation and parallel tasks.

---

## AgentBuilder — Fluent Constructor

```rust
pub struct AgentBuilder {
    config: AgentConfig,
    provider: Option<Arc<dyn LLMProvider>>,
    state_db: Option<Arc<SessionDb>>,
    tool_registry: Option<Arc<ToolRegistry>>,
}

// Usage:
let agent = AgentBuilder::new("anthropic/claude-opus-4.6")
    .provider(provider)
    .tools(registry)
    .state_db(db)
    .config(cfg)
    .build()?;
```

**Why builder?** Agent needs ~10 dependencies. Builder pattern:
- Prevents 10-argument constructors
- Makes optional dependencies explicit
- Readable fluent API

---

## Event & Streaming

### StreamEvent — Granular Conversation Events

```rust
pub enum StreamEvent {
    // Text tokens
    Text { token: String },
    
    // Tool invocation
    ToolCall { 
        name: String,
        args: serde_json::Value 
    },
    ToolResult { 
        name: String,
        result: String 
    },
    
    // State changes
    Compression { old_len: usize, new_len: usize },
    ContextPressure { 
        estimated_tokens: u64,
        threshold_tokens: u64 
    },
    
    // Async events
    SubAgentStart { task_index: usize, task_count: usize },
    SubAgentFinish { 
        task_index: usize,
        task_count: usize,
        status: String,
        duration_ms: u64 
    },
    
    // Flow control
    Clarify { question: String, choices: Option<Vec<String>> },
    Approval { command: String, reason: String },
    SecretRequest { var_name: String, is_sudo: bool },
    
    Done,
    Error(AgentError),
}

// Streaming interface
agent.chat_streaming("explain", tx).await?;
// → tx sends StreamEvent items as they arrive
```

---

## Key Design Patterns

### Pattern 1: Hot-Swap with Arc + RwLock

```
provider: RwLock<Arc<dyn LLMProvider>>

/model command:
  → acquire write lock
  → *prov = new_arc
  → drop lock

execute_loop:
  → acquire read lock
  → let prov_snapshot = Arc::clone(prov)
  → drop lock
  → use prov_snapshot for API call
  
Result: in-flight calls use the old provider; next turn uses new.
```

### Pattern 2: Separated Conversation & Session Locks

```
OLD: session.write() guards BOTH mutation + loop execution
     → blocks /status and inspection reads for full turn

NEW: 
  conversation_lock.lock()  ← serializes turns
  session.read/write()      ← only for immediate mutations
  
Result: /status can read session while turn is executing
```

### Pattern 3: Token Reset for Cancellation

```
cancel: std::sync::Mutex<CancellationToken>

execute_loop start:
  → new_token = CancellationToken::new()
  → *cancel.lock() = new_token
  → pass new_token to loop

execute_loop end:
  → token is moved, can't be un-cancelled
  → next turn gets a fresh token

Result: Ctrl+C stops current turn only; next turn still works
```

### Pattern 4: Lock-Free Budget

```
budget: Arc<IterationBudget>
  remaining: AtomicU32

execute_loop:
  while budget.try_consume() {  // CAS loop, no lock
      // tool invocation
  }
  
Result: No contention on hot path; runaway loops still capped
```

---

## Call Sites & Dependency Flow

```
┌──────────────────┐
│ edgecrab-cli     │
│ TUI main         │
└────────┬─────────┘
         │
         ├─→ AgentBuilder::new(model)
         │       .provider(provider)
         │       .tools(registry)
         │       .state_db(db)
         │       .build() → Agent
         │
         └─→ agent.chat(message)
                  │
                  ├─ lock conversation_lock
                  ├─ acquire provider snapshot
                  ├─ call execute_loop(provider, registry, messages, budget, cancel)
                  │   └─ in conversation.rs (ReAct loop)
                  │       ├─ llm.chat(messages, tools)
                  │       ├─ registry.dispatch(tool_name, args)
                  │       └─ while budget.try_consume() && !cancel
                  └─ unlock conversation_lock


┌──────────────────┐
│ edgecrab-gateway │
│ (messaging)      │
└────────┬─────────┘
         │
         ├─→ AgentBuilder (similar)
         │
         └─→ agent.chat_with_origin(message, "telegram")
                  │
                  ├─ set platform via config
                  ├─ call execute_loop
                  └─ deliver results via gateway_sender
```

---

## What Can Be Improved

### 1. **Reduce Lock Complexity**

**Current state:** 7 separate locks/mutexes across Agent fields.

```
config: RwLock
provider: RwLock
tool_registry: RwLock
gateway_sender: RwLock
session: RwLock
conversation_lock: Mutex
cancel: std::sync::Mutex<CancellationToken>
```

**Problem:** High cognitive load; hard to reason about deadlock freedom.

**Suggestion:** Consider a `DashMap` or `parking_lot::RwLock` for better contention patterns, or consolidate read-only fields into a single `Arc<Config>` snapshot strategy.

---

### 2. **Budget Type Discrepancy**

**Current state:** `IterationBudget` is `Arc<IterationBudget>`, but there's also `max_iterations: u32` in `AgentConfig`.

**Problem:** Two sources of truth; config change doesn't affect active budget.

**Suggestion:** Make budget mutable or derive it dynamically from config; or add explicit budget update API when config changes.

---

### 3. **StreamEvent Dispatch Clarity**

**Current state:** `StreamEvent` enum has ~15 variants, some with optional fields like `choices: Option<Vec<String>>`.

**Problem:** Receivers must pattern-match or ignore variants; no clear separation between token events (hot path) vs control events.

**Suggestion:** Split into `TextStreamEvent` (hot path) and `ControlEvent` (less frequent), or use trait objects for extensibility.

---

### 4. **Missing Session Isolation**

**Current state:** `session: RwLock<SessionState>` is shared across concurrent reads, but there's only one `conversation_lock: Mutex<()>`.

**Problem:** If two `/status` commands arrive during a single turn, they race to read session state; snapshot may be inconsistent.

**Suggestion:** Add versioning or snapshot epoch to session state, or guard session reads behind a generation counter.

---

### 5. **Weak Documentation on Provider Cloning**

**Current state:** Comments explain why we clone the provider Arc, but there's no explicit example or test.

**Problem:** New contributors may not understand why this is necessary; could lead to unsynchronized provider state.

**Suggestion:** Add inline example showing the race condition if we don't clone at loop start.

---

### 6. **No Metrics for Lock Contention**

**Current state:** No observability for lock wait times or failed CAS operations.

**Problem:** Can't detect lock contention in production.

**Suggestion:** Add optional metrics collection (e.g., `prometheus::Counter` for lock acquisitions, histograms for wait times).

---

### 7. **CancellationToken Wrapper Opacity**

**Current state:** `cancel: std::sync::Mutex<CancellationToken>` is wrapped, but the reason (token is one-way latch) could be clearer in code.

**Problem:** Maintainers might try to reuse the old token instead of replacing it.

**Suggestion:** Add `fn reset_cancel_token(&self)` explicit method with docs, or use a newtype `struct ReusableCancellationToken` that enforces reset-before-reuse.

---

### 8. **Compression State Machine Not Explicit**

**Current state:** Compression summary prefix is stored in session, but there's no enum for compression state (never compressed, compressed once, compressed N times, etc.).

**Problem:** Hard to debug compression history or reason about state transitions.

**Suggestion:** Add a `CompressionState` enum tracking compression history (timestamps, before/after token counts, etc.).

---

### 9. **Gateway Sender Optional Chaining**

**Current state:** `gateway_sender: RwLock<Option<Arc<dyn GatewaySender>>>` requires checking `is_some()` at every call site.

**Problem:** Boilerplate; risk of forgetting the check.

**Suggestion:** Use a default no-op `GatewaySender` instead of `Option`, or add a trait method `send_or_ignore()` that handles None gracefully.

---

### 10. **No Explicit Async Cancel Propagation**

**Current state:** Cancel token is passed to tools via `ToolContext`, but there's no top-level cancellation handler for in-flight API calls.

**Problem:** If an LLM provider doesn't respect cancellation, Ctrl+C may not interrupt immediately.

**Suggestion:** Add a wrapper around the provider that respects the cancel token, or document cancellation contract clearly.

---

### 11. **SessionState Tuple Fields for Origin Chat**

**Current state:** `origin_chat: Option<(String, String)>` uses a tuple for (platform, chat_id).

**Problem:** Unnamed fields; no type safety.

**Suggestion:** Create a `struct OriginChat { platform: String, chat_id: String }` or use a newtype for platform.

---

### 12. **Missing ToolContext Field Justification**

**Current state:** `tool_inventory()` constructs a full `ToolContext` just to query metadata.

**Problem:** Over-provisioning; most fields are unused.

**Suggestion:** Create a leaner `ToolQueryContext` for inventory queries, or add a public builder to `ToolContext` that makes defaults explicit.

---

### 13. **Test Coverage Gaps**

**Current state:** No explicit tests for:
- Hot-swap provider while executing a turn
- Budget exhaustion during tool loop
- Cancellation propagation to tools
- Session snapshot isolation under concurrent reads

**Suggestion:** Add integration tests in `tests/` directory covering these scenarios.

---

### 14. **No Explicit Error Recovery Path**

**Current state:** On `execute_loop` error, the error is returned but session state may be partially updated.

**Problem:** Hard to restore agent to a clean state after failure.

**Suggestion:** Add an explicit rollback mechanism or document recovery semantics clearly.

---

## Summary of Strengths

✅ **Builder pattern** prevents coupling between Agent construction and dependency setup.  
✅ **Separated locks** (conversation vs session) improve concurrency for read-only operations.  
✅ **Hot-swappable provider** enables model switching without restart.  
✅ **Lock-free budget** avoids contention on the hot path.  
✅ **Token reset strategy** ensures Ctrl+C only affects current turn.  
✅ **Rich StreamEvent** enables fine-grained UI feedback.  
✅ **Session snapshots** support non-blocking status queries.

## Summary of Improvements

⚠️ **Lock complexity** — 7 separate locks; hard to reason about.  
⚠️ **Budget type duplication** — Both in config and IterationBudget.  
⚠️ **StreamEvent variants** — 15+ variants; no hot-path separation.  
⚠️ **Session isolation** — No versioning under concurrent reads.  
⚠️ **Provider cloning docs** — Implicit; needs explicit example.  
⚠️ **Metrics gaps** — No lock contention observability.  
⚠️ **Cancel token opacity** — Wrapper reason not clear in code.  
⚠️ **Compression state** — No explicit enum for history.  
⚠️ **Gateway sender boilerplate** — `Option` everywhere.  
⚠️ **Tuple fields** — `(String, String)` instead of struct.  
⚠️ **ToolContext over-provisioning** — Full context for metadata query.  
⚠️ **Test coverage** — Missing hot-swap, cancellation, concurrent-read scenarios.  
⚠️ **Error recovery** — No explicit rollback path on failure.

---

## Next Steps

1. **Audit lock contention** — Profile under load; measure CAS failures in budget.
2. **Create snapshot examples** — Document hot-swap provider pattern with test cases.
3. **Split StreamEvent** — Separate token events from control events.
4. **Add type wrappers** — Replace tuples with newtypes (OriginChat, PlatformName).
5. **Write integration tests** — Cover hot-swap, cancellation, concurrent-read scenarios.
6. **Document cancellation contract** — Explicit semantics for provider interruption.
