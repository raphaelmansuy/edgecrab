//! Lightweight config reference for tool context.
//!
//! WHY a separate type: edgecrab-core owns AppConfig but edgecrab-tools
//! can't depend on edgecrab-core (that would create a cycle). Instead,
//! we define a minimal config view here that edgecrab-core populates.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::execution_tmp::shared_tmp_dir;
use crate::tools::backends::{
    BackendKind, DaytonaBackendConfig, DockerBackendConfig, ModalBackendConfig,
    SingularityBackendConfig, SshBackendConfig,
};
use edgecrab_security::path_policy::PathPolicy;

/// Resolve the EdgeCrab home directory.
///
/// Resolution order:
///   1. `EDGECRAB_HOME` env var
///   2. `~/.edgecrab`
///
/// Duplicated from edgecrab-core/config.rs to avoid a circular crate dep.
pub fn resolve_edgecrab_home() -> PathBuf {
    std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if cfg!(test) {
                return std::env::temp_dir()
                    .join(format!("edgecrab-test-home-{}", std::process::id()));
            }
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".edgecrab")
        })
}

/// Minimal configuration view passed to tools via ToolContext.
///
/// Populated from AppConfig by the agent before tool dispatch.
/// Only includes fields that tools actually need.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
pub struct LspServerConfigRef {
    pub command: String,
    pub args: Vec<String>,
    pub file_extensions: Vec<String>,
    pub language_id: String,
    pub root_markers: Vec<String>,
    pub env: HashMap<String, String>,
    pub initialization_options: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct AppConfigRef {
    /// Whether the gateway process is running (gates send_message)
    pub gateway_running: bool,
    /// Whether Honcho integration is active (gates honcho_* tools)
    pub honcho_active: bool,
    /// Whether Home Assistant is configured (gates ha_* tools)
    pub home_assistant_active: bool,
    /// Max file size in bytes for read operations
    pub max_file_read_bytes: usize,
    /// Max output length for terminal commands
    pub max_terminal_output: usize,
    /// Additional file roots trusted by file tools beyond the active workspace.
    pub file_allowed_roots: Vec<PathBuf>,
    /// Denied prefixes layered on top of the workspace and allow-roots policy.
    pub path_restrictions: Vec<PathBuf>,
    /// Whether LSP tools are enabled for this session.
    pub lsp_enabled: bool,
    /// Max file size eligible for LSP document sync.
    pub lsp_file_size_limit_bytes: u64,
    /// Named language-server configurations keyed by logical language/server id.
    pub lsp_servers: HashMap<String, LspServerConfigRef>,
    /// EdgeCrab home directory (memory, skills, sessions storage root).
    ///
    /// WHY renamed from workspace_root: memory and skills tools write to
    /// `~/.edgecrab/memories/` and `~/.edgecrab/skills/` — NOT to the
    /// user's project CWD. Using a descriptive name prevents the same
    /// bug from recurring. The user's project CWD is `ToolContext::cwd`.
    pub edgecrab_home: PathBuf,
    /// Whether subagent delegation is enabled.
    pub delegation_enabled: bool,
    /// Optional model override for delegated children.
    pub delegation_model: Option<String>,
    /// Optional provider override used when delegation_model has no prefix.
    pub delegation_provider: Option<String>,
    /// Maximum number of children allowed in a single batch delegation call.
    pub delegation_max_subagents: u32,
    /// Default max iterations per child when delegate_task omits max_iterations.
    pub delegation_max_iterations: u32,
    /// Parent active toolsets used to ensure children cannot gain capabilities.
    /// Empty means no explicit whitelist (all available toolsets).
    pub parent_active_toolsets: Vec<String>,
    /// Toolsets explicitly disabled for the current session.
    ///
    /// WHY separate from `parent_active_toolsets`: when the session uses the
    /// implicit "all toolsets" mode, the allow-list is empty but specific
    /// toolsets may still be denied via config. Dispatch-time checks need the
    /// explicit deny-list so a hallucinated tool call cannot bypass schema
    /// filtering.
    pub disabled_toolsets: Vec<String>,
    /// Tools explicitly enabled for the current session.
    ///
    /// WHY separate from toolsets: users may want one browser or MCP helper
    /// without exposing the rest of that toolset.
    pub enabled_tools: Vec<String>,
    /// Tools explicitly disabled for the current session.
    ///
    /// Disabled tools always win, even if their parent toolset is enabled.
    pub disabled_tools: Vec<String>,
    /// External skill directories to scan in addition to ~/.edgecrab/skills/.
    /// Supports ~ and ${VAR} expansion (hermes-compatible paths).
    pub external_skill_dirs: Vec<String>,
    /// Skill names disabled globally or per-platform (merged).
    /// Tools (skills_list) should skip these.
    pub disabled_skills: Vec<String>,
    /// Plugin names disabled globally or per-platform (merged for the current platform).
    pub disabled_plugins: Vec<String>,
    /// Install root used for plugin discovery and runtime assets.
    pub plugin_install_dir: PathBuf,
    /// Record browser sessions as WebM video files when true.
    /// Mirrors `browser.record_sessions` in config.yaml.
    pub browser_record_sessions: bool,
    /// Browser CDP call timeout in seconds. Mirrors `browser.command_timeout`.
    pub browser_command_timeout: u64,
    /// Auto-cleanup browser recordings older than N hours.
    pub browser_recording_max_age_hours: u64,
    /// Whether automatic checkpoints are enabled.
    /// Mirrors `checkpoints.enabled` in config.yaml (default: true).
    pub checkpoints_enabled: bool,
    /// Maximum number of checkpoints to keep per working directory.
    /// Mirrors `checkpoints.max_snapshots` in config.yaml (default: 50).
    pub checkpoints_max_snapshots: u32,
    /// Skills to preload into the system prompt (from -s/--skill flags).
    pub preloaded_skills: Vec<String>,

    /// Env-var names allowed to bypass the subprocess security blocklist.
    ///
    /// Populated from `terminal.env_passthrough` in config.yaml and
    /// injected into the local env-passthrough registry on agent startup.
    /// Skills that declare `required_environment_variables` also feed into
    /// this registry at load time via `register_env_passthrough()`.
    pub terminal_env_passthrough: Vec<String>,

    // ── Terminal backend configuration (gap/backend B-01a/B-01b/B-02/B-03/B-04) ──
    /// Which execution backend the terminal tool should use.
    /// Defaults to `BackendKind::Local` (direct host execution).
    /// Override via `EDGECRAB_TERMINAL_BACKEND=docker|ssh|modal` or config.yaml.
    pub terminal_backend: BackendKind,

    /// Docker-specific terminal backend configuration.
    pub terminal_docker: DockerBackendConfig,

    /// SSH-specific terminal backend configuration.
    pub terminal_ssh: SshBackendConfig,

    /// Modal-specific terminal backend configuration.
    pub terminal_modal: ModalBackendConfig,

    /// Daytona-specific terminal backend configuration.
    pub terminal_daytona: DaytonaBackendConfig,

    /// Singularity-specific terminal backend configuration.
    pub terminal_singularity: SingularityBackendConfig,
    /// Optional dedicated provider for auxiliary side tasks such as vision.
    pub auxiliary_provider: Option<String>,
    /// Optional dedicated model for auxiliary side tasks such as vision.
    pub auxiliary_model: Option<String>,
    /// Optional base URL override for the auxiliary provider.
    pub auxiliary_base_url: Option<String>,
    /// Optional API-key environment variable for the auxiliary provider.
    pub auxiliary_api_key_env: Option<String>,
    /// Preferred text-to-speech provider from config (`tts.provider`).
    pub tts_provider: Option<String>,
    /// Preferred text-to-speech voice from config (`tts.voice`).
    pub tts_voice: Option<String>,
    /// Optional Edge TTS rate override from config (`tts.rate`).
    pub tts_rate: Option<String>,
    /// Optional provider-specific TTS model (`tts.model`).
    pub tts_model: Option<String>,
    /// Optional ElevenLabs voice id from config (`tts.elevenlabs_voice_id`).
    pub tts_elevenlabs_voice_id: Option<String>,
    /// Optional ElevenLabs model id from config (`tts.elevenlabs_model_id`).
    pub tts_elevenlabs_model_id: Option<String>,
    /// Environment variable name for ElevenLabs credentials.
    pub tts_elevenlabs_api_key_env: Option<String>,
    /// Preferred speech-to-text provider from config (`stt.provider`).
    pub stt_provider: Option<String>,
    /// Preferred local Whisper model from config (`stt.whisper_model`).
    pub stt_whisper_model: Option<String>,
    /// Preferred image-generation provider from config (`image_generation.provider`).
    pub image_provider: Option<String>,
    /// Preferred image-generation model from config (`image_generation.model`).
    pub image_model: Option<String>,
    /// Whether the `moa` tool is enabled for this session.
    pub moa_enabled: bool,
    /// Default reference models for the `moa` tool.
    pub moa_reference_models: Vec<String>,
    /// Default aggregator model for the `moa` tool.
    pub moa_aggregator_model: Option<String>,
    /// Whether tool-result spill-to-artifact is enabled (default: true).
    pub result_spill: bool,
    /// Byte threshold above which tool results are spilled to artifact files.
    pub result_spill_threshold: usize,
    /// Number of preview lines kept in the spill stub.
    pub result_spill_preview_lines: usize,
}

impl Default for AppConfigRef {
    fn default() -> Self {
        Self {
            gateway_running: false,
            honcho_active: false,
            home_assistant_active: false,
            max_file_read_bytes: 2 * 1024 * 1024, // 2 MB
            max_terminal_output: 100_000,         // 100K chars
            file_allowed_roots: Vec::new(),
            path_restrictions: Vec::new(),
            lsp_enabled: true,
            lsp_file_size_limit_bytes: 10_000_000,
            lsp_servers: HashMap::new(),
            edgecrab_home: resolve_edgecrab_home(),
            delegation_enabled: true,
            delegation_model: None,
            delegation_provider: None,
            delegation_max_subagents: 3,
            delegation_max_iterations: 50,
            parent_active_toolsets: Vec::new(),
            disabled_toolsets: Vec::new(),
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            external_skill_dirs: Vec::new(),
            disabled_skills: Vec::new(),
            disabled_plugins: Vec::new(),
            plugin_install_dir: resolve_edgecrab_home().join("plugins"),
            browser_record_sessions: false,
            browser_command_timeout: 30,
            browser_recording_max_age_hours: 72,
            checkpoints_enabled: true,
            checkpoints_max_snapshots: 50,
            preloaded_skills: Vec::new(),
            terminal_env_passthrough: Vec::new(),
            terminal_backend: BackendKind::Local,
            terminal_docker: DockerBackendConfig::default(),
            terminal_ssh: SshBackendConfig::default(),
            terminal_modal: ModalBackendConfig::default(),
            terminal_daytona: DaytonaBackendConfig::default(),
            terminal_singularity: SingularityBackendConfig::default(),
            auxiliary_provider: None,
            auxiliary_model: None,
            auxiliary_base_url: None,
            auxiliary_api_key_env: None,
            tts_provider: None,
            tts_voice: None,
            tts_rate: None,
            tts_model: None,
            tts_elevenlabs_voice_id: None,
            tts_elevenlabs_model_id: None,
            tts_elevenlabs_api_key_env: None,
            stt_provider: None,
            stt_whisper_model: None,
            image_provider: None,
            image_model: None,
            moa_enabled: true,
            moa_reference_models: Vec::new(),
            moa_aggregator_model: None,
            result_spill: true,
            result_spill_threshold: 16_384,
            result_spill_preview_lines: 80,
        }
    }
}

impl AppConfigRef {
    /// Whether a toolset is allowed in the current session.
    ///
    /// Empty `parent_active_toolsets` means "no explicit whitelist" rather than
    /// "nothing is allowed". The disabled list always wins.
    pub fn is_toolset_enabled(&self, toolset: &str) -> bool {
        (self.parent_active_toolsets.is_empty()
            || self.parent_active_toolsets.iter().any(|t| t == toolset))
            && !self.disabled_toolsets.iter().any(|t| t == toolset)
    }

    /// Whether a specific tool is allowed in the current session.
    pub fn is_tool_enabled(&self, tool_name: &str, toolset: &str) -> bool {
        crate::toolsets::tool_enabled(
            Some(&self.parent_active_toolsets),
            Some(&self.disabled_toolsets),
            Some(&self.enabled_tools),
            Some(&self.disabled_tools),
            tool_name,
            toolset,
        )
    }

    pub fn is_plugin_enabled(&self, plugin_name: &str) -> bool {
        !self
            .disabled_plugins
            .iter()
            .any(|candidate| candidate == plugin_name)
    }

    /// Build the effective file path policy for a session workspace.
    pub fn file_path_policy(&self, cwd: &std::path::Path) -> PathPolicy {
        let file_tools_tmp_dir = self.file_tools_tmp_dir();
        let _ = std::fs::create_dir_all(&file_tools_tmp_dir);

        let mut allowed = self.file_allowed_roots.clone();
        // On Termux, add the Termux data directory so file tools can access
        // Termux-installed packages, shared storage, and user scripts.
        if *edgecrab_types::IS_TERMUX {
            if let Ok(prefix) = std::env::var("PREFIX") {
                allowed.push(std::path::PathBuf::from(prefix));
            } else {
                allowed.push(std::path::PathBuf::from("/data/data/com.termux/files"));
            }
        }

        PathPolicy::new(cwd.to_path_buf())
            .with_virtual_tmp_root(file_tools_tmp_dir)
            .with_allowed_roots(allowed)
            .with_denied_roots(self.path_restrictions.clone())
    }

    pub fn lsp_server_for_extension(&self, ext: &str) -> Option<(&str, &LspServerConfigRef)> {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        self.lsp_servers.iter().find_map(|(name, cfg)| {
            cfg.file_extensions
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&ext))
                .then_some((name.as_str(), cfg))
        })
    }

    // ── Well-known directory helpers ──────────────────────────────────────
    //
    // WHY methods instead of ad-hoc `.join("image_cache")` calls:
    // Each directory name appears in multiple places (gateway adapters, vision
    // tool, tests). A single method per directory is the single source of truth;
    // rename one string and every caller updates automatically.

    /// Where the TUI saves clipboard-pasted images before sending to vision.
    pub fn tui_images_dir(&self) -> std::path::PathBuf {
        self.edgecrab_home.join("images")
    }

    /// Where the WhatsApp Baileys bridge caches inbound images.
    ///
    /// The Baileys Node bridge writes `img_<hex>.{jpg,png,…}` here when a
    /// WhatsApp message with a photo arrives.  The Rust gateway reads the
    /// `mediaUrls` list from the bridge and forwards the path to the agent.
    /// vision_analyze must trust this directory so the path-jail check passes.
    pub fn gateway_image_cache_dir(&self) -> std::path::PathBuf {
        self.edgecrab_home.join("image_cache")
    }

    /// Root directory for Rust-native gateway adapter media downloads.
    ///
    /// Each adapter nests its files in a platform-named sub-directory, e.g.:
    ///   `gateway_media/telegram/`
    ///   `gateway_media/discord/`
    ///
    /// vision_analyze trusts the root so all current and future platform
    /// sub-directories are covered without per-platform changes.
    pub fn gateway_media_dir(&self) -> std::path::PathBuf {
        self.edgecrab_home.join("gateway_media")
    }

    /// Where gateway platform adapters cache inbound document attachments.
    ///
    /// The WhatsApp Baileys bridge writes PDF and document files here
    /// (e.g. `doc_<hex>_filename.pdf`) when a document message arrives.
    /// `pdf_to_markdown` and other file tools must trust this directory so the
    /// path-jail check passes when the agent processes a gateway-received document.
    pub fn document_cache_dir(&self) -> std::path::PathBuf {
        self.edgecrab_home.join("document_cache")
    }

    /// Dedicated temp root for file tools.
    ///
    /// WHY not host `/tmp`: global temp directories are shared, nondeterministic,
    /// and can expose unrelated process files. File tools get an EdgeCrab-owned
    /// temp tree with stable semantics instead.
    pub fn file_tools_tmp_dir(&self) -> std::path::PathBuf {
        shared_tmp_dir(&self.edgecrab_home)
    }
}
