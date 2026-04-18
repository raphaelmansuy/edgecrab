# EdgeCrab SDK — Full Specification

> **Cross-references:** [WHY](01-WHY.md) | [COMPARISON](03-COMPARISON.md) | [TUTORIAL](04-TUTORIAL.md) | [ADR](05-ADR.md) | [IMPL](06-IMPLEMENTATION.md)

---

## 1. Overview

EdgeCrab SDK is the canonical quad-language SDK (Rust, Python, Node.js, WASM) that provides programmatic access to the EdgeCrab agent runtime. It surfaces the full capabilities of the EdgeCrab framework — agent creation, tool execution, streaming, session persistence, multi-provider routing, security, skills, profile-aware configuration, and prompt-context management — through ergonomic, type-safe APIs. Legacy HTTP-client SDKs are compatibility layers only and are no longer the primary surface.

### 1.1 Design Goals

| Goal | Metric |
|------|--------|
| Time to first agent | < 30 seconds from install |
| API surface parity | 100% feature parity across Rust/Python/Node.js |
| Performance overhead | < 5% vs direct Rust calls for Python/Node.js |
| Type coverage | 100% typed — no `Any` or `unknown` in public API |
| Security by default | All tool operations inherit edgecrab-security |
| Profile awareness | Explicit selection of isolated homes/config via profile-aware config loading |
| Context transparency | AGENTS.md, SOUL.md, memory, and skills are visible and controllable |

### 1.2 Architecture

```
+----------------------------------------------------------------------+
|                     EdgeCrab SDK v2 Architecture                      |
+----------------------------------------------------------------------+
|                                                                      |
|  +---------------+  +---------------+  +---------------+            |
|  |  Python SDK   |  | Node.js SDK   |  |   Rust SDK    |            |
|  |  (edgecrab)   |  | (edgecrab)    |  |  (edgecrab)   |            |
|  +-------+-------+  +-------+-------+  +-------+-------+            |
|          |                  |                   |                     |
|  +-------v-------+  +------v--------+          |                     |
|  | PyO3 Bindings |  | Napi-RS Binds |          |                     |
|  +-------+-------+  +------+--------+          |                     |
|          |                 |                    |                     |
|          +--------+--------+--------------------+                    |
|                   |                                                  |
|        +----------v-----------+                                      |
|        |  edgecrab-sdk-core   |  <-- NEW CRATE                      |
|        +----------+-----------+                                      |
|                   |                                                  |
|        +----------v-----------+                                      |
|        |  edgecrab-core       |  Existing agent runtime              |
|        |  edgecrab-tools      |  100+ tools                         |
|        |  edgecrab-security   |  Security stack                      |
|        |  edgecrab-state      |  Session persistence                 |
|        |  edgecrab-plugins    |  Plugin system                       |
|        +----------------------+                                      |
|                                                                      |
|  +---------------------------------------------------------------+   |
|  |  WASM SDK (Lite)  — @edgecrab/wasm                            |   |
|  |  +-------------+  +-----------+  +------------+               |   |
|  |  | Agent Loop  |  | Custom    |  | Streaming  |               |   |
|  |  | (ReAct)     |  | Tools (JS)|  | (callback) |               |   |
|  |  +-------------+  +-----------+  +------------+               |   |
|  |  Targets: browser, Cloudflare Workers, Deno, Vercel Edge      |   |
|  |  Compiled via wasm-bindgen + wasm-pack                        |   |
|  |  No built-in tools (file I/O, terminal, etc.)                 |   |
|  +---------------------------------------------------------------+   |
+----------------------------------------------------------------------+
```

### 1.3 Communication Modes

The SDK supports two operational modes:

```
+--------------------------------------------------+
|          Mode 1: EMBEDDED (Recommended)          |
+--------------------------------------------------+
|                                                  |
|  Python/Node.js process                          |
|  +--------------------------------------------+ |
|  | SDK (PyO3/Napi)                             | |
|  |   +--------------------------------------+ | |
|  |   | edgecrab-sdk-core (Rust)              | | |
|  |   |   +--------------------------------+ | | |
|  |   |   | edgecrab-core (Agent runtime)   | | | |
|  |   |   | tokio async runtime             | | | |
|  |   |   +--------------------------------+ | | |
|  |   +--------------------------------------+ | |
|  +--------------------------------------------+ |
|                                                  |
|  Zero IPC. Zero serialization overhead.          |
|  Agent loop runs in-process on Rust tokio.       |
+--------------------------------------------------+

+--------------------------------------------------+
|          Mode 2: CLIENT (Fallback/Remote)        |
+--------------------------------------------------+
|                                                  |
|  Python/Node.js process     EdgeCrab Server      |
|  +------------------+      +------------------+ |
|  | SDK Client       | HTTP | API Server       | |
|  | (requests/fetch) |----->| (/v1/chat/...)   | |
|  +------------------+      +------------------+ |
|                                                  |
|  For environments where native builds fail.      |
|  HTTP/SSE based. OpenAI-compatible endpoint.     |
+--------------------------------------------------+
```

---

## 2. Core Types

### 2.1 Message

```
+------------------------------------------+
|                Message                    |
+------------------------------------------+
| role: Role                               |
| content: Option<Content>                 |
| tool_calls: Option<Vec<ToolCall>>        |
| tool_call_id: Option<String>             |
| name: Option<String>                     |
| reasoning: Option<String>                |
+------------------------------------------+

+------------------+
|      Role        |
+------------------+
| System           |
| User             |
| Assistant        |
| Tool             |
+------------------+

+------------------------------------------+
|              Content                      |
+------------------------------------------+
| Text(String)                             |
| Parts(Vec<ContentPart>)                  |
+------------------------------------------+

+------------------------------------------+
|           ContentPart                     |
+------------------------------------------+
| Text { text: String }                    |
| ImageUrl { url: String, detail?: String }|
+------------------------------------------+
```

#### Python

```python
class Role(Enum):
    SYSTEM = "system"
    USER = "user"
    ASSISTANT = "assistant"
    TOOL = "tool"

@dataclass
class Message:
    role: Role
    content: str | list[ContentPart] | None = None
    tool_calls: list[ToolCall] | None = None
    tool_call_id: str | None = None
    name: str | None = None
    reasoning: str | None = None

    @staticmethod
    def user(text: str) -> "Message": ...
    @staticmethod
    def system(text: str) -> "Message": ...
    @staticmethod
    def assistant(text: str) -> "Message": ...
```

#### TypeScript

```typescript
enum Role {
    System = "system",
    User = "user",
    Assistant = "assistant",
    Tool = "tool",
}

interface Message {
    role: Role;
    content?: string | ContentPart[];
    toolCalls?: ToolCall[];
    toolCallId?: string;
    name?: string;
    reasoning?: string;
}
```

#### Rust

```rust
// Re-exported from edgecrab-types
pub use edgecrab_types::{Message, Role, Content, ContentPart};
```

### 2.2 Agent

The central type. Constructed via builder pattern (Rust) or keyword arguments (Python/Node.js).

```
+------------------------------------------------------------+
|                        Agent                                |
+------------------------------------------------------------+
|                                                            |
|  CONSTRUCTION                                              |
|  +------------------------------------------------------+ |
|  | Agent(model)              Minimal constructor          | |
|  | Agent(model, **kwargs)    Full config                  | |
|  | AgentBuilder::new(model)  Rust builder pattern          | |
|  +------------------------------------------------------+ |
|                                                            |
|  SIMPLE API                                                |
|  +------------------------------------------------------+ |
|  | .chat(message) -> str         One-shot question        | |
|  | .stream(message) -> Stream    Streaming tokens         | |
|  | .run(task, **kw) -> Result    Full ReAct loop          | |
|  +------------------------------------------------------+ |
|                                                            |
|  ADVANCED API                                              |
|  +------------------------------------------------------+ |
|  | .run_conversation(msg, sys, history) -> Result         | |
|  | .fork() -> Agent              Branch conversation      | |
|  | .interrupt()                  Cancel current run       | |
|  | .new_session()                Reset conversation       | |
|  +------------------------------------------------------+ |
|                                                            |
|  SESSION                                                   |
|  +------------------------------------------------------+ |
|  | .session_id -> str            Current session          | |
|  | .history -> list[Message]     Conversation history     | |
|  | .search_sessions(q) -> [Hit]  FTS5 search              | |
|  | .export() -> SessionExport    Full export              | |
|  +------------------------------------------------------+ |
|                                                            |
|  MEMORY                                                    |
|  +------------------------------------------------------+ |
|  | .memory.read(key) -> str      Read memory              | |
|  | .memory.write(key, val)       Write memory             | |
|  +------------------------------------------------------+ |
|                                                            |
|  CONFIGURATION                                             |
|  +------------------------------------------------------+ |
|  | .model: str                                           | |
|  | .max_iterations: int                                  | |
|  | .temperature: float?                                  | |
|  | .tools: list[Tool]                                    | |
|  | .toolsets: list[str]                                  | |
|  | .platform: Platform                                   | |
|  | .streaming: bool                                      | |
|  | .reasoning_effort: str?                               | |
|  +------------------------------------------------------+ |
+------------------------------------------------------------+
```

#### Python API

```python
class Agent:
    def __init__(
        self,
        model: str = "anthropic/claude-sonnet-4-20250514",
        *,
        # Tool configuration
        tools: list[Tool] | None = None,
        toolsets: list[str] | None = None,
        disabled_toolsets: list[str] | None = None,
        disabled_tools: list[str] | None = None,
        # Agent behavior
        instructions: str | None = None,
        max_iterations: int = 90,
        temperature: float | None = None,
        reasoning_effort: str | None = None,
        # Session
        session_id: str | None = None,
        # Streaming
        on_stream: Callable[[StreamEvent], None] | None = None,
        on_tool_call: Callable[[ToolExecEvent], None] | None = None,
        on_tool_result: Callable[[ToolResultEvent], None] | None = None,
        # Context / memory control
        skip_context_files: bool | None = None,
        skip_memory: bool | None = None,
        # Security
        approval_handler: Callable[[ApprovalRequest], ApprovalResponse] | None = None,
        # Platform
        platform: Platform = Platform.CLI,
        # Config
        config: Config | None = None,
        config_path: str | None = None,
    ): ...

    @staticmethod
    def from_config(config: Config) -> "Agent": ...

    # --- Simple API ---
    async def chat(self, message: str) -> str: ...
    def chat_sync(self, message: str) -> str: ...

    # --- Streaming API ---
    async def stream(self, message: str) -> AsyncIterator[StreamEvent]: ...

    # --- Full API ---
    async def run(
        self,
        message: str,
        *,
        max_turns: int | None = None,
        cwd: str | Path | None = None,
    ) -> ConversationResult: ...

    # --- Conversation API ---
    async def run_conversation(
        self,
        message: str,
        *,
        system: str | None = None,
        history: list[Message] | None = None,
    ) -> ConversationResult: ...

    # --- Session management ---
    async def fork(self, **overrides) -> "Agent": ...
    def interrupt(self) -> None: ...
    async def new_session(self) -> None: ...

    @property
    def session_id(self) -> str: ...

    @property
    def history(self) -> list[Message]: ...

    async def search_sessions(
        self, query: str, limit: int = 20
    ) -> list[SessionSearchHit]: ...

    async def export(self) -> SessionExport: ...

    # --- Memory ---
    @property
    def memory(self) -> MemoryManager: ...

    # --- Configuration ---
    @property
    def model_catalog(self) -> ModelCatalog: ...
```

#### TypeScript API

```typescript
class Agent {
    constructor(model?: string, options?: AgentOptions);
    static fromConfig(config: Config, options?: AgentOptions): Agent;

    // Simple API
    chat(message: string): Promise<string>;

    // Streaming API
    stream(message: string): AsyncIterable<StreamEvent>;

    // Full API
    run(message: string, options?: RunOptions): Promise<ConversationResult>;

    // Conversation API
    runConversation(
        message: string,
        options?: ConversationOptions,
    ): Promise<ConversationResult>;

    // Session management
    fork(overrides?: Partial<AgentOptions>): Promise<Agent>;
    interrupt(): void;
    newSession(): Promise<void>;

    get sessionId(): string;
    get history(): Message[];
    searchSessions(query: string, limit?: number): Promise<SessionSearchHit[]>;
    export(): Promise<SessionExport>;

    // Memory
    get memory(): MemoryManager;

    // Model catalog
    get modelCatalog(): ModelCatalog;
}

interface Config {
    load(): Config;
    loadFrom(path: string): Config;
    loadProfile(name: string): Config;
}
```

#### Rust API

```rust
pub struct Agent { /* internal */ }

impl Agent {
    // Construction
    pub fn new(model: &str) -> AgentBuilder;
    pub fn from_config(config: &AppConfig) -> Result<Self, AgentError>;

    // Simple API
    pub async fn chat(&self, message: &str) -> Result<String, AgentError>;
    pub async fn chat_in_cwd(&self, msg: &str, cwd: &Path) -> Result<String, AgentError>;

    // Streaming API
    pub fn stream(&self, message: &str) -> impl Stream<Item = StreamEvent>;

    // Full API
    pub async fn run(&self, message: &str) -> Result<ConversationResult, AgentError>;
    pub async fn run_conversation(
        &self,
        message: &str,
        system: Option<&str>,
        history: Option<Vec<Message>>,
    ) -> Result<ConversationResult, AgentError>;

    // Session
    pub async fn fork(&self, options: ForkOptions) -> Result<Self, AgentError>;
    pub fn interrupt(&self);
    pub async fn new_session(&self);
    pub fn session_id(&self) -> &str;
    pub fn history(&self) -> &[Message];
    pub async fn search_sessions(&self, q: &str, limit: i64)
        -> Result<Vec<SessionSearchHit>, AgentError>;

    // Memory
    pub fn memory(&self) -> &MemoryManager;
}
```

### 2.2.1 Profile, Skills, and Context Surface

The SDK must expose the same runtime context controls used by the CLI and gateway:

- **Profile selection** — via config loading from named profile homes or explicit config paths
- **Skills / tools configuration** — via `toolsets`, `disabled_toolsets`, and `disabled_tools`
- **AGENTS.md / SOUL.md / memory injection** — enabled by default and controllable with `skip_context_files` and `skip_memory`
- **Home-directory awareness** — via `edgecrab_home()` / `ensure_edgecrab_home()` utilities and profile-specific config files

These are part of the public SDK surface, not hidden implementation details.

### 2.2.2 Agent Lifecycle Controls

Beyond construction and simple chat, the SDK surfaces these agent lifecycle operations:

```
+------------------------------------------------------------+
|                  Agent Lifecycle                            |
+------------------------------------------------------------+
|                                                            |
|  COMPRESSION                                               |
|  +------------------------------------------------------+ |
|  | .compress()                Trigger manual compression  | |
|  +------------------------------------------------------+ |
|                                                            |
|  MODEL CONTROL                                             |
|  +------------------------------------------------------+ |
|  | .set_reasoning_effort(lvl) Set reasoning effort       | |
|  | .set_streaming(enabled)    Toggle streaming            | |
|  +------------------------------------------------------+ |
|                                                            |
|  CWD OVERRIDE                                              |
|  +------------------------------------------------------+ |
|  | .chat_in_cwd(msg, cwd)     Chat with working dir      | |
|  +------------------------------------------------------+ |
|                                                            |
|  INTROSPECTION                                             |
|  +------------------------------------------------------+ |
|  | .session_snapshot -> dict   Full session state         | |
|  | .tool_names -> [str]        Active tool list           | |
|  | .toolset_summary -> [(s,n)] Toolset name + count      | |
|  | .is_cancelled -> bool       Cancellation state         | |
|  +------------------------------------------------------+ |
+------------------------------------------------------------+
```

**Code evidence:**
- `SdkAgent::chat_in_cwd()` — `crates/edgecrab-sdk-core/src/agent.rs`
- `SdkAgent::session_snapshot()` — `crates/edgecrab-sdk-core/src/agent.rs`
- `Agent::force_compress()` — `crates/edgecrab-core/src/agent.rs:1049`

### 2.3 Tool

Custom tools are defined as decorated functions (Python/Node.js) or trait implementations (Rust).

#### Python

```python
from edgecrab import Tool, ToolContext

# Decorator style (recommended)
@Tool("get_weather", description="Get weather for a city")
async def get_weather(city: str, units: str = "celsius") -> dict:
    """Get current weather for a city.

    Args:
        city: City name
        units: Temperature units (celsius or fahrenheit)
    """
    return {"city": city, "temp": 22, "units": units}

# Class style (advanced)
class DatabaseTool(ToolHandler):
    name = "query_db"
    toolset = "database"
    description = "Execute a database query"

    def schema(self) -> ToolSchema:
        return ToolSchema(
            name="query_db",
            description="Execute a read-only database query",
            parameters={
                "type": "object",
                "properties": {
                    "sql": {"type": "string", "description": "SQL query"}
                },
                "required": ["sql"]
            }
        )

    async def execute(self, args: dict, ctx: ToolContext) -> str:
        # ctx.cwd, ctx.session_id, ctx.platform available
        result = await self.db.execute(args["sql"])
        return json.dumps(result)
```

#### TypeScript

```typescript
import { Tool, ToolContext } from "edgecrab";

// Decorator style
const getWeather = Tool.create({
    name: "get_weather",
    description: "Get weather for a city",
    parameters: {
        city: { type: "string", description: "City name" },
        units: { type: "string", description: "Temperature units", default: "celsius" },
    },
    handler: async (args, ctx) => {
        return { city: args.city, temp: 22, units: args.units };
    },
});

// Class style
class DatabaseTool implements ToolHandler {
    name = "query_db";
    toolset = "database";

    schema(): ToolSchema { ... }
    async execute(args: Record<string, unknown>, ctx: ToolContext): Promise<string> { ... }
}
```

#### Rust

```rust
use edgecrab_sdk::{Tool, ToolHandler, ToolContext, ToolSchema, ToolError};

// Macro style
#[edgecrab::tool(name = "get_weather", description = "Get weather for a city")]
async fn get_weather(city: String, units: Option<String>) -> Result<String, ToolError> {
    Ok(serde_json::json!({
        "city": city,
        "temp": 22,
        "units": units.unwrap_or("celsius".into())
    }).to_string())
}

// Trait style (advanced)
struct DatabaseTool { db: Arc<Database> }

#[async_trait]
impl ToolHandler for DatabaseTool {
    fn name(&self) -> &'static str { "query_db" }
    fn toolset(&self) -> &'static str { "database" }
    fn schema(&self) -> ToolSchema { ... }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> { ... }
}
```

### 2.4 ConversationResult

Returned by `agent.run()` and `agent.run_conversation()`.

```
+------------------------------------------------------+
|              ConversationResult                       |
+------------------------------------------------------+
| response: str            Final agent response         |
| messages: [Message]      Full conversation trace      |
| session_id: str          Session identifier           |
| api_calls: int           Number of LLM API calls     |
| interrupted: bool        Was agent interrupted?       |
| budget_exhausted: bool   Hit iteration limit?         |
| model: str               Model used                  |
| usage: Usage             Token usage breakdown        |
| cost: Cost               USD cost breakdown           |
| tool_errors: [Error]     Any tool execution errors    |
+------------------------------------------------------+

+------------------------------------------------------+
|                    Usage                              |
+------------------------------------------------------+
| input_tokens: int                                    |
| output_tokens: int                                   |
| cache_read_tokens: int                               |
| cache_write_tokens: int                              |
| reasoning_tokens: int                                |
| total_tokens: int                                    |
+------------------------------------------------------+

+------------------------------------------------------+
|                    Cost                               |
+------------------------------------------------------+
| input_cost: float        Input token cost (USD)       |
| output_cost: float       Output token cost (USD)      |
| cache_read_cost: float   Cache read cost (USD)        |
| cache_write_cost: float  Cache write cost (USD)       |
| total_cost: float        Total cost (USD)             |
+------------------------------------------------------+
```

### 2.5 StreamEvent

```
+------------------------------------------------------+
|                 StreamEvent                           |
+------------------------------------------------------+
| Token(text: str)          Text token                  |
| Reasoning(text: str)      Reasoning/thinking token    |
| ToolExec(name, args_json) Tool execution started      |
| ToolResult(name, result)  Tool execution completed    |
| Done                      Stream completed            |
| Error(message: str)       Error occurred              |
+------------------------------------------------------+
```

### 2.6 Config

```python
class Config:
    """EdgeCrab configuration. Loaded from ~/.edgecrab/config.yaml by default."""

    @staticmethod
    def load() -> "Config": ...

    @staticmethod
    def load_from(path: str | Path) -> "Config": ...

    @staticmethod
    def load_profile(name: str) -> "Config": ...

    @staticmethod
    def default_config() -> "Config": ...

    def save(self) -> None: ...

    # --- Mutators (code evidence: SdkConfig in crates/edgecrab-sdk-core/src/config.rs:66-98) ---
    @property
    def default_model(self) -> str: ...
    def set_default_model(self, model: str) -> None: ...

    @property
    def max_iterations(self) -> int: ...
    def set_max_iterations(self, n: int) -> None: ...

    @property
    def temperature(self) -> float | None: ...
    def set_temperature(self, t: float | None) -> None: ...

    # Full config tree accessors (read-only)
    model: ModelConfig
    agent: AgentConfig
    tools: ToolsConfig
    security: SecurityConfig
    compression: CompressionConfig
    mcp_servers: dict[str, McpServerConfig]
    gateway: GatewayConfig
    # ...
```

```typescript
// Node.js
class Config {
    static load(): Config;
    static loadFrom(path: string): Config;
    static loadProfile(name: string): Config;
    static defaultConfig(): Config;

    save(): void;

    get defaultModel(): string;
    setDefaultModel(model: string): void;

    get maxIterations(): number;
    setMaxIterations(n: number): void;

    get temperature(): number | null;
    setTemperature(t: number | null): void;
}
```

**Code evidence:** `SdkConfig` in `crates/edgecrab-sdk-core/src/config.rs` exposes `save()`, `set_default_model()`, `set_max_iterations()`, `set_temperature()` — all backed by `AppConfig::save()` which writes YAML to `~/.edgecrab/config.yaml`.

### 2.7 Error Types

```
+------------------------------------------------------+
|                 Error Hierarchy                       |
+------------------------------------------------------+
|                                                      |
|  EdgeCrabError (base)                                |
|  +-- AgentError                                      |
|  |   +-- LlmError(message)                          |
|  |   +-- ContextLimitError(used, limit)              |
|  |   +-- BudgetExhaustedError(used, max)             |
|  |   +-- InterruptedError                            |
|  |   +-- ConfigError(message)                        |
|  |   +-- RateLimitedError(provider, retry_after_ms)  |
|  |   +-- CompressionFailedError(message)             |
|  |   +-- ApiRefusalError(message)                    |
|  +-- ToolError                                       |
|  |   +-- ToolNotFoundError(name)                     |
|  |   +-- InvalidArgsError(tool, message)             |
|  |   +-- ToolUnavailableError(tool, reason)          |
|  |   +-- ToolTimeoutError(tool, seconds)             |
|  |   +-- PermissionDeniedError(message)              |
|  |   +-- ExecutionFailedError(tool, message)         |
|  +-- ConnectionError                                 |
|  +-- AuthenticationError                             |
+------------------------------------------------------+
```

---

## 3. Advanced Features

### 3.1 Multi-Agent Delegation

```python
from edgecrab import Agent

# Parent agent automatically delegates complex sub-tasks
agent = Agent(
    model="anthropic/claude-sonnet-4",
    delegation=DelegationConfig(
        enabled=True,
        model="anthropic/claude-haiku-3",  # Cheaper model for sub-tasks
        max_subagents=5,
        max_iterations_per_subagent=30,
    ),
)

result = await agent.run("Refactor the auth module and update all tests")
# Agent may delegate to sub-agents: one for refactoring, one for tests
```

### 3.2 MCP Integration

```python
from edgecrab import Agent, McpServer

# Connect to MCP servers
agent = Agent(
    model="claude-sonnet-4",
    mcp_servers={
        # Stdio-based MCP server
        "my-tools": McpServer(
            command="npx",
            args=["-y", "@my-org/mcp-tools"],
        ),
        # HTTP-based MCP server
        "remote": McpServer(
            url="https://mcp.example.com/rpc",
            bearer_token="sk-...",
        ),
    },
)

# MCP tools are automatically discovered and available
result = await agent.run("Use the my-tools server to process the data")
```

### 3.3 Context Compression

```python
agent = Agent(
    model="claude-sonnet-4",
    compression=CompressionConfig(
        threshold=0.50,         # Compress at 50% context usage
        protect_last_n=20,      # Always keep last 20 messages
    ),
)

# Long conversations are automatically compressed
for i in range(100):
    await agent.chat(f"Process item {i}")
# Context compression happens transparently
```

### 3.4 Smart Model Routing

```python
agent = Agent(
    model="anthropic/claude-sonnet-4",       # Complex queries
    fast_model="anthropic/claude-haiku-3",   # Simple queries
    routing=RoutingConfig(enabled=True),
)

# Short, simple queries automatically route to the fast model
await agent.chat("What time is it?")      # -> haiku (fast, cheap)
await agent.chat("Refactor this module")  # -> sonnet (powerful)
```

### 3.5 Plugin System

```python
from edgecrab import Agent, Plugin

# Load plugins (Rhai scripts, Tool Servers, or Skills)
agent = Agent(
    model="claude-sonnet-4",
    plugins=[
        Plugin.from_path("./plugins/my-plugin/"),       # Tool Server or Rhai script
        Plugin.from_hub("official/code-analysis"),       # Skills hub
    ],
)
```

### 3.6 Approval Workflow

```python
from edgecrab import Agent, ApprovalRequest, ApprovalResponse

async def my_approval_handler(request: ApprovalRequest) -> ApprovalResponse:
    print(f"Agent wants to run: {request.command}")
    print(f"Reasons: {request.reasons}")
    # In production: prompt user, check policy, etc.
    return ApprovalResponse.ONCE  # or SESSION, ALWAYS, DENY

agent = Agent(
    model="claude-sonnet-4",
    approval_handler=my_approval_handler,
)
```

### 3.7 Model Catalog

```python
from edgecrab import ModelCatalog

catalog = ModelCatalog()

# List all providers
providers = catalog.provider_ids()  # ["anthropic", "openai", "google", ...]

# List models for a provider
models = catalog.models_for_provider("anthropic")
# [("claude-sonnet-4", "Claude Sonnet 4"), ("claude-haiku-3", "Claude Haiku 3"), ...]

# Get pricing (USD per million tokens)
pricing = catalog.pricing("anthropic", "claude-sonnet-4")
# {"input": 3.0, "output": 15.0, "cache_read": 0.3, "cache_write": 3.75}

# Get context window
window = catalog.context_window("anthropic", "claude-sonnet-4")  # 200000

# Enumerate all models across all providers
all_models = catalog.flat_catalog()
# [("anthropic", "claude-sonnet-4", "Claude Sonnet 4"), ...]

# Get default model for a provider
default = catalog.default_model_for("anthropic")  # "claude-sonnet-4"
```

```typescript
// Node.js
import { ModelCatalog } from "edgecrab";

const catalog = new ModelCatalog();
const providers = catalog.providerIds();
const pricing = catalog.pricingFor("anthropic", "claude-sonnet-4");
const allModels = catalog.flatCatalog();
const defaultModel = catalog.defaultModelFor("openai");
```

**Code evidence:** `SdkModelCatalog` in `crates/edgecrab-sdk-core/src/types.rs` wraps `ModelCatalog` with all 6 methods: `provider_ids()`, `models_for_provider()`, `flat_catalog()`, `context_window()`, `pricing()`, `default_model_for()`. Note: the SDK method is `pricing()` (delegates internally to `ModelCatalog::pricing_for()`).

### 3.8 Session Management

```python
from edgecrab import Session

# Standalone session access (no agent required)
db = Session.open()  # Default ~/.edgecrab/sessions.db
# or: db = Session.open("/path/to/sessions.db")

# List recent sessions
sessions = db.list_sessions(limit=10)

# Search across all sessions (FTS5)
hits = db.search_sessions("database migration", limit=20)

# Get full session with messages
messages = db.get_messages("ses_abc123")

# Delete a session
db.delete_session("ses_abc123")

# Prune old sessions (returns count deleted)
count = db.prune(older_than_days=90, source="cli")

# Rename a session
db.rename("ses_abc123", "Auth Refactor Sprint")
```

```typescript
// Node.js
import { Session } from "edgecrab";

const db = Session.open();
const sessions = db.listSessions(10);
const hits = db.searchSessions("migration", 20);
const messages = db.getMessages("ses_abc123");
db.deleteSession("ses_abc123");
```

**Code evidence:** `SdkSession` in `crates/edgecrab-sdk-core/src/session.rs` wraps `SessionDb` with `open()`, `list_sessions()`, `search_sessions()`, `get_messages()`, `delete_session()`. Prune/rename delegate to `SessionDb::prune_sessions()` and `SessionDb::update_session_title()` in `crates/edgecrab-state/`.

### 3.9 Compression Control

Context compression is automatic by default (triggered at 50% context window usage), but the SDK also exposes manual control for long-running conversations:

```python
from edgecrab import Agent

agent = Agent("anthropic/claude-sonnet-4")

# After many turns, manually trigger compression
for i in range(50):
    await agent.chat(f"Process item {i}")

await agent.compress()  # Structural pruning + LLM-based summarization
```

```typescript
// Node.js
await agent.compress();
```

**Code evidence:** `Agent::force_compress()` in `crates/edgecrab-core/src/agent.rs:1049` — triggers `compress_with_llm()` on the current session messages. The `SdkAgent` wrapper adds `compress()` which delegates to this method.

### 3.10 Health Check

Programmatic diagnostics for monitoring embedded agents in production:

```python
from edgecrab import HealthCheck

results = HealthCheck.run()
for r in results:
    print(f"{r.name}: {'OK' if r.ok else 'FAIL'} - {r.message}")
```

Returns structured results for:
- Config file existence and validity
- State directory access
- Provider API key presence
- Provider connectivity (ping with latency)
- MCP server configuration

**Code evidence:** `crates/edgecrab-cli/src/doctor.rs` implements these checks. SDK version extracts the logic into `edgecrab-sdk-core` returning structured `HealthResult` instead of CLI-formatted output.

### 3.11 Cost Estimation

Pre-flight cost estimation without making an API call:

```python
from edgecrab import ModelCatalog

catalog = ModelCatalog()
cost = catalog.estimate_cost("anthropic/claude-sonnet-4", input_tokens=1000, output_tokens=500)
# -> {"input_cost": 0.003, "output_cost": 0.0075, "total_cost": 0.0105}
```

**Code evidence:** `SdkModelCatalog::pricing()` provides per-million-token rates. `estimate_cost()` is a convenience wrapper: `rate * tokens / 1_000_000`.

---

## 4. Package Distribution

### 4.1 Python

```
Package: edgecrab
Install: pip install edgecrab
Python:  3.10+
Build:   maturin (PyO3 bindings)
Wheels:  manylinux2014_x86_64, manylinux2014_aarch64,
         macosx_11_0_arm64, macosx_10_12_x86_64,
         win_amd64
```

### 4.2 Node.js

```
Package: edgecrab
Install: npm install edgecrab
Node:    18+
Build:   napi-rs bindings
Targets: linux-x64-gnu, linux-arm64-gnu,
         darwin-arm64, darwin-x64,
         win32-x64-msvc
```

### 4.3 Rust

```
Crate:   edgecrab-sdk
Install: cargo add edgecrab-sdk
MSRV:    1.86.0
```

### 4.4 WASM (Browser / Edge)

```
Package: @edgecrab/wasm
Install: npm install @edgecrab/wasm
Build:   wasm-pack (wasm-bindgen)
Targets: wasm32-unknown-unknown (browser ESM),
         wasm32-wasi (edge runtimes)
Size:    ~3-5MB gzipped (.wasm + JS glue + .d.ts)
Note:    Lite variant — agent core + custom tools only,
         no built-in file/terminal/browser tools
```

**WASM SDK usage:**
```typescript
import init, { Agent, Tool } from "@edgecrab/wasm";
await init();

const agent = new Agent("openai/gpt-4o", { apiKey: "sk-..." });

// Register JS-native tools — full browser/edge API access
agent.addTool(Tool.create({
  name: "fetch_data",
  description: "Fetch data from an API",
  parameters: { url: { type: "string" } },
  handler: async ({ url }) => {
    const res = await fetch(url);
    return JSON.stringify(await res.json());
  },
}));

// Streaming returns an AsyncIterable
for await (const event of agent.stream("Analyze this data")) {
  if (event.type === "token") process.stdout.write(event.text);
}
```

### 4.5 Fallback HTTP Client

For environments where native bindings cannot be installed:

```
Package (Python): edgecrab-client
Package (Node):   @edgecrab/client
Protocol:         HTTP + SSE (OpenAI-compatible)
Server:           edgecrab serve (starts API server)
```

---

## 5. Wire Protocols

### 5.1 Embedded Mode (Primary)

Direct Rust FFI via PyO3/Napi-RS. No serialization for primitives. JSON for complex tool args.

### 5.2 HTTP Client Mode (Fallback)

OpenAI-compatible REST API:

```
POST /v1/chat/completions
Content-Type: application/json
Authorization: Bearer <token>

{
    "model": "anthropic/claude-sonnet-4",
    "messages": [{"role": "user", "content": "..."}],
    "tools": [...],
    "stream": true,
    "temperature": 0.7,
    "max_tokens": 4096
}
```

### 5.3 ACP Mode (VS Code/IDE)

JSON-RPC 2.0 over stdio for IDE integration:

```json
{
    "jsonrpc": "2.0",
    "method": "agent/run",
    "params": { "prompt": "..." },
    "id": 1
}
```

---

## Brutal Honest Assessment

### Strengths of This Spec
- Complete type definitions across all three languages with real parity
- Layered API (simple → full) accommodates beginners and power users
- Security integration is deep, not bolted on
- Session/memory persistence as first-class citizens, not afterthoughts

### Weaknesses and Risks
- **PyO3 wheel building for manylinux is complex** — need CI matrix for all platforms (see [ADR-003](05-ADR.md))
- **Napi-RS binary size** — the full edgecrab runtime is ~30MB; Node.js devs expect tiny packages
- **Tool schema inference from Python type hints** is fragile for complex types — need thorough testing
- **Fallback HTTP mode creates two code paths** — potential for feature divergence

### Improvements Made After Assessment
- Added explicit fallback HTTP client as a separate package to avoid confusion
- Specified MSRV for Rust crate
- Added `chat_sync()` for Python — not everyone wants async
- Clarified that `on_stream` callback is an alternative to `async for` streaming — users pick one
- Added `config_path` parameter for non-default config locations
