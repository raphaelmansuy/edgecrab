//! # app — Main TUI application
//!
//! WHY ratatui: Native terminal UI with 60+ FPS rendering, <5MB memory.
//! Replaces Python's prompt_toolkit + Rich with a unified Rust stack.
//!
//! ```text
//!  ┌─────────────────────────────────────────┐
//!  │               Output Area               │  ← scrollable, markdown-rendered
//!  │  Shows assistant responses, tool output, │
//!  │  system messages, and errors.            │
//!  ├─────────────────────────────────────────┤
//!  │ ⠋ Thinking ·  🦀 model │ 1.2k │ $0.02 │  ← status bar + spinner
//!  ├─────────────────────────────────────────┤
//!  │ ┌ /comp│pletion ─────────────────┐     │  ← completion overlay
//!  │ │  /compress                      │     │
//!  │ │  /config                        │     │
//!  │ └────────────────────────────────┘     │
//!  │ > user input here...                    │  ← tui-textarea (multi-line)
//!  └─────────────────────────────────────────┘
//! ```
//!
//! # Phase 9 UX/UI Overhaul
//!
//! - tui-textarea replaces manual String buffer (multi-line, unicode-safe, readline shortcuts)
//! - Tab completion overlay for 42+ slash commands with fuzzy matching
//! - Animated braille spinner in status bar during agent processing
//! - Markdown rendering in output (headers, bold, code blocks with │ prefix)
//! - Auto-updating status bar (tokens, cost, model) after each response
//! - Input line highlighting (cyan for valid commands, red for invalid)
//! - Fish-style ghost text from input history

use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, MouseButton,
    MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};
use tokio::sync::mpsc;
use tui_textarea::{CursorMove, TextArea};
use unicode_width::UnicodeWidthStr;

use crate::commands::{CommandRegistry, CommandResult};
use crate::edit_diff::{LocalEditSnapshot, capture_local_edit_snapshot, render_edit_diff_lines};
use crate::fuzzy_selector::{FuzzyItem, FuzzySelector};
use crate::markdown_render;
use crate::model_discovery::{self, DiscoverySource};
use crate::theme::{SkinConfig, Theme};
use crate::tool_display::{
    build_subagent_event_line, build_tool_done_line, build_tool_running_line, tool_action_verb,
    tool_icon, tool_status_preview,
};
use crate::vision_models::{
    available_vision_model_options_with_dynamic, canonical_provider, current_model_supports_vision,
    parse_selection_spec,
};
use edgecrab_core::ModelCatalog;
use edgecrab_core::{Agent, IsolatedAgentOptions};
use edgequake_llm::{ProviderFactory, VsCodeCopilotProvider};

const KEYBOARD_PROTOCOL_WARMUP: Duration = Duration::from_millis(25);

fn progressive_keyboard_flags() -> KeyboardEnhancementFlags {
    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
}

fn format_context_pressure_notice(estimated_tokens: usize, threshold_tokens: usize) -> String {
    let ratio = if threshold_tokens == 0 {
        0.0
    } else {
        (estimated_tokens as f32 / threshold_tokens as f32).clamp(0.0, 1.0)
    };
    let percent = (ratio * 100.0).round() as usize;
    let width = 16usize;
    let filled = ((ratio * width as f32).round() as usize).min(width);
    let bar = format!("{}{}", "▰".repeat(filled), "▱".repeat(width - filled));
    format!(
        "⚠ Context {bar} {percent}% to compression ({estimated_tokens}/{threshold_tokens} tokens)"
    )
}

fn context_usage_ratio(tokens: u64, context_window: Option<u64>) -> Option<f64> {
    context_window
        .filter(|&cw| cw > 0)
        .map(|cw| (tokens as f64 / cw as f64).clamp(0.0, 1.0))
}

/// Recursively copy a directory tree from `src` to `dst`.
/// Returns the count of files copied, or an IO error.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(dst)?;
    let mut count = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            count += copy_dir_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
            count += 1;
        }
    }
    Ok(count)
}

/// Install a skill from GitHub by fetching it via the GitHub raw/contents API.
///
/// For single .md files, fetches the raw content.
/// For directories, uses the GitHub contents API to download all files.
///
/// `path` is the path within the repo (e.g. "skills/ascii-diagram-master").
async fn install_skill_from_github(
    owner: &str,
    repo: &str,
    path: &str,
    skills_dir: &std::path::Path,
) -> Result<String, String> {
    // Build an HTTP client with a browser-like User-Agent (GitHub requires it)
    let token = std::env::var("GITHUB_TOKEN").ok();
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(t) = &token {
        let auth: reqwest::header::HeaderValue = format!("Bearer {t}")
            .parse()
            .map_err(|e| format!("Invalid GITHUB_TOKEN: {e}"))?;
        headers.insert(reqwest::header::AUTHORIZATION, auth);
    }
    let accept: reqwest::header::HeaderValue = "application/vnd.github+json"
        .parse()
        .map_err(|e| format!("header: {e}"))?;
    headers.insert(reqwest::header::ACCEPT, accept);
    let ua: reqwest::header::HeaderValue = "edgecrab-agent/1.0"
        .parse()
        .map_err(|e| format!("ua: {e}"))?;
    headers.insert(reqwest::header::USER_AGENT, ua);

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // Query the GitHub Contents API to find out if path is file or directory
    let api_url = format!("https://api.github.com/repos/{owner}/{repo}/contents/{path}");
    let resp = client
        .get(&api_url)
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(format!(
            "GitHub API returned {status}. Check that {owner}/{repo}/{path} exists and is public."
        ));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("GitHub API response parse error: {e}"))?;

    std::fs::create_dir_all(skills_dir).map_err(|e| format!("Cannot create skills dir: {e}"))?;

    if body.is_array() {
        // It's a directory
        let items = body.as_array().unwrap();
        let dir_name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string());
        let dest_dir = skills_dir.join(&dir_name);
        std::fs::create_dir_all(&dest_dir).map_err(|e| format!("Cannot create skill dir: {e}"))?;

        let mut count = 0;
        for item in items {
            let file_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if file_type == "file" {
                let raw_url = item
                    .get("download_url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                if raw_url.is_empty() {
                    continue;
                }
                let content = client
                    .get(raw_url)
                    .send()
                    .await
                    .map_err(|e| format!("Download failed for {name}: {e}"))?
                    .text()
                    .await
                    .map_err(|e| format!("Read failed for {name}: {e}"))?;
                let dest = dest_dir.join(name);
                std::fs::write(&dest, content)
                    .map_err(|e| format!("Write failed for {name}: {e}"))?;
                count += 1;
            }
        }
        Ok(format!(
            "Skill '{dir_name}' installed from GitHub ({count} files)."
        ))
    } else if body.get("type").and_then(|t| t.as_str()) == Some("file") {
        // It's a file
        let raw_url = body
            .get("download_url")
            .and_then(|u| u.as_str())
            .ok_or("GitHub API: missing download_url")?;
        let content = client
            .get(raw_url)
            .send()
            .await
            .map_err(|e| format!("Download failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("Read failed: {e}"))?;
        let file_name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill.md".to_string());
        let dest = skills_dir.join(&file_name);
        std::fs::write(&dest, content).map_err(|e| format!("Write failed: {e}"))?;
        Ok(format!("Skill '{file_name}' installed from GitHub."))
    } else {
        Err(format!("Unexpected GitHub API response for path '{path}'"))
    }
}

fn load_config_root_for_edit() -> anyhow::Result<(std::path::PathBuf, serde_yml::Value)> {
    let config_path = edgecrab_core::edgecrab_home().join("config.yaml");
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut root: serde_yml::Value =
        serde_yml::from_str(&content).unwrap_or(serde_yml::Value::Mapping(Default::default()));

    if !matches!(root, serde_yml::Value::Mapping(_)) {
        root = serde_yml::Value::Mapping(Default::default());
    }

    Ok((config_path, root))
}

fn write_config_root(config_path: &std::path::Path, root: &serde_yml::Value) -> anyhow::Result<()> {
    let yaml = serde_yml::to_string(root)?;
    let header = "# EdgeCrab configuration\n\
                  # Edit this file to customize your setup.\n\
                  # Run `edgecrab doctor` to validate.\n\n";
    std::fs::write(config_path, format!("{header}{yaml}"))?;
    Ok(())
}

/// Persist the user's model choice to ~/.edgecrab/config.yaml.
fn persist_model_to_config(model: &str) -> anyhow::Result<()> {
    let (config_path, mut root) = load_config_root_for_edit()?;

    if let serde_yml::Value::Mapping(ref mut map) = root {
        let model_key = serde_yml::Value::String("model".into());
        let dm_key = serde_yml::Value::String("default".into());
        let legacy_dm_key = serde_yml::Value::String("default_model".into());
        let model_section = map
            .entry(model_key)
            .or_insert_with(|| serde_yml::Value::Mapping(Default::default()));
        if let serde_yml::Value::Mapping(m) = model_section {
            m.insert(dm_key, serde_yml::Value::String(model.into()));
            m.remove(&legacy_dm_key);
        }
    }

    write_config_root(&config_path, &root)
}

/// Persist the auxiliary vision-model routing to ~/.edgecrab/config.yaml.
fn persist_vision_model_to_config(
    auxiliary: &edgecrab_core::config::AuxiliaryConfig,
) -> anyhow::Result<()> {
    let (config_path, mut root) = load_config_root_for_edit()?;

    if let serde_yml::Value::Mapping(ref mut map) = root {
        let auxiliary_key = serde_yml::Value::String("auxiliary".into());
        let provider_key = serde_yml::Value::String("provider".into());
        let model_key = serde_yml::Value::String("model".into());
        let base_url_key = serde_yml::Value::String("base_url".into());
        let api_key_env_key = serde_yml::Value::String("api_key_env".into());

        let has_auxiliary = auxiliary
            .provider
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty())
            || auxiliary
                .model
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty())
            || auxiliary
                .base_url
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty())
            || auxiliary
                .api_key_env
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty());

        if has_auxiliary {
            let auxiliary_section = map
                .entry(auxiliary_key.clone())
                .or_insert_with(|| serde_yml::Value::Mapping(Default::default()));
            if let serde_yml::Value::Mapping(aux_map) = auxiliary_section {
                if let Some(provider) = auxiliary
                    .provider
                    .as_deref()
                    .filter(|v| !v.trim().is_empty())
                {
                    aux_map.insert(provider_key, serde_yml::Value::String(provider.to_string()));
                } else {
                    aux_map.remove(&provider_key);
                }
                if let Some(model) = auxiliary.model.as_deref().filter(|v| !v.trim().is_empty()) {
                    aux_map.insert(model_key, serde_yml::Value::String(model.to_string()));
                } else {
                    aux_map.remove(&model_key);
                }
                if let Some(base_url) = auxiliary
                    .base_url
                    .as_deref()
                    .filter(|v| !v.trim().is_empty())
                {
                    aux_map.insert(base_url_key, serde_yml::Value::String(base_url.to_string()));
                } else {
                    aux_map.remove(&base_url_key);
                }
                if let Some(api_key_env) = auxiliary
                    .api_key_env
                    .as_deref()
                    .filter(|v| !v.trim().is_empty())
                {
                    aux_map.insert(
                        api_key_env_key,
                        serde_yml::Value::String(api_key_env.to_string()),
                    );
                } else {
                    aux_map.remove(&api_key_env_key);
                }
            }
        } else {
            map.remove(&auxiliary_key);
        }
    }

    write_config_root(&config_path, &root)
}

#[derive(Debug, Clone, Copy)]
struct DisplayPreferences {
    show_reasoning: bool,
    streaming_enabled: bool,
}

impl Default for DisplayPreferences {
    fn default() -> Self {
        Self {
            show_reasoning: false,
            streaming_enabled: true,
        }
    }
}

/// Load the persisted display preferences from config.
fn load_display_preferences() -> DisplayPreferences {
    edgecrab_core::AppConfig::load()
        .map(|cfg| DisplayPreferences {
            show_reasoning: cfg.display.show_reasoning,
            streaming_enabled: cfg.model.streaming && cfg.display.streaming,
        })
        .unwrap_or_default()
}

/// Persist display preferences to `config.yaml`.
fn persist_display_preferences(
    show_reasoning: Option<bool>,
    streaming_enabled: Option<bool>,
) -> anyhow::Result<()> {
    let mut config = edgecrab_core::AppConfig::load().unwrap_or_default();
    if let Some(enabled) = show_reasoning {
        config.display.show_reasoning = enabled;
    }
    if let Some(enabled) = streaming_enabled {
        config.model.streaming = enabled;
        config.display.streaming = enabled;
    }
    config.save()?;
    Ok(())
}

// ─── Spinner frames (braille rotation) ──────────────────────────────
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Fixed display-column width for the thinking verb in the status bar.
/// Padding to this width prevents horizontal jitter as words rotate during animation.
/// Value = longest default verb ("hypothesizing" / "extrapolating" / "orchestrating" = 13).
const VERB_DISPLAY_PAD: usize = 13;

// THINKING_VERBS: the Theme now supplies thinking_verbs from SkinConfig
// (defaulting to DEFAULT_THINKING_VERBS in theme.rs). This constant is kept
// for reference / tests only and is no longer used in render_status_bar.
#[allow(dead_code)]
const THINKING_VERBS: &[&str] = &[
    "pondering",
    "contemplating",
    "reasoning",
    "analyzing",
    "computing",
    "synthesizing",
    "formulating",
    "processing",
    "deliberating",
    "mulling",
    "cogitating",
    "ruminating",
    "brainstorming",
    "reflecting",
    "deducing",
    "hypothesizing",
    "extrapolating",
    "orchestrating",
    "calibrating",
    "optimizing",
];

// ─── Kaomoji pools for tool completion display ────────────────────────
// Inspired by hermes-agent's KAWAII_* arrays.
// Single-width characters only — no wide emoji — for safe non-ratatui contexts.

/// Pad `s` to `target_display_cols` columns using unicode display width.
/// Safe for strings containing emoji and multi-width codepoints.
/// Returns `s` unchanged if it already meets or exceeds the target.
fn unicode_pad_right(s: &str, target_display_cols: usize) -> String {
    let w = s.width();
    if w >= target_display_cols {
        return s.to_string();
    }
    format!("{}{}", s, " ".repeat(target_display_cols - w))
}

/// Truncate `s` to at most `max_cols` display columns (unicode-safe).
/// Appends "..." if truncation occurred.
fn unicode_trunc(s: &str, max_cols: usize) -> String {
    let w = s.width();
    if w <= max_cols {
        return s.to_string();
    }
    // Walk chars until we would exceed max_cols - 3 (for "...")
    let budget = max_cols.saturating_sub(3);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if used + cw > budget {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push_str("...");
    out
}

/// Build the `*** ATTACHED IMAGES ***` injection block for a list of local image paths.
///
/// WHY extracted: Both CLI (clipboard paste via `pending_images`) and gateway platforms
/// (WhatsApp, Telegram, …) must produce the exact same block format so that
/// `VISION_GUIDANCE` in the system prompt fires consistently on every turn.
/// Gateway uses the mirror function `platform::format_image_attachment_block` in
/// `edgecrab-gateway`. Any change to the format here must be kept in sync there.
fn format_image_attachment_block(image_paths: &[&str]) -> String {
    let image_list = image_paths.join(", ");
    let count = image_paths.len();
    format!(
        "*** ATTACHED IMAGES \u{2014} ACTION REQUIRED ***\n\
         The user has attached {count} image file(s): {image_list}\n\
         You MUST call vision_analyze for EACH image before responding.\n\
         - Use tool: vision_analyze\n\
         - Parameter: image_source = <the file path above>\n\
         - DO NOT use browser_vision (that captures web pages, not local files)\n\
         - DO NOT use read_file on image paths (binary files)\n\
         *** END ATTACHED IMAGES ***"
    )
}

/// A single line in the output area with a semantic role.
#[derive(Clone)]
pub struct OutputLine {
    pub text: String,
    pub role: OutputRole,
    /// Pre-built ratatui spans (for tool-done lines with emoji).
    /// When `Some`, these are used directly in render instead of re-parsing `text`.
    pub prebuilt_spans: Option<Vec<Span<'static>>>,
    /// Cached rendered lines (invalidated when text changes).
    rendered: Option<Vec<Line<'static>>>,
}

#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum OutputRole {
    Assistant,
    Tool,
    System,
    Reasoning,
    Error,
    User,
}

/// Display state machine for the spinner/status area.
#[derive(Clone)]
enum DisplayState {
    Idle,
    Thinking {
        frame: usize,
        started: Instant,
    },
    Streaming {
        token_count: u64,
        started: Instant,
    },
    #[allow(dead_code)]
    ToolExec {
        name: String,
        args_json: String,
        frame: usize,
        started: Instant,
    },
    /// A background I/O operation is in progress (e.g. model discovery).
    /// Shows a spinner with a label in the status bar. Does NOT block user input.
    BgOp {
        label: String,
        frame: usize,
        started: Instant,
    },
    /// The agent sent a clarifying question and is waiting for the user to reply.
    ///
    /// WHY separate from Idle: when `is_processing` is true but the state is
    /// Idle, the status bar shows nothing — users think the agent hung.  This
    /// variant lets `render_status_bar` display "❓ Waiting for reply" so the
    /// interaction intent is always clear even before the user reads the question.
    WaitingForClarify,
    /// The agent is requesting risk-graduated approval before executing a command.
    ///
    /// WHY separate: mirrors `WaitingForClarify` but routes keyboard input to the
    /// approval overlay (← → navigate choices, Enter confirm) rather than the text
    /// input area.  When active, the main input is locked and keybindings are
    /// redirected here.
    WaitingForApproval {
        /// Short display label (content after truncation to ~50 chars).
        command: String,
        /// Full command string shown only when user presses 'v' (view).
        full_command: String,
        /// Currently highlighted choice index (0–3 in [Once, Session, Always, Deny]).
        selected: usize,
        /// Whether the "view" mode is active (shows full_command in overlay).
        show_full: bool,
    },
    /// The agent is requesting a secret value from the user (e.g. an API key
    /// or sudo password).
    ///
    /// WHY separate: a masked-input overlay (`•••`) must replace the normal
    /// textarea so the secret never appears in scrollback. Keybindings are
    /// intercepted and the buffer is cleared immediately after sending.
    SecretCapture {
        /// Variable name or credential label shown in the overlay title.
        var_name: String,
        /// Human-readable prompt displayed inside the overlay.
        prompt: String,
        /// Whether this is a privilege-escalation (sudo) prompt — affects colour.
        is_sudo: bool,
        /// Currently typed buffer (never stored in history or output).
        buffer: String,
    },
}

/// Result payload delivered back to the main loop via `AgentResponse::BgOp`
/// once a spawned background task completes.
enum BackgroundOpResult {
    /// Model catalog loaded — open the model selector pre-focused on current.
    ModelCatalogReady {
        models: Vec<ModelEntry>,
        current_model: String,
    },
    /// Free-form text to push to the output pane (System role).
    SystemMsg(String),
    /// Provider swap succeeded — update model name and persist config.
    ModelSwitchDone { model: String },
    /// Context compression finished — show summary message.
    CompressDone { msg: String },
}

/// Tab-completion overlay state.
struct CompletionState {
    /// (name, description) — either a command token or a subcommand token.
    candidates: Vec<(String, String)>,
    selected: usize,
    active: bool,
    /// Byte offset in the textarea where the "current token" starts.
    ///
    /// * `0`   → completing the **command token** (text before the first space);
    ///   `accept_completion` replaces the command while preserving the
    ///   argument tail.
    /// * `> 0` → completing an **argument / subcommand token** that starts at
    ///   this offset; `accept_completion` keeps `text[..arg_start]`
    ///   verbatim and replaces only the fragment that follows it.
    arg_start: usize,
}

impl CompletionState {
    /// Compute the visible window for the completion popup.
    ///
    /// The inline slash-command selector should behave like the full-screen
    /// selectors: once the highlighted row moves past the visible height, the
    /// viewport must advance so the current selection stays on-screen.
    #[allow(dead_code)]
    fn visible_window(&self, max_visible: usize) -> (usize, usize) {
        if self.candidates.is_empty() || max_visible == 0 {
            return (0, 0);
        }

        let selected = self.selected.min(self.candidates.len() - 1);
        let start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };
        let end = (start + max_visible).min(self.candidates.len());
        (start, end)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputEditorMode {
    Inline,
    ComposeInsert,
    ComposeNormal,
}

impl InputEditorMode {
    fn is_compose(self) -> bool {
        !matches!(self, Self::Inline)
    }

    fn input_title(self, prompt_symbol: &str) -> String {
        match self {
            Self::Inline => format!(" {} ", prompt_symbol.trim()),
            Self::ComposeInsert => " Compose INSERT ".to_string(),
            Self::ComposeNormal => " Compose NORMAL ".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VimPending {
    Delete,
    Change,
    Yank,
    Go,
}

/// A single model entry for the model selector overlay.
#[derive(Clone)]
struct ModelEntry {
    /// Provider/model display name (e.g. "openai/gpt-4o")
    display: String,
    /// Provider name (e.g. "openai")
    provider: String,
    /// Model name (e.g. "gpt-4o")
    model_name: String,
    /// Supplemental searchable/display text for selectors.
    detail: String,
}

impl FuzzyItem for ModelEntry {
    fn primary(&self) -> &str {
        &self.display
    }
    fn secondary(&self) -> &str {
        &self.detail
    }
    fn tag(&self) -> &str {
        &self.provider
    }
}

/// A single skill entry for the skill selector table.
#[derive(Clone)]
struct SkillEntry {
    /// Skill name (without .md extension)
    name: String,
    /// Whether the skill is a directory (true) or a single file (false)
    is_dir: bool,
    /// First-line description extracted from the file, if available
    desc: String,
}

impl FuzzyItem for SkillEntry {
    fn primary(&self) -> &str {
        &self.name
    }
    fn secondary(&self) -> &str {
        &self.desc
    }
}

/// A single entry for the session browser overlay.
/// Wraps [`edgecrab_state::SessionSummary`] with pre-formatted strings so that
/// `FuzzyItem` filtering works without re-formatting on every keystroke.
#[derive(Clone)]
struct SessionBrowserEntry {
    /// Full session ID.
    id: String,
    /// Display name: title if set, otherwise first 8 chars of ID.
    display: String,
    /// Subtitle: model + message count.
    subtitle: String,
    /// Short date string (YYYY-MM-DD) derived from `started_at`.
    date: String,
}

impl SessionBrowserEntry {
    fn from_summary(s: &edgecrab_state::SessionSummary) -> Self {
        let display = s
            .title
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or(&s.id[..s.id.len().min(12)])
            .to_string();
        let model_tag = s.model.as_deref().unwrap_or("?");
        let subtitle = format!("model={model_tag}  msgs={}", s.message_count);
        // Convert unix-epoch float to a YYYY-MM-DD string
        let date = {
            let secs = s.started_at as i64;
            // Simple manual conversion (no chrono dep needed for this format)
            // Fallback to epoch string if overflow
            if secs > 0 {
                let d = time_secs_to_date(secs);
                format!("{:04}-{:02}-{:02}", d.0, d.1, d.2)
            } else {
                String::new()
            }
        };
        Self {
            id: s.id.clone(),
            display,
            subtitle,
            date,
        }
    }
}

impl FuzzyItem for SessionBrowserEntry {
    fn primary(&self) -> &str {
        &self.display
    }
    fn secondary(&self) -> &str {
        &self.subtitle
    }
}

// ── Skin browser entry ────────────────────────────────────────────────────

#[derive(Clone)]
struct SkinEntry {
    /// Skin name (file stem, e.g. "dracula")
    name: String,
    /// Short description shown in the secondary column
    desc: String,
    /// Whether this is the currently active skin
    is_active: bool,
}

impl FuzzyItem for SkinEntry {
    fn primary(&self) -> &str {
        &self.name
    }
    fn secondary(&self) -> &str {
        &self.desc
    }
}

/// Convert a Unix-epoch timestamp (seconds) to (year, month, day).
/// Simple Gregorian calendar implementation without external crates.
fn time_secs_to_date(secs: i64) -> (i32, u32, u32) {
    // Days since 1970-01-01
    let days = secs / 86400;
    // Use proleptic Gregorian algorithm
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Main TUI application state.
pub struct App {
    /// tui-textarea multi-line input widget
    textarea: TextArea<'static>,
    /// Explicit editor mode for the input widget.
    editor_mode: InputEditorMode,
    /// Pending Vim-style operator / motion prefix in compose normal mode.
    vim_pending: Option<VimPending>,
    /// Whether the terminal reports disambiguated modifier keys (kitty/CSI-u).
    keyboard_enhancement_enabled: bool,
    /// Output lines (scrollable)
    output: Vec<OutputLine>,
    /// Scroll offset from bottom
    scroll_offset: u16,
    /// Current theme
    theme: Theme,
    /// Slash command registry
    commands: CommandRegistry,
    /// Whether the app should exit
    should_exit: bool,
    /// Status bar info
    model_name: String,
    /// Context window size for the active model (tokens). Used to render a
    /// `12.4k / 200k (9%)` watermark in the status bar so the user can see
    /// context pressure at a glance. `None` when the model is not in the catalog.
    context_window: Option<u64>,
    /// Prompt-side context tokens shown in the status bar.
    ///
    /// This excludes completion-only tokens and includes cache hits/writes so
    /// the indicator tracks compression pressure rather than response length.
    total_tokens: u64,
    session_cost: f64,
    /// Agent for LLM dispatch
    agent: Option<Arc<Agent>>,
    /// Tokio runtime handle for spawning async tasks from the sync TUI loop
    rt_handle: tokio::runtime::Handle,
    /// Channel receiver for agent responses
    response_rx: mpsc::UnboundedReceiver<AgentResponse>,
    /// Channel sender (cloned into background tasks)
    response_tx: mpsc::UnboundedSender<AgentResponse>,
    /// Monotonic counter for isolated `/background` sessions in this TUI run.
    background_task_seq: u64,
    /// Monotonic sequence for live progress updates rendered in the UI.
    progress_seq: u64,
    /// Active isolated `/background` sessions keyed by task ID.
    background_tasks_active: std::collections::HashMap<String, BackgroundTaskStatus>,
    /// Active delegated child tasks for the foreground agent turn.
    active_subagents: std::collections::HashMap<usize, ActiveSubagentStatus>,
    /// Channel for cron job completion notifications from the background ticker.
    /// The background cron task sends formatted strings; the TUI drains them in
    /// `check_responses()` and displays them as assistant-style output lines.
    cron_rx: mpsc::UnboundedReceiver<String>,
    cron_tx: mpsc::UnboundedSender<String>,
    /// Whether the agent is currently processing a prompt
    is_processing: bool,
    /// Index of the output line currently being streamed into (-1 = none)
    streaming_line: Option<usize>,
    /// Index of the current reasoning / think-mode block for this turn.
    reasoning_line: Option<usize>,
    /// Whether extended thinking should be shown in the output pane.
    show_reasoning: bool,
    /// Whether live token streaming is enabled for future turns.
    streaming_enabled: bool,
    /// Verbose mode — show tool call details
    verbose: bool,
    /// Queued prompts to run after the current one completes
    prompt_queue: Vec<String>,
    /// Display state machine (spinner animation)
    display_state: DisplayState,
    /// Tab-completion overlay
    completion: CompletionState,
    /// Input history ring buffer
    input_history: Vec<String>,
    /// Current position in history (history.len() = "new input")
    history_pos: usize,
    /// Saved input before history navigation started
    history_stash: String,
    /// Last response completion time (for latency display)
    last_response_time: Option<Instant>,
    /// All command names for completion (cached at startup)
    all_command_names: Vec<String>,
    /// Command name → description (for completion overlay)
    command_descriptions: std::collections::HashMap<String, String>,
    /// Model selector overlay (activated by `/model` with no args)
    model_selector: FuzzySelector<ModelEntry>,
    /// Vision-model selector overlay (activated by `/vision_model`)
    vision_model_selector: FuzzySelector<ModelEntry>,
    /// Skill browser overlay (activated by `/skills` with no args)
    skill_selector: FuzzySelector<SkillEntry>,
    /// Session browser overlay (activated by F5 or `/session` with no args)
    session_browser: FuzzySelector<SessionBrowserEntry>,
    /// Skin browser overlay (activated by `/skin list`)
    skin_browser: FuzzySelector<SkinEntry>,
    /// Cached skill names (without leading /) for completion suggestions
    skills_completion_names: Vec<String>,
    /// Skills currently activated for injection into agent prompts.
    /// Each entry is a skill directory name under `skills_dir()`.
    /// Typing `/skill_name` toggles membership; the full SKILL.md content
    /// is prepended (hidden from display) to the next agent prompt.
    active_skills: Vec<String>,

    // ── Scroll tracking (best-practice UX) ──────────────────────────
    /// Estimated total visual rows in the output area (updated each render)
    output_visual_rows: u16,
    /// Height of the last rendered output viewport (updated each render)
    output_area_height: u16,
    /// True when user is at the very bottom — new content triggers auto-scroll
    at_bottom: bool,

    // ── Dirty flag — avoid redundant redraws ─────────────────────────
    /// True whenever state changed and a redraw is needed
    needs_redraw: bool,
    /// When true, the event loop calls `terminal.clear()` on the next iteration
    /// to force a full repaint of every cell.  Set by `clear_output()` so that
    /// any out-of-band characters written to the alternate screen (e.g. from
    /// `tracing::warn!` firing on a background task) are erased.
    needs_full_terminal_clear: bool,

    // ── Personality / UX animation state ─────────────────────────────
    /// Index into thinking_verbs — advances each full spinner rotation
    thinking_verb_idx: usize,
    /// Index into kaomoji_thinking — advances slower than the verb
    kaomoji_frame_idx: usize,
    /// Turn count — number of completed user→agent exchanges this session
    turn_count: usize,
    /// When the agent is waiting for a clarifying answer, this holds the
    /// oneshot sender used to relay the user's next input back to the tool.
    clarify_pending_tx: Option<tokio::sync::oneshot::Sender<String>>,
    /// When the agent is waiting for an approval choice, this holds the
    /// oneshot sender used to relay the user's `ApprovalChoice` back to the tool.
    /// The visual state is in `DisplayState::WaitingForApproval`.
    approval_pending_tx: Option<tokio::sync::oneshot::Sender<edgecrab_core::ApprovalChoice>>,
    /// When the agent is waiting for a secret value (API key, sudo password, etc.),
    /// this holds the oneshot sender used to relay the typed string back.
    /// The visual state is in `DisplayState::SecretCapture`.
    /// The buffer in `SecretCapture` is zeroed immediately after sending.
    secret_pending_tx: Option<tokio::sync::oneshot::Sender<String>>,
    /// Session-level approval cache: SHA-256 of commands approved with `Session` scope.
    /// Commands in this set skip the approval dialog for the rest of the session.
    session_approvals: std::collections::HashSet<String>,
    /// True when terminal mouse capture is enabled.
    mouse_capture_enabled: bool,
    /// Deferred terminal command to enable/disable mouse capture.
    pending_mouse_capture: Option<bool>,
    /// Timestamp of the last left-click, for double-click detection in SCROLL mode.
    last_left_click: Option<Instant>,
    /// When true, agent responses are read back via TTS after each turn.
    /// Mirrors hermes-agent's voice_mode feature (`/voice on`).
    voice_mode_enabled: bool,
    /// Accumulated text of the most recent agent response (from streaming tokens).
    /// Used by voice mode to feed TTS after each turn completes.
    last_agent_response_text: String,
    /// Session-level personality overlay name (e.g. "pirate", "concise").
    /// When Some, the named preset was applied this session via `/personality <name>`.
    /// Mirrors hermes-agent's `/personality` session overlay.
    session_personality: Option<String>,
    /// Session-level skin name (e.g. "dracula", "mono").
    /// Set via `/skin <name>`; used to show the active skin in `/skin` status.
    session_skin: Option<String>,
    /// Pending image paths from clipboard paste — injected into the next prompt.
    pending_images: Vec<std::path::PathBuf>,
    /// Number of tool calls currently in-flight in this turn.
    /// Incremented on ToolExec, decremented on ToolDone.
    /// Only transitions back to Thinking when this reaches zero,
    /// correctly handling parallel tool execution.
    in_flight_tool_count: u32,
    /// Cumulative streamed token count for the current turn.
    /// Persists across multiple streaming phases (separated by tool calls)
    /// so the status bar shows the running total rather than resetting.
    turn_stream_tokens: u64,
    /// FIFO queue of output-area indices for in-flight "running" tool lines.
    ///
    /// WHY VecDeque: On ToolExec we push a cyan "··· running ···" placeholder
    /// to the output area immediately (not waiting for ToolDone) — this gives
    /// visual feedback inside the scrollable transcript during long operations.
    /// On ToolDone we pop_front and update that line in-place with the final
    /// styled duration/result, so the placeholder is replaced without a
    /// disruptive layout shift.  FIFO order matches tool-dispatch ordering and
    /// handles sequential multi-tool turns without any per-tool book-keeping.
    pending_tool_lines: std::collections::VecDeque<PendingToolLine>,
    /// File-based hook registry — loaded from ~/.edgecrab/hooks/ at startup.
    /// Receives tool:pre/post, llm:pre/post, and cli:start/end events.
    hook_registry: std::sync::Arc<edgecrab_gateway::hooks::HookRegistry>,
}

#[derive(Debug)]
struct PendingToolLine {
    line_idx: usize,
    edit_snapshot: Option<LocalEditSnapshot>,
}

/// Events from the agent background task → TUI event loop.
enum AgentResponse {
    /// A partial streamed token — append to current streaming line.
    Token(String),
    /// A non-token runtime notice to show as a system line.
    Notice(String),
    /// A reasoning / think-mode delta or full reasoning block.
    Reasoning(String),
    /// A tool execution has started — show tool name + preview in status bar.
    ToolExec { name: String, args_json: String },
    /// A tool execution completed — push a rich formatted line to the output.
    ToolDone {
        name: String,
        args_json: String,
        result_preview: Option<String>,
        duration_ms: u64,
        is_error: bool,
    },
    SubAgentStart {
        task_index: usize,
        task_count: usize,
        goal: String,
    },
    SubAgentReasoning {
        task_index: usize,
        task_count: usize,
        text: String,
    },
    SubAgentToolExec {
        task_index: usize,
        task_count: usize,
        name: String,
        args_json: String,
    },
    SubAgentFinish {
        task_index: usize,
        task_count: usize,
        status: String,
        duration_ms: u64,
        summary: String,
        api_calls: u32,
        model: Option<String>,
    },
    /// Streaming complete — mark processing done.
    Done,
    /// An error occurred.
    Error(String),
    /// The agent needs an answer from the user before it can continue.
    Clarify {
        question: String,
        /// Up to 4 predefined answer choices, or None for open-ended.
        choices: Option<Vec<String>>,
        response_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// The agent is requesting risk-graduated approval before executing a command.
    Approval {
        command: String,
        full_command: String,
        response_tx: tokio::sync::oneshot::Sender<edgecrab_core::ApprovalChoice>,
    },
    /// The agent is requesting a secret value (API key, env var, sudo password).
    SecretRequest {
        var_name: String,
        prompt: String,
        is_sudo: bool,
        response_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// A background operation (model discovery, compress, swap) completed.
    BgOp(BackgroundOpResult),
    /// An isolated `/background` session finished successfully.
    BackgroundPromptComplete {
        task_num: u64,
        task_id: String,
        prompt_preview: String,
        response: String,
    },
    /// A background session reported progress.
    BackgroundPromptProgress { task_id: String, text: String },
    /// An isolated `/background` session failed.
    BackgroundPromptFailed {
        task_num: u64,
        task_id: String,
        error: String,
    },
}

#[derive(Clone, Debug)]
struct BackgroundTaskStatus {
    preview: String,
    last_progress: Option<String>,
    last_seq: u64,
}

#[derive(Clone, Debug)]
struct ActiveSubagentStatus {
    task_index: usize,
    task_count: usize,
    goal: String,
    last_detail: Option<String>,
    last_seq: u64,
}

fn background_progress_text(task_num: u64, event: &edgecrab_core::StreamEvent) -> Option<String> {
    match event {
        edgecrab_core::StreamEvent::ToolExec { name, args_json } => Some(format!(
            "↳ bg#{task_num} {}",
            tool_status_preview(name, args_json)
        )),
        edgecrab_core::StreamEvent::SubAgentStart {
            task_index,
            task_count,
            goal,
        } => Some(format!(
            "↳ bg#{task_num} [{}/{}] delegate: {}",
            task_index + 1,
            task_count,
            edgecrab_core::safe_truncate(goal, 72)
        )),
        edgecrab_core::StreamEvent::SubAgentReasoning {
            task_index,
            task_count,
            text,
        } => Some(format!(
            "↳ bg#{task_num} [{}/{}] thinking: {}",
            task_index + 1,
            task_count,
            edgecrab_core::safe_truncate(text.trim(), 72)
        )),
        edgecrab_core::StreamEvent::SubAgentToolExec {
            task_index,
            task_count,
            name,
            args_json,
        } => Some(format!(
            "↳ bg#{task_num} [{}/{}] {}",
            task_index + 1,
            task_count,
            tool_status_preview(name, args_json)
        )),
        edgecrab_core::StreamEvent::SubAgentFinish {
            task_index,
            task_count,
            status,
            duration_ms,
            ..
        } => Some(format!(
            "↳ bg#{task_num} [{}/{}] {} in {:.1}s",
            task_index + 1,
            task_count,
            status,
            *duration_ms as f64 / 1000.0
        )),
        _ => None,
    }
}

fn format_background_status_summary(
    active: &std::collections::HashMap<String, BackgroundTaskStatus>,
) -> Option<String> {
    let current = active.values().max_by_key(|status| status.last_seq)?;
    let detail = current
        .last_progress
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .unwrap_or(&current.preview);
    Some(edgecrab_core::safe_truncate(detail, 58).to_string())
}

fn format_subagent_status_summary(
    active: &std::collections::HashMap<usize, ActiveSubagentStatus>,
) -> Option<String> {
    let current = active.values().max_by_key(|status| status.last_seq)?;
    let detail = current
        .last_detail
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .map(|text| edgecrab_core::safe_truncate(text, 52).to_string())
        .unwrap_or_else(|| edgecrab_core::safe_truncate(&current.goal, 52).to_string());
    Some(format!(
        "[{}/{}] {}",
        current.task_index + 1,
        current.task_count,
        detail
    ))
}

impl App {
    pub fn new() -> Self {
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        let (cron_tx, cron_rx) = mpsc::unbounded_channel();
        let theme = Theme::load();
        let commands = CommandRegistry::new();
        let display_preferences = load_display_preferences();

        // Collect all command names + aliases for tab completion
        let all_command_names = {
            let mut names: Vec<String> = commands
                .all_names()
                .into_iter()
                .map(|n| format!("/{n}"))
                .collect();
            names.sort();
            names.dedup();
            names
        };

        // Build name → description lookup (aliases share parent description)
        let command_descriptions = commands.all_descriptions();

        // Configure tui-textarea
        let mut textarea = TextArea::default();
        textarea.set_max_histories(512);
        textarea.set_tab_length(4);
        textarea.set_cursor_line_style(Style::default());
        textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.input_border)
                .title(format!(" {} ", theme.prompt_symbol.trim())),
        );
        textarea.set_style(theme.input_text);
        textarea.set_placeholder_text("Type a message or /help for commands...");
        textarea.set_placeholder_style(
            Style::default()
                .fg(Color::Rgb(100, 100, 100))
                .add_modifier(Modifier::ITALIC),
        );

        let mut app = Self {
            textarea,
            editor_mode: InputEditorMode::Inline,
            vim_pending: None,
            keyboard_enhancement_enabled: false,
            output: Vec::new(),
            scroll_offset: 0,
            theme,
            commands,
            should_exit: false,
            model_name: "none".into(),
            context_window: None,
            total_tokens: 0,
            session_cost: 0.0,
            agent: None,
            rt_handle: tokio::runtime::Handle::current(),
            response_rx,
            response_tx,
            background_task_seq: 0,
            progress_seq: 0,
            background_tasks_active: std::collections::HashMap::new(),
            active_subagents: std::collections::HashMap::new(),
            cron_rx,
            cron_tx,
            is_processing: false,
            streaming_line: None,
            reasoning_line: None,
            show_reasoning: display_preferences.show_reasoning,
            streaming_enabled: display_preferences.streaming_enabled,
            verbose: false,
            prompt_queue: Vec::new(),
            display_state: DisplayState::Idle,
            completion: CompletionState {
                candidates: Vec::new(),
                selected: 0,
                active: false,
                arg_start: 0,
            },
            input_history: Vec::new(),
            history_pos: 0,
            history_stash: String::new(),
            last_response_time: None,
            all_command_names,
            command_descriptions,
            model_selector: {
                let mut ms: FuzzySelector<ModelEntry> = FuzzySelector::new();
                ms.set_items(
                    ModelCatalog::flat_catalog()
                        .into_iter()
                        .map(|(display, provider, model_name)| ModelEntry {
                            detail: model_name.clone(),
                            display,
                            provider,
                            model_name,
                        })
                        .collect(),
                );
                ms
            },
            vision_model_selector: FuzzySelector::new(),
            skill_selector: FuzzySelector::new(),
            session_browser: FuzzySelector::new(),
            skin_browser: FuzzySelector::new(),
            skills_completion_names: Vec::new(),
            active_skills: Vec::new(),
            output_visual_rows: 0,
            output_area_height: 24,
            at_bottom: true,
            needs_redraw: true,
            needs_full_terminal_clear: false,
            thinking_verb_idx: 0,
            kaomoji_frame_idx: 0,
            turn_count: 0,
            clarify_pending_tx: None,
            approval_pending_tx: None,
            secret_pending_tx: None,
            session_approvals: std::collections::HashSet::new(),
            mouse_capture_enabled: true, // scroll wheel on by default; F6 to switch
            pending_mouse_capture: None,
            last_left_click: None,
            voice_mode_enabled: false,
            last_agent_response_text: String::new(),
            session_personality: None,
            session_skin: None,
            pending_images: Vec::new(),
            in_flight_tool_count: 0,
            turn_stream_tokens: 0,
            pending_tool_lines: std::collections::VecDeque::new(),
            hook_registry: {
                let mut r = edgecrab_gateway::hooks::HookRegistry::new();
                r.discover_and_load();
                std::sync::Arc::new(r)
            },
        };

        app.apply_textarea_editor_style();

        // Load persisted command history from ~/.edgecrab/history
        app.load_history_file();

        // Pre-load skills list for completion overlay
        app.refresh_skills_list();

        app
    }

    /// Clone the cron notification sender for use by the background cron ticker.
    ///
    /// The background ticker sends formatted completion messages through this
    /// channel. The TUI drains it in `check_responses()` on every event-loop tick.
    pub fn cron_sender(&self) -> mpsc::UnboundedSender<String> {
        self.cron_tx.clone()
    }

    /// Set the agent for LLM dispatch.
    pub fn set_agent(&mut self, agent: Arc<Agent>) {
        self.agent = Some(agent);
    }

    /// Get a reference to the agent, or push an error and return None.
    fn require_agent(&mut self) -> Option<Arc<Agent>> {
        match self.agent.clone() {
            Some(a) => Some(a),
            None => {
                self.push_output("No agent configured.", OutputRole::Error);
                None
            }
        }
    }

    /// Blocking snapshot from agent session state.
    fn agent_snapshot(&self, agent: &Agent) -> edgecrab_core::SessionSnapshot {
        self.rt_handle
            .block_on(async { agent.session_snapshot().await })
    }

    /// Set model name for status bar display.
    pub fn set_model(&mut self, model: &str) {
        self.model_name = model.to_string();
        self.update_context_window();
    }

    /// Refresh `self.context_window` from the model catalog for the current model.
    ///
    /// Model names are in the format `provider/model` (e.g. `anthropic/claude-opus-4`).
    /// When the model is not found in the catalog, context_window is set to None.
    fn update_context_window(&mut self) {
        self.context_window = self
            .model_name
            .split_once('/')
            .and_then(|(provider, model)| ModelCatalog::context_window(provider, model));
    }

    /// Replace the visible transcript with a persisted message history.
    pub fn load_messages(&mut self, messages: Vec<edgecrab_types::Message>) {
        self.clear_output();
        for message in messages {
            match message.role {
                edgecrab_types::Role::System => {
                    self.push_output(message.text_content(), OutputRole::System);
                }
                edgecrab_types::Role::User => {
                    self.push_output(format!("> {}", message.text_content()), OutputRole::User);
                }
                edgecrab_types::Role::Assistant => {
                    if self.show_reasoning {
                        if let Some(reasoning) = message.reasoning.clone() {
                            if !reasoning.trim().is_empty() {
                                self.push_output(
                                    format!("🧠 Thinking\n{reasoning}"),
                                    OutputRole::Reasoning,
                                );
                            }
                        }
                    }
                    self.push_output(message.text_content(), OutputRole::Assistant);
                }
                edgecrab_types::Role::Tool => {
                    self.push_output(message.text_content(), OutputRole::Tool);
                }
            }
        }
    }

    /// Add a line to the output area.
    pub fn push_output(&mut self, text: impl Into<String>, role: OutputRole) {
        self.output.push(OutputLine {
            text: text.into(),
            role,
            prebuilt_spans: None,
            rendered: None,
        });
        // Only auto-scroll to bottom if the user was already at bottom.
        // Preserves scroll position when user has scrolled up to read history.
        if self.at_bottom {
            self.scroll_offset = 0;
        }
        self.needs_redraw = true;
    }

    /// Push a pre-built span line (for tool-done lines with emoji).
    /// Ratatui renders each Span with correct unicode column width,
    /// so emoji/wide characters align perfectly without format-string padding tricks.
    pub fn push_output_spans(&mut self, spans: Vec<Span<'static>>, role: OutputRole) {
        self.output.push(OutputLine {
            text: String::new(),
            role,
            prebuilt_spans: Some(spans),
            rendered: None,
        });
        if self.at_bottom {
            self.scroll_offset = 0;
        }
        self.needs_redraw = true;
    }

    /// Clear the output area.
    pub fn clear_output(&mut self) {
        self.output.clear();
        self.scroll_offset = 0;
        self.at_bottom = true;
        self.output_visual_rows = 0;
        // Reset streaming cursors so any in-flight agent events are handled
        // correctly: stale indices into the now-empty output vec would cause
        // tokens to be silently dropped or appended at wrong positions.
        self.streaming_line = None;
        self.reasoning_line = None;
        self.pending_tool_lines.clear();
        // Request a full terminal repaint.  ratatui's diff-based renderer
        // normally skips unchanged cells; if any out-of-band bytes reached the
        // alternate screen (e.g. tracing::warn! from a background task) those
        // cells are only erased by a full clear → next draw writes every cell.
        self.needs_full_terminal_clear = true;
        self.needs_redraw = true;
    }

    /// Push a borderless gradient welcome banner into the output area.
    ///
    /// # Design rationale — no box drawing
    ///
    /// Box-drawing characters (╔═╗╚╝║) are visually heavy and interact poorly
    /// with emoji content that follows — they set a rigid "formal" tone that
    /// clashes with the rest of the TUI.  Instead we use a lightweight wordmark
    /// layout:
    ///
    /// ```text
    ///   🦀  EdgeCrab           ·  AI-native terminal agent
    ///   ────────────────────────────────────────────────────
    ///      Model: claude-opus-4
    ///      Type a message or /help for commands…
    /// ```
    ///
    /// # Emoji-safe layout rules
    ///
    /// Wide chars (emoji, katakana, CJK) are **2 display columns** wide.  All
    /// column arithmetic uses `.width()` (UnicodeWidthStr) and
    /// `unicode_pad_right()`, never `.len()` or `format!("{:<n}")`.  Emoji are
    /// isolated in their own `Span` so ratatui measures each segment
    /// independently.
    pub fn push_colorful_banner(&mut self, model: &str) {
        let agent_name = self.theme.agent_name.clone();
        let welcome_msg = self.theme.welcome_msg.clone();

        // ── Palette ────────────────────────────────────────────────────────────
        let crab_style = Style::default()
            .fg(Color::Rgb(255, 160, 40))
            .add_modifier(Modifier::BOLD); // vivid copper
        let name_style = Style::default()
            .fg(Color::Rgb(255, 215, 0))
            .add_modifier(Modifier::BOLD); // gold
        let dot_style = Style::default().fg(Color::Rgb(100, 100, 120)); // dim separator
        let tagline_style = Style::default()
            .fg(Color::Rgb(184, 134, 11))
            .add_modifier(Modifier::DIM); // dark gold
        let rule_style = Style::default()
            .fg(Color::Rgb(70, 60, 40))
            .add_modifier(Modifier::DIM); // very dim amber
        let label_style = Style::default()
            .fg(Color::Rgb(140, 140, 155))
            .add_modifier(Modifier::DIM); // muted
        let value_style = Style::default().fg(Color::Rgb(255, 191, 0)); // amber
        let hint_style = Style::default()
            .fg(Color::Rgb(120, 120, 135))
            .add_modifier(Modifier::DIM); // dim hint

        // ── Row 0: blank breathing room ────────────────────────────────────────
        self.push_output_spans(vec![Span::raw("")], OutputRole::System);

        // ── Row 1: wordmark  🦀  Name  ·  tagline ─────────────────────────────
        //
        // Layout:  "  🦀 " (5) + name_padded (18) + " · " (3) + tagline
        // All widths measured with .width(); emoji in own Span.
        let name_cell = unicode_pad_right(&agent_name, 18);
        let tagline = "AI-native terminal agent";
        self.push_output_spans(
            vec![
                Span::styled("  ", Style::default()),
                // 🦀 = 2 display cols; isolated so ratatui measures it cleanly.
                Span::styled("🦀 ", crab_style),
                Span::styled(name_cell, name_style),
                Span::styled(" · ", dot_style),
                Span::styled(tagline.to_string(), tagline_style),
            ],
            OutputRole::System,
        );

        // ── Row 2: thin rule (no box chars — just a repeated ─) ────────────────
        //
        // 52 cols: matches the visual span of the wordmark line above.
        // "─" is U+2500 (box-drawing); it degrades gracefully on narrow fonts.
        let rule = "─".repeat(52);
        self.push_output_spans(
            vec![Span::styled(format!("  {rule}"), rule_style)],
            OutputRole::System,
        );

        // ── Row 3: model ────────────────────────────────────────────────────────
        let model_display = unicode_trunc(model, 55);
        self.push_output_spans(
            vec![
                Span::styled("     ", Style::default()),
                Span::styled("Model  ", label_style),
                Span::styled(model_display, value_style),
            ],
            OutputRole::System,
        );

        // ── Row 4: tools & skills counts ────────────────────────────────────────
        // Use block_in_place (safe inside spawn_blocking) instead of block_on
        // which panics when called from within a tokio runtime thread.
        let tool_count = if let Some(agent) = self.agent.clone() {
            tokio::task::block_in_place(|| {
                self.rt_handle
                    .block_on(async move { agent.tool_names().await })
            })
            .len()
        } else {
            0
        };
        let skill_count = self.skills_completion_names.len();
        let count_style = Style::default()
            .fg(Color::Rgb(160, 200, 160))
            .add_modifier(Modifier::DIM);
        self.push_output_spans(
            vec![
                Span::styled("     ", Style::default()),
                Span::styled(
                    format!("tools: {tool_count}   skills: {skill_count}"),
                    count_style,
                ),
            ],
            OutputRole::System,
        );

        // ── Row 5: welcome hint ─────────────────────────────────────────────────
        self.push_output_spans(
            vec![
                Span::styled("     ", Style::default()),
                Span::styled(welcome_msg, hint_style),
            ],
            OutputRole::System,
        );

        // ── Row 6: blank breathing room ────────────────────────────────────────
        self.push_output_spans(vec![Span::raw("")], OutputRole::System);

        // ── Async update check ──────────────────────────────────────────────────
        // Fire-and-forget: check if there are upstream commits available.
        // Result arrives as a SystemMsg only when updates are found.
        let tx = self.response_tx.clone();
        let home_dir = edgecrab_core::edgecrab_home();
        self.rt_handle.spawn(async move {
            let output = tokio::process::Command::new("git")
                .args(["-C", &home_dir.to_string_lossy(), "fetch", "--quiet"])
                .output()
                .await;
            if output.is_err() {
                return; // git not available or not a git repo — silent
            }
            let count_out = tokio::process::Command::new("git")
                .args([
                    "-C",
                    &home_dir.to_string_lossy(),
                    "rev-list",
                    "HEAD..origin/main",
                    "--count",
                ])
                .output()
                .await;
            if let Ok(out) = count_out {
                let count_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Ok(n) = count_str.parse::<u64>() {
                    if n > 0 {
                        let msg = format!(
                            "  💡 {n} update{s} available — run `git -C ~/.edgecrab pull` to upgrade",
                            s = if n == 1 { "" } else { "s" }
                        );
                        let _ = tx.send(AgentResponse::BgOp(BackgroundOpResult::SystemMsg(msg)));
                    }
                }
            }
        });
    }

    /// Push the goodbye message from the current skin into the output.
    #[allow(dead_code)]
    pub fn push_goodbye(&mut self) {
        let msg = self.theme.goodbye_msg.clone();
        self.push_output_spans(
            vec![Span::styled(
                format!("  {msg}"),
                Style::default()
                    .fg(Color::Rgb(184, 134, 11))
                    .add_modifier(Modifier::DIM),
            )],
            OutputRole::System,
        );
    }

    /// Returns true if the app should exit.
    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    /// Get current text from textarea as a single string.
    fn textarea_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    fn apply_textarea_editor_style(&mut self) {
        match self.editor_mode {
            InputEditorMode::Inline => {
                self.textarea.set_cursor_line_style(Style::default());
                self.textarea.set_cursor_style(
                    Style::default()
                        .fg(Color::Rgb(255, 248, 220))
                        .add_modifier(Modifier::REVERSED),
                );
                self.textarea.remove_line_number();
            }
            InputEditorMode::ComposeInsert => {
                self.textarea
                    .set_cursor_line_style(Style::default().bg(Color::Rgb(34, 38, 48)));
                self.textarea.set_cursor_style(
                    Style::default()
                        .fg(Color::Rgb(100, 230, 160))
                        .add_modifier(Modifier::REVERSED | Modifier::BOLD),
                );
                self.textarea
                    .set_line_number_style(Style::default().fg(Color::Rgb(85, 95, 110)));
            }
            InputEditorMode::ComposeNormal => {
                self.textarea
                    .set_cursor_line_style(Style::default().bg(Color::Rgb(45, 34, 18)));
                self.textarea.set_cursor_style(
                    Style::default()
                        .fg(Color::Rgb(255, 191, 0))
                        .add_modifier(Modifier::REVERSED | Modifier::BOLD),
                );
                self.textarea
                    .set_line_number_style(Style::default().fg(Color::Rgb(120, 105, 70)));
            }
        }
    }

    fn fresh_textarea(&self) -> TextArea<'static> {
        let mut fresh = TextArea::default();
        fresh.set_max_histories(512);
        fresh.set_tab_length(4);
        fresh.set_style(self.theme.input_text);
        fresh.set_placeholder_text("Type a message or /help for commands...");
        fresh.set_placeholder_style(
            Style::default()
                .fg(Color::Rgb(100, 100, 100))
                .add_modifier(Modifier::ITALIC),
        );
        fresh.set_block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(self.theme.input_border)
                .title(self.editor_mode.input_title(&self.theme.prompt_symbol)),
        );
        fresh
    }

    fn set_editor_mode(&mut self, mode: InputEditorMode) {
        self.editor_mode = mode;
        self.vim_pending = None;
        self.apply_textarea_editor_style();
        self.needs_redraw = true;
    }

    fn set_keyboard_enhancement_enabled(&mut self, enabled: bool) {
        self.keyboard_enhancement_enabled = enabled;
        self.needs_redraw = true;
    }

    fn inline_compose_hint(&self) -> &'static str {
        if self.keyboard_enhancement_enabled {
            "Shift+Enter=compose"
        } else {
            "Ctrl+J=compose"
        }
    }

    fn enter_compose_insert(&mut self) {
        if !matches!(self.editor_mode, InputEditorMode::ComposeInsert) {
            self.set_editor_mode(InputEditorMode::ComposeInsert);
        } else {
            self.vim_pending = None;
        }
    }

    fn enter_compose_normal(&mut self) {
        self.set_editor_mode(InputEditorMode::ComposeNormal);
    }

    fn exit_compose_mode(&mut self) {
        self.set_editor_mode(InputEditorMode::Inline);
    }

    fn submit_current_input(&mut self) {
        let input = self.textarea_text().trim().to_string();
        if !input.is_empty() {
            self.push_history(&input);
            self.process_input(&input);
            self.scroll_offset = 0;
            self.at_bottom = true;
        }
        self.exit_compose_mode();
        self.textarea_clear();
        self.completion.active = false;
    }

    /// Clear the textarea.
    fn textarea_clear(&mut self) {
        self.textarea = self.fresh_textarea();
        self.apply_textarea_editor_style();
    }

    /// Clear the textarea and set it to `text`.
    ///
    /// Single responsibility helper that eliminates the repeated
    /// `textarea_clear()` + `for ch in … { insert_char(ch) }` pattern that
    /// would otherwise appear at every call site.
    fn textarea_set_text(&mut self, text: &str) {
        self.textarea_clear();
        if !text.is_empty() {
            self.textarea.insert_str(text);
        }
    }

    /// Dispatch a prompt received before TUI starts (e.g. from CLI args).
    pub fn dispatch_initial_prompt(&mut self, prompt: String) {
        self.push_output(format!("> {}", prompt), OutputRole::User);
        self.process_input(&prompt);
    }

    // ─── Skills management helpers ──────────────────────────────────

    /// Return the path to the skills directory.
    ///
    /// Uses `~/.edgecrab/skills/` — the canonical skills location, matching
    /// the skills tools which use `ctx.config.edgecrab_home.join("skills")`.
    fn skills_dir() -> std::path::PathBuf {
        edgecrab_core::edgecrab_home().join("skills")
    }

    /// Find a skill's SKILL.md by name, searching recursively through category
    /// subdirectories.  Returns the path to SKILL.md, or None.
    ///
    /// 1. Direct flat lookup: `skills/<name>/SKILL.md`
    /// 2. Recursive search: find a directory whose leaf name matches `name`
    fn find_skill_md(name: &str) -> Option<std::path::PathBuf> {
        let skills_dir = Self::skills_dir();

        // 1. Direct flat lookup
        let direct = skills_dir.join(name).join("SKILL.md");
        if direct.is_file() {
            return Some(direct);
        }

        // 2. Recursive search by leaf directory name
        let mut stack = vec![skills_dir];
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(leaf) = path.file_name().and_then(|n| n.to_str()) {
                        if leaf == name {
                            let md = path.join("SKILL.md");
                            if md.is_file() {
                                return Some(md);
                            }
                        }
                    }
                    stack.push(path);
                }
            }
        }

        None
    }

    /// Parse a SKILL.md file and return the frontmatter `description:` value
    /// (truncated to 80 chars for the selector column).
    fn read_skill_desc(path: &std::path::Path) -> String {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        edgecrab_core::extract_skill_description(&content)
            .map(|d| unicode_trunc(&d, 80))
            .unwrap_or_default()
    }

    /// Reload the skills list from disk into `skill_selector` and
    /// `skills_completion_names`.  Called on startup and when the overlay opens.
    ///
    /// Recursively scans all directories under `skills_dir()` that contain a
    /// `SKILL.md` file, including category subdirectories (e.g.
    /// `skills/media/gif-search/SKILL.md`).  Uses the canonical frontmatter
    /// parser from `edgecrab-core` to extract names and descriptions.
    fn refresh_skills_list(&mut self) {
        let dir = Self::skills_dir();
        let mut entries: Vec<SkillEntry> = Vec::new();

        // Recursive scan: walk all subdirectories to find SKILL.md files
        let mut stack = vec![dir.clone()];
        while let Some(current) = stack.pop() {
            let read_dir = match std::fs::read_dir(&current) {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            for res in read_dir.flatten() {
                let path = res.path();
                if !path.is_dir() {
                    continue;
                }
                // Skip hidden/system dirs
                let dir_name = res.file_name().to_string_lossy().to_string();
                if dir_name.starts_with('.') {
                    continue;
                }
                let skill_md = path.join("SKILL.md");
                if skill_md.is_file() {
                    // This is a skill directory — use leaf dir name
                    let desc = Self::read_skill_desc(&skill_md);
                    entries.push(SkillEntry {
                        name: dir_name,
                        is_dir: true,
                        desc,
                    });
                } else {
                    // Not a skill — might be a category dir, recurse
                    stack.push(path);
                }
            }
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));

        // Update completion names cache (plain names, no slash prefix)
        self.skills_completion_names = entries.iter().map(|e| e.name.clone()).collect();

        // Reload selector state
        self.skill_selector.set_items(entries);
    }

    // ─── Tab Completion ─────────────────────────────────────────────

    /// Static table of known argument / subcommand completions per command.
    ///
    /// **Design principles**
    /// - Completions match the *canonical* command name **and** all aliases  
    ///   so `/session`, `/sessions` and `/sess` (if it were an alias) all work.
    /// - Each entry is `(token, short_description)` shown in the overlay.
    /// - Commands that accept only free-form text (e.g. `/model <name>`,
    ///   `/save <path>`) return an empty slice so Tab falls through to the
    ///   history ghost-hint.
    /// - Single-char aliases (`/r`, `/u`, `/v`, …) are mapped to their
    ///   canonical names so Tab works regardless of which form the user typed.
    fn command_arg_hints(cmd_token: &str) -> &'static [(&'static str, &'static str)] {
        match cmd_token {
            // ── Session management ─────────────────────────────────────────────
            "session" | "sessions" => &[
                ("list", "List all saved sessions"),
                ("new", "Start a fresh session"),
                ("switch", "Activate a session: switch <id-prefix>"),
                ("delete", "Delete a session: delete <id-prefix>"),
                ("rename", "Rename: rename <id-prefix> <new title>"),
                ("prune", "Remove sessions older than N days (default 90)"),
            ],
            // ── Model / reasoning ──────────────────────────────────────────────
            "reasoning" | "think" => &[
                ("low", "Minimal reasoning — fastest, cheapest"),
                ("medium", "Balanced reasoning effort"),
                ("high", "Maximum reasoning depth — slowest"),
                ("show", "Display reasoning steps in the output pane"),
                ("hide", "Suppress reasoning steps from the output pane"),
                ("status", "Show the current think-mode state"),
            ],
            "stream" | "streaming" => &[
                ("on", "Enable live token streaming"),
                ("off", "Show the final answer only after completion"),
                ("toggle", "Flip the current streaming mode"),
                ("status", "Show the current streaming mode"),
            ],
            // ── Voice / TTS ────────────────────────────────────────────────────
            "voice" | "tts" => &[
                ("on", "Enable TTS — agent responses are read aloud"),
                ("off", "Disable TTS"),
                ("status", "Show current voice mode"),
            ],
            // ── Mouse capture ──────────────────────────────────────────────────
            "mouse" => &[
                (
                    "on",
                    "Enable mouse capture (scroll wheel; disables native text selection)",
                ),
                ("off", "Disable mouse capture (free drag-to-copy; default)"),
                ("toggle", "Toggle current mouse mode"),
                ("status", "Show current mouse mode"),
            ],
            // ── Browser CDP ───────────────────────────────────────────────────
            "browser" => &[
                ("connect", "Connect to a running Chrome/Chromium instance"),
                ("disconnect", "Close the CDP connection"),
                ("status", "Show browser connection status"),
                ("tabs", "List open browser tabs"),
                ("recording", "Toggle recording: recording on | off"),
            ],
            // ── Cron scheduler ────────────────────────────────────────────────
            "cron" | "schedule" => &[
                ("list", "List all scheduled jobs"),
                ("add", "Add a job: add <cron-expr> <prompt>"),
                ("remove", "Remove a job: remove <id>"),
                ("run", "Run a job immediately: run <id>"),
                ("pause", "Pause a job: pause <id>"),
                ("resume", "Resume a paused job: resume <id>"),
                ("status", "Show scheduler status"),
            ],
            // ── MCP token management ──────────────────────────────────────────
            "mcp-token" => &[
                ("set", "Store a token: set <server-id> <token>"),
                ("remove", "Delete a token: remove <server-id>"),
                ("list", "List stored server tokens"),
            ],
            // ── Personality ───────────────────────────────────────────────────
            "personality" | "persona" => &[],
            // ── Appearance / skin ─────────────────────────────────────────────
            "theme" | "skin" => &[
                ("default", "Default (system) skin"),
                ("dracula", "Dracula dark theme"),
                ("solarized", "Solarized (light-friendly)"),
                ("nord", "Nord arctic theme"),
                ("monokai", "Monokai classic"),
                ("gruvbox", "Gruvbox retro theme"),
            ],
            // ── Models live discovery ─────────────────────────────────────────
            "models" => &[
                ("refresh", "Force-refresh model list from providers"),
                ("openai", "List OpenAI models"),
                ("anthropic", "List Anthropic models"),
                ("google", "List Google models"),
                ("copilot", "List GitHub Copilot models"),
                ("ollama", "List locally running Ollama models"),
            ],
            "vision_model" | "vision-model" => &[
                ("status", "Show the current vision-routing policy"),
                ("auto", "Use the current chat model when vision-capable"),
                ("openai/gpt-4o", "Route image analysis to a dedicated model"),
                ("copilot/gpt-5.4", "Use Copilot's multimodal backend"),
            ],
            // All other commands accept free-form arguments; fall through.
            _ => &[],
        }
    }

    /// Update completion candidates based on current input.
    ///
    /// **Two-level completion (first-principles design)**
    ///
    /// The input is parsed into two regions:
    ///
    /// ```text
    ///  /session        sw
    ///  └── cmd token ─┘└─ arg fragment ─┘
    ///  (byte 0)        (byte arg_start)
    /// ```
    ///
    /// *Argument context* (`text` contains a whitespace after the `/cmd`):
    ///   Completions are drawn from the static `command_arg_hints` table.
    ///   Only the arg fragment is matched; the cmd prefix is preserved verbatim.
    ///
    /// *Command context* (no whitespace yet):
    ///   Completions are drawn from the full command/skill name list.  Both
    ///   prefix-match and fuzzy-match (jaro-winkler) are used.
    fn update_completion(&mut self) {
        let text = self.textarea_text();
        if !text.starts_with('/') || text.contains('\n') {
            self.completion.active = false;
            return;
        }

        // ─── Argument / subcommand context ────────────────────────────────────
        if let Some(sp) = text.find(char::is_whitespace) {
            // cmd_token: "session" from "/session sw"
            let cmd_token = &text[1..sp];

            // Compute where the arg fragment starts (skip leading spaces after cmd).
            let after = &text[sp..]; // e.g. "  sw"
            let trimmed = after.trim_start(); // e.g. "sw"
            let leading_spaces = after.len() - trimmed.len();
            let arg_start = sp + leading_spaces;

            // If the fragment itself contains whitespace we are past the
            // first argument \u2014 no further static completions.
            if trimmed.contains(char::is_whitespace) {
                self.completion.active = false;
                return;
            }

            let hints: Vec<(String, String)> = if matches!(cmd_token, "personality" | "persona") {
                self.personality_arg_hints()
            } else {
                Self::command_arg_hints(cmd_token)
                    .iter()
                    .map(|(sub, desc)| ((*sub).to_string(), (*desc).to_string()))
                    .collect()
            };
            if hints.is_empty() {
                // If cmd_token is already an exact known command, free-form args follow —
                // nothing to complete.  But if it is only a *partial* token (e.g. "hel"
                // typed as "/hel some-query") we fall through to command-token completion
                // so the user can complete the command name while preserving the arg tail.
                let full_cmd = format!("/{cmd_token}");
                if self.all_command_names.contains(&full_cmd) {
                    self.completion.active = false;
                    return;
                }
                // Partial command with arg tail — fall through to command-token completion.
                // The command-token block below uses `text.trim()` as prefix which would be
                // the full input.  Instead we supply just the partial command prefix via
                // a `cmd_prefix_override`, avoiding the whitespace.
                let partial_prefix = format!("/{cmd_token}");
                self.completion.arg_start = 0; // accept_completion will preserve arg tail

                let desc_for = |name: &str| -> String {
                    self.command_descriptions
                        .get(name)
                        .cloned()
                        .unwrap_or_default()
                };
                let mut candidates: Vec<(String, String)> = self
                    .all_command_names
                    .iter()
                    .filter(|cmd| cmd.starts_with(&partial_prefix) && *cmd != &partial_prefix)
                    .map(|cmd| (cmd.clone(), desc_for(cmd)))
                    .collect();
                if candidates.is_empty() && partial_prefix.len() >= 2 {
                    let mut scored: Vec<(String, f64)> = self
                        .all_command_names
                        .iter()
                        .map(|cmd| (cmd.clone(), strsim::jaro_winkler(&partial_prefix, cmd)))
                        .filter(|(_, score)| *score > 0.7)
                        .collect();
                    scored
                        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    candidates = scored
                        .into_iter()
                        .take(8)
                        .map(|(cmd, _)| (cmd.clone(), desc_for(&cmd)))
                        .collect();
                }
                if candidates.is_empty() {
                    self.completion.active = false;
                } else {
                    self.completion.candidates = candidates;
                    self.completion.selected = 0;
                    self.completion.active = true;
                }
                return;
            }

            // hints is non-empty — offer subcommand/arg completions.

            // Filter: prefix-match, then fuzzy fallback for typos.
            let mut candidates: Vec<(String, String)> = hints
                .iter()
                .filter(|(sub, _)| sub.starts_with(trimmed))
                .map(|(sub, desc)| (sub.clone(), desc.clone()))
                .collect();

            // Fuzzy fallback: if prefix match yielded nothing and user typed ≥2 chars.
            if candidates.is_empty() && trimmed.len() >= 2 {
                let mut scored: Vec<(String, f64)> = hints
                    .iter()
                    .map(|(sub, _)| (sub.clone(), strsim::jaro_winkler(trimmed, sub)))
                    .filter(|(_, score)| *score > 0.65)
                    .collect();
                scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                candidates = scored
                    .into_iter()
                    .map(|(sub, _)| {
                        let d = hints
                            .iter()
                            .find(|(s, _)| *s == sub)
                            .map(|(_, d)| d.clone())
                            .unwrap_or_default();
                        (sub, d)
                    })
                    .collect();
            }

            // Suppress when the only candidate is an exact match of what was
            // already typed (nothing new to offer).
            let exact_only =
                candidates.len() == 1 && !trimmed.is_empty() && candidates[0].0 == trimmed;

            if candidates.is_empty() || exact_only {
                self.completion.active = false;
                return;
            }

            self.completion.candidates = candidates;
            self.completion.selected = 0;
            self.completion.arg_start = arg_start;
            self.completion.active = true;
            return;
        }

        // ─── Command token context ────────────────────────────────────────────
        self.completion.arg_start = 0;
        let prefix = text.trim(); // e.g. "/hel"

        let desc_for = |name: &str| -> String {
            self.command_descriptions
                .get(name)
                .cloned()
                .unwrap_or_default()
        };

        // Prefix match (fast path).
        let mut candidates: Vec<(String, String)> = self
            .all_command_names
            .iter()
            .filter(|cmd| cmd.starts_with(prefix) && *cmd != prefix)
            .map(|cmd| (cmd.clone(), desc_for(cmd)))
            .collect();

        // Fuzzy fallback for typos (jaro-winkler > 0.70).
        if candidates.is_empty() && prefix.len() >= 2 {
            let mut scored: Vec<(String, f64)> = self
                .all_command_names
                .iter()
                .map(|cmd| (cmd.clone(), strsim::jaro_winkler(prefix, cmd)))
                .filter(|(_, score)| *score > 0.7)
                .collect();
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            candidates = scored
                .into_iter()
                .take(8)
                .map(|(cmd, _)| {
                    let d = desc_for(&cmd);
                    (cmd, d)
                })
                .collect();
        }

        // Skill candidates \u2014 visually distinct with \ud83d\udcda prefix in description.
        let query_after_slash = prefix.trim_start_matches('/').to_lowercase();
        let skill_candidates: Vec<(String, String)> = self
            .skills_completion_names
            .iter()
            .filter(|sn| {
                let sn_l = sn.to_lowercase();
                if query_after_slash.is_empty() {
                    return false;
                }
                sn_l.starts_with(&query_after_slash)
                    || (query_after_slash.len() >= 2
                        && strsim::jaro_winkler(&query_after_slash, &sn_l) > 0.72)
            })
            .map(|sn| {
                let desc = self
                    .skill_selector
                    .items
                    .iter()
                    .find(|e| &e.name == sn)
                    .map(|e| {
                        let type_tag = if e.is_dir { "dir" } else { "md" };
                        if e.desc.is_empty() {
                            format!("📚 skill [{type_tag}]")
                        } else {
                            format!("📚 {}", unicode_trunc(&e.desc, 50))
                        }
                    })
                    .unwrap_or_else(|| "📚 skill".to_string());
                (format!("/{sn}"), desc)
            })
            .take(6)
            .collect();

        let existing_names: std::collections::HashSet<String> =
            candidates.iter().map(|(c, _)| c.clone()).collect();
        for sc in skill_candidates {
            if !existing_names.contains(&sc.0) {
                candidates.push(sc);
            }
        }

        if candidates.is_empty() {
            self.completion.active = false;
        } else {
            self.completion.candidates = candidates;
            self.completion.selected = 0;
            self.completion.active = true;
        }
    }

    /// Accept the currently selected completion candidate.
    ///
    /// **Invariant: no typed content is ever discarded.**
    ///
    /// *Command context* (`arg_start == 0`):
    ///   The command token is replaced; any argument tail that follows the
    ///   first whitespace is re-inserted verbatim.
    ///   `/mo op` + accept `/model` → `/model op`
    ///
    /// *Argument context* (`arg_start > 0`):
    ///   The text up to `arg_start` (the command + space prefix) is kept;
    ///   only the fragment at `arg_start..` is replaced.
    ///   `/session sw` + accept `switch` → `/session switch `
    fn accept_completion(&mut self) {
        if !self.completion.active || self.completion.candidates.is_empty() {
            return;
        }
        let (selected, _desc) = self.completion.candidates[self.completion.selected].clone();
        let current = self.textarea_text();

        if self.completion.arg_start > 0 {
            // Argument completion: keep the prefix (command + spaces), replace
            // the fragment.  `arg_start` is a validated byte offset into ASCII
            // command names so direct byte slicing is safe.
            let prefix_text = current[..self.completion.arg_start].to_string();
            self.textarea_set_text(&format!("{}{} ", prefix_text, selected));
        } else {
            // Command completion: replace the command token, preserve arg tail.
            let arg_tail: String = if let Some(sp) = current.find(char::is_whitespace) {
                current[sp..].to_string()
            } else {
                " ".to_string()
            };
            self.textarea_set_text(&format!("{}{}", selected, arg_tail));
        }
        self.completion.active = false;
    }

    /// Returns just the next whitespace-delimited word from the ghost hint.
    ///
    /// Used by Alt+Right to accept the suggestion one word at a time instead of
    /// all at once.  Leading whitespace in the hint is included so that word
    /// boundaries are preserved naturally.
    fn ghost_hint_next_word(&self) -> Option<String> {
        let hint = self.ghost_hint()?;
        if hint.is_empty() {
            return None;
        }
        // Collect chars so we can index safely over multi-byte content.
        let chars: Vec<char> = hint.chars().collect();
        let mut end = 0;
        // Consume any leading whitespace (e.g. the space before the next word).
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
        // Consume the non-whitespace word itself.
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        // Greedily include one trailing space so the cursor lands between words.
        if end < chars.len() && chars[end] == ' ' {
            end += 1;
        }
        if end == 0 {
            return None;
        }
        Some(chars[..end].iter().collect())
    }

    fn open_compose_editor(&mut self, insert_newline: bool) {
        self.enter_compose_insert();
        if insert_newline {
            self.textarea.insert_newline();
        }
        self.completion.active = false;
    }

    fn apply_vim_operator_to_motion(&mut self, pending: VimPending, motion: CursorMove) {
        self.textarea.start_selection();
        self.textarea.move_cursor(motion);
        match pending {
            VimPending::Delete => {
                self.textarea.cut();
            }
            VimPending::Change => {
                self.textarea.cut();
                self.enter_compose_insert();
            }
            VimPending::Yank => {
                self.textarea.copy();
            }
            VimPending::Go => {}
        }
    }

    fn apply_vim_operator_to_word_end(&mut self, pending: VimPending) {
        self.textarea.start_selection();
        self.textarea.move_cursor(CursorMove::WordEnd);
        self.textarea.move_cursor(CursorMove::Forward);
        match pending {
            VimPending::Delete => {
                self.textarea.cut();
            }
            VimPending::Change => {
                self.textarea.cut();
                self.enter_compose_insert();
            }
            VimPending::Yank => {
                self.textarea.copy();
            }
            VimPending::Go => {}
        }
    }

    fn apply_vim_line_operator(&mut self, pending: VimPending) {
        self.textarea.cancel_selection();
        self.textarea.move_cursor(CursorMove::Head);
        self.textarea.start_selection();
        let cursor = self.textarea.cursor();
        self.textarea.move_cursor(CursorMove::Down);
        if cursor == self.textarea.cursor() {
            self.textarea.move_cursor(CursorMove::End);
        }
        match pending {
            VimPending::Delete => {
                self.textarea.cut();
            }
            VimPending::Change => {
                self.textarea.cut();
                self.enter_compose_insert();
            }
            VimPending::Yank => {
                self.textarea.copy();
            }
            VimPending::Go => {}
        }
    }

    fn handle_inline_input_key(&mut self, key: event::KeyEvent) {
        match (key.modifiers, key.code) {
            (mods, KeyCode::Enter)
                if mods.contains(KeyModifiers::SHIFT) || mods.contains(KeyModifiers::ALT) =>
            {
                self.open_compose_editor(true);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('j')) => {
                self.open_compose_editor(true);
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                if self.textarea.lines().len() > 1 {
                    self.textarea.input(key);
                    return;
                }
                self.update_completion();
                if self.completion.active {
                    return;
                }
                let (row, col) = self.textarea.cursor();
                let at_eol = col >= self.textarea.lines().get(row).map(|s| s.len()).unwrap_or(0);
                if at_eol {
                    if let Some(hint) = self.ghost_hint() {
                        for ch in hint.chars() {
                            self.textarea.insert_char(ch);
                        }
                    }
                }
            }
            (mods, KeyCode::Enter) if !mods.contains(KeyModifiers::SHIFT) => {
                self.submit_current_input();
            }
            (KeyModifiers::NONE, KeyCode::Up) if self.textarea.lines().len() <= 1 => {
                self.history_up();
            }
            (KeyModifiers::NONE, KeyCode::Down) if self.textarea.lines().len() <= 1 => {
                self.history_down();
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                let (row, col) = self.textarea.cursor();
                let line_len = self.textarea.lines().get(row).map(|s| s.len()).unwrap_or(0);
                if col >= line_len {
                    if let Some(hint) = self.ghost_hint() {
                        for ch in hint.chars() {
                            self.textarea.insert_char(ch);
                        }
                        return;
                    }
                }
                self.textarea.input(key);
            }
            (KeyModifiers::ALT, KeyCode::Right) => {
                let (row, col) = self.textarea.cursor();
                let line_len = self.textarea.lines().get(row).map(|s| s.len()).unwrap_or(0);
                if col >= line_len {
                    if let Some(word) = self.ghost_hint_next_word() {
                        for ch in word.chars() {
                            self.textarea.insert_char(ch);
                        }
                        return;
                    }
                }
                self.textarea.input(key);
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                let (row, col) = self.textarea.cursor();
                let line_len = self.textarea.lines().get(row).map(|s| s.len()).unwrap_or(0);
                if col >= line_len {
                    if let Some(hint) = self.ghost_hint() {
                        for ch in hint.chars() {
                            self.textarea.insert_char(ch);
                        }
                        return;
                    }
                }
                self.textarea.input(key);
            }
            _ => {
                self.textarea.input(key);
            }
        }
    }

    fn handle_compose_insert_key(&mut self, key: event::KeyEvent) {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('s'))
            | (KeyModifiers::CONTROL, KeyCode::Enter) => self.submit_current_input(),
            (KeyModifiers::CONTROL, KeyCode::Char('j')) => self.textarea.insert_newline(),
            (KeyModifiers::CONTROL, KeyCode::Char('[')) | (_, KeyCode::Esc) => {
                self.enter_compose_normal();
            }
            _ => {
                self.textarea.input(key);
            }
        }
    }

    fn handle_compose_normal_key(&mut self, key: event::KeyEvent) {
        if matches!(
            (key.modifiers, key.code),
            (KeyModifiers::CONTROL, KeyCode::Char('s')) | (KeyModifiers::CONTROL, KeyCode::Enter)
        ) {
            self.submit_current_input();
            return;
        }

        if matches!(key.code, KeyCode::Esc)
            || matches!(
                (key.modifiers, key.code),
                (KeyModifiers::CONTROL, KeyCode::Char('['))
            )
        {
            if self.vim_pending.take().is_none() {
                self.exit_compose_mode();
            }
            return;
        }

        if let Some(pending) = self.vim_pending.take() {
            match (pending, key.code) {
                (VimPending::Go, KeyCode::Char('g')) => self.textarea.move_cursor(CursorMove::Top),
                (VimPending::Delete, KeyCode::Char('d')) => self.apply_vim_line_operator(pending),
                (VimPending::Change, KeyCode::Char('c')) => self.apply_vim_line_operator(pending),
                (VimPending::Yank, KeyCode::Char('y')) => self.apply_vim_line_operator(pending),
                (
                    VimPending::Delete | VimPending::Change | VimPending::Yank,
                    KeyCode::Char('w'),
                ) => {
                    self.apply_vim_operator_to_motion(pending, CursorMove::WordForward);
                }
                (
                    VimPending::Delete | VimPending::Change | VimPending::Yank,
                    KeyCode::Char('b'),
                ) => {
                    self.apply_vim_operator_to_motion(pending, CursorMove::WordBack);
                }
                (
                    VimPending::Delete | VimPending::Change | VimPending::Yank,
                    KeyCode::Char('e'),
                ) => {
                    self.apply_vim_operator_to_word_end(pending);
                }
                (VimPending::Delete | VimPending::Change, KeyCode::Char('$')) => {
                    self.textarea.delete_line_by_end();
                    if matches!(pending, VimPending::Change) {
                        self.enter_compose_insert();
                    }
                }
                (VimPending::Yank, KeyCode::Char('$')) => {
                    self.textarea.start_selection();
                    self.textarea.move_cursor(CursorMove::End);
                    self.textarea.copy();
                }
                _ => {}
            }
            return;
        }

        match (key.modifiers, key.code) {
            (_, KeyCode::Enter) => {}
            (KeyModifiers::NONE, KeyCode::Char('h')) => self.textarea.move_cursor(CursorMove::Back),
            (KeyModifiers::NONE, KeyCode::Char('j')) => self.textarea.move_cursor(CursorMove::Down),
            (KeyModifiers::NONE, KeyCode::Char('k')) => self.textarea.move_cursor(CursorMove::Up),
            (KeyModifiers::NONE, KeyCode::Char('l')) => {
                self.textarea.move_cursor(CursorMove::Forward)
            }
            (KeyModifiers::NONE, KeyCode::Char('w')) => {
                self.textarea.move_cursor(CursorMove::WordForward)
            }
            (KeyModifiers::NONE, KeyCode::Char('b')) => {
                self.textarea.move_cursor(CursorMove::WordBack)
            }
            (KeyModifiers::NONE, KeyCode::Char('e')) => {
                self.textarea.move_cursor(CursorMove::WordEnd)
            }
            (KeyModifiers::NONE, KeyCode::Char('0')) | (KeyModifiers::NONE, KeyCode::Char('^')) => {
                self.textarea.move_cursor(CursorMove::Head)
            }
            (KeyModifiers::NONE, KeyCode::Char('$')) => self.textarea.move_cursor(CursorMove::End),
            (KeyModifiers::NONE, KeyCode::Char('g')) => self.vim_pending = Some(VimPending::Go),
            (KeyModifiers::SHIFT, KeyCode::Char('g'))
            | (KeyModifiers::NONE, KeyCode::Char('G')) => {
                self.textarea.move_cursor(CursorMove::Bottom)
            }
            (KeyModifiers::NONE, KeyCode::Char('i')) => self.enter_compose_insert(),
            (KeyModifiers::NONE, KeyCode::Char('a')) => {
                self.textarea.move_cursor(CursorMove::Forward);
                self.enter_compose_insert();
            }
            (KeyModifiers::SHIFT, KeyCode::Char('i'))
            | (KeyModifiers::NONE, KeyCode::Char('I')) => {
                self.textarea.move_cursor(CursorMove::Head);
                self.enter_compose_insert();
            }
            (KeyModifiers::SHIFT, KeyCode::Char('a'))
            | (KeyModifiers::NONE, KeyCode::Char('A')) => {
                self.textarea.move_cursor(CursorMove::End);
                self.enter_compose_insert();
            }
            (KeyModifiers::NONE, KeyCode::Char('o')) => {
                self.textarea.move_cursor(CursorMove::End);
                self.textarea.insert_newline();
                self.enter_compose_insert();
            }
            (KeyModifiers::SHIFT, KeyCode::Char('o'))
            | (KeyModifiers::NONE, KeyCode::Char('O')) => {
                self.textarea.move_cursor(CursorMove::Head);
                self.textarea.insert_newline();
                self.textarea.move_cursor(CursorMove::Up);
                self.enter_compose_insert();
            }
            (KeyModifiers::NONE, KeyCode::Char('x')) => {
                self.textarea.delete_next_char();
            }
            (KeyModifiers::NONE, KeyCode::Char('p')) => {
                self.textarea.paste();
            }
            (KeyModifiers::NONE, KeyCode::Char('u')) => {
                self.textarea.undo();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                self.textarea.redo();
            }
            (KeyModifiers::NONE, KeyCode::Char('d')) => self.vim_pending = Some(VimPending::Delete),
            (KeyModifiers::NONE, KeyCode::Char('c')) => self.vim_pending = Some(VimPending::Change),
            (KeyModifiers::NONE, KeyCode::Char('y')) => self.vim_pending = Some(VimPending::Yank),
            (KeyModifiers::SHIFT, KeyCode::Char('d'))
            | (KeyModifiers::NONE, KeyCode::Char('D')) => {
                self.textarea.delete_line_by_end();
            }
            _ => {}
        }
    }

    /// Activate or deactivate a skill by name.
    ///
    /// Typing `/skill_name` a second time toggles the skill off.  Active
    /// skills have their SKILL.md content silently prepended to the next agent
    /// prompt via `build_prompt_with_skills()`.
    fn activate_skill(&mut self, name: &str) {
        let skill_md = Self::find_skill_md(name);
        let skill_md = match skill_md {
            Some(p) => p,
            None => {
                self.push_output(
                    format!("Skill '{name}' not found. Type /skills to browse available skills."),
                    OutputRole::Error,
                );
                return;
            }
        };
        // Toggle: typing /name again deactivates the skill.
        if let Some(pos) = self.active_skills.iter().position(|s| s == name) {
            self.active_skills.remove(pos);
            self.push_output(
                format!("📚 Skill '{name}' deactivated."),
                OutputRole::System,
            );
            return;
        }
        self.active_skills.push(name.to_string());
        let desc = Self::read_skill_desc(&skill_md);
        let msg = if desc.is_empty() {
            format!(
                "📚 Skill '{name}' activated — its context will be prepended to your next message."
            )
        } else {
            format!("📚 Skill '{name}' activated: {desc}")
        };
        self.push_output(msg, OutputRole::System);
    }

    /// Build the prompt actually sent to the agent by prepending active skill
    /// contexts to the user's raw input.
    ///
    /// The enriched prompt is invisible in the output pane — the user's message
    /// is displayed as-is; only the agent sees the skill content.  This keeps
    /// the conversation history readable while still injecting the full skill
    /// context.
    fn build_prompt_with_skills(&self, user_input: &str) -> String {
        if self.active_skills.is_empty() {
            return user_input.to_string();
        }
        let mut context = String::new();
        for name in &self.active_skills {
            if let Some(skill_md) = Self::find_skill_md(name) {
                if let Ok(content) = std::fs::read_to_string(&skill_md) {
                    context.push_str(&format!(
                        "--- SKILL: {name} ---\n{}\n--- END SKILL ---\n\n",
                        content.trim()
                    ));
                }
            }
        }
        if context.is_empty() {
            return user_input.to_string();
        }
        format!("{context}{user_input}")
    }

    // ─── Tab Completion ─────────────────────────────────────────────

    // Update completion candidates based on current input.
    // ─── History ────────────────────────────────────────────────────

    /// Path to the persistent history file.
    fn history_path() -> Option<std::path::PathBuf> {
        Some(edgecrab_core::edgecrab_home().join("history"))
    }

    /// Load history from disk (called once at startup).
    fn load_history_file(&mut self) {
        if let Some(path) = Self::history_path() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                self.input_history = content
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
                // Cap at 500
                if self.input_history.len() > 500 {
                    let excess = self.input_history.len() - 500;
                    self.input_history.drain(..excess);
                }
                self.history_pos = self.input_history.len();
            }
        }
    }

    /// Persist history to disk (called after each push and on exit).
    fn save_history_file(&self) {
        if let Some(path) = Self::history_path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let content = self.input_history.join("\n");
            let _ = std::fs::write(&path, content);
        }
    }

    fn history_up(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        if self.history_pos == self.input_history.len() {
            // Stash current input before navigating
            self.history_stash = self.textarea_text();
        }
        if self.history_pos > 0 {
            self.history_pos -= 1;
            let entry = self.input_history[self.history_pos].clone();
            self.textarea_set_text(&entry);
        }
    }

    fn history_down(&mut self) {
        if self.history_pos >= self.input_history.len() {
            return;
        }
        self.history_pos += 1;
        let text = if self.history_pos == self.input_history.len() {
            self.history_stash.clone()
        } else {
            self.input_history[self.history_pos].clone()
        };
        self.textarea_set_text(&text);
    }

    fn push_history(&mut self, entry: &str) {
        let trimmed = entry.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        // Dedup consecutive
        if self
            .input_history
            .last()
            .is_some_and(|last| *last == trimmed)
        {
            return;
        }
        self.input_history.push(trimmed);
        // Cap at 500
        if self.input_history.len() > 500 {
            self.input_history.remove(0);
        }
        self.history_pos = self.input_history.len();
        // Persist immediately
        self.save_history_file();
    }

    // ─── Ghost text (Fish-style history hint) ───────────────────────

    fn ghost_hint(&self) -> Option<String> {
        let text = self.textarea_text();
        if text.len() < 2 || text.contains('\n') {
            return None;
        }
        // Search history from most recent backwards
        for entry in self.input_history.iter().rev() {
            if entry.starts_with(&text) && entry != &text {
                return Some(entry[text.len()..].to_string());
            }
        }
        None
    }

    /// Scroll the output area by `delta` visual rows (positive = up, negative = down).
    fn scroll_output(&mut self, delta: i32) {
        let max_scroll = self
            .output_visual_rows
            .saturating_sub(self.output_area_height);
        let new_offset = (self.scroll_offset as i32 + delta).clamp(0, max_scroll as i32) as u16;
        self.scroll_offset = new_offset;
        self.at_bottom = self.scroll_offset == 0;
        self.needs_redraw = true;
    }

    /// Handle a pasted text string (from bracketed paste events or /paste command).
    ///
    /// If the pasted text is (or contains) a path to a local image file, the path is
    /// added to `pending_images` instead of the textarea so it is automatically
    /// analysed by the vision LLM when the user sends their next message.
    pub fn handle_paste(&mut self, text: String) {
        let trimmed = text.trim();
        if Self::is_image_path(trimmed) && std::path::Path::new(trimmed).is_file() {
            let path = std::path::PathBuf::from(trimmed);
            self.pending_images.push(path);
            let count = self.pending_images.len();
            self.push_output(
                format!(
                    "📎 Image file attached: {}  ({} image(s) queued — send a message to analyse with vision LLM)",
                    trimmed, count
                ),
                OutputRole::System,
            );
        } else {
            // Normal text paste — insert into textarea
            self.textarea.insert_str(&text);
        }
        self.needs_redraw = true;
    }

    /// Return true if `s` looks like a local image file path (by extension).
    fn is_image_path(s: &str) -> bool {
        let lower = s.to_ascii_lowercase();
        matches!(
            std::path::Path::new(&lower)
                .extension()
                .and_then(|e| e.to_str()),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "avif")
        )
    }

    /// Handle a crossterm mouse event.
    /// Scroll wheel maps to page-based output scroll so users can navigate
    /// without lifting hands from the mouse.
    pub fn handle_mouse_event(&mut self, event: event::MouseEvent) {
        if !self.mouse_capture_enabled {
            return;
        }

        match event.kind {
            // Scroll wheel up → scroll content upward (away from bottom)
            MouseEventKind::ScrollUp => {
                self.scroll_output(5);
            }
            // Scroll wheel down → scroll content downward (toward bottom)
            MouseEventKind::ScrollDown => {
                self.scroll_output(-5);
            }
            // Touchpad horizontal gestures: map to larger vertical scroll jumps.
            MouseEventKind::ScrollLeft => {
                self.scroll_output(12);
            }
            MouseEventKind::ScrollRight => {
                self.scroll_output(-12);
            }
            // Left click: collapse overlays.
            // Double-click (two clicks ≤400 ms apart) in SCROLL mode → switch to SELECT.
            // The reverse (SELECT→SCROLL) is not possible via click because mouse events
            // are not delivered to the process when capture is off; use F6, Ctrl+M, or
            // `/scroll on` instead.
            MouseEventKind::Down(MouseButton::Left) => {
                self.completion.active = false;
                self.model_selector.active = false;
                self.vision_model_selector.active = false;
                self.skill_selector.active = false;
                self.needs_redraw = true;
                let now = Instant::now();
                let is_double = self
                    .last_left_click
                    .map(|t| now.duration_since(t).as_millis() <= 400)
                    .unwrap_or(false);
                if is_double {
                    self.last_left_click = None;
                    self.toggle_mouse_capture_mode();
                } else {
                    self.last_left_click = Some(now);
                }
            }
            _ => {}
        }
    }

    /// Handle a crossterm key event.
    pub fn handle_key_event(&mut self, key: event::KeyEvent) {
        // Only process key press events, ignore release events (prevents double-fire on Windows)
        if key.kind == KeyEventKind::Release {
            return;
        }

        self.needs_redraw = true;

        // Global shortcuts first — these work regardless of any overlay
        match (key.modifiers, key.code) {
            // Ctrl+C — clear input → cancel agent → exit  (standard readline behaviour)
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                let text = self.textarea_text();
                if !text.is_empty() {
                    // Non-empty input: clear it (like ^C at a shell prompt)
                    self.textarea_clear();
                    self.completion.active = false;
                    self.history_pos = self.input_history.len();
                    self.push_output("^C", OutputRole::System);
                } else if self.is_processing {
                    // Agent is running: interrupt it
                    if let Some(ref agent) = self.agent {
                        agent.interrupt();
                    }
                    self.push_output("^C  (cancelled)", OutputRole::System);
                } else {
                    // Nothing to do: exit
                    self.should_exit = true;
                }
                return;
            }
            // Ctrl+D — exit (EOF signal, identical to shell behaviour)
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                let text = self.textarea_text();
                if text.is_empty() {
                    self.should_exit = true;
                }
                // Non-empty: let textarea handle delete-char (standard readline)
                return;
            }
            // Ctrl+L — clear screen (standard shell shortcut)
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                self.clear_output();
                return;
            }
            // Ctrl+Shift+V — paste clipboard image (or text) into conversation.
            // Ctrl+V (without Shift) arrives as a bracketed-paste Event::Paste, so
            // this shortcut gives explicit access to the arboard clipboard reader
            // which can capture raw images (screenshots, browser copies, etc.).
            (m, KeyCode::Char('v'))
                if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
            {
                self.handle_paste_clipboard();
                return;
            }
            // F6 — toggle mouse capture mode for copy/select ergonomics.
            (_, KeyCode::F(6)) => {
                self.toggle_mouse_capture_mode();
                return;
            }
            // Ctrl+M — alternate toggle for mouse capture mode.
            (KeyModifiers::CONTROL, KeyCode::Char('m')) => {
                self.toggle_mouse_capture_mode();
                return;
            }
            // Ctrl+U — clear current input line (standard readline shortcut)
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.textarea_clear();
                self.completion.active = false;
                return;
            }
            // Ctrl+G — scroll output to very bottom (jump back to live view)
            (KeyModifiers::CONTROL, KeyCode::Char('g')) => {
                self.scroll_offset = 0;
                self.at_bottom = true;
                self.needs_redraw = true;
                return;
            }
            // Ctrl+K — kill text from cursor to end of line (readline standard)
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                self.textarea.delete_line_by_end();
                self.needs_redraw = true;
                return;
            }
            // Ctrl+A — move cursor to beginning of line (readline standard)
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                self.textarea.move_cursor(CursorMove::Head);
                self.needs_redraw = true;
                return;
            }
            // Ctrl+E — move cursor to end of line (readline standard)
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                self.textarea.move_cursor(CursorMove::End);
                self.needs_redraw = true;
                return;
            }
            // Ctrl+Home — scroll output to very top
            (KeyModifiers::CONTROL, KeyCode::Home) => {
                let max_scroll = self
                    .output_visual_rows
                    .saturating_sub(self.output_area_height);
                self.scroll_offset = max_scroll;
                self.at_bottom = false;
                return;
            }
            // Ctrl+End — scroll output to very bottom
            (KeyModifiers::CONTROL, KeyCode::End) => {
                self.scroll_offset = 0;
                self.at_bottom = true;
                return;
            }
            // Shift+Up — scroll output up one line (doesn't conflict with history navigation)
            (KeyModifiers::SHIFT, KeyCode::Up) => {
                self.scroll_output(5);
                return;
            }
            // Shift+Down — scroll output down one line
            (KeyModifiers::SHIFT, KeyCode::Down) => {
                self.scroll_output(-5);
                return;
            }
            // Alt+Up — scroll output up (works in multi-line input mode)
            (KeyModifiers::ALT, KeyCode::Up) => {
                self.scroll_output(5);
                return;
            }
            // Alt+Down — scroll output down
            (KeyModifiers::ALT, KeyCode::Down) => {
                self.scroll_output(-5);
                return;
            }
            // F1 — show help overlay
            (_, KeyCode::F(1)) => {
                self.process_input("/help");
                return;
            }
            // F2 — open model selector
            (_, KeyCode::F(2)) => {
                self.refresh_model_selector_catalog();
                return;
            }
            // F3 — open skill browser (same experience as F2 for models)
            (_, KeyCode::F(3)) => {
                self.refresh_skills_list();
                self.skill_selector.activate();
                return;
            }
            // F7 — open dedicated vision-model selector
            (_, KeyCode::F(7)) => {
                self.open_vision_model_selector();
                return;
            }
            // F4 — open session browser overlay
            (_, KeyCode::F(4)) => {
                self.open_session_browser();
                return;
            }
            // F5 — retry last message
            (_, KeyCode::F(5)) => {
                self.process_input("/retry");
                return;
            }
            // F10 — toggle verbose mode
            (_, KeyCode::F(10)) => {
                self.process_input("/verbose");
                return;
            }
            _ => {}
        }

        // Approval overlay active — intercept all keys for choice navigation
        if matches!(self.display_state, DisplayState::WaitingForApproval { .. }) {
            self.handle_approval_key(key);
            return;
        }

        // Secret capture overlay active — intercept all keys for masked input
        if matches!(self.display_state, DisplayState::SecretCapture { .. }) {
            self.handle_secret_capture_key(key);
            return;
        }

        // Model selector overlay active — intercept all keys
        if self.model_selector.active {
            match key.code {
                KeyCode::Esc => {
                    self.model_selector.active = false;
                }
                KeyCode::Enter => {
                    if let Some(model) = self.model_selector.current().map(|e| e.display.clone()) {
                        self.model_selector.active = false;
                        self.handle_model_switch(model);
                    }
                }
                KeyCode::Up => self.model_selector.move_up(),
                KeyCode::Down => self.model_selector.move_down(),
                KeyCode::PageUp => self.model_selector.page_up(),
                KeyCode::PageDown => self.model_selector.page_down(),
                KeyCode::Backspace => self.model_selector.pop_char(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.model_selector.push_char(c);
                }
                _ => {}
            }
            return;
        }

        // Vision-model selector overlay active — same navigation as /model.
        if self.vision_model_selector.active {
            match key.code {
                KeyCode::Esc => {
                    self.vision_model_selector.active = false;
                }
                KeyCode::Enter => {
                    if let Some(model) = self
                        .vision_model_selector
                        .current()
                        .map(|entry| entry.display.clone())
                    {
                        self.vision_model_selector.active = false;
                        self.handle_set_vision_model(model);
                    }
                }
                KeyCode::Up => self.vision_model_selector.move_up(),
                KeyCode::Down => self.vision_model_selector.move_down(),
                KeyCode::PageUp => self.vision_model_selector.page_up(),
                KeyCode::PageDown => self.vision_model_selector.page_down(),
                KeyCode::Backspace => self.vision_model_selector.pop_char(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.vision_model_selector.push_char(c);
                }
                _ => {}
            }
            return;
        }

        // Skill selector overlay active — same key scheme as model selector
        if self.skill_selector.active {
            match key.code {
                KeyCode::Esc => {
                    self.skill_selector.active = false;
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.skill_selector.current() {
                        let skill_name = format!("/{} ", entry.name);
                        self.skill_selector.active = false;
                        self.textarea_set_text(&skill_name);
                        self.needs_redraw = true;
                    }
                }
                KeyCode::Up => self.skill_selector.move_up(),
                KeyCode::Down => self.skill_selector.move_down(),
                KeyCode::PageUp => self.skill_selector.page_up(),
                KeyCode::PageDown => self.skill_selector.page_down(),
                KeyCode::Backspace => self.skill_selector.pop_char(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.skill_selector.push_char(c);
                }
                _ => {}
            }
            return;
        }

        // Skin browser overlay active — select with Enter to hot-reload skin
        if self.skin_browser.active {
            match key.code {
                KeyCode::Esc => {
                    self.skin_browser.active = false;
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.skin_browser.current() {
                        let name = entry.name.clone();
                        self.skin_browser.active = false;
                        self.handle_switch_skin(name);
                    }
                }
                KeyCode::Up => self.skin_browser.move_up(),
                KeyCode::Down => self.skin_browser.move_down(),
                KeyCode::PageUp => self.skin_browser.page_up(),
                KeyCode::PageDown => self.skin_browser.page_down(),
                KeyCode::Backspace => self.skin_browser.pop_char(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.skin_browser.push_char(c);
                }
                _ => {}
            }
            return;
        }

        // Session browser overlay active — same key scheme as skill/model selectors
        if self.session_browser.active {
            match key.code {
                KeyCode::Esc => {
                    self.session_browser.active = false;
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.session_browser.current() {
                        let session_id = entry.id.clone();
                        self.session_browser.active = false;
                        self.handle_resume_session(Some(session_id));
                    }
                }
                KeyCode::Up => self.session_browser.move_up(),
                KeyCode::Down => self.session_browser.move_down(),
                KeyCode::PageUp => self.session_browser.page_up(),
                KeyCode::PageDown => self.session_browser.page_down(),
                KeyCode::Backspace => self.session_browser.pop_char(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.session_browser.push_char(c);
                }
                _ => {}
            }
            return;
        }

        // Completion overlay active — intercept Tab, Enter, Escape, arrows
        if self.completion.active {
            match key.code {
                KeyCode::Tab => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected =
                            (self.completion.selected + 1) % self.completion.candidates.len();
                    }
                    return;
                }
                KeyCode::BackTab => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = if self.completion.selected == 0 {
                            self.completion.candidates.len() - 1
                        } else {
                            self.completion.selected - 1
                        };
                    }
                    return;
                }
                KeyCode::Up => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = if self.completion.selected == 0 {
                            self.completion.candidates.len() - 1
                        } else {
                            self.completion.selected - 1
                        };
                    }
                    return;
                }
                KeyCode::Down => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected =
                            (self.completion.selected + 1) % self.completion.candidates.len();
                    }
                    return;
                }
                KeyCode::PageUp => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = self.completion.selected.saturating_sub(8);
                    }
                    return;
                }
                KeyCode::PageDown => {
                    if !self.completion.candidates.is_empty() {
                        let last = self.completion.candidates.len() - 1;
                        self.completion.selected = (self.completion.selected + 8).min(last);
                    }
                    return;
                }
                KeyCode::Home => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = 0;
                    }
                    return;
                }
                KeyCode::End => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = self.completion.candidates.len() - 1;
                    }
                    return;
                }
                KeyCode::Enter => {
                    self.accept_completion();
                    return;
                }
                KeyCode::Esc => {
                    self.completion.active = false;
                    return;
                }
                _ => {
                    // Any other key deactivates completion and falls through
                    self.completion.active = false;
                }
            }
        }

        match (key.modifiers, key.code) {
            // Page Up/Down — scroll output by viewport height
            (_, KeyCode::PageUp) => {
                let page = self.output_area_height.max(3).saturating_sub(2);
                self.scroll_output(page as i32);
                return;
            }
            (_, KeyCode::PageDown) => {
                let page = self.output_area_height.max(3).saturating_sub(2);
                self.scroll_output(-(page as i32));
                return;
            }
            _ => {}
        }

        match self.editor_mode {
            InputEditorMode::Inline => self.handle_inline_input_key(key),
            InputEditorMode::ComposeInsert => self.handle_compose_insert_key(key),
            InputEditorMode::ComposeNormal => self.handle_compose_normal_key(key),
        }
    }

    /// Process submitted input — either slash command or agent prompt.
    fn process_input(&mut self, input: &str) {
        // If the agent is waiting for a clarifying answer, route this input
        // directly back to the waiting tool instead of starting a new prompt.
        if let Some(tx) = self.clarify_pending_tx.take() {
            self.push_output(format!("> {}", input), OutputRole::User);
            let _ = tx.send(input.to_string());
            // Restore normal input border label now that the clarify reply is sent.
            self.textarea.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.theme.input_border)
                    .title(format!(" {} ", self.theme.prompt_symbol.trim())),
            );
            // The agent task is now unblocked and will resume processing;
            // is_processing is still true so the spinner stays active.
            self.display_state = DisplayState::Thinking {
                frame: 0,
                started: Instant::now(),
            };
            return;
        }

        // Try slash command first
        if let Some(result) = self.commands.dispatch(input) {
            self.handle_command_result(result, input);
            return;
        }

        // Unrecognised '/token' — check whether it names a known skill.
        // Skills are activated/deactivated by typing their name with a leading
        // slash, e.g. `/arxiv-impact-ranking`.  This keeps the command registry
        // (Single Responsibility) free of skill-discovery logic.
        //
        // Supports an optional inline query:  `/skill_name question here`
        // — the skill is activated first, then the inline text is submitted to
        // the agent immediately (with skill context injected).  This lets users
        // write both activation and question in a single Enter-press.
        if input.starts_with('/') {
            let cmd_name = input
                .strip_prefix('/')
                .unwrap_or("")
                .split_whitespace()
                .next()
                .unwrap_or("");
            if !cmd_name.is_empty() && self.skills_completion_names.contains(&cmd_name.to_string())
            {
                // Text after `/skill_name` (may be empty)
                let inline_query = input[1 + cmd_name.len()..].trim().to_string();
                self.activate_skill(cmd_name);
                if !inline_query.is_empty() {
                    // Re-enter process_input with just the query text so the
                    // full agent-dispatch path runs (output, is_processing check,
                    // build_prompt_with_skills, streaming) without duplication.
                    self.process_input(&inline_query);
                }
                return;
            }
            self.push_output(
                format!("Unknown command: /{cmd_name}. Type /help for commands or /skills to browse skills."),
                OutputRole::System,
            );
            return;
        }

        // Regular prompt — show it in output and dispatch to agent
        self.push_output(format!("> {}", input), OutputRole::User);

        let agent = match self.agent.clone() {
            Some(a) => a,
            None => {
                self.push_output(
                    "No agent configured. Run with a provider to enable chat.",
                    OutputRole::Error,
                );
                return;
            }
        };

        if self.is_processing {
            self.push_output("Still processing previous request...", OutputRole::System);
            return;
        }
        self.is_processing = true;
        self.reasoning_line = None;
        // Reset per-turn streaming counters.
        self.in_flight_tool_count = 0;
        self.active_subagents.clear();
        self.turn_stream_tokens = 0;
        // Reset the response accumulator for the new turn (voice mode uses it).
        self.last_agent_response_text.clear();
        self.display_state = DisplayState::Thinking {
            frame: 0,
            started: Instant::now(),
        };

        let tx = self.response_tx.clone();
        // Build the enriched prompt: active skill contexts are prepended
        // silently (the display above already shows the raw input to the user).
        //
        // If clipboard images are pending, append vision_analyze instructions
        // so the agent automatically processes the attached image(s).
        let mut effective_input = input.to_string();
        if !self.pending_images.is_empty() {
            let image_paths: Vec<String> = self
                .pending_images
                .drain(..)
                .map(|p| p.display().to_string())
                .collect();
            let path_refs: Vec<&str> = image_paths.iter().map(|s| s.as_str()).collect();
            effective_input = format!(
                "{input}\n\n{block}",
                input = input,
                block = format_image_attachment_block(&path_refs)
            );
        }
        let prompt = self.build_prompt_with_skills(&effective_input);
        let hook_registry_clone = self.hook_registry.clone();
        self.rt_handle.spawn(async move {
            use edgecrab_core::agent::StreamEvent;
            let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

            let agent_clone = Arc::clone(&agent);
            let prompt_clone = prompt.clone();
            let hook_registry = hook_registry_clone;
            let agent_task = tokio::spawn(async move {
                agent_clone.chat_streaming(&prompt_clone, chunk_tx).await
            });
            let mut saw_terminal_event = false;

            while let Some(event) = chunk_rx.recv().await {
                match event {
                    StreamEvent::Token(text) => {
                        let _ = tx.send(AgentResponse::Token(text));
                    }
                    StreamEvent::Reasoning(text) => {
                        let _ = tx.send(AgentResponse::Reasoning(text));
                    }
                    StreamEvent::ToolExec { name, args_json } => {
                        let _ = tx.send(AgentResponse::ToolExec { name, args_json });
                    }
                    StreamEvent::ToolDone {
                        name,
                        args_json,
                        result_preview,
                        duration_ms,
                        is_error,
                    } => {
                        let _ = tx.send(AgentResponse::ToolDone {
                            name,
                            args_json,
                            result_preview,
                            duration_ms,
                            is_error,
                        });
                    }
                    StreamEvent::SubAgentStart {
                        task_index,
                        task_count,
                        goal,
                    } => {
                        let _ = tx.send(AgentResponse::SubAgentStart {
                            task_index,
                            task_count,
                            goal,
                        });
                    }
                    StreamEvent::SubAgentReasoning {
                        task_index,
                        task_count,
                        text,
                    } => {
                        let _ = tx.send(AgentResponse::SubAgentReasoning {
                            task_index,
                            task_count,
                            text,
                        });
                    }
                    StreamEvent::SubAgentToolExec {
                        task_index,
                        task_count,
                        name,
                        args_json,
                    } => {
                        let _ = tx.send(AgentResponse::SubAgentToolExec {
                            task_index,
                            task_count,
                            name,
                            args_json,
                        });
                    }
                    StreamEvent::SubAgentFinish {
                        task_index,
                        task_count,
                        status,
                        duration_ms,
                        summary,
                        api_calls,
                        model,
                    } => {
                        let _ = tx.send(AgentResponse::SubAgentFinish {
                            task_index,
                            task_count,
                            status,
                            duration_ms,
                            summary,
                            api_calls,
                            model,
                        });
                    }
                    StreamEvent::Done => {
                        saw_terminal_event = true;
                        let _ = tx.send(AgentResponse::Done);
                        break;
                    }
                    StreamEvent::Error(e) => {
                        saw_terminal_event = true;
                        let _ = tx.send(AgentResponse::Error(e));
                        break;
                    }
                    StreamEvent::Clarify {
                        question,
                        choices,
                        response_tx,
                    } => {
                        let _ = tx.send(AgentResponse::Clarify {
                            question,
                            choices,
                            response_tx,
                        });
                        // Don't break — the agent is paused waiting for the answer.
                        // The TUI will send the answer via the oneshot channel, which
                        // unblocks the clarify tool and lets the agent continue.
                    }

                    StreamEvent::Approval {
                        command,
                        full_command,
                        reasons: _,
                        response_tx,
                    } => {
                        let _ = tx.send(AgentResponse::Approval {
                            command,
                            full_command,
                            response_tx,
                        });
                        // Don't break — the agent is paused waiting for the approval.
                    }

                    StreamEvent::SecretRequest {
                        var_name,
                        prompt,
                        is_sudo,
                        response_tx,
                    } => {
                        let _ = tx.send(AgentResponse::SecretRequest {
                            var_name,
                            prompt,
                            is_sudo,
                            response_tx,
                        });
                        // Don't break — agent is paused waiting for the secret value.
                    }

                    StreamEvent::HookEvent { event, context_json } => {
                        // Forward tool:pre/post, llm:pre/post events from the
                        // conversation loop to file-based hook scripts.
                        if let Ok(ctx) = serde_json::from_str::<
                            edgecrab_gateway::hooks::HookContext
                        >(&context_json) {
                            hook_registry.emit(&event, &ctx).await;
                        }
                        // HookEvent is internal — not forwarded to the TUI channel.
                    }

                    StreamEvent::ContextPressure { estimated_tokens, threshold_tokens } => {
                        let _ = tx.send(AgentResponse::Notice(format_context_pressure_notice(
                            estimated_tokens,
                            threshold_tokens,
                        )));
                    }
                }
            }

            if !saw_terminal_event {
                let message = match agent_task.await {
                    Ok(Ok(())) => {
                        "Agent stream closed unexpectedly before emitting completion.".to_string()
                    }
                    Ok(Err(err)) => err.to_string(),
                    Err(err) => format!("Agent task failed: {err}"),
                };
                let _ = tx.send(AgentResponse::Error(message));
            }
        });
    }

    /// Handle a CommandResult from the slash command registry.
    fn handle_command_result(&mut self, result: CommandResult, _input: &str) {
        match result {
            CommandResult::Output(text) => {
                self.push_output(text, OutputRole::System);
            }
            CommandResult::Clear => {
                self.clear_output();
            }
            CommandResult::Exit => {
                self.should_exit = true;
            }
            CommandResult::Noop => {}
            CommandResult::ModelSwitch(model) => {
                self.handle_model_switch(model);
            }
            CommandResult::ModelSelector => {
                self.refresh_model_selector_catalog();
            }
            CommandResult::VisionModelSelector => {
                self.open_vision_model_selector();
            }
            CommandResult::ShowVisionModel => {
                self.handle_show_vision_model();
            }
            CommandResult::SetVisionModel(spec) => {
                self.handle_set_vision_model(spec);
            }
            CommandResult::SessionNew => {
                if let Some(ref agent) = self.agent {
                    let agent = Arc::clone(agent);
                    self.rt_handle.block_on(async move {
                        agent.new_session().await;
                    });
                }
                self.clear_output();
                self.push_output("New session started.", OutputRole::System);
            }
            CommandResult::ReloadTheme => {
                self.theme = Theme::from_skin(&SkinConfig::load());
                // Update textarea style
                self.textarea.set_block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(self.theme.input_border)
                        .title(format!(" {} ", self.theme.prompt_symbol.trim())),
                );
                self.textarea.set_style(self.theme.input_text);
                self.push_output(
                    "Theme reloaded from ~/.edgecrab/skin.yaml",
                    OutputRole::System,
                );
            }
            CommandResult::Stop => {
                if let Some(ref agent) = self.agent {
                    agent.interrupt();
                }
                self.push_output("Stopping current request...", OutputRole::System);
            }
            CommandResult::Retry => {
                if let Some(agent) = self.agent.clone() {
                    let messages = self.rt_handle.block_on(async { agent.messages().await });
                    if let Some(last_user) = messages
                        .iter()
                        .rev()
                        .find(|m| m.role == edgecrab_types::Role::User)
                    {
                        let text = last_user.text_content();
                        let agent2 = agent.clone();
                        self.rt_handle
                            .block_on(async { agent2.undo_last_turn().await });
                        self.process_input(&text);
                    } else {
                        self.push_output("No previous user message to retry.", OutputRole::System);
                    }
                } else {
                    self.push_output("No agent configured.", OutputRole::Error);
                }
            }
            CommandResult::Undo => {
                if let Some(agent) = self.agent.clone() {
                    let removed = self
                        .rt_handle
                        .block_on(async { agent.undo_last_turn().await });
                    if removed > 0 {
                        self.push_output(
                            format!("Undone — removed {removed} message(s) from history."),
                            OutputRole::System,
                        );
                    } else {
                        self.push_output("Nothing to undo.", OutputRole::System);
                    }
                } else {
                    self.push_output("No agent configured.", OutputRole::Error);
                }
            }
            CommandResult::Compress => {
                if let Some(agent) = self.agent.clone() {
                    let tx = self.response_tx.clone();
                    self.display_state = DisplayState::BgOp {
                        label: "Compressing context…".to_string(),
                        frame: 0,
                        started: Instant::now(),
                    };
                    self.needs_redraw = true;
                    self.rt_handle.spawn(async move {
                        let before_messages = agent.messages().await;
                        let before_count = before_messages.len();
                        let before_tokens =
                            edgecrab_core::compression::estimate_tokens(&before_messages);
                        agent.force_compress().await;
                        let after_messages = agent.messages().await;
                        let after_count = after_messages.len();
                        let after_tokens =
                            edgecrab_core::compression::estimate_tokens(&after_messages);
                        let msg = format!(
                            "Compression done: {before_count} → {after_count} messages (~{before_tokens} → ~{after_tokens} tokens)."
                        );
                        let _ = tx.send(AgentResponse::BgOp(BackgroundOpResult::CompressDone {
                            msg,
                        }));
                    });
                } else {
                    self.push_output("No agent configured.", OutputRole::Error);
                }
            }
            CommandResult::ShowStatus => {
                self.handle_show_status();
            }
            CommandResult::ShowCost => {
                self.handle_show_cost();
            }
            CommandResult::ShowUsage => {
                self.handle_show_usage();
            }
            CommandResult::ShowPrompt => {
                self.handle_show_prompt();
            }
            CommandResult::ShowHistory => {
                self.handle_show_history();
            }
            CommandResult::ToggleVerbose => {
                self.verbose = !self.verbose;
                let state = if self.verbose { "ON" } else { "OFF" };
                self.push_output(format!("Verbose mode: {state}"), OutputRole::System);
            }
            CommandResult::SaveSession(path) => {
                self.handle_save_session(path);
            }
            CommandResult::ExportSession(path) => {
                self.handle_export_session(path);
            }
            CommandResult::SetTitle(title) => {
                self.handle_set_title(title);
            }
            CommandResult::SessionList => {
                self.handle_session_list();
            }
            CommandResult::SessionSwitch(id) => {
                self.handle_resume_session(Some(id));
            }
            CommandResult::SessionDelete(id) => {
                self.handle_session_delete(id);
            }
            CommandResult::ResumeSession(id) => {
                self.handle_resume_session(id);
            }
            CommandResult::SessionRename(id, title) => {
                self.handle_session_rename(id, title);
            }
            CommandResult::SessionPrune(days) => {
                self.handle_session_prune(days);
            }
            CommandResult::QueuePrompt(prompt) => {
                self.prompt_queue.push(prompt.clone());
                let n = self.prompt_queue.len();
                let preview = &prompt[..prompt.len().min(60)];
                self.push_output(
                    format!("Queued ({n} pending): {preview}"),
                    OutputRole::System,
                );
            }
            CommandResult::BackgroundPrompt(prompt) => {
                self.handle_background_prompt(prompt);
            }
            CommandResult::Approve => {
                self.push_output("Approved. (No pending actions.)", OutputRole::System);
            }
            CommandResult::Deny => {
                self.push_output("Denied. (No pending actions.)", OutputRole::System);
            }
            CommandResult::ShowSkills(args) => {
                self.handle_show_skills(args);
            }
            CommandResult::SkillSelector => {
                self.refresh_skills_list();
                self.skill_selector.activate();
            }
            CommandResult::ShowTools => {
                self.handle_show_tools();
            }
            CommandResult::ShowToolsets => {
                self.handle_show_toolsets();
            }
            CommandResult::SetReasoning(level) => {
                self.handle_set_reasoning(level);
            }
            CommandResult::SetStreaming(mode) => {
                self.handle_set_streaming(mode);
            }
            CommandResult::ListModels(filter) => {
                self.handle_list_models(filter);
            }
            CommandResult::ShowCronStatus(args) => {
                self.handle_show_cron_status(args);
            }
            CommandResult::ShowPlugins => {
                self.handle_show_plugins();
            }
            CommandResult::ShowPlatforms => {
                self.handle_show_platforms();
            }
            CommandResult::ShowPersonality => {
                self.handle_show_personality();
            }
            CommandResult::SwitchPersonality(name) => {
                self.handle_switch_personality(name);
            }
            CommandResult::SwitchSkin(name) => {
                self.handle_switch_skin(name);
            }
            CommandResult::ShowInsights => {
                self.handle_show_insights();
            }
            CommandResult::PasteClipboard => {
                self.handle_paste_clipboard();
            }
            CommandResult::CopilotAuth => {
                self.handle_copilot_auth();
            }
            CommandResult::MouseMode(mode) => {
                self.handle_mouse_mode(mode);
            }
            #[cfg(target_os = "macos")]
            CommandResult::MacosPermissions(args) => {
                let report = crate::permissions::run_permissions_command(&args);
                self.push_output(report, OutputRole::System);
            }
            CommandResult::RollbackCheckpoint(args) => {
                self.handle_rollback_checkpoint(args);
            }
            CommandResult::ReloadMcp => {
                self.handle_reload_mcp();
            }
            CommandResult::VoiceMode(args) => {
                self.handle_voice_mode(args);
            }
            CommandResult::McpToken(args) => {
                self.handle_mcp_token(args);
            }
            CommandResult::BrowserCommand(args) => {
                self.handle_browser_command(args);
            }
        }
    }

    fn toggle_mouse_capture_mode(&mut self) {
        let next = !self.mouse_capture_enabled;
        self.set_mouse_capture_mode(next);
    }

    fn set_mouse_capture_mode(&mut self, enabled: bool) {
        if self.mouse_capture_enabled == enabled {
            return;
        }
        self.mouse_capture_enabled = enabled;
        self.pending_mouse_capture = Some(enabled);
        self.needs_redraw = true;
        if enabled {
            self.push_output(
                "[SCROLL] Mouse capture on — wheel scrolls output. Press F6 or /mouse off to switch to SELECT mode (native drag-to-copy).",
                OutputRole::System,
            );
        } else {
            self.push_output(
                "[SELECT] Native selection active — drag to copy. Press F6 or /mouse on to switch to SCROLL mode (wheel scrolling).",
                OutputRole::System,
            );
        }
    }

    fn handle_mouse_mode(&mut self, mode: String) {
        let normalized = mode.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "" | "toggle" => self.toggle_mouse_capture_mode(),
            "on" | "enable" | "enabled" => self.set_mouse_capture_mode(true),
            "off" | "disable" | "disabled" => self.set_mouse_capture_mode(false),
            "status" => {
                let text = if self.mouse_capture_enabled {
                    "Mouse mode: capture ON (wheel scrolling and click interactions enabled)."
                } else {
                    "Mouse mode: capture OFF (terminal native selection/copy mode)."
                };
                self.push_output(text, OutputRole::System);
            }
            _ => {
                self.push_output("Usage: /mouse [on|off|toggle|status]", OutputRole::System);
            }
        }
    }

    fn take_mouse_capture_request(&mut self) -> Option<bool> {
        self.pending_mouse_capture.take()
    }

    /// Check for agent responses from background tasks.
    pub fn check_responses(&mut self) {
        while let Ok(resp) = self.response_rx.try_recv() {
            match resp {
                AgentResponse::Token(text) => {
                    // Accumulate per-turn token count regardless of streaming mode.
                    self.turn_stream_tokens += 1;
                    // Transition to streaming state on first token of a new phase.
                    if self.streaming_enabled
                        && matches!(
                            self.display_state,
                            DisplayState::Thinking { .. } | DisplayState::ToolExec { .. }
                        )
                    {
                        // WHY turn_stream_tokens: initialise from the running total so
                        // the status bar shows cumulative tokens even after tool-call
                        // interruptions, rather than resetting to 0 each streaming phase.
                        self.display_state = DisplayState::Streaming {
                            token_count: self.turn_stream_tokens,
                            started: Instant::now(),
                        };
                    }
                    // Keep the Streaming state's token_count in sync with the turn total.
                    if let DisplayState::Streaming {
                        ref mut token_count,
                        ..
                    } = self.display_state
                    {
                        *token_count = self.turn_stream_tokens;
                    }

                    if let Some(idx) = self.streaming_line {
                        if idx < self.output.len() {
                            self.output[idx].text.push_str(&text);
                            self.output[idx].rendered = None; // invalidate cache
                        }
                    } else {
                        self.output.push(OutputLine {
                            text: text.clone(),
                            role: OutputRole::Assistant,
                            prebuilt_spans: None,
                            rendered: None,
                        });
                        self.streaming_line = Some(self.output.len() - 1);
                        // Only auto-scroll to bottom if the user is already there
                        if self.at_bottom {
                            self.scroll_offset = 0;
                        }
                    }
                    // Accumulate response text for voice mode TTS readback.
                    self.last_agent_response_text.push_str(&text);
                    self.needs_redraw = true;
                }
                AgentResponse::Notice(text) => {
                    self.push_output(text, OutputRole::System);
                    self.needs_redraw = true;
                }
                AgentResponse::Reasoning(text) => {
                    if self.show_reasoning && !text.trim().is_empty() {
                        if let Some(idx) = self.reasoning_line {
                            if idx < self.output.len() {
                                self.output[idx].text.push_str(&text);
                                self.output[idx].rendered = None;
                            }
                        } else {
                            let line = OutputLine {
                                text: format!("🧠 Thinking\n{text}"),
                                role: OutputRole::Reasoning,
                                prebuilt_spans: None,
                                rendered: None,
                            };
                            if let Some(idx) = self.streaming_line {
                                let insert_idx = idx.min(self.output.len());
                                self.output.insert(insert_idx, line);
                                self.reasoning_line = Some(insert_idx);
                                self.streaming_line = Some(insert_idx + 1);
                            } else {
                                self.output.push(line);
                                self.reasoning_line = Some(self.output.len() - 1);
                            }
                            if self.at_bottom {
                                self.scroll_offset = 0;
                            }
                        }
                        self.needs_redraw = true;
                    }
                }
                AgentResponse::ToolExec { name, args_json } => {
                    // CRITICAL: Break the streaming buffer at the tool boundary.
                    // Without this, tokens arriving after the tool call append to
                    // the pre-tool text, visually merging text before and after
                    // the tool call into a single garbled line.
                    self.streaming_line = None;
                    // Track parallel in-flight tools — multiple ToolExec events
                    // may arrive before any ToolDone (parallel tool dispatch).
                    self.in_flight_tool_count = self.in_flight_tool_count.saturating_add(1);
                    self.display_state = DisplayState::ToolExec {
                        name: name.clone(),
                        args_json: args_json.clone(),
                        frame: 0,
                        started: Instant::now(),
                    };
                    // Push a live "in-flight" placeholder line to the output area.
                    //
                    // WHY immediately: Long tool operations (web fetch, terminal,
                    // delegate) can take 10-60 s.  Without this placeholder the
                    // output area appears frozen — only the status-bar spinner
                    // moves.  The placeholder gives the user a place in the
                    // scrollable transcript to see that work is happening, and
                    // ToolDone later upgrades it in-place with timing/result
                    // info (no layout shift).
                    let edit_snapshot = capture_local_edit_snapshot(&name, &args_json);
                    let running_spans =
                        build_tool_running_line(&name, &args_json, &self.theme.tool_emojis);
                    let line_idx = self.output.len();
                    self.output.push(OutputLine {
                        text: String::new(),
                        role: OutputRole::Tool,
                        prebuilt_spans: Some(running_spans),
                        rendered: None,
                    });
                    self.pending_tool_lines.push_back(PendingToolLine {
                        line_idx,
                        edit_snapshot,
                    });
                    if self.at_bottom {
                        self.scroll_offset = 0;
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::ToolDone {
                    name,
                    args_json,
                    result_preview,
                    duration_ms,
                    is_error,
                } => {
                    // Build the final styled completion spans.
                    let spans = build_tool_done_line(
                        &name,
                        &args_json,
                        result_preview.as_deref(),
                        duration_ms,
                        is_error,
                        &self.theme.tool_emojis,
                    );
                    // Upgrade the in-flight placeholder in-place (if present).
                    //
                    // WHY in-place: replacing the placeholder avoids appending a
                    // second line for the same tool call — the layout stays stable
                    // (no shift), and the cyan "···" naturally becomes the gold
                    // timing string without any visual flash.
                    let pending = self.pending_tool_lines.pop_front();
                    if let Some(PendingToolLine { line_idx, .. }) = pending.as_ref() {
                        if *line_idx < self.output.len() {
                            self.output[*line_idx].prebuilt_spans = Some(spans);
                            self.output[*line_idx].rendered = None; // invalidate cache
                        } else {
                            // Index out of range — fall back to append (shouldn't happen).
                            self.push_output_spans(spans, OutputRole::Tool);
                        }
                    } else {
                        // No pending placeholder (e.g. streaming disabled, or the
                        // tool fired before the feature was introduced) — append.
                        self.push_output_spans(spans, OutputRole::Tool);
                    }
                    if let Some(diff_lines) = render_edit_diff_lines(
                        &name,
                        &args_json,
                        is_error,
                        pending
                            .as_ref()
                            .and_then(|entry| entry.edit_snapshot.as_ref()),
                    ) {
                        for line in diff_lines {
                            self.push_output_spans(line, OutputRole::Tool);
                        }
                    }
                    // Decrement the in-flight counter. Only transition back to
                    // Thinking when ALL parallel tools have completed; otherwise
                    // stay in ToolExec state so the status bar stays accurate.
                    self.in_flight_tool_count = self.in_flight_tool_count.saturating_sub(1);
                    if self.in_flight_tool_count == 0 {
                        self.display_state = DisplayState::Thinking {
                            frame: 0,
                            started: Instant::now(),
                        };
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::SubAgentStart {
                    task_index,
                    task_count,
                    goal,
                } => {
                    self.progress_seq = self.progress_seq.saturating_add(1);
                    self.active_subagents.insert(
                        task_index,
                        ActiveSubagentStatus {
                            task_index,
                            task_count,
                            goal: goal.clone(),
                            last_detail: None,
                            last_seq: self.progress_seq,
                        },
                    );
                    self.streaming_line = None;
                    self.output.push(OutputLine {
                        text: String::new(),
                        role: OutputRole::Tool,
                        prebuilt_spans: Some(build_subagent_event_line(
                            task_index, task_count, "subagent", &goal, "running",
                        )),
                        rendered: None,
                    });
                    if self.at_bottom {
                        self.scroll_offset = 0;
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::SubAgentReasoning {
                    task_index,
                    task_count: _task_count,
                    text,
                } => {
                    self.progress_seq = self.progress_seq.saturating_add(1);
                    if let Some(status) = self.active_subagents.get_mut(&task_index) {
                        status.last_detail = Some(format!(
                            "thinking: {}",
                            edgecrab_core::safe_truncate(text.trim(), 72)
                        ));
                        status.last_seq = self.progress_seq;
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::SubAgentToolExec {
                    task_index,
                    task_count,
                    name,
                    args_json,
                } => {
                    self.progress_seq = self.progress_seq.saturating_add(1);
                    self.streaming_line = None;
                    let preview = crate::tool_display::extract_tool_preview(&name, &args_json);
                    let detail = if preview.is_empty() {
                        name.clone()
                    } else {
                        format!("{name}  {preview}")
                    };
                    if let Some(status) = self.active_subagents.get_mut(&task_index) {
                        status.last_detail = Some(detail.clone());
                        status.last_seq = self.progress_seq;
                    }
                    self.output.push(OutputLine {
                        text: String::new(),
                        role: OutputRole::Tool,
                        prebuilt_spans: Some(build_subagent_event_line(
                            task_index, task_count, "tool", &detail, "running",
                        )),
                        rendered: None,
                    });
                    if self.at_bottom {
                        self.scroll_offset = 0;
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::SubAgentFinish {
                    task_index,
                    task_count,
                    status,
                    duration_ms,
                    summary,
                    api_calls,
                    model,
                } => {
                    self.active_subagents.remove(&task_index);
                    self.streaming_line = None;
                    let mut parts = vec![
                        format!("{} in {:.1}s", status, duration_ms as f64 / 1000.0),
                        format!("api {api_calls}"),
                    ];
                    if let Some(model) = model.filter(|m| !m.is_empty()) {
                        parts.push(model);
                    }
                    if !summary.trim().is_empty() {
                        parts.push(
                            summary
                                .lines()
                                .next()
                                .unwrap_or_default()
                                .trim()
                                .to_string(),
                        );
                    }
                    let tone = if status == "completed" {
                        "success"
                    } else {
                        "error"
                    };
                    self.output.push(OutputLine {
                        text: String::new(),
                        role: OutputRole::Tool,
                        prebuilt_spans: Some(build_subagent_event_line(
                            task_index,
                            task_count,
                            "subagent",
                            &parts.join("  "),
                            tone,
                        )),
                        rendered: None,
                    });
                    if self.at_bottom {
                        self.scroll_offset = 0;
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::Done => {
                    self.is_processing = false;
                    self.streaming_line = None;
                    self.reasoning_line = None;
                    // Reset per-turn streaming counters for next turn.
                    self.in_flight_tool_count = 0;
                    self.active_subagents.clear();
                    self.turn_stream_tokens = 0;
                    self.pending_tool_lines.clear();
                    self.display_state = DisplayState::Idle;
                    self.last_response_time = Some(Instant::now());
                    self.turn_count += 1;
                    self.needs_redraw = true;

                    // Auto-update status bar tokens/cost from agent
                    self.auto_update_status();

                    // Voice mode: speak the response via TTS after each turn.
                    // WHY background task: TTS is async and we don't want to
                    // block the event loop or show a fake "processing" state.
                    // Mirrors hermes-agent's voice_mode readback pattern.
                    let response_text = std::mem::take(&mut self.last_agent_response_text);
                    if self.voice_mode_enabled && !response_text.is_empty() {
                        if let Some(agent) = self.agent.clone() {
                            let tts_prompt = format!(
                                "Please call the text_to_speech tool to read the following aloud: \
                                 {response_text}"
                            );
                            self.rt_handle.spawn(async move {
                                let _ = agent.chat(&tts_prompt).await;
                            });
                        }
                    }

                    if let Some(next) = self.prompt_queue.first().cloned() {
                        self.prompt_queue.remove(0);
                        self.process_input(&next);
                    }
                }
                AgentResponse::Error(err) => {
                    self.is_processing = false;
                    self.streaming_line = None;
                    self.reasoning_line = None;
                    self.in_flight_tool_count = 0;
                    self.active_subagents.clear();
                    self.turn_stream_tokens = 0;
                    self.pending_tool_lines.clear();
                    self.display_state = DisplayState::Idle;
                    self.push_output(err, OutputRole::Error);
                    self.needs_redraw = true;
                }
                AgentResponse::Clarify {
                    question,
                    choices,
                    response_tx,
                } => {
                    // Display the question prominently and wait for the user.
                    // The agent is paused — it will resume once the oneshot sender
                    // is fulfilled. We store the sender and route the user's next
                    // Enter key press to it instead of treating it as a new prompt.
                    self.display_state = DisplayState::WaitingForClarify;
                    self.push_output(format!("❓ {question}"), OutputRole::System);
                    // Render predefined choices as a numbered list so the user can
                    // type a number or their own answer. A 5th "Other" option is
                    // implied; the user may also type free-form text.
                    if let Some(ref list) = choices {
                        for (i, choice) in list.iter().enumerate() {
                            self.push_output(
                                format!("  {}. {}", i + 1, choice),
                                OutputRole::System,
                            );
                        }
                        self.push_output(
                            format!("  {}. Other (type your answer)", list.len() + 1),
                            OutputRole::System,
                        );
                    }
                    self.clarify_pending_tx = Some(response_tx);
                    self.textarea.set_block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Rgb(255, 220, 80)))
                            .title(" ❓ Reply: "),
                    );
                    self.needs_redraw = true;
                }
                AgentResponse::Approval {
                    command,
                    full_command,
                    response_tx,
                } => {
                    // Check the session-level approval cache first.
                    // SHA-256 key is the exact full_command string so permission is
                    // tight — "rm -rf /tmp/a" and "rm -rf /tmp/b" are distinct keys.
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    full_command.hash(&mut h);
                    let cache_key = format!("{:x}", h.finish());

                    if self.session_approvals.contains(&cache_key) {
                        // Already approved for this session — auto-accept.
                        let _ = response_tx.send(edgecrab_core::ApprovalChoice::Once);
                        self.needs_redraw = true;
                    } else {
                        // Surface the approval overlay.
                        self.display_state = DisplayState::WaitingForApproval {
                            command: if command.len() > 50 {
                                format!("{}…", edgecrab_core::safe_truncate(&command, 47))
                            } else {
                                command
                            },
                            full_command,
                            selected: 0,
                            show_full: false,
                        };
                        self.approval_pending_tx = Some(response_tx);
                        self.needs_redraw = true;
                    }
                }
                AgentResponse::SecretRequest {
                    var_name,
                    prompt,
                    is_sudo,
                    response_tx,
                } => {
                    // Surface the masked-input overlay.
                    self.display_state = DisplayState::SecretCapture {
                        var_name,
                        prompt,
                        is_sudo,
                        buffer: String::new(),
                    };
                    self.secret_pending_tx = Some(response_tx);
                    self.needs_redraw = true;
                }
                AgentResponse::BgOp(result) => {
                    // A background task completed — restore idle state, then act
                    // on the specific result type.
                    self.display_state = DisplayState::Idle;
                    self.needs_redraw = true;
                    match result {
                        BackgroundOpResult::ModelCatalogReady {
                            models,
                            current_model,
                        } => {
                            self.model_selector.set_items(models);
                            self.model_selector.activate_with_primary(&current_model);
                        }
                        BackgroundOpResult::SystemMsg(text) => {
                            self.push_output(text, OutputRole::System);
                        }
                        BackgroundOpResult::ModelSwitchDone { model } => {
                            self.model_name = model.clone();
                            self.update_context_window();
                            match persist_model_to_config(&model) {
                                Ok(()) => self.push_output(
                                    format!("Model switched to: {model} (saved as default for next run)"),
                                    OutputRole::System,
                                ),
                                Err(e) => self.push_output(
                                    format!("Model switched to: {model} (warning: failed to save default: {e})"),
                                    OutputRole::System,
                                ),
                            }
                        }
                        BackgroundOpResult::CompressDone { msg } => {
                            self.push_output(msg, OutputRole::System);
                        }
                    }
                }
                AgentResponse::BackgroundPromptComplete {
                    task_num,
                    task_id,
                    prompt_preview,
                    response,
                } => {
                    self.background_tasks_active.remove(&task_id);
                    let body = if response.trim().is_empty() {
                        "(No response generated)".to_string()
                    } else {
                        response
                    };
                    self.push_output(
                        format!(
                            "EdgeCrab (background #{task_num})\nTask ID: {task_id}\nPrompt: \"{prompt_preview}\"\n\n{body}"
                        ),
                        OutputRole::Assistant,
                    );
                }
                AgentResponse::BackgroundPromptProgress { task_id, text, .. } => {
                    if let Some(status) = self.background_tasks_active.get_mut(&task_id) {
                        self.progress_seq = self.progress_seq.saturating_add(1);
                        status.last_progress = Some(text.clone());
                        status.last_seq = self.progress_seq;
                        self.push_output(text, OutputRole::System);
                    }
                }
                AgentResponse::BackgroundPromptFailed {
                    task_num,
                    task_id,
                    error,
                } => {
                    self.background_tasks_active.remove(&task_id);
                    self.push_output(
                        format!(
                            "Background task #{task_num} failed\nTask ID: {task_id}\nError: {error}"
                        ),
                        OutputRole::Error,
                    );
                }
            }
        }

        // Drain cron job completion notifications from the background scheduler.
        // These arrive as pre-formatted markdown strings and are shown as
        // assistant-style output so the user knows a job completed.
        while let Ok(msg) = self.cron_rx.try_recv() {
            self.push_output(msg, OutputRole::Assistant);
            self.needs_redraw = true;
        }
    }

    /// Auto-update status bar after each agent response.
    fn auto_update_status(&mut self) {
        if let Some(agent) = self.agent.clone() {
            let snap = self
                .rt_handle
                .block_on(async { agent.session_snapshot().await });
            self.total_tokens = snap.prompt_tokens();
            self.model_name = snap.model;
            self.update_context_window();

            let usage = edgecrab_core::CanonicalUsage {
                input_tokens: snap.input_tokens,
                output_tokens: snap.output_tokens,
                cache_read_tokens: snap.cache_read_tokens,
                cache_write_tokens: snap.cache_write_tokens,
                reasoning_tokens: snap.reasoning_tokens,
            };
            let cost_result = edgecrab_core::estimate_cost(&usage, &self.model_name);
            if let Some(usd) = cost_result.amount_usd {
                self.session_cost = usd;
            }
        }
    }

    /// Handle a key event when the approval overlay is active.
    ///
    /// Choice order: [Once, Session, Always, Deny] (indices 0–3).
    /// ← / → navigate; Enter confirms; 'v' toggles full-command view; Esc = Deny.
    fn handle_approval_key(&mut self, key: crossterm::event::KeyEvent) {
        const CHOICES: usize = 4; // Once / Session / Always / Deny

        // Extract mutable fields we need while avoiding the borrow-checker
        let (selected, show_full, full_cmd_clone) = if let DisplayState::WaitingForApproval {
            ref mut selected,
            ref mut show_full,
            ref full_command,
            ..
        } = self.display_state
        {
            let full_cmd_clone = full_command.clone();
            (*selected, *show_full, full_cmd_clone)
        } else {
            return;
        };

        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                if let DisplayState::WaitingForApproval {
                    ref mut selected, ..
                } = self.display_state
                {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if let DisplayState::WaitingForApproval {
                    ref mut selected, ..
                } = self.display_state
                {
                    if *selected + 1 < CHOICES {
                        *selected += 1;
                    }
                }
            }
            KeyCode::Char('v') => {
                if let DisplayState::WaitingForApproval {
                    ref mut show_full, ..
                } = self.display_state
                {
                    *show_full = !*show_full;
                }
            }
            KeyCode::Enter => {
                let choice = match selected {
                    0 => edgecrab_core::ApprovalChoice::Once,
                    1 => edgecrab_core::ApprovalChoice::Session,
                    2 => edgecrab_core::ApprovalChoice::Always,
                    _ => edgecrab_core::ApprovalChoice::Deny,
                };

                // Cache session-level approvals so subsequent identical commands skip
                // the dialog for the rest of this session.
                if choice == edgecrab_core::ApprovalChoice::Session {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    full_cmd_clone.hash(&mut h);
                    let cache_key = format!("{:x}", h.finish());
                    self.session_approvals.insert(cache_key);
                }

                if let Some(tx) = self.approval_pending_tx.take() {
                    let _ = tx.send(choice);
                }
                self.display_state = DisplayState::Thinking {
                    frame: 0,
                    started: std::time::Instant::now(),
                };
            }
            KeyCode::Esc => {
                // Esc = deny
                if let Some(tx) = self.approval_pending_tx.take() {
                    let _ = tx.send(edgecrab_core::ApprovalChoice::Deny);
                }
                self.display_state = DisplayState::Idle;
            }
            _ => {}
        }

        let _ = (selected, show_full); // suppress unused warnings
        self.needs_redraw = true;
    }

    /// Handle a key press when the secret-capture overlay is active.
    ///
    /// - Printable characters are appended to the buffer.
    /// - Backspace deletes the last character.
    /// - Enter sends the buffer to the agent and returns to `Idle`.
    /// - Esc sends an empty string (abort) and returns to `Idle`.
    fn handle_secret_capture_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let DisplayState::SecretCapture { ref mut buffer, .. } = self.display_state {
                    buffer.push(c);
                }
            }
            KeyCode::Backspace => {
                if let DisplayState::SecretCapture { ref mut buffer, .. } = self.display_state {
                    buffer.pop();
                }
            }
            KeyCode::Enter => {
                let secret = if let DisplayState::SecretCapture { ref mut buffer, .. } =
                    self.display_state
                {
                    let s = buffer.clone();
                    // Zero the buffer immediately for security.
                    buffer.clear();
                    s
                } else {
                    String::new()
                };
                if let Some(tx) = self.secret_pending_tx.take() {
                    let _ = tx.send(secret);
                }
                self.display_state = DisplayState::Thinking {
                    frame: 0,
                    started: std::time::Instant::now(),
                };
            }
            KeyCode::Esc => {
                // Esc = abort (send empty string)
                if let DisplayState::SecretCapture { ref mut buffer, .. } = self.display_state {
                    buffer.clear();
                }
                if let Some(tx) = self.secret_pending_tx.take() {
                    let _ = tx.send(String::new());
                }
                self.display_state = DisplayState::Idle;
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    /// Advance spinner frame (called on every tick).
    fn tick_spinner(&mut self) {
        let advance_verb = match &mut self.display_state {
            DisplayState::Thinking { frame, .. } => {
                *frame = (*frame + 1) % SPINNER_FRAMES.len();
                // Advance thinking verb on each full braille rotation
                *frame == 0
            }
            DisplayState::Streaming { .. } => {
                // Token streaming — redraw handled by check_responses
                return;
            }
            DisplayState::ToolExec { frame, .. } => {
                *frame = (*frame + 1) % SPINNER_FRAMES.len();
                false
            }
            DisplayState::BgOp { frame, .. } => {
                *frame = (*frame + 1) % SPINNER_FRAMES.len();
                false
            }
            DisplayState::Idle
            | DisplayState::WaitingForClarify
            | DisplayState::WaitingForApproval { .. }
            | DisplayState::SecretCapture { .. } => {
                return; // Nothing to animate — don't force redraw
            }
        };
        if advance_verb {
            self.thinking_verb_idx = self.thinking_verb_idx.wrapping_add(1);
            // Advance kaomoji face every 3 verb changes (slower rotation)
            if self.thinking_verb_idx % 3 == 0 {
                self.kaomoji_frame_idx = self.kaomoji_frame_idx.wrapping_add(1);
            }
        }
        self.needs_redraw = true;
    }

    // ────────────────────────────────────────────────────────────────
    // Handlers (unchanged from before, just methods on the new App)
    // ────────────────────────────────────────────────────────────────

    fn handle_model_switch(&mut self, model: String) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let (provider_str, model_name) = match model.split_once('/') {
            Some((p, m)) => (p, m),
            None => {
                self.push_output(
                    format!("Invalid format: use 'provider/model-name' (e.g. copilot/gpt-4.1-mini). Got: '{model}'"),
                    OutputRole::Error,
                );
                return;
            }
        };
        // Map user-friendly provider aliases to edgequake-llm canonical names
        let canonical = match provider_str {
            "copilot" => "vscode-copilot",
            other => other,
        };
        // Special-case copilot: use VsCodeCopilotProvider::new() directly (direct API mode).
        // ProviderFactory::create_llm_provider forces proxy mode (localhost:4141).
        let new_provider = if canonical == "vscode-copilot" {
            match VsCodeCopilotProvider::new()
                .model(model_name)
                .with_vision(true) // Enable vision so copilot-vision-request header is sent
                .build()
            {
                Ok(p) => std::sync::Arc::new(p) as Arc<dyn edgequake_llm::LLMProvider>,
                Err(e) => {
                    self.push_output(
                        format!("Failed to create copilot provider: {e}"),
                        OutputRole::Error,
                    );
                    return;
                }
            }
        } else {
            match ProviderFactory::create_llm_provider(canonical, model_name) {
                Ok(p) => p,
                Err(e) => {
                    self.push_output(
                        format!("Failed to create provider '{provider_str}': {e}"),
                        OutputRole::Error,
                    );
                    return;
                }
            }
        };
        let model_clone = model.clone();
        let tx = self.response_tx.clone();
        self.display_state = DisplayState::BgOp {
            label: format!("Switching to {}…", model),
            frame: 0,
            started: Instant::now(),
        };
        self.needs_redraw = true;
        self.rt_handle.spawn(async move {
            agent.swap_model(model_clone.clone(), new_provider).await;
            let _ = tx.send(AgentResponse::BgOp(BackgroundOpResult::ModelSwitchDone {
                model: model_clone,
            }));
        });
    }

    fn handle_show_vision_model(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };

        let current_model = self.rt_handle.block_on(agent.model());
        let auxiliary = self.rt_handle.block_on(agent.auxiliary_config());
        let text = match (auxiliary.provider.as_deref(), auxiliary.model.as_deref()) {
            (Some(provider), Some(model)) => format!(
                "Vision routing:\n\
                 Dedicated vision model: {}/{}\n\
                 Chat model fallback:    {}\n\
                 Mode:                   explicit override{}{}",
                match provider {
                    "vscode-copilot" => "copilot",
                    "gemini" => "google",
                    other => other,
                },
                model,
                current_model,
                auxiliary
                    .base_url
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| format!("\nBase URL:                {value}"))
                    .unwrap_or_default(),
                auxiliary
                    .api_key_env
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| format!("\nAPI key env:             {value}"))
                    .unwrap_or_default(),
            ),
            _ => format!(
                "Vision routing:\n\
                 Dedicated vision model: auto\n\
                 Chat model fallback:    {}\n\
                 Current model vision:   {}\n\
                 Mode:                   use current model when declared vision-capable, otherwise fail over to configured vision backends",
                current_model,
                if current_model_supports_vision(&current_model) {
                    "yes"
                } else {
                    "no"
                }
            ),
        };
        self.push_output(text, OutputRole::System);
    }

    fn handle_set_vision_model(&mut self, spec: String) {
        let Some(agent) = self.require_agent() else {
            return;
        };

        let trimmed = spec.trim();
        if trimmed.eq_ignore_ascii_case("auto") || trimmed.eq_ignore_ascii_case("off") {
            let auxiliary = edgecrab_core::config::AuxiliaryConfig::default();
            let agent_clone = Arc::clone(&agent);
            self.rt_handle.block_on(async move {
                agent_clone.set_auxiliary_config(auxiliary).await;
            });
            match persist_vision_model_to_config(&edgecrab_core::config::AuxiliaryConfig::default())
            {
                Ok(()) => self.push_output(
                    "Vision model reset to auto. EdgeCrab will reuse the current chat model when it is declared vision-capable, otherwise it will fall back to configured vision backends."
                        .to_string(),
                    OutputRole::System,
                ),
                Err(err) => self.push_output(
                    format!("Vision model updated for this session, but config save failed: {err}"),
                    OutputRole::Error,
                ),
            }
            return;
        }

        let Some((parsed_provider, canonical_model)) = parse_selection_spec(trimmed) else {
            self.push_output(
                format!("Invalid format: use 'provider/model-name' or 'auto'. Got: '{trimmed}'"),
                OutputRole::Error,
            );
            return;
        };
        let provider = canonical_provider(&parsed_provider);
        let model = canonical_model;
        let display_provider = match provider.as_str() {
            "vscode-copilot" => "copilot",
            "gemini" => "google",
            other => other,
        };

        let auxiliary = edgecrab_core::config::AuxiliaryConfig {
            provider: Some(provider.clone()),
            model: Some(model.clone()),
            base_url: None,
            api_key_env: None,
        };
        let agent_clone = Arc::clone(&agent);
        self.rt_handle.block_on(async move {
            agent_clone.set_auxiliary_config(auxiliary.clone()).await;
        });
        match persist_vision_model_to_config(&edgecrab_core::config::AuxiliaryConfig {
            provider: Some(provider.clone()),
            model: Some(model.clone()),
            base_url: None,
            api_key_env: None,
        }) {
            Ok(()) => self.push_output(
                format!(
                    "Dedicated vision model set to {display_provider}/{model}. Future image analysis will prefer this backend."
                ),
                OutputRole::System,
            ),
            Err(err) => self.push_output(
                format!("Vision model updated for this session, but config save failed: {err}"),
                OutputRole::Error,
            ),
        }
    }

    fn handle_show_status(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let snap = self.agent_snapshot(&agent);
        let auxiliary = self.rt_handle.block_on(agent.auxiliary_config());
        let vision_routing = match (auxiliary.provider.as_deref(), auxiliary.model.as_deref()) {
            (Some(provider), Some(model)) => format!("{provider}/{model}"),
            _ => "auto".to_string(),
        };
        let text = format!(
            "Session status:\n\
             Session ID:  {}\n\
             Model:       {}\n\
             Vision:      {}\n\
             Messages:    {}\n\
             User turns:  {}\n\
             API calls:   {}\n\
             Budget:      {}/{} iterations remaining",
            snap.session_id.as_deref().unwrap_or("(none)"),
            snap.model,
            vision_routing,
            snap.message_count,
            snap.user_turn_count,
            snap.api_call_count,
            snap.budget_remaining,
            snap.budget_max,
        );
        self.push_output(text, OutputRole::System);
    }

    fn handle_show_cost(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let snap = self.agent_snapshot(&agent);
        let total = snap.input_tokens + snap.output_tokens;
        let usage = edgecrab_core::CanonicalUsage {
            input_tokens: snap.input_tokens,
            output_tokens: snap.output_tokens,
            cache_read_tokens: snap.cache_read_tokens,
            cache_write_tokens: snap.cache_write_tokens,
            reasoning_tokens: snap.reasoning_tokens,
        };
        let cost_result = edgecrab_core::estimate_cost(&usage, &snap.model);
        let cost_line = match cost_result.amount_usd {
            Some(usd) => format!("${:.6} ({})", usd, cost_result.label),
            None => cost_result.label.clone(),
        };
        let text = format!(
            "Token usage & cost:\n\
             Input tokens:       {}\n\
             Output tokens:      {}\n\
             Cache read tokens:  {}\n\
             Cache write tokens: {}\n\
             Reasoning tokens:   {}\n\
             Total tokens:       {}\n\
             API calls:          {}\n\
             \n\
             Estimated cost: {}",
            snap.input_tokens,
            snap.output_tokens,
            snap.cache_read_tokens,
            snap.cache_write_tokens,
            snap.reasoning_tokens,
            total,
            snap.api_call_count,
            cost_line,
        );
        self.push_output(text, OutputRole::System);
        self.total_tokens = snap.prompt_tokens();
        if let Some(usd) = cost_result.amount_usd {
            self.session_cost = usd;
        }
    }

    fn handle_show_usage(&mut self) {
        self.handle_show_cost();
    }

    fn handle_show_prompt(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let prompt = self
            .rt_handle
            .block_on(async move { agent.system_prompt().await });
        match prompt {
            Some(p) => {
                let preview = edgecrab_core::safe_truncate(&p, 2000);
                self.push_output(
                    format!("System prompt ({} chars):\n{}", p.len(), preview),
                    OutputRole::System,
                );
            }
            None => {
                self.push_output(
                    "System prompt: (not yet assembled — send a message first)",
                    OutputRole::System,
                );
            }
        }
    }

    fn handle_show_history(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let snap = self.agent_snapshot(&agent);
        let text = format!(
            "Session history:\n\
             Messages:   {}\n\
             User turns: {}\n\
             API calls:  {}\n\
             Tokens:     {} in / {} out\n\
             \nUse /export to save the full conversation as Markdown.",
            snap.message_count,
            snap.user_turn_count,
            snap.api_call_count,
            snap.input_tokens,
            snap.output_tokens,
        );
        self.push_output(text, OutputRole::System);
    }

    fn handle_save_session(&mut self, path: Option<String>) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let messages = self
            .rt_handle
            .block_on(async move { agent.messages().await });
        if messages.is_empty() {
            self.push_output("No messages to save.", OutputRole::System);
            return;
        }
        let path = path.unwrap_or_else(|| {
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            format!("edgecrab-session-{ts}.json")
        });
        let expanded = shellexpand::tilde(&path).to_string();
        match serde_json::to_string_pretty(&messages) {
            Ok(json) => match std::fs::write(&expanded, &json) {
                Ok(()) => {
                    self.push_output(
                        format!(
                            "Session saved to {} ({} messages, {} bytes)",
                            expanded,
                            messages.len(),
                            json.len()
                        ),
                        OutputRole::System,
                    );
                }
                Err(e) => self.push_output(
                    format!("Failed to write {expanded}: {e}"),
                    OutputRole::Error,
                ),
            },
            Err(e) => self.push_output(format!("Serialization error: {e}"), OutputRole::Error),
        }
    }

    fn handle_export_session(&mut self, path: Option<String>) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let (messages, snap) = self.rt_handle.block_on(async {
            let msgs = agent.messages().await;
            let s = agent.session_snapshot().await;
            (msgs, s)
        });
        if messages.is_empty() {
            self.push_output("No messages to export.", OutputRole::System);
            return;
        }
        let path = path.unwrap_or_else(|| {
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            format!("edgecrab-conversation-{ts}.md")
        });
        let expanded = shellexpand::tilde(&path).to_string();
        let mut md = format!(
            "# EdgeCrab Conversation\n\nModel: {}\n\n---\n\n",
            snap.model
        );
        for msg in &messages {
            let role = msg.role.as_str();
            let content = msg.text_content();
            md.push_str(&format!("## {}\n\n{}\n\n", role, content));
        }
        match std::fs::write(&expanded, &md) {
            Ok(()) => self.push_output(
                format!("Exported to {} ({} messages)", expanded, messages.len()),
                OutputRole::System,
            ),
            Err(e) => self.push_output(
                format!("Failed to write {expanded}: {e}"),
                OutputRole::Error,
            ),
        }
    }

    fn handle_set_title(&mut self, title: String) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let t = title.clone();
        self.rt_handle.block_on(async move {
            agent.set_session_title(t).await;
        });
        self.push_output(format!("Session title set to: {title}"), OutputRole::System);
    }

    /// Open the session browser overlay: load up to 50 sessions from the DB,
    /// convert them to `SessionBrowserEntry`, activate the `FuzzySelector`.
    fn open_skin_browser(&mut self) {
        let engine = crate::skin_engine::SkinEngine::new();
        let current = self
            .session_skin
            .clone()
            .unwrap_or_else(|| "default".into());
        let entries: Vec<SkinEntry> = engine
            .list_skins()
            .into_iter()
            .map(|name| {
                let is_active = name == current;
                let desc = if is_active {
                    "active".to_string()
                } else {
                    String::new()
                };
                SkinEntry {
                    name,
                    desc,
                    is_active,
                }
            })
            .collect();
        self.skin_browser.set_items(entries);
        // Pre-select the currently active skin
        self.skin_browser.activate_with_primary(current.as_str());
        self.needs_redraw = true;
    }

    fn open_session_browser(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        if !agent.has_state_db() {
            self.push_output(
                "No state database configured (run with --session to enable)",
                OutputRole::System,
            );
            return;
        }
        match agent.list_sessions(50) {
            Ok(sessions) if sessions.is_empty() => {
                self.push_output("No saved sessions to browse.", OutputRole::System);
            }
            Ok(sessions) => {
                let entries: Vec<SessionBrowserEntry> = sessions
                    .iter()
                    .map(SessionBrowserEntry::from_summary)
                    .collect();
                self.session_browser.set_items(entries);
                self.session_browser.active = true;
                self.needs_redraw = true;
            }
            Err(e) => {
                self.push_output(format!("DB error: {e}"), OutputRole::Error);
            }
        }
    }

    fn handle_session_list(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        if !agent.has_state_db() {
            self.push_output(
                "No state database configured (run with --session to enable)",
                OutputRole::System,
            );
            return;
        }
        match agent.list_sessions(20) {
            Ok(sessions) if sessions.is_empty() => {
                self.push_output("No saved sessions.", OutputRole::System)
            }
            Ok(sessions) => {
                let mut text = format!("Sessions ({} found):\n", sessions.len());
                for s in &sessions {
                    let title = s.title.as_deref().unwrap_or("-");
                    let model = s.model.as_deref().unwrap_or("?");
                    text.push_str(&format!(
                        "  {}  {}  model={}  msgs={}\n",
                        &s.id[..s.id.len().min(8)],
                        title,
                        model,
                        s.message_count
                    ));
                }
                self.push_output(text, OutputRole::System);
            }
            Err(e) => self.push_output(format!("DB error: {e}"), OutputRole::Error),
        }
    }

    fn handle_session_delete(&mut self, id_prefix: String) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        if !agent.has_state_db() {
            self.push_output("No state database configured.", OutputRole::System);
            return;
        }
        match agent.list_sessions(100) {
            Ok(sessions) => {
                let matches: Vec<_> = sessions
                    .iter()
                    .filter(|s| s.id.starts_with(&id_prefix))
                    .collect();
                match matches.len() {
                    0 => self.push_output(
                        format!("No session matching '{id_prefix}'"),
                        OutputRole::Error,
                    ),
                    1 => {
                        let sid = matches[0].id.clone();
                        match agent.delete_session(&sid) {
                            Ok(()) => self.push_output(
                                format!("Deleted session {}", &sid[..sid.len().min(8)]),
                                OutputRole::System,
                            ),
                            Err(e) => {
                                self.push_output(format!("Delete failed: {e}"), OutputRole::Error)
                            }
                        }
                    }
                    n => self.push_output(
                        format!(
                            "Ambiguous prefix '{id_prefix}' matches {n} sessions — be more specific"
                        ),
                        OutputRole::Error,
                    ),
                }
            }
            Err(e) => self.push_output(format!("DB error: {e}"), OutputRole::Error),
        }
    }

    fn handle_resume_session(&mut self, id: Option<String>) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        if !agent.has_state_db() {
            self.push_output("No state database configured.", OutputRole::System);
            return;
        }
        let target = match id {
            Some(id_or_title) => {
                // Try resolve via DB (ID prefix, exact ID, title, lineage)
                let db = self.rt_handle.block_on(agent.state_db());
                match db {
                    Some(db) => match db.resolve_session(&id_or_title) {
                        Ok(Some(resolved)) => resolved,
                        Ok(None) => {
                            self.push_output(
                                format!("No session matching '{id_or_title}'"),
                                OutputRole::Error,
                            );
                            return;
                        }
                        Err(e) => {
                            self.push_output(format!("DB error: {e}"), OutputRole::Error);
                            return;
                        }
                    },
                    None => {
                        self.push_output("No state database configured.", OutputRole::System);
                        return;
                    }
                }
            }
            None => match agent.list_sessions(1) {
                Ok(sessions) if !sessions.is_empty() => sessions[0].id.clone(),
                Ok(_) => {
                    self.push_output("No saved sessions.", OutputRole::System);
                    return;
                }
                Err(e) => {
                    self.push_output(format!("DB error: {e}"), OutputRole::Error);
                    return;
                }
            },
        };

        // Build a recap of the conversation before loading it
        let restored = self.rt_handle.block_on(async {
            agent.restore_session(&target).await?;
            let messages = agent.messages().await;
            let snap = agent.session_snapshot().await;
            Ok::<_, edgecrab_types::AgentError>((messages, snap))
        });
        match restored {
            Ok((messages, snap)) => {
                // Show conversation recap before loading
                let recap = build_session_recap(&messages);
                let prompt_tokens = snap.prompt_tokens();
                self.load_messages(messages);
                self.model_name = snap.model;
                self.update_context_window();
                self.total_tokens = prompt_tokens;
                if !recap.is_empty() {
                    self.push_output(recap, OutputRole::System);
                }
                self.push_output(
                    format!("Resumed session {}", &target[..target.len().min(12)]),
                    OutputRole::System,
                );
            }
            Err(e) => self.push_output(format!("Resume failed: {e}"), OutputRole::Error),
        }
    }

    fn handle_session_rename(&mut self, id_prefix: String, title: String) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        if !agent.has_state_db() {
            self.push_output("No state database configured.", OutputRole::System);
            return;
        }
        match agent.list_sessions(100) {
            Ok(sessions) => {
                let matches: Vec<_> = sessions
                    .iter()
                    .filter(|s| s.id.starts_with(&id_prefix))
                    .collect();
                match matches.len() {
                    0 => self.push_output(
                        format!("No session matching '{id_prefix}'"),
                        OutputRole::Error,
                    ),
                    1 => {
                        let sid = matches[0].id.clone();
                        match agent.rename_session(&sid, &title) {
                            Ok(()) => self.push_output(
                                format!("Renamed {} → \"{}\"", &sid[..sid.len().min(8)], title),
                                OutputRole::System,
                            ),
                            Err(e) => {
                                self.push_output(format!("Rename failed: {e}"), OutputRole::Error)
                            }
                        }
                    }
                    n => self.push_output(
                        format!("Ambiguous prefix '{id_prefix}' matches {n} sessions"),
                        OutputRole::Error,
                    ),
                }
            }
            Err(e) => self.push_output(format!("DB error: {e}"), OutputRole::Error),
        }
    }

    fn handle_session_prune(&mut self, older_than_days: u32) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        if !agent.has_state_db() {
            self.push_output("No state database configured.", OutputRole::System);
            return;
        }
        match agent.prune_sessions(older_than_days, None) {
            Ok(count) => self.push_output(
                format!("Pruned {count} ended session(s) older than {older_than_days} days."),
                OutputRole::System,
            ),
            Err(e) => self.push_output(format!("Prune failed: {e}"), OutputRole::Error),
        }
    }

    fn handle_background_prompt(&mut self, prompt: String) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let tx = self.response_tx.clone();
        self.background_task_seq = self.background_task_seq.saturating_add(1);
        let task_num = self.background_task_seq;
        let task_id = format!(
            "bg_{}_{}",
            chrono::Local::now().format("%H%M%S"),
            uuid::Uuid::new_v4().simple()
        );
        let preview = edgecrab_core::safe_truncate(&prompt, 60).to_string();
        let preview_suffix = if preview.len() < prompt.len() {
            "..."
        } else {
            ""
        };
        self.progress_seq = self.progress_seq.saturating_add(1);
        self.background_tasks_active.insert(
            task_id.clone(),
            BackgroundTaskStatus {
                preview: preview.clone(),
                last_progress: None,
                last_seq: self.progress_seq,
            },
        );
        self.push_output(
            format!(
                "🔄 Background task #{task_num} started: \"{preview}{preview_suffix}\"\nTask ID: {task_id}"
            ),
            OutputRole::System,
        );
        self.rt_handle.spawn(async move {
            let background_agent = match agent
                .fork_isolated(IsolatedAgentOptions {
                    session_id: Some(task_id.clone()),
                    quiet_mode: Some(true),
                    ..Default::default()
                })
                .await
            {
                Ok(child) => child,
                Err(e) => {
                    let _ = tx.send(AgentResponse::BackgroundPromptFailed {
                        task_num,
                        task_id,
                        error: e.to_string(),
                    });
                    return;
                }
            };

            let (event_tx, mut event_rx) =
                tokio::sync::mpsc::unbounded_channel::<edgecrab_core::StreamEvent>();
            let prompt_for_stream = prompt.clone();
            let stream_task = tokio::spawn(async move {
                background_agent
                    .chat_streaming(&prompt_for_stream, event_tx)
                    .await
            });

            let mut response = String::new();
            let mut stream_error: Option<String> = None;
            let mut last_progress: Option<String> = None;
            while let Some(event) = event_rx.recv().await {
                if let Some(text) = background_progress_text(task_num, &event) {
                    if last_progress.as_deref() != Some(text.as_str()) {
                        let _ = tx.send(AgentResponse::BackgroundPromptProgress {
                            task_id: task_id.clone(),
                            text: text.clone(),
                        });
                        last_progress = Some(text);
                    }
                }

                match event {
                    edgecrab_core::StreamEvent::Token(text) => response.push_str(&text),
                    edgecrab_core::StreamEvent::Error(err) => stream_error = Some(err),
                    edgecrab_core::StreamEvent::Done => break,
                    edgecrab_core::StreamEvent::Approval { response_tx, .. } => {
                        let _ = response_tx.send(edgecrab_core::ApprovalChoice::Deny);
                    }
                    edgecrab_core::StreamEvent::SecretRequest { response_tx, .. } => {
                        let _ = response_tx.send(String::new());
                    }
                    edgecrab_core::StreamEvent::Clarify { response_tx, .. } => {
                        let _ = response_tx.send(String::new());
                    }
                    _ => {}
                }
            }

            match stream_task.await {
                Ok(Ok(())) if stream_error.is_none() => {
                    let _ = tx.send(AgentResponse::BackgroundPromptComplete {
                        task_num,
                        task_id,
                        prompt_preview: preview,
                        response,
                    });
                }
                Ok(Ok(())) => {
                    let _ = tx.send(AgentResponse::BackgroundPromptFailed {
                        task_num,
                        task_id,
                        error: stream_error.unwrap_or_else(|| "background task failed".into()),
                    });
                }
                Ok(Err(e)) => {
                    let _ = tx.send(AgentResponse::BackgroundPromptFailed {
                        task_num,
                        task_id,
                        error: e.to_string(),
                    });
                }
                Err(e) => {
                    let _ = tx.send(AgentResponse::BackgroundPromptFailed {
                        task_num,
                        task_id,
                        error: format!("background task join error: {e}"),
                    });
                }
            }
        });
    }

    fn handle_show_skills(&mut self, args: String) {
        // Use ~/.edgecrab/skills/ — mirrors skills tool (edgecrab_home-based)
        let skills_dir = edgecrab_core::edgecrab_home().join("skills");

        let mut parts = args.trim().splitn(2, ' ');
        let subcommand = parts.next().unwrap_or("").trim();
        let operand = parts.next().unwrap_or("").trim();

        match subcommand {
            "" | "list" | "ls" => {
                if !skills_dir.exists() {
                    self.push_output(
                        format!("Skills directory not found: {}\nCreate it and add .md skill files.", skills_dir.display()),
                        OutputRole::System,
                    );
                    return;
                }
                match std::fs::read_dir(&skills_dir) {
                    Ok(entries) => {
                        let mut skills: Vec<_> = entries
                            .filter_map(|e| e.ok())
                            .filter(|e| {
                                let p = e.path();
                                p.extension().is_some_and(|ext| ext == "md")
                                    || p.is_dir()
                            })
                            .collect();
                        if skills.is_empty() {
                            self.push_output(
                                "No skills installed. Add .md files or skill directories to ~/.edgecrab/skills/\n\
                                 Run `/skills install <path>` to install a skill.",
                                OutputRole::System,
                            );
                        } else {
                            skills.sort_by_key(|e| e.file_name());
                            let mut text = format!("Skills ({}):\n", skills.len());
                            for s in &skills {
                                let fname = s.file_name();
                                let name = fname.to_string_lossy();
                                let skill_type = if s.path().is_dir() { "[dir]" } else { "[md]" };
                                text.push_str(&format!("  {skill_type} {name}\n"));
                            }
                            text.push_str("\nUsage: /skills view <name.md>  /skills install <path>");
                            self.push_output(text, OutputRole::System);
                        }
                    }
                    Err(e) => self.push_output(format!("Cannot read skills dir: {e}"), OutputRole::Error),
                }
            }

            "view" => {
                if operand.is_empty() {
                    self.push_output("Usage: /skills view <skill-name.md>", OutputRole::System);
                    return;
                }
                // Try the operand as-is, then with .md appended, then as a directory
                let candidates = vec![
                    skills_dir.join(operand),
                    skills_dir.join(format!("{operand}.md")),
                    skills_dir.join(operand).join("SKILL.md"),
                ];
                let skill_file = candidates.into_iter().find(|p| p.is_file());
                match skill_file {
                    Some(path) => {
                        match std::fs::read_to_string(&path) {
                            Ok(content) => {
                                let header = format!("=== {} ===\n", path.file_name().unwrap_or_default().to_string_lossy());
                                self.push_output(format!("{header}{content}"), OutputRole::System);
                            }
                            Err(e) => self.push_output(format!("Cannot read skill: {e}"), OutputRole::Error),
                        }
                    }
                    None => {
                        self.push_output(
                            format!("Skill '{}' not found in {}", operand, skills_dir.display()),
                            OutputRole::Error,
                        );
                    }
                }
            }

            "install" => {
                if operand.is_empty() {
                    self.push_output(
                        "Usage:\n\
                         /skills install <local-path>              — install local skill file/dir\n\
                         /skills install owner/repo/path/skill.md  — install from GitHub",
                        OutputRole::System,
                    );
                    return;
                }

                // Detect GitHub-style operand: at least 2 slashes, not an absolute path
                let looks_like_github = !operand.starts_with('/')
                    && !operand.starts_with('.')
                    && !operand.starts_with('~')
                    && operand.matches('/').count() >= 2
                    && !std::path::Path::new(operand).exists();

                if looks_like_github {
                    // Parse: owner/repo/path
                    let mut parts_gh = operand.splitn(3, '/');
                    let owner = parts_gh.next().unwrap_or("");
                    let repo  = parts_gh.next().unwrap_or("");
                    let path  = parts_gh.next().unwrap_or("");

                    if owner.is_empty() || repo.is_empty() || path.is_empty() {
                        self.push_output(
                            "GitHub format: /skills install owner/repo/path/to/skill.md",
                            OutputRole::Error,
                        );
                        return;
                    }

                    let skills_dir_c = skills_dir.clone();
                    let owner  = owner.to_string();
                    let repo   = repo.to_string();
                    let path   = path.to_string();

                    self.push_output(
                        format!("Fetching skill from github.com/{owner}/{repo}/{path} …"),
                        OutputRole::System,
                    );

                    // Run network fetch in async context
                    let result: Result<String, String> = self.rt_handle.block_on(async {
                        install_skill_from_github(&owner, &repo, &path, &skills_dir_c).await
                    });

                    match result {
                        Ok(msg) => self.push_output(msg, OutputRole::System),
                        Err(e)  => self.push_output(format!("GitHub install failed: {e}"), OutputRole::Error),
                    }
                    return;
                }

                let src = std::path::Path::new(operand);
                if !src.exists() {
                    self.push_output(format!("Path not found: {operand}"), OutputRole::Error);
                    return;
                }
                // Create skills dir if needed
                if let Err(e) = std::fs::create_dir_all(&skills_dir) {
                    self.push_output(format!("Cannot create skills dir: {e}"), OutputRole::Error);
                    return;
                }
                if src.is_file() {
                    let dest = skills_dir.join(src.file_name().unwrap_or_default());
                    match std::fs::copy(src, &dest) {
                        Ok(_) => self.push_output(
                            format!("Skill installed: {}", dest.file_name().unwrap_or_default().to_string_lossy()),
                            OutputRole::System,
                        ),
                        Err(e) => self.push_output(format!("Install failed: {e}"), OutputRole::Error),
                    }
                } else if src.is_dir() {
                    let dir_name = src.file_name().unwrap_or_default();
                    let dest = skills_dir.join(dir_name);
                    match copy_dir_recursive(src, &dest) {
                        Ok(n) => self.push_output(
                            format!("Skill directory '{}' installed ({n} files).", dir_name.to_string_lossy()),
                            OutputRole::System,
                        ),
                        Err(e) => self.push_output(format!("Install failed: {e}"), OutputRole::Error),
                    }
                }
            }

            "hub" | "search" => {
                // Simple hub browser — lists skills from the default well-known taps
                let query = operand;
                self.push_output(
                    format!(
                        "Skills Hub — search '{query}'\n\
                         \n\
                         To install a skill from GitHub:\n\
                         /skills install owner/repo/path/to/skill.md\n\
                         /skills install owner/repo/skills/skill-name\n\
                         \n\
                         Example (edgecrab skills):\n\
                         /skills install raphaelmansuy/edgecrab/skills/ascii-diagram-master\n\
                         \n\
                         Set GITHUB_TOKEN env var for higher rate limits."
                    ),
                    OutputRole::System,
                );
            }

            "remove" | "uninstall" | "rm" => {
                if operand.is_empty() {
                    self.push_output("Usage: /skills remove <skill-name>", OutputRole::System);
                    return;
                }
                let candidates = vec![
                    skills_dir.join(operand),
                    skills_dir.join(format!("{operand}.md")),
                ];
                let target = candidates.into_iter().find(|p| p.exists());
                match target {
                    Some(path) if path.is_file() => {
                        match std::fs::remove_file(&path) {
                            Ok(_) => self.push_output(format!("Skill '{}' removed.", operand), OutputRole::System),
                            Err(e) => self.push_output(format!("Remove failed: {e}"), OutputRole::Error),
                        }
                    }
                    Some(path) if path.is_dir() => {
                        match std::fs::remove_dir_all(&path) {
                            Ok(_) => self.push_output(format!("Skill directory '{}' removed.", operand), OutputRole::System),
                            Err(e) => self.push_output(format!("Remove failed: {e}"), OutputRole::Error),
                        }
                    }
                    _ => self.push_output(format!("Skill '{}' not found.", operand), OutputRole::Error),
                }
            }

            _ => self.push_output(
                "Usage: /skills [list | view <name> | install <path-or-owner/repo/path> | remove <name> | hub [query]]",
                OutputRole::System,
            ),
        }
    }

    fn handle_show_tools(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let names = self.rt_handle.block_on(async { agent.tool_names().await });
        if names.is_empty() {
            self.push_output("No tools registered.", OutputRole::System);
        } else {
            let mut text = format!("Registered tools ({}):\n", names.len());
            for name in &names {
                text.push_str(&format!("  {name}\n"));
            }
            self.push_output(text, OutputRole::System);
        }
    }

    fn handle_show_toolsets(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let toolsets = self
            .rt_handle
            .block_on(async { agent.toolset_summary().await });
        if toolsets.is_empty() {
            self.push_output("No toolsets registered.", OutputRole::System);
        } else {
            let mut text = String::from("Available toolsets:\n");
            for (name, count) in &toolsets {
                text.push_str(&format!("  {name:<14} ({count} tools)\n"));
            }
            self.push_output(text, OutputRole::System);
        }
    }

    fn remove_reasoning_output_block(&mut self) {
        if let Some(idx) = self.reasoning_line.take() {
            if idx < self.output.len() {
                self.output.remove(idx);
                if let Some(stream_idx) = self.streaming_line {
                    self.streaming_line = match stream_idx.cmp(&idx) {
                        std::cmp::Ordering::Greater => Some(stream_idx - 1),
                        std::cmp::Ordering::Equal => None,
                        std::cmp::Ordering::Less => Some(stream_idx),
                    };
                }
                self.needs_redraw = true;
            }
        }
    }

    fn set_reasoning_visibility(&mut self, enabled: bool) -> anyhow::Result<()> {
        self.show_reasoning = enabled;
        if !enabled {
            self.remove_reasoning_output_block();
        }
        persist_display_preferences(Some(enabled), None)
    }

    fn set_streaming_preference(&mut self, enabled: bool) -> anyhow::Result<()> {
        self.streaming_enabled = enabled;
        if let Some(agent) = self.agent.clone() {
            self.rt_handle.block_on(async {
                agent.set_streaming(enabled).await;
            });
        }
        persist_display_preferences(None, Some(enabled))
    }

    fn handle_set_reasoning(&mut self, level: String) {
        let normalized = level.trim().to_ascii_lowercase();
        let partial_note = if self.is_processing {
            " Existing output updates immediately; full effect is guaranteed on the next prompt."
        } else {
            ""
        };

        let msg = match normalized.as_str() {
            "low" | "medium" | "high" => {
                if let Some(agent) = self.agent.clone() {
                    self.rt_handle.block_on(async {
                        agent.set_reasoning_effort(Some(normalized.clone())).await;
                    });
                }
                format!("Reasoning effort set to: {normalized}")
            }
            "show" => match self.set_reasoning_visibility(true) {
                Ok(()) => {
                    format!("Think mode: ON — reasoning will appear above answers.{partial_note}")
                }
                Err(e) => {
                    format!("Think mode: ON for this session, but saving config failed: {e}")
                }
            },
            "hide" => match self.set_reasoning_visibility(false) {
                Ok(()) => {
                    format!(
                        "Think mode: OFF — reasoning is hidden from the output pane.{partial_note}"
                    )
                }
                Err(e) => {
                    format!("Think mode: OFF for this session, but saving config failed: {e}")
                }
            },
            "" | "status" => {
                let state = if self.show_reasoning { "on" } else { "off" };
                format!("Think mode: {state}. Usage: /reasoning <low|medium|high|show|hide|status>")
            }
            _ => "Unknown reasoning option. Use: low, medium, high, show, hide, status".into(),
        };
        self.push_output(msg, OutputRole::System);
    }

    fn handle_set_streaming(&mut self, mode: String) {
        let normalized = mode.trim().to_ascii_lowercase();
        let defer_note = if self.is_processing {
            " The current prompt keeps its existing behavior; this applies fully on the next prompt."
        } else {
            ""
        };

        let msg = match normalized.as_str() {
            "" | "status" => {
                let state = if self.streaming_enabled { "on" } else { "off" };
                format!("Streaming: {state}. Usage: /stream <on|off|toggle|status>")
            }
            "on" | "enable" | "enabled" => match self.set_streaming_preference(true) {
                Ok(()) => {
                    format!("Streaming: ON — live token updates are enabled.{defer_note}")
                }
                Err(e) => {
                    format!("Streaming: ON for this session, but saving config failed: {e}")
                }
            },
            "off" | "disable" | "disabled" => match self.set_streaming_preference(false) {
                Ok(()) => {
                    format!(
                        "Streaming: OFF — replies will appear as a complete answer.{defer_note}"
                    )
                }
                Err(e) => {
                    format!("Streaming: OFF for this session, but saving config failed: {e}")
                }
            },
            "toggle" => {
                if self.streaming_enabled {
                    match self.set_streaming_preference(false) {
                        Ok(()) => format!(
                            "Streaming: OFF — replies will appear as a complete answer.{defer_note}"
                        ),
                        Err(e) => format!(
                            "Streaming: OFF for this session, but saving config failed: {e}"
                        ),
                    }
                } else {
                    match self.set_streaming_preference(true) {
                        Ok(()) => {
                            format!("Streaming: ON — live token updates are enabled.{defer_note}")
                        }
                        Err(e) => {
                            format!("Streaming: ON for this session, but saving config failed: {e}")
                        }
                    }
                }
            }
            _ => "Unknown streaming option. Use: on, off, toggle, status".into(),
        };
        self.push_output(msg, OutputRole::System);
    }

    /// Spawn model catalog discovery in the background.
    /// Sets `BgOp` spinner immediately, opens the model selector when done.
    /// If a background op is already running this is a no-op.
    fn refresh_model_selector_catalog(&mut self) {
        if matches!(self.display_state, DisplayState::BgOp { .. }) {
            return; // already loading
        }

        let mut providers: Vec<String> = vec![
            "openrouter".to_string(),
            "ollama".to_string(),
            "lmstudio".to_string(),
        ];
        if let Some((provider, _)) = self.model_name.split_once('/') {
            let provider = provider.to_lowercase();
            if !providers.iter().any(|p| p == &provider) {
                providers.push(provider);
            }
        }
        let current_model = self.model_name.clone();
        let tx = self.response_tx.clone();

        self.display_state = DisplayState::BgOp {
            label: "Loading models…".to_string(),
            frame: 0,
            started: Instant::now(),
        };
        self.needs_redraw = true;

        self.rt_handle.spawn(async move {
            let static_catalog = ModelCatalog::grouped_catalog();
            let discovered = model_discovery::discover_multiple(&providers).await;
            let merged =
                model_discovery::merge_grouped_catalog_with_dynamic(&static_catalog, &discovered);
            let mut all_models: Vec<ModelEntry> = Vec::new();
            for (provider, models) in merged {
                for model in models {
                    all_models.push(ModelEntry {
                        display: format!("{provider}/{model}"),
                        provider: provider.clone(),
                        detail: model.clone(),
                        model_name: model,
                    });
                }
            }
            all_models.sort_by(|a, b| a.display.cmp(&b.display));
            let _ = tx.send(AgentResponse::BgOp(BackgroundOpResult::ModelCatalogReady {
                models: all_models,
                current_model,
            }));
        });
    }

    fn open_vision_model_selector(&mut self) {
        let current_model = self.model_name.clone();
        let auxiliary = self
            .agent
            .as_ref()
            .map(|agent| self.rt_handle.block_on(agent.auxiliary_config()))
            .unwrap_or_default();
        let dynamic_providers = vec!["ollama".to_string(), "lmstudio".to_string()];
        let dynamic_models = self
            .rt_handle
            .block_on(model_discovery::discover_multiple(&dynamic_providers));
        let dynamic_pairs: Vec<(String, Vec<String>)> = dynamic_models
            .into_iter()
            .map(|entry| (entry.provider, entry.models))
            .collect();

        let auto_detail = if current_model_supports_vision(&current_model) {
            format!("Auto policy - current chat model {current_model} is vision-capable")
        } else {
            format!(
                "Auto policy — reuse {current_model} when possible, otherwise fall back to configured vision backends"
            )
        };

        let mut entries = vec![ModelEntry {
            display: "auto".to_string(),
            provider: "policy".to_string(),
            model_name: "auto".to_string(),
            detail: auto_detail,
        }];

        entries.extend(
            available_vision_model_options_with_dynamic(&dynamic_pairs)
                .into_iter()
                .map(|option| ModelEntry {
                    display: option.selection_spec,
                    provider: option.provider,
                    model_name: option.model,
                    detail: option.detail,
                }),
        );

        let current_primary = match (auxiliary.provider.as_deref(), auxiliary.model.as_deref()) {
            (Some(provider), Some(model))
                if !provider.trim().is_empty() && !model.trim().is_empty() =>
            {
                let provider = match provider.trim() {
                    "vscode-copilot" => "copilot",
                    "gemini" => "google",
                    other => other,
                };
                format!("{}/{}", provider, model.trim())
            }
            _ => "auto".to_string(),
        };

        self.vision_model_selector.set_items(entries);
        self.vision_model_selector
            .activate_with_primary(&current_primary);
    }

    fn handle_list_models(&mut self, args: String) {
        if matches!(self.display_state, DisplayState::BgOp { .. }) {
            self.push_output("Model discovery already in progress…", OutputRole::System);
            return;
        }
        let static_catalog = ModelCatalog::grouped_catalog();
        let known_providers = ModelCatalog::provider_ids();
        let current = self.model_name.clone();
        let trimmed = args.trim();

        if let Some(refresh_target) = trimmed.strip_prefix("refresh") {
            let target = refresh_target.trim().to_lowercase();
            let providers: Vec<String> = if target.is_empty() || target == "all" {
                vec![
                    "openrouter".to_string(),
                    "ollama".to_string(),
                    "lmstudio".to_string(),
                ]
            } else {
                vec![target.clone()]
            };
            let tx = self.response_tx.clone();
            self.display_state = DisplayState::BgOp {
                label: "Discovering models…".to_string(),
                frame: 0,
                started: Instant::now(),
            };
            self.needs_redraw = true;
            self.rt_handle.spawn(async move {
                let discovered = model_discovery::discover_multiple(&providers).await;
                let mut text = String::from("Model discovery refresh:\n\n");
                for entry in discovered {
                    let source = match entry.source {
                        DiscoverySource::Live => "live API",
                        DiscoverySource::Cache => "cache",
                        DiscoverySource::Static => "static catalog",
                    };
                    text.push_str(&format!(
                        "  {}/{} models ({})\n",
                        entry.provider,
                        entry.models.len(),
                        source
                    ));
                }
                text.push_str(
                    "\nUse /models <provider> to inspect the list, or /model to open selector.",
                );
                let _ = tx.send(AgentResponse::BgOp(BackgroundOpResult::SystemMsg(text)));
            });
            return;
        }

        let filter = trimmed.to_lowercase();
        let is_exact_provider = !filter.is_empty() && known_providers.iter().any(|p| p == &filter);

        if is_exact_provider {
            let tx = self.response_tx.clone();
            let filter_owned = filter.clone();
            self.display_state = DisplayState::BgOp {
                label: format!("Discovering {filter}…"),
                frame: 0,
                started: Instant::now(),
            };
            self.needs_redraw = true;
            self.rt_handle.spawn(async move {
                let discovered = model_discovery::discover_provider_models(&filter_owned).await;
                let source = match discovered.source {
                    DiscoverySource::Live => "live API",
                    DiscoverySource::Cache => "cache",
                    DiscoverySource::Static => "static catalog",
                };
                let mut text = format!(
                    "Models for '{}' (* = current, source: {}):\n\n",
                    filter_owned, source
                );
                text.push_str(&format!("  {}\n", filter_owned));
                for model in discovered.models {
                    let full = format!("{}/{}", filter_owned, model);
                    let marker = if current == full { " *" } else { "" };
                    text.push_str(&format!("    {}{}\n", full, marker));
                }
                text.push_str("\nSwitch with: /model <provider/model-name>");
                let _ = tx.send(AgentResponse::BgOp(BackgroundOpResult::SystemMsg(text)));
            });
            return;
        }

        let mut text = String::new();
        for (provider, models) in &static_catalog {
            if !filter.is_empty() && !provider.contains(&filter) {
                continue;
            }
            text.push_str(&format!("  {}\n", provider));
            for model in models {
                let full = format!("{}/{}", provider, model);
                let marker = if current == full { " *" } else { "" };
                text.push_str(&format!("    {}{}\n", full, marker));
            }
            text.push('\n');
        }

        if text.is_empty() {
            text = format!(
                "No provider matching '{}'. Use /models without args to see all.\n",
                filter
            );
        } else {
            let header = if filter.is_empty() {
                "Available models (* = current):\n\n".to_string()
            } else {
                format!("Models for '{}' (* = current):\n\n", filter)
            };
            text = format!(
                "{}{}Tip: /models <provider> uses dynamic discovery when available.\nTip: /models refresh [provider|all]\n\nSwitch with: /model <provider/model-name>",
                header, text
            );
        }

        self.push_output(text, OutputRole::System);
    }

    // ─── Wired slash command handlers (Phase 8.1) ───────────────────

    /// Handle `/cron [subcommand] [args]` from the TUI input bar.
    ///
    /// Supported subcommands (parity with `hermes cron`):
    ///   list [--all]             — show scheduled jobs (markdown)
    ///   add <schedule> <prompt>  — create a new job (alias: create)
    ///   pause <id>               — pause a job
    ///   resume <id>              — resume a paused job
    ///   run <id>                 — trigger job on next scheduler tick
    ///   remove <id>              — delete a job (alias: rm, delete)
    ///   status                   — show status summary (markdown)
    ///   help                     — full command reference (markdown)
    ///   (empty)                  — same as "status"
    ///
    /// All outputs are pushed as `OutputRole::Assistant` so the TUI
    /// markdown renderer applies full styling (headers, bold, code spans,
    /// blockquotes).
    fn handle_show_cron_status(&mut self, args: String) {
        use crate::cron_cmd;

        let parts: Vec<&str> = args.split_whitespace().collect();
        let sub = parts.first().copied().unwrap_or("");

        let result: anyhow::Result<String> = match sub {
            // ── display subcommands → markdown ──────────────────────────────
            "" | "status" => cron_cmd::status_md(),
            "list" | "ls" => {
                let show_all = parts.contains(&"--all") || parts.contains(&"-a");
                cron_cmd::list_jobs_md(show_all)
            }
            "help" => Ok(cron_cmd::cron_help_md()),

            // ── mutation subcommands ─────────────────────────────────────────
            "add" | "create" => {
                // /cron add [--deliver <target>] <schedule> <prompt...>
                // --deliver/-d may appear anywhere before the schedule/prompt.
                // Schedule may be "every 2h" (two tokens) or "0 9 * * *" (5 tokens).
                if parts.len() < 3 {
                    Err(anyhow::anyhow!(
                        "Usage: `/cron add <schedule> <prompt>`\n\n\
                         **Examples:**\n\
                         - `/cron add 30m Check the build`\n\
                         - `/cron add every 2h Summarize news`\n\
                         - `/cron add 0 9 * * * Morning briefing`\n\
                         - `/cron add --deliver telegram 0 9 * * * Morning HN summary`\n\
                         - `/cron add --deliver origin every 2h Check server status`"
                    ))
                } else {
                    // Strip --deliver/-d <value> from candidates before schedule parsing.
                    let mut deliver: Option<String> = None;
                    let raw_candidates = &parts[1..];
                    let mut stripped: Vec<&str> = Vec::new();
                    let mut skip_next = false;
                    for (i, tok) in raw_candidates.iter().enumerate() {
                        if skip_next {
                            skip_next = false;
                            continue;
                        }
                        if *tok == "--deliver" || *tok == "-d" {
                            if let Some(val) = raw_candidates.get(i + 1) {
                                deliver = Some(val.to_string());
                            }
                            skip_next = true;
                        } else if let Some(val) = tok.strip_prefix("--deliver=") {
                            deliver = Some(val.to_string());
                        } else {
                            stripped.push(tok);
                        }
                    }

                    if stripped.len() < 2 {
                        Err(anyhow::anyhow!(
                            "Usage: `/cron add <schedule> <prompt>` — both schedule and prompt are required.\n\
                             Run `/cron help` for the full reference."
                        ))
                    } else {
                        let mut found: Option<(String, String)> = None;
                        for sched_len in 1..stripped.len() {
                            let sched = stripped[..sched_len].join(" ");
                            if edgecrab_cron::parse_schedule(&sched).is_ok() {
                                let prompt = stripped[sched_len..].join(" ");
                                if !prompt.is_empty() {
                                    found = Some((sched, prompt));
                                    break;
                                }
                            }
                        }
                        match found {
                            Some((schedule, prompt)) => {
                                // Default deliver to "origin" (the TUI terminal) when the
                                // user doesn't specify --deliver. This makes the cron output
                                // appear inline in this chat rather than only saving to a file.
                                let effective_deliver = deliver.as_deref().or(Some("origin"));
                                cron_cmd::create_job_text(
                                    &schedule,
                                    &prompt,
                                    None,
                                    &[],
                                    None,
                                    effective_deliver,
                                )
                            }
                            None => Err(anyhow::anyhow!(
                                "Could not parse schedule from: `{}`\n\n\
                             Try: `/cron add 30m <prompt>`  or  `/cron add every 2h <prompt>`\n\
                             Run `/cron help` for schedule format reference.",
                                stripped.join(" ")
                            )),
                        }
                    } // else stripped.len() >= 2
                }
            }
            "pause" => {
                let id = parts.get(1).copied().unwrap_or("");
                if id.is_empty() {
                    Err(anyhow::anyhow!("Usage: `/cron pause <job_id>`"))
                } else {
                    cron_cmd::pause_job_text(id)
                }
            }
            "resume" => {
                let id = parts.get(1).copied().unwrap_or("");
                if id.is_empty() {
                    Err(anyhow::anyhow!("Usage: `/cron resume <job_id>`"))
                } else {
                    cron_cmd::resume_job_text(id)
                }
            }
            "run" | "trigger" => {
                let id = parts.get(1).copied().unwrap_or("");
                if id.is_empty() {
                    Err(anyhow::anyhow!("Usage: `/cron run <job_id>`"))
                } else {
                    cron_cmd::trigger_job_text(id)
                }
            }
            "remove" | "rm" | "delete" => {
                let id = parts.get(1).copied().unwrap_or("");
                if id.is_empty() {
                    Err(anyhow::anyhow!("Usage: `/cron remove <job_id>`"))
                } else {
                    cron_cmd::remove_job_text(id)
                }
            }
            other => Err(anyhow::anyhow!(
                "Unknown cron subcommand `{other}`.\n\n\
                 Available: `list` `add` `pause` `resume` `run` `remove` `status` `help`\n\
                 Run `/cron help` for the full command reference."
            )),
        };

        // All cron output uses Assistant role so the TUI markdown renderer
        // applies full styling (H2/H3 headers, bold, inline code, blockquotes).
        match result {
            Ok(text) => self.push_output(text, OutputRole::Assistant),
            Err(e) => self.push_output(format!("cron: {e}"), OutputRole::Error),
        }
    }

    fn handle_show_plugins(&mut self) {
        let mut manager = crate::plugins::PluginManager::new();
        manager.discover_all();
        let plugins = manager.plugins();
        if plugins.is_empty() {
            self.push_output(
                "No plugins discovered.\n\
                 Install with: edgecrab plugins install <repo>",
                OutputRole::System,
            );
        } else {
            let mut text = format!("Plugins ({}):\n", plugins.len());
            for p in plugins {
                text.push_str(&format!(
                    "  {} v{}  ({}, {} tools, {} hooks)\n",
                    p.name,
                    p.version,
                    p.source,
                    p.tools.len(),
                    p.hooks.len(),
                ));
            }
            self.push_output(text, OutputRole::System);
        }
    }

    fn handle_show_platforms(&mut self) {
        let mut text = String::from("Gateway platforms:\n");
        // Webhook is always available
        text.push_str("  webhook   ✓ available (always-on HTTP adapter)\n");
        // WhatsApp: check if bridge config exists
        let wa_available = edgecrab_core::edgecrab_home()
            .join("whatsapp")
            .join("config.json")
            .exists();
        text.push_str(&format!(
            "  whatsapp  {} {}\n",
            if wa_available { "✓" } else { "✗" },
            if wa_available {
                "configured"
            } else {
                "not configured (run: edgecrab whatsapp)"
            },
        ));
        // Check env vars for other platforms
        let telegram = std::env::var("TELEGRAM_BOT_TOKEN").is_ok();
        text.push_str(&format!(
            "  telegram  {} {}\n",
            if telegram { "✓" } else { "✗" },
            if telegram {
                "token found"
            } else {
                "TELEGRAM_BOT_TOKEN not set"
            },
        ));
        let discord = std::env::var("DISCORD_BOT_TOKEN").is_ok();
        text.push_str(&format!(
            "  discord   {} {}\n",
            if discord { "✓" } else { "✗" },
            if discord {
                "token found"
            } else {
                "DISCORD_BOT_TOKEN not set"
            },
        ));
        let slack = std::env::var("SLACK_BOT_TOKEN").is_ok();
        text.push_str(&format!(
            "  slack     {} {}\n",
            if slack { "✓" } else { "✗" },
            if slack {
                "token found"
            } else {
                "SLACK_BOT_TOKEN not set"
            },
        ));
        text.push_str("\nRun `edgecrab gateway start` to launch the gateway server.");
        self.push_output(text, OutputRole::System);
    }

    fn handle_show_personality(&mut self) {
        let config = self.load_runtime_config();
        let home = edgecrab_core::edgecrab_home();
        let global_soul = home.join("SOUL.md");
        let configured = config.display.personality.trim();
        let session = self.session_personality.as_deref().unwrap_or("(none)");

        let mut text = String::from("Personalities\n");
        text.push_str(&format!(
            "SOUL.md:    {}\n",
            if global_soul.exists() {
                "~/.edgecrab/SOUL.md"
            } else {
                "(built-in fallback identity)"
            }
        ));
        text.push_str(&format!(
            "Config:     {}\n",
            if configured.is_empty() {
                "default"
            } else {
                configured
            }
        ));
        text.push_str(&format!("Session:    {session}\n\n"));
        text.push_str("Available:\n");
        text.push_str("  clear        Remove the session overlay\n");
        for (name, preview) in edgecrab_core::config::personality_catalog(&config) {
            text.push_str(&format!(
                "  {name:<12} {}\n",
                truncate_preview(&preview, 72)
            ));
        }
        text.push_str("\nUsage: /personality <name>");
        self.push_output(text, OutputRole::System);
    }

    fn handle_switch_personality(&mut self, name: String) {
        let config = self.load_runtime_config();
        let name = name.trim().to_ascii_lowercase();

        if name == "show" {
            self.handle_show_personality();
            return;
        }

        // "clear" / "default" / "none" removes the active overlay
        if matches!(name.as_str(), "clear" | "default" | "none" | "neutral") {
            self.session_personality = None;
            if let Some(agent) = self.agent.clone() {
                let configured = edgecrab_core::config::resolve_personality(
                    &config,
                    &config.display.personality,
                );
                self.rt_handle.block_on(async {
                    agent.set_personality_addon(configured).await;
                    agent
                        .inject_assistant_context(
                            "System note: the temporary personality overlay was cleared. Future replies must follow the active base persona only.",
                        )
                        .await;
                });
            }
            self.push_output("Personality overlay cleared.", OutputRole::System);
            return;
        }

        match edgecrab_core::config::resolve_personality(&config, &name) {
            Some(overlay_text) => {
                self.session_personality = Some(name.clone());
                if let Some(agent) = self.agent.clone() {
                    let note = format!(
                        "System note: personality switched to '{name}'. This supersedes any previous session style. Future replies must follow only this personality."
                    );
                    self.rt_handle.block_on(async {
                        agent.set_personality_addon(Some(overlay_text)).await;
                        agent.inject_assistant_context(&note).await;
                    });
                }
                self.push_output(
                    format!("Personality switched to '{name}'."),
                    OutputRole::System,
                );
            }
            None => {
                let presets = edgecrab_core::config::personality_catalog(&config)
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.push_output(
                    format!("Unknown personality '{name}'. Available: {presets}\nUse /personality clear to remove the current overlay."),
                    OutputRole::System,
                );
            }
        }
    }

    fn load_runtime_config(&self) -> edgecrab_core::AppConfig {
        let config_path = edgecrab_core::edgecrab_home().join("config.yaml");
        edgecrab_core::AppConfig::load_from(&config_path).unwrap_or_default()
    }

    fn personality_arg_hints(&self) -> Vec<(String, String)> {
        let mut hints = vec![
            (
                "show".to_string(),
                "Show the current personality state and available presets".to_string(),
            ),
            (
                "clear".to_string(),
                "Remove the active session personality overlay".to_string(),
            ),
        ];
        hints.extend(
            edgecrab_core::config::personality_catalog(&self.load_runtime_config())
                .into_iter()
                .map(|(name, preview)| (name, truncate_preview(&preview, 72))),
        );
        hints
    }

    fn handle_switch_skin(&mut self, name: String) {
        let name = name.trim().to_string();

        let engine = crate::skin_engine::SkinEngine::new();

        // No-arg (empty string) or "list" → open fuzzy skin browser overlay
        if name.is_empty() || name == "list" {
            self.open_skin_browser();
            return;
        }

        let available_names = engine.list_skins();
        if !available_names.contains(&name) {
            let available = available_names.join(", ");
            self.push_output(
                format!("Unknown skin '{name}'. Available: {available}"),
                OutputRole::System,
            );
            return;
        }

        {
            let skin = engine.get(&name);

            // Helper: parse hex string to ratatui Color
            fn hex_to_color(hex: &str) -> Option<ratatui::style::Color> {
                let hex = hex.trim_start_matches('#');
                if hex.len() != 6 {
                    return None;
                }
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(ratatui::style::Color::Rgb(r, g, b))
            }

            let primary =
                hex_to_color(&skin.colors.ui_accent).unwrap_or(ratatui::style::Color::White);
            let secondary =
                hex_to_color(&skin.colors.banner_accent).unwrap_or(ratatui::style::Color::Cyan);
            let tool = hex_to_color(&skin.colors.ui_warn).unwrap_or(ratatui::style::Color::Yellow);
            let error = hex_to_color(&skin.colors.ui_error).unwrap_or(ratatui::style::Color::Red);
            let system = hex_to_color(&skin.colors.ui_label).unwrap_or(ratatui::style::Color::Gray);

            // Apply colors to the live Theme
            self.theme.input_border = ratatui::style::Style::default().fg(primary);
            self.theme.output_assistant = ratatui::style::Style::default().fg(secondary);
            self.theme.status_bar_model = ratatui::style::Style::default().fg(secondary);
            self.theme.output_tool = ratatui::style::Style::default()
                .fg(tool)
                .add_modifier(ratatui::style::Modifier::DIM);
            self.theme.output_error = ratatui::style::Style::default().fg(error);
            self.theme.output_system = ratatui::style::Style::default()
                .fg(system)
                .add_modifier(ratatui::style::Modifier::ITALIC);

            // Update prompt symbol if the skin defines one
            if !skin.branding.prompt_symbol.is_empty() && skin.branding.prompt_symbol != ">>> " {
                self.theme.prompt_symbol = skin.branding.prompt_symbol.clone();
            }

            // Apply per-tool emoji overrides from the named skin
            if !skin.tool_emojis.is_empty() {
                self.theme.tool_emojis = skin.tool_emojis.clone();
            }

            // Refresh textarea border to pick up the new border color
            self.textarea.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.theme.input_border)
                    .title(format!(" {} ", self.theme.prompt_symbol.trim())),
            );
            self.textarea.set_style(self.theme.input_text);

            self.session_skin = Some(name.clone());
            self.push_output(format!("Skin switched to '{name}'."), OutputRole::System);
        }
    }

    fn handle_show_insights(&mut self) {
        let Some(agent) = self.require_agent() else {
            return;
        };
        let snap = self
            .rt_handle
            .block_on(async { agent.session_snapshot().await });

        // ── Current session ────────────────────────────────────────────
        let total_tokens = snap.input_tokens + snap.output_tokens;
        let cost = edgecrab_core::pricing::estimate_cost(
            &edgecrab_core::pricing::CanonicalUsage {
                input_tokens: snap.input_tokens,
                output_tokens: snap.output_tokens,
                cache_read_tokens: snap.cache_read_tokens,
                cache_write_tokens: snap.cache_write_tokens,
                reasoning_tokens: snap.reasoning_tokens,
            },
            &snap.model,
        );

        let mut text = String::from("── Current session ─────────────────────\n");
        text.push_str(&format!("  User turns:     {}\n", snap.user_turn_count));
        text.push_str(&format!("  Messages:       {}\n", snap.message_count));
        text.push_str(&format!("  API calls:      {}\n", snap.api_call_count));
        text.push_str(&format!("  Input tokens:   {}\n", snap.input_tokens));
        text.push_str(&format!("  Output tokens:  {}\n", snap.output_tokens));
        text.push_str(&format!("  Total tokens:   {total_tokens}\n"));
        if snap.cache_read_tokens > 0 {
            text.push_str(&format!("  Cache hit:      {}\n", snap.cache_read_tokens));
        }
        if snap.reasoning_tokens > 0 {
            text.push_str(&format!("  Reasoning:      {}\n", snap.reasoning_tokens));
        }
        text.push_str(&format!(
            "  Budget left:    {}/{}\n",
            snap.budget_remaining, snap.budget_max
        ));
        text.push_str(&format!(
            "  Est. cost:      ${:.4}\n",
            cost.amount_usd.unwrap_or(0.0)
        ));

        // ── Historical (30-day) ────────────────────────────────────────
        let db_opt = self.rt_handle.block_on(async { agent.state_db().await });
        if let Some(db) = db_opt {
            match db.query_insights(30) {
                Ok(report) if report.overview.total_sessions > 0 => {
                    let ov = &report.overview;
                    text.push_str("\n── Last 30 days (all sessions) ─────────\n");
                    text.push_str(&format!("  Sessions:       {}\n", ov.total_sessions));
                    text.push_str(&format!("  Messages:       {}\n", ov.total_messages));
                    text.push_str(&format!("  Tool calls:     {}\n", ov.total_tool_calls));
                    let hist_total = ov.total_input_tokens + ov.total_output_tokens;
                    text.push_str(&format!("  Total tokens:   {hist_total}\n"));
                    if ov.total_cache_read_tokens > 0 {
                        text.push_str(&format!(
                            "  Cache hits:     {}\n",
                            ov.total_cache_read_tokens
                        ));
                    }
                    text.push_str(&format!(
                        "  Est. cost:      ${:.2}\n",
                        ov.estimated_total_cost_usd
                    ));

                    if !report.models.is_empty() {
                        text.push_str("\n  Models:\n");
                        for m in report.models.iter().take(5) {
                            text.push_str(&format!(
                                "    {:30} {:4} sessions  ${:.2}\n",
                                m.model, m.sessions, m.estimated_cost_usd
                            ));
                        }
                    }
                    if !report.top_tools.is_empty() {
                        text.push_str("\n  Top tools:\n");
                        for t in report.top_tools.iter().take(5) {
                            text.push_str(&format!("    {:30} {} calls\n", t.name, t.count));
                        }
                    }
                    if !report.daily_activity.is_empty() {
                        text.push_str("\n  Daily activity (last 14 days):\n");
                        let peak = report
                            .daily_activity
                            .iter()
                            .map(|d| d.sessions)
                            .max()
                            .unwrap_or(1)
                            .max(1);
                        for d in &report.daily_activity {
                            let bar_len = (d.sessions * 20 / peak) as usize;
                            let bar = "█".repeat(bar_len);
                            text.push_str(&format!("    {} {:>2} {}\n", d.day, d.sessions, bar));
                        }
                    }
                }
                Ok(_) => {
                    text.push_str("\n  No historical sessions found for the last 30 days.\n");
                }
                Err(e) => {
                    text.push_str(&format!("\n  (Historical insights unavailable: {e})\n"));
                }
            }
        }

        self.push_output(text, OutputRole::System);
    }

    fn handle_copilot_auth(&mut self) {
        use edgequake_llm::providers::vscode::token::TokenManager;
        self.push_output("Checking for GitHub Copilot token...", OutputRole::System);
        let result = self.rt_handle.block_on(async {
            let manager = TokenManager::new()?;
            manager.import_vscode_token().await
        });
        match result {
            Ok(true) => {
                self.push_output(
                    "Copilot token imported from VS Code. You can now use copilot/... models.",
                    OutputRole::System,
                );
            }
            Ok(false) => {
                self.push_output(
                    "VS Code Copilot token not found (~/.config/github-copilot/hosts.json missing or no token).\nRun: edgecrab setup  — to authenticate via GitHub device code flow.",
                    OutputRole::System,
                );
            }
            Err(e) => {
                self.push_output(
                    format!("Copilot auth error: {e}\nRun: edgecrab setup  — to authenticate via GitHub device code flow."),
                    OutputRole::Error,
                );
            }
        }
    }

    fn handle_paste_clipboard(&mut self) {
        // Try text first
        match arboard::Clipboard::new().and_then(|mut cb| cb.get_text()) {
            Ok(text) if !text.is_empty() => {
                self.textarea.insert_str(&text);
                self.push_output(
                    format!("Pasted {} chars from clipboard.", text.len()),
                    OutputRole::System,
                );
                return;
            }
            _ => {}
        }

        // Try image clipboard
        match arboard::Clipboard::new().and_then(|mut cb| cb.get_image()) {
            Ok(img) => {
                // Save as PNG to ~/.edgecrab/images/
                let images_dir = edgecrab_core::config::edgecrab_home().join("images");
                if std::fs::create_dir_all(&images_dir).is_err() {
                    self.push_output("Failed to create images directory.", OutputRole::Error);
                    return;
                }
                let filename = format!(
                    "clipboard_{}.png",
                    chrono::Utc::now().format("%Y%m%d_%H%M%S")
                );
                let path = images_dir.join(&filename);

                // arboard ImageData → raw RGBA → encode as PNG
                let width = img.width;
                let height = img.height;
                let rgba_data = img.bytes.into_owned();

                // Write PNG using a minimal encoder
                match write_rgba_png(&path, &rgba_data, width as u32, height as u32) {
                    Ok(()) => {
                        self.pending_images.push(path.clone());
                        let count = self.pending_images.len();
                        self.push_output(
                            format!(
                                "📎 Image pasted from clipboard ({width}×{height}). {} image(s) attached — send a message to analyze.",
                                count
                            ),
                            OutputRole::System,
                        );
                    }
                    Err(e) => {
                        self.push_output(
                            format!("Failed to save clipboard image: {e}"),
                            OutputRole::Error,
                        );
                    }
                }
            }
            Err(_) => {
                self.push_output("Clipboard is empty (no text or image).", OutputRole::System);
            }
        }
    }

    // ─── Rollback / Checkpoint ──────────────────────────────────────
    //
    // WHY: hermes-agent wires /rollback to the checkpoint tool via the
    // agent. We mirror this by sending a natural-language request to the
    // agent which calls checkpoint(action="list") or checkpoint(action=
    // "restore", name=<name>) as appropriate.

    fn handle_rollback_checkpoint(&mut self, args: String) {
        let prompt = match args.trim() {
            "" | "list" => {
                "Please list all available checkpoints by calling the checkpoint tool with action='list'.".to_string()
            }
            name => {
                format!(
                    "Please restore the checkpoint named '{}' by calling the checkpoint tool \
                     with action='restore', name='{}'.",
                    name, name
                )
            }
        };
        let Some(agent) = self.require_agent() else {
            return;
        };
        let tx = self.response_tx.clone();
        self.push_output(
            if args.trim().is_empty() || args.trim() == "list" {
                "Listing checkpoints...".into()
            } else {
                format!("Restoring checkpoint '{}'...", args.trim())
            },
            OutputRole::System,
        );
        self.rt_handle.spawn(async move {
            match agent.chat(&prompt).await {
                Ok(resp) => {
                    let _ = tx.send(AgentResponse::Token(resp));
                    let _ = tx.send(AgentResponse::Done);
                }
                Err(e) => {
                    let _ = tx.send(AgentResponse::Error(format!("Rollback error: {e}")));
                }
            }
        });
    }

    // ─── MCP Reload ─────────────────────────────────────────────────
    //
    // WHY: `/reload-mcp` drops all cached subprocess connections so they
    // are re-established on the next mcp_list_tools / mcp_call_tool call.
    // This lets users restart or reconfigure MCP servers without
    // restarting EdgeCrab.

    fn handle_reload_mcp(&mut self) {
        edgecrab_tools::tools::mcp_client::reload_mcp_connections();
        self.push_output(
            "MCP server connections cleared.  They will be re-established on the next tool call.\n\
             (Configured via ~/.edgecrab/mcp.json or the mcp_servers section in config.yaml)",
            OutputRole::System,
        );
    }

    // ─── Voice Mode ─────────────────────────────────────────────────
    //
    // WHY mirrors hermes-agent's /voice: When voice mode is enabled,
    // the agent's text response is spoken via the `text_to_speech` tool
    // after each turn.  This provides audio feedback without blocking
    // the streaming UI.
    //
    // Unlike hermes (which uses sounddevice for push-to-talk recording),
    // EdgeCrab's voice mode is currently TTS-only.  Push-to-talk requires
    // platform audio capture (cpal) and is tracked as a future enhancement.

    fn handle_voice_mode(&mut self, args: String) {
        let trimmed = args.trim();
        // Check for "/voice tts <text>" — immediate TTS of arbitrary text
        if let Some(text) = trimmed
            .strip_prefix("tts ")
            .or_else(|| trimmed.strip_prefix("tts\t"))
        {
            let text = text.trim();
            if text.is_empty() {
                self.push_output("Usage: /voice tts <text to speak>", OutputRole::System);
                return;
            }
            let Some(agent) = self.require_agent() else {
                return;
            };
            let tx = self.response_tx.clone();
            let text_owned = text.to_string();
            self.push_output(
                format!("Speaking: {}", &text_owned[..text_owned.len().min(80)]),
                OutputRole::System,
            );
            self.rt_handle.spawn(async move {
                let tts_prompt = format!(
                    "Please use the text_to_speech tool to speak the following text aloud: {}",
                    text_owned
                );
                match agent.chat(&tts_prompt).await {
                    Ok(resp) => {
                        let _ = tx.send(AgentResponse::Token(resp));
                        let _ = tx.send(AgentResponse::Done);
                    }
                    Err(e) => {
                        let _ = tx.send(AgentResponse::Error(format!("TTS error: {e}")));
                    }
                }
            });
            return;
        }

        match trimmed {
            "on" => {
                self.voice_mode_enabled = true;
                self.push_output(
                    "Voice mode: ON — agent responses will be read aloud via TTS.\n\
                     (Requires TTS_PROVIDER or OPENAI_API_KEY for text_to_speech tool.)",
                    OutputRole::System,
                );
            }
            "off" => {
                self.voice_mode_enabled = false;
                self.push_output("Voice mode: OFF", OutputRole::System);
            }
            "status" | "" => {
                let status = if self.voice_mode_enabled { "ON" } else { "OFF" };
                self.push_output(
                    format!(
                        "Voice mode: {status}\n\
                         /voice on           — enable TTS readback\n\
                         /voice off          — disable TTS readback\n\
                         /voice tts <text>   — speak text immediately"
                    ),
                    OutputRole::System,
                );
            }
            _ => {
                self.push_output(
                    "Usage: /voice <on|off|status|tts <text>>",
                    OutputRole::System,
                );
            }
        }
    }

    /// Manage MCP OAuth Bearer tokens stored at ~/.edgecrab/mcp-tokens/.
    ///
    /// Subcommands:
    ///   set <server> <token>  — store a Bearer token for an HTTP MCP server
    ///   remove <server>       — delete stored tokens for a server
    ///   list                  — list servers with stored tokens
    fn handle_mcp_token(&mut self, args: String) {
        use edgecrab_tools::tools::mcp_client::{remove_mcp_token, write_mcp_token};

        let parts: Vec<&str> = args.trim().splitn(3, ' ').collect();
        match parts.as_slice() {
            ["set", server, token] => match write_mcp_token(server, token) {
                Ok(()) => {
                    self.push_output(
                        format!("MCP token stored for server '{server}'."),
                        OutputRole::System,
                    );
                }
                Err(e) => {
                    self.push_output(format!("Failed to store MCP token: {e}"), OutputRole::Error);
                }
            },
            ["remove", server] => {
                remove_mcp_token(server);
                self.push_output(
                    format!("MCP token removed for server '{server}'."),
                    OutputRole::System,
                );
            }
            ["list"] | [] | [""] => {
                // List servers that have a stored token by reading the tokens dir
                let dir = edgecrab_core::edgecrab_home().join("mcp-tokens");
                {
                    if dir.is_dir() {
                        let entries: Vec<String> = std::fs::read_dir(&dir)
                            .ok()
                            .into_iter()
                            .flatten()
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                            .filter_map(|e| {
                                e.path()
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().into_owned())
                            })
                            .collect();
                        if entries.is_empty() {
                            self.push_output(
                                "No MCP OAuth tokens stored.\n\
                                 Use: /mcp-token set <server> <bearer-token>",
                                OutputRole::System,
                            );
                        } else {
                            let mut out = String::from("Stored MCP OAuth tokens:\n");
                            for srv in &entries {
                                out.push_str(&format!("  {srv}\n"));
                            }
                            out.push_str("\nUsage:\n");
                            out.push_str("  /mcp-token set <server> <token>  — store token\n");
                            out.push_str("  /mcp-token remove <server>       — remove token\n");
                            self.push_output(out, OutputRole::System);
                        }
                        return;
                    }
                }
                self.push_output(
                    "No MCP OAuth tokens stored.\n\
                     Use: /mcp-token set <server> <bearer-token>",
                    OutputRole::System,
                );
            }
            _ => {
                self.push_output(
                    "Usage:\n\
                     /mcp-token set <server> <token>  — store Bearer token\n\
                     /mcp-token remove <server>        — remove stored token\n\
                     /mcp-token list                   — list servers with tokens",
                    OutputRole::System,
                );
            }
        }
    }

    // ─── Browser CDP connection ─────────────────────────────────────

    /// Handle `/browser connect|disconnect|status|tabs|recording` — manage live Chrome CDP.
    ///
    /// Exceeds hermes-agent in:
    /// - Port TCP reachability check before accepting connect
    /// - Auto-launch Chrome if not running (macOS/Linux multi-binary scan)
    /// - Startup wait loop (up to 5 s polling)
    /// - Manual instruction fallback with OS-specific command
    /// - Agent context injection via prompt_queue (model sees the switch)
    /// - `/browser status` shows live reachability + Chrome version info
    /// - `/browser tabs` lists all open Chrome tabs with title + URL
    /// - `/browser recording on|off` toggles session recording at runtime
    fn handle_browser_command(&mut self, args: String) {
        use edgecrab_tools::tools::browser::{
            auto_detect_running_chrome_cdp, cdp_override_status, chrome_launch_command,
            clear_cdp_override, close_all_sessions, get_chrome_info, get_recording_override,
            launch_chrome_for_debugging, list_cdp_tabs, probe_cdp_port, set_cdp_override,
            set_recording_override, wait_for_cdp_ready,
        };

        let sub = args.trim().to_lowercase();
        let sub = if sub.is_empty() {
            "status".to_string()
        } else {
            sub
        };

        // ── connect ──────────────────────────────────────────────────────────
        if sub.starts_with("connect") {
            // Parse optional CDP URL: /browser connect ws://host:port
            let url_arg = args
                .trim()
                .strip_prefix("connect")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("http://localhost:9222");

            // Extract host/port for probing
            let (probe_host, probe_port) = {
                let stripped = url_arg
                    .trim_start_matches("ws://")
                    .trim_start_matches("wss://")
                    .trim_start_matches("http://")
                    .trim_start_matches("https://");
                let host_port = stripped.split('/').next().unwrap_or(stripped);
                if let Some((h, p)) = host_port.rsplit_once(':') {
                    let port: u16 = p.parse().unwrap_or(9222);
                    (h.to_string(), port)
                } else if let Ok(p) = host_port.parse::<u16>() {
                    ("127.0.0.1".to_string(), p)
                } else {
                    ("127.0.0.1".to_string(), 9222u16)
                }
            };
            let is_default_endpoint =
                probe_port == 9222 && (probe_host == "127.0.0.1" || probe_host == "localhost");

            // Close existing sessions before switching backends
            close_all_sessions();

            // Step 1: check if Chrome is already listening
            let already_up = self
                .rt_handle
                .block_on(probe_cdp_port(&probe_host, probe_port));

            let chrome_ready = if already_up {
                self.push_output(
                    format!("  ✓ Chrome is already listening on port {probe_port}"),
                    OutputRole::System,
                );
                true
            } else if is_default_endpoint {
                // Step 2a: auto-detect an already-running Chrome with CDP
                //          (reads DevToolsActivePort files + port scan)
                let detected = self.rt_handle.block_on(auto_detect_running_chrome_cdp());

                if let Some(ref found_ep) = detected {
                    if found_ep.port != probe_port {
                        self.push_output(
                            format!(
                                "  ℹ Detected running Chrome with CDP on port {} \
                                 (you requested {probe_port}).\n\
                                 Tip: use `/browser connect {}` to attach to it.",
                                found_ep.port, found_ep.port
                            ),
                            OutputRole::System,
                        );
                    }
                }

                // Step 2b: try auto-launching a new headless Chrome on the requested port
                self.push_output(
                    format!(
                        "  Chrome isn't running with remote debugging on port {probe_port} — \
                         attempting to launch..."
                    ),
                    OutputRole::System,
                );
                let launched = launch_chrome_for_debugging(probe_port);
                if launched {
                    // Step 3: wait up to 5 s for the port to come up
                    let ready =
                        self.rt_handle
                            .block_on(wait_for_cdp_ready(&probe_host, probe_port, 5));
                    if ready {
                        self.push_output(
                            format!("  ✓ Chrome launched and listening on port {probe_port}"),
                            OutputRole::System,
                        );
                        true
                    } else {
                        self.push_output(
                            format!(
                                "  ⚠ Chrome launched but port {probe_port} isn't responding yet.\n\
                                 You may need to close existing Chrome windows first and retry."
                            ),
                            OutputRole::System,
                        );
                        false
                    }
                } else {
                    // Auto-launch failed — show manual instructions
                    let cmd = chrome_launch_command(probe_port);
                    self.push_output(
                        format!(
                            "  ⚠ Could not auto-launch Chrome.\n\
                             Start Chrome manually with CDP enabled:\n\n\
                             {cmd}"
                        ),
                        OutputRole::System,
                    );
                    false
                }
            } else {
                // Custom endpoint not reachable
                self.push_output(
                    format!("  ⚠ Port {probe_port} is not reachable at {probe_host}:{probe_port}"),
                    OutputRole::System,
                );
                false
            };

            match set_cdp_override(url_arg) {
                Ok(ep) => {
                    let reachable_line = if chrome_ready {
                        "  Status: ✓ reachable"
                    } else {
                        "  Status: ⚠ not yet reachable (connect again once Chrome is running)"
                    };
                    self.push_output(
                        format!(
                            "\n🌐 Browser connected to live Chrome via CDP\n\
                             Endpoint: {}:{}\n\
                             {reachable_line}\n\n\
                             All browser tools (browser_navigate, browser_click, etc.) \
                             now operate on your live Chrome instance.\n\
                             Use /browser disconnect to revert to headless mode.",
                            ep.host, ep.port
                        ),
                        OutputRole::System,
                    );

                    // Two-part context injection so the model is aware of
                    // the connection on its very next turn:
                    //
                    // 1. Append to the system prompt (affects tool-use guidance)
                    // 2. Inject a synthetic assistant message into history so
                    //    the model's own "voice" acknowledges the change and
                    //    overcomes any prior turns where it claimed no access.
                    if let Some(agent) = self.agent.clone() {
                        let system_note = "## Live Chrome Browser Connected\n\
                             The user has connected browser tools to their running Chrome \
                             browser via Chrome DevTools Protocol (CDP). From this point:\n\
                             - browser_navigate, browser_snapshot, browser_click, browser_type, \
                             and all other browser_* tools now control their REAL browser.\n\
                             - The browser already has tabs open with the user's cookies and \
                             logged-in sessions.\n\
                             - ALWAYS call browser_snapshot() first to see what page is currently \
                             open and get the actual URL before extracting or summarising content.\n\
                             - Do NOT use placeholder URLs like CURRENT_TAB — snapshot the page \
                             to get the real URL.\n\
                             - Be careful: your actions affect their real browser — don't close \
                             tabs or navigate away from important pages without asking.";
                        self.rt_handle
                            .block_on(agent.append_to_system_prompt(system_note));
                        // Injecting an assistant acknowledgment into message history
                        // ensures the model won't revert to claiming it has no
                        // browser access, even if it said so in a previous turn.
                        self.rt_handle.block_on(agent.inject_assistant_context(
                            "[Context update] My browser tools are now connected to the \
                             user's live Chrome browser via CDP. I have full access to \
                             browser_navigate, browser_snapshot, browser_click, browser_type, \
                             browser_scroll, browser_press, browser_back, browser_vision, \
                             browser_console, browser_get_images, and browser_close — and they \
                             all operate on the user's real browser with their existing sessions \
                             and cookies. I'll call browser_snapshot() before any action to see \
                             what page is currently open.",
                        ));
                    }
                }
                Err(e) => {
                    self.push_output(format!("Failed to connect: {e}"), OutputRole::Error);
                }
            }

        // ── disconnect ────────────────────────────────────────────────────────
        } else if sub == "disconnect" {
            if cdp_override_status().is_some() || std::env::var("BROWSER_CDP_URL").is_ok() {
                clear_cdp_override();
                // SAFETY: single-threaded TUI; no other thread reads this env var concurrently.
                #[allow(unsafe_code)]
                unsafe {
                    std::env::remove_var("BROWSER_CDP_URL")
                };
                close_all_sessions();
                self.push_output(
                    "🌐 Browser disconnected from live Chrome\n\
                     Browser tools reverted to default mode (local headless Chromium).",
                    OutputRole::System,
                );
                if let Some(agent) = self.agent.clone() {
                    let disconnect_note = "## Live Chrome Browser Disconnected\n\
                             The user has disconnected the browser from live Chrome. \
                             Browser tools are back to default headless Chromium mode.";
                    self.rt_handle
                        .block_on(agent.append_to_system_prompt(disconnect_note));
                    self.rt_handle.block_on(agent.inject_assistant_context(
                        "[Context update] The browser connection to live Chrome has been \
                             closed. Browser tools now use the default headless Chromium mode.",
                    ));
                }
            } else {
                self.push_output(
                    "Browser is not connected to live Chrome (already using default mode).",
                    OutputRole::System,
                );
            }

        // ── status ────────────────────────────────────────────────────────────
        } else if sub == "status" {
            let mut lines = String::new();

            if let Some(endpoint) = cdp_override_status() {
                // Check live reachability and fetch Chrome version info
                let (reachable, info) = self.rt_handle.block_on(async {
                    let ep_parts: Vec<&str> = endpoint.splitn(2, ':').collect();
                    let host = ep_parts.first().copied().unwrap_or("127.0.0.1");
                    let port: u16 = ep_parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(9222);
                    let ok = probe_cdp_port(host, port).await;
                    let info = if ok { get_chrome_info().await } else { None };
                    (ok, info)
                });

                lines.push_str("🌐 Browser: connected to live Chrome via CDP\n");
                lines.push_str(&format!("   Endpoint: {endpoint}\n"));
                if reachable {
                    lines.push_str("   Status:   ✓ reachable\n");
                    if let Some(ref ci) = info {
                        if !ci.browser.is_empty() {
                            lines.push_str(&format!("   Browser:  {}\n", ci.browser));
                        }
                        if !ci.protocol_version.is_empty() {
                            lines.push_str(&format!("   CDP:      v{}\n", ci.protocol_version));
                        }
                    }
                } else {
                    lines.push_str("   Status:   ⚠ not reachable (Chrome may have closed)\n");
                }
            } else if let Ok(url) = std::env::var("BROWSER_CDP_URL") {
                lines.push_str("🌐 Browser: CDP override via BROWSER_CDP_URL env var\n");
                lines.push_str(&format!("   Endpoint: {url}\n"));
            } else {
                // Default headless mode — show what binary is available
                lines.push_str("🌐 Browser: local headless Chrome/Chromium\n");
                // Try to get info from default endpoint (may not be running)
                let info = self.rt_handle.block_on(get_chrome_info());
                if let Some(ci) = info {
                    if !ci.browser.is_empty() {
                        lines.push_str(&format!("   Browser:  {}\n", ci.browser));
                    }
                }

                // Auto-detect any already-running Chrome with CDP
                let detected = self.rt_handle.block_on(auto_detect_running_chrome_cdp());
                if let Some(ref ep) = detected {
                    lines.push_str(&format!(
                        "   ℹ Detected running Chrome with CDP on port {} \
                         — run `/browser connect {}` to attach to it.\n",
                        ep.port, ep.port
                    ));
                }
            }

            // Show recording status
            let recording_on = get_recording_override().unwrap_or(false);
            lines.push_str(&format!(
                "   Recording: {}\n",
                if recording_on { "on ✓" } else { "off" }
            ));

            lines.push('\n');
            lines.push_str("   /browser connect ws://host:port — connect to specific endpoint\n");
            lines.push_str("   /browser disconnect            — revert to default\n");
            lines.push_str("   /browser tabs                  — list open Chrome tabs\n");
            lines.push_str("   /browser recording on|off      — toggle session recording\n");

            self.push_output(lines, OutputRole::System);

        // ── tabs ──────────────────────────────────────────────────────────────
        } else if sub == "tabs" {
            // Resolve the active endpoint (override or default)
            let (tab_host, tab_port) = if let Some(ref ep) = cdp_override_status() {
                let parts: Vec<&str> = ep.splitn(2, ':').collect();
                let h = parts.first().copied().unwrap_or("127.0.0.1").to_string();
                let p: u16 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(9222);
                (h, p)
            } else {
                ("127.0.0.1".to_string(), 9222u16)
            };

            // Probe before hitting the HTTP API so we give a specific error
            let port_up = self.rt_handle.block_on(probe_cdp_port(&tab_host, tab_port));
            if !port_up {
                let tip = if cdp_override_status().is_some() {
                    format!(
                        "Chrome is not reachable at {tab_host}:{tab_port}.\n\
                         It may have closed after you connected.\n\
                         Run /browser connect to reconnect, or /browser disconnect to revert."
                    )
                } else {
                    format!(
                        "Chrome is not running with remote debugging on port {tab_port}.\n\
                         Run /browser connect to auto-launch Chrome with CDP enabled."
                    )
                };
                self.push_output(tip, OutputRole::System);
            } else {
                let tabs = self.rt_handle.block_on(list_cdp_tabs());
                if tabs.is_empty() {
                    self.push_output(
                        "Chrome is reachable but has no open page tabs yet.\n\
                     Open a page in Chrome and try /browser tabs again.",
                        OutputRole::System,
                    );
                } else {
                    let mut text = format!("🌐 Open Chrome tabs ({}):\n\n", tabs.len());
                    for (i, tab) in tabs.iter().enumerate() {
                        if tab.tab_type == "page" {
                            text.push_str(&format!(
                                "  {:2}. {}\n      {}\n\n",
                                i + 1,
                                tab.title,
                                tab.url
                            ));
                        }
                    }
                    // Also list non-page targets (service workers, extensions)
                    let other: Vec<_> = tabs.iter().filter(|t| t.tab_type != "page").collect();
                    if !other.is_empty() {
                        text.push_str(&format!(
                            "  ({} background target(s): service workers, extensions)\n",
                            other.len()
                        ));
                    }
                    self.push_output(text, OutputRole::System);
                }
            } // end port_up branch

        // ── recording on|off ──────────────────────────────────────────────────
        } else if sub.starts_with("recording") {
            let toggle = args
                .trim()
                .to_lowercase()
                .strip_prefix("recording")
                .map(str::trim)
                .map(str::to_string)
                .unwrap_or_default();
            match toggle.as_str() {
                "on" | "true" | "1" | "yes" => {
                    set_recording_override(true);
                    self.push_output(
                        "🔴 Browser session recording: ON\n\
                         Sessions will be recorded starting from the next browser_navigate.\n\
                         Recordings saved to ~/.edgecrab/browser_recordings/",
                        OutputRole::System,
                    );
                }
                "off" | "false" | "0" | "no" => {
                    set_recording_override(false);
                    self.push_output(
                        "⚫ Browser session recording: OFF\n\
                         New sessions will not be recorded. (In-progress recordings finish normally.)",
                        OutputRole::System,
                    );
                }
                _ => {
                    let current = get_recording_override()
                        .map_or("(config default)", |v| if v { "on" } else { "off" });
                    self.push_output(
                        format!(
                            "Browser recording is currently: {current}\n\n\
                             /browser recording on   — enable session recording\n\
                             /browser recording off  — disable session recording"
                        ),
                        OutputRole::System,
                    );
                }
            }

        // ── help / unknown ────────────────────────────────────────────────────
        } else {
            self.push_output(
                "Usage: /browser <subcommand>\n\n\
                 connect [url]       Connect browser tools to your live Chrome session\n\
                 disconnect          Revert to default headless browser backend\n\
                 status              Show current browser mode, endpoint, and Chrome version\n\
                 tabs                List all open Chrome tabs with titles and URLs\n\
                 recording on|off    Toggle session recording at runtime\n\n\
                 Examples:\n\
                 /browser connect\n\
                 /browser connect ws://192.168.1.10:9222\n\
                 /browser tabs\n\
                 /browser recording on",
                OutputRole::System,
            );
        }
    }

    // ─── Rendering ──────────────────────────────────────────────────

    /// Render the full application frame.
    pub fn render(&mut self, frame: &mut Frame) {
        let max_input_height = if self.editor_mode.is_compose() {
            frame.area().height.saturating_sub(6).clamp(6, 16)
        } else {
            10
        };
        let min_input_height = if self.editor_mode.is_compose() { 5 } else { 3 };
        let textarea_height =
            (self.textarea.lines().len() as u16 + 2).clamp(min_input_height, max_input_height);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),                  // output area
                Constraint::Length(1),               // separator
                Constraint::Length(1),               // status bar
                Constraint::Length(textarea_height), // input area (dynamic height)
            ])
            .split(frame.area());

        self.render_output(frame, chunks[0]);
        // Thin horizontal separator between output and status
        let sep = Paragraph::new(Line::from("─".repeat(chunks[1].width as usize)))
            .style(Style::default().fg(Color::Rgb(60, 60, 70)));
        frame.render_widget(sep, chunks[1]);
        self.render_status_bar(frame, chunks[2]);
        self.render_input(frame, chunks[3]);

        // Model selector overlay (full screen)
        if self.model_selector.active {
            self.render_model_selector(frame, frame.area());
        }

        // Vision-model selector overlay (full screen)
        if self.vision_model_selector.active {
            self.render_vision_model_selector(frame, frame.area());
        }

        // Skill selector overlay (full screen, takes precedence over model selector)
        if self.skill_selector.active {
            self.render_skill_selector(frame, frame.area());
        }

        // Session browser overlay (full screen, same precedence as skill browser)
        if self.session_browser.active {
            self.render_session_browser(frame, frame.area());
        }

        // Skin browser overlay (full screen, same precedence as session browser)
        if self.skin_browser.active {
            self.render_skin_browser(frame, frame.area());
        }

        // Approval overlay (full screen, highest precedence)
        if matches!(self.display_state, DisplayState::WaitingForApproval { .. }) {
            self.render_approval_overlay(frame, frame.area());
        }

        // Secret capture overlay (full screen, highest precedence — masks the secret)
        if matches!(self.display_state, DisplayState::SecretCapture { .. }) {
            self.render_secret_capture_overlay(frame, frame.area());
        }
    }

    /// Render the scrollable output area with markdown formatting and a scrollbar.
    fn render_output(&mut self, frame: &mut Frame, area: Rect) {
        // ── Pass 1: ensure every OutputLine has a cached render ──────
        for ol in &mut self.output {
            if ol.rendered.is_none() {
                let rendered = if let Some(ref spans) = ol.prebuilt_spans {
                    // Pre-built spans (tool-done lines with emoji) — use directly.
                    // Ratatui measures each Span's display width via unicode-width,
                    // so emoji and wide characters align correctly.
                    vec![Line::from(spans.clone())]
                } else if ol.role == OutputRole::Assistant {
                    markdown_render::render_markdown(&ol.text)
                } else {
                    let style = match ol.role {
                        OutputRole::Assistant => unreachable!(),
                        OutputRole::Tool => Style::default()
                            .fg(Color::Rgb(255, 191, 0))
                            .add_modifier(Modifier::DIM),
                        OutputRole::System => Style::default()
                            .fg(Color::Rgb(140, 140, 150))
                            .add_modifier(Modifier::ITALIC),
                        OutputRole::Reasoning => Style::default()
                            .fg(Color::Rgb(170, 170, 190))
                            .add_modifier(Modifier::ITALIC | Modifier::DIM),
                        OutputRole::Error => Style::default().fg(Color::Rgb(239, 83, 80)),
                        OutputRole::User => Style::default().fg(Color::Rgb(255, 248, 220)),
                    };
                    ol.text
                        .lines()
                        .map(|l| Line::from(Span::styled(l.to_string(), style)))
                        .collect()
                };
                ol.rendered = Some(rendered);
            }
        }

        // ── Pass 2: build visual lines with role bars + turn separators ─
        // Each message gets a 2-char left accent: coloured "▎ " for most roles,
        // "· " (dimmed dot) for system messages. User messages get a thin
        // horizontal rule injected before them (except the very first).
        let sep_style = Style::default()
            .fg(Color::Rgb(45, 45, 58))
            .add_modifier(Modifier::DIM);
        // Dynamic separator width: fill the content column minus bar + scrollbar
        let sep_width = (area.width.saturating_sub(4) as usize).max(10);

        let mut lines: Vec<Line<'static>> = Vec::new();
        for (idx, ol) in self.output.iter().enumerate() {
            // Turn separator: thin rule before each user message that follows
            // at least one other message (marks start of a new conversation turn).
            if ol.role == OutputRole::User && idx > 0 {
                // Blank line + subtle separator rule
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled("─".repeat(sep_width), sep_style),
                ]));
                lines.push(Line::from(""));
            }

            // Role bar: the 2-char left accent column
            let (bar, bar_style): (&'static str, Style) = match ol.role {
                OutputRole::User => ("▎ ", Style::default().fg(Color::Rgb(255, 248, 220))),
                OutputRole::Assistant => ("▎ ", Style::default().fg(Color::Rgb(77, 208, 225))),
                OutputRole::Tool => ("▎ ", Style::default().fg(Color::Rgb(255, 191, 0))),
                OutputRole::Error => ("▎ ", Style::default().fg(Color::Rgb(239, 83, 80))),
                OutputRole::System => (". ", Style::default().fg(Color::Rgb(60, 60, 72))),
                OutputRole::Reasoning => ("~ ", Style::default().fg(Color::Rgb(95, 95, 115))),
            };

            // Prepend bar to every rendered sub-line
            for rendered_line in ol.rendered.as_ref().unwrap() {
                let mut spans: Vec<Span<'static>> = vec![Span::styled(bar, bar_style)];
                spans.extend(rendered_line.spans.clone());
                lines.push(Line::from(spans));
            }
        }

        // ── Scroll math ───────────────────────────────────────────────
        //
        // Scrollbar is on the LEFT (1 col).  Content starts at x+1.
        // WHY left: the content's natural reading edge is the right margin;
        // placing the scroll indicator on the left avoids it competing with
        // text flow and emoji that may appear near the right edge.
        //   area.x  ← scrollbar (1 col)
        //   area.x+1 .. area.right()  ← text content (width − 1 cols)
        //
        // content_width: used for word-wrap row count estimation.
        // Subtract 4 = 1 (scrollbar) + 1 (gap) + 2 (role bar "▎ ").
        let content_width = area.width.saturating_sub(4) as usize;

        let visual_rows: u16 = if content_width == 0 {
            lines.len() as u16
        } else {
            lines
                .iter()
                .map(|l| {
                    let w = l.width();
                    if w == 0 {
                        1u16
                    } else {
                        w.div_ceil(content_width) as u16
                    }
                })
                .sum()
        };

        let visible_height = area.height;
        let max_scroll = visual_rows.saturating_sub(visible_height);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
        let scroll = self.scroll_offset;

        self.output_visual_rows = visual_rows;
        self.output_area_height = visible_height;
        self.at_bottom = scroll == 0;

        let top_row = visual_rows.saturating_sub(visible_height + scroll);

        // ── Render: scrollbar LEFT, 1-col gap, then content ──────────
        let scrollbar_area = Rect {
            x: area.x,
            y: area.y,
            width: 1,
            height: area.height,
        };
        // Content column: skip 1 col (scrollbar) + 1 col (breathing gap).
        let content_area = Rect {
            x: area.x + 2,
            y: area.y,
            width: area.width.saturating_sub(2),
            height: area.height,
        };

        let paragraph = Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((top_row, 0));
        frame.render_widget(paragraph, content_area);

        if visual_rows > visible_height {
            let scrollbar_pos = max_scroll.saturating_sub(scroll) as usize;
            let mut scrollbar_state =
                ScrollbarState::new(max_scroll as usize).position(scrollbar_pos);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalLeft)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .track_symbol(Some("│"))
                    .thumb_symbol("█"),
                scrollbar_area,
                &mut scrollbar_state,
            );
        }

        // "Scrolled ↑" hint — anchored to right edge of the content area
        // (not the scrollbar edge) so it stays readable.
        if scroll > 0 {
            let hint = format!(" ↑{}  ^G=end  ↕scroll  PgUp/Dn ", scroll);
            let hint_len = hint
                .len()
                .min(content_area.width.saturating_sub(1) as usize);
            let hint_x = content_area.x + content_area.width.saturating_sub(hint_len as u16);
            let hint_area = Rect::new(hint_x, area.y, hint_len as u16, 1);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    hint,
                    Style::default()
                        .fg(Color::Rgb(255, 210, 50))
                        .bg(Color::Rgb(30, 30, 38))
                        .add_modifier(Modifier::BOLD),
                )),
                hint_area,
            );
        }
    }

    /// Render the status bar with spinner and color-coded metrics.
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let mut left_spans = Vec::new();

        // ── Brand badge ─────────────────────────────────────────────
        // A small copper "EC" badge anchors the left side of the status bar.
        left_spans.push(Span::styled(
            " EC ",
            Style::default()
                .fg(Color::Rgb(205, 127, 50))
                .add_modifier(Modifier::BOLD),
        ));
        left_spans.push(Span::styled(
            "│",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));

        // ── Spinner / state indicator ────────────────────────────────
        match &self.display_state {
            DisplayState::Thinking { frame: f, started } => {
                let elapsed = started.elapsed().as_secs();
                let spinner = SPINNER_FRAMES[*f % SPINNER_FRAMES.len()];

                // Rotate through skin-configurable thinking verbs
                let verb = if !self.theme.thinking_verbs.is_empty() {
                    &self.theme.thinking_verbs
                        [self.thinking_verb_idx % self.theme.thinking_verbs.len()]
                } else {
                    "thinking"
                };

                // Kaomoji face — rotates slower than the verb
                let face = if !self.theme.kaomoji_thinking.is_empty() {
                    let idx = self.kaomoji_frame_idx % self.theme.kaomoji_thinking.len();
                    self.theme.kaomoji_thinking[idx].as_str()
                } else {
                    ""
                };

                // Spinner wings (skin-configurable decorations around spinner)
                let (left_wing, right_wing) = if !self.theme.spinner_wings.is_empty() {
                    let idx = self.kaomoji_frame_idx % self.theme.spinner_wings.len();
                    let wing = &self.theme.spinner_wings[idx];
                    (wing[0].as_str(), wing[1].as_str())
                } else {
                    ("", "")
                };

                // Pad verb to VERB_DISPLAY_PAD cols so the status-bar right
                // section (model, tokens, cost) stays at a stable column as
                // the verb animates through words of different lengths.
                let verb_padded = unicode_pad_right(verb, VERB_DISPLAY_PAD);
                let core = if face.is_empty() {
                    format!("{spinner} {verb_padded}")
                } else {
                    format!("{spinner} {face} {verb_padded}")
                };

                let msg = if elapsed > 10 {
                    format!("{left_wing}{core} {elapsed}s  ^C=stop{right_wing}")
                } else if elapsed > 3 {
                    format!("{left_wing}{core} {elapsed}s{right_wing}")
                } else {
                    format!("{left_wing}{core}{right_wing}")
                };
                left_spans.push(Span::styled(
                    format!(" {msg} "),
                    Style::default().fg(Color::Rgb(255, 220, 80)),
                ));
            }
            DisplayState::Streaming {
                token_count,
                started,
            } => {
                let elapsed = started.elapsed().as_secs_f64();
                // Only show rate once enough tokens and time have elapsed to
                // produce a meaningful estimate — avoids "1t/s" flicker on start.
                let rate_str = if elapsed > 1.0 && *token_count > 5 {
                    let rate = *token_count as f64 / elapsed;
                    format!("  {rate:.0}t/s")
                } else {
                    String::new()
                };
                left_spans.push(Span::styled(
                    format!(" ▶ {token_count}tok{rate_str} "),
                    Style::default().fg(Color::Rgb(100, 230, 100)),
                ));
            }
            DisplayState::ToolExec {
                name,
                args_json,
                frame: f,
                started,
            } => {
                let spinner = SPINNER_FRAMES[*f % SPINNER_FRAMES.len()];
                let elapsed_secs = started.elapsed().as_secs();
                // Show elapsed time after 3 s (mirrors Thinking state behaviour).
                // Show the stop hint after 10 s for long-running tools.
                let time_part = if elapsed_secs >= 3 {
                    format!(" {elapsed_secs}s")
                } else {
                    String::new()
                };
                let stop_hint = if elapsed_secs >= 10 { "  ^C=stop" } else { "" };
                // When multiple tools are in-flight (parallel dispatch), show
                // the count rather than a single name — prevents misleading display
                // (e.g. showing only the last-dispatched tool while 3 run in parallel).
                let content = if self.in_flight_tool_count > 1 {
                    format!(
                        " {spinner} {} tools in parallel{time_part}{stop_hint} ",
                        self.in_flight_tool_count
                    )
                } else {
                    let icon = tool_icon(name);
                    // Tool-specific verb ("searching", "executing", "reading", …)
                    // gives more context than a generic spinner label.
                    let verb = tool_action_verb(name);
                    let preview = tool_status_preview(name, args_json);
                    format!(" {spinner} {verb} {icon} {preview}{time_part}{stop_hint} ")
                };
                left_spans.push(Span::styled(
                    content,
                    Style::default().fg(Color::Rgb(77, 208, 225)),
                ));
            }
            DisplayState::BgOp {
                label,
                frame: f,
                started,
            } => {
                let spinner = SPINNER_FRAMES[*f % SPINNER_FRAMES.len()];
                let elapsed = started.elapsed().as_secs();
                let msg = if elapsed > 3 {
                    format!(" {spinner} {label} {elapsed}s ")
                } else {
                    format!(" {spinner} {label} ")
                };
                left_spans.push(Span::styled(
                    msg,
                    Style::default().fg(Color::Rgb(180, 180, 255)),
                ));
            }
            DisplayState::Idle => {
                left_spans.push(Span::raw(" "));
            }
            DisplayState::WaitingForClarify => {
                // Agent is paused waiting for a user reply to a clarifying question.
                // Show a distinct amber label so the user knows input is expected.
                left_spans.push(Span::styled(
                    " ❓ Waiting for reply ",
                    Style::default()
                        .fg(Color::Rgb(255, 220, 80))
                        .add_modifier(Modifier::BOLD),
                ));
            }
            DisplayState::WaitingForApproval { command, .. } => {
                // Agent is waiting for a risk-graduated approval from the user.
                let short = if command.len() > 30 {
                    format!("{}…", edgecrab_core::safe_truncate(command, 27))
                } else {
                    command.clone()
                };
                left_spans.push(Span::styled(
                    format!(" ⚠  Approve: {short} "),
                    Style::default()
                        .fg(Color::Rgb(255, 140, 0))
                        .add_modifier(Modifier::BOLD),
                ));
            }
            DisplayState::SecretCapture {
                var_name, is_sudo, ..
            } => {
                // Agent is waiting for a secret value from the user.
                let label = if *is_sudo {
                    format!(" 🔒 sudo: {var_name} ")
                } else {
                    format!(" 🔑 secret: {var_name} ")
                };
                left_spans.push(Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Rgb(255, 80, 80))
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }

        left_spans.push(Span::styled(
            "│",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));

        // Model name
        left_spans.push(Span::styled(
            format!(" {} ", self.model_name),
            self.theme.status_bar_model,
        ));

        // Token count with color threshold.
        // When context window is known, show a watermark: `12.4k / 200k (7%)`.
        // Color: green → yellow → red at 50% / 80% of context window.
        let ctx_pct = self
            .context_window
            .and_then(|cw| context_usage_ratio(self.total_tokens, Some(cw)));
        let token_style = if ctx_pct.is_some_and(|p| p > 0.80) || self.total_tokens > 100_000 {
            Style::default().fg(Color::Red)
        } else if ctx_pct.is_some_and(|p| p > 0.50) || self.total_tokens > 50_000 {
            Style::default().fg(Color::Yellow)
        } else {
            self.theme.status_bar_tokens
        };
        let token_display = if let (Some(cw), Some(pct)) = (self.context_window, ctx_pct) {
            format!(
                " {}/{} ({:.0}%)",
                format_tokens(self.total_tokens),
                format_tokens(cw),
                pct * 100.0
            )
        } else {
            format!(" {}", format_tokens(self.total_tokens))
        };
        left_spans.push(Span::styled(token_display, token_style));

        // Cost with color threshold
        let cost_style = if self.session_cost >= 1.0 {
            Style::default().fg(Color::Red)
        } else if self.session_cost >= 0.10 {
            Style::default().fg(Color::Yellow)
        } else {
            self.theme.status_bar_cost
        };
        left_spans.push(Span::styled(
            format!(" ${:.4}", self.session_cost),
            cost_style,
        ));
        if !self.active_subagents.is_empty() {
            left_spans.push(Span::styled(
                " │ ",
                Style::default().fg(Color::Rgb(50, 50, 65)),
            ));
            left_spans.push(Span::styled(
                format!(" DG {} ", self.active_subagents.len()),
                Style::default()
                    .fg(Color::Rgb(10, 24, 38))
                    .bg(Color::Rgb(95, 170, 255))
                    .add_modifier(Modifier::BOLD),
            ));
            if let Some(summary) = format_subagent_status_summary(&self.active_subagents) {
                left_spans.push(Span::styled(
                    format!(" {summary} "),
                    Style::default()
                        .fg(Color::Rgb(165, 205, 245))
                        .add_modifier(Modifier::DIM),
                ));
            }
        }
        if !self.background_tasks_active.is_empty() {
            left_spans.push(Span::styled(
                " │ ",
                Style::default().fg(Color::Rgb(50, 50, 65)),
            ));
            left_spans.push(Span::styled(
                format!(" BG {} ", self.background_tasks_active.len()),
                Style::default()
                    .fg(Color::Rgb(20, 20, 28))
                    .bg(Color::Rgb(110, 180, 255))
                    .add_modifier(Modifier::BOLD),
            ));
            if let Some(summary) = format_background_status_summary(&self.background_tasks_active) {
                left_spans.push(Span::styled(
                    format!(" {summary} "),
                    Style::default()
                        .fg(Color::Rgb(180, 220, 255))
                        .add_modifier(Modifier::DIM),
                ));
            }
        }

        // Right side: keyboard hints + turn counter
        let mut right_spans = Vec::new();
        if self.turn_count > 0 {
            right_spans.push(Span::styled(
                format!(" turn {} ", self.turn_count),
                Style::default().fg(Color::Rgb(80, 90, 110)),
            ));
            right_spans.push(Span::styled(
                "│",
                Style::default().fg(Color::Rgb(50, 50, 65)),
            ));
        }
        if self.scroll_offset > 0 {
            right_spans.push(Span::styled(
                " ↑SCROLLED  ^G=↓  ↕scroll  PgUp/Dn ",
                Style::default()
                    .fg(Color::Rgb(255, 210, 50))
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            // ── Mode pill ─────────────────────────────────────────────────────
            // Always visible so the user knows the active mode and the key to
            // switch.  SCROLL (green) = mouse capture on, wheel scrolls output.
            //           SELECT (amber) = mouse capture off, native drag=copy.
            if self.mouse_capture_enabled {
                right_spans.push(Span::styled(
                    " SCROLL ",
                    Style::default()
                        .fg(Color::Rgb(20, 20, 28))
                        .bg(Color::Rgb(60, 185, 105))
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                right_spans.push(Span::styled(
                    " SELECT ",
                    Style::default()
                        .fg(Color::Rgb(20, 20, 28))
                        .bg(Color::Rgb(255, 200, 50))
                        .add_modifier(Modifier::BOLD),
                ));
            }
            // ── State-specific hints ──────────────────────────────────────────
            if !self.mouse_capture_enabled {
                right_spans.push(Span::styled(
                    " drag=copy  F6=scroll  Tab=complete  ^C=cancel ",
                    Style::default()
                        .fg(Color::Rgb(255, 210, 50))
                        .add_modifier(Modifier::BOLD),
                ));
            } else if self.clarify_pending_tx.is_some() {
                // Agent is awaiting a reply — emphasise the prompt so users know
                // their next Enter submits an answer, not a new conversation turn.
                right_spans.push(Span::styled(
                    " ↵=send reply  ^C=cancel  ↕scroll ",
                    Style::default()
                        .fg(Color::Rgb(255, 220, 80))
                        .add_modifier(Modifier::BOLD),
                ));
            } else if self.is_processing {
                right_spans.push(Span::styled(
                    " ^C=cancel  ↕scroll ",
                    Style::default().fg(Color::Rgb(70, 75, 95)),
                ));
            } else if matches!(self.editor_mode, InputEditorMode::ComposeInsert) {
                right_spans.push(Span::styled(
                    " COMPOSE ",
                    Style::default()
                        .fg(Color::Rgb(20, 20, 28))
                        .bg(Color::Rgb(90, 200, 150))
                        .add_modifier(Modifier::BOLD),
                ));
                right_spans.push(Span::styled(
                    " INSERT  ↵=newline  ^S=send  Esc=normal ",
                    Style::default().fg(Color::Rgb(90, 210, 170)),
                ));
            } else if matches!(self.editor_mode, InputEditorMode::ComposeNormal) {
                right_spans.push(Span::styled(
                    " COMPOSE ",
                    Style::default()
                        .fg(Color::Rgb(20, 20, 28))
                        .bg(Color::Rgb(255, 191, 0))
                        .add_modifier(Modifier::BOLD),
                ));
                right_spans.push(Span::styled(
                    " NORMAL  vim hjkl/wbe  i/a/o edit  ^S=send  Esc=inline ",
                    Style::default().fg(Color::Rgb(255, 210, 80)),
                ));
            } else if !self.active_skills.is_empty() {
                // Show active skill names so the user knows which skills are loaded.
                // Typing /skill_name again deactivates; /skills opens the browser.
                let names = self.active_skills.join(" + ");
                right_spans.push(Span::styled(
                    format!(" 📚 {names}  F6=select  /skill off  ^C=cancel "),
                    Style::default()
                        .fg(Color::Rgb(100, 210, 120))
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                right_spans.push(Span::styled(
                    format!(
                        " F6=select  F1=help  {}  F2=model  F3=skills  F7=vision  Tab=complete  ^C=cancel ",
                        self.inline_compose_hint()
                    ),
                    Style::default().fg(Color::Rgb(70, 75, 95)),
                ));
            }
        }

        // Build two-sided status bar
        let right_line = Line::from(right_spans);
        let right_text = right_line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        // WHY .width() not .len(): multi-byte Unicode chars (↑↓↕ = 3 bytes, 📚 = 4 bytes)
        // inflate .len() past the terminal column count, causing right_area.right() to
        // exceed the ratatui buffer bounds → panic. UnicodeWidthStr gives display cols.
        let right_width = (right_text.width() as u16).min(area.width);

        let left_area = Rect {
            width: area.width.saturating_sub(right_width),
            ..area
        };
        let right_area = Rect {
            x: area.x + area.width.saturating_sub(right_width),
            width: right_width,
            ..area
        };

        let status = Paragraph::new(Line::from(left_spans))
            .style(Style::default().bg(Color::Rgb(30, 30, 38)));
        frame.render_widget(status, left_area);

        let right_status = Paragraph::new(right_line)
            .style(Style::default().bg(Color::Rgb(30, 30, 38)))
            .alignment(Alignment::Right);
        frame.render_widget(right_status, right_area);
    }

    fn render_model_like_selector(
        &self,
        frame: &mut Frame,
        area: Rect,
        selector: &FuzzySelector<ModelEntry>,
        title: &str,
        placeholder: &str,
        count_label: &str,
    ) {
        // Clear background
        frame.render_widget(Clear, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search box
                Constraint::Min(1),    // model list
                Constraint::Length(1), // help line
            ])
            .split(area);

        // Search input
        let search_text = if selector.query.is_empty() {
            placeholder.to_string()
        } else {
            selector.query.clone()
        };
        let search_style = if selector.query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search = Paragraph::new(Line::from(Span::styled(
            format!("  > {search_text}"),
            search_style,
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(format!(" {title} ")),
        );
        frame.render_widget(search, chunks[0]);

        // Model list grouped by provider
        let max_visible = chunks[1].height as usize;
        let filtered = &selector.filtered;
        let selected = selector.selected;

        // Scroll to keep selection visible
        let scroll_start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };

        let items: Vec<ListItem> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(vis_idx, &model_idx)| {
                let entry = &selector.items[model_idx];
                let (display, provider) = (&entry.display, &entry.provider);
                let is_selected = vis_idx + scroll_start == selected;
                let style = if is_selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 70))
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(200, 200, 200))
                };
                let provider_style = if is_selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 70))
                        .fg(Color::Rgb(120, 120, 150))
                } else {
                    Style::default().fg(Color::Rgb(80, 80, 100))
                };
                let mut spans = vec![
                    Span::styled(format!("  {:<12}", provider), provider_style),
                    Span::styled(display.clone(), style),
                ];
                if !entry.detail.is_empty() && entry.detail != entry.model_name {
                    let detail_style = if is_selected {
                        Style::default()
                            .bg(Color::Rgb(50, 50, 70))
                            .fg(Color::Rgb(160, 160, 180))
                    } else {
                        Style::default().fg(Color::Rgb(110, 110, 130))
                    };
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(entry.detail.clone(), detail_style));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let model_count = filtered.len();
        let list = List::new(items).style(Style::default().bg(Color::Rgb(20, 20, 28)));
        frame.render_widget(list, chunks[1]);

        // Help line
        let help = Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Cyan)),
            Span::styled("navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(Color::Cyan)),
            Span::styled("select  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Cyan)),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{model_count} {count_label}"),
                Style::default().fg(Color::Rgb(80, 80, 100)),
            ),
        ]));
        frame.render_widget(help, chunks[2]);
    }

    /// Render the full-screen model selector overlay.
    fn render_model_selector(&self, frame: &mut Frame, area: Rect) {
        self.render_model_like_selector(
            frame,
            area,
            &self.model_selector,
            "Select Model",
            "Type to filter models... (Esc to cancel)",
            "models",
        );
    }

    /// Render the full-screen vision-model selector overlay.
    fn render_vision_model_selector(&self, frame: &mut Frame, area: Rect) {
        self.render_model_like_selector(
            frame,
            area,
            &self.vision_model_selector,
            "Select Vision Model",
            "Type to filter vision backends... (Esc to cancel)",
            "options",
        );
    }

    /// Render the full-screen skill browser overlay.
    ///
    /// UX mirrors `render_model_selector` — same search-box + list + help-line
    /// layout — so users get a consistent experience between `/model` and `/skills`.
    fn render_skill_selector(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search input
                Constraint::Min(1),    // skill table
                Constraint::Length(1), // help line
            ])
            .split(area);

        // ── Search box ───────────────────────────────────────────────
        let search_text = if self.skill_selector.query.is_empty() {
            "Type to search skills…  (Esc to cancel)".to_string()
        } else {
            self.skill_selector.query.clone()
        };
        let search_style = if self.skill_selector.query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search = Paragraph::new(Line::from(vec![
            Span::styled("  📚 ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled(search_text, search_style),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(255, 191, 0)))
                .title(" Browse Skills "),
        );
        frame.render_widget(search, chunks[0]);

        // ── Skill table ──────────────────────────────────────────────
        let max_visible = chunks[1].height as usize;
        let filtered = &self.skill_selector.filtered;
        let selected = self.skill_selector.selected;

        let scroll_start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };

        // Column widths:  type(4) + gap(1) + name(28) + gap(2) + desc(rest)
        let type_w = 4usize;
        let name_w = 28usize;

        let items: Vec<ListItem> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(vis_idx, &skill_idx)| {
                let entry = &self.skill_selector.items[skill_idx];
                let is_selected = vis_idx + scroll_start == selected;

                let bg = if is_selected {
                    Color::Rgb(40, 35, 15)
                } else {
                    Color::Rgb(20, 20, 28)
                };
                let type_tag = if entry.is_dir { " dir" } else { "  md" };
                let type_style = if is_selected {
                    Style::default().bg(bg).fg(Color::Rgb(120, 110, 60))
                } else {
                    Style::default().fg(Color::Rgb(80, 75, 40))
                };
                let name_str = unicode_pad_right(&format!("/{}", entry.name), name_w + 1);
                let name_style = if is_selected {
                    Style::default()
                        .bg(bg)
                        .fg(Color::Rgb(255, 215, 0))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(220, 200, 100))
                };
                let desc_str = unicode_trunc(&entry.desc, 60);
                let desc_style = if is_selected {
                    Style::default().bg(bg).fg(Color::Rgb(160, 150, 90))
                } else {
                    Style::default().fg(Color::Rgb(100, 95, 55))
                };

                let _ = type_w; // used for width planning

                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {type_tag}"), type_style),
                    Span::styled(format!("  {name_str}"), name_style),
                    Span::styled(format!("  {desc_str}"), desc_style),
                ]))
            })
            .collect();

        let skill_count = filtered.len();
        let list = List::new(items).style(Style::default().bg(Color::Rgb(20, 20, 28)));
        frame.render_widget(list, chunks[1]);

        // ── Help line ────────────────────────────────────────────────
        let help = Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("insert /skill-name  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{skill_count} skill(s)"),
                Style::default().fg(Color::Rgb(80, 75, 40)),
            ),
        ]));
        frame.render_widget(help, chunks[2]);
    }

    /// Render the session browser overlay (activated by F4 or `/session` with no args).
    ///
    /// Layout mirrors the skill browser: search box + list + help line.
    fn render_session_browser(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search input
                Constraint::Min(1),    // session list
                Constraint::Length(1), // help line
            ])
            .split(area);

        // ── Search box ───────────────────────────────────────────────
        let search_text = if self.session_browser.query.is_empty() {
            "Type to search sessions…  (Esc to cancel)".to_string()
        } else {
            self.session_browser.query.clone()
        };
        let search_style = if self.session_browser.query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search = Paragraph::new(Line::from(vec![
            Span::styled("  💾 ", Style::default().fg(Color::Rgb(100, 200, 255))),
            Span::styled(search_text, search_style),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(100, 200, 255)))
                .title(" Browse Sessions  [F4] "),
        );
        frame.render_widget(search, chunks[0]);

        // ── Session list ─────────────────────────────────────────────
        let max_visible = chunks[1].height as usize;
        let filtered = &self.session_browser.filtered;
        let selected = self.session_browser.selected;

        let scroll_start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };

        let date_w = 10usize;
        let title_w = 30usize;

        let items: Vec<ListItem> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(vis_idx, &entry_idx)| {
                let entry = &self.session_browser.items[entry_idx];
                let is_selected = vis_idx + scroll_start == selected;

                let bg = if is_selected {
                    Color::Rgb(15, 30, 50)
                } else {
                    Color::Rgb(20, 20, 28)
                };

                let date_style = if is_selected {
                    Style::default().bg(bg).fg(Color::Rgb(100, 150, 180))
                } else {
                    Style::default().fg(Color::Rgb(60, 90, 110))
                };
                let title_str = unicode_pad_right(&entry.display, title_w);
                let title_style = if is_selected {
                    Style::default()
                        .bg(bg)
                        .fg(Color::Rgb(130, 210, 255))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(100, 180, 220))
                };
                let subtitle_style = if is_selected {
                    Style::default().bg(bg).fg(Color::Rgb(100, 140, 160))
                } else {
                    Style::default().fg(Color::Rgb(70, 100, 120))
                };

                let _ = date_w;
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {:10}", entry.date), date_style),
                    Span::styled(format!("  {title_str}"), title_style),
                    Span::styled(format!("  {}", entry.subtitle), subtitle_style),
                ]))
            })
            .collect();

        let session_count = filtered.len();
        let list = List::new(items).style(Style::default().bg(Color::Rgb(20, 20, 28)));
        frame.render_widget(list, chunks[1]);

        // ── Help line ────────────────────────────────────────────────
        let help = Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(100, 200, 255))),
            Span::styled("navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(Color::Rgb(100, 200, 255))),
            Span::styled("resume session  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(100, 200, 255))),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{session_count} session(s)"),
                Style::default().fg(Color::Rgb(60, 100, 120)),
            ),
        ]));
        frame.render_widget(help, chunks[2]);
    }

    /// Render a masked-input overlay for secret/sudo capture.
    ///
    /// The typed buffer is shown as `••••••••` so the secret never appears in
    /// plain text. The overlay is full-screen to prevent accidental shoulder-
    /// surfing from the scrollback buffer behind it.
    fn render_secret_capture_overlay(&self, frame: &mut Frame, area: Rect) {
        let (var_name, prompt, is_sudo, buffer_len) = if let DisplayState::SecretCapture {
            ref var_name,
            ref prompt,
            is_sudo,
            ref buffer,
        } = self.display_state
        {
            (var_name.as_str(), prompt.as_str(), is_sudo, buffer.len())
        } else {
            return;
        };

        frame.render_widget(Clear, area);

        // Centre a small dialog in the terminal
        let dlg_w = area.width.min(60);
        let dlg_h = 8u16;
        let x = area.x + (area.width.saturating_sub(dlg_w)) / 2;
        let y = area.y + (area.height.saturating_sub(dlg_h)) / 2;
        let dlg = Rect::new(x, y, dlg_w, dlg_h);

        let accent = if is_sudo {
            Color::Rgb(220, 80, 80)
        } else {
            Color::Rgb(80, 180, 220)
        };
        let icon = if is_sudo { "🔒" } else { "🔑" };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // prompt
                Constraint::Length(3), // input box
                Constraint::Length(1), // help
            ])
            .split(dlg);

        // Prompt line
        let prompt_para = Paragraph::new(Line::from(vec![
            Span::styled(format!("  {icon} "), Style::default().fg(accent)),
            Span::styled(
                prompt,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::TOP | Borders::RIGHT)
                .border_style(Style::default().fg(accent))
                .title(format!(" {} ", var_name)),
        );
        frame.render_widget(prompt_para, chunks[0]);

        // Masked input box
        let masked = "•".repeat(buffer_len);
        let input_para = Paragraph::new(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(masked, Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(accent)), // cursor
        ]))
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::BOTTOM | Borders::RIGHT)
                .border_style(Style::default().fg(accent)),
        );
        frame.render_widget(input_para, chunks[1]);

        // Help line
        let help = Paragraph::new(Line::from(vec![
            Span::styled("  Enter ", Style::default().fg(accent)),
            Span::styled("submit  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(accent)),
            Span::styled("abort", Style::default().fg(Color::DarkGray)),
        ]));
        frame.render_widget(help, chunks[2]);
    }

    fn render_skin_browser(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let accent = Color::Rgb(255, 150, 80); // warm tangerine
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search input
                Constraint::Min(1),    // skin list
                Constraint::Length(1), // help line
            ])
            .split(area);

        // ── Search box ───────────────────────────────────────────────
        let search_text = if self.skin_browser.query.is_empty() {
            "Type to filter skins…  (Esc to cancel)".to_string()
        } else {
            self.skin_browser.query.clone()
        };
        let search_style = if self.skin_browser.query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search = Paragraph::new(Line::from(vec![
            Span::styled("  🎨 ", Style::default().fg(accent)),
            Span::styled(search_text, search_style),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(accent))
                .title(" Browse Skins  [/skin] "),
        );
        frame.render_widget(search, chunks[0]);

        // ── Skin list ─────────────────────────────────────────────────
        let max_visible = chunks[1].height as usize;
        let filtered = &self.skin_browser.filtered;
        let selected = self.skin_browser.selected;

        let scroll_start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };

        let name_w = 20usize;

        let items: Vec<ListItem> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(vis_idx, &entry_idx)| {
                let entry = &self.skin_browser.items[entry_idx];
                let is_selected = vis_idx + scroll_start == selected;

                let name_cell = unicode_pad_right(&entry.name, name_w);
                let badge = if entry.is_active { " ✓ active" } else { "" };

                let bg = if is_selected {
                    Color::Rgb(60, 40, 20)
                } else {
                    Color::Reset
                };
                let name_fg = if is_selected {
                    Color::White
                } else {
                    Color::Rgb(220, 180, 100)
                };
                let badge_fg = Color::Rgb(100, 200, 100);

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {name_cell}"),
                        Style::default().fg(name_fg).bg(bg),
                    ),
                    Span::styled(badge, Style::default().fg(badge_fg).bg(bg)),
                ]))
            })
            .collect();

        let skin_list =
            List::new(items).block(Block::default().borders(Borders::LEFT | Borders::RIGHT));
        frame.render_widget(skin_list, chunks[1]);

        // ── Help line ─────────────────────────────────────────────────
        let help = Paragraph::new(Line::from(vec![
            Span::styled("  ↑↓ ", Style::default().fg(accent)),
            Span::styled("navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(accent)),
            Span::styled("apply  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(accent)),
            Span::styled("cancel", Style::default().fg(Color::DarkGray)),
        ]));
        frame.render_widget(help, chunks[2]);
    }

    /// Render the risk-graduated approval overlay.
    ///
    /// Layout:
    /// ```text
    ///   ┌─ Approval required ──────────────────────────────────┐
    ///   │                                                       │
    ///   │  ⚠  rm -rf /tmp/build                               │
    ///   │                                                       │
    ///   │   > [once]  [session]  [always]  [deny]  [v]iew      │
    ///   │                                                       │
    ///   │  ← → select  Enter confirm  v view  Esc deny         │
    ///   └───────────────────────────────────────────────────────┘
    /// ```
    fn render_approval_overlay(&self, frame: &mut Frame, area: Rect) {
        // Only render when in WaitingForApproval state
        let (command, show_full, full_command, selected) =
            if let DisplayState::WaitingForApproval {
                ref command,
                ref full_command,
                show_full,
                selected,
            } = self.display_state
            {
                (command.as_str(), show_full, full_command.as_str(), selected)
            } else {
                return;
            };

        frame.render_widget(Clear, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // command display + optional full view
                Constraint::Length(3), // choice buttons
                Constraint::Length(1), // help line
            ])
            .split(area);

        // ── Command display ──────────────────────────────────────────
        let cmd_text = if show_full { full_command } else { command };
        let cmd_lines: Vec<Line> = cmd_text
            .lines()
            .map(|l| {
                Line::from(vec![
                    Span::styled(
                        "  ⚠  ",
                        Style::default()
                            .fg(Color::Rgb(255, 140, 0))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        l.to_string(),
                        Style::default().fg(Color::Rgb(255, 220, 180)),
                    ),
                ])
            })
            .collect();
        let cmd_para = Paragraph::new(cmd_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(255, 140, 0)))
                    .title(" ⚠  Approval required "),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(cmd_para, chunks[0]);

        // ── Choice buttons ───────────────────────────────────────────
        const LABELS: [&str; 4] = ["once", "session", "always", "deny"];
        let mut btn_spans: Vec<Span> = vec![Span::raw("  ")];
        for (i, label) in LABELS.iter().enumerate() {
            let is_sel = i == selected;
            let style = if is_sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(255, 140, 0))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 180, 200))
            };
            btn_spans.push(Span::styled(format!(" [{label}] "), style));
            btn_spans.push(Span::raw(" "));
        }
        // View toggle indicator
        let view_style = if show_full {
            Style::default()
                .fg(Color::Rgb(255, 140, 0))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(100, 100, 130))
        };
        btn_spans.push(Span::styled(" [v]iew ", view_style));

        let buttons = Paragraph::new(Line::from(btn_spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(80, 80, 100))),
        );
        frame.render_widget(buttons, chunks[1]);

        // ── Help line ────────────────────────────────────────────────
        let help = Paragraph::new(Line::from(vec![
            Span::styled(" ← → ", Style::default().fg(Color::Rgb(255, 140, 0))),
            Span::styled("select  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(Color::Rgb(255, 140, 0))),
            Span::styled("confirm  ", Style::default().fg(Color::DarkGray)),
            Span::styled("v ", Style::default().fg(Color::Rgb(255, 140, 0))),
            Span::styled("view full  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(255, 140, 0))),
            Span::styled("deny", Style::default().fg(Color::DarkGray)),
        ]));
        frame.render_widget(help, chunks[2]);
    }

    /// Render the input box + completion overlay + ghost text.
    fn render_input(&mut self, frame: &mut Frame, area: Rect) {
        // Render the textarea widget
        frame.render_widget(&self.textarea, area);

        // Ghost text overlay (Fish-style hint)
        if matches!(self.editor_mode, InputEditorMode::Inline) {
            if let Some(hint) = self.ghost_hint() {
                let (row, col) = self.textarea.cursor();
                let ghost_x = area.x + 1 + col as u16; // +1 for border
                let ghost_y = area.y + 1 + row as u16;
                if ghost_x < area.x + area.width - 1 {
                    let max_width = (area.x + area.width - 1 - ghost_x) as usize;
                    let display = edgecrab_core::safe_truncate(&hint, max_width);
                    let ghost_area = Rect::new(ghost_x, ghost_y, display.len() as u16, 1);
                    let ghost = Paragraph::new(Span::styled(
                        display.to_string(),
                        Style::default().fg(Color::DarkGray),
                    ));
                    frame.render_widget(ghost, ghost_area);
                }
            }
        }

        // Completion overlay
        if matches!(self.editor_mode, InputEditorMode::Inline)
            && self.completion.active
            && !self.completion.candidates.is_empty()
        {
            let total_candidates = self.completion.candidates.len();
            let max_items = 8.min(total_candidates);
            let (scroll_start, scroll_end) = self.completion.visible_window(max_items);
            // +2 for top/bottom border, +1 for count footer
            let overlay_height = max_items as u16 + 3;
            let overlay_width = self
                .completion
                .candidates
                .iter()
                .map(|(cmd, desc)| {
                    let desc_len = if desc.is_empty() { 0 } else { 3 + desc.len() }; // " — desc"
                    cmd.len() + desc_len
                })
                .max()
                .unwrap_or(10) as u16
                + 4; // padding
            let overlay_width = overlay_width.clamp(24, area.width.saturating_sub(2));

            // Position above input area (with 1-row gap from input border)
            let overlay_y = area.y.saturating_sub(overlay_height);
            let overlay_x = area.x + 1;
            let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

            // Clear area behind overlay
            frame.render_widget(Clear, overlay_area);

            // Count indicator for the overlay title
            let sel_idx = self.completion.selected;
            let count_title = format!(
                " Commands {}/{} ",
                (sel_idx + 1).min(total_candidates),
                total_candidates
            );

            let items: Vec<ListItem> = self
                .completion
                .candidates
                .iter()
                .skip(scroll_start)
                .take(scroll_end.saturating_sub(scroll_start))
                .enumerate()
                .map(|(i, (cmd, desc))| {
                    let candidate_idx = scroll_start + i;
                    let is_selected = candidate_idx == self.completion.selected;
                    let cmd_style = if is_selected {
                        Style::default()
                            .bg(Color::Rgb(55, 55, 75))
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(200, 200, 210))
                    };
                    let desc_style = if is_selected {
                        Style::default()
                            .bg(Color::Rgb(55, 55, 75))
                            .fg(Color::Rgb(140, 145, 165))
                    } else {
                        Style::default().fg(Color::Rgb(95, 100, 120))
                    };
                    let mut spans = vec![Span::styled(format!(" {cmd}"), cmd_style)];
                    if !desc.is_empty() {
                        spans.push(Span::styled(format!(" — {desc}"), desc_style));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();

            let footer_line = if total_candidates > max_items {
                let hidden =
                    total_candidates.saturating_sub(scroll_end.saturating_sub(scroll_start));
                format!(" Tab/↑↓ navigate  PgUp/Dn jump  +{} more ", hidden)
            } else {
                " Tab/↑↓ navigate  Enter select  Esc cancel ".to_string()
            };

            // Split area: list body + footer
            let inner_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),    // list items
                    Constraint::Length(1), // footer hint
                ])
                .vertical_margin(1)
                .horizontal_margin(0)
                .split(overlay_area);

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Rgb(70, 75, 100)))
                        .title(count_title)
                        .title_style(Style::default().fg(Color::Rgb(140, 145, 165))),
                )
                .style(Style::default().bg(Color::Rgb(25, 25, 35)));
            frame.render_widget(list, overlay_area);

            // Render footer hint inside the border
            let footer_area = inner_chunks[1];
            let footer = Paragraph::new(Span::styled(
                footer_line,
                Style::default().fg(Color::Rgb(80, 85, 110)),
            ))
            .style(Style::default().bg(Color::Rgb(25, 25, 35)));
            frame.render_widget(footer, footer_area);
        }

        // Input line highlighting: color the border based on input validity + busy state
        let text = self.textarea_text();
        if self.is_processing {
            // Dimmed border while agent is processing — signals "not ready for input"
            self.textarea.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(
                        Style::default()
                            .fg(Color::Rgb(60, 60, 75))
                            .add_modifier(Modifier::DIM),
                    )
                    .title(self.editor_mode.input_title("⧗ waiting…")),
            );
        } else if text.starts_with('/') {
            let cmd_name = text.split_whitespace().next().unwrap_or("");
            let is_valid = self.all_command_names.iter().any(|c| c == cmd_name);
            let border_color = if is_valid {
                Color::Cyan
            } else if cmd_name.len() > 1 {
                Color::Rgb(239, 83, 80) // Red for invalid
            } else {
                self.theme.input_border.fg.unwrap_or(Color::White)
            };
            self.textarea.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .title(self.editor_mode.input_title(&self.theme.prompt_symbol)),
            );
        } else if text.starts_with('@') {
            self.textarea.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    .title(self.editor_mode.input_title(&self.theme.prompt_symbol)),
            );
        } else {
            self.textarea.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.theme.input_border)
                    .title(self.editor_mode.input_title(&self.theme.prompt_symbol)),
            );
        }
    }
}

/// Build a compact recap string showing the last few exchanges in a resumed session.
///
/// Mirrors hermes-agent's conversation recap panel:
/// - User messages shown with ● prefix, truncated to 300 chars
/// - Assistant messages shown with ◆ prefix, truncated to 200 chars
/// - Tool calls collapsed into a single line
/// - At most 10 recent exchanges shown, with "...N earlier messages..." indicator
fn build_session_recap(messages: &[edgecrab_types::Message]) -> String {
    use edgecrab_types::Role;

    const MAX_SHOWN: usize = 10;
    const USER_MAX: usize = 300;
    const ASSISTANT_MAX: usize = 200;

    // Filter to user/assistant turns (skip system, tool results)
    let turns: Vec<_> = messages
        .iter()
        .filter(|m| matches!(m.role, Role::User | Role::Assistant))
        .collect();

    if turns.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    lines.push("── Session Recap ──".to_string());

    let skip = turns.len().saturating_sub(MAX_SHOWN);
    if skip > 0 {
        lines.push(format!("  ...{skip} earlier messages..."));
    }

    for msg in turns.iter().skip(skip) {
        let text = msg.text_content();
        if text.is_empty() {
            // Assistant turn with only tool calls
            if let Some(ref tc) = msg.tool_calls {
                let names: Vec<_> = tc.iter().map(|t| t.function.name.as_str()).collect();
                lines.push(format!("  ◆ [tool calls: {}]", names.join(", ")));
            }
            continue;
        }
        let (icon, max_len) = match msg.role {
            Role::User => ("●", USER_MAX),
            Role::Assistant => ("◆", ASSISTANT_MAX),
            _ => continue,
        };
        let truncated = if text.len() > max_len {
            format!(
                "{}…",
                &text[..text
                    .char_indices()
                    .take_while(|(i, _)| *i < max_len)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(max_len)]
            )
        } else {
            text
        };
        // Collapse to single line
        let oneline = truncated.lines().collect::<Vec<_>>().join(" ");
        lines.push(format!("  {icon} {oneline}"));
    }

    lines.push("────────────────────".to_string());
    lines.join("\n")
}

/// Format token count for display (e.g. 1234 → "1.2k", 1234567 → "1.2M")
fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        format!("{count}")
    }
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let shortened: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{shortened}...")
}

/// Run the interactive TUI event loop.
pub fn run_tui(app: &mut App) -> io::Result<()> {
    // Install a panic hook that restores the terminal before printing the panic.
    // Without this, a crash leaves the terminal in raw mode with no cursor.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best-effort terminal restore
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            io::stderr(),
            PopKeyboardEnhancementFlags,
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture,
            crossterm::event::DisableBracketedPaste,
            crossterm::cursor::Show,
        );
        original_hook(info);
    }));

    crossterm::terminal::enable_raw_mode()?;
    let keyboard_enhancement_supported =
        crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        // Mouse capture is ON by default so the scroll wheel moves the output
        // pane.  This intercepts mouse events at the PTY level; native
        // drag-to-copy is unavailable while capture is active (Shift+drag only
        // bypasses it in iTerm2/WezTerm/Kitty, NOT in macOS Terminal.app).
        // Press F6 in the TUI to toggle SELECT mode (capture off = drag=copy).
        crossterm::event::EnableBracketedPaste,
    )?;

    let mut keyboard_enhancement_enabled = false;
    if keyboard_enhancement_supported
        && crossterm::execute!(
            stdout,
            PushKeyboardEnhancementFlags(progressive_keyboard_flags())
        )
        .is_ok()
    {
        keyboard_enhancement_enabled = true;
        // The terminal switches keyboard decoding mode asynchronously after
        // it consumes the CSI-u command. Wait briefly before polling for
        // user input so the first printable key does not race the legacy
        // layout path on non-US keyboards.
        stdout.flush()?;
        std::thread::sleep(KEYBOARD_PROTOCOL_WARMUP);
    }
    app.set_keyboard_enhancement_enabled(keyboard_enhancement_enabled);
    if !keyboard_enhancement_enabled {
        app.push_output(
            "Keyboard note: this terminal does not expose Shift+Enter separately. Use Ctrl+J to open compose mode and insert a newline; use Ctrl+S to send from compose mode.",
            OutputRole::System,
        );
    }

    // Honour the initial mouse_capture_enabled flag (default: true = SCROLL mode).
    if app.mouse_capture_enabled {
        crossterm::execute!(stdout, crossterm::event::EnableMouseCapture)?;
    };
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = event_loop(&mut terminal, app);

    crossterm::terminal::disable_raw_mode()?;
    if keyboard_enhancement_enabled {
        crossterm::execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)?;
    }
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableBracketedPaste,
    )?;
    terminal.show_cursor()?;

    // Print goodbye message to stdout after the TUI is torn down.
    // This appears on the normal terminal after the alternate screen closes.
    println!("{}", app.theme.goodbye_msg);

    result
}

fn event_loop(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = std::time::Duration::from_millis(80); // spinner animation rate

    loop {
        // Check for agent responses first (non-blocking)
        app.check_responses();

        // Advance spinner on each tick
        let now_elapsed = last_tick.elapsed();
        if now_elapsed >= tick_rate {
            app.tick_spinner();
            last_tick = Instant::now();
            app.needs_redraw = true;
        }

        // Only redraw when state changed — reduces CPU on idle
        if app.needs_redraw {
            // When clear_output() was called, force a full terminal repaint
            // before drawing so that any out-of-band characters that landed on
            // the alternate screen (e.g. tracing output from a background task)
            // are erased.  terminal.clear() resets ratatui's internal prev-buffer
            // so the next draw() writes every cell from scratch.
            if app.needs_full_terminal_clear {
                terminal.clear()?;
                app.needs_full_terminal_clear = false;
            }
            terminal.draw(|f| app.render(f))?;
            app.needs_redraw = false;
        }

        // Poll with a short timeout so we loop quickly while streaming/thinking
        let poll_timeout = if app.is_processing {
            std::time::Duration::from_millis(16) // ~60fps while streaming
        } else {
            std::time::Duration::from_millis(50) // 20fps idle
        };

        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    app.handle_key_event(key);
                }
                Event::Paste(text) => {
                    // Bracketed paste — insert text directly (safe from injection:
                    // bracketed paste prevents escape sequences from being executed)
                    app.handle_paste(text);
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse_event(mouse);
                }
                Event::Resize(_, _) => {
                    // Terminal resized — force redraw and invalidate all render caches
                    app.needs_redraw = true;
                    for line in &mut app.output {
                        line.rendered = None;
                    }
                }
                _ => {}
            }
        }

        if let Some(enabled) = app.take_mouse_capture_request() {
            if enabled {
                crossterm::execute!(terminal.backend_mut(), crossterm::event::EnableMouseCapture)?;
            } else {
                crossterm::execute!(
                    terminal.backend_mut(),
                    crossterm::event::DisableMouseCapture
                )?;
            }
        }

        if app.should_exit() {
            return Ok(());
        }
    }
}

/// Write raw RGBA pixel data as a minimal PNG file.
///
/// Uses a very simple approach: writes uncompressed DEFLATE blocks via
/// the `flate2` or stdlib. Since we don't have an image crate dep,
/// we write a valid PNG manually using zlib-compressed IDAT chunks.
fn write_rgba_png(
    path: &std::path::Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = std::fs::File::create(path)?;

    // PNG signature
    file.write_all(&[137, 80, 78, 71, 13, 10, 26, 10])?;

    // Helper to write a PNG chunk
    fn write_chunk(w: &mut impl Write, chunk_type: &[u8; 4], data: &[u8]) -> std::io::Result<()> {
        let len = data.len() as u32;
        w.write_all(&len.to_be_bytes())?;
        w.write_all(chunk_type)?;
        w.write_all(data)?;
        let mut crc_data = Vec::with_capacity(4 + data.len());
        crc_data.extend_from_slice(chunk_type);
        crc_data.extend_from_slice(data);
        let crc = png_crc32(&crc_data);
        w.write_all(&crc.to_be_bytes())?;
        Ok(())
    }

    // IHDR
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // color type: RGBA
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    write_chunk(&mut file, b"IHDR", &ihdr)?;

    // IDAT — build raw image data (filter byte 0 per row + RGBA pixels)
    let row_len = 1 + (width as usize * 4);
    let mut raw = Vec::with_capacity(row_len * height as usize);
    for y in 0..height as usize {
        raw.push(0u8); // filter: None
        let start = y * width as usize * 4;
        let end = start + width as usize * 4;
        if end <= rgba.len() {
            raw.extend_from_slice(&rgba[start..end]);
        } else {
            // Pad with transparent black if data is short
            raw.extend(std::iter::repeat_n(0u8, width as usize * 4));
        }
    }

    // Simple DEFLATE using stored blocks (no compression, but always valid)
    let compressed = deflate_stored(&raw);
    write_chunk(&mut file, b"IDAT", &compressed)?;

    // IEND
    write_chunk(&mut file, b"IEND", &[])?;

    Ok(())
}

/// Minimal stored-only DEFLATE wrapper (zlib format).
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // zlib header: CM=8, CINFO=7 (32K window), FCHECK so header%31==0
    out.push(0x78);
    out.push(0x01);

    // Split into 65535-byte stored blocks
    let mut offset = 0;
    while offset < data.len() {
        let remaining = data.len() - offset;
        let block_len = remaining.min(65535);
        let is_final = offset + block_len >= data.len();
        out.push(if is_final { 0x01 } else { 0x00 }); // BFINAL + BTYPE=00 (stored)
        let len = block_len as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&data[offset..offset + block_len]);
        offset += block_len;
    }

    // Adler-32 checksum
    let adler = adler32(data);
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn png_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn app_init() {
        let app = App::new();
        assert!(!app.should_exit());
        assert!(app.output.is_empty());
    }

    #[tokio::test]
    async fn app_push_output() {
        let mut app = App::new();
        app.push_output("hello", OutputRole::Assistant);
        assert_eq!(app.output.len(), 1);
        assert_eq!(app.output[0].text, "hello");
    }

    #[tokio::test]
    async fn app_clear_output() {
        let mut app = App::new();
        app.push_output("line1", OutputRole::System);
        app.push_output("line2", OutputRole::System);
        app.clear_output();
        assert!(app.output.is_empty());
    }

    #[tokio::test]
    async fn background_prompt_uses_isolated_session() {
        let provider: Arc<dyn edgequake_llm::LLMProvider> =
            Arc::new(edgequake_llm::MockProvider::new());
        let agent = Arc::new(
            edgecrab_core::AgentBuilder::new("mock")
                .provider(provider)
                .build()
                .expect("build agent"),
        );

        let _ = agent
            .chat("foreground turn")
            .await
            .expect("foreground chat");
        let before = agent.session_snapshot().await;

        let mut app = App::new();
        app.set_agent(agent.clone());
        app.handle_background_prompt("background turn".into());

        tokio::time::sleep(Duration::from_millis(25)).await;
        app.check_responses();

        let after = agent.session_snapshot().await;
        assert_eq!(after.message_count, before.message_count);
        assert!(
            app.output
                .iter()
                .any(|line| line.text.contains("Background task #1 started"))
        );
        assert!(
            app.output
                .iter()
                .any(|line| line.text.contains("EdgeCrab (background #1)"))
        );
        assert!(app.background_tasks_active.is_empty());
    }

    #[test]
    fn background_progress_text_formats_tool_events() {
        let event = edgecrab_core::StreamEvent::ToolExec {
            name: "terminal".into(),
            args_json: r#"{"command":"cargo test -p edgecrab-core"}"#.into(),
        };
        let text = background_progress_text(2, &event).expect("progress text");
        assert!(text.contains("bg#2"));
        assert!(text.contains("terminal"));
    }

    #[test]
    fn background_progress_text_formats_subagent_completion() {
        let event = edgecrab_core::StreamEvent::SubAgentFinish {
            task_index: 1,
            task_count: 3,
            status: "completed".into(),
            duration_ms: 2500,
            summary: "done".into(),
            api_calls: 2,
            model: Some("mock/model".into()),
        };
        let text = background_progress_text(4, &event).expect("progress text");
        assert!(text.contains("bg#4"));
        assert!(text.contains("[2/3]"));
        assert!(text.contains("completed in 2.5s"));
    }

    #[test]
    fn background_status_summary_prefers_latest_progress() {
        let mut tasks = std::collections::HashMap::new();
        tasks.insert(
            "bg-1".into(),
            BackgroundTaskStatus {
                preview: "first task".into(),
                last_progress: Some("↳ bg#1 terminal: cargo test".into()),
                last_seq: 1,
            },
        );
        tasks.insert(
            "bg-2".into(),
            BackgroundTaskStatus {
                preview: "second task".into(),
                last_progress: Some("↳ bg#2 [1/2] terminal: rg delegate".into()),
                last_seq: 2,
            },
        );

        let summary = format_background_status_summary(&tasks).expect("summary");
        assert!(summary.contains("bg#2"));
        assert!(summary.contains("delegate"));
    }

    #[test]
    fn subagent_status_summary_prefers_latest_tool_detail() {
        let mut tasks = std::collections::HashMap::new();
        tasks.insert(
            0,
            ActiveSubagentStatus {
                task_index: 0,
                task_count: 3,
                goal: "inspect repo".into(),
                last_detail: Some("terminal  command: cargo test".into()),
                last_seq: 4,
            },
        );
        tasks.insert(
            1,
            ActiveSubagentStatus {
                task_index: 1,
                task_count: 3,
                goal: "audit gateway".into(),
                last_detail: Some("file_search  query: delegate_task".into()),
                last_seq: 7,
            },
        );

        let summary = format_subagent_status_summary(&tasks).expect("summary");
        assert!(summary.contains("[2/3]"));
        assert!(summary.contains("delegate_task"));
    }

    #[test]
    fn background_progress_text_formats_subagent_reasoning() {
        let event = edgecrab_core::StreamEvent::SubAgentReasoning {
            task_index: 0,
            task_count: 2,
            text: "searching the workspace for delegation regressions".into(),
        };
        let text = background_progress_text(3, &event).expect("progress text");
        assert!(text.contains("bg#3"));
        assert!(text.contains("[1/2]"));
        assert!(text.contains("thinking"));
    }

    #[test]
    fn context_usage_ratio_clamps_at_one_hundred_percent() {
        assert_eq!(context_usage_ratio(210_000, Some(200_000)), Some(1.0));
        assert_eq!(context_usage_ratio(100_000, Some(200_000)), Some(0.5));
        assert_eq!(context_usage_ratio(100, Some(0)), None);
        assert_eq!(context_usage_ratio(100, None), None);
    }

    #[tokio::test]
    async fn app_slash_command_exit() {
        let mut app = App::new();
        app.process_input("/quit");
        assert!(app.should_exit());
    }

    #[tokio::test]
    async fn app_slash_command_help() {
        let mut app = App::new();
        app.process_input("/help");
        assert!(!app.output.is_empty());
        assert!(
            app.output
                .last()
                .is_some_and(|l| l.text.contains("EdgeCrab")
                    || l.text.contains("slash commands")
                    || l.text.contains("Navigation"))
        );
    }

    #[tokio::test]
    async fn app_regular_input() {
        let mut app = App::new();
        app.process_input("explain this code");
        assert!(
            app.output
                .iter()
                .any(|l| l.text.contains("explain this code"))
        );
    }

    #[tokio::test]
    async fn app_handle_ctrl_c_exits_on_empty() {
        let mut app = App::new();
        app.handle_key_event(event::KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ));
        assert!(app.should_exit());
    }

    #[test]
    fn progressive_keyboard_flags_request_full_csi_u_path() {
        let flags = progressive_keyboard_flags();
        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
    }

    #[tokio::test]
    async fn history_push_and_recall() {
        let mut app = App::new();
        app.input_history.clear();
        app.history_pos = 0;
        app.push_history("/help");
        app.push_history("/status");
        assert_eq!(app.input_history.len(), 2);
        assert_eq!(app.history_pos, 2);

        // Navigate up
        app.history_up();
        assert_eq!(app.history_pos, 1);
        app.history_up();
        assert_eq!(app.history_pos, 0);
        // Should not go below 0
        app.history_up();
        assert_eq!(app.history_pos, 0);

        // Navigate down
        app.history_down();
        assert_eq!(app.history_pos, 1);
        app.history_down();
        assert_eq!(app.history_pos, 2); // back to "new input"
    }

    #[tokio::test]
    async fn history_dedup_consecutive() {
        let mut app = App::new();
        app.input_history.clear();
        app.history_pos = 0;
        app.push_history("/help");
        app.push_history("/help");
        assert_eq!(app.input_history.len(), 1);
    }

    #[tokio::test]
    async fn completion_candidates_prefix() {
        let app = App::new();
        let candidates: Vec<_> = app
            .all_command_names
            .iter()
            .filter(|c| c.starts_with("/he"))
            .cloned()
            .collect();
        assert!(candidates.contains(&"/help".to_string()));
    }

    #[tokio::test]
    async fn completion_fuzzy_match() {
        let app = App::new();
        // Test fuzzy match for a typo
        let scored: Vec<_> = app
            .all_command_names
            .iter()
            .map(|cmd| (cmd.clone(), strsim::jaro_winkler("/hepl", cmd)))
            .filter(|(_, score)| *score > 0.7)
            .collect();
        assert!(
            !scored.is_empty(),
            "fuzzy match should find /help for /hepl"
        );
    }

    #[tokio::test]
    async fn format_tokens_display() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1500), "1.5k");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[tokio::test]
    async fn ghost_hint_works() {
        let mut app = App::new();
        app.push_history("/model copilot/gpt-4.1-mini");
        // Simulate typing "/mod"
        for ch in "/mod".chars() {
            app.textarea.insert_char(ch);
        }
        let hint = app.ghost_hint();
        assert!(hint.is_some(), "ghost hint should be available");
        assert!(
            hint.unwrap().starts_with("el"),
            "hint should complete /model..."
        );
    }

    // ── Tab completion: argument preservation ────────────────────────────────

    /// Pressing Tab when the user has typed "/hel some-query" should NOT wipe
    /// "some-query" — it should only complete/replace the command token "/hel".
    #[tokio::test]
    async fn completion_preserves_argument_tail() {
        let mut app = App::new();
        // Make sure "/help" is a known command so we can trigger the exact path.
        assert!(
            app.all_command_names.contains(&"/help".to_string()),
            "/help must be a registered command for this test"
        );

        // Type a partial command (NOT an exact alias) plus an argument.
        // "/hel" is a partial, not an exact alias — "/help" is the full command.
        for ch in "/hel some-query".chars() {
            app.textarea.insert_char(ch);
        }
        // update_completion must find "/help" as a prefix candidate.
        app.update_completion();
        assert!(app.completion.active, "completion should activate for /hel");

        // Force the selected candidate to /help.
        let help_idx = app
            .completion
            .candidates
            .iter()
            .position(|(c, _)| c == "/help");
        assert!(help_idx.is_some(), "/help must appear in candidates");
        app.completion.selected = help_idx.unwrap();

        // Accept — the textarea should now be "/help some-query" (arg preserved).
        app.accept_completion();
        let result = app.textarea_text();
        assert!(
            result.starts_with("/help"),
            "accepted command should be /help, got: {result}"
        );
        assert!(
            result.contains("some-query"),
            "argument 'some-query' must be preserved, got: {result}"
        );
    }

    /// When the command token exactly names a known command AND args follow,
    /// update_completion must suppress the overlay (nothing to complete).
    #[tokio::test]
    async fn completion_suppressed_when_exact_command_with_args() {
        let mut app = App::new();
        assert!(app.all_command_names.contains(&"/help".to_string()));

        for ch in "/help advanced-topic".chars() {
            app.textarea.insert_char(ch);
        }
        app.update_completion();
        assert!(
            !app.completion.active,
            "completion must not activate when full command + args are typed"
        );
    }

    // ── Tab key: ghost hint acceptance priority ──────────────────────────────

    /// Tab at end-of-line with an available ghost hint should accept the hint
    /// (fish-shell behaviour) instead of opening the completion overlay.
    #[tokio::test]
    async fn tab_key_accepts_ghost_hint_first() {
        let mut app = App::new();
        app.push_history("explain this code in detail");

        // Type enough of the entry to produce a ghost hint.
        for ch in "explain".chars() {
            app.textarea.insert_char(ch);
        }
        assert!(
            app.ghost_hint().is_some(),
            "ghost hint must exist before the test is meaningful"
        );

        // Fire the Tab key.
        app.handle_key_event(event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        let text = app.textarea_text();
        assert_eq!(
            text, "explain this code in detail",
            "Tab should have accepted the full ghost hint"
        );
        // Completion overlay must NOT have been activated.
        assert!(
            !app.completion.active,
            "completion overlay must stay closed when ghost hint was accepted"
        );
    }

    /// Tab with no ghost hint present should open the command completion overlay.
    #[tokio::test]
    async fn tab_key_opens_completion_when_no_ghost() {
        let mut app = App::new();
        app.input_history.clear(); // ensure no ghost hint

        // Type a partial command with no history match.
        for ch in "/he".chars() {
            app.textarea.insert_char(ch);
        }
        assert!(app.ghost_hint().is_none(), "precondition: no ghost hint");

        app.handle_key_event(event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(
            app.completion.active,
            "Tab without ghost hint must open completion overlay for /he"
        );
    }

    #[tokio::test]
    async fn shift_enter_enters_compose_insert_and_inserts_newline() {
        let mut app = App::new();
        app.textarea.insert_str("hello");

        app.handle_key_event(event::KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));

        assert_eq!(app.editor_mode, InputEditorMode::ComposeInsert);
        assert_eq!(app.textarea_text(), "hello\n");
    }

    #[tokio::test]
    async fn ctrl_j_is_terminal_safe_multiline_fallback() {
        let mut app = App::new();
        app.textarea.insert_str("alpha");

        app.handle_key_event(event::KeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::CONTROL,
        ));

        assert_eq!(app.editor_mode, InputEditorMode::ComposeInsert);
        assert_eq!(app.textarea_text(), "alpha\n");
    }

    #[tokio::test]
    async fn compose_insert_escape_switches_to_normal_and_back_to_inline() {
        let mut app = App::new();
        app.open_compose_editor(false);
        assert_eq!(app.editor_mode, InputEditorMode::ComposeInsert);

        app.handle_key_event(event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.editor_mode, InputEditorMode::ComposeNormal);

        app.handle_key_event(event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.editor_mode, InputEditorMode::Inline);
    }

    #[tokio::test]
    async fn compose_normal_x_deletes_character() {
        let mut app = App::new();
        app.textarea.insert_str("hello");
        app.open_compose_editor(false);
        app.handle_key_event(event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key_event(event::KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE));
        app.handle_key_event(event::KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        assert_eq!(app.editor_mode, InputEditorMode::ComposeNormal);
        assert_eq!(app.textarea_text(), "ello");
    }

    #[tokio::test]
    async fn compose_normal_o_opens_new_line_and_enters_insert() {
        let mut app = App::new();
        app.textarea.insert_str("hello");
        app.open_compose_editor(false);
        app.handle_key_event(event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key_event(event::KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));

        assert_eq!(app.editor_mode, InputEditorMode::ComposeInsert);
        assert_eq!(app.textarea_text(), "hello\n");
    }

    #[test]
    fn completion_visible_window_tracks_selected_row() {
        let completion = CompletionState {
            candidates: (0..20)
                .map(|i| (format!("/cmd{i}"), String::new()))
                .collect(),
            selected: 12,
            active: true,
            arg_start: 0,
        };

        let (start, end) = completion.visible_window(8);
        assert_eq!((start, end), (5, 13));
        assert!(
            (start..end).contains(&completion.selected),
            "selected row must stay inside the rendered window"
        );
    }

    #[tokio::test]
    async fn completion_page_down_moves_beyond_first_viewport() {
        let mut app = App::new();
        app.completion = CompletionState {
            candidates: (0..20)
                .map(|i| (format!("/cmd{i}"), String::new()))
                .collect(),
            selected: 0,
            active: true,
            arg_start: 0,
        };

        app.handle_key_event(event::KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));

        assert_eq!(app.completion.selected, 8);
        let (start, end) = app.completion.visible_window(8);
        assert_eq!((start, end), (1, 9));
        assert!(
            (start..end).contains(&app.completion.selected),
            "page navigation must move the popup viewport with the selection"
        );
    }

    // ── ghost_hint_next_word ─────────────────────────────────────────────────

    #[tokio::test]
    async fn ghost_hint_next_word_returns_first_word() {
        let mut app = App::new();
        app.push_history("/model copilot/gpt-4.1-mini");
        for ch in "/model".chars() {
            app.textarea.insert_char(ch);
        }
        // Ghost hint should be " copilot/gpt-4.1-mini"
        let word = app.ghost_hint_next_word();
        assert!(word.is_some(), "next-word hint must be available");
        let w = word.unwrap();
        // Should contain the space + the model name token (not the full hint).
        assert!(
            w.contains("copilot"),
            "next word should be 'copilot/gpt-4.1-mini', got: {w}"
        );
        // Must NOT span all the way to the end when there is only one token.
        assert!(!w.is_empty(), "next word must not be empty");
    }

    /// Alt+Right at EOL with a ghost hint should accept only the next word.
    #[tokio::test]
    async fn alt_right_accepts_next_ghost_word() {
        let mut app = App::new();
        app.push_history("explain this code in detail");
        for ch in "explain".chars() {
            app.textarea.insert_char(ch);
        }
        assert!(app.ghost_hint_next_word().is_some(), "precondition");

        app.handle_key_event(event::KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));

        let text = app.textarea_text();
        // Should have accepted " this" (one word) but NOT the full hint.
        assert!(text.starts_with("explain"), "prefix must be preserved");
        assert!(
            text.len() > "explain".len(),
            "at least one word should have been added"
        );
        assert!(
            text != "explain this code in detail",
            "full hint must NOT have been accepted; only one word"
        );
    }

    #[tokio::test]
    async fn mouse_mode_toggle_sets_pending_request() {
        let mut app = App::new();
        // Default is ON (SCROLL mode — wheel scrolling active).
        assert!(app.mouse_capture_enabled);
        // Turning on when already on is a no-op — no pending request.
        app.handle_mouse_mode("on".into());
        assert!(app.mouse_capture_enabled);
        assert_eq!(app.take_mouse_capture_request(), None);
        app.handle_mouse_mode("off".into());
        assert!(!app.mouse_capture_enabled);
        assert_eq!(app.take_mouse_capture_request(), Some(false));
        // Now turning on should queue a pending enable.
        app.handle_mouse_mode("on".into());
        assert!(app.mouse_capture_enabled);
        assert_eq!(app.take_mouse_capture_request(), Some(true));
    }

    #[tokio::test]
    async fn remove_reasoning_output_block_keeps_stream_alignment() {
        let mut app = App::new();
        app.output.push(OutputLine {
            text: "🧠 Thinking\nstep 1".into(),
            role: OutputRole::Reasoning,
            prebuilt_spans: None,
            rendered: None,
        });
        app.output.push(OutputLine {
            text: "answer".into(),
            role: OutputRole::Assistant,
            prebuilt_spans: None,
            rendered: None,
        });
        app.reasoning_line = Some(0);
        app.streaming_line = Some(1);

        app.remove_reasoning_output_block();

        assert!(app.reasoning_line.is_none());
        assert_eq!(app.streaming_line, Some(0));
        assert_eq!(app.output.len(), 1);
        assert!(matches!(app.output[0].role, OutputRole::Assistant));
    }

    // ─── image paste detection ──────────────────────────────────────────

    #[test]
    fn is_image_path_recognises_common_extensions() {
        assert!(App::is_image_path("/home/user/photo.png"));
        assert!(App::is_image_path("/tmp/screen.jpg"));
        assert!(App::is_image_path("/tmp/screen.jpeg"));
        assert!(App::is_image_path("/tmp/anim.gif"));
        assert!(App::is_image_path("/tmp/art.webp"));
        assert!(App::is_image_path("/tmp/scan.bmp"));
        assert!(App::is_image_path("/tmp/scan.tiff"));
        assert!(App::is_image_path("/tmp/scan.tif"));
        assert!(App::is_image_path("/tmp/next.avif"));
    }

    #[test]
    fn is_image_path_case_insensitive() {
        assert!(App::is_image_path("/tmp/PHOTO.PNG"));
        assert!(App::is_image_path("/tmp/Photo.Jpg"));
    }

    #[test]
    fn is_image_path_rejects_non_image() {
        assert!(!App::is_image_path("hello world"));
        assert!(!App::is_image_path("/tmp/archive.zip"));
        assert!(!App::is_image_path("/tmp/doc.pdf"));
        assert!(!App::is_image_path("no extension"));
        assert!(!App::is_image_path(""));
    }

    #[tokio::test]
    async fn handle_paste_text_goes_to_textarea() {
        let mut app = App::new();
        app.handle_paste("hello world".into());
        assert_eq!(app.textarea_text(), "hello world");
        assert!(app.pending_images.is_empty());
    }

    // ── Subcommand / argument completion ────────────────────────────────────

    /// command_arg_hints should return non-empty slices for known commands.
    #[test]
    fn command_arg_hints_returns_subcommands_for_session() {
        let hints = App::command_arg_hints("session");
        assert!(!hints.is_empty(), "session should have subcommand hints");
        let names: Vec<&str> = hints.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"list"), "session hints must include 'list'");
        assert!(
            names.contains(&"switch"),
            "session hints must include 'switch'"
        );
        assert!(names.contains(&"new"), "session hints must include 'new'");
    }

    /// Alias "sessions" and canonical "session" must return the same hints.
    #[test]
    fn command_arg_hints_alias_sessions_matches_session() {
        assert_eq!(
            App::command_arg_hints("sessions"),
            App::command_arg_hints("session"),
            "alias 'sessions' must mirror 'session'"
        );
    }

    /// command_arg_hints for an unknown token should return an empty slice.
    #[test]
    fn command_arg_hints_unknown_command_returns_empty() {
        assert!(App::command_arg_hints("nonexistent_cmd").is_empty());
    }

    #[test]
    fn command_arg_hints_streaming_alias_matches() {
        let hints = App::command_arg_hints("stream");
        assert_eq!(hints, App::command_arg_hints("streaming"));
        let names: Vec<&str> = hints.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&"toggle"));
        assert!(names.contains(&"status"));
    }

    /// After typing "/session " (with trailing space) update_completion should
    /// populate candidates with subcommands and set arg_start > 0.
    #[tokio::test]
    async fn update_completion_arg_context_populates_subcommands() {
        let mut app = App::new();
        // "/session" must be a registered command for arg-context to fire.
        assert!(
            app.all_command_names
                .iter()
                .any(|c| c == "/session" || c == "/sessions"),
            "/session must be a registered command"
        );
        for ch in "/session ".chars() {
            app.textarea.insert_char(ch);
        }
        app.update_completion();
        assert!(
            app.completion.active,
            "completion must activate in arg context"
        );
        assert!(
            app.completion.arg_start > 0,
            "arg_start must be set to a non-zero byte offset"
        );
        let candidate_names: Vec<&str> = app
            .completion
            .candidates
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert!(
            candidate_names.contains(&"list"),
            "must have 'list' candidate"
        );
        assert!(
            candidate_names.contains(&"switch"),
            "must have 'switch' candidate"
        );
    }

    /// Prefix "/session sw" should narrow candidates to those starting with "sw".
    #[tokio::test]
    async fn update_completion_arg_prefix_filters_candidates() {
        let mut app = App::new();
        for ch in "/session sw".chars() {
            app.textarea.insert_char(ch);
        }
        app.update_completion();
        assert!(app.completion.active);
        let names: Vec<&str> = app
            .completion
            .candidates
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert!(
            names.iter().all(|n| n.starts_with("sw")),
            "all candidates should match prefix 'sw', got: {names:?}"
        );
        assert!(
            names.contains(&"switch"),
            "switch must be in narrowed candidates"
        );
    }

    /// Accepting a subcommand must produce "/session switch " (cmd preserved, arg appended).
    #[tokio::test]
    async fn accept_completion_arg_context_preserves_command_prefix() {
        let mut app = App::new();
        for ch in "/session ".chars() {
            app.textarea.insert_char(ch);
        }
        app.update_completion();
        assert!(app.completion.active, "completion must be active");

        // Select "switch"
        let switch_idx = app
            .completion
            .candidates
            .iter()
            .position(|(n, _)| n == "switch");
        assert!(switch_idx.is_some(), "'switch' must be among candidates");
        app.completion.selected = switch_idx.unwrap();

        app.accept_completion();
        let result = app.textarea_text();
        assert!(
            result.starts_with("/session "),
            "command prefix '/session ' must be preserved, got: {result}"
        );
        assert!(
            result.ends_with("switch "),
            "result must end with 'switch ', got: {result}"
        );
    }

    /// Fuzzy match: "/session lisst" should still surface "list" when score ≥ 0.65.
    #[tokio::test]
    async fn update_completion_arg_fuzzy_typo_matches_list() {
        let mut app = App::new();
        for ch in "/session lisst".chars() {
            app.textarea.insert_char(ch);
        }
        app.update_completion();
        // May or may not be active depending on fuzzy score — just check it
        // doesn't hard-crash and that if active it contains 'list'.
        if app.completion.active {
            let names: Vec<&str> = app
                .completion
                .candidates
                .iter()
                .map(|(n, _)| n.as_str())
                .collect();
            assert!(
                names.contains(&"list"),
                "fuzzy match for 'lisst' should find 'list', got: {names:?}"
            );
        }
        // (If not active, that's also acceptable — the typo was too far off.)
    }

    /// textarea_set_text should replace current content atomically.
    #[tokio::test]
    async fn textarea_set_text_replaces_content() {
        let mut app = App::new();
        for ch in "old content".chars() {
            app.textarea.insert_char(ch);
        }
        app.textarea_set_text("new content");
        assert_eq!(app.textarea_text(), "new content");
    }

    /// textarea_set_text with empty string should clear the textarea.
    #[tokio::test]
    async fn textarea_set_text_empty_clears_textarea() {
        let mut app = App::new();
        for ch in "something".chars() {
            app.textarea.insert_char(ch);
        }
        app.textarea_set_text("");
        assert_eq!(app.textarea_text(), "");
    }
}
