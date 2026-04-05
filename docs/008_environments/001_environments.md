# 008.001 — Environments

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.001 Architecture](../002_architecture/001_system_architecture.md) | [→ 004.002 Tool Catalogue](../004_tools_system/002_tool_catalogue.md)
> **Source**: `edgecrab-tools/src/tools/terminal.rs`, `edgecrab-tools/src/tools/execute_code.rs` — verified against real implementation

## 1. Terminal Backend Abstraction

The agent executes commands in isolated terminal environments. EdgeCrab supports 6 backends via a unified trait, matching the scope of agent frameworks like Nous Hermes and OpenClaw.

```rust
// edgecrab-tools/src/terminal/mod.rs

#[async_trait]
pub trait TerminalBackend: Send + Sync {
    /// Execute a command and return output
    async fn execute(&self, cmd: &str, timeout: Duration) -> Result<CommandOutput>;

    /// Get current working directory
    async fn cwd(&self) -> Result<PathBuf>;

    /// Change directory
    async fn cd(&self, path: &Path) -> Result<()>;

    /// Check if backend is alive
    async fn is_alive(&self) -> bool;

    /// Cleanup resources
    async fn cleanup(&self) -> Result<()>;

    /// Backend identifier
    fn backend_type(&self) -> BackendType;
}

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub duration: Duration,
}

pub enum BackendType {
    Local,
    Docker,
    Ssh,
    Modal,
    Daytona,
    Singularity,
}
```

## 1.1 Sandbox Directory Convention

All backends share a configurable host-side root for sandbox storage (Docker
workspaces, Singularity overlays/SIF cache, etc.):

```rust
/// Configurable via TERMINAL_SANDBOX_DIR env var.
/// Defaults to {EDGECRAB_HOME}/sandboxes/.
pub fn get_sandbox_dir() -> PathBuf {
    std::env::var("TERMINAL_SANDBOX_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| get_edgecrab_home().join("sandboxes"))
}
```

## 1.2 Sudo Handling

All backends call `_prepare_command()` before execution to transform
sudo-prefixed commands when `SUDO_PASSWORD` is available (heredoc-based
stdin injection). The Rust equivalent is a shared helper in `edgecrab-tools`:

```rust
fn prepare_command(command: &str) -> (String, Option<String>) {
    transform_sudo_command(command) // returns (transformed_cmd, sudo_stdin)
}
```

## 2. Backend Implementations

### Local Backend
```rust
// Uses tokio::process::Command
struct LocalBackend {
    cwd: RwLock<PathBuf>,
    env_passthrough: Vec<String>,
}
```

### Docker Backend
```rust
// Uses bollard crate for Docker API
struct DockerBackend {
    client: bollard::Docker,
    container_id: String,
    image: String,
}

impl DockerBackend {
    async fn create_container(image: &str, config: &DockerConfig) -> Result<Self> {
        let client = bollard::Docker::connect_with_defaults()?;
        let container = client.create_container(/* ... */).await?;
        client.start_container(&container.id, None).await?;
        Ok(Self { client, container_id: container.id, image: image.into() })
    }
}
```

### SSH Backend
```rust
// Uses russh crate
struct SshBackend {
    session: Arc<Mutex<russh::client::Handle<SshHandler>>>,
    host: String,
    user: String,
}
```

### Modal / Daytona / Singularity
```rust
// HTTP API clients wrapping cloud sandbox providers
// Modal uses a built-in _AsyncWorker thread internally — makes it safe for
// both CLI and Atropos use without monkey-patching.
struct ModalBackend { client: reqwest::Client, sandbox_id: String }
struct DaytonaBackend { client: reqwest::Client, workspace_id: String }
struct SingularityBackend { client: reqwest::Client, instance_id: String }
```

### Modal Transport Modes

EdgeCrab's Modal backend now supports two transport variants behind one typed config:

```rust
pub enum ModalTransportMode {
    Auto,
    Direct,
    Managed,
}

pub struct ModalBackendConfig {
    pub mode: ModalTransportMode,
    pub image: String,
    pub token_id: String,
    pub token_secret: String,
    pub managed_gateway_url: Option<String>,
    pub managed_user_token: Option<String>,
    pub cpu: u32,
    pub memory_mb: u32,
    pub disk_mb: u32,
    pub persistent_filesystem: bool,
}
```

- `direct`: uses Modal token ID / secret and the public sandbox REST surface.
- `managed`: uses a gateway-owned sandbox with Bearer auth.
- `auto`: prefers direct credentials when present, otherwise falls back to the managed gateway.

Managed gateway auth resolves from explicit config first, then environment overrides, then `~/.edgecrab/auth.json` Nous provider tokens.

## 2.1 Persistent Shell Mixin

A **file-based IPC protocol** for long-lived bash shells. Each backend can optionally
be `persistent`, keeping a single bash process alive across multiple `execute()` calls
(interactive mode).

```rust
// edgecrab-tools/src/terminal/persistent_shell.rs

/// File-based IPC: temp files for stdout, stderr, exit status, cwd, pid.
/// Session ID scopes temp files: /tmp/edgecrab-persistent-{session_id}-{stdout|stderr|status|cwd|pid}
pub struct PersistentShell {
    session_id: String,        // uuid hex[:12]
    shell_proc: Option<Child>, // tokio::process::Child
    shell_alive: AtomicBool,
    shell_pid: Option<u32>,
    poll_interval_start: Duration, // 10ms initial
    poll_interval_max: Duration,   // 250ms max — reduces I/O for long commands
}

impl PersistentShell {
    /// Spawn bash, create temp files, read PID with 3s deadline
    pub async fn init(&mut self) -> Result<()> { ... }

    /// Execute command via file-based IPC:
    /// 1. Truncate temp files
    /// 2. Write command wrapper (redirect stdout/stderr to temp, write exit code)
    /// 3. Poll temp status file with exponential backoff
    /// 4. Read output + exit code from temp files
    /// 5. Update cwd from temp cwd file
    pub async fn execute_persistent(
        &mut self, command: &str, cwd: &str, timeout: Duration,
    ) -> Result<CommandOutput> { ... }

    /// Falls back to oneshot if stdin_data is needed (sudo) or shell died
    pub async fn execute(
        &mut self, command: &str, cwd: &str, timeout: Duration,
        stdin_data: Option<&str>,
    ) -> Result<CommandOutput> {
        if self.persistent && stdin_data.is_none() {
            self.execute_persistent(command, cwd, timeout).await
        } else {
            self.execute_oneshot(command, cwd, timeout, stdin_data).await
        }
    }

    /// Kill shell children on interrupt (SIGTERM → SIGKILL with 3s grace)
    pub async fn kill_children(&self) { ... }

    /// Cleanup: remove temp files, terminate shell process
    pub async fn cleanup(&mut self) { ... }
}
```

**Key implementation details:**
- `_drain_shell_output()` runs as a background tokio task, consuming stdout to prevent pipe blocking
- If the persistent shell dies, `execute()` auto-restarts it
- Commands with `stdin_data` or `sudo_stdin` always fall back to oneshot mode
- Interrupt support: checks `is_interrupted()` flag during polling loop

## 3. SWE Benchmark Environment

```rust
// edgecrab-environments/src/swe_env.rs

pub struct SweBenchEnv {
    backend: Box<dyn TerminalBackend>,
    repo_path: PathBuf,
    test_cmd: String,
    patch_format: PatchFormat,
    timeout: Duration,
}

impl SweBenchEnv {
    pub async fn setup(&self, instance: &SweInstance) -> Result<()> {
        // 1. Clone repo at specified commit
        // 2. Apply test patch
        // 3. Install dependencies
        // 4. Verify baseline
        todo!()
    }

    pub async fn evaluate(&self, agent_patch: &str) -> Result<EvalResult> {
        // 1. Apply agent's patch
        // 2. Run test suite
        // 3. Parse results
        // 4. Return pass/fail
        todo!()
    }
}
```

## 4. RL Training Environment — Atropos Integration

All RL environments extend `HermesAgentBaseEnv`, which provides Atropos
integration plumbing. hermes-agent has **two-mode operation**:

- **Phase 1** (OpenAI server type): Uses `server.chat_completion()` directly.
  Server handles tool call parsing natively. `DummyManagedServer` provides
  placeholder tokens. Good for SFT data gen, evaluation, and verifier testing.
- **Phase 2** (VLLM server type): Uses `ManagedServer` for exact token IDs +
  logprobs via `/generate`. Client-side tool call parser reconstructs structured
  `tool_calls` from raw output. Full RL training capability.

### 4.1 Base Environment Config

```rust
// edgecrab-environments/src/hermes_base_env.rs

pub struct HermesAgentEnvConfig {
    // --- Toolset configuration (mutually exclusive) ---
    pub enabled_toolsets: Option<Vec<String>>,    // explicit list
    pub disabled_toolsets: Option<Vec<String>>,    // filter on top
    pub distribution: Option<String>,             // from toolset_distributions

    // --- Agent loop ---
    pub max_agent_turns: u32,                     // default: 30
    pub system_prompt: Option<String>,
    pub agent_temperature: f64,                   // default: 1.0

    // --- Terminal backend ---
    pub terminal_backend: String,                 // local|docker|modal|daytona|ssh|singularity
    pub terminal_timeout: u32,                    // per-command timeout (default: 120s)
    pub terminal_lifetime: u32,                   // sandbox inactivity lifetime (default: 3600s)

    // --- Dataset ---
    pub dataset_name: Option<String>,             // HuggingFace dataset
    pub dataset_split: String,                    // default: "train"
    pub prompt_field: String,                     // default: "prompt"

    // --- Thread pool ---
    pub tool_pool_size: usize,                    // default: 128

    // --- Phase 2: Tool call parsing ---
    pub tool_call_parser: String,                 // default: "hermes"

    // --- Provider-specific passthrough ---
    pub extra_body: Option<serde_json::Value>,    // OpenRouter prefs, transforms, etc.
}
```

### 4.2 Base Environment Trait

```rust
// Subclasses only implement 5 methods:
#[async_trait]
pub trait HermesAgentEnv: Send + Sync {
    /// Load dataset, initialize state
    async fn setup(&mut self) -> Result<()>;
    /// Return the next item from the dataset
    async fn get_next_item(&mut self) -> Result<Item>;
    /// Convert a dataset item into the user message
    fn format_prompt(&self, item: &Item) -> String;
    /// Score the rollout (has full ToolContext access)
    async fn compute_reward(&self, item: &Item, result: &AgentResult, ctx: &ToolContext) -> Result<f64>;
    /// Periodic evaluation
    async fn evaluate(&mut self) -> Result<()>;
}
```

### 4.3 Reward Function

```rust
#[async_trait]
pub trait RewardFunction: Send + Sync {
    async fn compute(&self, trajectory: &[Message], outcome: &str) -> f64;
}
```

## 5. Agent Loop (RL Training)

```rust
// edgecrab-environments/src/agent_loop.rs

/// Record of a tool execution error during the agent loop.
pub struct ToolError {
    pub turn: u32,
    pub tool_name: String,
    pub arguments: String,  // truncated
    pub error: String,
    pub tool_result: String,
}

pub struct AgentResult {
    pub messages: Vec<Message>,
    /// ManagedServer.get_state() if available (Phase 2), None otherwise
    pub managed_state: Option<serde_json::Value>,
    pub turns_used: u32,
    pub finished_naturally: bool,  // model stopped vs hit max_turns
    /// Extracted reasoning content per turn (from multiple provider formats)
    pub reasoning_per_turn: Vec<Option<String>>,
    /// Tool errors encountered during the loop
    pub tool_errors: Vec<ToolError>,
}

pub struct HermesAgentLoop {
    server: Arc<dyn LlmServer>,
    tool_schemas: Vec<ToolDef>,
    valid_tool_names: HashSet<String>,
    max_turns: u32,
    task_id: String,
    temperature: f64,
    max_tokens: Option<u32>,
    /// Extra body parameters for OpenRouter prefs, transforms, etc.
    extra_body: Option<serde_json::Value>,
}

impl HermesAgentLoop {
    pub async fn run(&self, messages: Vec<Message>) -> Result<AgentResult> {
        let mut result = AgentResult::default();
        // Per-loop TodoStore (ephemeral, dies with the loop)
        let todo_store = TodoStore::new();

        for turn in 0..self.max_turns {
            let response = self.call_model(&messages).await?;
            // Extract reasoning from multiple provider formats:
            //   1. message.reasoning_content (some providers)
            //   2. message.reasoning (some providers)
            //   3. message.reasoning_details[].text (OpenRouter style)
            result.reasoning_per_turn.push(extract_reasoning(&response));

            // Dispatch tool calls via thread pool
            for tc in response.tool_calls() {
                match self.run_tool_in_pool(&tc).await {
                    Ok(output) => { /* append tool result to messages */ }
                    Err(e) => {
                        result.tool_errors.push(ToolError {
                            turn, tool_name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                            error: e.to_string(),
                            tool_result: format!("Error: {e}"),
                        });
                    }
                }
            }
            if response.is_final() { break; }
        }
        Ok(result)
    }
}

/// Global tool executor — resized at runtime by HermesAgentBaseEnv::new()
static TOOL_EXECUTOR: Lazy<RwLock<ThreadPool>> = ...;
pub fn resize_tool_pool(max_workers: usize) { ... }
```

## 6. Feature Gates

```toml
[features]
docker-backend = ["bollard"]
ssh-backend = ["russh"]
modal-backend = []       # just reqwest
daytona-backend = []     # just reqwest
singularity-backend = [] # just reqwest
swe-bench = ["docker-backend"]
rl-training = []
rl-swe = ["rl-training", "modal-backend"]
rl-web-research = ["rl-training"]
rl-opd = ["rl-training"]
rl-terminal-test = ["rl-training"]
rl-benchmarks = ["rl-training"]        # tblite, tb2, yc_bench
tool-call-parsers = []                 # 11 model-specific parsers for Phase 2
persistent-shell = []                  # file-based IPC for long-lived bash
```

## 7. Tool Context (RL Thread Bridge)

In RL environments, tools execute in a separate thread to avoid blocking the
async runtime. Mirrors hermes `environments/tool_context.py`:

```rust
// edgecrab-environments/src/tool_context.rs

pub struct ToolContext {
    task_id: String,
    tool_pool: Arc<rayon::ThreadPool>,
}

impl ToolContext {
    pub fn new(task_id: &str) -> Self {
        Self {
            task_id: task_id.to_string(),
            tool_pool: Arc::new(rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap()),
        }
    }

    /// Run a tool in a thread, returning its output string
    pub async fn run_tool(&self, tool_name: &str, args: serde_json::Value) -> Result<String> {
        let name = tool_name.to_string();
        let tid = self.task_id.clone();
        let pool = self.tool_pool.clone();
        tokio::task::spawn_blocking(move || {
            pool.install(|| dispatch_tool(&name, &args, &tid))
        })
        .await?
    }

    /// Terminal shortcut — runs command with timeout
    pub fn terminal(&self, command: &str, timeout: Duration) -> Result<CommandOutput> {
        // ... execute via persistent shell
        todo!()
    }
}
```

## 8. Agentic Online Policy Distillation (OPD) Environment

The most advanced RL environment. First Atropos environment to
populate `distill_token_ids` / `distill_logprobs` fields on `ScoredDataGroup`,
enabling on-policy distillation (OPD) training.

**Reference**: Wang et al., "OpenClaw-RL: Train Any Agent Simply by Talking",
arXiv:2603.10165, March 2026.

**Key idea**: Every time an agent receives a next-state signal (tool result,
error trace, test verdict), that signal contains hindsight information about
how the agent's PREVIOUS response could have been better. This environment:

1. Runs standard agentic rollouts (tool-calling agent loop)
2. Walks the conversation to find (assistant_turn, next_state) pairs
3. Uses an LLM judge to extract "hints" from next-state signals
4. Builds an enhanced prompt (original context + hint)
5. Scores the student's response tokens under the enhanced distribution
   using VLLM's `prompt_logprobs` (via Atropos's `get_logprobs` API)
6. Packages the teacher's top-K predictions as `distill_token_ids` /
   `distill_logprobs` on the `ScoredDataGroup`

**Per-token advantage**: `A_t = teacher_logprob(token_t) - student_logprob(token_t)`
- Positive → teacher approves this token (upweight)
- Negative → teacher disapproves (downweight)

This gives **dense, token-level training signal** from every tool interaction,
instead of just a scalar reward at the end of the trajectory.

**Requires**: VLLM backend (server_type: vllm) + Phase 2 mode (ManagedServer)
for token-level tracking.

```rust
// edgecrab-environments/src/agentic_opd_env.rs

pub struct AgenticOPDConfig {
    pub base: HermesAgentEnvConfig,
    pub judge_model: String,
    pub hint_model: String,
    pub hint_votes: usize,            // majority vote for hint quality
    pub max_hint_attempts: usize,
    pub reward_threshold: f64,
    pub wandb_project: Option<String>,
}

pub struct AgenticOPDEnv {
    config: AgenticOPDConfig,
    backend: Box<dyn TerminalBackend>,
    judge_client: Arc<dyn LlmClient>,
    hint_client: Arc<dyn LlmClient>,
}

impl AgenticOPDEnv {
    /// Collect trajectories with tool-use agent loop
    pub async fn collect_trajectories(&mut self, items: &[Item]) -> Result<Vec<Trajectory>> { ... }

    /// Compute reward via LLM judge + heuristic scoring
    pub async fn compute_reward(&self, item: &Item, result: &str) -> Result<f64> { ... }

    /// Apply OPD pipeline: extract hint → inject into trajectory → re-run
    /// Then score student tokens under teacher distribution via VLLM prompt_logprobs
    pub async fn apply_opd_pipeline(&self, group: &mut ScoredDataGroup) -> Result<()> {
        // 1. Find (assistant_turn, next_state) pairs
        // 2. Extract hindsight hints via LLM judge (majority vote)
        // 3. Build enhanced prompt with hint injection
        // 4. Score tokens via get_logprobs API
        // 5. Populate group.distill_token_ids / group.distill_logprobs
        ...
    }

    /// Extract turn pairs for distillation (user↔assistant pairs)
    fn extract_turn_pairs(messages: &[Message]) -> Vec<TurnPair> { ... }

    /// LLM-based hint extraction with majority voting
    async fn extract_hint(&self, trajectory: &Trajectory, fail_point: usize) -> Result<String> { ... }
}
```

## 9. Web Research Environment

Specialized RL environment for training web research capabilities.  
**Dataset**: FRAMES benchmark (Google, 2024) — multi-hop factual questions.  
HuggingFace: `google/frames-benchmark`. Falls back to built-in sample questions
when HuggingFace is unavailable.

**Reward signals** (4 components):
1. **Answer correctness** — LLM judge, 0.0–1.0
2. **Source diversity** — bonus for using ≥2 distinct domains
3. **Efficiency** — penalizes excessive tool calls
4. **Tool usage** — bonus for actually using web tools

```rust
// edgecrab-environments/src/web_research_env.rs

pub struct WebResearchEnvConfig {
    pub base: HermesAgentEnvConfig,
    pub judge_model: String,
    pub heuristic_weight: f64,     // blend heuristic + LLM judge scores
    pub max_search_steps: usize,
    pub domains_bonus: bool,       // bonus for sourcing multiple domains
}

pub struct WebResearchEnv {
    config: WebResearchEnvConfig,
    backend: Box<dyn TerminalBackend>,
    judge_client: Arc<dyn LlmClient>,
}

impl WebResearchEnv {
    /// LLM judge: compare model answer to expected answer
    async fn llm_judge(&self, question: &str, expected: &str, actual: &str) -> Result<f64> { ... }

    /// Heuristic: token overlap + domain diversity scoring
    fn heuristic_score(expected: &str, actual: &str) -> f64 { ... }

    /// Blend LLM judge + heuristic + domain diversity + efficiency
    pub async fn compute_reward(&self, item: &Item, result: &str) -> Result<f64> {
        let judge = self.llm_judge(&item.question, &item.expected, result).await?;
        let heur = Self::heuristic_score(&item.expected, result);
        let base = judge * (1.0 - self.config.heuristic_weight)
                 + heur * self.config.heuristic_weight;
        // Add domain diversity bonus + efficiency penalty
        Ok(base + domain_bonus + efficiency_penalty)
    }
}
```

## 10. HermesAgentLoop (RL Agent Execution)

The core agent loop used by all RL environments. Identical pattern to
`run_agent.py`: passes `tools=` to the API, checks `response.tool_calls`,
dispatches via `handle_function_call()`. Works with any server type.

```rust
// edgecrab-environments/src/agent_loop.rs
// (See Section 5 for full AgentResult and HermesAgentLoop definitions)

/// Extract reasoning from multiple provider formats:
///   1. message.reasoning_content (Anthropic, some providers)
///   2. message.reasoning (some providers)
///   3. message.reasoning_details[].text (OpenRouter style)
/// Note: <think> block extraction from content is NOT done here —
/// handled by the server in Phase 1, or ManagedServer patch in Phase 2.
fn extract_reasoning_from_message(message: &AssistantMessage) -> Option<String> {
    if let Some(rc) = &message.reasoning_content { return Some(rc.clone()); }
    if let Some(r) = &message.reasoning { return Some(r.clone()); }
    if let Some(details) = &message.reasoning_details {
        for d in details {
            if let Some(text) = &d.text { return Some(text.clone()); }
        }
    }
    None
}
```

## 11. Environment Patches

In hermes-agent, `patches.py` was originally a monkey-patch layer for making
async tools work inside Atropos's event loop. **It is now a no-op** — Modal
async safety is built directly into `ModalEnvironment` via the `_AsyncWorker`
class. The module is kept for backward compatibility.

In Rust this is moot — tokio's `spawn_blocking` + rayon thread pools handle
async↔sync bridging natively. No equivalent module needed.

```rust
// edgecrab-environments/src/patches.rs
// No-op in Rust — async safety is structural, not patched.
// Keep as empty module for code structure parity.
```

## 12. Tool Call Parsers (Phase 2)

hermes-agent has **11 model-specific tool call parsers** for Phase 2 (VLLM
ManagedServer) where the model generates raw text and the client-side parser
reconstructs structured `tool_calls`. Each parser handles a different model
family's output format.

**All 11 parsers need Rust ports** for full Phase 2 RL training support:

| Parser | File | Model family |
|--------|------|-------------|
| hermes | `hermes_parser.rs` | Hermes (NousResearch) — `<tool_call>` XML |
| mistral | `mistral_parser.rs` | Mistral — `[TOOL_CALLS]` prefix |
| llama | `llama_parser.rs` | Llama 3.x — `<\|python_tag\|>` or JSON |
| qwen | `qwen_parser.rs` | Qwen 2.5 — `✿FUNCTION✿` / `✿RESULT✿` |
| qwen3_coder | `qwen3_coder_parser.rs` | Qwen 3 Coder — updated format |
| deepseek_v3 | `deepseek_v3_parser.rs` | DeepSeek V3 — `<｜tool▁call▁begin｜>` |
| deepseek_v3_1 | `deepseek_v3_1_parser.rs` | DeepSeek V3.1 — updated format |
| glm45 | `glm45_parser.rs` | GLM-4.5 (Zhipu) |
| glm47 | `glm47_parser.rs` | GLM-4.7 (Zhipu) |
| kimi_k2 | `kimi_k2_parser.rs` | Kimi K2 (Moonshot) |
| longcat | `longcat_parser.rs` | Longcat |

```rust
// edgecrab-environments/src/tool_call_parsers/mod.rs

pub trait ToolCallParser: Send + Sync {
    /// Parse raw model output text into structured tool calls
    fn parse(&self, raw_output: &str) -> Vec<ParsedToolCall>;
    /// Parser name (matches config string)
    fn name(&self) -> &str;
}

pub fn get_parser(name: &str) -> Result<Box<dyn ToolCallParser>> {
    match name {
        "hermes" => Ok(Box::new(HermesParser)),
        "mistral" => Ok(Box::new(MistralParser)),
        "llama" | "llama3_json" => Ok(Box::new(LlamaParser)),
        "qwen" => Ok(Box::new(QwenParser)),
        "qwen3_coder" => Ok(Box::new(Qwen3CoderParser)),
        "deepseek_v3" => Ok(Box::new(DeepseekV3Parser)),
        "deepseek_v3_1" => Ok(Box::new(DeepseekV3_1Parser)),
        "glm45" => Ok(Box::new(Glm45Parser)),
        "glm47" => Ok(Box::new(Glm47Parser)),
        "kimi_k2" => Ok(Box::new(KimiK2Parser)),
        "longcat" => Ok(Box::new(LongcatParser)),
        _ => Err(anyhow!("Unknown tool call parser: {name}")),
    }
}
```

## 13. Benchmark Environments

hermes-agent ships 3 additional benchmark environments beyond the core RL envs:

| Benchmark | Directory | Description |
|-----------|-----------|-------------|
| **TBLite** | `benchmarks/tblite/` | Lightweight terminal benchmark. Ships with `default.yaml`, `local.yaml`, `local_vllm.yaml` configs + `run_eval.sh`. |
| **TerminalBench 2** | `benchmarks/terminalbench_2/` | Full terminal benchmark suite (TB2). Ships with `default.yaml` + `run_eval.sh`. |
| **YC Bench** | `benchmarks/yc_bench/` | Y Combinator-style coding benchmark. Ships with `default.yaml` + `run_eval.sh`. |

Each benchmark has its own env class (e.g., `TbliteEnv`, `Terminalbench2Env`,
`YcBenchEnv`) extending `HermesAgentBaseEnv` with dataset-specific
`format_prompt()` and `compute_reward()`.

## 14. Terminal Test Environment

A self-contained validation environment with **inline tasks** (no external
dataset needed). Each task asks the model to create a file at a known path
with specific content. The reward verifier cats the file and checks content.

- Training tasks: 3 (greeting.txt, count.txt, answer.txt)
- Eval tasks: 1 (result.txt)
- Toolsets: terminal + file only
- Backend: Modal (default), configurable

Used as a smoke test to validate the entire RL stack end-to-end.
