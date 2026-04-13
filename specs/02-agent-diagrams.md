# Agent Architecture — Detailed ASCII Diagrams (Part 1)

## 1. Agent Initialization & Lock States

```
┌─────────────────────────────────────────────────────────────────┐
│ Agent Initialization (AgentBuilder)                             │
├─────────────────────────────────────────────────────────────────┤
│  AgentBuilder::new("model")                                    │
│    .provider(provider)                                         │
│    .tools(registry)                                            │
│    .state_db(db)                                               │
│    .build()                                                    │
│    ─────→ Agent {                                              │
│             config: RwLock(AgentConfig),                       │
│             provider: RwLock(Arc<dyn LLMProvider>),           │
│             session: RwLock(SessionState { messages: [] }),    │
│             conversation_lock: Mutex(()),                      │
│             budget: Arc(IterationBudget { max: 90 }),         │
│             cancel: Mutex(CancellationToken),                  │
│            }                                                    │
└─────────────────────────────────────────────────────────────────┘
```

## 2. Concurrent Access Patterns

```
Main Thread (TUI)
  ├─ agent.chat("hello")
  │   ├─ acquire conversation_lock
  │   ├─ let provider_snapshot = Arc::clone(provider)
  │   ├─ execute_loop(provider_snapshot, ...)
  │   │   └─ while budget.try_consume() && !cancel { ... }
  │   └─ release conversation_lock

Status Thread (concurrent)
  ├─ agent.session_snapshot()
  │   ├─ let session = session.read()  [OK, parallel!]
  │   └─ return SessionSnapshot
  │
  └─ [No lock contention with main thread]

Model Swap Thread (rare)
  ├─ agent.swap_model(new_model, new_provider)
  │   ├─ config.write() ← brief
  │   ├─ provider.write() ← brief
  │   └─ [In-flight turn unaffected - uses snapshot]
  │
  └─ [Safe: clone at loop start isolates]
```

## 3. Conversation Loop State Machine

```
execute_loop() State Machine:

START_ITERATION
  ├─ budget.try_consume() [CAS, atomic]
  ├─ check cancel token
  └─ check if compression needed

CALL_PROVIDER
  ├─ provider.chat(messages, tools)
  ├─ stream tokens via StreamEvent::Text
  └─ parse response.tool_calls

DISPATCH_TOOLS or RETURN_RESPONSE
  ├─ if has tool calls:
  │   ├─ for each call:
  │   │   ├─ tool_result = registry.dispatch(name, args)
  │   │   ├─ append to session.messages
  │   │   └─ emit StreamEvent::ToolResult
  │   └─ [LOOP AGAIN]
  │
  └─ if no tool calls:
      └─ return ConversationResult { final_response, usage, ... }

EXIT CONDITIONS:
  ├─ budget.try_consume() = false → TooManyIterations
  ├─ cancel token set → Interrupted
  ├─ no tool calls → Success
  └─ provider/tool error → Error
```

## 4. Hot-Swap Provider Pattern

```
T0: agent.chat() starts
    provider_snapshot = Arc::clone(&self.provider.read())
    execute_loop starts with snapshot

    T0.5: [During iteration 1]
          User: /model openai/gpt-4o
          self.provider.write() ← swap!
          [But iteration 1 still uses OLD snapshot]

    T0.7: [Iteration 2]
          provider.chat() ← uses OLD snapshot
          (still old provider)

    T1: agent.chat() next turn
        provider_snapshot = Arc::clone(&self.provider.read())
        execute_loop uses NEW provider ✓
```

## 5. Lock Contention Problem vs Solution

```
OLD (Problematic):
  session.write() holds lock for 2650ms
    ├─ prompt assembly (50ms)
    ├─ API call (2000ms)
    ├─ tool dispatch (100ms)
    └─ compression (500ms)
  
  /status command blocks entire time! ✗

NEW (Fixed):
  conversation_lock held only briefly
    ├─ config.read() → 0.1ms
    ├─ provider snapshot → 0.1ms
    └─ release immediately
  
  execute_loop runs without lock!
  
  /status can read session anytime ✓
```

## 6. Iteration Budget — Lock-Free CAS

```
pub struct IterationBudget {
    remaining: AtomicU32,
    max: u32,
}

try_consume():
  loop {
    current = remaining.load()
    if current == 0 { return false; }
    
    CAS(current → current - 1)
      ✓ success: return true
      ✗ failed: retry loop
  }

Why CAS?
  ├─ Atomic check + update
  ├─ No Mutex (lock-free hot path)
  ├─ Multiple threads can race safely
  └─ Each unit of work accounted
```

## 7. Cancellation Token Reset

```
pub cancel: std::sync::Mutex<CancellationToken>

Turn 1:
  execute_loop start:
    *cancel.lock() = CancellationToken::new()
  [User Ctrl+C]
    token.cancel() ← flag set
  execute_loop end

Turn 2:
  execute_loop start:
    *cancel.lock() = CancellationToken::new() ← FRESH!
    is_cancelled() = false ✓
  [Proceeds normally]
```

## 8. Session Snapshot Isolation

```
Potential race: non-atomic field reads

Main thread updates:
  user_turn_count = 5
  api_call_count = 4
  session_tokens = 60000

Concurrent snapshot read:
  read user_turn_count = 5 (at time T1)
  read api_call_count = 3 (at time T2, after T1!)
  read session_tokens = 60000 (at time T3)
  
  Result: Inconsistent snapshot (api_call is stale)
  
Severity: LOW
  ├─ Stale, not corrupt
  ├─ Eventually consistent
  └─ Only visible with rapid polling
```

