use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;

use edgecrab_core::{Agent, AgentBuilder, AppConfig, ensure_edgecrab_home};
use edgecrab_plugins::script::engine::ScriptRuntime;
use edgecrab_plugins::tool_server::client::ToolServerClient;
use edgecrab_plugins::{DiscoveredPlugin, PluginKind, PluginStatus, discover_plugins};
use edgecrab_state::SessionDb;
use edgecrab_tools::ToolRegistry;
use edgecrab_tools::registry::{ToolContext, ToolHandler};
use edgecrab_types::{Message, ToolError, ToolSchema};
use edgequake_llm::LLMProvider;
use serde_json::json;

pub struct RuntimeContext {
    pub config_path: PathBuf,
    pub state_db_path: PathBuf,
    pub config: AppConfig,
}

pub fn load_runtime(
    config_override: Option<&str>,
    model_override: Option<&str>,
    toolsets_override: Option<&[String]>,
) -> anyhow::Result<RuntimeContext> {
    let home = if let Some(path) = config_override {
        Path::new(path)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    } else {
        ensure_edgecrab_home().context("failed to initialize edgecrab home")?
    };

    // Load secrets from ~/.edgecrab/.env into the process environment BEFORE
    // config parsing so that tokens saved by `edgecrab gateway configure`
    // (or `edgecrab setup`) are available to adapter constructors and
    // `apply_env_overrides()`. This is safe: set_var only affects the current
    // process and existing env vars are NOT overwritten.
    load_dot_env(&home.join(".env"));

    // ── Bundled skills sync ──────────────────────────────────────────
    // Seed / update bundled skills from the repo's skills/ directory into
    // ~/.edgecrab/skills/. Safe and idempotent — respects user modifications.
    if let Some(report) = edgecrab_tools::tools::skills_sync::sync_on_startup() {
        let summary = report.summary();
        if summary != "No changes" {
            tracing::info!(skills_sync = %summary, "bundled skills synced");
        }
    }

    let config_path = config_override
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("config.yaml"));

    let mut config = if config_path.is_file() {
        AppConfig::load_from(&config_path).context("failed to load config")?
    } else {
        AppConfig::default()
    };

    if let Some(model) = model_override {
        config.model.default_model = model.to_string();
    }
    if let Some(toolsets) = toolsets_override {
        config.tools.enabled_toolsets = Some(toolsets.to_vec());
    }

    Ok(RuntimeContext {
        state_db_path: home.join("state.db"),
        config_path,
        config,
    })
}

pub fn open_state_db(path: &Path) -> anyhow::Result<Arc<SessionDb>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create state db parent directory {}",
                parent.display()
            )
        })?;
    }
    let db = SessionDb::open(path)
        .with_context(|| format!("failed to open state db {}", path.display()))?;
    Ok(Arc::new(db))
}

pub fn build_tool_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    register_plugin_tools(&mut registry, &AppConfig::default());
    Arc::new(registry)
}

/// Build a `ToolRegistry` and populate it with dynamically discovered MCP
/// tools from the configured `mcp_servers` in `config`.
///
/// Each enabled MCP server is connected, its tool list is fetched, and the
/// tools are registered as `mcp_<server>_<tool>` dynamic tool handlers.  Any
/// server that fails to connect is warned about but does not block the others.
///
/// Falls back to a plain registry if MCP configuration is not present.
pub async fn build_tool_registry_with_mcp_discovery(
    config: &edgecrab_core::AppConfig,
) -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    edgecrab_tools::tools::mcp_client::discover_and_register_mcp_tools(&mut registry).await;
    register_plugin_tools(&mut registry, config);
    Arc::new(registry)
}

enum PluginToolBackend {
    ToolServer(Arc<ToolServerClient>),
    Script(Arc<ScriptRuntime>),
}

struct PluginToolProxy {
    tool_name: &'static str,
    plugin_name: String,
    description: String,
    backend: PluginToolBackend,
}

#[async_trait]
impl ToolHandler for PluginToolProxy {
    fn name(&self) -> &'static str {
        self.tool_name
    }

    fn toolset(&self) -> &'static str {
        "plugins"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.tool_name.into(),
            description: self.description.clone(),
            parameters: json!({
                "type": "object",
                "additionalProperties": true
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        if !ctx.config.is_plugin_enabled(&self.plugin_name) {
            return Err(ToolError::Unavailable {
                tool: self.tool_name.into(),
                reason: format!("plugin '{}' is disabled for this session", self.plugin_name),
            });
        }

        match &self.backend {
            PluginToolBackend::ToolServer(client) => client
                .tool_call(self.tool_name, args, ctx)
                .await
                .map(|value| value.to_string())
                .map_err(|error| ToolError::ExecutionFailed {
                    tool: self.tool_name.into(),
                    message: error.to_string(),
                }),
            PluginToolBackend::Script(runtime) => {
                let output =
                    runtime
                        .call_tool(self.tool_name, &args)
                        .map_err(|error| ToolError::ExecutionFailed {
                            tool: self.tool_name.into(),
                            message: error.to_string(),
                        })?;
                if let Some(queue) = &ctx.injected_messages {
                    let emitted = runtime.take_emitted_messages();
                    if !emitted.is_empty() {
                        let mut guard = queue.blocking_lock();
                        for message in emitted {
                            guard.push(Message::assistant(&message));
                        }
                    }
                }
                Ok(output)
            }
        }
    }

    fn emoji(&self) -> &'static str {
        "🔌"
    }
}

fn register_plugin_tools(registry: &mut ToolRegistry, config: &AppConfig) {
    let discovery = match discover_plugins(&config.plugins, edgecrab_types::Platform::Cli) {
        Ok(discovery) => discovery,
        Err(error) => {
            tracing::warn!(?error, "plugin discovery failed");
            return;
        }
    };

    for plugin in discovery.plugins {
        register_plugin(registry, plugin);
    }
}

fn register_plugin(registry: &mut ToolRegistry, plugin: DiscoveredPlugin) {
    if !should_register_runtime_plugin(&plugin) {
        tracing::debug!(
            plugin = %plugin.name,
            status = ?plugin.status,
            "skipping non-available plugin during runtime tool registration"
        );
        return;
    }
    let Some(manifest) = plugin.manifest.as_ref() else {
        return;
    };
    match manifest.plugin.kind {
        PluginKind::ToolServer | PluginKind::Hermes => {
            let Some(exec) = manifest.exec.clone() else {
                return;
            };
            let client = Arc::new(ToolServerClient::new(
                plugin.path.clone(),
                plugin.name.clone(),
                exec,
                manifest.capabilities.clone(),
            ));
            for tool in &manifest.tools {
                registry.register_dynamic(Box::new(PluginToolProxy {
                    tool_name: Box::leak(tool.name.clone().into_boxed_str()),
                    plugin_name: plugin.name.clone(),
                    description: tool.description.clone(),
                    backend: PluginToolBackend::ToolServer(client.clone()),
                }));
            }
        }
        PluginKind::Script => {
            let Some(script) = manifest.script.clone() else {
                return;
            };
            let runtime = match ScriptRuntime::load(
                &plugin.path.join(&script.file),
                script.max_operations,
                script.max_call_depth,
            ) {
                Ok(runtime) => Arc::new(runtime),
                Err(error) => {
                    tracing::warn!(plugin = %plugin.name, ?error, "script plugin failed to load");
                    return;
                }
            };
            for tool in &manifest.tools {
                registry.register_dynamic(Box::new(PluginToolProxy {
                    tool_name: Box::leak(tool.name.clone().into_boxed_str()),
                    plugin_name: plugin.name.clone(),
                    description: tool.description.clone(),
                    backend: PluginToolBackend::Script(runtime.clone()),
                }));
            }
        }
        PluginKind::Skill => {}
    }
}

fn should_register_runtime_plugin(plugin: &DiscoveredPlugin) -> bool {
    plugin.status == PluginStatus::Available
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_registration_requires_available_status() {
        let plugin = DiscoveredPlugin {
            name: "demo".into(),
            version: "1.0.0".into(),
            description: "Demo".into(),
            kind: PluginKind::Hermes,
            status: PluginStatus::SetupNeeded,
            path: PathBuf::from("/tmp/demo"),
            manifest: None,
            skill: None,
            tools: Vec::new(),
            hooks: Vec::new(),
            trust_level: edgecrab_plugins::TrustLevel::Unverified,
            enabled: true,
            source: edgecrab_plugins::SkillSource::User,
            missing_env: vec!["API_KEY".into()],
        };

        assert!(!should_register_runtime_plugin(&plugin));
    }
}

pub fn build_agent(
    runtime: &RuntimeContext,
    provider: Arc<dyn LLMProvider>,
    state_db: Arc<SessionDb>,
    tool_registry: Arc<ToolRegistry>,
    platform: edgecrab_types::Platform,
    quiet: bool,
    session_id: Option<String>,
) -> anyhow::Result<Arc<Agent>> {
    let mut builder = AgentBuilder::from_config(&runtime.config)
        .provider(provider)
        .state_db(state_db)
        .tools(tool_registry)
        .platform(platform)
        .quiet_mode(quiet);

    if let Some(session_id) = session_id {
        builder = builder.session_id(session_id);
    }

    Ok(Arc::new(builder.build()?))
}

pub fn render_markdown_export(messages: &[Message], model: &str, session_id: &str) -> String {
    let mut out = format!(
        "# EdgeCrab Conversation\n\nSession: `{session_id}`\n\nModel: `{model}`\n\n---\n\n"
    );
    for msg in messages {
        out.push_str(&format!(
            "## {}\n\n{}\n\n",
            msg.role.as_str(),
            msg.text_content()
        ));
    }
    out
}

pub fn default_export_path(prefix: &str, session_id: &str, ext: &str) -> PathBuf {
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    PathBuf::from(format!("{prefix}-{session_id}-{ts}.{ext}"))
}

/// Load `KEY=VALUE` pairs from a `.env` file into the process environment.
///
/// - Silently does nothing if the file does not exist.
/// - Lines starting with `#` or blank lines are skipped.
/// - Existing env vars are NOT overwritten (allows shell env to take precedence).
pub fn load_dot_env(path: &Path) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            // Only set if not already present — shell env takes highest priority
            if !key.is_empty() && std::env::var(key).is_err() {
                // SAFETY: single-threaded at this point (called before tokio runtime)
                #[allow(unsafe_code)]
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
}
