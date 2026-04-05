# 002.001 — System Architecture

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 001.001 Overview](../001_overview/001_project_summary.md) | [→ 002.002 Crate Graph](002_crate_dependency_graph.md) | [→ 013.001 Libraries](../013_library_selection/001_library_selection.md)

## 1. Layered Architecture

```
+======================================================================+
|                        USER INTERFACES                                |
+======================================================================+
|                                                                      |
|  +------------------+  +-------------------+  +------------------+   |
|  | Interactive TUI  |  | Messaging Gateway |  | ACP Server       |   |
|  | (ratatui +       |  | (tokio async,     |  | (editor plugin)  |   |
|  |  crossterm)      |  |  14 platforms)    |  |                  |   |
|  |                  |  |                   |  | edgecrab-acp/    |   |
|  | edgecrab-cli/    |  | edgecrab-gateway/ |  +------------------+   |
|  +--------+---------+  +--------+----------+          |              |
|           |                     |                     |              |
+-----------+---------------------+---------------------+--------------+
            |                     |                     |
            v                     v                     v
+======================================================================+
|                        AGENT CORE                                     |
+======================================================================+
|                                                                      |
|  +----------------------------------------------------------------+  |
|  |                edgecrab-core (lib crate)                        |  |
|  |                                                                |  |
|  |  +----------------+ +------------------+ +------------------+  |  |
|  |  | Agent struct   | | PromptBuilder    | | ContextCompressor|  |  |
|  |  | (Send + Sync)  | | (system prompt   | | (async compress  |  |  |
|  |  | run_conversa-  | |  pipeline)       | |  via aux LLM)   |  |  |
|  |  | tion() async   | |                  | |                  |  |  |
|  |  +----------------+ +------------------+ +------------------+  |  |
|  |                                                                |  |
|  |  +----------------+ +------------------+ +------------------+  |  |
|  |  | ModelRouter    | | IterationBudget  | | CallbackRegistry |  |  |
|  |  | (provider sel, | | (AtomicU32,      | | (trait objects    |  |  |
|  |  |  fallback)     | |  compare_swap)   | |  for UI events)  |  |  |
|  |  +----------------+ +------------------+ +------------------+  |  |
|  |                                                                |  |
|  |  [crate: edgequake-llm] — LLMProvider trait, 13 native providers     |
|  [module: tool_call_parsers] — 12 model-specific parsers             |  |
|  +----------------------------------------------------------------+  |
|                                                                      |
+==================================+===================================+
                                   |
                                   v
+======================================================================+
|                     TOOL ORCHESTRATION                                 |
+======================================================================+
|                                                                      |
|  +----------------------------------------------------------------+  |
|  |            edgecrab-tools (lib crate)                           |  |
|  |                                                                |  |
|  |  +-------------------+  +--------------------+                 |  |
|  |  | ToolRegistry      |  | ToolsetResolver    |                 |  |
|  |  | (inventory-based  |  | (compile-time +    |                 |  |
|  |  |  + runtime reg.)  |  |  runtime groups)   |                 |  |
|  |  +-------------------+  +--------------------+                 |  |
|  |                                                                |  |
|  |  trait ToolHandler: Send + Sync {                              |  |
|  |      fn name() -> &'static str;                                |  |
|  |      fn schema() -> ToolSchema;                                |  |
|  |      async fn execute(&self, args: Value, ctx: &ToolContext)   |  |
|  |          -> Result<ToolResult>;                                 |  |
|  |  }                                                             |  |
|  +----------------------------------------------------------------+  |
|                                                                      |
+======================================================================+
                                   |
                                   v
+======================================================================+
|                     TOOL IMPLEMENTATIONS                              |
+======================================================================+
|                                                                      |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|  | terminal | | file     | | web      | | browser   | | mcp      |   |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|  | delegate | | code_exec| | memory   | | skills    | | session  |   |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|  | vision   | | tts      | | todo     | | clarify   | | cron     |   |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|  | honcho   | | homeassit| | send_msg | | image_gen | | MoA      |   |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|  |voice_mode| |transcrip.| |rl_train  | |checkpoint | |fuzzy_mat.|   |
|  +----------+ +----------+ +----------+ +-----------+ +----------+   |
|                                                                      |
+======================================================================+
                                   |
                                   v
+======================================================================+
|                     EXECUTION BACKENDS                                 |
+======================================================================+
|                                                                      |
|  trait TerminalBackend: Send + Sync {                                |
|      async fn execute(&self, cmd: &str, ctx: &ExecCtx)              |
|          -> Result<ExecResult>;                                      |
|      async fn create_file(&self, path: &str, content: &str)         |
|          -> Result<()>;                                              |
|      async fn cleanup(&self) -> Result<()>;                          |
|  }                                                                   |
|                                                                      |
|  +-------+ +--------+ +------+ +-------+ +--------+ +-----------+   |
|  | local | | docker | | ssh  | | modal | | daytona| | singularity|  |
|  +-------+ +--------+ +------+ +-------+ +--------+ +-----------+   |
|                                                                      |
+======================================================================+
                                   |
                                   v
+======================================================================+
|                     PERSISTENCE & STATE                                |
+======================================================================+
|                                                                      |
|  +----------------------------------------------------------------+  |
|  |            edgecrab-state (lib crate)                           |  |
|  |                                                                |  |
|  |  +------------------+ +------------------+ +----------------+  |  |
|  |  | SessionDb        | | ConfigManager    | | CronScheduler  |  |  |
|  |  | (rusqlite+FTS5)  | | (serde_yaml)     | | (cron + tokio) |  |  |
|  |  +------------------+ +------------------+ +----------------+  |  |
|  |  +------------------+ +------------------+                     |  |
|  |  | MemoryStore      | | SkillStore       |                     |  |
|  |  | (MEMORY.md/USER) | | (SKILL.md FS)    |                     |  |
|  |  +------------------+ +------------------+                     |  |
|  +----------------------------------------------------------------+  |
|                                                                      |
|  +----------------------------------------------------------------+  |
|  |            edgecrab-security (lib crate)                        |  |
|  |                                                                |  |
|  |  InjectionScanner | SecretRedactor | CommandApprover           |  |
|  |  UrlSafetyChecker | PermissionEnforcer                         |  |
|  +----------------------------------------------------------------+  |
|                                                                      |
+======================================================================+
```

## 2. Component Interaction Flow

### 2.1 CLI User Message Flow

```
User types message in ratatui TUI
        |
        v
+-------------------+
| EdgeCrabCli       |    edgecrab-cli/src/app.rs
| .process_input()  |
+--------+----------+
         |
         | If slash command → dispatch via CommandRouter
         | If message:
         |   1. Parse @ context references (@file, @url, @diff, @staged, @folder, @git)
         |   2. Expand references (inject file content, URL content, diff output)
         |   3. Pass expanded message to Agent
         v
+-------------------+
| Agent             |    edgecrab-core/src/agent.rs
| .run_conversation |
| (msg, ctx).await  |
+--------+----------+
         |
         v
+-------------------+     +--------------------+
| PromptBuilder     |---->| SystemPrompt       |
| .build(ctx).await |     | (skills, memory,   |
|                   |     |  context files)     |
+--------+----------+     +--------------------+
         |
         v
+-------------------+
| CONVERSATION LOOP |     [→ 003.002]
| async stream      |
+--------+----------+
    |         ^
    |         |
    v         |
+--------+ +----------+
| LLM API| | Tool     |
| (edge- | | dispatch |
| quake- | | (tokio   |
| llm)   | |  spawn)  |
+--------+ +----------+
```

### 2.2 Gateway Message Flow

```
Platform event (Telegram WebHook / Discord WS / etc.)
        |
        v
+-------------------+
| PlatformAdapter   |    edgecrab-gateway/src/platforms/<name>.rs
| (impl Adapter)    |
+--------+----------+
         |
         v  (async channel)
+-------------------+
| GatewayRouter     |    edgecrab-gateway/src/router.rs
| .handle(event)    |
+--------+----------+
         |
         | Resolve session, check pairing
         | Get/create Agent (Arc<Agent>)
         v
+-------------------+
| Agent             |    edgecrab-core/src/agent.rs
| .run_conversation |
+--------+----------+
         |
         v  (StreamExt on response channel)
+-------------------+
| DeliveryRouter    |
| .send(chunks)     |
+-------------------+
```

## 3. Three API Modes (via edgequake-llm)

EdgeCrab delegates all LLM communication to `edgequake-llm` which provides a unified
`LLMProvider` trait. Three API mode variants are handled natively:

```
+====================================================================+
|                     API MODE MAPPING                                |
+====================================================================+
|                                                                    |
|  hermes-agent mode        edgequake-llm provider                   |
|  ─────────────────        ──────────────────────                   |
|  chat_completions    →    OpenAIProvider / OpenRouterProvider       |
|                           OllamaProvider / LMStudioProvider         |
|                           OpenAICompatProvider                      |
|                                                                    |
|  anthropic_messages  →    AnthropicProvider (native Messages API)   |
|                                                                    |
|  codex_responses     →    OpenAIProvider (Responses API mode)       |
|                                                                    |
+====================================================================+
```

All providers implement `LLMProvider::chat_with_tools()` which handles:
- Tool definition schemas
- Streaming responses (enabled by default in CLI)
- Multi-turn tool calling
- Provider-specific message format conversion
- Anthropic prompt caching (cache_control markers)
- Auto-recovery from rejected tool_choice (retry without)
- Eager fallback to backup model on rate-limit errors
- Context length detection (models.dev, /v1/props, custom endpoint probing)

## 4. Callback Architecture (Rust-native)

```rust
/// Callbacks are trait objects for UI integration.
/// All callbacks must be Send + Sync for cross-thread safety.
pub trait AgentCallbacks: Send + Sync {
    fn on_tool_progress(&self, tool: &str, preview: &str) {}
    fn on_thinking(&self, text: &str) {}
    fn on_reasoning(&self, text: &str) {}
    fn on_stream_delta(&self, delta: &str) {}
    fn on_step(&self, step: &StepInfo) {}
    fn on_status(&self, msg: &str) {}

    /// Blocking callbacks (run on dedicated thread)
    fn on_clarify(&self, question: &str, choices: &[String])
        -> Option<String> { None }
    fn on_approval(&self, cmd: &str, risk: RiskLevel)
        -> ApprovalDecision { ApprovalDecision::Deny }
}
```

Default no-op implementations allow headless/batch mode with zero callback overhead.

### 4.1 Safe I/O Wrapper (hermes-agent `_SafeWriter` equivalent)

hermes-agent wraps stdout/stderr in `_SafeWriter` to catch `OSError`/`ValueError` from broken
pipes (systemd, Docker headless, thread teardown). EdgeCrab handles this via Rust's `Write` trait:

```rust
/// Transparent I/O wrapper that silently absorbs broken pipe errors.
/// Used in headless/daemon mode where stdout may disconnect.
pub struct SafeWriter<W: Write>(W);

impl<W: Write> Write for SafeWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.0.write(buf) {
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(buf.len()),
            other => other,
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush().or(Ok(()))
    }
}
```

## 5. Compile-Time Feature Gates

```toml
[features]
default = ["cli", "local-backend"]

# UI
cli = ["ratatui", "crossterm", "clap"]

# Gateway platforms (each adds ~50-200KB)
telegram = ["teloxide"]
discord = ["serenity"]
slack = ["dep:reqwest"]
whatsapp = ["dep:reqwest"]
signal = ["dep:reqwest"]
matrix = ["matrix-sdk"]
mattermost = ["dep:reqwest"]
homeassistant = ["dep:reqwest"]
email = ["lettre", "mail-parser"]
sms = ["dep:reqwest"]
dingtalk = ["dep:reqwest"]
webhook = ["dep:reqwest"]
api-server = ["axum"]

# Terminal backends
local-backend = []
docker-backend = ["bollard"]
ssh-backend = ["russh"]
modal-backend = ["dep:reqwest"]
daytona-backend = ["dep:reqwest"]
singularity-backend = []

# Optional features
mcp = ["mcp-rust-sdk"]
acp = ["axum"]
tts = ["dep:reqwest"]        # Edge TTS via HTTP
voice = ["cpal"]
rl = ["dep:reqwest"]         # Atropos integration

# RL environments (feature-gated for binary size)
rl-swe = ["rl"]              # SWE-bench environment
rl-web-research = ["rl"]     # FRAMES web research environment
rl-opd = ["rl"]              # Agentic OPD environment
rl-terminal-test = ["rl"]    # Terminal tool testing environment
rl-tblite = ["rl"]           # TBLite terminal benchmark
rl-terminalbench2 = ["rl"]   # TerminalBench 2 benchmark
rl-yc-bench = ["rl"]         # YC Bench benchmark

# Tool-call parser support
parsers = []                 # Hermes, DeepSeek V3/V3.1, GLM 4.5/4.7, Kimi K2, Llama, LongCat, Mistral, Qwen/Qwen3
honcho = ["dep:reqwest"]

# Build variants
all-platforms = ["telegram", "discord", "slack", "whatsapp", "signal",
                 "matrix", "mattermost", "homeassistant", "email",
                 "sms", "dingtalk", "webhook", "api-server"]
all-backends = ["local-backend", "docker-backend", "ssh-backend",
                "modal-backend", "daytona-backend", "singularity-backend"]
full = ["cli", "all-platforms", "all-backends", "mcp", "acp",
        "tts", "voice", "honcho"]
```

This means a minimal CLI-only build excludes all gateway platform code, reducing binary
size by ~40% and compile time by ~60%.
