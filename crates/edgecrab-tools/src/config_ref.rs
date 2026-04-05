//! Lightweight config reference for tool context.
//!
//! WHY a separate type: edgecrab-core owns AppConfig but edgecrab-tools
//! can't depend on edgecrab-core (that would create a cycle). Instead,
//! we define a minimal config view here that edgecrab-core populates.

use std::path::PathBuf;

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
fn resolve_edgecrab_home() -> PathBuf {
    std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".edgecrab")
        })
}

/// Minimal configuration view passed to tools via ToolContext.
///
/// Populated from AppConfig by the agent before tool dispatch.
/// Only includes fields that tools actually need.
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
    /// External skill directories to scan in addition to ~/.edgecrab/skills/.
    /// Supports ~ and ${VAR} expansion (hermes-compatible paths).
    pub external_skill_dirs: Vec<String>,
    /// Skill names disabled globally or per-platform (merged).
    /// Tools (skills_list) should skip these.
    pub disabled_skills: Vec<String>,
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
            edgecrab_home: resolve_edgecrab_home(),
            delegation_enabled: true,
            delegation_model: None,
            delegation_provider: None,
            delegation_max_subagents: 3,
            delegation_max_iterations: 50,
            parent_active_toolsets: Vec::new(),
            disabled_toolsets: Vec::new(),
            external_skill_dirs: Vec::new(),
            disabled_skills: Vec::new(),
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

    /// Build the effective file path policy for a session workspace.
    pub fn file_path_policy(&self, cwd: &std::path::Path) -> PathPolicy {
        PathPolicy::new(cwd.to_path_buf())
            .with_allowed_roots(self.file_allowed_roots.clone())
            .with_denied_roots(self.path_restrictions.clone())
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
}
