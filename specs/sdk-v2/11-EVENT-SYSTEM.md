# 11 — Event System: Real-Time Agent Observability

> **Build the best AI agent UI ever.**
>
> EdgeCrab's event system exposes every heartbeat of the agent lifecycle — token streaming, tool orchestration, sub-agent delegation, cost tracking, context pressure, and interactive approvals — through a single, typed event bus. No other agent SDK gives UI developers this level of visibility.

---

## Table of Contents

1. [Why a Dedicated Event System](#1-why-a-dedicated-event-system)
2. [Architecture Overview](#2-architecture-overview)
3. [Complete Event Taxonomy](#3-complete-event-taxonomy)
4. [Event Flow & Lifecycle](#4-event-flow--lifecycle)
5. [Transport Layers](#5-transport-layers)
6. [SDK Integration Patterns](#6-sdk-integration-patterns)
7. [Web UI Architecture](#7-web-ui-architecture)
8. [Interactive Events (Human-in-the-Loop)](#8-interactive-events-human-in-the-loop)
9. [State Machine for UI Reducers](#9-state-machine-for-ui-reducers)
10. [Event Recording, Replay & Time-Travel Debugging](#10-event-recording-replay--time-travel-debugging)
11. [Metrics, Tracing & Analytics](#11-metrics-tracing--analytics)
12. [Performance & Backpressure](#12-performance--backpressure)
13. [Competitor Comparison](#13-competitor-comparison)
14. [Complete Dashboard Example](#14-complete-dashboard-example)
15. [Honest Assessment](#15-honest-assessment)

---

## 1. Why a Dedicated Event System

Most agent SDKs treat observability as an afterthought — a log file, a callback, or a third-party integration you bolt on. EdgeCrab treats it as **architecture**.

**The problem with existing approaches:**

| Approach | SDK Examples | Limitation |
|----------|-------------|------------|
| Logging only | LangChain, CrewAI | Unstructured text; can't drive UI |
| Trace/span model | OpenAI Agents SDK, Pydantic AI | Post-hoc analysis only; no real-time streaming; requires external backend (Logfire, Datadog) |
| Callback hooks | Google ADK | Imperative, hard to compose; no event replay |
| No observability | Most open-source agents | Black box — impossible to debug or build UIs |

**EdgeCrab's approach:**

```
+------------------------------------------------------------------+
|                    The EdgeCrab Difference                        |
+------------------------------------------------------------------+
|                                                                  |
|  Other SDKs:     Agent ──→ Text Output                           |
|                           (black box)                            |
|                                                                  |
|  EdgeCrab:       Agent ──→ StreamEvent Bus ──→ Rich UI           |
|                    │                             │               |
|                    │    21 typed event variants   │               |
|                    │    Real-time streaming       │               |
|                    │    Interactive (bi-dir)      │               |
|                    │    Correlation IDs           │               |
|                    │    Duration tracking         │               |
|                    │    Error attribution         │               |
|                    │    Sub-agent visibility      │               |
|                    │    Cost-per-token            │               |
|                    │    Context pressure          │               |
|                    │                             │               |
|                    └──────── ANY UI ─────────────┘               |
|                    TUI  │  Web  │  Mobile │  IDE │  Dashboard    |
+------------------------------------------------------------------+
```

**Key insight:** The event system is not a debugging tool — it's the **primary interface** between the agent runtime and any UI. The TUI, gateway, and ACP adapter are all just different consumers of the same event bus.

---

## 2. Architecture Overview

### 2.1 Producer → Bus → Consumer Pipeline

```
+------------------------------------------------------------------+
|                       Event Architecture                         |
+------------------------------------------------------------------+
|                                                                  |
|  ┌──────────────────────────────────────────────────────────┐    |
|  │                   PRODUCERS (Rust Core)                   │    |
|  │                                                          │    |
|  │  ┌────────────┐  ┌────────────┐  ┌────────────────────┐ │    |
|  │  │ ReAct Loop │  │ Tool       │  │ Sub-Agent Runner   │ │    |
|  │  │            │  │ Dispatcher │  │                    │ │    |
|  │  │ • Token    │  │ • ToolExec │  │ • SubAgentStart    │ │    |
|  │  │ • Reasoning│  │ • Progress │  │ • SubAgentFinish   │ │    |
|  │  │ • Done     │  │ • ToolDone │  │ • SubAgentToolExec │ │    |
|  │  │ • Error    │  │ • Approval │  │ • SubAgentReasoning│ │    |
|  │  └─────┬──────┘  └─────┬──────┘  └─────────┬──────────┘ │    |
|  │        │               │                    │            │    |
|  │        └───────────────┼────────────────────┘            │    |
|  │                        │                                 │    |
|  │                        ▼                                 │    |
|  │        ┌───────────────────────────────┐                 │    |
|  │        │  UnboundedSender<StreamEvent>  │                 │    |
|  │        │  (tokio mpsc channel)          │                 │    |
|  │        └──────────────┬────────────────┘                 │    |
|  └───────────────────────│──────────────────────────────────┘    |
|                          │                                       |
|                          ▼                                       |
|  ┌──────────────────────────────────────────────────────────┐    |
|  │                    EVENT BUS                              │    |
|  │                                                          │    |
|  │  Fan-out to all registered consumers:                    │    |
|  │                                                          │    |
|  │  ┌─────────┐  ┌──────────┐  ┌────────┐  ┌────────────┐ │    |
|  │  │ TUI     │  │ Gateway  │  │  ACP   │  │ WebSocket  │ │    |
|  │  │ (ratatui│  │ Stream   │  │ (JSON- │  │ Server     │ │    |
|  │  │  render)│  │ Consumer │  │  RPC)  │  │ (future)   │ │    |
|  │  └─────────┘  └──────────┘  └────────┘  └────────────┘ │    |
|  └──────────────────────────────────────────────────────────┘    |
+------------------------------------------------------------------+
```

### 2.2 Channel Mechanics (Rust)

```rust
// Producer side (agent.rs)
let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

// Agent emits events during execution
tx.send(StreamEvent::Token("Hello".into()))?;
tx.send(StreamEvent::ToolExec {
    tool_call_id: "call_abc123".into(),
    name: "file_read".into(),
    args_json: r#"{"path": "src/main.rs"}"#.into(),
})?;

// Consumer side (any UI)
while let Some(event) = rx.recv().await {
    match event {
        StreamEvent::Token(text) => append_to_response(text),
        StreamEvent::ToolExec { name, .. } => show_tool_spinner(name),
        StreamEvent::ToolDone { duration_ms, .. } => hide_spinner(duration_ms),
        StreamEvent::Done => break,
        _ => { /* handle other events */ }
    }
}
```

### 2.3 Design Principles

| Principle | Implementation |
|-----------|----------------|
| **Typed, not stringly** | Every event is a Rust enum variant with named fields — no JSON parsing in hot paths |
| **Correlation by ID** | `tool_call_id` links ToolExec → ToolProgress → ToolDone; `task_index` links SubAgent* events |
| **Duration built-in** | `duration_ms: u64` on ToolDone and SubAgentFinish — no client-side timing needed |
| **Error attribution** | `is_error: bool` on ToolDone tells you exactly which tool failed |
| **Interactive** | Clarify, Approval, SecretRequest carry `oneshot::Sender` — UI responds, agent continues |
| **Non-fatal** | Consumer errors never crash the agent — events are fire-and-forget on the producer side |
| **Backpressure-free** | `UnboundedSender` means the agent never blocks waiting for the UI — events are buffered |
| **Zero-cost when unused** | If no `tx` is provided, no events are allocated or sent |

---

## 3. Complete Event Taxonomy

### 3.1 All 21 StreamEvent Variants

Every event the agent can emit, documented with exact Rust field types:

#### Response Events

```rust
/// A chunk of the agent's text response
Token(String)

/// A chunk of the agent's internal reasoning (thinking)
Reasoning(String)

/// The agent has finished responding — conversation turn complete
Done

/// An error occurred during the agent run
Error(String)
```

#### Tool Lifecycle Events

```rust
/// A tool invocation has started
ToolExec {
    tool_call_id: String,   // Unique ID from the LLM (e.g. "call_abc123")
    name: String,           // Tool name (e.g. "file_read", "terminal")
    args_json: String,      // Raw JSON arguments string
}

/// Live progress update from a running tool
ToolProgress {
    tool_call_id: String,   // Links to the originating ToolExec
    name: String,           // Tool name
    message: String,        // Progress message (e.g. "Downloading 45%...")
}

/// A tool invocation has completed
ToolDone {
    tool_call_id: String,   // Links to the originating ToolExec
    name: String,           // Tool name
    args_json: String,      // Original arguments (for UI display)
    result_preview: Option<String>,  // Truncated result (first ~500 chars)
    duration_ms: u64,       // Wall-clock execution time
    is_error: bool,         // Whether the tool returned an error
}
```

#### Sub-Agent Events

```rust
/// A sub-agent delegation task has started
SubAgentStart {
    task_index: usize,      // 0-based index in the batch
    task_count: usize,      // Total tasks in this delegation
    goal: String,           // Natural language goal description
}

/// Sub-agent is thinking/reasoning
SubAgentReasoning {
    task_index: usize,
    task_count: usize,
    text: String,           // Reasoning content
}

/// Sub-agent invoked a tool
SubAgentToolExec {
    task_index: usize,
    task_count: usize,
    name: String,           // Tool name
    args_json: String,      // Tool arguments
}

/// Sub-agent completed its task
SubAgentFinish {
    task_index: usize,
    task_count: usize,
    status: String,         // "success" | "error" | "timeout"
    duration_ms: u64,       // Total sub-agent execution time
    summary: String,        // Sub-agent's final response summary
    api_calls: u32,         // Number of LLM API calls made
    model: Option<String>,  // Model used (may differ from parent)
}
```

#### Interactive Events (Bidirectional)

```rust
/// Agent needs clarification from the user
Clarify {
    question: String,                  // The question to ask
    choices: Option<Vec<String>>,      // Optional preset choices
    response_tx: oneshot::Sender<String>,  // Send answer back to agent
}

/// Agent needs approval for a dangerous command
Approval {
    command: String,                   // Short command description
    full_command: String,              // Full command text
    reasons: Vec<String>,             // Why this needs approval
    response_tx: oneshot::Sender<ApprovalChoice>,  // Send decision
}

/// Agent needs a secret/credential
SecretRequest {
    var_name: String,                  // Environment variable name
    prompt: String,                    // Human-readable prompt
    is_sudo: bool,                     // Whether this is a sudo password
    response_tx: oneshot::Sender<String>,  // Send secret back
}
```

#### System Events

```rust
/// Lifecycle hook emitted by the gateway
HookEvent {
    event: String,           // Event name (e.g. "tool:post", "session:start")
    context_json: String,    // JSON payload with hook context
}

/// Context window pressure warning
ContextPressure {
    estimated_tokens: u64,   // Current estimated token count
    threshold_tokens: u64,   // Compression threshold
}
```

### 3.2 ApprovalChoice Enum

```rust
pub enum ApprovalChoice {
    Once,     // Allow this single invocation
    Session,  // Allow for the rest of this session
    Always,   // Persist approval to disk (never ask again)
    Deny,     // Refuse execution
}
```

### 3.3 Event Categories Summary

```
+------------------------------------------------------------------+
|                     Event Categories                             |
+------------------------------------------------------------------+
|                                                                  |
|  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ |
|  │   STREAMING      │  │   TOOL LIFECYCLE │  │  SUB-AGENTS     │ |
|  │                  │  │                  │  │                 │ |
|  │  Token           │  │  ToolExec        │  │  SubAgentStart  │ |
|  │  Reasoning       │  │  ToolProgress    │  │  SubAgentReason │ |
|  │  Done            │  │  ToolDone        │  │  SubAgentTool   │ |
|  │  Error           │  │                  │  │  SubAgentFinish │ |
|  └─────────────────┘  └─────────────────┘  └─────────────────┘ |
|                                                                  |
|  ┌─────────────────┐  ┌─────────────────┐                       |
|  │  INTERACTIVE     │  │  SYSTEM          │                      |
|  │  (bidirectional) │  │                  │                      |
|  │                  │  │  HookEvent       │                      |
|  │  Clarify         │  │  ContextPressure │                      |
|  │  Approval        │  │                  │                      |
|  │  SecretRequest   │  │                  │                      |
|  └─────────────────┘  └─────────────────┘                       |
+------------------------------------------------------------------+
```

---

## 4. Event Flow & Lifecycle

### 4.1 Single Tool Execution Timeline

```
Time ──────────────────────────────────────────────────────────→

Agent Loop                        Tool Dispatcher
    │                                  │
    │  ToolExec ─────────────────────→ │
    │  {call_id: "c1",                 │
    │   name: "terminal",              │ ← tool starts
    │   args: "{\"cmd\":\"ls\"}"}      │
    │                                  │
    │  ToolProgress ←──────────────── │
    │  {call_id: "c1",                 │ ← live output
    │   message: "Running: ls"}        │
    │                                  │
    │  ToolProgress ←──────────────── │
    │  {call_id: "c1",                 │ ← more output
    │   message: "file1.rs file2.rs"}  │
    │                                  │
    │  ToolDone ←─────────────────── │
    │  {call_id: "c1",                 │ ← tool complete
    │   duration_ms: 42,               │
    │   is_error: false,               │
    │   result_preview: "file1.rs..."}│
    │                                  │
```

### 4.2 Full ReAct Loop with Sub-Agents

```
Time ──────────────────────────────────────────────────────────────→

Iteration 1:
  Token("I'll analyze") → Token(" the codebase") → Token(".")
  ToolExec{c1, "file_search", ...}
  ToolDone{c1, 120ms, ok}
  Token("Found 3 files. Let me delegate...")

Iteration 2:
  ToolExec{c2, "delegate_task", ...}
  SubAgentStart{0, 2, "Analyze auth module"}
  SubAgentToolExec{0, 2, "file_read", ...}
  SubAgentFinish{0, 2, "success", 3400ms, "Auth uses JWT...", 4, "claude-sonnet"}
  SubAgentStart{1, 2, "Analyze DB module"}
  SubAgentToolExec{1, 2, "file_read", ...}
  SubAgentFinish{1, 2, "success", 2100ms, "DB uses SQLite...", 3, "claude-sonnet"}
  ToolDone{c2, 5500ms, ok}

Iteration 3:
  ContextPressure{85000, 64000}     // ← 66% of context used
  Token("Based on the analysis...") → Token(" Here's my recommendation:")
  Done
```

### 4.3 Interactive Event Flow (Approval)

```
Agent                UI                     User
  │                   │                       │
  │  Approval ──────→ │                       │
  │  {command: "rm",   │  Show dialog ──────→ │
  │   reasons: [...],  │  "Allow rm -rf?"     │
  │   response_tx}     │                       │
  │                   │  ← Click "Once" ───── │
  │  ← Once ─────── │                       │
  │                   │                       │
  │  (continues       │                       │
  │   execution)      │                       │
```

**Critical detail:** The `response_tx` is a `tokio::sync::oneshot::Sender`. The agent loop is **suspended** until the UI sends a response. This means the UI has full control — it can show a modal dialog, wait for user input, and the agent resumes seamlessly. No polling, no timeouts (unless the agent's own timeout triggers).

---

## 5. Transport Layers

The event bus is transport-agnostic. EdgeCrab supports (or will support) multiple ways to deliver events to consumers:

### 5.1 In-Process (Current — TUI, Gateway)

```rust
// Direct channel consumption — zero serialization overhead
let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
agent.chat_streaming("query", tx).await?;

while let Some(event) = rx.recv().await {
    handle_event(event);
}
```

**Used by:** TUI (`app.rs`), Gateway stream consumer, ACP adapter

### 5.2 Server-Sent Events (SSE) — HTTP Streaming

For web UIs that consume events over HTTP:

```
POST /api/agent/chat
Content-Type: application/json
Accept: text/event-stream

{"message": "Explain this code", "session_id": "s123"}

--- Response (SSE stream) ---

event: token
data: {"text": "I'll"}

event: token
data: {"text": " analyze"}

event: tool_exec
data: {"tool_call_id": "c1", "name": "file_read", "args_json": "..."}

event: tool_done
data: {"tool_call_id": "c1", "name": "file_read", "duration_ms": 42, "is_error": false}

event: context_pressure
data: {"estimated_tokens": 85000, "threshold_tokens": 64000}

event: done
data: {}
```

**Implementation sketch (Rust — axum):**

```rust
async fn chat_stream(
    State(agent): State<Arc<Agent>>,
    Json(req): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();

    tokio::spawn(async move {
        agent.chat_streaming(&req.message, tx).await.ok();
    });

    let stream = async_stream::stream! {
        while let Some(event) = rx.recv().await {
            let (event_type, data) = serialize_event(&event);
            yield Ok(Event::default().event(event_type).data(data));
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

### 5.3 WebSocket — Full Duplex (Interactive Events)

For UIs that need to **respond** to interactive events (Clarify, Approval, SecretRequest):

```
WebSocket /ws/agent

→ Client sends:
{"type": "chat", "message": "Delete old logs", "session_id": "s123"}

← Server streams:
{"type": "token", "text": "I'll delete"}
{"type": "approval", "id": "approval_1", "command": "rm -rf /var/log/old",
 "reasons": ["Recursive delete", "System directory"]}

→ Client responds:
{"type": "approval_response", "id": "approval_1", "choice": "once"}

← Server continues:
{"type": "tool_exec", "tool_call_id": "c1", "name": "terminal", ...}
{"type": "tool_done", "tool_call_id": "c1", "duration_ms": 150, ...}
{"type": "token", "text": "Done! Deleted 42 files."}
{"type": "done"}
```

**Why WebSocket over SSE for interactive events:**

| Feature | SSE | WebSocket |
|---------|-----|-----------|
| Server → Client streaming | ✅ | ✅ |
| Client → Server (approval/clarify) | ❌ Needs separate POST | ✅ Same connection |
| Connection multiplexing | ❌ One stream per request | ✅ Multiple sessions |
| Browser support | ✅ Universal | ✅ Universal |
| Reconnection | ✅ Built-in | ⚠️ Manual |

**Recommendation:** Use SSE for read-only dashboards, WebSocket for interactive agent UIs.

### 5.4 JSON Serialization Format

All events serialize to a standard JSON format for network transport:

```typescript
// Discriminated union — "type" field determines the shape
type WireEvent =
  | { type: "token"; text: string }
  | { type: "reasoning"; text: string }
  | { type: "tool_exec"; tool_call_id: string; name: string; args_json: string }
  | { type: "tool_progress"; tool_call_id: string; name: string; message: string }
  | { type: "tool_done"; tool_call_id: string; name: string; args_json: string;
      result_preview: string | null; duration_ms: number; is_error: boolean }
  | { type: "sub_agent_start"; task_index: number; task_count: number; goal: string }
  | { type: "sub_agent_reasoning"; task_index: number; task_count: number; text: string }
  | { type: "sub_agent_tool_exec"; task_index: number; task_count: number;
      name: string; args_json: string }
  | { type: "sub_agent_finish"; task_index: number; task_count: number;
      status: string; duration_ms: number; summary: string;
      api_calls: number; model: string | null }
  | { type: "done" }
  | { type: "error"; message: string }
  | { type: "clarify"; id: string; question: string; choices: string[] | null }
  | { type: "approval"; id: string; command: string; full_command: string;
      reasons: string[] }
  | { type: "secret_request"; id: string; var_name: string; prompt: string;
      is_sudo: boolean }
  | { type: "hook_event"; event: string; context_json: string }
  | { type: "context_pressure"; estimated_tokens: number; threshold_tokens: number };
```

---

## 6. SDK Integration Patterns

### 6.1 Python SDK — Async Iterator

```python
import edgecrab

agent = edgecrab.Agent(model="anthropic/claude-sonnet-4-20250514")

# Simple streaming (text only)
async for token in agent.stream("Explain quicksort"):
    print(token, end="", flush=True)

# Full event stream (all 21 event types)
async for event in agent.stream_events("Explain quicksort"):
    match event:
        case edgecrab.TokenEvent(text=text):
            print(text, end="", flush=True)
        case edgecrab.ToolExecEvent(name=name, tool_call_id=cid):
            print(f"\n🔧 Running {name}...", end="")
        case edgecrab.ToolDoneEvent(name=name, duration_ms=ms, is_error=err):
            print(f" {'❌' if err else '✅'} ({ms}ms)")
        case edgecrab.ContextPressureEvent(estimated=est, threshold=thr):
            print(f"\n⚠️ Context {est}/{thr} tokens ({100*est//thr}%)")
        case edgecrab.DoneEvent():
            print("\n--- Done ---")
```

### 6.2 Node.js SDK — EventEmitter + AsyncIterator

```typescript
import { Agent, StreamEvent } from "@edgecrab/sdk";

const agent = new Agent({ model: "anthropic/claude-sonnet-4-20250514" });

// EventEmitter pattern
const stream = agent.stream("Explain quicksort");

stream.on("token", (text: string) => process.stdout.write(text));
stream.on("tool_exec", ({ name, tool_call_id }) => console.log(`\n🔧 ${name}...`));
stream.on("tool_done", ({ name, duration_ms, is_error }) =>
  console.log(` ${is_error ? "❌" : "✅"} (${duration_ms}ms)`)
);
stream.on("done", () => console.log("\n--- Done ---"));

// Or async iterator
for await (const event of agent.streamEvents("Explain quicksort")) {
  switch (event.type) {
    case "token": process.stdout.write(event.text); break;
    case "tool_done": console.log(`${event.name}: ${event.duration_ms}ms`); break;
  }
}
```

### 6.3 Rust SDK — Direct Channel

```rust
use edgecrab_core::{Agent, AgentBuilder, StreamEvent};
use tokio::sync::mpsc;

let agent = AgentBuilder::new("anthropic/claude-sonnet-4-20250514")
    .tools(registry)
    .build()?;

let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();

tokio::spawn(async move {
    agent.chat_streaming("Explain quicksort", tx).await.ok();
});

while let Some(event) = rx.recv().await {
    match event {
        StreamEvent::Token(text) => print!("{text}"),
        StreamEvent::ToolDone { name, duration_ms, is_error, .. } => {
            println!("\n{} {} ({}ms)",
                if is_error { "❌" } else { "✅" }, name, duration_ms);
        }
        StreamEvent::Done => break,
        _ => {}
    }
}
```

### 6.4 WASM SDK — Browser Callbacks

```typescript
import init, { WasmAgent } from "@edgecrab/wasm";

await init();

const agent = new WasmAgent({
  model: "anthropic/claude-sonnet-4-20250514",
  apiKey: "sk-...",
  onEvent: (event: StreamEvent) => {
    // All 21 event types delivered here
    switch (event.type) {
      case "token":
        document.getElementById("output")!.textContent += event.text;
        break;
      case "tool_exec":
        showSpinner(event.name);
        break;
      case "tool_done":
        hideSpinner(event.tool_call_id, event.duration_ms);
        break;
      case "approval":
        showApprovalDialog(event, (choice) => {
          agent.respondToApproval(event.id, choice);
        });
        break;
    }
  },
});

await agent.chat("Analyze this repository");
```

---

## 7. Web UI Architecture

### 7.1 Component Hierarchy

```
+------------------------------------------------------------------+
|  AgentDashboard (root)                                           |
|                                                                  |
|  ┌──────────────────────────────┐  ┌──────────────────────────┐ |
|  │  ResponsePane                │  │  SidePanel               │ |
|  │                              │  │                          │ |
|  │  ┌────────────────────────┐  │  │  ┌────────────────────┐ │ |
|  │  │  StreamingText         │  │  │  │  CostMeter         │ │ |
|  │  │  (token-by-token)      │  │  │  │  $0.042 / $0.10    │ │ |
|  │  └────────────────────────┘  │  │  │  ████████░░ 42%    │ │ |
|  │                              │  │  └────────────────────┘ │ |
|  │  ┌────────────────────────┐  │  │                          │ |
|  │  │  ThinkingIndicator     │  │  │  ┌────────────────────┐ │ |
|  │  │  (reasoning events)    │  │  │  │  ContextGauge      │ │ |
|  │  └────────────────────────┘  │  │  │  85K / 128K tokens │ │ |
|  │                              │  │  │  ██████████████░░░ │ │ |
|  └──────────────────────────────┘  │  │  66% — warn at 50% │ │ |
|                                    │  └────────────────────┘ │ |
|  ┌──────────────────────────────┐  │                          │ |
|  │  ToolTimeline                │  │  ┌────────────────────┐ │ |
|  │                              │  │  │  ModelInfo         │ │ |
|  │  ┌──────────────────────┐   │  │  │  claude-sonnet-4   │ │ |
|  │  │  ToolCard (file_read)│   │  │  │  3 API calls       │ │ |
|  │  │  ✅ 42ms             │   │  │  │  Iteration 2/90    │ │ |
|  │  │  ▸ src/main.rs       │   │  │  └────────────────────┘ │ |
|  │  └──────────────────────┘   │  │                          │ |
|  │  ┌──────────────────────┐   │  │  ┌────────────────────┐ │ |
|  │  │  ToolCard (terminal) │   │  │  │  SubAgentPanel     │ │ |
|  │  │  ⏳ running...       │   │  │  │                    │ │ |
|  │  │  ▸ "npm test"        │   │  │  │  Task 1/3 ✅ 3.4s  │ │ |
|  │  │  └ Progress: 45%     │   │  │  │  Task 2/3 ⏳ 1.2s  │ │ |
|  │  └──────────────────────┘   │  │  │  Task 3/3 ⬚        │ │ |
|  │  ┌──────────────────────┐   │  │  └────────────────────┘ │ |
|  │  │  ToolCard (web)      │   │  │                          │ |
|  │  │  ❌ 1200ms (SSRF)    │   │  └──────────────────────────┘ |
|  │  └──────────────────────┘   │                                |
|  └──────────────────────────────┘                                |
|                                                                  |
|  ┌──────────────────────────────────────────────────────────┐   |
|  │  ApprovalOverlay (modal — shown on Approval events)      │   |
|  │  ┌──────────────────────────────────────────────────┐    │   |
|  │  │  "rm -rf /tmp/old_logs"                          │    │   |
|  │  │  Reasons: Recursive delete, System directory     │    │   |
|  │  │                                                  │    │   |
|  │  │  [ Once ]  [ This Session ]  [ Always ]  [ Deny ]│    │   |
|  │  └──────────────────────────────────────────────────┘    │   |
|  └──────────────────────────────────────────────────────────┘   |
+------------------------------------------------------------------+
```

### 7.2 React Component: ToolTimeline

```tsx
import { useState, useEffect, useRef } from "react";
import type { WireEvent } from "@edgecrab/sdk";

interface ToolExecution {
  tool_call_id: string;
  name: string;
  args_json: string;
  startedAt: number;
  status: "running" | "success" | "error";
  duration_ms?: number;
  result_preview?: string;
  progress?: string;
}

export function ToolTimeline({ events }: { events: WireEvent[] }) {
  const [tools, setTools] = useState<Map<string, ToolExecution>>(new Map());

  useEffect(() => {
    const map = new Map(tools);

    for (const event of events) {
      switch (event.type) {
        case "tool_exec":
          map.set(event.tool_call_id, {
            tool_call_id: event.tool_call_id,
            name: event.name,
            args_json: event.args_json,
            startedAt: Date.now(),
            status: "running",
          });
          break;

        case "tool_progress":
          const running = map.get(event.tool_call_id);
          if (running) {
            map.set(event.tool_call_id, { ...running, progress: event.message });
          }
          break;

        case "tool_done":
          const existing = map.get(event.tool_call_id);
          if (existing) {
            map.set(event.tool_call_id, {
              ...existing,
              status: event.is_error ? "error" : "success",
              duration_ms: event.duration_ms,
              result_preview: event.result_preview ?? undefined,
            });
          }
          break;
      }
    }

    setTools(map);
  }, [events]);

  return (
    <div className="tool-timeline">
      {[...tools.values()].map((tool) => (
        <ToolCard key={tool.tool_call_id} tool={tool} />
      ))}
    </div>
  );
}

function ToolCard({ tool }: { tool: ToolExecution }) {
  const icon = tool.status === "running" ? "⏳"
    : tool.status === "error" ? "❌" : "✅";

  return (
    <div className={`tool-card tool-card--${tool.status}`}>
      <div className="tool-card__header">
        <span className="tool-card__icon">{icon}</span>
        <span className="tool-card__name">{tool.name}</span>
        {tool.duration_ms !== undefined && (
          <span className="tool-card__duration">{tool.duration_ms}ms</span>
        )}
      </div>
      {tool.progress && (
        <div className="tool-card__progress">{tool.progress}</div>
      )}
      {tool.result_preview && (
        <details className="tool-card__result">
          <summary>Result preview</summary>
          <pre>{tool.result_preview}</pre>
        </details>
      )}
    </div>
  );
}
```

### 7.3 React Component: ContextPressureGauge

```tsx
export function ContextPressureGauge({
  estimated_tokens,
  threshold_tokens,
  context_window = 128_000,
}: {
  estimated_tokens: number;
  threshold_tokens: number;
  context_window?: number;
}) {
  const usage = estimated_tokens / context_window;
  const thresholdPct = threshold_tokens / context_window;

  const color = usage > 0.85 ? "var(--color-danger)"
    : usage > thresholdPct ? "var(--color-warning)"
    : "var(--color-success)";

  return (
    <div className="context-gauge" role="meter"
         aria-valuenow={estimated_tokens}
         aria-valuemax={context_window}
         aria-label="Context window usage">
      <div className="context-gauge__label">
        Context: {(estimated_tokens / 1000).toFixed(1)}K / {(context_window / 1000).toFixed(0)}K
      </div>
      <div className="context-gauge__track">
        <div className="context-gauge__fill" style={{ width: `${usage * 100}%`, background: color }} />
        <div className="context-gauge__threshold"
             style={{ left: `${thresholdPct * 100}%` }}
             title={`Compression at ${(thresholdPct * 100).toFixed(0)}%`} />
      </div>
      <div className="context-gauge__pct">{(usage * 100).toFixed(0)}%</div>
    </div>
  );
}
```

### 7.4 React Component: SubAgentProgress

```tsx
interface SubAgentTask {
  task_index: number;
  task_count: number;
  goal: string;
  status: "pending" | "running" | "success" | "error" | "timeout";
  duration_ms?: number;
  summary?: string;
  api_calls?: number;
  model?: string;
  currentTool?: string;
}

export function SubAgentProgress({ events }: { events: WireEvent[] }) {
  const [tasks, setTasks] = useState<SubAgentTask[]>([]);

  useEffect(() => {
    const taskMap = new Map<number, SubAgentTask>();

    for (const event of events) {
      switch (event.type) {
        case "sub_agent_start":
          taskMap.set(event.task_index, {
            task_index: event.task_index,
            task_count: event.task_count,
            goal: event.goal,
            status: "running",
          });
          // Initialize pending tasks
          for (let i = 0; i < event.task_count; i++) {
            if (!taskMap.has(i)) {
              taskMap.set(i, {
                task_index: i,
                task_count: event.task_count,
                goal: "",
                status: "pending",
              });
            }
          }
          break;

        case "sub_agent_tool_exec": {
          const t = taskMap.get(event.task_index);
          if (t) taskMap.set(event.task_index, { ...t, currentTool: event.name });
          break;
        }

        case "sub_agent_finish":
          taskMap.set(event.task_index, {
            task_index: event.task_index,
            task_count: event.task_count,
            goal: taskMap.get(event.task_index)?.goal ?? "",
            status: event.status as "success" | "error" | "timeout",
            duration_ms: event.duration_ms,
            summary: event.summary,
            api_calls: event.api_calls,
            model: event.model ?? undefined,
          });
          break;
      }
    }

    setTasks([...taskMap.values()].sort((a, b) => a.task_index - b.task_index));
  }, [events]);

  if (tasks.length === 0) return null;

  const completed = tasks.filter((t) => t.status !== "pending" && t.status !== "running").length;

  return (
    <div className="sub-agent-progress">
      <h3>Sub-Agents ({completed}/{tasks.length})</h3>
      {tasks.map((task) => (
        <div key={task.task_index} className={`sub-task sub-task--${task.status}`}>
          <span className="sub-task__icon">
            {task.status === "pending" ? "⬚" :
             task.status === "running" ? "⏳" :
             task.status === "success" ? "✅" :
             task.status === "error" ? "❌" : "⏱️"}
          </span>
          <span className="sub-task__goal">{task.goal || `Task ${task.task_index + 1}`}</span>
          {task.duration_ms !== undefined && (
            <span className="sub-task__duration">{(task.duration_ms / 1000).toFixed(1)}s</span>
          )}
          {task.currentTool && task.status === "running" && (
            <span className="sub-task__tool">🔧 {task.currentTool}</span>
          )}
        </div>
      ))}
    </div>
  );
}
```

### 7.5 React Hook: useAgentStream

The central hook that connects a React app to the EdgeCrab event stream:

```tsx
import { useReducer, useCallback, useRef } from "react";

interface AgentState {
  status: "idle" | "streaming" | "waiting_approval" | "waiting_clarify" | "done" | "error";
  responseText: string;
  reasoningText: string;
  events: WireEvent[];
  tools: Map<string, ToolExecution>;
  subAgents: SubAgentTask[];
  contextPressure: { estimated: number; threshold: number } | null;
  pendingApproval: { id: string; command: string; reasons: string[] } | null;
  pendingClarify: { id: string; question: string; choices: string[] | null } | null;
  error: string | null;
}

type AgentAction =
  | { type: "reset" }
  | { type: "event"; event: WireEvent };

function agentReducer(state: AgentState, action: AgentAction): AgentState {
  if (action.type === "reset") {
    return { ...initialState, status: "streaming" };
  }

  const event = action.event;
  const events = [...state.events, event];

  switch (event.type) {
    case "token":
      return { ...state, events, responseText: state.responseText + event.text };

    case "reasoning":
      return { ...state, events, reasoningText: state.reasoningText + event.text };

    case "tool_exec":
      const newTools = new Map(state.tools);
      newTools.set(event.tool_call_id, {
        tool_call_id: event.tool_call_id,
        name: event.name,
        args_json: event.args_json,
        startedAt: Date.now(),
        status: "running",
      });
      return { ...state, events, tools: newTools };

    case "tool_done":
      const updTools = new Map(state.tools);
      const existing = updTools.get(event.tool_call_id);
      if (existing) {
        updTools.set(event.tool_call_id, {
          ...existing,
          status: event.is_error ? "error" : "success",
          duration_ms: event.duration_ms,
          result_preview: event.result_preview ?? undefined,
        });
      }
      return { ...state, events, tools: updTools };

    case "context_pressure":
      return { ...state, events, contextPressure: {
        estimated: event.estimated_tokens,
        threshold: event.threshold_tokens
      }};

    case "approval":
      return { ...state, events, status: "waiting_approval", pendingApproval: {
        id: event.id, command: event.full_command, reasons: event.reasons
      }};

    case "clarify":
      return { ...state, events, status: "waiting_clarify", pendingClarify: {
        id: event.id, question: event.question, choices: event.choices
      }};

    case "done":
      return { ...state, events, status: "done" };

    case "error":
      return { ...state, events, status: "error", error: event.message };

    default:
      return { ...state, events };
  }
}

const initialState: AgentState = {
  status: "idle",
  responseText: "",
  reasoningText: "",
  events: [],
  tools: new Map(),
  subAgents: [],
  contextPressure: null,
  pendingApproval: null,
  pendingClarify: null,
  error: null,
};

export function useAgentStream(wsUrl: string) {
  const [state, dispatch] = useReducer(agentReducer, initialState);
  const wsRef = useRef<WebSocket | null>(null);

  const send = useCallback((message: string) => {
    dispatch({ type: "reset" });
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => {
      ws.send(JSON.stringify({ type: "chat", message }));
    };

    ws.onmessage = (msg) => {
      const event: WireEvent = JSON.parse(msg.data);
      dispatch({ type: "event", event });
    };

    ws.onerror = () => dispatch({ type: "event", event: { type: "error", message: "Connection lost" } });
    ws.onclose = () => { wsRef.current = null; };
  }, [wsUrl]);

  const respondToApproval = useCallback((id: string, choice: string) => {
    wsRef.current?.send(JSON.stringify({ type: "approval_response", id, choice }));
    dispatch({ type: "event", event: { type: "done" } }); // Reset status
  }, []);

  const respondToClarify = useCallback((id: string, answer: string) => {
    wsRef.current?.send(JSON.stringify({ type: "clarify_response", id, answer }));
  }, []);

  return { state, send, respondToApproval, respondToClarify };
}
```

---

## 8. Interactive Events (Human-in-the-Loop)

### 8.1 Approval Dialog

The `Approval` event carries a `oneshot::Sender<ApprovalChoice>` that suspends the agent until the UI responds. This is **not a callback** — it's a first-class flow control mechanism.

**UI must display:**
- The `command` (short description)
- The `full_command` (complete command text for inspection)
- The `reasons` array (why this needs approval: e.g., "Recursive delete", "System directory")
- Four action buttons: **Once**, **This Session**, **Always**, **Deny**

**Graduated trust model:**

| Choice | Behavior | Persistence |
|--------|----------|-------------|
| `Once` | Execute this single invocation | None |
| `Session` | Auto-approve this command pattern for the rest of the session | In-memory |
| `Always` | Persist to `~/.edgecrab/approvals.yaml` | Across sessions |
| `Deny` | Refuse execution; agent receives error and adapts | None |

### 8.2 Clarify Dialog

```
┌──────────────────────────────────────────────────┐
│  🤔 Agent needs clarification                    │
│                                                  │
│  "Which database should I use for the migration?"│
│                                                  │
│  ○ PostgreSQL                                    │
│  ○ SQLite                                        │
│  ○ MySQL                                         │
│                                                  │
│  Or type a custom answer:                        │
│  ┌──────────────────────────────────────────┐    │
│  │                                          │    │
│  └──────────────────────────────────────────┘    │
│                                                  │
│           [ Submit ]                             │
└──────────────────────────────────────────────────┘
```

When `choices` is `Some(vec)`, render radio buttons. When `None`, render a free-text input. Both send back a `String` via the `response_tx`.

### 8.3 Secret Request Dialog

```
┌──────────────────────────────────────────────────┐
│  🔑 Credential Required                         │
│                                                  │
│  "Enter your GITHUB_TOKEN for API access"        │
│                                                  │
│  ┌──────────────────────────────────────────┐    │
│  │ •••••••••••••••••                        │    │
│  └──────────────────────────────────────────┘    │
│                                                  │
│  ⚠️ This value will not be logged or stored      │
│                                                  │
│           [ Submit ]     [ Cancel ]              │
└──────────────────────────────────────────────────┘
```

When `is_sudo: true`, the UI should indicate this is a system password. The value is never persisted — it flows directly to the tool that requested it.

---

## 9. State Machine for UI Reducers

### 9.1 Agent State Machine

```
                    ┌─────────────────────────────────────────┐
                    │                                         │
                    ▼                                         │
  ┌──────┐    ┌───────────┐    ┌──────┐                      │
  │ IDLE │───→│ STREAMING │───→│ DONE │                      │
  └──────┘    └─────┬─────┘    └──────┘                      │
                    │                                         │
              ┌─────┼─────────────────┐                      │
              │     │                 │                      │
              ▼     ▼                 ▼                      │
     ┌────────────┐ ┌──────────────┐ ┌───────────────────┐  │
     │ WAITING    │ │ WAITING      │ │ WAITING           │  │
     │ APPROVAL   │ │ CLARIFY      │ │ SECRET            │  │
     └─────┬──────┘ └──────┬───────┘ └─────────┬─────────┘  │
           │               │                   │             │
           └───────────────┴───────────────────┘             │
                           │                                 │
                           └────── (response sent) ──────────┘
                                   back to STREAMING

  Any state ──→ ERROR (on Error event)
```

### 9.2 Tool State Machine

```
  ┌─────────┐    ToolExec    ┌─────────┐
  │ UNKNOWN │───────────────→│ RUNNING │
  └─────────┘                └────┬────┘
                                  │
                      ┌───────────┤
                      │           │
               ToolProgress   ToolDone
                      │           │
                      ▼           │
                ┌──────────┐     │
                │ RUNNING  │     │
                │ (updated)│     │
                └────┬─────┘     │
                     │           │
                     └───────────┤
                                 │
                    ┌────────────┼────────────┐
                    │            │            │
                    ▼            ▼            ▼
              ┌──────────┐ ┌──────────┐ (should not happen)
              │ SUCCESS  │ │  ERROR   │
              │ +dur_ms  │ │ +dur_ms  │
              └──────────┘ └──────────┘
```

### 9.3 Sub-Agent Task State Machine

```
  ┌─────────┐   SubAgentStart    ┌─────────┐
  │ PENDING │───────────────────→│ RUNNING │
  └─────────┘                    └────┬────┘
                                      │
                          SubAgentFinish
                                      │
                     ┌────────────────┼────────────────┐
                     │                │                │
                     ▼                ▼                ▼
               ┌──────────┐    ┌──────────┐    ┌──────────┐
               │ SUCCESS  │    │  ERROR   │    │ TIMEOUT  │
               │ +summary │    │ +summary │    │ +summary │
               │ +dur_ms  │    │ +dur_ms  │    │ +dur_ms  │
               │ +api_calls│    │ +api_calls│    │ +api_calls│
               └──────────┘    └──────────┘    └──────────┘
```

---

## 10. Event Recording, Replay & Time-Travel Debugging

### 10.1 Event Recording

Every event can be recorded as JSONL for post-hoc analysis and replay:

```jsonl
{"ts":1719504000000,"type":"token","text":"I'll"}
{"ts":1719504000050,"type":"token","text":" analyze"}
{"ts":1719504000100,"type":"tool_exec","tool_call_id":"c1","name":"file_read","args_json":"{\"path\":\"src/main.rs\"}"}
{"ts":1719504000142,"type":"tool_done","tool_call_id":"c1","name":"file_read","duration_ms":42,"is_error":false,"result_preview":"fn main() {..."}
{"ts":1719504000200,"type":"context_pressure","estimated_tokens":85000,"threshold_tokens":64000}
{"ts":1719504003600,"type":"done"}
```

**Recording API:**

```typescript
// Record events during a session
const recorder = new EventRecorder();
const stream = agent.streamEvents("Analyze this");

for await (const event of stream) {
  recorder.record(event);  // Appends with timestamp
  handleEvent(event);      // Normal processing
}

// Save to file
await recorder.save("session_2025-01-15_14-30.jsonl");

// Integrate with trajectory system
// (EdgeCrab already saves full trajectories to ~/.edgecrab/trajectories/)
```

### 10.2 Event Replay

Replay recorded sessions in the UI — invaluable for debugging, demos, and testing:

```typescript
class EventReplayer {
  private events: TimestampedEvent[];
  private cursor: number = 0;
  private playbackSpeed: number = 1.0;

  constructor(events: TimestampedEvent[]) {
    this.events = events;
  }

  // Play forward with real-time delays
  async play(onEvent: (event: WireEvent) => void): Promise<void> {
    while (this.cursor < this.events.length) {
      const current = this.events[this.cursor];
      const next = this.events[this.cursor + 1];

      onEvent(current.event);
      this.cursor++;

      if (next) {
        const delay = (next.ts - current.ts) / this.playbackSpeed;
        await sleep(delay);
      }
    }
  }

  // Step forward one event at a time
  stepForward(onEvent: (event: WireEvent) => void): boolean {
    if (this.cursor >= this.events.length) return false;
    onEvent(this.events[this.cursor].event);
    this.cursor++;
    return true;
  }

  // Step backward (time-travel!)
  stepBackward(onEvent: (events: WireEvent[]) => void): boolean {
    if (this.cursor <= 0) return false;
    this.cursor--;
    // Replay all events from start to cursor to reconstruct state
    onEvent(this.events.slice(0, this.cursor).map(e => e.event));
    return true;
  }

  // Jump to specific event index
  seekTo(index: number, onEvent: (events: WireEvent[]) => void): void {
    this.cursor = Math.max(0, Math.min(index, this.events.length));
    onEvent(this.events.slice(0, this.cursor).map(e => e.event));
  }

  setSpeed(speed: number): void {
    this.playbackSpeed = speed;
  }
}
```

### 10.3 Time-Travel Debugging UI

```
+------------------------------------------------------------------+
|  Time-Travel Debugger                                            |
|                                                                  |
|  Event Timeline:                                                 |
|  ┌──────────────────────────────────────────────────────────┐   |
|  │ ● ● ● ● ● ●  ◆  ● ● ●  ◆  ● ●  ▲  ● ● ●  ◆  ● ● ● │   |
|  │ t t t t t t  TE  t t t  TD  t t  CP  t t t  TE  t t t   │   |
|  └──────────────────────────┬───────────────────────────────┘   |
|                             │                                    |
|  Legend: ● Token  ◆ Tool  ▲ System  ▼ cursor                   |
|                                                                  |
|  ┌────────────────────────────────────────────────────────┐     |
|  │  Event #12: ToolDone                                   │     |
|  │  tool_call_id: "call_abc123"                           │     |
|  │  name: "file_read"                                     │     |
|  │  duration_ms: 42                                       │     |
|  │  is_error: false                                       │     |
|  │  result_preview: "fn main() { println!(\"Hello\"); }"  │     |
|  └────────────────────────────────────────────────────────┘     |
|                                                                  |
|  Controls:                                                       |
|  [ ◁ Step Back ]  [ ▷ Step Forward ]  [ ▶ Play 1x ]  [ 2x ]   |
|                                                                  |
|  Playback: Event 12 / 47  |  Elapsed: 3.6s / 12.4s             |
+------------------------------------------------------------------+
```

### 10.4 Integration with Trajectory System

EdgeCrab already saves full conversation trajectories to `~/.edgecrab/trajectories/trajectory_samples.jsonl` when `save_trajectories = true`. The event recording system complements this:

| System | What it captures | Granularity | Use case |
|--------|-----------------|-------------|----------|
| **Trajectories** | Full message history (system, user, assistant, tool) | Per-turn | RL training, analysis |
| **Event Recording** | All 21 StreamEvent variants with timestamps | Per-event (sub-ms) | UI replay, debugging |

Both can be enabled simultaneously. Event recordings include everything trajectories do, plus intermediate streaming tokens, progress updates, and timing data.

---

## 11. Metrics, Tracing & Analytics

### 11.1 OpenTelemetry Integration

EdgeCrab events map naturally to OpenTelemetry spans:

```
Trace: agent_conversation (session_id)
├── Span: llm_generation (iteration 1)
│   ├── Attribute: gen_ai.system = "anthropic"
│   ├── Attribute: gen_ai.request.model = "claude-sonnet-4-20250514"
│   ├── Attribute: gen_ai.usage.input_tokens = 2400
│   └── Attribute: gen_ai.usage.output_tokens = 150
├── Span: tool_execution (file_read)
│   ├── Attribute: tool.name = "file_read"
│   ├── Attribute: tool.call_id = "call_abc123"
│   ├── Attribute: tool.duration_ms = 42
│   └── Attribute: tool.is_error = false
├── Span: llm_generation (iteration 2)
│   └── ...
├── Span: sub_agent_delegation
│   ├── Span: sub_agent_task_0
│   │   ├── Attribute: sub_agent.goal = "Analyze auth module"
│   │   ├── Attribute: sub_agent.model = "claude-sonnet-4-20250514"
│   │   └── Attribute: sub_agent.api_calls = 4
│   └── Span: sub_agent_task_1
│       └── ...
└── Span: llm_generation (iteration 3)
```

**Implementation:**

```rust
use opentelemetry::{global, trace::{Tracer, SpanKind}};

fn emit_otel_span(event: &StreamEvent) {
    let tracer = global::tracer("edgecrab");

    match event {
        StreamEvent::ToolExec { tool_call_id, name, .. } => {
            let span = tracer.span_builder(format!("tool:{name}"))
                .with_kind(SpanKind::Internal)
                .with_attributes(vec![
                    KeyValue::new("tool.name", name.clone()),
                    KeyValue::new("tool.call_id", tool_call_id.clone()),
                ])
                .start(&tracer);
            // Store span for later completion on ToolDone
        }
        StreamEvent::ToolDone { tool_call_id, duration_ms, is_error, .. } => {
            // Complete the span started by ToolExec
        }
        _ => {}
    }
}
```

### 11.2 Cost Analytics

EdgeCrab's pricing engine (`pricing.rs`) provides real-time cost tracking:

```typescript
// Derived from ConversationResult after each turn
interface CostMetrics {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  reasoning_tokens: number;
  total_cost_usd: number;
  cost_status: "estimated" | "included" | "unknown";
  cost_source: "official_docs" | "user_override" | "unknown";
}

// Zero-cost providers (subscription-included or local)
const FREE_PROVIDERS = ["copilot", "ollama", "lmstudio"];
```

**UI pattern — live cost meter:**

```tsx
function CostMeter({ cost, budget }: { cost: CostMetrics; budget: number }) {
  const pct = cost.total_cost_usd / budget;
  const color = pct > 0.9 ? "red" : pct > 0.7 ? "orange" : "green";

  return (
    <div className="cost-meter">
      <div className="cost-meter__amount">
        ${cost.total_cost_usd.toFixed(4)}
        <span className="cost-meter__status">({cost.cost_status})</span>
      </div>
      <div className="cost-meter__breakdown">
        <span>In: {cost.input_tokens.toLocaleString()}</span>
        <span>Out: {cost.output_tokens.toLocaleString()}</span>
        {cost.cache_read_tokens > 0 && (
          <span>Cache: {cost.cache_read_tokens.toLocaleString()}</span>
        )}
        {cost.reasoning_tokens > 0 && (
          <span>Think: {cost.reasoning_tokens.toLocaleString()}</span>
        )}
      </div>
      <div className="cost-meter__bar">
        <div style={{ width: `${Math.min(pct * 100, 100)}%`, background: color }} />
      </div>
    </div>
  );
}
```

### 11.3 Event Aggregation for Dashboards

```typescript
interface SessionAnalytics {
  total_events: number;
  total_tokens: number;
  total_tools: number;
  tool_success_rate: number;
  avg_tool_duration_ms: number;
  slowest_tool: { name: string; duration_ms: number };
  total_sub_agents: number;
  sub_agent_success_rate: number;
  context_pressure_events: number;
  approvals_requested: number;
  approvals_denied: number;
  total_cost_usd: number;
  session_duration_ms: number;
}

function computeAnalytics(events: TimestampedEvent[]): SessionAnalytics {
  const toolDones = events
    .filter((e): e is { ts: number; event: ToolDoneEvent } => e.event.type === "tool_done");

  return {
    total_events: events.length,
    total_tokens: events.filter(e => e.event.type === "token").length,
    total_tools: toolDones.length,
    tool_success_rate: toolDones.length > 0
      ? toolDones.filter(t => !t.event.is_error).length / toolDones.length
      : 1.0,
    avg_tool_duration_ms: toolDones.length > 0
      ? toolDones.reduce((sum, t) => sum + t.event.duration_ms, 0) / toolDones.length
      : 0,
    slowest_tool: toolDones.reduce(
      (max, t) => t.event.duration_ms > max.duration_ms
        ? { name: t.event.name, duration_ms: t.event.duration_ms }
        : max,
      { name: "", duration_ms: 0 }
    ),
    // ... other aggregations
  };
}
```

---

## 12. Performance & Backpressure

### 12.1 Event Batching for UI Performance

React renders are expensive. Batch high-frequency events (tokens arrive every ~20ms) to reduce re-renders:

```typescript
function useBatchedEvents(wsUrl: string, batchIntervalMs = 50) {
  const [state, dispatch] = useReducer(agentReducer, initialState);
  const bufferRef = useRef<WireEvent[]>([]);

  useEffect(() => {
    const ws = new WebSocket(wsUrl);

    ws.onmessage = (msg) => {
      bufferRef.current.push(JSON.parse(msg.data));
    };

    // Flush buffer every batchIntervalMs
    const interval = setInterval(() => {
      const batch = bufferRef.current.splice(0);
      if (batch.length > 0) {
        // Process all buffered events in one dispatch → one render
        for (const event of batch) {
          dispatch({ type: "event", event });
        }
      }
    }, batchIntervalMs);

    return () => { clearInterval(interval); ws.close(); };
  }, [wsUrl, batchIntervalMs]);

  return state;
}
```

### 12.2 Event Filtering

Not every UI needs every event. Filter at the consumer level:

```typescript
// Dashboard only cares about tool lifecycle + cost
const DASHBOARD_EVENTS = new Set([
  "tool_exec", "tool_progress", "tool_done",
  "sub_agent_start", "sub_agent_finish",
  "context_pressure", "done", "error",
]);

// Chat UI needs tokens + interactive
const CHAT_EVENTS = new Set([
  "token", "reasoning", "done", "error",
  "clarify", "approval", "secret_request",
]);

function filterEvents(event: WireEvent, allowedTypes: Set<string>): boolean {
  return allowedTypes.has(event.type);
}
```

### 12.3 Backpressure Strategy

| Layer | Strategy | Implementation |
|-------|----------|----------------|
| **Producer (Agent)** | Fire-and-forget | `UnboundedSender` — agent never blocks |
| **Bus (Channel)** | Unbounded buffer | Events queue in memory; typical session <1000 events |
| **Consumer (UI)** | Batch + drop | UI batches at 50ms; if buffer grows >10K, drop old tokens |
| **Network (WS/SSE)** | TCP backpressure | WebSocket/SSE rely on TCP flow control |
| **Recording** | Async flush | JSONL appends buffered, flushed every 1s |

### 12.4 Memory Overhead

Typical event sizes:

| Event | Approx. size | Frequency |
|-------|-------------|-----------|
| Token | ~50 bytes | 50-100/s during streaming |
| ToolExec | ~200 bytes | 1-5 per iteration |
| ToolDone | ~300 bytes | 1-5 per iteration |
| SubAgentFinish | ~500 bytes | 1-10 per delegation |
| ContextPressure | ~50 bytes | 0-1 per iteration |

A typical 10-turn conversation generates ~500-2000 events, using ~100-400 KB of memory. This is negligible compared to the conversation history itself (which can be 50-200 KB of text).

---

## 13. Competitor Comparison

### 13.1 Feature Matrix

| Feature | EdgeCrab | OpenAI Agents SDK | Pydantic AI | LangChain | Google ADK |
|---------|----------|-------------------|-------------|-----------|------------|
| **Real-time event streaming** | ✅ 21 typed variants | ❌ Post-hoc traces only | ❌ Post-hoc spans | ❌ Callbacks | ⚠️ Lifecycle hooks |
| **Tool execution timeline** | ✅ ToolExec→Progress→Done | ❌ function_span (post-hoc) | ❌ tool span | ⚠️ on_tool_start/end | ⚠️ on_tool_use |
| **Tool duration tracking** | ✅ Built-in `duration_ms` | ❌ Span timestamps | ❌ Span timestamps | ❌ Manual | ❌ Manual |
| **Tool error attribution** | ✅ `is_error` field | ❌ Span status | ❌ Span status | ❌ Exception | ❌ Exception |
| **Tool live progress** | ✅ ToolProgress events | ❌ | ❌ | ❌ | ❌ |
| **Sub-agent visibility** | ✅ 4 event types | ❌ Nested traces | ❌ | ⚠️ Callbacks | ❌ |
| **Context pressure** | ✅ ContextPressure event | ❌ | ❌ | ❌ | ❌ |
| **Interactive approval** | ✅ Bidirectional (oneshot) | ❌ | ⚠️ ApprovalRequired exc. | ❌ | ❌ |
| **Interactive clarify** | ✅ Bidirectional | ❌ | ❌ | ⚠️ HumanApprovalCallbackHandler | ❌ |
| **Secret request** | ✅ SecretRequest event | ❌ | ❌ | ❌ | ❌ |
| **Cost tracking** | ✅ Per-turn, 15 providers | ✅ usage object | ✅ usage info | ⚠️ Callbacks | ❌ |
| **Event recording** | ✅ Trajectory JSONL | ✅ Traces dashboard | ✅ Logfire | ⚠️ LangSmith | ❌ |
| **Event replay** | ✅ (spec'd) | ❌ | ❌ | ❌ | ❌ |
| **Lifecycle hooks** | ✅ 24 event types | ❌ | ❌ | ⚠️ Callbacks | ⚠️ on_* hooks |
| **OpenTelemetry** | ✅ (spec'd) | ✅ TracingProcessor | ✅ Native | ⚠️ Plugin | ❌ |
| **Typed events** | ✅ Rust enum | ⚠️ SpanData classes | ✅ OTel spans | ❌ Dict-based | ❌ |
| **Transport options** | In-proc, SSE, WS | HTTP (batch export) | HTTP (OTel) | Varies | HTTP |

### 13.2 Key Differentiators

**1. Real-time vs. Post-hoc:**
OpenAI Agents SDK and Pydantic AI use a trace/span model — events are collected, batched, and exported to a backend (OpenAI Traces, Logfire) after execution. You cannot drive a real-time UI from their event system. EdgeCrab streams events as they happen.

**2. Interactive bidirectional events:**
Only EdgeCrab supports bidirectional flow control through the event bus. When the agent needs approval, the UI receives an `Approval` event with a response channel. The agent **suspends** until the UI responds. No other SDK has this.

**3. Tool progress:**
Only EdgeCrab has `ToolProgress` events — live status updates from running tools (e.g., download progress, test output). Others only know "tool started" and "tool finished".

**4. Sub-agent hierarchy:**
EdgeCrab emits 4 event types for sub-agent delegation with `task_index/task_count` correlation. OpenAI creates nested traces (post-hoc only). Others have no sub-agent visibility.

**5. Context pressure:**
Only EdgeCrab warns the UI when context window usage is approaching the compression threshold. This lets UIs show a "memory pressure" gauge — critical for long-running agent sessions.

---

## 14. Complete Dashboard Example

A full working dashboard that uses all the components from this spec:

```tsx
import { useAgentStream } from "./hooks/useAgentStream";
import { ToolTimeline } from "./components/ToolTimeline";
import { ContextPressureGauge } from "./components/ContextPressureGauge";
import { SubAgentProgress } from "./components/SubAgentProgress";
import { ApprovalDialog } from "./components/ApprovalDialog";
import { ClarifyDialog } from "./components/ClarifyDialog";
import { CostMeter } from "./components/CostMeter";

export function AgentDashboard() {
  const { state, send, respondToApproval, respondToClarify } = useAgentStream(
    "ws://localhost:8080/ws/agent"
  );
  const [input, setInput] = useState("");

  return (
    <div className="agent-dashboard">
      {/* Header */}
      <header className="dashboard-header">
        <h1>EdgeCrab Agent</h1>
        <div className="dashboard-status">
          <span className={`status-badge status-badge--${state.status}`}>
            {state.status}
          </span>
        </div>
      </header>

      {/* Main content area */}
      <div className="dashboard-main">
        {/* Left: Response + Input */}
        <div className="dashboard-response">
          {/* Reasoning indicator */}
          {state.reasoningText && (
            <div className="thinking-indicator">
              <details>
                <summary>💭 Thinking...</summary>
                <pre>{state.reasoningText}</pre>
              </details>
            </div>
          )}

          {/* Streaming response */}
          <div className="response-text">
            <ReactMarkdown>{state.responseText}</ReactMarkdown>
            {state.status === "streaming" && <span className="cursor">▊</span>}
          </div>

          {/* Tool timeline */}
          <ToolTimeline events={state.events} />

          {/* Input */}
          <form onSubmit={(e) => { e.preventDefault(); send(input); setInput(""); }}>
            <input
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder="Ask anything..."
              disabled={state.status === "streaming"}
            />
            <button type="submit" disabled={state.status === "streaming"}>
              Send
            </button>
          </form>
        </div>

        {/* Right: Side panel */}
        <aside className="dashboard-sidebar">
          {/* Context pressure */}
          {state.contextPressure && (
            <ContextPressureGauge
              estimated_tokens={state.contextPressure.estimated}
              threshold_tokens={state.contextPressure.threshold}
            />
          )}

          {/* Cost meter */}
          <CostMeter cost={state.cost} budget={1.00} />

          {/* Sub-agent progress */}
          <SubAgentProgress events={state.events} />
        </aside>
      </div>

      {/* Modal overlays */}
      {state.pendingApproval && (
        <ApprovalDialog
          command={state.pendingApproval.command}
          reasons={state.pendingApproval.reasons}
          onChoice={(choice) => respondToApproval(state.pendingApproval!.id, choice)}
        />
      )}

      {state.pendingClarify && (
        <ClarifyDialog
          question={state.pendingClarify.question}
          choices={state.pendingClarify.choices}
          onAnswer={(answer) => respondToClarify(state.pendingClarify!.id, answer)}
        />
      )}
    </div>
  );
}
```

### 14.1 CSS Architecture (Key Classes)

```css
.agent-dashboard {
  display: grid;
  grid-template-rows: auto 1fr;
  grid-template-columns: 1fr 300px;
  height: 100vh;
  gap: 1rem;
}

.dashboard-header { grid-column: 1 / -1; }
.dashboard-response { grid-column: 1; overflow-y: auto; }
.dashboard-sidebar { grid-column: 2; }

/* Tool timeline */
.tool-card { border-left: 3px solid var(--color-muted); padding: 0.5rem; margin: 0.25rem 0; }
.tool-card--running { border-color: var(--color-info); animation: pulse 1s infinite; }
.tool-card--success { border-color: var(--color-success); }
.tool-card--error { border-color: var(--color-danger); }

/* Context gauge */
.context-gauge__track { height: 8px; background: var(--color-muted); border-radius: 4px; }
.context-gauge__fill { height: 100%; border-radius: 4px; transition: width 0.3s; }
.context-gauge__threshold { position: absolute; width: 2px; height: 12px; background: var(--color-warning); }

/* Status badges */
.status-badge--idle { background: var(--color-muted); }
.status-badge--streaming { background: var(--color-info); animation: pulse 1s infinite; }
.status-badge--done { background: var(--color-success); }
.status-badge--error { background: var(--color-danger); }
.status-badge--waiting_approval { background: var(--color-warning); }

/* Approval overlay */
.approval-dialog { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: grid; place-items: center; z-index: 100; }

/* Responsive */
@media (max-width: 768px) {
  .agent-dashboard { grid-template-columns: 1fr; }
  .dashboard-sidebar { grid-column: 1; grid-row: 3; }
}

/* Accessibility */
@media (prefers-reduced-motion: reduce) {
  .tool-card--running { animation: none; }
  .status-badge--streaming { animation: none; }
}

/* Dark mode */
@media (prefers-color-scheme: dark) {
  :root {
    --color-success: #4caf50;
    --color-danger: #f44336;
    --color-warning: #ff9800;
    --color-info: #2196f3;
    --color-muted: #424242;
  }
}
```

---

## 15. Honest Assessment

### What Exists Today (Implemented in Rust)

| Feature | Status | Location |
|---------|--------|----------|
| 21 StreamEvent variants | ✅ Production | `crates/edgecrab-core/src/agent.rs` |
| Unbounded channel bus | ✅ Production | `tokio::sync::mpsc` |
| TUI consumer | ✅ Production | `crates/edgecrab-cli/src/app.rs` |
| Gateway stream consumer | ✅ Production | `crates/edgecrab-gateway/src/stream_consumer.rs` |
| ACP consumer (VS Code) | ✅ Production | `crates/edgecrab-acp/` |
| HookRegistry (24 events) | ✅ Production | `crates/edgecrab-gateway/src/hooks.rs` |
| Trajectory recording | ✅ Production | `crates/edgecrab-types/src/trajectory.rs` |
| Pricing engine | ✅ Production | `crates/edgecrab-core/src/pricing.rs` |
| ApprovalChoice (4 variants) | ✅ Production | `crates/edgecrab-core/src/agent.rs` |
| Interactive events (oneshot) | ✅ Production | Clarify, Approval, SecretRequest |

### What Needs to Be Built

| Feature | Priority | Effort | Notes |
|---------|----------|--------|-------|
| SSE transport (HTTP streaming) | P0 | Medium | Needed for web UIs |
| WebSocket transport (bidirectional) | P0 | Medium | Needed for interactive web UIs |
| JSON serialization format | P0 | Low | `serde_json` derive on StreamEvent |
| TypeScript SDK types | P0 | Low | Generate from Rust types |
| Event recording to JSONL | P1 | Low | Augment existing trajectory system |
| Event replay API | P1 | Medium | Client-side; no Rust changes |
| OpenTelemetry span emission | P1 | Medium | Map events to OTel spans |
| Time-travel debugging UI | P2 | High | Frontend only; events are immutable |
| React component library | P2 | High | ToolTimeline, ContextGauge, etc. |
| Prometheus metrics exporter | P2 | Medium | Tool duration histograms, error rates |
| Cost analytics dashboard | P2 | Medium | Aggregate across sessions |
| Event filtering on server | P3 | Low | Reduce network traffic for dashboards |

### Gaps vs. Competitors

| Gap | Impact | Mitigation |
|-----|--------|------------|
| No hosted dashboard (vs. OpenAI Traces, Logfire) | Users must build their own UI | Provide excellent component library + examples |
| No batch export to external backends | Limits enterprise adoption | OpenTelemetry integration (P1) bridges this |
| No built-in web server | Users need their own HTTP layer | Provide axum/actix integration examples |

### Why EdgeCrab Wins Anyway

1. **Real-time streaming beats post-hoc traces.** You can build a live dashboard with EdgeCrab. You can't with OpenAI Agents SDK or Pydantic AI — their events are exported in batches after execution.

2. **Bidirectional events are unique.** No other SDK has interactive approval/clarify/secret events that suspend the agent and wait for UI response.

3. **The data is already there.** All 21 event types are implemented in production Rust code. Competitors would need to add event types; EdgeCrab just needs transport layers and client libraries.

4. **Type safety.** Rust enum → TypeScript discriminated union → no runtime surprises. Compare to LangChain's dict-based callbacks where anything can be anything.

---

## Cross-References

- **[08-DEVELOPER-DOCS.md §7.1](08-DEVELOPER-DOCS.md)** — Event system overview (summary version)
- **[02-SPEC.md §4](02-SPEC.md)** — SDK specification (streaming API design)
- **[09-CUSTOM-TOOLS.md](09-CUSTOM-TOOLS.md)** — Custom tools emit ToolExec/ToolDone events
- **[10-EXAMPLES.md §11](10-EXAMPLES.md)** — WASM browser agent example (uses event callbacks)
- **[05-ADR.md §ADR-006](05-ADR.md)** — WASM SDK decision (lite event subset)
- **AGENTS.md** — StreamEvent enum source of truth in `agent.rs`

---

*This document specifies the event system that makes EdgeCrab the most observable agent SDK ever built. The runtime already emits 21 typed events through a production-grade async channel. What remains is transport layers (SSE, WebSocket), client libraries (TypeScript, Python), and reference UI components — all straightforward engineering on top of a solid foundation.*
