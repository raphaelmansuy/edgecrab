# EdgeCrab SDK — Developer Documentation

> **Cross-references:** [SPEC](02-SPEC.md) | [TUTORIAL](04-TUTORIAL.md) | [ADR](05-ADR.md)

---

## WHY This Document Exists

API reference documentation is the first thing developers reach for after the tutorial. Every public type, method, and parameter must be documented — not with auto-generated noise, but with context that explains *when* and *why* to use each feature.

---

## Table of Contents

1. [Agent](#1-agent)
2. [Tool](#2-tool)
3. [Types](#3-types)
4. [Config](#4-config)
5. [Session & Memory](#5-session--memory)
6. [Model Catalog](#6-model-catalog)
7. [Streaming](#7-streaming)
8. [Errors](#8-errors)
9. [MCP Integration](#9-mcp-integration)
10. [Security](#10-security)
11. [Advanced Patterns](#11-advanced-patterns)

---

## 1. Agent

### Constructor

```python
Agent(
    model: str = "anthropic/claude-sonnet-4-20250514",
    *,
    tools: list[Tool] | None = None,
    toolsets: list[str] | None = None,
    disabled_toolsets: list[str] | None = None,
    disabled_tools: list[str] | None = None,
    instructions: str | None = None,
    max_iterations: int = 90,
    temperature: float | None = None,
    reasoning_effort: str | None = None,
    session_id: str | None = None,
    on_stream: Callable[[StreamEvent], None] | None = None,
    on_tool_call: Callable[[ToolExecEvent], None] | None = None,
    on_tool_result: Callable[[ToolResultEvent], None] | None = None,
    approval_handler: Callable[[ApprovalRequest], ApprovalResponse] | None = None,
    platform: Platform = Platform.CLI,
    config: Config | None = None,
    config_path: str | None = None,
    state_dir: str | None = None,
    mcp_servers: dict[str, McpServer] | None = None,
    delegation: DelegationConfig | None = None,
    compression: CompressionConfig | None = None,
)
```

**Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `model` | `str` | `"anthropic/claude-sonnet-4-20250514"` | Model in `provider/model` format. See [Model Catalog](#6-model-catalog) for all options. |
| `tools` | `list[Tool]` | `None` | Custom tools to add alongside built-in tools. |
| `toolsets` | `list[str]` | `None` | Whitelist of toolsets. When set, only these toolsets are enabled. |
| `disabled_toolsets` | `list[str]` | `None` | Blacklist of toolsets to disable. |
| `disabled_tools` | `list[str]` | `None` | Specific tool names to disable. |
| `instructions` | `str` | `None` | Custom system prompt. Appended to the default identity prompt. |
| `max_iterations` | `int` | `90` | Maximum ReAct loop iterations before `BudgetExhaustedError`. |
| `temperature` | `float` | `None` | LLM temperature. `None` uses model default. |
| `reasoning_effort` | `str` | `None` | For reasoning models: `"low"`, `"medium"`, `"high"`. |
| `session_id` | `str` | `None` | Resume existing session. `None` creates new session. |
| `on_stream` | `Callable` | `None` | Callback for streaming events. Alternative to `agent.stream()`. |
| `on_tool_call` | `Callable` | `None` | Callback when a tool is about to execute. |
| `on_tool_result` | `Callable` | `None` | Callback when a tool finishes executing. |
| `approval_handler` | `Callable` | `None` | Called before dangerous commands. See [Security](#10-security). |
| `platform` | `Platform` | `CLI` | Affects system prompt hints: `CLI`, `TELEGRAM`, `DISCORD`, etc. |
| `config` | `Config` | `None` | Full configuration object. Overrides config file. |
| `config_path` | `str` | `None` | Path to config.yaml. Overrides default `~/.edgecrab/config.yaml`. |
| `state_dir` | `str` | `None` | Directory for sessions, memories, skills. Overrides `EDGECRAB_HOME`. |
| `mcp_servers` | `dict` | `None` | MCP server configurations. See [MCP Integration](#9-mcp-integration). |
| `delegation` | `DelegationConfig` | `None` | Sub-agent delegation settings. |
| `compression` | `CompressionConfig` | `None` | Context compression settings. |

### Methods

#### `chat(message: str) -> str` (async)

One-shot question. Maintains conversation history.

```python
response = await agent.chat("What is Rust?")
```

- Returns the agent's text response
- Conversation history is preserved — subsequent calls build on context
- Tools are invoked automatically if the model decides to use them
- Raises `AgentError` on failure

#### `chat_sync(message: str) -> str`

Synchronous wrapper around `chat()`. **Do not call from inside an async context.**

```python
response = agent.chat_sync("What is Rust?")
```

#### `stream(message: str) -> AsyncIterator[StreamEvent]`

Stream tokens as they arrive. See [Streaming](#7-streaming).

```python
async for event in agent.stream("Explain async/await"):
    ...
```

#### `run(message: str, *, max_turns: int = None, cwd: str = None) -> ConversationResult`

Full agent run with detailed result.

```python
result = await agent.run("Refactor the auth module")
print(result.response)           # Final text
print(result.cost.total_cost)    # USD cost
print(result.api_calls)          # Number of LLM calls
print(result.tool_errors)        # Any tool failures
```

**Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `max_turns` | `int` | `None` | Override `max_iterations` for this run only. |
| `cwd` | `str` | `None` | Working directory for file/terminal tools. |

#### `run_conversation(message, *, system, history) -> ConversationResult`

Full control over the conversation.

```python
result = await agent.run_conversation(
    "Summarize this",
    system="You are a summarizer. Be concise.",
    history=[
        Message.user("Here is the text: ..."),
        Message.assistant("I understand. What should I summarize?"),
    ],
)
```

#### `fork(**overrides) -> Agent` (async)

Create an independent agent copy with shared conversation history.

```python
# Fork with different model for a sub-task
cheap_agent = await agent.fork(model="anthropic/claude-haiku-3")
summary = await cheap_agent.chat("Summarize our conversation")
# Original agent's history is unaffected by cheap_agent's responses
```

#### `interrupt() -> None`

Cancel the current run. Safe to call from any thread.

```python
import signal

def handle_sigint(sig, frame):
    agent.interrupt()

signal.signal(signal.SIGINT, handle_sigint)
```

#### `new_session() -> None` (async)

Start a fresh conversation. Previous history is saved to SessionDb.

#### Properties

| Property | Type | Description |
|----------|------|-------------|
| `session_id` | `str` | Current session identifier |
| `history` | `list[Message]` | Current conversation messages |
| `model` | `str` | Active model name |
| `memory` | `MemoryManager` | Access agent memory (read/write) |
| `model_catalog` | `ModelCatalog` | Access model catalog |

---

## 2. Tool

### Decorator Pattern (Recommended)

```python
from edgecrab import Tool, ToolContext

@Tool("tool_name", description="What this tool does")
async def my_tool(
    required_param: str,
    optional_param: int = 10,
    ctx: ToolContext = None,  # Injected automatically if present
) -> dict | str:
    """Extended description (used as tool description if no explicit one).

    Args:
        required_param: Description shown to the model
        optional_param: Description shown to the model
    """
    return {"result": required_param}
```

**Schema auto-inference rules:**

| Python Type | JSON Schema Type | Notes |
|-------------|-----------------|-------|
| `str` | `"string"` | |
| `int` | `"integer"` | |
| `float` | `"number"` | |
| `bool` | `"boolean"` | |
| `list[str]` | `{"type": "array", "items": {"type": "string"}}` | |
| `Optional[T]` | Same as T, not in `required` | |
| `T = default` | Same as T, not in `required` | Default shown in description |
| Complex types | **Error** — must provide explicit schema | |

**Decorator parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `name` | `str` | required | Tool name (alphanumeric + underscore) |
| `description` | `str` | From docstring | Tool description for the model |
| `toolset` | `str` | `"custom"` | Toolset grouping |
| `timeout` | `int` | `120` | Execution timeout in seconds |
| `schema` | `dict` | Auto-inferred | Explicit JSON Schema for parameters |
| `emoji` | `str` | `"🔧"` | Display emoji in TUI |
| `parallel_safe` | `bool` | `True` | Whether tool can run concurrently |

### Class Pattern (Advanced)

```python
class MyTool(ToolHandler):
    name = "my_tool"
    toolset = "my_toolset"
    description = "What this tool does"
    emoji = "🔧"

    def schema(self) -> ToolSchema:
        return ToolSchema(...)

    async def execute(self, args: dict, ctx: ToolContext) -> str:
        return json.dumps({"result": "..."})

    def is_available(self) -> bool:
        return True  # Check requirements
```

### ToolContext

Provided to tools that accept a `ctx` parameter:

| Field | Type | Description |
|-------|------|-------------|
| `cwd` | `Path` | Working directory |
| `session_id` | `str` | Current session |
| `platform` | `Platform` | Current platform |
| `cancel_token` | `CancellationToken` | Check for cancellation |
| `agent_model` | `str` | Current model name |

### ToolSchema

```python
@dataclass
class ToolSchema:
    name: str
    description: str
    parameters: dict  # JSON Schema object
    strict: bool | None = None  # OpenAI strict mode
```

---

## 3. Types

### Message

```python
@dataclass
class Message:
    role: Role
    content: str | list[ContentPart] | None = None
    tool_calls: list[ToolCall] | None = None
    tool_call_id: str | None = None
    name: str | None = None
    reasoning: str | None = None

    @staticmethod
    def user(text: str) -> Message: ...
    @staticmethod
    def system(text: str) -> Message: ...
    @staticmethod
    def assistant(text: str) -> Message: ...
    @staticmethod
    def tool(call_id: str, content: str) -> Message: ...
```

### Role

```python
class Role(Enum):
    SYSTEM = "system"
    USER = "user"
    ASSISTANT = "assistant"
    TOOL = "tool"
```

### ConversationResult

```python
@dataclass
class ConversationResult:
    response: str                    # Final agent response text
    messages: list[Message]          # Full conversation trace
    session_id: str                  # Session identifier
    api_calls: int                   # Number of LLM API calls made
    interrupted: bool                # Was the run interrupted?
    budget_exhausted: bool           # Did we hit max_iterations?
    model: str                       # Model used
    usage: Usage                     # Token counts
    cost: Cost                       # USD cost breakdown
    tool_errors: list[ToolExecError] # Any tool execution failures
```

### Usage

```python
@dataclass
class Usage:
    input_tokens: int
    output_tokens: int
    cache_read_tokens: int
    cache_write_tokens: int
    reasoning_tokens: int
    total_tokens: int    # Computed: input + output + reasoning
```

### Cost

```python
@dataclass
class Cost:
    input_cost: float        # USD
    output_cost: float       # USD
    cache_read_cost: float   # USD
    cache_write_cost: float  # USD
    total_cost: float        # USD (sum of all)
```

### Platform

```python
class Platform(Enum):
    CLI = "cli"
    TELEGRAM = "telegram"
    DISCORD = "discord"
    SLACK = "slack"
    WHATSAPP = "whatsapp"
    API = "api"
    # ... 17 total platforms
```

---

## 4. Config

### Loading Configuration

```python
from edgecrab import Config

# Default: ~/.edgecrab/config.yaml
config = Config.load()

# Custom path
config = Config.load_from("./my-config.yaml")

# Programmatic
config = Config()
config.agent.max_iterations = 50
config.agent.streaming = True
```

### Configuration Structure

```yaml
# ~/.edgecrab/config.yaml
model: anthropic/claude-sonnet-4-20250514
max_iterations: 90
streaming: true
save_trajectories: false
skip_context_files: false
skip_memory: false

compression:
  threshold: 0.50
  protect_last_n: 20

mcp_servers:
  postgres:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-postgres"]
    env:
      DATABASE_URL: "postgresql://localhost/mydb"
  remote:
    url: "https://mcp.example.com/rpc"
    bearer_token: "sk-..."
    enabled: true
```

---

## 5. Session & Memory

### SessionDb

```python
from edgecrab import SessionDb

db = SessionDb.open()  # Default: ~/.edgecrab/sessions.db
# or
db = SessionDb.open("/custom/path/sessions.db")
```

| Method | Return | Description |
|--------|--------|-------------|
| `list_sessions(limit=20)` | `list[SessionRecord]` | Recent sessions |
| `get_session(id)` | `SessionRecord` | Session metadata |
| `get_messages(id)` | `list[Message]` | Full message history |
| `search_sessions(query, limit=20)` | `list[SearchHit]` | FTS5 full-text search |
| `delete_session(id)` | `None` | Delete session and messages |
| `get_insights(days=30)` | `InsightsReport` | Usage statistics |

### SessionRecord

```python
@dataclass
class SessionRecord:
    id: str
    title: str | None
    model: str
    created_at: datetime
    updated_at: datetime
    message_count: int
```

### SearchHit

```python
@dataclass
class SearchHit:
    session_id: str
    title: str | None
    snippet: str      # Highlighted matching text
    rank: float       # FTS5 relevance score
```

### MemoryManager

```python
# Access via agent.memory
agent = Agent("claude-sonnet-4")

# Read memory
content = await agent.memory.read("project-notes")

# Write memory
await agent.memory.write("project-notes", "Key decision: use PyO3")

# List memories
memories = await agent.memory.list()
```

---

## 6. Model Catalog

```python
from edgecrab import ModelCatalog

catalog = ModelCatalog.get()  # Singleton, thread-safe
```

| Method | Return | Description |
|--------|--------|-------------|
| `provider_ids()` | `list[str]` | All provider IDs |
| `models_for(provider)` | `list[ModelEntry]` | Models for a provider |
| `pricing(provider, model)` | `PricingPair` | Token pricing (USD per 1M tokens) |
| `context_window(provider, model)` | `int` | Context window size |
| `supports_vision(provider, model)` | `bool` | Vision capability |
| `supports_reasoning(provider, model)` | `bool` | Extended thinking |

### Supported Providers

| Provider ID | Provider Name | Example Models |
|-------------|--------------|----------------|
| `anthropic` | Anthropic | claude-opus-4, claude-sonnet-4, claude-haiku-3 |
| `openai` | OpenAI | gpt-4o, o3-mini, gpt-4.1 |
| `google` | Google | gemini-2.5-flash, gemini-2.5-pro |
| `deepseek` | DeepSeek | deepseek-chat, deepseek-reasoner |
| `mistral` | Mistral | mistral-large, codestral |
| `groq` | Groq | llama-3.3-70b-versatile |
| `copilot` | GitHub Copilot | gpt-4o (via Copilot) |
| `huggingface` | Hugging Face | meta-llama/Llama-3.3-70B |
| `openrouter` | OpenRouter | (proxies all providers) |
| `ollama` | Ollama (local) | llama3.3:70b, qwen2.5-coder |
| `xai` | xAI | grok-3, grok-3-mini |
| `zai` | Z.ai | zai-default |
| `vertexai` | Google Vertex AI | gemini-2.5-flash (via Vertex) |
| `bedrock` | AWS Bedrock | anthropic.claude-sonnet-4 |
| `lmstudio` | LM Studio (local) | lmstudio-community/qwen2.5-coder |

---

## 7. Streaming

### StreamEvent

```python
class StreamEvent:
    # Core variants
    Token(text: str)                        # Text token from model
    Reasoning(text: str)                    # Reasoning/thinking token
    ToolExec(name: str, args: str)          # Tool about to execute
    ToolProgress(name: str, progress: str)  # Tool progress update
    ToolDone(name: str, result: str)        # Tool finished
    Done()                                  # Stream complete
    Error(message: str)                     # Error occurred

    # Sub-agent variants
    SubAgentStart(task: str)                # Sub-agent delegation started
    SubAgentReasoning(text: str)            # Sub-agent reasoning token
    SubAgentToolExec(name: str, args: str)  # Sub-agent tool execution
    SubAgentFinish(result: str)             # Sub-agent completed

    # Interaction variants
    Clarify(question: str)                  # Agent asking user for clarification
    Approval(command: str, reasons: list)   # Agent requesting command approval
    SecretRequest(name: str)                # Agent requesting a secret value

    # System variants
    HookEvent(hook: str, data: dict)        # Lifecycle hook fired
    ContextPressure(usage_pct: float)       # Context window pressure warning

    # Convenience
    @property
    def is_token(self) -> bool: ...
    @property
    def is_done(self) -> bool: ...
```

### Usage Patterns

```python
# Pattern 1: Async iterator
async for event in agent.stream("hello"):
    if event.is_token:
        print(event.text, end="")

# Pattern 2: Callback (set in constructor)
agent = Agent("model", on_stream=lambda e: print(e.text) if e.is_token else None)
await agent.run("hello")

# Pattern 3: Collect all tokens
tokens = [e.text async for e in agent.stream("hello") if e.is_token]
full_response = "".join(tokens)
```

---

## 7.1 Event System — Observability for Web UIs

> **📖 Comprehensive specification:** See [11-EVENT-SYSTEM.md](11-EVENT-SYSTEM.md) for the full event system spec — complete event taxonomy (21 variants), transport layers (SSE, WebSocket), state machines, interactive events, time-travel debugging, competitor comparison, and a complete React dashboard example.

### Overview

The `StreamEvent` system is not just for streaming text — it's a **full observability bus** that exposes every stage of the agent lifecycle. This makes it trivial to build rich Web UIs that show tool execution timelines, cost meters, context pressure gauges, sub-agent progress, and real-time approval dialogs.

```
+------------------------------------------------------------------------+
|                    Event System Architecture                           |
+------------------------------------------------------------------------+
|                                                                        |
|  Agent Loop (Rust / WASM)                                              |
|  +------------------------------------------------------------------+ |
|  | ReAct Iteration                                                  | |
|  |                                                                  | |
|  |  LLM Call ──→ Token/Reasoning events ──→ ToolExec ──→ ToolDone  | |
|  |      │              │                        │            │      | |
|  |      │              │                        │            │      | |
|  |      ▼              ▼                        ▼            ▼      | |
|  +------│──────────────│────────────────────────│────────────│──────+  |
|         │              │                        │            │        |
|         ▼              ▼                        ▼            ▼        |
|  +------------------------------------------------------------------+ |
|  | StreamEvent Bus (UnboundedSender<StreamEvent>)                   | |
|  |                                                                  | |
|  |  ┌─────────┐ ┌───────────┐ ┌──────────┐ ┌──────────┐           | |
|  |  │ Token   │ │ ToolExec  │ │ ToolDone │ │ Context  │ ...       | |
|  |  │ (text)  │ │ (name,    │ │ (name,   │ │ Pressure │           | |
|  |  │         │ │  args,    │ │  result, │ │ (usage%, │           | |
|  |  │         │ │  call_id) │ │  dur_ms, │ │  thresh) │           | |
|  |  │         │ │           │ │  is_err) │ │          │           | |
|  |  └────┬────┘ └─────┬─────┘ └────┬─────┘ └────┬─────┘           | |
|  +───────│───────────│───────────│───────────│──────────────────────+ |
|          │           │           │           │                        |
|          ▼           ▼           ▼           ▼                        |
|  +------------------------------------------------------------------+ |
|  | Consumers: Web UI, TUI, Gateway, Logging, Metrics                | |
|  +------------------------------------------------------------------+ |
+------------------------------------------------------------------------+
```

### Complete StreamEvent Variants (Actual Codebase)

Every event variant includes enough metadata to drive a rich UI:

```typescript
// TypeScript discriminated union — mirrors Rust enum exactly
type StreamEvent =
  // ── Text streaming ──────────────────────────────────────────
  | { type: "token"; text: string }
  | { type: "reasoning"; text: string }

  // ── Tool lifecycle (with correlation ID) ────────────────────
  | { type: "tool_exec";
      toolCallId: string;          // correlation ID across exec→progress→done
      name: string;                // e.g. "web_search", "file_read"
      argsJson: string;            // raw JSON args (for preview)
    }
  | { type: "tool_progress";
      toolCallId: string;
      name: string;
      message: string;             // human-readable progress
    }
  | { type: "tool_done";
      toolCallId: string;
      name: string;
      argsJson: string;
      resultPreview: string | null; // short summary of result
      durationMs: number;           // elapsed time
      isError: boolean;             // did the tool fail?
    }

  // ── Sub-agent delegation ────────────────────────────────────
  | { type: "sub_agent_start";
      taskIndex: number;           // 0-based index in delegation batch
      taskCount: number;           // total tasks in batch
      goal: string;                // delegation goal text
    }
  | { type: "sub_agent_reasoning";
      taskIndex: number;
      taskCount: number;
      text: string;
    }
  | { type: "sub_agent_tool_exec";
      taskIndex: number;
      taskCount: number;
      name: string;
      argsJson: string;
    }
  | { type: "sub_agent_finish";
      taskIndex: number;
      taskCount: number;
      status: string;              // "success" | "error" | "interrupted"
      durationMs: number;
      summary: string;
      apiCalls: number;
      model: string | null;
    }

  // ── Interaction (require user response) ─────────────────────
  | { type: "clarify";
      question: string;
      choices: string[] | null;    // predefined options or open-ended
    }
  | { type: "approval";
      command: string;             // short description
      fullCommand: string;         // full command text
      reasons: string[];           // why approval is needed
    }
  | { type: "secret_request";
      varName: string;             // e.g. "OPENAI_API_KEY"
      prompt: string;              // display text
      isSudo: boolean;             // privilege escalation?
    }

  // ── System ──────────────────────────────────────────────────
  | { type: "hook_event";
      event: string;               // "tool:pre", "tool:post", "llm:pre", "llm:post"
      contextJson: string;         // serialized context payload
    }
  | { type: "context_pressure";
      estimatedTokens: number;     // current usage
      thresholdTokens: number;     // compression threshold
    }

  // ── Terminal ────────────────────────────────────────────────
  | { type: "done" }
  | { type: "error"; message: string };
```

### Web UI Observability Patterns

#### Pattern 1: Tool Execution Timeline

Build a visual timeline showing each tool call, its duration, success/failure, and result preview:

```typescript
// React component for tool execution timeline
import { Agent } from "@edgecrab/wasm";       // or Python/Node.js SDK
import { useState, useCallback } from "react";

interface ToolExecution {
  id: string;               // toolCallId — correlates exec→progress→done
  name: string;
  args: Record<string, any>;
  status: "running" | "success" | "error";
  startedAt: number;
  durationMs?: number;
  resultPreview?: string;
  progressMessages: string[];
}

function useAgentWithTimeline(agent: Agent) {
  const [tokens, setTokens] = useState("");
  const [tools, setTools] = useState<Map<string, ToolExecution>>(new Map());
  const [cost, setCost] = useState({ tokens: 0, usd: 0 });

  const chat = useCallback(async (message: string) => {
    setTokens("");
    setTools(new Map());

    for await (const event of agent.stream(message)) {
      switch (event.type) {
        case "token":
          setTokens(prev => prev + event.text);
          break;

        case "tool_exec":
          setTools(prev => {
            const next = new Map(prev);
            next.set(event.toolCallId, {
              id: event.toolCallId,
              name: event.name,
              args: JSON.parse(event.argsJson),
              status: "running",
              startedAt: Date.now(),
              progressMessages: [],
            });
            return next;
          });
          break;

        case "tool_progress":
          setTools(prev => {
            const next = new Map(prev);
            const tool = next.get(event.toolCallId);
            if (tool) {
              tool.progressMessages.push(event.message);
              next.set(event.toolCallId, { ...tool });
            }
            return next;
          });
          break;

        case "tool_done":
          setTools(prev => {
            const next = new Map(prev);
            const tool = next.get(event.toolCallId);
            if (tool) {
              next.set(event.toolCallId, {
                ...tool,
                status: event.isError ? "error" : "success",
                durationMs: event.durationMs,
                resultPreview: event.resultPreview ?? undefined,
              });
            }
            return next;
          });
          break;
      }
    }
  }, [agent]);

  return { tokens, tools, cost, chat };
}

// Render component
function ToolTimeline({ tools }: { tools: Map<string, ToolExecution> }) {
  return (
    <div className="tool-timeline">
      {Array.from(tools.values()).map(tool => (
        <div key={tool.id} className={`tool-card tool-${tool.status}`}>
          <div className="tool-header">
            <span className="tool-icon">{toolEmoji(tool.name)}</span>
            <span className="tool-name">{tool.name}</span>
            {tool.status === "running" && <Spinner />}
            {tool.durationMs != null && (
              <span className="tool-duration">{tool.durationMs}ms</span>
            )}
          </div>
          <div className="tool-args">
            <code>{JSON.stringify(tool.args, null, 2)}</code>
          </div>
          {tool.progressMessages.length > 0 && (
            <div className="tool-progress">
              {tool.progressMessages.map((msg, i) => (
                <div key={i} className="progress-msg">{msg}</div>
              ))}
            </div>
          )}
          {tool.resultPreview && (
            <div className="tool-result">{tool.resultPreview}</div>
          )}
        </div>
      ))}
    </div>
  );
}
```

#### Pattern 2: Context Pressure Gauge

Show a real-time gauge of context window usage with compression warnings:

```typescript
function ContextGauge({ event }: { event: StreamEvent & { type: "context_pressure" } }) {
  const usagePct = (event.estimatedTokens / event.thresholdTokens) * 100;
  const level = usagePct > 95 ? "critical" : usagePct > 85 ? "warning" : "ok";

  return (
    <div className={`context-gauge context-${level}`}>
      <div className="gauge-bar" style={{ width: `${Math.min(usagePct, 100)}%` }} />
      <span className="gauge-label">
        {event.estimatedTokens.toLocaleString()} / {event.thresholdTokens.toLocaleString()} tokens
        ({usagePct.toFixed(1)}%)
      </span>
      {level === "critical" && (
        <span className="gauge-warning">⚠ Compression imminent</span>
      )}
    </div>
  );
}
```

#### Pattern 3: Sub-Agent Progress Dashboard

When the agent delegates to sub-agents, show parallel task progress:

```typescript
interface SubAgentTask {
  index: number;
  total: number;
  goal: string;
  status: "running" | "done" | "error";
  reasoning: string;
  toolCalls: string[];
  durationMs?: number;
  apiCalls?: number;
  model?: string;
}

function SubAgentDashboard({ tasks }: { tasks: Map<number, SubAgentTask> }) {
  return (
    <div className="subagent-grid">
      {Array.from(tasks.values()).map(task => (
        <div key={task.index} className={`subagent-card subagent-${task.status}`}>
          <div className="subagent-header">
            Task {task.index + 1}/{task.total}
            {task.status === "running" && <Spinner size="sm" />}
            {task.status === "done" && <span>✓</span>}
          </div>
          <div className="subagent-goal">{task.goal}</div>
          {task.toolCalls.length > 0 && (
            <div className="subagent-tools">
              {task.toolCalls.map((name, i) => (
                <span key={i} className="tool-badge">{name}</span>
              ))}
            </div>
          )}
          {task.durationMs != null && (
            <div className="subagent-stats">
              {task.durationMs}ms · {task.apiCalls} API calls
              {task.model && ` · ${task.model}`}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
```

#### Pattern 4: Approval Dialog (Interactive)

Handle the `approval` event by rendering a dialog and sending the user's choice back:

```typescript
// For native SDKs (Python/Node.js), use the on_stream callback:
const agent = new Agent("anthropic/claude-sonnet-4", {
  onApproval: async (event) => {
    // Render a dialog and wait for user response
    const choice = await showApprovalDialog({
      command: event.command,
      fullCommand: event.fullCommand,
      reasons: event.reasons,
      options: ["approve_once", "approve_session", "approve_always", "deny"],
    });
    return choice;  // sends back to agent via response_tx
  },
});

// For WASM SDK, use the stream event loop:
for await (const event of agent.stream(message)) {
  if (event.type === "approval") {
    // Pause streaming, show dialog
    const choice = await renderApprovalModal(event);
    agent.respondToApproval(choice);  // unblocks the agent
  }
}
```

#### Pattern 5: Full Observability Dashboard (Complete Example)

A complete Web UI that combines all patterns:

```typescript
// dashboard.tsx — Full agent observability dashboard
import init, { Agent, Tool } from "@edgecrab/wasm";
import { useReducer, useEffect } from "react";

// ── State machine for the entire dashboard ──────────────────
type DashboardState = {
  response: string;
  reasoning: string;
  tools: Map<string, ToolExecution>;
  subAgents: Map<number, SubAgentTask>;
  contextPressure: { estimated: number; threshold: number } | null;
  hooks: Array<{ event: string; timestamp: number; context: any }>;
  pendingApproval: ApprovalEvent | null;
  pendingClarify: ClarifyEvent | null;
  status: "idle" | "streaming" | "waiting_approval" | "waiting_clarify" | "done" | "error";
  error: string | null;
};

type DashboardAction =
  | { type: "reset" }
  | { type: "event"; event: StreamEvent };

function dashboardReducer(state: DashboardState, action: DashboardAction): DashboardState {
  if (action.type === "reset") return initialState;

  const event = action.event;
  switch (event.type) {
    case "token":
      return { ...state, status: "streaming", response: state.response + event.text };

    case "reasoning":
      return { ...state, reasoning: state.reasoning + event.text };

    case "tool_exec": {
      const tools = new Map(state.tools);
      tools.set(event.toolCallId, {
        id: event.toolCallId,
        name: event.name,
        args: JSON.parse(event.argsJson),
        status: "running",
        startedAt: Date.now(),
        progressMessages: [],
      });
      return { ...state, tools };
    }

    case "tool_progress": {
      const tools = new Map(state.tools);
      const tool = tools.get(event.toolCallId);
      if (tool) {
        tools.set(event.toolCallId, {
          ...tool,
          progressMessages: [...tool.progressMessages, event.message],
        });
      }
      return { ...state, tools };
    }

    case "tool_done": {
      const tools = new Map(state.tools);
      const tool = tools.get(event.toolCallId);
      if (tool) {
        tools.set(event.toolCallId, {
          ...tool,
          status: event.isError ? "error" : "success",
          durationMs: event.durationMs,
          resultPreview: event.resultPreview ?? undefined,
        });
      }
      return { ...state, tools };
    }

    case "sub_agent_start": {
      const subAgents = new Map(state.subAgents);
      subAgents.set(event.taskIndex, {
        index: event.taskIndex,
        total: event.taskCount,
        goal: event.goal,
        status: "running",
        reasoning: "",
        toolCalls: [],
      });
      return { ...state, subAgents };
    }

    case "sub_agent_finish": {
      const subAgents = new Map(state.subAgents);
      const task = subAgents.get(event.taskIndex);
      if (task) {
        subAgents.set(event.taskIndex, {
          ...task,
          status: event.status === "success" ? "done" : "error",
          durationMs: event.durationMs,
          apiCalls: event.apiCalls,
          model: event.model ?? undefined,
        });
      }
      return { ...state, subAgents };
    }

    case "context_pressure":
      return {
        ...state,
        contextPressure: {
          estimated: event.estimatedTokens,
          threshold: event.thresholdTokens,
        },
      };

    case "hook_event":
      return {
        ...state,
        hooks: [...state.hooks, {
          event: event.event,
          timestamp: Date.now(),
          context: JSON.parse(event.contextJson),
        }],
      };

    case "approval":
      return { ...state, status: "waiting_approval", pendingApproval: event };

    case "clarify":
      return { ...state, status: "waiting_clarify", pendingClarify: event };

    case "done":
      return { ...state, status: "done" };

    case "error":
      return { ...state, status: "error", error: event.message };

    default:
      return state;
  }
}

// ── Dashboard Component ─────────────────────────────────────
function AgentDashboard() {
  const [state, dispatch] = useReducer(dashboardReducer, initialState);
  const [agent, setAgent] = useState<Agent | null>(null);

  async function handleSend(message: string) {
    if (!agent) return;
    dispatch({ type: "reset" });

    for await (const event of agent.stream(message)) {
      dispatch({ type: "event", event });
    }
  }

  return (
    <div className="dashboard">
      {/* Main response panel */}
      <ResponsePanel text={state.response} status={state.status} />

      {/* Reasoning sidebar (collapsible) */}
      {state.reasoning && <ReasoningPanel text={state.reasoning} />}

      {/* Tool execution timeline */}
      <ToolTimeline tools={state.tools} />

      {/* Sub-agent progress grid */}
      {state.subAgents.size > 0 && <SubAgentDashboard tasks={state.subAgents} />}

      {/* Context pressure gauge */}
      {state.contextPressure && <ContextGauge {...state.contextPressure} />}

      {/* Hook event log (developer mode) */}
      <HookEventLog hooks={state.hooks} />

      {/* Modal dialogs */}
      {state.pendingApproval && (
        <ApprovalDialog
          event={state.pendingApproval}
          onRespond={(choice) => agent?.respondToApproval(choice)}
        />
      )}
      {state.pendingClarify && (
        <ClarifyDialog
          event={state.pendingClarify}
          onRespond={(answer) => agent?.respondToClarify(answer)}
        />
      )}

      {/* Error banner */}
      {state.error && <ErrorBanner message={state.error} />}
    </div>
  );
}
```

### Event System Design Principles

| Principle | Implementation |
|-----------|---------------|
| **Correlation** | Every tool event carries a `toolCallId` that correlates `ToolExec` → `ToolProgress` → `ToolDone`. Sub-agent events carry `taskIndex`. This enables UI components to track individual operations. |
| **Timing** | `ToolDone.durationMs` and `SubAgentFinish.durationMs` provide precise timing without requiring the UI to track start/end times manually. |
| **Error context** | `ToolDone.isError` + `resultPreview` gives the UI enough to render error states without parsing result JSON. `Error(message)` provides the top-level failure reason. |
| **Backpressure** | The `StreamEvent` bus uses an unbounded channel (`tokio::sync::mpsc::UnboundedSender`). The UI consumer processes events as fast as possible. If the UI is slower than the agent, events queue in memory. For browser WASM, this is fine — the event loop is single-threaded and processes synchronously. |
| **Interactivity** | `Clarify`, `Approval`, and `SecretRequest` events carry a `response_tx` (in native SDKs) or require an explicit `agent.respondTo*()` call (WASM). The agent loop **blocks** until the user responds. |
| **Extensibility** | `HookEvent` is a catch-all for custom lifecycle hooks. Gateway adapters, plugins, and user code can emit arbitrary hook events that the UI can render without SDK changes. |

### Event Filtering & Subscription (Advanced)

For high-frequency events (tokens arrive every ~10-50ms), the SDK supports filtered subscriptions:

```python
# Python — filtered stream
async for event in agent.stream("analyze this", 
    filter=["tool_exec", "tool_done", "context_pressure", "done"]):
    # Only receives tool lifecycle + system events
    # Tokens are silently consumed and accumulated in agent.last_response
    handle_ui_event(event)

# Python — separate callbacks for different concerns
agent = Agent("model",
    on_stream=lambda e: update_response(e),     # tokens → response panel
    on_tool_call=lambda e: add_to_timeline(e),   # tools → timeline
    on_tool_result=lambda e: update_timeline(e), # results → timeline
)
```

```typescript
// TypeScript / WASM — event grouping for batched UI updates
import { batchEvents } from "@edgecrab/wasm/utils";

// Batch token events into 50ms windows to reduce React re-renders
const batched = batchEvents(agent.stream(message), {
  batchWindow: 50,  // ms
  batchTypes: ["token", "reasoning"],  // only batch text events
  passThrough: ["tool_exec", "tool_done", "done", "error"],  // emit immediately
});

for await (const events of batched) {
  // events is either:
  //   - a single tool/done/error event (passed through immediately)
  //   - an array of batched token events (accumulated over 50ms)
  if (Array.isArray(events)) {
    const text = events.map(e => e.text).join("");
    appendToResponse(text);  // single DOM update for all tokens in window
  } else {
    handleEvent(events);
  }
}
```

### Metrics & Tracing Integration

The event system integrates with standard observability stacks:

```python
# OpenTelemetry integration via hook events
import opentelemetry.trace as trace

tracer = trace.get_tracer("edgecrab-agent")

async def trace_agent(agent: Agent, message: str):
    with tracer.start_as_current_span("agent.chat") as span:
        span.set_attribute("agent.model", agent.model)

        async for event in agent.stream(message):
            match event:
                case StreamEvent.ToolExec(name=name, tool_call_id=tid):
                    span.add_event("tool.start", {"tool.name": name, "tool.call_id": tid})
                case StreamEvent.ToolDone(name=name, duration_ms=dur, is_error=err):
                    span.add_event("tool.end", {
                        "tool.name": name,
                        "tool.duration_ms": dur,
                        "tool.is_error": err,
                    })
                case StreamEvent.ContextPressure(estimated_tokens=est, threshold_tokens=thr):
                    span.set_attribute("context.usage_pct", est / thr * 100)
                case StreamEvent.Done():
                    span.set_status(trace.StatusCode.OK)
                case StreamEvent.Error(message=msg):
                    span.set_status(trace.StatusCode.ERROR, msg)
```

---

## 8. Errors

### Exception Hierarchy

```python
class EdgeCrabError(Exception): ...

class AgentError(EdgeCrabError):
    class LlmError(AgentError): ...
    class ContextLimitError(AgentError):
        used: int
        limit: int
    class BudgetExhaustedError(AgentError):
        used: int
        max: int
    class InterruptedError(AgentError): ...
    class ConfigError(AgentError): ...
    class RateLimitedError(AgentError):
        provider: str
        retry_after_ms: int
    class CompressionFailedError(AgentError): ...
    class ApiRefusalError(AgentError): ...

class ToolError(EdgeCrabError):
    class ToolNotFoundError(ToolError): ...
    class InvalidArgsError(ToolError):
        tool: str
    class ToolUnavailableError(ToolError):
        tool: str
        reason: str
    class ToolTimeoutError(ToolError):
        tool: str
        seconds: int
    class PermissionDeniedError(ToolError): ...
    class ExecutionFailedError(ToolError):
        tool: str

class ConnectionError(EdgeCrabError): ...
class AuthenticationError(EdgeCrabError): ...
```

### Error Handling Patterns

```python
from edgecrab import Agent, AgentError

try:
    result = await agent.run("complex task")
except AgentError.RateLimitedError as e:
    await asyncio.sleep(e.retry_after_ms / 1000)
    result = await agent.run("complex task")  # Retry
except AgentError.BudgetExhaustedError:
    print("Agent hit iteration limit — task too complex")
except AgentError.InterruptedError:
    print("Agent was interrupted by user")
except AgentError as e:
    print(f"Agent error: {e}")
```

---

## 9. MCP Integration

### McpServer Configuration

```python
from edgecrab import McpServer

# Stdio transport (subprocess)
server = McpServer(
    command="npx",
    args=["-y", "@modelcontextprotocol/server-postgres"],
    env={"DATABASE_URL": "postgresql://localhost/mydb"},
    cwd="/app",
)

# HTTP transport
server = McpServer(
    url="https://mcp.example.com/rpc",
    bearer_token="sk-...",
)
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `command` | `str` | None | Executable for stdio transport |
| `args` | `list[str]` | `[]` | Command arguments |
| `env` | `dict` | `{}` | Environment variables |
| `cwd` | `str` | None | Working directory |
| `url` | `str` | None | URL for HTTP transport |
| `bearer_token` | `str` | None | Bearer token for HTTP auth |
| `enabled` | `bool` | `True` | Whether server is active |

MCP tools are discovered automatically on first use and added to the agent's tool registry with the prefix `mcp__<server_name>__<tool_name>`.

---

## 10. Security

### Built-in Security Stack

EdgeCrab SDK inherits the full security stack from the runtime:

```
+----------------------------------------------------------+
|                    Security Layers                        |
+----------------------------------------------------------+
|                                                          |
|  Layer 1: Path Safety                                    |
|  - All file operations validate paths against jail root  |
|  - Prevents traversal (../../etc/passwd)                 |
|  - Symlink resolution before validation                  |
|                                                          |
|  Layer 2: SSRF Guard                                     |
|  - All HTTP requests validate URLs                       |
|  - Blocks private IPs (10.x, 172.16.x, 192.168.x, 127.x)|
|  - Blocks link-local, IPv6 loopback                      |
|                                                          |
|  Layer 3: Command Injection Scanner                      |
|  - Terminal commands scanned for shell metacharacters     |
|  - Blocks command chaining (;, &&, ||, |)                |
|  - Blocks subshell execution ($(), ``)                   |
|                                                          |
|  Layer 4: Prompt Injection Detection                     |
|  - Context files scanned for injection patterns          |
|  - Invisible Unicode detection                           |
|  - Homoglyph detection                                   |
|                                                          |
|  Layer 5: Secret Redaction                               |
|  - API keys, tokens, passwords redacted from output      |
|  - Pattern-based detection (sk-..., ghp_..., etc.)       |
|                                                          |
|  Layer 6: Skill Security Scanner                         |
|  - External skills scanned for 23 threat patterns        |
|  - Severity scoring before installation                  |
+----------------------------------------------------------+
```

### Approval Handler

```python
from edgecrab import ApprovalRequest, ApprovalResponse

@dataclass
class ApprovalRequest:
    command: str          # The command or action
    tool: str             # Tool name requesting approval
    reasons: list[str]    # Why this is flagged
    severity: str         # "low", "medium", "high", "critical"

class ApprovalResponse(Enum):
    ONCE = "once"           # Allow this one time
    SESSION = "session"     # Allow for rest of session
    ALWAYS = "always"       # Allow permanently
    DENY = "deny"           # Block the action
```

---

## 11. Advanced Patterns

### DelegationConfig

```python
@dataclass
class DelegationConfig:
    enabled: bool = False
    model: str | None = None              # Model for sub-agents
    max_subagents: int = 5                # Max concurrent sub-agents
    max_iterations_per_subagent: int = 30  # Per-subagent iteration limit
```

### CompressionConfig

```python
@dataclass
class CompressionConfig:
    threshold: float = 0.50       # Compress at 50% context usage
    protect_last_n: int = 20      # Always keep last N messages
```

### Context Manager

```python
async with Agent("claude-sonnet-4") as agent:
    result = await agent.run("Do something")
# Agent resources (tokio runtime, DB connections) cleaned up
```

### Batch Processing

```python
import asyncio
from edgecrab import Agent

async def process_batch(items: list[str]) -> list[str]:
    agent = Agent("claude-haiku-3")
    tasks = [agent.chat(item) for item in items]
    return await asyncio.gather(*tasks)
    # Note: concurrent calls share the same agent session
```

---

## Brutal Honest Assessment

### Strengths
- Complete API reference for every public type and method
- Parameter tables with types, defaults, and descriptions
- Error handling patterns with real recovery logic
- Security documentation is specific, not hand-wavy

### Weaknesses
- **No runnable code samples** — examples assume SDK exists; must be validated when SDK is built
- **Missing: migration guide** — developers coming from Pydantic AI or LangChain need step-by-step
- **Missing: performance guide** — when to use streaming vs run, how to optimize token usage
- **Missing: deployment guide** — Docker, Kubernetes, Lambda, Cloud Run recipes

### Improvements Made After Assessment
- Added ToolContext field documentation
- Added batch processing pattern
- Added context manager pattern for cleanup
- Added MCP prefix naming convention (`mcp__server__tool`)
- Added all 15 provider IDs with example models
