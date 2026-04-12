# Agent.rs — Improvement Priority Matrix

## Visual Improvements Roadmap

```
┌─────────────────────────────────────────────────────────────────┐
│ IMPROVEMENT CATEGORIES BY IMPACT & EFFORT                       │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│ HIGH IMPACT / LOW EFFORT (DO FIRST)                            │
│ ─────────────────────────────────────────                       │
│                                                                 │
│   ✓ Add OriginChat struct                                       │
│     from: Option<(String, String)>                             │
│     to:   struct OriginChat { platform, chat_id }              │
│     effort: ~30 min refactoring                                │
│     impact: type safety, self-documenting code                 │
│                                                                 │
│   ✓ Document cancellation contract                              │
│     add: LLMProvider trait docs on interrupt semantics         │
│     add: test case: Ctrl+C during API call                     │
│     effort: ~1 hour (docs + test)                              │
│     impact: prevents mysterious hangs                          │
│                                                                 │
│   ✓ Add provider swap test                                      │
│     test: swap model while turn executing                      │
│     test: in-flight turn uses old provider                     │
│     test: next turn uses new provider                          │
│     effort: ~45 min                                             │
│     impact: prevents regression on hot-swap                    │
│                                                                 │
│ HIGH IMPACT / MEDIUM EFFORT (PLAN NEXT)                        │
│ ────────────────────────────────────────────                   │
│                                                                 │
│   ⚠ Add CompressionState enum                                  │
│     current: compression_summary_prefix: Option<String>       │
│     new: enum CompressionState {                               │
│          Never,                                                 │
│          Compressed {                                          │
│            timestamp: DateTime,                                │
│            before_tokens: u64,                                 │
│            after_tokens: u64,                                  │
│            count: u32,                                         │
│          }                                                      │
│        }                                                        │
│     effort: ~2 hours (refactoring + tests)                     │
│     impact: debuggable compression history                     │
│                                                                 │
│   ⚠ Split StreamEvent enum                                     │
│     current: 15 variants mixed (text + control)               │
│     new:  enum TextStreamEvent {                               │
│            Token { token: String },                            │
│          }                                                      │
│          enum ControlEvent {                                   │
│            ToolCall, Compression, ContextPressure, ...        │
│          }                                                      │
│     effort: ~1.5 hours (refactoring, receiver updates)        │
│     impact: clearer separation, hot-path optimization         │
│                                                                 │
│   ⚠ Add session snapshot versioning                             │
│     current: non-atomic multi-field read                       │
│     new: SessionState { generation: AtomicU64, ... }          │
│          snapshot checks generation before/after              │
│     effort: ~1 hour                                             │
│     impact: prevents stale snapshots under contention          │
│                                                                 │
│ MEDIUM IMPACT / HIGH EFFORT (BACKLOG)                          │
│ ──────────────────────────────────────────                     │
│                                                                 │
│   ◎ Lock consolidation strategy                                 │
│     current: 7 separate locks                                  │
│     options:                                                    │
│       A) DashMap for hot fields                                │
│       B) Arc<Config> immutable snapshot pattern               │
│       C) parking_lot::RwLock (better contention)              │
│     effort: 4-6 hours (significant refactor)                   │
│     impact: easier to reason about correctness                 │
│                                                                 │
│   ◎ Merge budget types                                          │
│     current: max_iterations in config + IterationBudget      │
│     new: single source of truth                                │
│     effort: 2 hours                                             │
│     impact: consistency, no duplicate state                    │
│                                                                 │
│   ◎ Default GatewaySender no-op                                │
│     current: Option<Arc<GatewaySender>> everywhere             │
│     new: Arc<dyn GatewaySender> with no-op default            │
│     effort: ~1.5 hours                                          │
│     impact: less boilerplate, cleaner API                      │
│                                                                 │
│ LOW IMPACT / VARIABLE EFFORT (OPTIONAL)                        │
│ ────────────────────────────────────────────                   │
│                                                                 │
│   ○ Lock contention metrics                                     │
│     add: prometheus counters for acquisitions                  │
│     add: histograms for wait times                              │
│     effort: 2-3 hours                                           │
│     impact: production observability                           │
│                                                                 │
│   ○ Lean ToolQueryContext                                       │
│     current: tool_inventory() builds full ToolContext         │
│     new: ToolQueryContext with minimal fields                 │
│     effort: 1 hour                                              │
│     impact: reduces over-provisioning                          │
│                                                                 │
│   ○ Error recovery rollback                                     │
│     add: explicit rollback path on failure                     │
│     effort: 2-3 hours                                           │
│     impact: cleaner error semantics                            │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Improvement Details by Category

### Category 1: Type Safety (HIGH PRIORITY)

#### Issue: Tuple Fields for Platform + Chat ID
```rust
// CURRENT (bad)
pub origin_chat: Option<(String, String)>,  // what do these mean?

// IMPROVED
pub struct OriginChat {
    pub platform: String,      // "telegram", "discord", etc.
    pub chat_id: String,       // Chat/channel ID
}

pub origin_chat: Option<OriginChat>,
```

**Impact:** Self-documenting, IDE autocomplete, type safety  
**Effort:** 30 min refactoring + grep/replace  
**Risk:** Low (internal type)

---

#### Issue: CompressionState History Lost
```rust
// CURRENT (bad)
pub compression_summary_prefix: Option<String>,  // When? How many times? Tokens saved?

// IMPROVED
pub enum CompressionState {
    Never,
    Compressed {
        first_time: DateTime<Utc>,
        last_time: DateTime<Utc>,
        total_compressions: u32,
        original_tokens: u64,
        compressed_tokens: u64,
        cumulative_tokens_saved: u64,
    }
}

pub compression: CompressionState,
```

**Impact:** Debuggable compression behavior, analytics  
**Effort:** 1.5-2 hours (refactor + session schema change)  
**Risk:** Medium (schema change, needs migration)

---

### Category 2: Event Stream Clarity (MEDIUM PRIORITY)

#### Issue: StreamEvent Has Too Many Unrelated Variants
```rust
// CURRENT: 15 variants, mixed concerns
pub enum StreamEvent {
    Text { token: String },                          // HOT PATH
    ToolCall { name, args },                         // HOT PATH
    ToolResult { name, result },                     // HOT PATH
    
    Compression { ... },                             // CONTROL
    ContextPressure { ... },                         // CONTROL
    SubAgentStart { ... },                           // CONTROL
    SubAgentFinish { ... },                          // CONTROL
    Clarify { question, choices },                   // CONTROL
    Approval { command, reason },                    // CONTROL
    SecretRequest { var_name, is_sudo },            // CONTROL
    Error(AgentError),                               // CONTROL
    Done,                                             // CONTROL
}

// IMPROVED: Separate hot-path from control
pub enum StreamEvent {
    // Hot path (tokens, tools) — receivers care about these
    Text { token: String },
    ToolCall { name: String, args: Value },
    ToolResult { name: String, result: String },
    
    // Control flow — less frequent, separate concerns
    Control(ControlEvent),
}

pub enum ControlEvent {
    Compression { old_len: usize, new_len: usize },
    ContextPressure { estimated: u64, threshold: u64 },
    SubAgentStart { task_index: usize, task_count: usize },
    SubAgentFinish { task_index: usize, status: String, ms: u64 },
    Clarify { question: String, choices: Option<Vec<String>> },
    Approval { command: String, reason: String },
    SecretRequest { var_name: String, is_sudo: bool },
    Done,
    Error(AgentError),
}
```

**Impact:** Cleaner receiver logic, potential hot-path optimization  
**Effort:** 1.5 hours (refactor + receiver updates)  
**Risk:** Medium (public API change)

---

### Category 3: Concurrency & Observability (HIGH PRIORITY)

#### Issue: No Lock Contention Visibility
```rust
// ADD: Metrics collection
pub struct Agent {
    // ... existing fields ...
    
    // Observability
    lock_acquisition_counts: Metrics {
        config_write: Counter,
        config_read: Counter,
        provider_write: Counter,
        provider_read: Counter,
        session_write: Counter,
        session_read: Counter,
        budget_cas: Counter,
        budget_cas_failures: Counter,
    },
    
    lock_wait_times: Metrics {
        conversation_lock: Histogram,
        session_lock_write: Histogram,
        session_lock_read: Histogram,
    },
}

// Usage (production)
self.metrics.budget_cas.inc();
if cas_retry_count > 0 {
    self.metrics.budget_cas_failures.inc_by(cas_retry_count as u64);
}
```

**Impact:** Detect contention bottlenecks in production  
**Effort:** 2-3 hours (add instrumentation, test)  
**Risk:** Low (optional feature)

---

#### Issue: Session Snapshot Not Atomic
```rust
// CURRENT (potentially stale)
pub async fn session_snapshot(&self) -> SessionSnapshot {
    let s = self.session.read().await;
    SessionSnapshot {
        user_turn_count: s.user_turn_count,      // read at T1
        api_call_count: s.api_call_count,        // read at T2 (may have changed!)
        session_tokens: s.total_tokens(),        // read at T3
    }
}

// IMPROVED: Versioning
pub struct SessionState {
    pub generation: AtomicU64,  // Incremented on every update
    pub messages: Vec<Message>,
    pub user_turn_count: u32,
    pub api_call_count: u32,
    pub session_tokens: u64,
    // ...
}

pub async fn session_snapshot(&self) -> SessionSnapshot {
    loop {
        let s = self.session.read().await;
        let gen_before = s.generation.load(Ordering::Acquire);
        
        let snapshot = SessionSnapshot {
            user_turn_count: s.user_turn_count,
            api_call_count: s.api_call_count,
            session_tokens: s.total_tokens(),
        };
        
        let gen_after = s.generation.load(Ordering::Acquire);
        
        if gen_before == gen_after {
            return snapshot;  // Consistent snapshot
        }
        // else: concurrent mutation detected, retry
    }
}
```

**Impact:** Guarantees consistent multi-field snapshots  
**Effort:** 1 hour  
**Risk:** Low (transparent optimization)

---

### Category 4: Code Organization (MEDIUM PRIORITY)

#### Issue: Too Many Locks, Hard to Reason About

```rust
// CURRENT: 7 separate lock fields
pub struct Agent {
    pub config: RwLock<AgentConfig>,           // ← 1
    pub provider: RwLock<Arc<...>>,            // ← 2
    pub tool_registry: RwLock<...>,            // ← 3
    pub gateway_sender: RwLock<Option<...>>,   // ← 4
    pub conversation_lock: Mutex<()>,          // ← 5
    pub session: RwLock<SessionState>,         // ← 6
    pub cancel: std::sync::Mutex<Token>,       // ← 7
}

// IMPROVED OPTION A: Consolidate immutable config
pub struct Agent {
    // Immutable config snapshot (cloned at builder)
    config: Arc<AgentConfig>,                  // ✓ No lock needed
    
    // Mutable state
    mutable: RwLock<AgentMutable>,             // ← Single lock for quick updates
    
    // Hot-path concurrency
    conversation_lock: Mutex<()>,              // ← Serializes turns
    budget: Arc<IterationBudget>,              // ← Lock-free atomic
    cancel: Mutex<CancellationToken>,          // ← Cancelable per turn
}

pub struct AgentMutable {
    provider: Arc<dyn LLMProvider>,
    tool_registry: Option<Arc<ToolRegistry>>,
    gateway_sender: Option<Arc<dyn GatewaySender>>,
    session: SessionState,
}

// IMPROVED OPTION B: Use DashMap for hot fields
pub struct Agent {
    config: Arc<AgentConfig>,
    provider: DashMap<String, Arc<dyn LLMProvider>>,  // Concurrent writes
    session: RwLock<SessionState>,                    // Stays, less contended
    // ...
}
```

**Impact:** Easier to reason about correctness  
**Effort:** 4-6 hours (significant refactor)  
**Risk:** High (structural change)

---

## Quick Reference: What to Fix First

```
Week 1 (High-Impact Wins):
├─ Add OriginChat struct              (30 min)
├─ Document cancellation contract     (1 hour)
├─ Add provider swap test             (45 min)
└─ Total: ~2 hours, significant improvement

Week 2 (Foundation Work):
├─ Add CompressionState enum          (2 hours)
├─ Split StreamEvent                  (1.5 hours)
├─ Add session snapshot versioning    (1 hour)
└─ Total: ~4.5 hours, cleaner foundation

Week 3+ (Optional Refactors):
├─ Lock contention metrics            (2-3 hours)
├─ Lock consolidation strategy        (4-6 hours)
├─ Merge budget types                 (2 hours)
└─ Default GatewaySender no-op        (1.5 hours)
```

---

## Validation Checklist

After implementing improvements:

- [ ] All public types are self-documenting (no raw tuples)
- [ ] Cancellation behavior documented in provider trait
- [ ] Integration tests cover hot-swap, interruption, snapshot isolation
- [ ] Lock contention metrics available (optional)
- [ ] Compression state is debuggable (dates, token counts)
- [ ] StreamEvent split doesn't break receivers
- [ ] Session snapshots are atomic or versioned
- [ ] Error recovery paths are explicit

