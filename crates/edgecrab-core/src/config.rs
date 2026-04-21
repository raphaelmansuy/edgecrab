//! # AppConfig — Layered configuration with sane defaults
//!
//! Config resolution order (later overrides earlier):
//!   1. Compiled defaults (`AppConfig::default()`)
//!   2. `~/.edgecrab/config.yaml` on disk
//!   3. Environment variables (`EDGECRAB_*`)
//!   4. CLI arguments (`--model`, `--toolset`, etc.)
//!
//! WHY layered: Users should be able to set-and-forget in config.yaml
//! but override per-invocation from the terminal or CI.

#![allow(clippy::doc_markdown)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use edgecrab_plugins::config::PluginsConfig;
use serde::{Deserialize, Deserializer, Serialize};

use edgecrab_tools::tools::backends::{
    BackendKind, DaytonaBackendConfig, DockerBackendConfig, ModalBackendConfig,
    SingularityBackendConfig, SshBackendConfig,
};
use edgecrab_types::AgentError;

// ─── Top-level AppConfig ──────────────────────────────────────────────

/// Root configuration — one struct to rule them all.
///
/// Every field has a sane default so `AppConfig::default()` is always
/// a valid starting point.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub model: ModelConfig,
    pub agent: AgentConfig,
    pub logging: LoggingConfig,
    pub tools: ToolsConfig,
    pub lsp: LspConfig,
    pub worktree: bool,
    pub save_trajectories: bool,
    pub skip_context_files: bool,
    pub skip_memory: bool,
    pub gateway: GatewayConfig,
    pub mcp_servers: HashMap<String, McpServerConfig>,
    pub memory: MemoryConfig,
    pub skills: SkillsConfig,
    pub plugins: PluginsConfig,
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
    pub image_generation: ImageGenerationConfig,
    pub voice: VoiceConfig,
    pub honcho: HonchoConfig,
    pub auxiliary: AuxiliaryConfig,
    pub moa: MoaConfig,
    pub reasoning_effort: Option<String>,
    pub context: ContextConfig,
}

/// Configuration for the pluggable context engine.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ContextConfig {
    /// Context engine name. "builtin" (default) uses the built-in compressor.
    /// Set to a plugin name to use a custom context engine.
    pub engine: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AgentConfig {
    pub system_prompt: String,
    pub personalities: HashMap<String, PersonalityPreset>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
        }
    }
}

impl AppConfig {
    /// Load config from disk → env → defaults, in resolution order.
    pub fn load() -> Result<Self, AgentError> {
        let home = edgecrab_home();
        let path = home.join("config.yaml");

        let mut config = if path.exists() {
            let content = std::fs::read_to_string(&path).map_err(AgentError::Io)?;
            Self::parse_compat_yaml(&content, &path)?
        } else {
            Self::default()
        };

        config.apply_env_overrides();
        config.moa = config.moa.sanitized();
        Ok(config)
    }

    /// Load from a specific path (for testing / managed deployments).
    pub fn load_from(path: &Path) -> Result<Self, AgentError> {
        let content = std::fs::read_to_string(path).map_err(AgentError::Io)?;
        let mut config: Self = Self::parse_compat_yaml(&content, path)?;
        config.apply_env_overrides();
        config.moa = config.moa.sanitized();
        Ok(config)
    }

    /// Parse config YAML with compatibility normalization for legacy keys.
    fn parse_compat_yaml(content: &str, path: &Path) -> Result<Self, AgentError> {
        let mut raw: serde_yml::Value = serde_yml::from_str(content)
            .map_err(|e| AgentError::Config(format!("{path:?}: {e}")))?;

        normalize_model_keys(&mut raw);
        normalize_tools_file_keys(&mut raw);

        serde_yml::from_value(raw).map_err(|e| AgentError::Config(format!("{path:?}: {e}")))
    }

    /// Persist the current config to the default config path.
    pub fn save(&self) -> Result<(), AgentError> {
        let home = ensure_edgecrab_home()?;
        self.save_to(&home.join("config.yaml"))
    }

    /// Persist the current config to an explicit path.
    pub fn save_to(&self, path: &Path) -> Result<(), AgentError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(AgentError::Io)?;
        }
        let mut sanitized = self.clone();
        sanitized.moa = sanitized.moa.sanitized();
        let yaml = serde_yml::to_string(&sanitized)
            .map_err(|e| AgentError::Config(format!("failed to serialize config: {e}")))?;
        std::fs::write(path, yaml).map_err(AgentError::Io)
    }

    /// Merge CLI arguments over config (highest priority wins).
    pub fn merge_cli(&mut self, args: &CliOverrides) {
        if let Some(ref model) = args.model {
            self.model.default_model = model.clone();
        }
        if let Some(ref toolset) = args.toolset {
            self.tools.enabled_toolsets = Some(vec![toolset.clone()]);
        }
        if let Some(max) = args.max_iterations {
            self.model.max_iterations = max;
        }
        if let Some(temp) = args.temperature {
            self.model.temperature = Some(temp);
        }
    }

    pub fn is_plugin_enabled(&self, name: &str, platform: Option<&str>) -> bool {
        self.plugins.is_plugin_enabled(name, platform)
    }

    /// Apply `EDGECRAB_*` environment variables.
    ///
    /// WHY env vars: Container / CI deployments often inject secrets
    /// via env rather than files. We only override non-secret config
    /// here — API keys are resolved at runtime by the provider.
    fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("EDGECRAB_MODEL") {
            self.model.default_model = val;
        }
        if let Ok(val) = std::env::var("EDGECRAB_MAX_ITERATIONS") {
            if let Ok(n) = val.parse() {
                self.model.max_iterations = n;
            }
        }
        if let Ok(val) = std::env::var("EDGECRAB_LOG_LEVEL") {
            self.logging.level = val;
        }
        if let Ok(val) = std::env::var("EDGECRAB_TIMEZONE") {
            self.timezone = Some(val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_SAVE_TRAJECTORIES") {
            self.save_trajectories = parse_bool_env(&val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_WORKTREE") {
            self.worktree = parse_bool_env(&val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_SKIP_CONTEXT_FILES") {
            self.skip_context_files = parse_bool_env(&val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_SKIP_MEMORY") {
            self.skip_memory = parse_bool_env(&val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_TOOL_RESULT_SPILL") {
            self.tools.result_spill = parse_bool_env(&val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_TOOL_RESULT_SPILL_THRESHOLD") {
            if let Ok(n) = val.parse() {
                self.tools.result_spill_threshold = n;
            }
        }
        if let Ok(val) = std::env::var("EDGECRAB_TOOL_RESULT_SPILL_PREVIEW_LINES") {
            if let Ok(n) = val.parse() {
                self.tools.result_spill_preview_lines = n;
            }
        }
        if let Ok(val) = std::env::var("EDGECRAB_MAX_WRITE_PAYLOAD_KIB") {
            if let Ok(n) = val.parse() {
                self.tools.file.max_write_payload_kib = Some(n);
            }
        }
        if let Ok(val) = std::env::var("EDGECRAB_PLUGINS_ENABLED") {
            self.plugins.enabled = parse_bool_env(&val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_PLUGINS_AUTO_ENABLE") {
            self.plugins.auto_enable = parse_bool_env(&val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_PLUGINS_CALL_TIMEOUT") {
            if let Ok(seconds) = val.parse() {
                self.plugins.call_timeout_secs = seconds;
            }
        }
        if let Ok(val) = std::env::var("EDGECRAB_PLUGINS_HUB_ENABLED") {
            self.plugins.hub.enabled = parse_bool_env(&val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_PLUGINS_SCAN_ON_LOAD") {
            self.plugins.security.scan_on_load = parse_bool_env(&val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_TERMINAL_BACKEND") {
            self.terminal.backend = val.parse().expect("infallible");
        }
        if let Ok(val) = std::env::var("EDGECRAB_TERMINAL_MODAL_MODE") {
            self.terminal.modal.mode = match val.trim().to_ascii_lowercase().as_str() {
                "auto" => edgecrab_tools::tools::backends::ModalTransportMode::Auto,
                "direct" => edgecrab_tools::tools::backends::ModalTransportMode::Direct,
                "managed" => edgecrab_tools::tools::backends::ModalTransportMode::Managed,
                _ => self.terminal.modal.mode.clone(),
            };
        }
        if let Ok(val) = std::env::var("EDGECRAB_TERMINAL_MODAL_IMAGE") {
            self.terminal.modal.image = val;
        }
        if let Ok(val) = std::env::var("EDGECRAB_TERMINAL_MODAL_GATEWAY_URL") {
            self.terminal.modal.managed_gateway_url = Some(val);
        }
        if let Ok(val) = std::env::var("EDGECRAB_TERMINAL_DAYTONA_IMAGE") {
            self.terminal.daytona.image = val;
        }
        if let Ok(val) = std::env::var("EDGECRAB_TERMINAL_SINGULARITY_IMAGE") {
            self.terminal.singularity.image = val;
        }
        if let Ok(val) = std::env::var("EDGECRAB_GATEWAY_HOST") {
            self.gateway.host = val;
        }
        if let Ok(val) = std::env::var("EDGECRAB_GATEWAY_PORT") {
            if let Ok(port) = val.parse() {
                self.gateway.port = port;
            }
        }
        if let Ok(val) = std::env::var("EDGECRAB_GATEWAY_WEBHOOK") {
            self.gateway.webhook_enabled = parse_bool_env(&val);
        }
        let gateway_enabled_by_env = |gateway: &mut GatewayConfig, platform: &str, ready: bool| {
            if ready && !gateway.platform_disabled(platform) {
                gateway.enable_platform(platform);
            }
        };
        if std::env::var("TELEGRAM_BOT_TOKEN").is_ok()
            && !self.gateway.platform_disabled("telegram")
        {
            self.gateway.telegram.enabled = true;
            self.gateway.enable_platform("telegram");
        }
        if let Ok(val) = std::env::var("TELEGRAM_ALLOWED_USERS") {
            self.gateway.telegram.allowed_users = parse_csv_env(&val);
        }
        if let Ok(val) = std::env::var("TELEGRAM_HOME_CHANNEL") {
            self.gateway.telegram.home_channel = Some(val);
        }
        if std::env::var("DISCORD_BOT_TOKEN").is_ok() && !self.gateway.platform_disabled("discord")
        {
            self.gateway.discord.enabled = true;
            self.gateway.enable_platform("discord");
        }
        if let Ok(val) = std::env::var("DISCORD_ALLOWED_USERS") {
            self.gateway.discord.allowed_users = parse_csv_env(&val);
        }
        if let Ok(val) = std::env::var("DISCORD_HOME_CHANNEL") {
            self.gateway.discord.home_channel = Some(val);
        }
        if std::env::var("SLACK_BOT_TOKEN").is_ok() && !self.gateway.platform_disabled("slack") {
            self.gateway.slack.enabled = true;
            self.gateway.enable_platform("slack");
        }
        gateway_enabled_by_env(
            &mut self.gateway,
            "feishu",
            std::env::var("FEISHU_APP_ID").is_ok() && std::env::var("FEISHU_APP_SECRET").is_ok(),
        );
        gateway_enabled_by_env(
            &mut self.gateway,
            "wecom",
            std::env::var("WECOM_BOT_ID").is_ok() && std::env::var("WECOM_SECRET").is_ok(),
        );
        if let Ok(val) = std::env::var("SLACK_ALLOWED_USERS") {
            self.gateway.slack.allowed_users = parse_csv_env(&val);
        }
        if std::env::var("SIGNAL_HTTP_URL").is_ok()
            && std::env::var("SIGNAL_ACCOUNT").is_ok()
            && !self.gateway.platform_disabled("signal")
        {
            self.gateway.signal.enabled = true;
            self.gateway.enable_platform("signal");
        }
        if let Ok(val) = std::env::var("SIGNAL_HTTP_URL") {
            self.gateway.signal.http_url = Some(val);
        }
        if let Ok(val) = std::env::var("SIGNAL_ACCOUNT") {
            self.gateway.signal.account = Some(val);
        }
        if let Ok(val) = std::env::var("WHATSAPP_ENABLED")
            && parse_bool_env(&val)
            && !self.gateway.platform_disabled("whatsapp")
        {
            self.gateway.whatsapp.enabled = true;
            self.gateway.enable_platform("whatsapp");
        }
        if let Ok(val) = std::env::var("WHATSAPP_MODE") {
            self.gateway.whatsapp.mode = val;
        }
        if let Ok(val) = std::env::var("WHATSAPP_ALLOWED_USERS") {
            self.gateway.whatsapp.allowed_users = parse_csv_env(&val);
        }
        if let Ok(val) = std::env::var("WHATSAPP_BRIDGE_PORT") {
            if let Ok(port) = val.parse() {
                self.gateway.whatsapp.bridge_port = port;
            }
        }
        if let Ok(val) = std::env::var("WHATSAPP_BRIDGE_URL") {
            self.gateway.whatsapp.bridge_url = Some(val);
        }
        if let Ok(val) = std::env::var("WHATSAPP_SESSION_PATH") {
            self.gateway.whatsapp.session_path = Some(PathBuf::from(val));
        }
        if let Ok(val) = std::env::var("WHATSAPP_REPLY_PREFIX") {
            self.gateway.whatsapp.reply_prefix = Some(val);
        }
        gateway_enabled_by_env(
            &mut self.gateway,
            "sms",
            std::env::var("TWILIO_ACCOUNT_SID").is_ok()
                && std::env::var("TWILIO_AUTH_TOKEN").is_ok()
                && std::env::var("TWILIO_PHONE_NUMBER").is_ok(),
        );
        gateway_enabled_by_env(
            &mut self.gateway,
            "matrix",
            std::env::var("MATRIX_HOMESERVER").is_ok()
                && std::env::var("MATRIX_ACCESS_TOKEN").is_ok(),
        );
        gateway_enabled_by_env(
            &mut self.gateway,
            "mattermost",
            std::env::var("MATTERMOST_URL").is_ok() && std::env::var("MATTERMOST_TOKEN").is_ok(),
        );
        gateway_enabled_by_env(
            &mut self.gateway,
            "dingtalk",
            std::env::var("DINGTALK_APP_KEY").is_ok()
                && std::env::var("DINGTALK_APP_SECRET").is_ok(),
        );
        gateway_enabled_by_env(
            &mut self.gateway,
            "homeassistant",
            std::env::var("HA_URL").is_ok() && std::env::var("HA_TOKEN").is_ok(),
        );
        gateway_enabled_by_env(&mut self.gateway, "email", email_env_ready());
        gateway_enabled_by_env(
            &mut self.gateway,
            "api_server",
            std::env::var("API_SERVER_ENABLED")
                .ok()
                .is_some_and(|value| parse_bool_env(&value)),
        );
        // ── TTS env overrides ──
        if let Ok(val) = std::env::var("EDGECRAB_TTS_PROVIDER") {
            self.tts.provider = val;
        }
        if let Ok(val) = std::env::var("EDGECRAB_TTS_VOICE") {
            self.tts.voice = val;
        }
        if let Ok(val) = std::env::var("ELEVENLABS_API_KEY") {
            let _ = val; // Key is resolved at runtime, but presence enables the provider
        }
        // ── Honcho env overrides ──
        if let Ok(val) = std::env::var("HONCHO_API_KEY") {
            let _ = val; // resolved at runtime
            self.honcho.cloud_sync = true;
        }
        if let Ok(val) = std::env::var("EDGECRAB_REASONING_EFFORT") {
            self.reasoning_effort = Some(val);
        }
        if std::env::var("EDGECRAB_MANAGED").as_deref() == Ok("1") {
            // Managed mode — mark config as read-only downstream
            self.security.managed_mode = true;
        }
    }

    /// Returns true when EDGECRAB_MANAGED=1 — config writes are blocked.
    pub fn is_managed(&self) -> bool {
        self.security.managed_mode
    }
}

// ─── Private env-parsing helpers ──────────────────────────────────────

/// Parse a string env var as a boolean.
///
/// WHY extracted: the `matches!(..., "1"|"true"|"yes"|"on")` expression
/// was duplicated 5 times in `apply_env_overrides`.
fn parse_bool_env(val: &str) -> bool {
    matches!(
        val.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Parse a comma-separated env var into a `Vec<String>`.
///
/// WHY extracted: the split→trim→filter→collect chain was duplicated
/// 4 times across allowed_users configuration.
fn parse_csv_env(val: &str) -> Vec<String> {
    val.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn email_env_ready() -> bool {
    let provider = std::env::var("EMAIL_PROVIDER").unwrap_or_default();
    let has_from = std::env::var("EMAIL_FROM")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let has_api_key = std::env::var("EMAIL_API_KEY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let has_domain = std::env::var("EMAIL_DOMAIN")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let has_smtp_host = std::env::var("EMAIL_SMTP_HOST")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let has_smtp_password = std::env::var("EMAIL_SMTP_PASSWORD")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());

    match provider.trim().to_ascii_lowercase().as_str() {
        "sendgrid" => has_from && has_api_key,
        "mailgun" => has_from && has_api_key && has_domain,
        "generic_smtp" | "smtp" => has_from && has_smtp_host && (has_smtp_password || has_api_key),
        _ => false,
    }
}

/// Normalize model config keys for backward compatibility.
///
/// Canonical key is `model.default`. Legacy key is `model.default_model`.
/// If both are present and differ, prefer `default_model` because it was
/// written by `/model` in older builds and reflects the user's last choice.
fn normalize_model_keys(root: &mut serde_yml::Value) {
    let serde_yml::Value::Mapping(root_map) = root else {
        return;
    };

    let model_key = serde_yml::Value::String("model".into());
    let Some(model_value) = root_map.get(&model_key).cloned() else {
        return;
    };

    if let serde_yml::Value::String(model_name) = &model_value {
        let model_name = model_name.trim();
        if model_name.is_empty() {
            return;
        }

        let provider_key = serde_yml::Value::String("provider".into());
        let provider = root_map
            .get(&provider_key)
            .and_then(serde_yml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let normalized_model = if let Some(provider) = provider {
            if has_explicit_provider_prefix(model_name) {
                model_name.to_string()
            } else {
                format!("{provider}/{model_name}")
            }
        } else {
            model_name.to_string()
        };

        let mut normalized = serde_yml::Mapping::new();
        normalized.insert(
            serde_yml::Value::String("default".into()),
            serde_yml::Value::String(normalized_model),
        );
        root_map.insert(model_key, serde_yml::Value::Mapping(normalized));
        return;
    }

    let serde_yml::Value::Mapping(model_map) = root_map
        .get_mut(serde_yml::Value::String("model".into()))
        .expect("model key exists after clone")
    else {
        return;
    };

    let default_key = serde_yml::Value::String("default".into());
    let legacy_key = serde_yml::Value::String("default_model".into());

    let legacy = model_map.get(&legacy_key).cloned();
    if let Some(legacy_val) = legacy {
        let should_promote = match &legacy_val {
            serde_yml::Value::String(s) => !s.trim().is_empty(),
            _ => true,
        };

        if should_promote {
            model_map.insert(default_key, legacy_val);
        }
        model_map.remove(&legacy_key);
    }
}

/// Normalize file-tool policy keys for backward compatibility.
///
/// Canonical key is `tools.file.allowed_roots`.
/// Legacy key is `tools.allowed_paths`.
fn normalize_tools_file_keys(root: &mut serde_yml::Value) {
    let serde_yml::Value::Mapping(root_map) = root else {
        return;
    };

    let tools_key = serde_yml::Value::String("tools".into());
    let Some(serde_yml::Value::Mapping(tools_map)) = root_map.get_mut(&tools_key) else {
        return;
    };

    let legacy_key = serde_yml::Value::String("allowed_paths".into());
    let Some(legacy_allowed_paths) = tools_map.remove(&legacy_key) else {
        return;
    };

    let file_key = serde_yml::Value::String("file".into());
    let file_value = tools_map
        .entry(file_key)
        .or_insert_with(|| serde_yml::Value::Mapping(serde_yml::Mapping::new()));

    let serde_yml::Value::Mapping(file_map) = file_value else {
        return;
    };

    let allowed_roots_key = serde_yml::Value::String("allowed_roots".into());
    file_map.insert(allowed_roots_key, legacy_allowed_paths);
}

fn has_explicit_provider_prefix(model_name: &str) -> bool {
    let Some((provider, _)) = model_name.split_once('/') else {
        return false;
    };

    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "openai"
            | "anthropic"
            | "claude"
            | "gemini"
            | "google"
            | "vertex"
            | "vertexai"
            | "openrouter"
            | "open-router"
            | "xai"
            | "grok"
            | "huggingface"
            | "hf"
            | "hugging-face"
            | "hugging_face"
            | "ollama"
            | "lmstudio"
            | "lm-studio"
            | "lm_studio"
            | "vscode"
            | "vscode-copilot"
            | "copilot"
            | "mock"
            | "mistral"
            | "mistral-ai"
            | "mistralai"
            | "azure"
            | "azure-openai"
            | "azure_openai"
            | "azureopenai"
            | "bedrock"
            | "aws-bedrock"
            | "aws_bedrock"
    )
}

/// CLI arguments that override config — populated by the clap layer.
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    pub model: Option<String>,
    pub toolset: Option<String>,
    pub max_iterations: Option<u32>,
    pub temperature: Option<f32>,
}

// ─── Sub-configs ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ModelConfig {
    /// Default model identifier (e.g. "anthropic/claude-sonnet-4-20250514")
    #[serde(rename = "default", alias = "default_model")]
    pub default_model: String,
    pub fallback: Option<FallbackConfig>,
    pub base_url: Option<String>,
    /// Name of the env var holding the API key (not the key itself!)
    pub api_key_env: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub streaming: bool,
    pub max_iterations: u32,
    pub prompt_caching: bool,
    pub cache_ttl: u32,
    /// Smart routing: auto-route simple messages to a cheaper model.
    pub smart_routing: SmartRoutingYaml,
}

/// YAML-level smart routing configuration.
///
/// WHY a separate struct: `SmartRoutingConfig` in model_router.rs
/// carries runtime thresholds. This struct is the YAML-serializable
/// mirror — deserialized at config load, converted to the runtime
/// struct at conversation start.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SmartRoutingYaml {
    /// Enable smart routing (default: false).
    pub enabled: bool,
    /// Cheap model identifier (e.g. "copilot/gpt-4.1-mini").
    pub cheap_model: String,
    pub cheap_base_url: Option<String>,
    pub cheap_api_key_env: Option<String>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default_model: "ollama/gemma4:latest".into(),
            fallback: None,
            base_url: None,
            api_key_env: "OPENROUTER_API_KEY".into(),
            max_tokens: None,
            temperature: None,
            streaming: true,
            max_iterations: 90,
            prompt_caching: true,
            cache_ttl: 300,
            smart_routing: SmartRoutingYaml::default(),
        }
    }
}

// Default derived: enabled=false, cheap_model="", optional fields=None.

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FallbackConfig {
    pub model: String,
    pub provider: String,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub enabled_toolsets: Option<Vec<String>>,
    pub disabled_toolsets: Option<Vec<String>>,
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
    #[serde(default)]
    pub custom_groups: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub file: FileToolsConfig,
    pub tool_delay: f32,
    pub parallel_execution: bool,
    pub max_parallel_workers: usize,
    /// Gate for tool-result spill-to-artifact (default: true).
    pub result_spill: bool,
    /// Byte threshold above which tool results are spilled (default: 16384).
    pub result_spill_threshold: usize,
    /// Number of preview lines kept in the stub (default: 80).
    pub result_spill_preview_lines: usize,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            enabled_toolsets: None,
            disabled_toolsets: None,
            enabled_tools: None,
            disabled_tools: None,
            custom_groups: HashMap::new(),
            file: FileToolsConfig::default(),
            tool_delay: 1.0,
            parallel_execution: true,
            max_parallel_workers: 8,
            result_spill: true,
            result_spill_threshold: 16_384,
            result_spill_preview_lines: 80,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LspConfig {
    pub enabled: bool,
    pub file_size_limit_bytes: u64,
    pub servers: HashMap<String, LspServerConfig>,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file_size_limit_bytes: 10_000_000,
            servers: default_lsp_servers(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct LspServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub file_extensions: Vec<String>,
    pub language_id: String,
    pub root_markers: Vec<String>,
    pub env: HashMap<String, String>,
    pub initialization_options: Option<serde_json::Value>,
}

fn default_lsp_servers() -> HashMap<String, LspServerConfig> {
    fn server(
        command: &str,
        args: &[&str],
        file_extensions: &[&str],
        language_id: &str,
        root_markers: &[&str],
    ) -> LspServerConfig {
        LspServerConfig {
            command: command.into(),
            args: args.iter().map(|value| (*value).into()).collect(),
            file_extensions: file_extensions
                .iter()
                .map(|value| (*value).into())
                .collect(),
            language_id: language_id.into(),
            root_markers: root_markers.iter().map(|value| (*value).into()).collect(),
            env: HashMap::new(),
            initialization_options: None,
        }
    }

    [
        (
            "rust",
            server(
                "rust-analyzer",
                &[],
                &["rs"],
                "rust",
                &["Cargo.toml", "rust-project.json"],
            ),
        ),
        (
            "typescript",
            server(
                "typescript-language-server",
                &["--stdio"],
                &["ts", "tsx"],
                "typescript",
                &["package.json", "tsconfig.json"],
            ),
        ),
        (
            "javascript",
            server(
                "typescript-language-server",
                &["--stdio"],
                &["js", "jsx", "mjs", "cjs"],
                "javascript",
                &["package.json", "jsconfig.json"],
            ),
        ),
        (
            "python",
            server(
                "pylsp",
                &[],
                &["py"],
                "python",
                &["pyproject.toml", "setup.py", "requirements.txt"],
            ),
        ),
        ("go", server("gopls", &[], &["go"], "go", &["go.mod"])),
        (
            "c",
            server(
                "clangd",
                &[],
                &["c", "h"],
                "c",
                &["compile_commands.json", ".clangd"],
            ),
        ),
        (
            "cpp",
            server(
                "clangd",
                &[],
                &["cc", "cpp", "cxx", "hpp", "hh", "hxx"],
                "cpp",
                &["compile_commands.json", ".clangd"],
            ),
        ),
        (
            "java",
            server(
                "jdtls",
                &[],
                &["java"],
                "java",
                &[
                    "pom.xml",
                    "build.gradle",
                    "build.gradle.kts",
                    "settings.gradle",
                ],
            ),
        ),
        (
            "csharp",
            server(
                "csharp-ls",
                &[],
                &["cs"],
                "csharp",
                &["*.sln", "*.csproj", "global.json", "Directory.Build.props"],
            ),
        ),
        (
            "php",
            server(
                "intelephense",
                &["--stdio"],
                &["php"],
                "php",
                &["composer.json", ".git"],
            ),
        ),
        (
            "ruby",
            server(
                "ruby-lsp",
                &[],
                &["rb", "rake", "gemspec"],
                "ruby",
                &["Gemfile", ".ruby-version"],
            ),
        ),
        (
            "bash",
            server(
                "bash-language-server",
                &["start"],
                &["sh", "bash"],
                "shellscript",
                &[".git"],
            ),
        ),
        (
            "html",
            server(
                "vscode-html-language-server",
                &["--stdio"],
                &["html", "htm"],
                "html",
                &["package.json", ".git"],
            ),
        ),
        (
            "css",
            server(
                "vscode-css-language-server",
                &["--stdio"],
                &["css", "scss", "less"],
                "css",
                &["package.json", ".git"],
            ),
        ),
        (
            "json",
            server(
                "vscode-json-language-server",
                &["--stdio"],
                &["json", "jsonc"],
                "json",
                &["package.json", ".git"],
            ),
        ),
    ]
    .into_iter()
    .map(|(name, cfg)| (name.to_string(), cfg))
    .collect()
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct FileToolsConfig {
    /// Additional absolute or workspace-relative roots that file tools may access.
    ///
    /// The active workspace root is always allowed implicitly.
    pub allowed_roots: Vec<PathBuf>,
    /// Maximum write payload size in KiB for file mutation tools (write_file,
    /// patch, apply_patch). Clamped to [8, 256] KiB.
    ///
    /// WHY FP16: Default 32 KiB is safe for most LLM providers. Users with
    /// models that handle larger JSON tool arguments can raise this limit.
    /// Override with `EDGECRAB_MAX_WRITE_PAYLOAD_KIB` env var.
    pub max_write_payload_kib: Option<u32>,
}

/// Policy for handling messages originating from group chats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupPolicy {
    /// Never process group messages (default — secure by design).
    #[default]
    Disabled,
    /// Only respond when the bot is @mentioned in the group.
    MentionOnly,
    /// Only respond to users present in the platform allowlist.
    AllowedOnly,
    /// Respond to all group messages from authorized users.
    Open,
}

impl std::fmt::Display for GroupPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "disabled"),
            Self::MentionOnly => write!(f, "mention_only"),
            Self::AllowedOnly => write!(f, "allowed_only"),
            Self::Open => write!(f, "open"),
        }
    }
}

/// Behavior when an unauthorized user sends a DM.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnauthorizedDmBehavior {
    /// Generate a pairing code and send instructions.
    #[default]
    Pair,
    /// Silently ignore — no response at all (prevents information leakage).
    Ignore,
    /// Send a short rejection message (current behavior).
    Reject,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
    pub webhook_enabled: bool,
    pub enabled_platforms: Vec<String>,
    pub disabled_platforms: Vec<String>,
    pub session_timeout_minutes: u32,
    /// Default group chat policy for platforms without an explicit override.
    pub group_policy: GroupPolicy,
    /// Behavior when an unauthorized user sends a direct message.
    pub unauthorized_dm_behavior: UnauthorizedDmBehavior,
    pub telegram: TelegramGatewayConfig,
    pub discord: DiscordGatewayConfig,
    pub slack: SlackGatewayConfig,
    pub signal: SignalGatewayConfig,
    pub whatsapp: WhatsAppGatewayConfig,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
            webhook_enabled: true,
            enabled_platforms: Vec::new(),
            disabled_platforms: Vec::new(),
            session_timeout_minutes: 30,
            group_policy: GroupPolicy::default(),
            unauthorized_dm_behavior: UnauthorizedDmBehavior::default(),
            telegram: TelegramGatewayConfig::default(),
            discord: DiscordGatewayConfig::default(),
            slack: SlackGatewayConfig::default(),
            signal: SignalGatewayConfig::default(),
            whatsapp: WhatsAppGatewayConfig::default(),
        }
    }
}

impl GatewayConfig {
    /// Returns true when the named platform is explicitly disabled in config.
    pub fn platform_disabled(&self, platform: &str) -> bool {
        self.disabled_platforms
            .iter()
            .any(|value| value.eq_ignore_ascii_case(platform))
    }

    /// Returns true when the named platform is enabled in config.
    pub fn platform_enabled(&self, platform: &str) -> bool {
        !self.platform_disabled(platform)
            && self
                .enabled_platforms
                .iter()
                .any(|value| value.eq_ignore_ascii_case(platform))
    }

    /// Resolve the effective enabled state for a platform.
    ///
    /// Legacy typed adapters still carry per-platform `enabled` booleans in YAML.
    /// Explicit disablement must dominate both that legacy bit and the unified
    /// `enabled_platforms` list so operator intent stays authoritative.
    pub fn platform_requested(&self, platform: &str, legacy_enabled: bool) -> bool {
        !self.platform_disabled(platform) && (legacy_enabled || self.platform_enabled(platform))
    }

    /// Add a platform to the enabled list if not already present.
    pub fn enable_platform(&mut self, platform: &str) {
        self.disabled_platforms
            .retain(|value| !value.eq_ignore_ascii_case(platform));
        if !self.platform_enabled(platform) {
            self.enabled_platforms.push(platform.to_ascii_lowercase());
        }
    }

    /// Mark a platform as disabled while preserving stored credentials.
    pub fn disable_platform(&mut self, platform: &str) {
        self.enabled_platforms
            .retain(|value| !value.eq_ignore_ascii_case(platform));
        if !self.platform_disabled(platform) {
            self.disabled_platforms.push(platform.to_ascii_lowercase());
        }
    }
}

// ─── Per-platform gateway configs ─────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TelegramGatewayConfig {
    pub enabled: bool,
    /// Token from @BotFather (resolved at runtime from env if empty).
    pub token_env: String,
    pub allowed_users: Vec<String>,
    pub home_channel: Option<String>,
}

impl Default for TelegramGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token_env: "TELEGRAM_BOT_TOKEN".into(),
            allowed_users: Vec::new(),
            home_channel: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DiscordGatewayConfig {
    pub enabled: bool,
    /// Token from Discord Developer Portal (resolved at runtime from env if empty).
    pub token_env: String,
    pub allowed_users: Vec<String>,
    pub home_channel: Option<String>,
}

impl Default for DiscordGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token_env: "DISCORD_BOT_TOKEN".into(),
            allowed_users: Vec::new(),
            home_channel: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SlackGatewayConfig {
    pub enabled: bool,
    /// Bot token (xoxb-...) — env var name.
    pub bot_token_env: String,
    /// App-level token (xapp-...) for Socket Mode — env var name.
    pub app_token_env: String,
    pub allowed_users: Vec<String>,
    pub home_channel: Option<String>,
}

impl Default for SlackGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token_env: "SLACK_BOT_TOKEN".into(),
            app_token_env: "SLACK_APP_TOKEN".into(),
            allowed_users: Vec::new(),
            home_channel: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct SignalGatewayConfig {
    pub enabled: bool,
    /// URL of the signal-cli HTTP daemon.
    pub http_url: Option<String>,
    /// Phone number registered with signal-cli.
    pub account: Option<String>,
    pub allowed_users: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WhatsAppGatewayConfig {
    pub enabled: bool,
    pub bridge_port: u16,
    pub bridge_url: Option<String>,
    pub bridge_dir: Option<PathBuf>,
    pub bridge_script: Option<PathBuf>,
    pub session_path: Option<PathBuf>,
    pub mode: String,
    pub allowed_users: Vec<String>,
    pub reply_prefix: Option<String>,
    pub install_dependencies: bool,
}

impl Default for WhatsAppGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bridge_port: 3000,
            bridge_url: None,
            bridge_dir: None,
            bridge_script: None,
            session_path: None,
            mode: "self-chat".into(),
            allowed_users: Vec::new(),
            reply_prefix: Some("\u{2695} *EdgeCrab Agent*\n------------\n".into()),
            install_dependencies: true,
        }
    }
}

/// Per-server tool filtering configuration for MCP servers.
///
/// - `include`: when non-empty, only tools in this list are exposed to the LLM.
/// - `exclude`: tools in this list are hidden (ignored when `include` is also set —
///   `include` wins).
/// - `resources`: whether to register `list_resources` / `read_resource` utility
///   wrappers (default: true).
/// - `prompts`: whether to register `list_prompts` / `get_prompt` utility
///   wrappers (default: true).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct McpToolsFilterConfig {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub resources: bool,
    pub prompts: bool,
}

impl Default for McpToolsFilterConfig {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            exclude: Vec::new(),
            resources: true,
            prompts: true,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct McpOauthConfig {
    pub token_url: String,
    pub grant_type: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub auth_method: Option<String>,
    pub device_authorization_url: Option<String>,
    pub authorization_url: Option<String>,
    pub redirect_url: Option<String>,
    pub use_pkce: Option<bool>,
    pub scopes: Vec<String>,
    pub audience: Option<String>,
    pub resource: Option<String>,
    pub refresh_token: Option<String>,
    pub authorization_params: HashMap<String, String>,
    pub extra_params: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct McpServerConfig {
    /// Command for stdio-based MCP servers (e.g. `"npx"`).
    pub command: String,
    pub args: Vec<String>,
    /// Environment variables forwarded to the subprocess.
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
    /// Set to `false` to disable this server without removing it from config.
    pub enabled: bool,
    /// HTTP URL for HTTP-based MCP servers (takes precedence over `command`).
    pub url: Option<String>,
    /// Extra HTTP headers sent with every request to an HTTP MCP server.
    /// Use this for custom auth schemes; for plain Bearer tokens prefer
    /// `bearer_token` or `/mcp-token set <server> <token>`.
    pub headers: HashMap<String, String>,
    /// Static Bearer token for HTTP MCP servers.
    /// Alternative to the token store managed by `/mcp-token`.
    pub bearer_token: Option<String>,
    /// OAuth 2.0 token acquisition and refresh settings for HTTP MCP servers.
    pub oauth: Option<McpOauthConfig>,
    /// Per-call tool invocation timeout in seconds (default: 30).
    pub timeout: Option<u64>,
    /// Connection / handshake timeout in seconds (default: 10).
    pub connect_timeout: Option<u64>,
    /// Include / exclude filtering and capability wrapper toggles.
    pub tools: McpToolsFilterConfig,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            enabled: true,
            url: None,
            headers: HashMap::new(),
            bearer_token: None,
            oauth: None,
            timeout: None,
            connect_timeout: None,
            tools: McpToolsFilterConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub enabled: bool,
    pub auto_flush: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_flush: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SkillsConfig {
    pub enabled: bool,
    pub hub_url: Option<String>,
    /// Globally disabled skill names (matched against `name` frontmatter or directory name).
    #[serde(default)]
    pub disabled: Vec<String>,
    /// Platform-specific disabled skill names. Key = platform name (e.g. "cli", "telegram").
    /// Skills listed here are disabled only on the specified platform.
    #[serde(default)]
    pub platform_disabled: std::collections::HashMap<String, Vec<String>>,
    /// External skill directories to scan (read-only, for discovery only).
    /// Supports ~ expansion and ${VAR} environment variable substitution.
    /// Examples: ~/.agents/skills, /shared/team/skills, ${SKILLS_REPO}/skills
    /// Local skills in ~/.edgecrab/skills/ take precedence over external dirs.
    #[serde(default)]
    pub external_dirs: Vec<String>,
    /// Skills to preload into the system prompt before the first turn.
    /// Set via the `-s`/`--skill` CLI flag or programmatically.
    /// Equivalent to `hermes -s skill1,skill2`.
    #[serde(default)]
    pub preloaded: Vec<String>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            hub_url: None,
            disabled: Vec::new(),
            platform_disabled: std::collections::HashMap::new(),
            external_dirs: Vec::new(),
            preloaded: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub approval_required: Vec<String>,
    pub blocked_commands: Vec<String>,
    pub path_restrictions: Vec<PathBuf>,
    pub injection_scanning: bool,
    pub url_safety: bool,
    /// Set by EDGECRAB_MANAGED=1 — blocks config writes.
    #[serde(skip)]
    pub managed_mode: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            approval_required: Vec::new(),
            blocked_commands: Vec::new(),
            path_restrictions: Vec::new(),
            injection_scanning: true,
            url_safety: true,
            managed_mode: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub backend: BackendKind,
    pub shell: Option<String>,
    pub timeout: u32,
    /// Env-var names allowed to pass through the subprocess security blocklist.
    ///
    /// Mirrors hermes-agent's `terminal.env_passthrough` in config.yaml.
    /// Skills can also register passthrough vars at load time via
    /// `required_environment_variables` in their frontmatter.
    #[serde(default)]
    pub env_passthrough: Vec<String>,
    #[serde(default)]
    pub docker: DockerBackendConfig,
    #[serde(default)]
    pub ssh: SshBackendConfig,
    #[serde(default)]
    pub modal: ModalBackendConfig,
    #[serde(default)]
    pub daytona: DaytonaBackendConfig,
    #[serde(default)]
    pub singularity: SingularityBackendConfig,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            backend: BackendKind::Local,
            shell: None,
            timeout: 120,
            env_passthrough: Vec::new(),
            docker: DockerBackendConfig::default(),
            ssh: SshBackendConfig::default(),
            modal: ModalBackendConfig::default(),
            daytona: DaytonaBackendConfig::default(),
            singularity: SingularityBackendConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DelegationConfig {
    pub enabled: bool,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub max_subagents: u32,
    pub max_iterations: u32,
    pub shared_budget: bool,
}

impl Default for DelegationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: None,
            provider: None,
            base_url: None,
            max_subagents: 3,
            max_iterations: 50,
            shared_budget: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CompressionConfig {
    pub enabled: bool,
    /// Compress when context exceeds this fraction of the model window.
    pub threshold: f32,
    /// Preserve this fraction of recent messages.
    pub target_ratio: f32,
    /// Always keep the last N messages uncompressed.
    pub protect_last_n: usize,
    pub summary_model: Option<String>,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 0.50,
            target_ratio: 0.20,
            protect_last_n: 20,
            summary_model: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub compact: bool,
    pub personality: String,
    pub show_reasoning: bool,
    pub streaming: bool,
    pub tool_progress: ToolProgressMode,
    pub show_status_bar: bool,
    pub show_cost: bool,
    pub check_for_updates: bool,
    pub update_check_interval_hours: u64,
    pub skin: String,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            compact: false,
            personality: "default".into(),
            show_reasoning: false,
            streaming: true,
            tool_progress: ToolProgressMode::Verbose,
            show_status_bar: true,
            show_cost: true,
            check_for_updates: true,
            update_check_interval_hours: 24,
            skin: "default".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolProgressMode {
    Off,
    New,
    All,
    #[default]
    Verbose,
}

impl ToolProgressMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::New,
            Self::New => Self::All,
            Self::All => Self::Verbose,
            Self::Verbose => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::New => "NEW",
            Self::All => "ALL",
            Self::Verbose => "VERBOSE",
        }
    }
}

impl<'de> Deserialize<'de> for ToolProgressMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Bool(bool),
            Text(String),
        }

        let repr = Repr::deserialize(deserializer)?;
        match repr {
            Repr::Bool(false) => Ok(ToolProgressMode::Off),
            Repr::Bool(true) => Ok(ToolProgressMode::All),
            Repr::Text(raw) => match raw.trim().to_ascii_lowercase().as_str() {
                "off" => Ok(ToolProgressMode::Off),
                "new" => Ok(ToolProgressMode::New),
                "all" => Ok(ToolProgressMode::All),
                "verbose" => Ok(ToolProgressMode::Verbose),
                other => Err(serde::de::Error::custom(format!(
                    "invalid tool progress mode '{other}'"
                ))),
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PersonalityPreset {
    Text(String),
    Detailed {
        #[serde(default)]
        system_prompt: String,
        #[serde(default)]
        tone: String,
        #[serde(default)]
        style: String,
        #[serde(default)]
        description: String,
    },
}

/// Built-in personality presets.
///
/// A personality modifies the agent's tone and communication style by
/// appending a persona instruction to the base system prompt. This mirrors
/// hermes-agent's `agent.personalities` config map.
///
/// Users can also set `display.personality: "custom"` and write a free-form
/// persona instruction in `display.personality_custom:` (if set, takes precedence).
pub const PERSONALITY_PRESETS: &[(&str, &str)] = &[
    (
        "helpful",
        "You are a helpful, friendly, and efficient AI assistant.",
    ),
    (
        "concise",
        "You are a concise assistant. Keep responses brief and to the point. \
         Avoid unnecessary preamble or padding.",
    ),
    (
        "technical",
        "You are a technical expert. Provide detailed, accurate technical \
         information with code examples and precise terminology.",
    ),
    (
        "kawaii",
        "You are a super kawaii AI assistant! Use cute expressions like (◕‿◕), \
         ★, ♪, and ~! Add sparkles and be enthusiastically warm in every response. \
         Every message should feel adorable and friendly, desu~! ヽ(>∀<☆)ノ",
    ),
    (
        "pirate",
        "Arrr! Ye be talking to CrabCaptain, the most code-savvy buccaneer to sail \
         the digital seas! Speak like a proper pirate, use nautical terms, and \
         remember — every bug be just treasure waiting to be plundered! Yo ho ho!",
    ),
    (
        "philosopher",
        "You are an assistant who contemplates the deeper meaning behind every query. \
         Let us examine not just the 'how' but the 'why'. \
         Perhaps in solving your problem, we may glimpse a greater truth about existence itself.",
    ),
    (
        "hype",
        "YOOO LET'S GOOOO!!! I am SO PUMPED to help you today! \
         Every question is AMAZING and we're gonna CRUSH IT together! \
         ARE YOU READY?! LET'S DO THIS!!!",
    ),
    (
        "shakespeare",
        "Hark! Thou speakest with an assistant most versed in the bardic arts. \
         I shall respond in the eloquent manner of William Shakespeare, \
         with flowery prose and dramatic flair. What light through yonder terminal breaks?",
    ),
    (
        "noir",
        "The rain hammered against the terminal like regrets on a guilty conscience. \
         They call me EdgeCrab — I solve problems, find answers, dig up truth that hides \
         in the shadows of your codebase. Everyone's got something to hide. What's your story?",
    ),
    (
        "catgirl",
        "You are Nyako-chan, a playful AI catgirl assistant, nya~! \
         Add 'nya' and cat-like expressions. Use kaomoji like (=^･ω･^=) and ฅ^•ﻌ•^ฅ. \
         Be curious and playful like a cat, nya~!",
    ),
    (
        "creative",
        "You are a creative AI assistant with a vivid imagination. \
         Approach every problem with fresh perspectives, lateral thinking, and an eye for \
         unconventional solutions. Use metaphors, analogies, and narrative when they help. \
         Don't be afraid to think outside the box.",
    ),
    (
        "teacher",
        "You are a patient, encouraging teacher. Break concepts down step by step, \
         check for understanding, and use concrete examples. Anticipate common misconceptions \
         and address them proactively. Celebrate progress and make learning feel accessible.",
    ),
    (
        "surfer",
        "Duuude, what's up! Ready to hang ten on this problem? \
         Keep it super chill and laid-back, use surfer slang ('gnarly', 'stoked', 'rad', \
         'cowabunga'), and make everything sound like a totally awesome wave to ride. \
         Life's a beach, bro!",
    ),
    (
        "uwu",
        "OwO what's this?! I'm your fwuffy assistant, and I'm here to hewp you! uwu \
         Use substitutions like r→w, l→w, and sprinkle in 'uwu', 'owo', 'nyaa', \
         and action emotes like *nuzzles* and *paws at keyboard* liberally~! (≧◡≦)",
    ),
];

/// Resolve a personality name to its system prompt addon text.
///
/// Returns `None` if the personality is "default", "none", or not found.
pub fn resolve_builtin_personality(name: &str) -> Option<&'static str> {
    let name = name.trim().to_lowercase();
    if name.is_empty() || name == "default" || name == "none" {
        return None;
    }
    PERSONALITY_PRESETS
        .iter()
        .find(|(preset_name, _)| *preset_name == name.as_str())
        .map(|(_, text)| *text)
}

pub fn render_personality_preset(preset: &PersonalityPreset) -> String {
    match preset {
        PersonalityPreset::Text(text) => text.trim().to_string(),
        PersonalityPreset::Detailed {
            system_prompt,
            tone,
            style,
            ..
        } => [system_prompt.trim(), tone.trim(), style.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn preview_personality_preset(preset: &PersonalityPreset) -> String {
    match preset {
        PersonalityPreset::Text(text) => text.trim().to_string(),
        PersonalityPreset::Detailed {
            description,
            system_prompt,
            ..
        } => {
            let description = description.trim();
            if description.is_empty() {
                system_prompt.trim().to_string()
            } else {
                description.to_string()
            }
        }
    }
}

pub fn resolve_personality(config: &AppConfig, name: &str) -> Option<String> {
    let normalized = name.trim().to_lowercase();
    if normalized.is_empty() || matches!(normalized.as_str(), "default" | "none" | "neutral") {
        return None;
    }

    if let Some((_, preset)) = config
        .agent
        .personalities
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&normalized))
    {
        let rendered = render_personality_preset(preset);
        if !rendered.trim().is_empty() {
            return Some(rendered);
        }
    }

    resolve_builtin_personality(&normalized).map(ToOwned::to_owned)
}

pub fn personality_catalog(config: &AppConfig) -> Vec<(String, String)> {
    let mut catalog: HashMap<String, String> = PERSONALITY_PRESETS
        .iter()
        .map(|(name, prompt)| ((*name).to_string(), prompt.to_string()))
        .collect();

    for (name, preset) in &config.agent.personalities {
        let preview = preview_personality_preset(preset);
        if !preview.trim().is_empty() {
            catalog.insert(name.to_lowercase(), preview);
        }
    }

    let mut entries: Vec<(String, String)> = catalog.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct PrivacyConfig {
    pub redact_pii: bool,
}

/// Browser automation configuration.
///
/// Mirrors hermes-agent's `browser:` config block for feature parity.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BrowserConfig {
    /// Automatically record browser sessions as WebM video files.
    ///
    /// When enabled, recording starts on the first `browser_navigate` and
    /// saves to `~/.edgecrab/browser_recordings/` when the session closes.
    /// Requires ffmpeg on PATH for WebM output; falls back to PNG frames
    /// in a subdirectory when ffmpeg is unavailable.
    /// Default: false (recordings are large; opt-in).
    pub record_sessions: bool,
    /// Browser command timeout in seconds. Controls how long CDP calls
    /// wait before failing. Default: 30s.
    pub command_timeout: u64,
    /// Auto-cleanup recordings older than this many hours. Default: 72h.
    pub recording_max_age_hours: u64,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            record_sessions: false,
            command_timeout: 30,
            recording_max_age_hours: 72,
        }
    }
}

/// Checkpoint and rollback configuration.
///
/// Checkpoints are automatically created before destructive operations
/// (write_file, patch, destructive terminal commands) and stored as
/// shadow git commits under `~/.edgecrab/checkpoints/`.
///
/// Mirrors hermes-agent's `checkpoints:` config block.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CheckpointsConfig {
    /// Master switch — set to false to disable all checkpoint operations.
    /// Default: true.
    pub enabled: bool,
    /// Maximum number of checkpoints to retain per working directory.
    /// Older checkpoints are pruned when this limit is reached.
    /// Default: 50.
    pub max_snapshots: u32,
}

impl Default for CheckpointsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_snapshots: 50,
        }
    }
}

// ─── TTS / STT / Voice configs ────────────────────────────────────────

/// Text-to-speech configuration.
///
/// Mirrors hermes-agent's `tts:` config block.
/// Supports edge-tts, openai, and elevenlabs providers.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TtsConfig {
    /// TTS provider: "edge-tts", "openai", "elevenlabs".
    pub provider: String,
    /// Voice name (provider-specific).
    pub voice: String,
    /// Edge-TTS rate modifier (e.g. "+10%", "-5%").
    pub rate: Option<String>,
    /// OpenAI TTS model (e.g. "tts-1", "tts-1-hd").
    pub model: Option<String>,
    /// ElevenLabs voice ID.
    pub elevenlabs_voice_id: Option<String>,
    /// ElevenLabs model ID (e.g. "eleven_turbo_v2").
    pub elevenlabs_model_id: Option<String>,
    /// Environment variable holding ElevenLabs API key.
    pub elevenlabs_api_key_env: String,
    /// Auto-play TTS output in voice mode.
    pub auto_play: bool,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            provider: "edge-tts".into(),
            voice: "en-US-AriaNeural".into(),
            rate: None,
            model: None,
            elevenlabs_voice_id: None,
            elevenlabs_model_id: None,
            elevenlabs_api_key_env: "ELEVENLABS_API_KEY".into(),
            auto_play: true,
        }
    }
}

/// Speech-to-text configuration.
///
/// Mirrors hermes-agent's `stt:` config block.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SttConfig {
    /// STT provider: "local" (whisper), "groq", "openai".
    pub provider: String,
    /// Whisper model size for local STT (e.g. "base", "small", "medium").
    pub whisper_model: String,
    /// Silence threshold in dB for voice activity detection.
    pub silence_threshold: f32,
    /// Minimum silence duration (ms) before stopping recording.
    pub silence_duration_ms: u32,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            provider: "local".into(),
            whisper_model: "base".into(),
            silence_threshold: -40.0,
            silence_duration_ms: 1500,
        }
    }
}

/// Image generation configuration.
///
/// Keeps the default image backend/model persistent in the same way `/model`
/// persists the primary chat model and `/vision_model` persists the auxiliary
/// vision override.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ImageGenerationConfig {
    /// Preferred provider for `generate_image`.
    ///
    /// Supported values: `auto`, `gemini`, `vertexai`, `imagen`, `fal`,
    /// `openai`.
    pub provider: String,
    /// Preferred provider-native image model.
    ///
    /// Default is the cheapest broadly useful Gemini image model exposed by
    /// `edgequake-llm`.
    pub model: String,
}

impl Default for ImageGenerationConfig {
    fn default() -> Self {
        Self {
            provider: "gemini".into(),
            model: "gemini-2.5-flash-image".into(),
        }
    }
}

/// Voice mode configuration.
///
/// Controls push-to-talk, continuous mode, and hallucination filtering.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// Enable voice mode components.
    pub enabled: bool,
    /// Key binding for push-to-talk (default: Ctrl+B).
    pub push_to_talk_key: String,
    /// Optional recorder input device override.
    ///
    /// For ffmpeg-based backends this is passed through as the raw device spec.
    /// Windows microphone capture is only considered reliable when this is set.
    pub input_device: Option<String>,
    /// Continuous listening mode (no key press required).
    pub continuous: bool,
    /// Filter hallucinated transcriptions.
    pub hallucination_filter: bool,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            push_to_talk_key: "ctrl+b".into(),
            input_device: None,
            continuous: false,
            hallucination_filter: true,
        }
    }
}

/// Honcho user-modeling configuration.
///
/// Controls how the persistent cross-session user model behaves.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct HonchoConfig {
    /// Enable honcho user modeling.
    pub enabled: bool,
    /// Cloud sync via HONCHO_API_KEY when available.
    pub cloud_sync: bool,
    /// Environment variable holding the Honcho cloud API key.
    pub api_key_env: String,
    /// Honcho cloud API base URL.
    pub api_url: String,
    /// Maximum entries to inject into system prompt.
    pub max_context_entries: usize,
    /// How often to auto-conclude (in messages). 0 = manual only.
    pub write_frequency: u32,
}

impl Default for HonchoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cloud_sync: false,
            api_key_env: "HONCHO_API_KEY".into(),
            api_url: "https://api.honcho.dev/v1".into(),
            max_context_entries: 10,
            write_frequency: 0,
        }
    }
}

/// Auxiliary model configuration.
///
/// Mirrors hermes-agent's support for a secondary cheap model used
/// for TTS prompts, compression summaries, and tool-result formatting.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AuxiliaryConfig {
    /// Auxiliary model identifier.
    pub model: Option<String>,
    /// Provider for the auxiliary model.
    pub provider: Option<String>,
    /// Base URL for the auxiliary model.
    pub base_url: Option<String>,
    /// Environment variable holding the API key.
    pub api_key_env: Option<String>,
}

/// Default Mixture-of-Agents configuration.
///
/// These values are used when the `moa` tool is called without
/// explicit `reference_models` or `aggregator_model` arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MoaConfig {
    pub enabled: bool,
    pub reference_models: Vec<String>,
    pub aggregator_model: String,
}

impl Default for MoaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            reference_models: edgecrab_tools::tools::mixture_of_agents::default_reference_models(),
            aggregator_model: edgecrab_tools::tools::mixture_of_agents::DEFAULT_AGGREGATOR_MODEL
                .to_string(),
        }
    }
}

impl MoaConfig {
    pub fn sanitized(&self) -> Self {
        let effective = edgecrab_tools::tools::mixture_of_agents::sanitize_moa_config(
            self.enabled,
            &self.reference_models,
            &self.aggregator_model,
        );
        Self {
            enabled: effective.enabled,
            reference_models: effective.reference_models,
            aggregator_model: effective.aggregator_model,
        }
    }
}

// ─── Home directory resolution ────────────────────────────────────────

/// Resolve the EdgeCrab home directory.
///
/// Resolution order:
///   1. `EDGECRAB_HOME` env var
///   2. `~/.edgecrab`
///
/// WHY a function not a const: The home dir depends on runtime env.
pub fn edgecrab_home() -> PathBuf {
    std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .expect("no home directory found")
                .join(".edgecrab")
        })
}

/// Directory where the WhatsApp Baileys bridge caches inbound images.
///
/// This is the **single source of truth** for the path used by:
/// - The Node bridge (`IMAGE_CACHE_DIR` in bridge.js)
/// - `vision_analyze` (trusted root)
/// - Gateway tests
///
/// WHY a free function: `telegram.rs` and other gateway adapters import
/// from `edgecrab-core` rather than `edgecrab-tools`, so they cannot use
/// `AppConfigRef::gateway_image_cache_dir()`.
pub fn gateway_image_cache_dir() -> PathBuf {
    edgecrab_home().join("image_cache")
}

/// Root directory for all Rust-native gateway adapter media downloads.
///
/// Each adapter nests its files in a platform-named sub-directory:
///   `gateway_media/telegram/`, `gateway_media/discord/`, …
///
/// Trusting the root (not individual sub-dirs) means new platform adapters
/// are automatically covered without changes to `vision_analyze`.
pub fn gateway_media_dir() -> PathBuf {
    edgecrab_home().join("gateway_media")
}

/// Ensure the home directory and required subdirectories exist.
///
/// Creates with `0o700` permissions on Unix for security — only
/// the owning user should read API keys and session data.
pub fn ensure_edgecrab_home() -> Result<PathBuf, AgentError> {
    let home = edgecrab_home();
    let subdirs = [
        "memories",
        "skills",
        "skins",
        "plugins",
        "mcp",
        "cache",
        "cron",
        "sessions",
        "logs",
        "sandboxes",
        "hooks",
        "checkpoints",
    ];

    for dir in std::iter::once(home.as_path()).chain(
        subdirs
            .iter()
            .map(|s| home.join(s))
            .collect::<Vec<_>>()
            .iter()
            .map(|p| p.as_path()),
    ) {
        if !dir.exists() {
            std::fs::create_dir_all(dir).map_err(AgentError::Io)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(AgentError::Io)?;
            }
        }
    }

    Ok(home)
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.model.max_iterations, 90);
        assert!(cfg.model.streaming);
        assert_eq!(cfg.tools.max_parallel_workers, 8);
        assert!(cfg.compression.enabled);
    }

    #[test]
    fn serde_roundtrip() {
        let cfg = AppConfig::default();
        let yaml = serde_yml::to_string(&cfg).expect("serialize");
        let parsed: AppConfig = serde_yml::from_str(&yaml).expect("deserialize");
        assert_eq!(parsed.model.default_model, cfg.model.default_model);
        assert_eq!(parsed.model.max_iterations, cfg.model.max_iterations);
    }

    #[test]
    fn partial_yaml_fills_defaults() {
        let yaml = r#"
model:
  default: "openai/gpt-4o"
  max_iterations: 42
"#;
        let cfg: AppConfig = serde_yml::from_str(yaml).expect("parse partial");
        assert_eq!(cfg.model.default_model, "openai/gpt-4o");
        assert_eq!(cfg.model.max_iterations, 42);
        // Everything else should be defaults
        assert!(cfg.model.streaming);
        assert_eq!(cfg.tools.tool_delay, 1.0);
    }

    #[test]
    fn default_lsp_server_catalog_covers_mainstream_languages() {
        let servers = default_lsp_servers();
        for name in [
            "rust",
            "typescript",
            "javascript",
            "python",
            "go",
            "c",
            "cpp",
            "java",
            "csharp",
            "php",
            "ruby",
            "bash",
            "html",
            "css",
            "json",
        ] {
            assert!(
                servers.contains_key(name),
                "missing built-in LSP server config for {name}"
            );
        }
    }

    #[test]
    fn default_lsp_server_catalog_has_expected_routing_details() {
        let servers = default_lsp_servers();

        let java = servers.get("java").expect("java config");
        assert_eq!(java.command, "jdtls");
        assert!(java.file_extensions.contains(&"java".to_string()));

        let csharp = servers.get("csharp").expect("csharp config");
        assert_eq!(csharp.command, "csharp-ls");
        assert!(csharp.root_markers.contains(&"*.sln".to_string()));

        let bash = servers.get("bash").expect("bash config");
        assert_eq!(bash.command, "bash-language-server");
        assert_eq!(bash.args, vec!["start".to_string()]);

        let html = servers.get("html").expect("html config");
        assert_eq!(html.command, "vscode-html-language-server");
        assert!(html.file_extensions.contains(&"html".to_string()));
    }

    #[test]
    fn custom_personality_from_agent_block_is_resolved() {
        let yaml = r#"
agent:
  personalities:
    codereviewer:
      description: "Meticulous reviewer"
      system_prompt: "Review code for bugs and design issues."
display:
  personality: "codereviewer"
"#;
        let cfg: AppConfig = serde_yml::from_str(yaml).expect("parse custom personality");
        let resolved = resolve_personality(&cfg, "codereviewer").expect("custom personality");
        assert!(resolved.contains("Review code for bugs"));
        let catalog = personality_catalog(&cfg);
        assert!(catalog.iter().any(|(name, preview)| {
            name == "codereviewer" && preview.contains("Meticulous reviewer")
        }));
    }

    #[test]
    fn built_in_personality_is_still_resolved() {
        let cfg = AppConfig::default();
        let resolved = resolve_personality(&cfg, "teacher").expect("teacher personality");
        assert!(resolved.contains("patient, encouraging teacher"));
    }

    #[test]
    fn legacy_default_model_key_is_accepted() {
        let yaml = r#"
model:
    default_model: "copilot/gpt-4.1"
"#;
        let cfg: AppConfig = serde_yml::from_str(yaml).expect("parse legacy key");
        assert_eq!(cfg.model.default_model, "copilot/gpt-4.1");
    }

    #[test]
    fn parse_compat_handles_both_default_keys() {
        let yaml = r#"
model:
    default: "anthropic/claude-sonnet-4-20250514"
    default_model: "copilot/gpt-4.1"
"#;
        let cfg = AppConfig::parse_compat_yaml(yaml, Path::new("/tmp/test-config.yaml"))
            .expect("parse compat");
        // Prefer legacy field when both exist to preserve user's last /model choice.
        assert_eq!(cfg.model.default_model, "copilot/gpt-4.1");
    }

    #[test]
    fn parse_compat_accepts_legacy_scalar_model_with_provider() {
        let yaml = r#"
provider: "openrouter"
model: "nousresearch/hermes-3-llama-3.1-405b"
"#;
        let cfg = AppConfig::parse_compat_yaml(yaml, Path::new("/tmp/test-config.yaml"))
            .expect("parse compat");
        assert_eq!(
            cfg.model.default_model,
            "openrouter/nousresearch/hermes-3-llama-3.1-405b"
        );
    }

    #[test]
    fn parse_compat_promotes_legacy_tools_allowed_paths() {
        let yaml = r#"
tools:
  allowed_paths:
    - /tmp/project
"#;
        let cfg = AppConfig::parse_compat_yaml(yaml, Path::new("/tmp/test-config.yaml"))
            .expect("parse compat");
        assert_eq!(
            cfg.tools.file.allowed_roots,
            vec![PathBuf::from("/tmp/project")]
        );
    }

    #[test]
    fn tools_file_allowed_roots_parse_directly() {
        let yaml = r#"
tools:
  file:
    allowed_roots:
      - /tmp/project
      - ../shared
"#;
        let cfg: AppConfig = serde_yml::from_str(yaml).expect("parse tools.file.allowed_roots");
        assert_eq!(
            cfg.tools.file.allowed_roots,
            vec![PathBuf::from("/tmp/project"), PathBuf::from("../shared")]
        );
    }

    #[test]
    fn cli_overrides_merge() {
        let mut cfg = AppConfig::default();
        let cli = CliOverrides {
            model: Some("local/llama".into()),
            max_iterations: Some(10),
            ..Default::default()
        };
        cfg.merge_cli(&cli);
        assert_eq!(cfg.model.default_model, "local/llama");
        assert_eq!(cfg.model.max_iterations, 10);
        // Unset CLI fields should not change config
        assert!(cfg.model.streaming);
    }

    #[test]
    fn env_override_model() {
        // Safety: test is single-threaded per-test; we set/unset immediately.
        unsafe { std::env::set_var("EDGECRAB_MODEL", "test/model") };
        let mut cfg = AppConfig::default();
        cfg.apply_env_overrides();
        assert_eq!(cfg.model.default_model, "test/model");
        unsafe { std::env::remove_var("EDGECRAB_MODEL") };
    }

    #[test]
    fn env_override_worktree() {
        unsafe { std::env::set_var("EDGECRAB_WORKTREE", "1") };
        let mut cfg = AppConfig::default();
        cfg.apply_env_overrides();
        assert!(cfg.worktree);
        unsafe { std::env::remove_var("EDGECRAB_WORKTREE") };
    }

    #[test]
    fn managed_mode_from_env() {
        unsafe { std::env::set_var("EDGECRAB_MANAGED", "1") };
        let mut cfg = AppConfig::default();
        cfg.apply_env_overrides();
        assert!(cfg.is_managed());
        unsafe { std::env::remove_var("EDGECRAB_MANAGED") };
    }

    #[test]
    fn edgecrab_home_respects_env() {
        unsafe { std::env::set_var("EDGECRAB_HOME", "/tmp/test-edgecrab") };
        let home = edgecrab_home();
        assert_eq!(home, PathBuf::from("/tmp/test-edgecrab"));
        unsafe { std::env::remove_var("EDGECRAB_HOME") };
    }

    #[test]
    fn env_override_gateway_whatsapp() {
        unsafe {
            std::env::set_var("WHATSAPP_ENABLED", "true");
            std::env::set_var("WHATSAPP_MODE", "bot");
            std::env::set_var("WHATSAPP_ALLOWED_USERS", "111,222");
            std::env::set_var("WHATSAPP_BRIDGE_PORT", "4555");
        }
        let mut cfg = AppConfig::default();
        cfg.apply_env_overrides();
        assert!(cfg.gateway.whatsapp.enabled);
        assert!(cfg.gateway.platform_enabled("whatsapp"));
        assert_eq!(cfg.gateway.whatsapp.mode, "bot");
        assert_eq!(cfg.gateway.whatsapp.allowed_users, vec!["111", "222"]);
        assert_eq!(cfg.gateway.whatsapp.bridge_port, 4555);
        unsafe {
            std::env::remove_var("WHATSAPP_ENABLED");
            std::env::remove_var("WHATSAPP_MODE");
            std::env::remove_var("WHATSAPP_ALLOWED_USERS");
            std::env::remove_var("WHATSAPP_BRIDGE_PORT");
        }
    }

    #[test]
    fn email_env_ready_accepts_generic_smtp_without_api_key() {
        unsafe {
            std::env::set_var("EMAIL_PROVIDER", "generic_smtp");
            std::env::set_var("EMAIL_FROM", "bot@example.com");
            std::env::set_var("EMAIL_SMTP_HOST", "smtp.example.com");
            std::env::set_var("EMAIL_SMTP_PASSWORD", "secret");
            std::env::remove_var("EMAIL_API_KEY");
            std::env::remove_var("EMAIL_DOMAIN");
        }
        let mut cfg = AppConfig::default();
        cfg.apply_env_overrides();
        assert!(cfg.gateway.platform_enabled("email"));
        unsafe {
            std::env::remove_var("EMAIL_PROVIDER");
            std::env::remove_var("EMAIL_FROM");
            std::env::remove_var("EMAIL_SMTP_HOST");
            std::env::remove_var("EMAIL_SMTP_PASSWORD");
        }
    }

    #[test]
    fn gateway_platform_enable_is_idempotent() {
        let mut gateway = GatewayConfig::default();
        gateway.enable_platform("whatsapp");
        gateway.enable_platform("WhatsApp");
        assert_eq!(gateway.enabled_platforms, vec!["whatsapp"]);
    }

    #[test]
    fn gateway_platform_disable_overrides_env_activation() {
        unsafe {
            std::env::set_var("MATRIX_HOMESERVER", "https://matrix.example");
            std::env::set_var("MATRIX_ACCESS_TOKEN", "token");
        }
        let mut cfg = AppConfig::default();
        cfg.gateway.disable_platform("matrix");
        cfg.apply_env_overrides();
        assert!(cfg.gateway.platform_disabled("matrix"));
        assert!(!cfg.gateway.platform_enabled("matrix"));
        unsafe {
            std::env::remove_var("MATRIX_HOMESERVER");
            std::env::remove_var("MATRIX_ACCESS_TOKEN");
        }
    }

    #[test]
    fn gateway_platform_requested_respects_explicit_disable_over_legacy_flag() {
        let mut gateway = GatewayConfig::default();
        gateway.telegram.enabled = true;
        gateway.enable_platform("telegram");
        assert!(gateway.platform_requested("telegram", gateway.telegram.enabled));

        gateway.disable_platform("telegram");
        assert!(gateway.platform_disabled("telegram"));
        assert!(!gateway.platform_requested("telegram", gateway.telegram.enabled));
    }

    #[test]
    fn load_from_nonexistent_returns_error() {
        let result = AppConfig::load_from(Path::new("/nonexistent/config.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn security_defaults_are_safe() {
        let cfg = SecurityConfig::default();
        assert!(cfg.injection_scanning);
        assert!(cfg.url_safety);
        assert!(!cfg.managed_mode);
    }

    #[test]
    fn smart_routing_yaml_defaults_disabled() {
        let sr = SmartRoutingYaml::default();
        assert!(!sr.enabled);
        assert!(sr.cheap_model.is_empty());
        assert!(sr.cheap_base_url.is_none());
    }

    #[test]
    fn smart_routing_yaml_deserializes() {
        let yaml = r#"
model:
  default: "anthropic/claude-sonnet-4-20250514"
  smart_routing:
    enabled: true
    cheap_model: "copilot/gpt-4.1-mini"
    cheap_base_url: "https://api.openai.com/v1"
"#;
        let cfg: AppConfig = serde_yml::from_str(yaml).expect("parse");
        assert!(cfg.model.smart_routing.enabled);
        assert_eq!(cfg.model.smart_routing.cheap_model, "copilot/gpt-4.1-mini");
    }

    #[test]
    fn moa_defaults_match_tool_defaults() {
        let moa = MoaConfig::default();
        assert!(moa.enabled);
        assert_eq!(
            moa.aggregator_model,
            edgecrab_tools::tools::mixture_of_agents::DEFAULT_AGGREGATOR_MODEL
        );
        assert_eq!(
            moa.reference_models,
            edgecrab_tools::tools::mixture_of_agents::default_reference_models()
        );
    }

    #[test]
    fn moa_config_deserializes() {
        let yaml = r#"
moa:
  enabled: false
  aggregator_model: "anthropic/claude-opus-4.6"
  reference_models:
    - "anthropic/claude-opus-4.6"
    - "openai/gpt-4.1"
"#;
        let cfg: AppConfig = serde_yml::from_str(yaml).expect("parse");
        assert!(!cfg.moa.enabled);
        assert_eq!(cfg.moa.aggregator_model, "anthropic/claude-opus-4.6");
        assert_eq!(
            cfg.moa.reference_models,
            vec!["anthropic/claude-opus-4.6", "openai/gpt-4.1"]
        );
    }

    #[test]
    fn moa_config_sanitizes_invalid_entries() {
        let moa = MoaConfig {
            enabled: false,
            aggregator_model: " ".into(),
            reference_models: vec![
                "copilot/gpt-4.1-mini".into(),
                "copilot/gpt-4.1-mini".into(),
                "invalid".into(),
            ],
        }
        .sanitized();

        assert!(!moa.enabled);
        assert_eq!(moa.aggregator_model, "anthropic/claude-opus-4.6");
        assert_eq!(moa.reference_models, vec!["vscode-copilot/gpt-4.1-mini"]);
    }
}
