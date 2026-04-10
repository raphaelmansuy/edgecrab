# Plugin Registry — Design & Implementation

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [001_adr_architecture], [004_plugin_types], [005_lifecycle], [012_crate_structure]

---

## 1. Overview

The `PluginRegistry` is the runtime table that:
1. Stores all installed plugins (state, manifest, handles)
2. Routes tool dispatch to plugin handlers (ToolServer / Script kinds)
3. Feeds prompt injection content to `PromptBuilder` (Skill kind)
4. Manages plugin lifecycle (start, stop, restart, uninstall)

It wraps — but does NOT replace — the compile-time `ToolRegistry` from `edgecrab-tools`.

```
 Tool dispatch pipeline:
 ───────────────────────

 ConversationLoop calls:
     registry.dispatch("some_tool", args, ctx)
           │
           ├── ToolRegistry (compile-time inventory! tools)
           │     ├── exact match? → execute native handler
           │     └── no match → fall through to PluginRegistry
           │
           └── PluginRegistry (runtime plugins)
                 ├── exact match in runtime_tools table? → route to plugin
                 └── no match → error with fuzzy suggestion
```

---

## 2. Traits

### 2.1 PluginRegistry trait (consumer-facing)

```rust
/// The interface that edgecrab-core and edgecrab-cli consume.
/// Implemented by DefaultPluginRegistry in edgecrab-plugins.
/// DIP: core depends on this trait, not the concrete struct.
#[async_trait]
pub trait PluginRegistry: Send + Sync {
    // ── Query ───────────────────────────────────────────────────────

    /// List all installed plugins with their current state.
    fn list_plugins(&self) -> Vec<PluginInfo>;

    /// Get a single plugin's info by name.
    fn get_plugin(&self, name: &str) -> Option<PluginInfo>;

    /// Returns true if a tool name is provided by a plugin (not compile-time).
    fn has_plugin_tool(&self, tool_name: &str) -> bool;

    /// Get all ToolSchema objects from running plugins.
    /// Called by ConversationLoop to build the tools array for the LLM.
    fn plugin_tool_schemas(&self) -> Vec<ToolSchema>;

    /// Prompt content from all enabled SkillPlugins.
    /// Called by PromptBuilder::add_plugin_skills().
    fn skill_prompt_content(&self) -> Vec<SkillContent>;

    // ── Dispatch ────────────────────────────────────────────────────

    /// Dispatch a tool call to the responsible plugin.
    /// Returns Err if tool_name is not a plugin-provided tool.
    async fn dispatch(
        &self,
        tool_name: &str,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, PluginError>;

    // ── Lifecycle ───────────────────────────────────────────────────

    /// Load all installed plugins from the registry DB.
    /// Called once at agent startup.
    async fn load_all(&self) -> Result<(), PluginError>;

    /// Install a plugin from a source (hub URL, GitHub path, local dir).
    async fn install(
        &self,
        source: &str,
        options: InstallOptions,
    ) -> Result<PluginInfo, PluginError>;

    /// Enable a disabled plugin (start subprocess / register tools).
    async fn enable(&self, name: &str) -> Result<(), PluginError>;

    /// Disable a running plugin (remove from dispatch, shut down subprocess).
    async fn disable(&self, name: &str) -> Result<(), PluginError>;

    /// Uninstall a plugin (disable + delete files + remove from DB).
    async fn uninstall(&self, name: &str) -> Result<(), PluginError>;

    /// Upgrade a plugin to a newer version.
    async fn upgrade(&self, name: &str) -> Result<PluginInfo, PluginError>;

    // ── Events ──────────────────────────────────────────────────────

    /// Subscribe to plugin lifecycle events.
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<PluginEvent>;
}
```

### 2.2 Supporting Types

```rust
/// Snapshot of a plugin's metadata + runtime state (cheap to clone).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginInfo {
    pub name:          String,
    pub version:       String,
    pub kind:          PluginKind,
    pub description:   String,
    pub state:         PluginState,
    pub trust_level:   TrustLevel,
    pub tools:         Vec<String>,   // tool names provided
    pub installed_at:  chrono::DateTime<chrono::Utc>,
    pub source:        Option<String>,
    pub fail_reason:   Option<String>,
    pub restart_count: u32,
}

/// Content injected into the system prompt by a SkillPlugin.
#[derive(Debug, Clone)]
pub struct SkillContent {
    pub plugin_name: String,
    pub heading:     String,  // e.g. "## Rust Patterns"
    pub body:        String,  // The SKILL.md content (filtered for this platform)
}

pub struct InstallOptions {
    pub force:       bool,    // allow community caution verdict
    pub auto_enable: bool,    // start plugin immediately after install
    pub no_prompt:   bool,    // suppress user confirmation (for scripted use)
}
```

---

## 3. DefaultPluginRegistry Implementation

### 3.1 Internal Structure

```rust
pub struct DefaultPluginRegistry {
    /// All installed plugins (name → Arc<dyn Plugin>).
    /// RwLock: many concurrent readers (tool dispatch), rare writers (install/uninstall).
    plugins: Arc<RwLock<HashMap<String, Arc<dyn Plugin>>>>,

    /// Runtime dispatch table: tool_name → (plugin_name, ToolSchema).
    /// Separate from `plugins` for O(1) dispatch without scanning all plugins.
    /// Atomic swap on plugin enable/disable to avoid holding write lock during dispatch.
    runtime_tools: Arc<RwLock<HashMap<String, RuntimeToolEntry>>>,

    /// Plugin state persistence.
    db: Arc<PluginDb>,

    /// Host API handler (processes reverse calls from plugin subprocesses).
    host_api: Arc<HostApiRouter>,

    /// Security scanner used for install.
    scanner: Arc<PluginSecurityScanner>,

    /// Lifecycle event broadcast.
    event_tx: tokio::sync::broadcast::Sender<PluginEvent>,
}

struct RuntimeToolEntry {
    plugin_name: String,
    schema:      ToolSchema,
}
```

### 3.2 Dispatch Algorithm

```
dispatch("create_github_issue", args, ctx)
     │
     ├── read-lock runtime_tools
     ├── lookup("create_github_issue") → RuntimeToolEntry { plugin_name: "github-tools", ... }
     ├── release read-lock
     │
     ├── read-lock plugins
     ├── lookup("github-tools") → Arc<dyn Plugin>
     ├── release read-lock
     │
     ├── plugin.state() == Running? → yes → proceed
     │                               no  → Err(PluginError::NotRunning)
     │
     └── plugin.call_tool("create_github_issue", args, ctx).await
```

This is O(1) for both lookups. No scan of all plugins needed.

### 3.3 Tool Table Atomic Update

When enabling a plugin:

```rust
async fn enable_plugin_internal(&self, plugin: Arc<dyn Plugin>) -> Result<(), PluginError> {
    // 1. Start the plugin (subprocess / script compile)
    plugin.start().await?;

    // 2. Discover tools it provides
    let tools = plugin.list_tools().await;

    // 3. Check for collisions with compile-time tools (INV-4)
    for tool in &tools {
        if STATIC_TOOL_NAMES.contains(tool.name.as_str()) {
            return Err(PluginError::ToolNameConflict { ... });
        }
    }

    // 4. Check for collisions with OTHER running plugins
    {
        let existing = self.runtime_tools.read().await;
        for tool in &tools {
            if let Some(entry) = existing.get(&tool.name) {
                if entry.plugin_name != plugin.name() {
                    return Err(PluginError::ToolNameConflict { ... });
                }
            }
        }
    }

    // 5. Atomic swap: build new table with these tools added
    let mut tbl = self.runtime_tools.write().await;
    for tool in tools {
        tbl.insert(tool.name.clone(), RuntimeToolEntry {
            plugin_name: plugin.name().to_string(),
            schema: tool,
        });
    }
    // RwLock released here

    // 6. Update plugin state in DB
    self.db.set_state(plugin.name(), PluginState::Running).await?;

    // 7. Broadcast event
    let _ = self.event_tx.send(PluginEvent::Started { name: plugin.name().to_string() });

    Ok(())
}
```

### 3.4 Graceful Disable

```rust
async fn disable_plugin_internal(&self, name: &str) -> Result<(), PluginError> {
    let plugin = {
        let plugins = self.plugins.read().await;
        plugins.get(name).cloned().ok_or(PluginError::NotFound)?
    };

    // 1. FIRST: remove tools from dispatch table (INV-8 satisfied immediately)
    {
        let mut tbl = self.runtime_tools.write().await;
        let tool_names: Vec<String> = plugin.list_tools().await
            .into_iter().map(|t| t.name).collect();
        for name in &tool_names {
            tbl.remove(name);
        }
    }

    // 2. THEN: shut down the subprocess / free script state
    plugin.shutdown().await?;

    // 3. Update DB
    self.db.set_state(name, PluginState::Disabled).await?;

    // 4. Invalidate skills cache if it was a SkillPlugin
    if plugin.kind() == PluginKind::Skill {
        edgecrab_core::prompt_builder::invalidate_skills_cache();
    }

    Ok(())
}
```

### 3.5 Concurrent Tool Call Safety

Multiple concurrent tool calls to the same ToolServerPlugin are safe:

```
ConversationLoop calls dispatch("tool_a", ...) → ToolServerPlugin::call_tool()
ConversationLoop calls dispatch("tool_b", ...) → ToolServerPlugin::call_tool()
     │                                                │
     │  id=42, method=tools/call, tool_a             │  id=43, method=tools/call, tool_b
     └──── write to plugin stdin ────────────────────┘ (Mutex<ChildStdin>)
                                                     waiting for id=42 AND id=43
   reader task receives id=43 response → signals oneshot for call_b
   reader task receives id=42 response → signals oneshot for call_a
```

The `pending: HashMap<u64, oneshot::Sender<JsonValue>>` plus a monotonic `AtomicU64`
ID counter ensures each call waits for exactly its own response — identical to mcp_client.rs.

---

## 4. PluginDb

```rust
/// Thin wrapper around SQLite for plugin state persistence.
/// Re-uses the same rusqlite connection pool as edgecrab-state.
pub struct PluginDb {
    path: PathBuf,
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
}

impl PluginDb {
    pub fn open(home: &Path) -> Result<Self, PluginError>;
    pub async fn insert(&self, manifest: &PluginManifest) -> Result<(), PluginError>;
    pub async fn set_state(&self, name: &str, state: PluginState) -> Result<(), PluginError>;
    pub async fn get_all(&self) -> Result<Vec<PluginRow>, PluginError>;
    pub async fn remove(&self, name: &str) -> Result<(), PluginError>;
    pub async fn get_checksum(&self, name: &str) -> Result<Option<String>, PluginError>;
    pub async fn set_restart_count(&self, name: &str, count: u32) -> Result<(), PluginError>;
}
```

---

## 5. Integration with edgecrab-core

### 5.1 AgentBuilder

```rust
// In edgecrab-core/src/agent.rs
pub struct AgentBuilder {
    // ... existing fields ...
    plugin_registry: Option<Arc<dyn PluginRegistry>>,
}

impl AgentBuilder {
    pub fn plugin_registry(mut self, r: Arc<dyn PluginRegistry>) -> Self {
        self.plugin_registry = Some(r);
        self
    }
}
```

### 5.2 ConversationLoop dispatch

```rust
// In edgecrab-core/src/conversation.rs
async fn dispatch_tool_call(
    &self,
    tool_name: &str,
    args: Value,
    ctx: &ToolContext,
) -> Result<String, ToolError> {
    // 1. Try compile-time tools first (INV-4: they win)
    if let Some(handler) = self.tool_registry.get(tool_name) {
        return handler.execute(args, ctx).await.map_err(Into::into);
    }

    // 2. Try plugin registry
    if let Some(plugin_reg) = &self.plugin_registry {
        if plugin_reg.has_plugin_tool(tool_name) {
            return plugin_reg.dispatch(tool_name, args, ctx).await
                .map_err(|e| ToolError::Plugin(e.to_string()));
        }
    }

    // 3. Not found → fuzzy suggest
    Err(ToolError::NotFound {
        name: tool_name.into(),
        suggestion: self.fuzzy_suggest(tool_name),
    })
}
```

### 5.3 PromptBuilder integration

```rust
// In edgecrab-core/src/prompt_builder.rs
impl PromptBuilder {
    pub fn add_plugin_skills(
        mut self,
        plugin_registry: &dyn PluginRegistry,
        platform: Platform,
    ) -> Self {
        for content in plugin_registry.skill_prompt_content() {
            // Apply platform filter from SKILL.md frontmatter
            // ... filter logic ...
            self.sections.push(content.body);
        }
        self
    }
}
```

---

## 6. Performance Budget

| Operation | Target Latency | Notes |
|---|---|---|
| `has_plugin_tool()` | < 1 µs | Hash lookup (no lock contention in read path) |
| `dispatch()` (script kind) | < 5 ms | Rhai eval in spawn_blocking |
| `dispatch()` (tool-server, steady state) | < 20 ms | JSON encode + pipe write + read |
| `load_all()` at startup | < 500 ms | Sequential startup, all plugins |
| `enable()` (skill) | < 5 ms | No subprocess spawn needed |
| `enable()` (tool-server) | < 3 s | Subprocess spawn + initialize handshake |
| `install()` full flow | < 10 s | Download + scan + copy + start |
