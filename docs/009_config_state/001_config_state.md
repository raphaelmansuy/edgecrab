# 009.001 — Config & State

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 003.001 Agent Struct](../003_agent_core/001_agent_struct.md) | [→ 010 Data Models](../010_data_models/001_data_models.md)
> **Source**: `edgecrab-core/src/config.rs`, `edgecrab-state/src/session_db.rs`, `edgecrab-cli/src/profile.rs`

## 1. Config Directory Layout

```
~/.edgecrab/                      ← EDGECRAB_HOME (or env var)
│
├── config.yaml                   ← Main configuration
├── .active_profile               ← Active profile name (e.g. "work")
├── state.db                      ← SQLite WAL session database (FTS5)
├── .env                          ← API keys (loaded by dotenvy)
│
├── memories/
│   ├── MEMORY.md                 ← Agent project/session memory
│   └── USER.md                   ← Long-term user preferences
│
├── skins/
│   ├── default.yaml              ← (override built-in defaults)
│   └── my_theme.yaml             ← User custom skins
│
├── skills/
│   └── my_skill/
│       └── SKILL.md              ← Individual skill documents
│
├── profiles/
│   ├── work/                     ← Isolated profile (own config, db, etc.)
│   │   ├── config.yaml
│   │   ├── .env
│   │   ├── SOUL.md
│   │   ├── memories/
│   │   ├── skills/
│   │   └── state.db
│   └── personal/
│       └── ...
│
├── mcp/
│   └── servers.yaml              ← MCP server configurations
│
├── cache/
│   └── model_context_lengths.json
│
├── cron/
│   └── jobs.json                 ← Persisted cron jobs
│
├── logs/                         ← Session logs
├── sandboxes/                    ← Container sandbox storage
├── hooks/                        ← Gateway event hook scripts
│   └── my_hook/
│       ├── HOOK.yaml
│       └── handler.py
│
├── checkpoints/                  ← Filesystem snapshots for /rollback
└── SOUL.md                       ← Global default personality
```

> **Note**: `ensure_edgecrab_home()` creates subdirectories on first run with `0o700` permissions.

### Managed Mode

When `EDGECRAB_MANAGED=1`, all config-write operations are blocked. Used in enterprise deployments where config is managed centrally.

## 2. Config Resolution Order

```
1. Compiled defaults  (AppConfig::default())
2. ~/.edgecrab/config.yaml  (or profile home config.yaml)
3. Environment variables  (EDGECRAB_* prefixed)
4. CLI arguments  (--model, --toolset, --session, etc.)
```

## 3. AppConfig Schema (config.rs)

```rust
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub model: ModelConfig,
    pub tools: ToolsConfig,
    pub save_trajectories: bool,         // EDGECRAB_SAVE_TRAJECTORIES
    pub skip_context_files: bool,        // EDGECRAB_SKIP_CONTEXT_FILES
    pub skip_memory: bool,               // EDGECRAB_SKIP_MEMORY
    pub gateway: GatewayConfig,
    pub mcp_servers: HashMap<String, McpServerConfig>,
    pub memory: MemoryConfig,
    pub skills: SkillsConfig,
    pub security: SecurityConfig,
    pub terminal: TerminalConfig,
    pub delegation: DelegationConfig,
    pub compression: CompressionConfig,
    pub display: DisplayConfig,
    pub privacy: PrivacyConfig,
    pub browser: BrowserConfig,
    pub checkpoints: CheckpointsConfig,
    pub timezone: Option<String>,
    pub tts: TtsConfig,
    pub stt: SttConfig,
    pub voice: VoiceConfig,
    pub honcho: HonchoConfig,
    pub auxiliary: AuxiliaryConfig,
    pub reasoning_effort: Option<String>,
}
```

### Key Sub-configs

```rust
pub struct ModelConfig {
    pub default: String,           // "anthropic/claude-opus-4.6"
    pub fallback: Option<FallbackConfig>,
    pub base_url: Option<String>,
    pub api_key_env: String,       // default "OPENROUTER_API_KEY"
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub streaming: bool,           // default true
    pub max_iterations: u32,       // default 90
    pub prompt_caching: bool,      // default true (Anthropic)
}

pub struct ToolsConfig {
    pub enabled_toolsets: Option<Vec<String>>,
    pub disabled_toolsets: Option<Vec<String>>,
    pub parallel_execution: bool,  // default true
    pub max_parallel_workers: usize, // default 8
}

pub struct SecurityConfig {
    pub approval_required: Vec<String>,  // tool names requiring approval
    pub blocked_commands: Vec<String>,   // shell patterns → always deny
    pub path_restrictions: Vec<PathBuf>, // allowed paths (empty = unrestricted)
    pub injection_scanning: bool,        // default true
    pub url_safety: bool,                // default true (SSRF guard)
}

pub struct DelegationConfig {
    pub enabled: bool,             // default true
    pub model: Option<String>,
    pub provider: Option<String>,
    pub max_subagents: u32,        // default 3
    pub max_iterations: u32,       // default 50
}

pub struct CompressionConfig {
    pub enabled: bool,             // default true
    pub threshold: f32,            // default 0.50
    pub protect_last_n: usize,     // default 20
    pub summary_model: Option<String>,
}

pub struct DisplayConfig {
    pub skin: String,              // default "default" (skin engine)
    pub personality: String,       // personality addon (default "none")
    pub show_cost: bool,           // $ in status bar
    pub show_reasoning: bool,      // show thinking blocks
    pub streaming: bool,
    pub busy_input_mode: String,   // "interrupt" | "queue"
    pub bell_on_complete: bool,
}

pub struct CheckpointsConfig {
    pub enabled: bool,             // default true
    pub max_snapshots: u32,        // default 50 per dir
}

/// One auxiliary model per task type — cheaper models for side tasks.
pub struct AuxiliaryConfig {
    pub vision: AuxEndpoint,
    pub web_extract: AuxEndpoint,
    pub compression: AuxEndpoint,
    pub session_search: AuxEndpoint,
    pub approval: AuxEndpoint,
    pub flush_memories: AuxEndpoint,
}
```

## 4. Environment Variable Overrides

| Env Var | Config Key | Default |
|---------|------------|---------|
| `EDGECRAB_HOME` | (base dir) | `~/.edgecrab` |
| `EDGECRAB_SAVE_TRAJECTORIES` | `save_trajectories` | false |
| `EDGECRAB_SKIP_CONTEXT_FILES` | `skip_context_files` | false |
| `EDGECRAB_SKIP_MEMORY` | `skip_memory` | false |
| `EDGECRAB_MANAGED` | (managed mode) | false |
| `API_SERVER_ENABLED` | `gateway.api.enabled` | false |
| `API_SERVER_PORT` | `gateway.api.port` | 8642 |
| `API_SERVER_HOST` | `gateway.api.host` | 127.0.0.1 |
| `API_SERVER_KEY` | `gateway.api.key_env` | — |
| `API_SERVER_CORS_ORIGINS` | `gateway.api.cors_origins` | `""` |

## 5. SQLite Session Store (edgecrab-state)

```rust
// edgecrab-state/src/session_db.rs

pub struct SessionDb {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

// Key types re-exported from session_db:
pub struct SessionRecord { ... }
pub struct SessionSummary { ... }
pub struct SessionRichSummary { ... }
pub struct SearchResult { ... }
pub struct SessionStats { ... }
pub struct InsightsReport { ... }
pub struct DailyActivity { ... }
pub struct ModelBreakdown { ... }
pub struct PlatformBreakdown { ... }
pub struct ToolUsage { ... }
pub struct InsightsOverview { ... }
pub struct SessionExport { ... }
```

**Storage**: WAL mode, bundled SQLite (rusqlite `bundled` feature), FTS5 for full-text search.

### Schema (logical)

```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    title TEXT,
    platform TEXT,
    model TEXT,
    created_at INTEGER,
    updated_at INTEGER,
    message_count INTEGER,
    input_tokens INTEGER,
    output_tokens INTEGER,
    estimated_cost REAL
);

CREATE VIRTUAL TABLE sessions_fts USING fts5(
    id UNINDEXED,
    title,
    content,
    content=sessions
);
```

## 6. MCP Server Configuration

```yaml
# config.yaml
mcp_servers:
  filesystem:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"]
    env: {}
    enabled: true

  github:
    command: "docker"
    args: ["run", "-i", "mcp/github"]
    env:
      GITHUB_PERSONAL_ACCESS_TOKEN: "${GITHUB_TOKEN}"
    http_url: null     # or HTTP URL for remote MCP servers
    bearer_token_env: null
    enabled: true
```

## 7. Cron Jobs (cron/jobs.json)

```json
[
  {
    "id": "abc123",
    "name": "daily_summary",
    "expression": "0 9 * * *",
    "task": "Generate a daily productivity summary",
    "platform": "telegram",
    "chat_id": "12345678",
    "model": null,
    "enabled": true,
    "last_run": null
  }
]
```

Managed by `edgecrab-cron` crate (tokio-cron-scheduler). Jobs are persisted to `cron/jobs.json` and reloaded on startup.


## 8. Config Loading

```rust
impl AppConfig {
    pub fn load() -> Result<Self> {
        let home = edgecrab_home();
        let path = home.join("config.yaml");
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(serde_yaml::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    /// Merge CLI args over config file
    pub fn merge_cli_args(&mut self, args: &CliArgs) {
        if let Some(model) = &args.model { self.model.default = model.clone(); }
        if let Some(ts) = &args.toolset { self.tools.enabled_toolsets = Some(vec![ts.clone()]); }
        // ...
    }
}

pub fn edgecrab_home() -> PathBuf {
    std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap().join(".edgecrab"))
}
```


## 9. Hermes Constants → EdgeCrab Constants

```rust
// edgecrab-types/src/constants.rs

pub const DEFAULT_MODEL: &str = "anthropic/claude-opus-4.6";
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
pub const NOUS_API_BASE_URL: &str = "https://inference-api.nousresearch.com/v1";
pub const AI_GATEWAY_BASE_URL: &str = "https://ai-gateway.vercel.sh/v1";
pub const DEFAULT_MAX_ITERATIONS: u32 = 90;
pub const DEFAULT_TOOL_DELAY_MS: u64 = 1000;
pub const DEFAULT_SESSION_TIMEOUT_MINUTES: u32 = 30;
pub const VALID_REASONING_EFFORTS: &[&str] = &["xhigh", "high", "medium", "low", "minimal"];
pub const CONFIG_SCHEMA_VERSION: u32 = 10;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

## 10. Environment Variable Substitution

Config values support `${ENV_VAR}` and `${ENV_VAR:-default}` syntax:

```rust
/// Expand ${VAR} and ${VAR:-default} in config strings
fn expand_env_vars(input: &str) -> String {
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\$\{(\w+)(?::-([^}]*))?\}").unwrap()
    });
    RE.replace_all(input, |caps: &regex::Captures| {
        let var = &caps[1];
        match std::env::var(var) {
            Ok(val) if !val.is_empty() => val,
            _ => caps.get(2).map_or(String::new(), |m| m.as_str().to_string()),
        }
    }).to_string()
}
```

## 11. Real-Time Config Reload

Config changes are detected via filesystem watcher and applied without restart:

```rust
pub struct ConfigWatcher {
    watcher: notify::RecommendedWatcher,
    config: Arc<RwLock<AppConfig>>,
}

impl ConfigWatcher {
    pub fn start(config: Arc<RwLock<AppConfig>>, path: &Path) -> Result<Self> {
        let config_clone = config.clone();
        let path_clone = path.to_owned();
        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                if event.kind.is_modify() {
                    if let Ok(new) = AppConfig::load_from(&path_clone) {
                        *config_clone.write() = new;
                        tracing::info!("Config reloaded");
                    }
                }
            }
        })?;
        watcher.watch(path, notify::RecursiveMode::NonRecursive)?;
        Ok(Self { watcher, config })
    }
}
```

## 12. Custom Models Registry

`custom_models.yaml` allows users to register custom model endpoints:

```yaml
# ~/.edgecrab/custom_models.yaml
models:
  local-llama:
    base_url: "http://localhost:8080/v1"
    api_key_env: "LOCAL_API_KEY"
    context_length: 32768
    supports_tools: true
    supports_vision: false
    cost_per_1k_input: 0.0
    cost_per_1k_output: 0.0

  my-finetuned:
    base_url: "https://my-endpoint.com/v1"
    api_key_env: "MY_API_KEY"
    context_length: 128000
    supports_tools: true
    supports_streaming: true
```

```rust
#[derive(Deserialize)]
pub struct CustomModelsFile {
    pub models: HashMap<String, CustomModelDef>,
}

#[derive(Deserialize)]
pub struct CustomModelDef {
    pub base_url: String,
    pub api_key_env: String,
    pub context_length: u32,
    pub supports_tools: bool,
    pub supports_vision: Option<bool>,
    pub supports_streaming: Option<bool>,
    pub cost_per_1k_input: Option<f64>,
    pub cost_per_1k_output: Option<f64>,
}
```

## 13. Checkpoint Manager

Checkpoints save full agent state (conversation, config, files) for later restore:

```rust
// edgecrab-state/src/checkpoint.rs

pub struct CheckpointManager {
    checkpoint_dir: PathBuf,  // ~/.edgecrab/checkpoints/
}

pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    pub messages: Vec<Message>,
    pub config_snapshot: AppConfig,
    pub files_snapshot: HashMap<PathBuf, Vec<u8>>,
    pub created_at: DateTime<Utc>,
    pub description: Option<String>,
}

impl CheckpointManager {
    /// Save current state as a checkpoint
    pub fn save(&self, session: &Session, config: &AppConfig) -> Result<String> { ... }
    
    /// Restore agent state from a checkpoint
    pub fn restore(&self, id: &str) -> Result<Checkpoint> { ... }
    
    /// List available checkpoints for a session
    pub fn list(&self, session_id: &str) -> Result<Vec<CheckpointSummary>> { ... }
}
```

## 14. Project-Local Config (.hermes.md / .edgecrab.md)

Per-project configuration file injected into the system prompt:

```rust
/// Search for project-local config: .edgecrab.md > .hermes.md
pub fn find_project_config(cwd: &Path) -> Option<PathBuf> {
    let candidates = [".edgecrab.md", ".hermes.md"];
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        for name in &candidates {
            let p = d.join(name);
            if p.is_file() { return Some(p); }
        }
        dir = d.parent();
    }
    None
}
```
