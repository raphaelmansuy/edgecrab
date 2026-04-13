# Agent.rs Documentation Summary

## Files Created

1. **`specs/01-agent.md`** (27.9 KB)
   - Complete agent architecture documentation
   - Data structure overview with ASCII diagrams
   - Public API reference
   - Design patterns explained
   - 14 identified improvement areas
   - Strengths & weaknesses summary

2. **`specs/02-agent-diagrams.md`** (5.9 KB)
   - Focused ASCII diagrams
   - Lock contention scenario
   - Hot-swap provider pattern
   - CAS loop visualization
   - Token reset mechanics
   - Session snapshot isolation

---

## Quick Reference

### File Stats
- **Source:** `crates/edgecrab-core/src/agent.rs`
- **Lines:** 2,594
- **Purpose:** Session orchestration, model hot-swap, dependency injection

### Core Types

| Type | Purpose |
|------|---------|
| `Agent` | Main conversation orchestrator |
| `AgentConfig` | Runtime configuration (193 fields!) |
| `SessionState` | Live conversation state |
| `IterationBudget` | Lock-free iteration counter |
| `AgentBuilder` | Fluent constructor |
| `StreamEvent` | Granular conversation events |

### Key Mechanisms

| Mechanism | Pattern |
|-----------|---------|
| **Hot-swap provider** | Arc + RwLock snapshot + delayed effect |
| **Lock separation** | conversation_lock (thin) vs session lock (data) |
| **Iteration budget** | Atomic CAS loop (no Mutex) |
| **Token reset** | Replace token each turn (one-way latch workaround) |
| **Compression** | Optional LLM summarization + fallback structural |

---

## Top 5 Improvements

### 1. **Reduce Lock Complexity** (HIGH IMPACT)
**Current:** 7 separate locks across Agent  
**Impact:** Hard to reason about deadlock freedom  
**Suggestion:** DashMap or Arc<Config> snapshot strategy

### 2. **Split StreamEvent** (MEDIUM)
**Current:** 15 variants, hot-path & control mixed  
**Impact:** Receivers must pattern-match everything  
**Suggestion:** Separate TextStreamEvent vs ControlEvent

### 3. **Type-Safe Origin Chat** (LOW)
**Current:** `Option<(String, String)>` tuple  
**Impact:** Unnamed fields, no type safety  
**Suggestion:** `struct OriginChat { platform: String, chat_id: String }`

### 4. **Add Compression State History** (MEDIUM)
**Current:** Just `compression_summary_prefix` string  
**Impact:** Can't debug compression history  
**Suggestion:** `enum CompressionState` with timestamps, token counts

### 5. **Explicit Cancellation Contract** (HIGH)
**Current:** No documentation on provider interruption  
**Impact:** Ctrl+C may not work for all providers  
**Suggestion:** Wrapper around provider respecting cancel token, clear docs

---

## Architecture Highlights

```
Agent (Session Orchestrator)
├── config: RwLock (model, iteration limit, toolsets, ...)
├── provider: RwLock (LLM, hot-swappable)
├── session: RwLock (messages, metrics, history)
├── conversation_lock: Mutex (serializes turns, brief hold)
├── tool_registry: RwLock (tools, hot-swappable)
├── process_table: Arc (background process namespace)
├── budget: Arc<IterationBudget> (lock-free, CAS)
├── cancel: Mutex<CancellationToken> (reset each turn)
├── gc_cancel: CancellationToken (long-lived, background GC)
└── todo_store: Arc (survives compression)
```

---

## Lock Separation Insight

The **key innovation** is separating conversation_lock from session lock:

**Old design** (problematic):
```
session.write()  ← Holds for entire turn (2650ms!)
  ├─ prompt assembly
  ├─ API call
  ├─ tool execution
  └─ compression
```
Result: `/status` command blocks for 2.6 seconds ✗

**New design** (fixed):
```
conversation_lock  ← Thin, released immediately
  ├─ read config snapshot
  ├─ read provider snapshot
  └─ release

execute_loop  ← No lock held (2650ms execution)
  └─ brief session.write() for each message append

/status can read session anytime ✓
```

---

## Concurrency Model

- **Hot-swap provider:** Arc snapshot at loop start prevents in-flight interference
- **Lock-free budget:** CAS loop avoids contention on hot path
- **Token reset:** Fresh token each turn (workaround for one-way latch)
- **Session reads:** Can occur during turn execution (read lock only)
- **Compression:** Optional LLM call with structural fallback

---

## What's Missing in Source Code

1. **Provider cloning race condition example** — Implicit in comments, needs explicit test
2. **Cancellation contract** — No documentation of provider interruption semantics
3. **Session snapshot versioning** — Potential inconsistency under concurrent reads
4. **Lock contention metrics** — No observability for CAS failures, lock waits
5. **Error recovery path** — No explicit rollback on execute_loop failure
6. **Hot-swap integration test** — No test for model swap during turn
7. **Budget exhaustion scenario** — No test for runaway tool loops

---

## Files Generated

```
specs/
├── 01-agent.md              ← Main documentation (27.9 KB)
│   ├─ Executive summary
│   ├─ Data structure overview with ASCII
│   ├─ AgentConfig details
│   ├─ SessionState details
│   ├─ Public interface (30+ methods)
│   ├─ Builder pattern
│   ├─ StreamEvent types
│   ├─ 7 design patterns explained
│   ├─ Call sites & dependency flow
│   ├─ 14 improvement areas (detailed)
│   └─ Strengths & weaknesses summary
│
└── 02-agent-diagrams.md     ← ASCII diagrams (5.9 KB)
    ├─ Agent initialization
    ├─ Concurrent access patterns
    ├─ State machine (8 states)
    ├─ Hot-swap timeline
    ├─ Lock contention OLD vs NEW
    ├─ CAS loop mechanics
    ├─ Token reset sequence
    └─ Snapshot isolation race
```

---

## How to Use These Docs

1. **New contributor:** Read 01-agent.md Executive Summary + Data Structure
2. **Adding feature:** Consult Public Interface section for relevant methods
3. **Understanding concurrency:** Study Design Patterns (7 detailed explainers)
4. **Debugging lock issues:** Reference 02-agent-diagrams.md Lock Contention
5. **Proposing changes:** Check Improvements section (14 areas with suggestions)

---

## Validation

✅ All major structures documented  
✅ Call sites traced from CLI to execute_loop  
✅ Concurrency patterns explained with race conditions  
✅ Hot-swap provider lifecycle traced  
✅ Lock separation rationale justified  
✅ Atomic operations explained (CAS loop, token reset)  
✅ Improvements grounded in current code  

---

## Next Steps

1. Implement type-safe improvements (OriginChat struct, CompressionState enum)
2. Add integration tests for hot-swap, cancellation, snapshot isolation
3. Profile lock contention under load
4. Document cancellation contract for provider implementations
5. Consider lock reduction strategy (consolidation, DashMap, Arc snapshot)

