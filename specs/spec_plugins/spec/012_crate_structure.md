# edgecrab-plugins Crate Structure

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [001_adr_architecture], [004_plugin_types], [007_registry], [011_config_schema]

---

## 1. Overview

The `edgecrab-plugins` crate is a new workspace member.

```
edgecrab/
  crates/
    edgecrab-types/        (no change)
    edgecrab-security/     (no change)
    edgecrab-state/        (no change)
    edgecrab-tools/        (small additions: plugin_manage tool)
    edgecrab-core/         (small additions: PluginRegistry trait field in AgentBuilder)
    edgecrab-cli/          (small additions: /plugins command handlers)
    edgecrab-plugins/      ← NEW
      Cargo.toml
      src/
        lib.rs
        config.rs
        error.rs
        manifest.rs
        registry.rs
        db.rs
        hub.rs
        host_api.rs
        audit.rs
        kinds/
          mod.rs
          skill.rs
          tool_server.rs
          script.rs
        security/
          mod.rs
          scanner.rs
          patterns.rs
```

---

## 2. Cargo.toml

```toml
[package]
name    = "edgecrab-plugins"
version = "0.1.0"
edition = "2024"
rust-version = "1.86.0"
license = "Apache-2.0"
description = "Plugin system for the EdgeCrab agent"

[dependencies]
# Workspace deps (from root Cargo.toml)
edgecrab-types    = { path = "../edgecrab-types" }
edgecrab-security = { path = "../edgecrab-security" }

tokio         = { workspace = true, features = ["full"] }
serde         = { workspace = true, features = ["derive"] }
serde_json    = { workspace = true }
async-trait   = { workspace = true }
tracing       = { workspace = true }
thiserror     = { workspace = true }
anyhow        = { workspace = true }

# Manifest parsing
toml          = "0.8"

# Subprocess management
tokio-pipe    = "0.2"       # async stdin/stdout wrappers

# SQLite (re-use same version as edgecrab-state)
rusqlite      = { version = "0.31", features = ["bundled", "column_decltype"] }
r2d2          = "0.8"
r2d2_sqlite   = "0.24"

# Scripting engine
rhai          = { version = "1.19", features = ["sync", "serde", "no_closure"] }

# Checksums
sha2          = "0.10"
hex           = "0.4"

# HTTP (for hub client)
reqwest       = { version = "0.12", default-features = false,
                  features = ["json", "rustls-tls"] }

# Async concurrency helpers
dashmap       = "6"         # concurrent HashMap for pending calls
tokio-util    = "0.7"      # framed codec for NDJSON

# Dirs
dirs          = "5"

# Archive extraction (future: .zip plugin archives)
# zip          = "2"    # uncomment when archive install is implemented

[dev-dependencies]
tempfile      = "3"
tokio-test    = "0.4"
```

Add to root `Cargo.toml`:

```toml
# Cargo.toml (workspace root)
[workspace]
members = [
    "crates/edgecrab-types",
    "crates/edgecrab-security",
    "crates/edgecrab-state",
    "crates/edgecrab-cron",
    "crates/edgecrab-lsp",
    "crates/edgecrab-tools",
    "crates/edgecrab-core",
    "crates/edgecrab-cli",
    "crates/edgecrab-gateway",
    "crates/edgecrab-acp",
    "crates/edgecrab-migrate",
    "crates/edgecrab-plugins",     # ← ADD THIS
]
```

---

## 3. Crate Dependency Graph

```
edgecrab-types     (no deps — imported by all)
       │
edgecrab-security  (types only)
       │
edgecrab-plugins   (types + security)
       │
edgecrab-tools     (types + security + plugins [for plugin_manage tool])
       │
edgecrab-core      (tools + state + security + types + plugins)
       │
edgecrab-cli, edgecrab-gateway, edgecrab-acp
```

`edgecrab-plugins` does NOT depend on `edgecrab-tools` or `edgecrab-core`.
This avoids circular dependencies — `edgecrab-core` can `impl PluginRegistry` for the concrete type.

---

## 4. Module-by-Module Specification

### 4.1 `lib.rs`

Public re-exports; the API surface of the crate.

```rust
// edgecrab-plugins/src/lib.rs

pub mod config;
pub mod error;
pub mod manifest;
pub mod registry;
pub mod hub;
pub mod kinds;
pub(crate) mod db;
pub(crate) mod host_api;
pub(crate) mod audit;
pub(crate) mod security;

// Re-export the main types consumers need
pub use config::{PluginsConfig, PluginsHubConfig, PluginsSecurityConfig};
pub use error::PluginError;
pub use manifest::{PluginManifest, PluginKind, TrustLevel};
pub use registry::{PluginRegistry, DefaultPluginRegistry, PluginInfo, PluginEvent};
pub use hub::HubClient;
```

### 4.2 `config.rs`

Config structs as defined in [011_config_schema].

```rust
// edgecrab-plugins/src/config.rs
// All struct definitions from spec 011
// No logic — pure data with serde Deserialize/Serialize
```

### 4.3 `error.rs`

All plugin error variants in one enum.

```rust
// edgecrab-plugins/src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found: {name}")]
    NotFound { name: String },

    #[error("Plugin {name} is not running (state: {state:?})")]
    NotRunning { name: String, state: PluginState },

    #[error("Tool name conflict: plugin {plugin} wants to register '{tool}' already owned by {owner}")]
    ToolNameConflict { plugin: String, tool: String, owner: String },

    #[error("Capability not granted: {capability}")]
    CapabilityNotGranted { capability: String },

    #[error("Security scan blocked install: {verdict:?}")]
    SecurityBlocked { verdict: SecurityVerdict },

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Subprocess startup timeout after {secs}s")]
    StartupTimeout { secs: u64 },

    #[error("Tool call timeout after {secs}s")]
    CallTimeout { secs: u64 },

    #[error("Subprocess exited with code {code:?}")]
    ProcessExited { code: Option<i32> },

    #[error("JSON-RPC error {code}: {message}")]
    RpcError { code: i64, message: String },

    #[error("Manifest parse error: {0}")]
    ManifestParse(#[from] toml::de::Error),

    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Hub error: {0}")]
    Hub(String),

    #[error("Rhai error: {0}")]
    Rhai(String),

    #[error("Reentrancy limit exceeded")]
    ReentrancyLimit,

    #[error("Rate limit exceeded for {method}")]
    RateLimitExceeded { method: String },
}
```

### 4.4 `manifest.rs`

TOML manifest deserialization.

```rust
// edgecrab-plugins/src/manifest.rs

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PluginManifest {
    pub plugin:       PluginSection,
    pub exec:         Option<ExecSection>,
    pub script:       Option<ScriptSection>,
    pub tools:        Vec<ToolDeclaration>,
    pub capabilities: CapabilitiesSection,
    pub trust:        TrustSection,
    pub integrity:    Option<IntegritySection>,
}

impl PluginManifest {
    /// Parse from a plugin.toml string.
    pub fn from_toml(s: &str) -> Result<Self, PluginError>;

    /// Load from a directory (reads plugin.toml inside).
    pub fn from_dir(dir: &Path) -> Result<Self, PluginError>;

    /// Validate all fields per the rules in spec 003.
    pub fn validate(&self) -> Result<(), PluginError>;
}

// ... all section structs as defined in spec 003_manifest ...
```

### 4.5 `registry.rs`

`PluginRegistry` trait + `DefaultPluginRegistry` as defined in [007_registry].

```rust
// edgecrab-plugins/src/registry.rs
// Full implementation per spec 007
```

Key items:
- `PluginRegistry` trait (public)
- `DefaultPluginRegistry` struct (public, created via `DefaultPluginRegistry::new(config, db, host_api)`)
- `PluginInfo` struct (public)
- `PluginEvent` enum (public)
- `RuntimeToolEntry` struct (private)
- `InstallOptions` struct (public)

### 4.6 `db.rs`

SQLite persistence for plugin state.

```rust
// edgecrab-plugins/src/db.rs

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS plugins (
    name             TEXT PRIMARY KEY,
    version          TEXT NOT NULL,
    kind             TEXT NOT NULL,
    state            TEXT NOT NULL DEFAULT 'Installed',
    trust_level      TEXT NOT NULL DEFAULT 'Unverified',
    install_dir      TEXT NOT NULL,
    source           TEXT,
    manifest_toml    TEXT NOT NULL,
    checksum         TEXT,
    installed_at     TEXT NOT NULL DEFAULT (datetime('now')),
    restart_count    INTEGER NOT NULL DEFAULT 0,
    fail_reason      TEXT
);

CREATE INDEX IF NOT EXISTS plugins_state_idx ON plugins(state);
"#;

pub struct PluginDb {
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
}

impl PluginDb {
    pub fn open(home: &Path) -> Result<Self, PluginError> {
        let path = home.join(".edgecrab/plugins/registry.db");
        std::fs::create_dir_all(path.parent().unwrap())?;
        let manager = r2d2_sqlite::SqliteConnectionManager::file(&path);
        let pool = r2d2::Pool::new(manager)?;
        pool.get()?.execute_batch(SCHEMA)?;
        Ok(Self { pool })
    }
    // ... insert, get_all, set_state, remove, get_checksum, set_restart_count
}
```

### 4.7 `hub.rs`

Hub discovery client as defined in [009_discovery_hub].

```rust
// edgecrab-plugins/src/hub.rs
pub struct HubClient { ... }
pub struct HubIndex { ... }
pub struct PluginSearchResult { ... }
```

### 4.8 `host_api.rs`

HostApiRouter as defined in [008_host_api].

```rust
// edgecrab-plugins/src/host_api.rs
pub struct HostApiRouter { ... }
pub struct HostApiError { ... }
```

### 4.9 `audit.rs`

Append-only audit log as defined in [006_security].

```rust
// edgecrab-plugins/src/audit.rs

const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024; // 10 MiB

pub struct AuditLog {
    path: PathBuf,
    file: Mutex<std::fs::File>,
}

impl AuditLog {
    pub fn open(home: &Path) -> Result<Self, PluginError>;
    pub async fn record_install(&self, name: &str, version: &str, verdict: &str, findings: usize);
    pub async fn record_lifecycle(&self, name: &str, event: &str);
    pub async fn record_host_call(&self, plugin_name: &str, method: &str, ok: bool);
    pub async fn tail(&self, n: usize) -> Vec<AuditEntry>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditEntry {
    pub ts:          chrono::DateTime<chrono::Utc>,
    pub event_type:  String,
    pub plugin_name: String,
    pub details:     serde_json::Value,
}
```

### 4.10 `security/mod.rs` + `security/scanner.rs` + `security/patterns.rs`

Extends `edgecrab-security` with plugin-specific scanning.

```rust
// edgecrab-plugins/src/security/mod.rs
pub use scanner::{PluginSecurityScanner, ScanReport};
pub use patterns::PLUGIN_PATTERNS;

// edgecrab-plugins/src/security/scanner.rs
//   Wraps skills_guard.rs patterns + adds plugin-specific patterns
//   ScanResult contains severity-aggregated Verdict + Vec<Finding>

// edgecrab-plugins/src/security/patterns.rs
//   Full pattern catalog from spec 006, plus:
//   - plugin_manifest patterns (detect dangerous capability declarations in plugin.toml)
//   - tool_schema patterns (detect excessive permission requests in tool schemas)
```

### 4.11 `kinds/mod.rs`

The `Plugin` trait + `PluginKind` enum + `PluginState` enum.

```rust
// edgecrab-plugins/src/kinds/mod.rs

#[async_trait::async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self)         -> &str;
    fn kind(&self)         -> PluginKind;
    fn state(&self)        -> PluginState;
    fn manifest(&self)     -> &PluginManifest;

    async fn start(&self)  -> Result<(), PluginError>;
    async fn shutdown(&self) -> Result<(), PluginError>;
    async fn list_tools(&self) -> Vec<ToolSchema>;
    async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, PluginError>;

    // SkillPlugin only — returns None for ToolServer / Script
    fn skill_content(&self) -> Option<SkillContent> { None }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PluginKind {
    Skill,
    ToolServer,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PluginState {
    Approved,
    Installed,
    Starting,
    Running,
    Disabled,
    Failed,
}
```

### 4.12 `kinds/skill.rs`

SKILL.md plugin — no subprocess.

```rust
pub struct SkillPlugin {
    manifest:  PluginManifest,
    content:   String,        // parsed SKILL.md body (after frontmatter)
    state:     AtomicU8,      // Running or Disabled
}
```

### 4.13 `kinds/tool_server.rs`

Subprocess JSON-RPC 2.0 plugin.

```rust
pub struct ToolServerPlugin {
    manifest:  PluginManifest,
    state:     Arc<AtomicU8>,
    process:   Arc<Mutex<Option<tokio::process::Child>>>,
    stdin:     Arc<Mutex<Option<tokio::process::ChildStdin>>>,
    pending:   Arc<DashMap<u64, oneshot::Sender<serde_json::Value>>>,
    next_id:   Arc<AtomicU64>,
    tools:     Arc<RwLock<Vec<ToolSchema>>>,
}
```

### 4.14 `kinds/script.rs`

Rhai in-process script plugin.

```rust
pub struct ScriptPlugin {
    manifest: PluginManifest,
    engine:   Arc<rhai::Engine>,
    ast:      Arc<rhai::AST>,
    state:    AtomicU8,
    tools:    Vec<ToolSchema>,
}
```

---

## 5. Public API Checklist

The following items are the ONLY public exports that `edgecrab-core` and `edgecrab-cli` need:

```
PluginRegistry (trait)
DefaultPluginRegistry (struct) — created at agent startup
PluginsConfig (struct) — embedded in AppConfig
PluginInfo (struct) — for /plugins list display
PluginEvent (enum) — for TUI updates
PluginError (enum) — for error handling
PluginManifest (struct) — for install validation
PluginKind (enum) — for display
TrustLevel (enum) — for display
HubClient (struct) — for /plugins hub commands
```

Everything else is `pub(crate)`.

---

## 6. Test Coverage Targets

| Module | Unit tests | Integration tests |
|---|---|---|
| `manifest.rs` | 20+ (all validation rules) | — |
| `db.rs` | 10+ (CRUD + schema migration) | — |
| `security/scanner.rs` | 30+ (one per pattern) | — |
| `kinds/skill.rs` | 5 | — |
| `kinds/tool_server.rs` | 10 | 3 (with mock subprocess) |
| `kinds/script.rs` | 10 | 3 (real Rhai execution) |
| `registry.rs` | 15 | 5 (dispatch, conflict, concurrent) |
| `hub.rs` | 10 | 2 (mock HTTP server) |
| `host_api.rs` | 10 | — |

Minimum coverage target: **80% line coverage** for `src/` (excluding `kinds/tool_server.rs`
integration code that requires subprocess setup).

---

## 7. File Size Budget

Each source file must stay under 800 lines (Rust). Refactor into submodules when approaching this limit.

| File | Target LOC |
|---|---|
| `lib.rs` | < 50 |
| `config.rs` | < 200 |
| `error.rs` | < 80 |
| `manifest.rs` | < 350 |
| `registry.rs` | < 600 |
| `db.rs` | < 200 |
| `hub.rs` | < 400 |
| `host_api.rs` | < 350 |
| `audit.rs` | < 150 |
| `security/scanner.rs` | < 400 |
| `security/patterns.rs` | < 300 |
| `kinds/mod.rs` | < 100 |
| `kinds/skill.rs` | < 200 |
| `kinds/tool_server.rs` | < 500 |
| `kinds/script.rs` | < 300 |
| **Total** | ~4000 LOC |
