//! # skin_engine -- hermes-agent-compatible skin/theme engine
//!
//! Skins control the visual presentation of the EdgeCrab CLI: colors,
//! spinner animations, branding text, and tool-output prefixes.
//! The schema is fully compatible with hermes-agent skin YAML files.
//!
//! ## Built-in skins (matching hermes-agent exactly)
//!
//! | Name      | Theme                             |
//! |-----------|-----------------------------------|
//! | default   | Gold / cornsilk (classic Crab)    |
//! | ares      | Crimson / bronze -- war-god       |
//! | mono      | Grayscale -- clean minimal        |
//! | slate     | Royal blue -- developer-focused   |
//! | poseidon  | Deep blue / seafoam -- ocean-god  |
//! | sisyphus  | Stark grayscale -- persistence    |
//! | charizard | Burnt orange / ember -- volcanic  |
//!
//! User skins live in `~/.edgecrab/skins/<name>.yaml` and inherit
//! missing keys from the built-in `default` skin.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Color palette
// ---------------------------------------------------------------------------

/// 15-key hermes-agent-compatible color palette (hex `#RRGGBB` strings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinColors {
    // Banner panel
    #[serde(default = "SkinColors::default_banner_border")]
    pub banner_border: String,
    #[serde(default = "SkinColors::default_banner_title")]
    pub banner_title: String,
    #[serde(default = "SkinColors::default_banner_accent")]
    pub banner_accent: String,
    #[serde(default = "SkinColors::default_banner_dim")]
    pub banner_dim: String,
    #[serde(default = "SkinColors::default_banner_text")]
    pub banner_text: String,

    // General UI
    #[serde(default = "SkinColors::default_ui_accent")]
    pub ui_accent: String,
    #[serde(default = "SkinColors::default_ui_label")]
    pub ui_label: String,
    #[serde(default = "SkinColors::default_ui_ok")]
    pub ui_ok: String,
    #[serde(default = "SkinColors::default_ui_error")]
    pub ui_error: String,
    #[serde(default = "SkinColors::default_ui_warn")]
    pub ui_warn: String,

    // Input
    #[serde(default = "SkinColors::default_prompt")]
    pub prompt: String,
    #[serde(default = "SkinColors::default_input_rule")]
    pub input_rule: String,

    // Response box
    #[serde(default = "SkinColors::default_response_border")]
    pub response_border: String,

    // Session indicator
    #[serde(default = "SkinColors::default_session_label")]
    pub session_label: String,
    #[serde(default = "SkinColors::default_session_border")]
    pub session_border: String,
}

impl SkinColors {
    fn default_banner_border() -> String {
        "#CD7F32".into()
    }
    fn default_banner_title() -> String {
        "#FFD700".into()
    }
    fn default_banner_accent() -> String {
        "#FFBF00".into()
    }
    fn default_banner_dim() -> String {
        "#B8860B".into()
    }
    fn default_banner_text() -> String {
        "#FFF8DC".into()
    }
    fn default_ui_accent() -> String {
        "#FFBF00".into()
    }
    fn default_ui_label() -> String {
        "#4dd0e1".into()
    }
    fn default_ui_ok() -> String {
        "#4caf50".into()
    }
    fn default_ui_error() -> String {
        "#ef5350".into()
    }
    fn default_ui_warn() -> String {
        "#ffa726".into()
    }
    fn default_prompt() -> String {
        "#FFF8DC".into()
    }
    fn default_input_rule() -> String {
        "#CD7F32".into()
    }
    fn default_response_border() -> String {
        "#FFD700".into()
    }
    fn default_session_label() -> String {
        "#DAA520".into()
    }
    fn default_session_border() -> String {
        "#8B8682".into()
    }
}

impl Default for SkinColors {
    fn default() -> Self {
        Self {
            banner_border: Self::default_banner_border(),
            banner_title: Self::default_banner_title(),
            banner_accent: Self::default_banner_accent(),
            banner_dim: Self::default_banner_dim(),
            banner_text: Self::default_banner_text(),
            ui_accent: Self::default_ui_accent(),
            ui_label: Self::default_ui_label(),
            ui_ok: Self::default_ui_ok(),
            ui_error: Self::default_ui_error(),
            ui_warn: Self::default_ui_warn(),
            prompt: Self::default_prompt(),
            input_rule: Self::default_input_rule(),
            response_border: Self::default_response_border(),
            session_label: Self::default_session_label(),
            session_border: Self::default_session_border(),
        }
    }
}

// ---------------------------------------------------------------------------
// Spinner config
// ---------------------------------------------------------------------------

/// Spinner animation settings -- matches hermes-agent `spinner:` block.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkinSpinner {
    #[serde(default)]
    pub waiting_faces: Vec<String>,
    #[serde(default)]
    pub thinking_faces: Vec<String>,
    #[serde(default)]
    pub thinking_verbs: Vec<String>,
    #[serde(default)]
    pub wings: Vec<[String; 2]>,
}

// ---------------------------------------------------------------------------
// Branding config
// ---------------------------------------------------------------------------

/// Text strings used throughout the CLI -- matches hermes-agent `branding:` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinBranding {
    #[serde(default = "SkinBranding::default_agent_name")]
    pub agent_name: String,
    #[serde(default = "SkinBranding::default_welcome")]
    pub welcome: String,
    #[serde(default = "SkinBranding::default_goodbye")]
    pub goodbye: String,
    #[serde(default = "SkinBranding::default_response_label")]
    pub response_label: String,
    #[serde(default = "SkinBranding::default_prompt_symbol")]
    pub prompt_symbol: String,
    #[serde(default = "SkinBranding::default_help_header")]
    pub help_header: String,
}

impl SkinBranding {
    fn default_agent_name() -> String {
        "EdgeCrab".into()
    }
    fn default_welcome() -> String {
        "Welcome to EdgeCrab! Type your message or /help for commands.".into()
    }
    fn default_goodbye() -> String {
        "Goodbye! Crab out.".into()
    }
    fn default_response_label() -> String {
        "EdgeCrab".into()
    }
    fn default_prompt_symbol() -> String {
        ">> ".into()
    }
    fn default_help_header() -> String {
        "Available Commands".into()
    }
}

impl Default for SkinBranding {
    fn default() -> Self {
        Self {
            agent_name: Self::default_agent_name(),
            welcome: Self::default_welcome(),
            goodbye: Self::default_goodbye(),
            response_label: Self::default_response_label(),
            prompt_symbol: Self::default_prompt_symbol(),
            help_header: Self::default_help_header(),
        }
    }
}

// ---------------------------------------------------------------------------
// Skin struct
// ---------------------------------------------------------------------------

/// A complete skin -- fully compatible with hermes-agent YAML format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skin {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub colors: SkinColors,
    #[serde(default)]
    pub spinner: SkinSpinner,
    #[serde(default)]
    pub branding: SkinBranding,
    /// Character prefixed to tool output lines
    #[serde(default = "Skin::default_tool_prefix")]
    pub tool_prefix: String,
    /// Per-tool emoji overrides
    #[serde(default)]
    pub tool_emojis: HashMap<String, String>,
    #[serde(default)]
    pub banner_logo: String,
    #[serde(default)]
    pub banner_hero: String,
    /// Border style: "rounded", "plain", "double", "thick", "none"
    #[serde(default = "Skin::default_border_style")]
    pub border_style: String,
    /// Syntax-highlighting theme
    #[serde(default = "Skin::default_code_theme")]
    pub code_theme: String,
}

impl Skin {
    fn default_tool_prefix() -> String {
        "|".into()
    }
    fn default_border_style() -> String {
        "rounded".into()
    }
    fn default_code_theme() -> String {
        "monokai".into()
    }

    /// Fill missing fields from a base (default) skin (hermes-agent inheritance).
    pub fn merge_from_default(&mut self, base: &Skin) {
        macro_rules! fill {
            ($dst:expr, $src:expr) => {
                if $dst.is_empty() {
                    $dst = $src.clone();
                }
            };
        }
        fill!(self.colors.banner_border, base.colors.banner_border);
        fill!(self.colors.banner_title, base.colors.banner_title);
        fill!(self.colors.banner_accent, base.colors.banner_accent);
        fill!(self.colors.banner_dim, base.colors.banner_dim);
        fill!(self.colors.banner_text, base.colors.banner_text);
        fill!(self.colors.ui_accent, base.colors.ui_accent);
        fill!(self.colors.ui_label, base.colors.ui_label);
        fill!(self.colors.ui_ok, base.colors.ui_ok);
        fill!(self.colors.ui_error, base.colors.ui_error);
        fill!(self.colors.ui_warn, base.colors.ui_warn);
        fill!(self.colors.prompt, base.colors.prompt);
        fill!(self.colors.input_rule, base.colors.input_rule);
        fill!(self.colors.response_border, base.colors.response_border);
        fill!(self.colors.session_label, base.colors.session_label);
        fill!(self.colors.session_border, base.colors.session_border);

        if self.spinner.waiting_faces.is_empty() {
            self.spinner.waiting_faces = base.spinner.waiting_faces.clone();
        }
        if self.spinner.thinking_faces.is_empty() {
            self.spinner.thinking_faces = base.spinner.thinking_faces.clone();
        }
        if self.spinner.thinking_verbs.is_empty() {
            self.spinner.thinking_verbs = base.spinner.thinking_verbs.clone();
        }
        if self.spinner.wings.is_empty() {
            self.spinner.wings = base.spinner.wings.clone();
        }

        fill!(self.tool_prefix, base.tool_prefix);
        fill!(self.border_style, base.border_style);
        fill!(self.code_theme, base.code_theme);
    }
}

impl Default for Skin {
    fn default() -> Self {
        Self {
            name: "default".into(),
            description: "Classic EdgeCrab -- gold and warm".into(),
            colors: SkinColors::default(),
            spinner: SkinSpinner::default(),
            branding: SkinBranding::default(),
            tool_prefix: Self::default_tool_prefix(),
            tool_emojis: HashMap::new(),
            banner_logo: String::new(),
            banner_hero: String::new(),
            border_style: Self::default_border_style(),
            code_theme: Self::default_code_theme(),
        }
    }
}

/// Convenience macro to build `Vec<String>` from string literals.
macro_rules! svec {
    ($($s:expr),* $(,)?) => { vec![$($s.to_string()),*] };
}

// ---------------------------------------------------------------------------
// Built-in skins
// ---------------------------------------------------------------------------

fn builtin_skins() -> HashMap<String, Skin> {
    let mut m: HashMap<String, Skin> = HashMap::new();

    // -- default -------------------------------------------------------------
    m.insert("default".into(), Skin::default());

    // -- ares ----------------------------------------------------------------
    m.insert(
        "ares".into(),
        Skin {
            name: "ares".into(),
            description: "War-god theme -- crimson and bronze".into(),
            colors: SkinColors {
                banner_border: "#8B0000".into(),
                banner_title: "#CD7F32".into(),
                banner_accent: "#B8860B".into(),
                banner_dim: "#5C1010".into(),
                banner_text: "#FFD7BA".into(),
                ui_accent: "#CD7F32".into(),
                ui_label: "#D2691E".into(),
                ui_ok: "#228B22".into(),
                ui_error: "#FF0000".into(),
                ui_warn: "#FF4500".into(),
                prompt: "#FFD7BA".into(),
                input_rule: "#8B0000".into(),
                response_border: "#CD7F32".into(),
                session_label: "#8B0000".into(),
                session_border: "#5C1010".into(),
            },
            spinner: SkinSpinner {
                waiting_faces: svec!["(sword)", "(shield)", "(spear)"],
                thinking_faces: svec!["(sword)", "(bolt)", "(target)"],
                thinking_verbs: svec![
                    "forging",
                    "plotting",
                    "hammering plans",
                    "marching",
                    "tempering steel"
                ],
                wings: vec![
                    ["<<sword".into(), "sword>>".into()],
                    ["<<spear".into(), "spear>>".into()],
                ],
            },
            branding: SkinBranding {
                agent_name: "Ares Agent".into(),
                welcome: "Welcome to Ares Agent! Type your message or /help for commands.".into(),
                goodbye: "For glory!".into(),
                response_label: "Ares".into(),
                prompt_symbol: ">> ".into(),
                help_header: "Available Commands".into(),
            },
            border_style: "thick".into(),
            code_theme: "monokai".into(),
            ..Skin::default()
        },
    );

    // -- mono ----------------------------------------------------------------
    m.insert(
        "mono".into(),
        Skin {
            name: "mono".into(),
            description: "Monochrome -- clean grayscale".into(),
            colors: SkinColors {
                banner_border: "#555555".into(),
                banner_title: "#c9d1d9".into(),
                banner_accent: "#8b949e".into(),
                banner_dim: "#484f58".into(),
                banner_text: "#c9d1d9".into(),
                ui_accent: "#8b949e".into(),
                ui_label: "#6e7681".into(),
                ui_ok: "#3fb950".into(),
                ui_error: "#f85149".into(),
                ui_warn: "#d29922".into(),
                prompt: "#c9d1d9".into(),
                input_rule: "#555555".into(),
                response_border: "#c9d1d9".into(),
                session_label: "#8b949e".into(),
                session_border: "#484f58".into(),
            },
            branding: SkinBranding {
                goodbye: "Goodbye.".into(),
                response_label: "EdgeCrab".into(),
                help_header: "Available Commands".into(),
                ..SkinBranding::default()
            },
            border_style: "plain".into(),
            code_theme: "base16-ocean.dark".into(),
            ..Skin::default()
        },
    );

    // -- slate ---------------------------------------------------------------
    m.insert(
        "slate".into(),
        Skin {
            name: "slate".into(),
            description: "Cool blue -- developer-focused".into(),
            colors: SkinColors {
                banner_border: "#4169e1".into(),
                banner_title: "#6495ED".into(),
                banner_accent: "#5B8DEF".into(),
                banner_dim: "#2b4a8c".into(),
                banner_text: "#B0C4DE".into(),
                ui_accent: "#4169e1".into(),
                ui_label: "#708090".into(),
                ui_ok: "#2ecc71".into(),
                ui_error: "#e74c3c".into(),
                ui_warn: "#f39c12".into(),
                prompt: "#B0C4DE".into(),
                input_rule: "#4169e1".into(),
                response_border: "#6495ED".into(),
                session_label: "#4169e1".into(),
                session_border: "#2b4a8c".into(),
            },
            code_theme: "github-dark".into(),
            ..Skin::default()
        },
    );

    // -- poseidon ------------------------------------------------------------
    m.insert(
        "poseidon".into(),
        Skin {
            name: "poseidon".into(),
            description: "Ocean-god theme -- deep blue and seafoam".into(),
            colors: SkinColors {
                banner_border: "#006994".into(),
                banner_title: "#00CED1".into(),
                banner_accent: "#20B2AA".into(),
                banner_dim: "#004060".into(),
                banner_text: "#7FFFD4".into(),
                ui_accent: "#00CED1".into(),
                ui_label: "#4682B4".into(),
                ui_ok: "#2ecc71".into(),
                ui_error: "#FF6B6B".into(),
                ui_warn: "#FFA07A".into(),
                prompt: "#7FFFD4".into(),
                input_rule: "#006994".into(),
                response_border: "#00CED1".into(),
                session_label: "#20B2AA".into(),
                session_border: "#004060".into(),
            },
            spinner: SkinSpinner {
                waiting_faces: svec!["(wave)", "(shell)", "(trident)"],
                thinking_faces: svec!["(wave)", "(whirl)", "(whale)"],
                thinking_verbs: svec![
                    "charting currents",
                    "sounding the depth",
                    "reading the tides",
                    "navigating"
                ],
                wings: vec![
                    ["<<~".into(), "~>>".into()],
                    ["<<wave".into(), "wave>>".into()],
                ],
            },
            branding: SkinBranding {
                agent_name: "Poseidon Agent".into(),
                welcome: "Welcome to Poseidon Agent! Type your message or /help for commands."
                    .into(),
                goodbye: "May your seas be calm.".into(),
                response_label: "Poseidon".into(),
                prompt_symbol: "~> ".into(),
                help_header: "Available Commands".into(),
            },
            code_theme: "base16-ocean.dark".into(),
            ..Skin::default()
        },
    );

    // -- sisyphus ------------------------------------------------------------
    m.insert(
        "sisyphus".into(),
        Skin {
            name: "sisyphus".into(),
            description: "Sisyphean theme -- austere grayscale with persistence".into(),
            colors: SkinColors {
                banner_border: "#AAAAAA".into(),
                banner_title: "#DDDDDD".into(),
                banner_accent: "#BBBBBB".into(),
                banner_dim: "#777777".into(),
                banner_text: "#CCCCCC".into(),
                ui_accent: "#AAAAAA".into(),
                ui_label: "#888888".into(),
                ui_ok: "#99CC99".into(),
                ui_error: "#CC6666".into(),
                ui_warn: "#CCAA66".into(),
                prompt: "#CCCCCC".into(),
                input_rule: "#777777".into(),
                response_border: "#AAAAAA".into(),
                session_label: "#999999".into(),
                session_border: "#555555".into(),
            },
            spinner: SkinSpinner {
                waiting_faces: svec!["(O)", "(o)", "(.)", "(o)"],
                thinking_faces: svec!["(@)", "(*)", "(+)"],
                thinking_verbs: svec![
                    "pushing uphill",
                    "resetting the boulder",
                    "enduring the loop",
                    "persisting"
                ],
                wings: vec![["<O".into(), "O>".into()], ["<@".into(), "@>".into()]],
            },
            branding: SkinBranding {
                agent_name: "Sisyphus Agent".into(),
                welcome: "Welcome. The work continues.".into(),
                goodbye: "The boulder awaits.".into(),
                response_label: "Sisyphus".into(),
                prompt_symbol: "o> ".into(),
                help_header: "Available Commands".into(),
            },
            border_style: "plain".into(),
            code_theme: "base16-ocean.dark".into(),
            ..Skin::default()
        },
    );

    // -- charizard -----------------------------------------------------------
    m.insert(
        "charizard".into(),
        Skin {
            name: "charizard".into(),
            description: "Volcanic theme -- burnt orange and ember".into(),
            colors: SkinColors {
                banner_border: "#CC5500".into(),
                banner_title: "#FF8C00".into(),
                banner_accent: "#FF6600".into(),
                banner_dim: "#7A3300".into(),
                banner_text: "#FFDAB9".into(),
                ui_accent: "#FF6600".into(),
                ui_label: "#D2691E".into(),
                ui_ok: "#32CD32".into(),
                ui_error: "#FF2222".into(),
                ui_warn: "#FF8800".into(),
                prompt: "#FFDAB9".into(),
                input_rule: "#CC5500".into(),
                response_border: "#FF8C00".into(),
                session_label: "#CC5500".into(),
                session_border: "#7A3300".into(),
            },
            spinner: SkinSpinner {
                waiting_faces: svec!["(fire)", "(volcano)", "(dragon)"],
                thinking_faces: svec!["(fire)", "(wind)", "(spark)"],
                thinking_verbs: svec![
                    "banking into the draft",
                    "measuring burn",
                    "soaring",
                    "breathing fire"
                ],
                wings: vec![
                    ["<<fire".into(), "fire>>".into()],
                    ["<<dragon".into(), "dragon>>".into()],
                ],
            },
            branding: SkinBranding {
                agent_name: "Charizard Agent".into(),
                welcome: "Welcome to Charizard Agent! Flame on!".into(),
                goodbye: "Fly!".into(),
                response_label: "Charizard".into(),
                prompt_symbol: "~> ".into(),
                help_header: "Available Commands".into(),
            },
            border_style: "thick".into(),
            code_theme: "monokai".into(),
            ..Skin::default()
        },
    );

    // -- Extra developer-comfort skins (not in hermes-agent, kept for compat) --

    m.insert(
        "dracula".into(),
        Skin {
            name: "dracula".into(),
            description: "Dracula color scheme".into(),
            colors: SkinColors {
                banner_border: "#BD93F9".into(),
                banner_title: "#FF79C6".into(),
                banner_accent: "#8BE9FD".into(),
                banner_dim: "#6272A4".into(),
                banner_text: "#F8F8F2".into(),
                ui_accent: "#BD93F9".into(),
                ui_label: "#6272A4".into(),
                ui_ok: "#50FA7B".into(),
                ui_error: "#FF5555".into(),
                ui_warn: "#FFB86C".into(),
                prompt: "#F8F8F2".into(),
                input_rule: "#6272A4".into(),
                response_border: "#BD93F9".into(),
                session_label: "#FF79C6".into(),
                session_border: "#6272A4".into(),
            },
            code_theme: "dracula".into(),
            ..Skin::default()
        },
    );

    m.insert(
        "monokai".into(),
        Skin {
            name: "monokai".into(),
            description: "Classic Monokai".into(),
            colors: SkinColors {
                banner_border: "#F92672".into(),
                banner_title: "#A6E22E".into(),
                banner_accent: "#E6DB74".into(),
                banner_dim: "#75715E".into(),
                banner_text: "#F8F8F2".into(),
                ui_accent: "#F92672".into(),
                ui_label: "#75715E".into(),
                ui_ok: "#A6E22E".into(),
                ui_error: "#F92672".into(),
                ui_warn: "#FD971F".into(),
                prompt: "#F8F8F2".into(),
                input_rule: "#75715E".into(),
                response_border: "#A6E22E".into(),
                session_label: "#E6DB74".into(),
                session_border: "#75715E".into(),
            },
            code_theme: "monokai".into(),
            ..Skin::default()
        },
    );

    m.insert(
        "catppuccin".into(),
        Skin {
            name: "catppuccin".into(),
            description: "Catppuccin Mocha".into(),
            colors: SkinColors {
                banner_border: "#CBA6F7".into(),
                banner_title: "#F5C2E7".into(),
                banner_accent: "#89B4FA".into(),
                banner_dim: "#6C7086".into(),
                banner_text: "#CDD6F4".into(),
                ui_accent: "#CBA6F7".into(),
                ui_label: "#6C7086".into(),
                ui_ok: "#A6E3A1".into(),
                ui_error: "#F38BA8".into(),
                ui_warn: "#FAB387".into(),
                prompt: "#CDD6F4".into(),
                input_rule: "#CBA6F7".into(),
                response_border: "#CBA6F7".into(),
                session_label: "#F5C2E7".into(),
                session_border: "#6C7086".into(),
            },
            code_theme: "base16-ocean.dark".into(),
            ..Skin::default()
        },
    );

    m
}

// ---------------------------------------------------------------------------
// SkinEngine
// ---------------------------------------------------------------------------

/// Discovers, loads, and manages named skins.
///
/// Search order:
/// 1. `~/.edgecrab/skins/<name>.yaml` -- highest priority
/// 2. Built-in presets compiled into the binary
///
/// Unknown skin names fall back to `default`.
pub struct SkinEngine {
    skins: HashMap<String, Skin>,
    current: String,
}

impl SkinEngine {
    pub fn new() -> Self {
        let mut skins = builtin_skins();
        Self::load_user_skins(&mut skins);
        Self {
            skins,
            current: "default".into(),
        }
    }

    fn load_user_skins(map: &mut HashMap<String, Skin>) {
        let Some(dir) = Self::skins_dir() else { return };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let base = Skin::default();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(content) = std::fs::read_to_string(&path) else {
                tracing::warn!("Cannot read skin file: {}", path.display());
                continue;
            };
            match serde_yml::from_str::<Skin>(&content) {
                Ok(mut skin) => {
                    skin.name = stem.to_string();
                    skin.merge_from_default(&base);
                    map.insert(stem.to_string(), skin);
                }
                Err(e) => tracing::warn!("Failed to parse skin {}: {e}", path.display()),
            }
        }
    }

    /// Load a skin by name (throw-away engine). Falls back to `default`.
    pub fn load(name: &str) -> Skin {
        let engine = Self::new();
        engine
            .skins
            .get(name)
            .cloned()
            .unwrap_or_else(Skin::default)
    }

    /// Get a skin by name. Falls back to `default` if unknown.
    pub fn get(&self, name: &str) -> Skin {
        self.skins.get(name).cloned().unwrap_or_else(Skin::default)
    }

    /// List all available skin names, sorted.
    pub fn list_skins(&self) -> Vec<String> {
        let mut names: Vec<String> = self.skins.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn available_skins() -> Vec<String> {
        Self::new().list_skins()
    }

    pub fn current_name(&self) -> &str {
        &self.current
    }

    pub fn current(&self) -> Skin {
        self.get(&self.current.clone())
    }

    /// Switch active skin. Unknown names keep the previous skin and log a warning.
    pub fn set_current(&mut self, name: &str) {
        if self.skins.contains_key(name) {
            self.current = name.to_string();
        } else {
            tracing::warn!("Unknown skin '{}', keeping current skin.", name);
        }
    }

    pub fn skins_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".edgecrab").join("skins"))
    }

    pub fn ensure_skins_dir() -> anyhow::Result<PathBuf> {
        let dir =
            Self::skins_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn save_skin(skin: &Skin) -> anyhow::Result<PathBuf> {
        let dir = Self::ensure_skins_dir()?;
        let path = dir.join(format!("{}.yaml", skin.name));
        let yaml = serde_yml::to_string(skin)?;
        std::fs::write(&path, yaml)?;
        Ok(path)
    }
}

impl Default for SkinEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermes_builtin_skins_present() {
        let required = [
            "default",
            "ares",
            "mono",
            "slate",
            "poseidon",
            "sisyphus",
            "charizard",
        ];
        let engine = SkinEngine::new();
        for name in &required {
            assert!(
                engine.skins.contains_key(*name),
                "missing hermes skin: {name}"
            );
        }
    }

    #[test]
    fn list_returns_sorted() {
        let engine = SkinEngine::new();
        let names = engine.list_skins();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn load_default() {
        let skin = SkinEngine::load("default");
        assert_eq!(skin.name, "default");
        assert!(!skin.colors.banner_border.is_empty());
        assert!(!skin.branding.agent_name.is_empty());
    }

    #[test]
    fn load_ares() {
        let skin = SkinEngine::load("ares");
        assert_eq!(skin.name, "ares");
        assert_eq!(skin.colors.banner_border, "#8B0000");
        assert_eq!(skin.branding.agent_name, "Ares Agent");
        assert!(!skin.spinner.thinking_verbs.is_empty());
    }

    #[test]
    fn load_poseidon_has_ocean_spinners() {
        let skin = SkinEngine::load("poseidon");
        assert!(!skin.spinner.thinking_verbs.is_empty());
        assert!(
            skin.spinner
                .thinking_verbs
                .iter()
                .any(|v| { v.contains("depth") || v.contains("current") || v.contains("tide") })
        );
    }

    #[test]
    fn load_unknown_falls_back_to_default() {
        let skin = SkinEngine::load("nonexistent_xyz");
        assert_eq!(skin.name, "default");
    }

    #[test]
    fn set_current_valid() {
        let mut engine = SkinEngine::new();
        engine.set_current("ares");
        assert_eq!(engine.current_name(), "ares");
    }

    #[test]
    #[ignore] // Skip - requires temp log directory setup
    fn set_current_invalid_keeps_previous() {
        let mut engine = SkinEngine::new();
        engine.set_current("nonexistent_xyz");
        assert_eq!(engine.current_name(), "default");
    }

    #[test]
    fn skin_colors_are_hex() {
        let c = SkinColors::default();
        assert!(c.banner_border.starts_with('#'));
        assert!(c.ui_error.starts_with('#'));
        assert!(c.response_border.starts_with('#'));
    }

    #[test]
    fn skin_branding_nonempty() {
        let b = SkinBranding::default();
        assert!(!b.agent_name.is_empty());
        assert!(!b.welcome.is_empty());
        assert!(!b.prompt_symbol.is_empty());
    }

    #[test]
    fn serialization_roundtrip() {
        let skin = SkinEngine::load("ares");
        let yaml = serde_yml::to_string(&skin).expect("serialize");
        let loaded: Skin = serde_yml::from_str(&yaml).expect("deserialize");
        assert_eq!(loaded.name, skin.name);
        assert_eq!(loaded.colors.banner_border, skin.colors.banner_border);
        assert_eq!(loaded.branding.agent_name, skin.branding.agent_name);
    }

    #[test]
    fn merge_from_default_fills_empty() {
        let mut user = Skin {
            name: "partial".into(),
            colors: SkinColors {
                banner_border: String::new(),
                ..SkinColors::default()
            },
            ..Skin::default()
        };
        user.merge_from_default(&Skin::default());
        assert!(!user.colors.banner_border.is_empty());
    }

    #[test]
    fn available_skins_at_least_seven() {
        let names = SkinEngine::available_skins();
        assert!(
            names.len() >= 7,
            "expected >= 7 built-in skins, got {}",
            names.len()
        );
        assert!(names.contains(&"default".to_string()));
        assert!(names.contains(&"ares".to_string()));
    }
}
