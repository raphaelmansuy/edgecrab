use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;

use edgecrab_core::{Agent, AgentBuilder, AppConfig, ensure_edgecrab_home};
use edgecrab_state::SessionDb;
use edgecrab_tools::ToolRegistry;
use edgecrab_types::Message;
use edgequake_llm::LLMProvider;

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
    Arc::new(ToolRegistry::new())
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
    let _ = config; // currently discovery uses its own disk-read; config ref reserved for future use
    let mut registry = ToolRegistry::new();
    edgecrab_tools::tools::mcp_client::discover_and_register_mcp_tools(&mut registry).await;
    Arc::new(registry)
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
