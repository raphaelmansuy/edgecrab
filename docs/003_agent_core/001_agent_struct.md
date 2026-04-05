# 003.001 — Agent Struct

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.001 Architecture](../002_architecture/001_system_architecture.md) | [→ 003.002 Conversation Loop](002_conversation_loop.md)
> **Source**: `edgecrab-core/src/agent.rs` — verified against real implementation

## 1. Agent Struct

The `Agent` is the core execution unit. All mutable fields use `RwLock` or `Arc` for hot-swapping and concurrent access.

```rust
// edgecrab-core/src/agent.rs

pub struct Agent {
    /// WHY RwLock: /model command swaps model name at runtime; all concurrent
    /// reads during the loop are unblocked, writes are rare.
    pub(crate) config: RwLock<AgentConfig>,

    /// WHY RwLock: /model command swaps LLM provider; conversation loop
    /// clones Arc at start, so in-flight conversations are unaffected.
    pub(crate) provider: RwLock<Arc<dyn LLMProvider>>,

    pub(crate) state_db: Option<Arc<SessionDb>>,
    pub(crate) tool_registry: Option<Arc<ToolRegistry>>,

    /// WHY on Agent: All tool invocations in the same session share the
    /// process namespace. Agent lifetime == session lifetime.
    pub(crate) process_table: Arc<ProcessTable>,

    pub(crate) session: RwLock<SessionState>,
    pub(crate) budget: Arc<IterationBudget>,

    /// WHY Mutex<CancellationToken>: Token is one-way latch (can't un-cancel).
    /// Replaced with fresh token at each conversation start so Ctrl+C only
    /// stops the current turn, not all future turns.
    pub(crate) cancel: std::sync::Mutex<CancellationToken>,
}
```

```
┌─────────────────────────────────────────────────────┐
│                     Agent                           │
│                                                     │
│  config: RwLock<AgentConfig>     ◄── /model swap    │
│  provider: RwLock<Arc<LLMProvider>>  ◄── /model     │
│  state_db: Option<Arc<SessionDb>>                   │
│  tool_registry: Option<Arc<ToolRegistry>>           │
│  process_table: Arc<ProcessTable>                   │
│  session: RwLock<SessionState>                      │
│  budget: Arc<IterationBudget>                       │
│  cancel: Mutex<CancellationToken>                   │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │  .chat("hi")        → simple String          │   │
│  │  .run_conversation() → ConversationResult    │   │
│  │  .interrupt()        → cancel current turn   │   │
│  │  .set_model()        → hot-swap provider     │   │
│  │  .new_session()      → reset for next conv   │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

## 2. AgentConfig

Immutable per-agent configuration (subset of `AppConfig` relevant to the conversation loop):

```rust
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub model: String,                        // default: "anthropic/claude-opus-4.6"
    pub max_iterations: u32,                  // default: 90
    pub enabled_toolsets: Vec<String>,
    pub disabled_toolsets: Vec<String>,
    pub streaming: bool,                      // default: true
    pub temperature: Option<f32>,
    pub platform: Platform,                   // Cli | Telegram | Discord | ...
    pub api_mode: ApiMode,
    pub session_id: Option<String>,
    pub quiet_mode: bool,
    pub save_trajectories: bool,
    pub skip_context_files: bool,
    pub skip_memory: bool,
    pub reasoning_effort: Option<String>,

    /// Personality from config.display.personality → appended to system prompt
    pub personality_addon: Option<String>,

    /// Model config for routing (base_url, api_key_env, smart routing)
    pub model_config: crate::config::ModelConfig,

    /// Skills config — disabled skills, platform-specific disabled
    pub skills_config: crate::config::SkillsConfig,

    // Delegation runtime controls (from AppConfig.delegation)
    pub delegation_enabled: bool,             // default: true
    pub delegation_model: Option<String>,
    pub delegation_provider: Option<String>,
    pub delegation_max_subagents: u32,        // default: 3
    pub delegation_max_iterations: u32,       // default: 50

    /// Gateway origin — (platform_name, chat_id).
    /// Enables cron jobs to target the correct delivery channel.
    /// None for CLI / cron / test sessions.
    pub origin_chat: Option<(String, String)>,

    pub browser: crate::config::BrowserConfig,
    pub checkpoints_enabled: bool,            // default: true
    pub checkpoints_max_snapshots: u32,       // default: 50
}
```

## 3. SessionState

Per-conversation mutable state, protected by `RwLock`:

```rust
#[derive(Default)]
pub struct SessionState {
    /// Unique session identifier — set once at conversation start,
    /// persisted to SQLite at loop end for session search/history.
    pub session_id: Option<String>,
    pub messages: Vec<Message>,
    pub cached_system_prompt: Option<String>,
    pub user_turn_count: u32,
    pub api_call_count: u32,
    pub session_input_tokens: u64,
    pub session_output_tokens: u64,
    pub session_cache_read_tokens: u64,
    pub session_cache_write_tokens: u64,
    pub session_reasoning_tokens: u64,
    pub session_tool_call_count: u32,
}
```

> **Note**: Memory, todo, and honcho state are **not** stored in `SessionState`. Memory lives on disk (`~/.edgecrab/memories/`), todos are managed by the `manage_todo_list` tool, and honcho context is fetched per-turn via the HonchoClient.

## 4. IterationBudget

Lock-free atomic iteration counter that prevents runaway tool loops:

```rust
pub struct IterationBudget {
    remaining: AtomicU32,
    max: u32,
}

impl IterationBudget {
    pub fn new(max: u32) -> Self {
        Self {
            remaining: AtomicU32::new(max),
            max,
        }
    }

    /// Try to consume one iteration. Returns false when exhausted.
    /// Uses CAS loop — no mutex contention on the hot path.
    pub fn try_consume(&self) -> bool {
        loop {
            let current = self.remaining.load(Ordering::Relaxed);
            if current == 0 {
                return false;
            }
            if self.remaining
                .compare_exchange_weak(
                    current, current - 1,
                    Ordering::Relaxed, Ordering::Relaxed,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn remaining(&self) -> u32 { ... }
    pub fn max(&self) -> u32 { self.max }
    pub fn used(&self) -> u32 { self.max - self.remaining() }
    pub fn reset(&self) { self.remaining.store(self.max, Ordering::Relaxed); }
}
```

## 5. Builder Pattern

```rust
AgentBuilder::new("anthropic/claude-opus-4.6")
    .provider(provider)          // Arc<dyn LLMProvider>
    .tools(registry)             // Arc<ToolRegistry>
    .state_db(db)                // Arc<SessionDb>
    .config(cfg)                 // AgentConfig
    .build()?  →  Agent
```

## 6. Public API

```rust
impl Agent {
    /// Simple interface — returns final response string
    pub async fn chat(&self, message: &str) -> Result<String>

    /// Streaming interface — tokens sent via UnboundedSender<StreamEvent>
    pub async fn chat_streaming(
        &self,
        message: &str,
        tx: UnboundedSender<StreamEvent>,
    ) -> Result<ConversationResult>

    /// Full interface — returns structured ConversationResult.
    /// Internally delegates to execute_loop() (see 003.002).
    pub async fn run_conversation(
        &self,
        user_message: &str,
        system_message: Option<&str>,
        conversation_history: Option<Vec<Message>>,
        task_id: Option<&str>,
    ) -> Result<ConversationResult>

    /// Cancel the current in-flight conversation (replaces cancel token)
    pub fn interrupt(&self)

    /// Reset session for a new conversation
    pub async fn new_session(&self) -> Result<()>

    /// Hot-swap the LLM model at runtime (from /model command)
    pub async fn set_model(&self, model: &str, provider: Arc<dyn LLMProvider>)
}
```

## 7. ConversationResult

```rust
pub struct ConversationResult {
    pub final_response: String,
    pub messages: Vec<Message>,
    pub session_id: String,
    pub api_calls: u32,
    pub interrupted: bool,
    pub model: String,
    pub usage: Usage,
    pub cost: Cost,
}
```

## 8. Key Config Defaults

| Key | Default | Override |
|-----|---------|----------|
| `model` | `anthropic/claude-opus-4.6` | `--model` flag or config |
| `max_iterations` | `90` | `config.model.max_iterations` |
| `streaming` | `true` | `config.model.streaming` |
| `platform` | `Platform::Cli` | Set by gateway per-platform |
| `delegation_enabled` | `true` | `config.delegation.enabled` |
| `delegation_max_subagents` | `3` | `config.delegation.max_subagents` |
| `delegation_max_iterations` | `50` | `config.delegation.max_iterations` |
| `checkpoints_enabled` | `true` | `config.checkpoints.enabled` |
| `checkpoints_max_snapshots` | `50` | `config.checkpoints.max_snapshots` |
| `skip_context_files` | `false` | `EDGECRAB_SKIP_CONTEXT_FILES` env |
| `skip_memory` | `false` | `EDGECRAB_SKIP_MEMORY` env |
| `save_trajectories` | `false` | `EDGECRAB_SAVE_TRAJECTORIES` env |

## 9. Design Decisions

| Aspect | Python agent pattern | EdgeCrab |
|--------|---------------------|----------|
| Thread safety | Manual locks, fragile | `Send + Sync` enforced at compile time |
| Iteration budget | `threading.Lock` | `AtomicU32` (lock-free CAS) |
| Event loop | Workaround bridge functions | Native async, no bridges |
| Builder | `__init__` with 50+ kwargs | Type-safe builder pattern |
| Cancellation | `threading.Event` | `CancellationToken` (structured, replaceable) |
| State isolation | Mixed mutable state | `RwLock<SessionState>` |
| Model hot-swap | Restart required | `RwLock` on config + provider |
