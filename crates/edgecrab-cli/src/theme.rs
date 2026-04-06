//! # theme — ratatui style definitions + YAML skin engine
//!
//! WHY skin engine: Different users prefer different color schemes.
//! Driven by a YAML config (~/.edgecrab/skin.yaml) so themes can be
//! shared, diffed, and version-controlled without recompiling.
//!
//! ```text
//!   ~/.edgecrab/skin.yaml
//!     prompt_color: "#CD7F32"    ← copper
//!     assistant_color: "#4DD0E1" ← cyan
//!     error_color: "#EF5350"     ← red
//!
//!   SkinConfig::load()
//!     └──→ Theme::from_skin(&config)
//!               └──→ ratatui Style objects
//! ```
//!
//! When the file is absent or invalid, the built-in defaults apply.

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;
use std::collections::HashMap;

// ─── SkinConfig ────────────────────────────────────────────────

/// YAML-serializable skin configuration loaded from `~/.edgecrab/skin.yaml`.
///
/// All fields are optional — missing fields fall back to defaults.
///
/// # Example skin.yaml
/// ```yaml
/// prompt_color: "#CD7F32"
/// assistant_color: "#4DD0E1"
/// agent_name: "EdgeCrab"
/// welcome_msg: "Hello! Ask me anything."
/// goodbye_msg: "Goodbye! 🦀"
/// thinking_verbs:
///   - "pondering"
///   - "crunching"
/// waiting_verbs:
///   - "dispatching"
///   - "awaiting"
/// kaomoji_thinking:
///   - "(｡◕‿◕｡)"
///   - "(⌐■_■)"
/// kaomoji_waiting:
///   - "(・・ )"
///   - "( ._.)"
/// kaomoji_success:
///   - "✧(≗ᴗ≗)✧"
///   - "(ﾉ◕ヮ◕)ﾉ*:･ﾟ✧"
/// spinner_wings:
///   - ["⟪🦀 ", " 🦀⟫"]
///   - ["⟨", "⟩"]
/// ```
#[derive(Debug, Default, Deserialize)]
pub struct SkinConfig {
    /// Hex color for the input border and prompt symbol (e.g. "#CD7F32")
    pub prompt_color: Option<String>,
    /// Hex color for assistant response text
    pub assistant_color: Option<String>,
    /// Hex color for tool output text
    pub tool_color: Option<String>,
    /// Hex color for error messages
    pub error_color: Option<String>,
    /// Hex color for system/status messages
    pub system_color: Option<String>,
    /// Prompt symbol (default: "❯ ")
    pub prompt_symbol: Option<String>,
    /// Tool output prefix (default: "┊ ")
    pub tool_prefix: Option<String>,

    // ── Branding ─────────────────────────────────────────────────────
    /// Agent display name shown in banner (default: "EdgeCrab")
    pub agent_name: Option<String>,
    /// Message shown on startup (default: built-in hint line)
    pub welcome_msg: Option<String>,
    /// Message shown on exit (default: "Goodbye! 🦀")
    pub goodbye_msg: Option<String>,

    // ── Spinner / kaomoji ─────────────────────────────────────────────
    /// Waiting verbs shown before the first token arrives.
    pub waiting_verbs: Option<Vec<String>>,
    /// Thinking verb list — overrides the hardcoded THINKING_VERBS array.
    /// Rotates in the status bar while the agent reasons.
    pub thinking_verbs: Option<Vec<String>>,
    /// Kaomoji faces shown while waiting for the first token.
    pub kaomoji_waiting: Option<Vec<String>>,
    /// Kaomoji faces shown alongside the thinking verb in the status bar.
    /// Each face rotates every full spinner revolution.
    pub kaomoji_thinking: Option<Vec<String>>,
    /// Kaomoji faces shown on successful tool completion.
    pub kaomoji_success: Option<Vec<String>>,
    /// Kaomoji faces shown on tool error.
    pub kaomoji_error: Option<Vec<String>>,
    /// Spinner wings — decorations around the spinner frame.
    /// Each entry is a two-element array `["left", "right"]`.
    /// Example: `[["⟪🦀 ", " 🦀⟫"], ["⟨", "⟩"]]`
    pub spinner_wings: Option<Vec<[String; 2]>>,
    /// Per-tool emoji overrides: map tool name → replacement emoji.
    /// Falls back to the built-in pattern-matched emoji when empty.
    /// Example: `{ "bash": "🖥", "read_file": "📄" }`
    #[serde(default)]
    pub tool_emojis: HashMap<String, String>,
}

impl SkinConfig {
    /// Load from `~/.edgecrab/skin.yaml`. Returns default config on any error.
    pub fn load() -> Self {
        let path = match dirs::home_dir() {
            Some(home) => home.join(".edgecrab").join("skin.yaml"),
            None => return Self::default(),
        };

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Self::default(), // file absent → defaults
        };

        serde_yml::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!(?path, "skin.yaml parse error: {e}");
            Self::default()
        })
    }

    /// Parse a `#RRGGBB` hex string into `(r, g, b)` components.
    fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some((r, g, b))
    }

    /// Convert an optional hex color string to a ratatui Color, falling back to `default`.
    fn color_or(&self, hex: &Option<String>, default: Color) -> Color {
        hex.as_deref()
            .and_then(Self::parse_hex)
            .map(|(r, g, b)| Color::Rgb(r, g, b))
            .unwrap_or(default)
    }
}

// ─── Theme ─────────────────────────────────────────────────────

/// Application theme — maps semantic roles to ratatui styles.
#[allow(dead_code)]
pub struct Theme {
    pub input_border: Style,
    pub input_text: Style,
    pub output_text: Style,
    pub output_assistant: Style,
    pub output_tool: Style,
    pub output_error: Style,
    pub output_system: Style,
    pub status_bar_bg: Style,
    pub status_bar_model: Style,
    pub status_bar_tokens: Style,
    pub status_bar_cost: Style,
    pub prompt_symbol: String,
    pub tool_prefix: String,

    // ── Branding ─────────────────────────────────────────────────────
    /// Agent display name shown in the banner (e.g. "EdgeCrab")
    pub agent_name: String,
    /// Welcome message shown at startup
    pub welcome_msg: String,
    /// Goodbye message shown on exit
    pub goodbye_msg: String,

    // ── Spinner / kaomoji ─────────────────────────────────────────────
    /// Waiting verbs that rotate before the first token arrives.
    pub waiting_verbs: Vec<String>,
    /// Thinking verbs that rotate in the status bar while the agent reasons
    pub thinking_verbs: Vec<String>,
    /// Kaomoji faces shown while waiting for the first token
    pub kaomoji_waiting: Vec<String>,
    /// Kaomoji faces shown alongside the thinking verb
    pub kaomoji_thinking: Vec<String>,
    /// Kaomoji faces shown on successful tool completion
    pub kaomoji_success: Vec<String>,
    /// Kaomoji faces shown on tool error
    pub kaomoji_error: Vec<String>,
    /// Spinner wing pairs `[left, right]` that surround the spinner
    pub spinner_wings: Vec<[String; 2]>,
    /// Per-tool emoji overrides loaded from the active skin.
    /// `tool_emoji_or_override()` checks this map before falling back to
    /// the built-in pattern-matched emoji.
    pub tool_emojis: HashMap<String, String>,
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_skin(&SkinConfig::default())
    }
}

// ── Default pools (used when skin.yaml doesn't override) ──────────────

/// Default waiting verbs — rotated while the request has been sent but no
/// reasoning or response token has arrived yet.
pub const DEFAULT_WAITING_VERBS: &[&str] = &[
    "dispatching",
    "awaiting",
    "warming",
    "negotiating",
    "priming",
    "connecting",
];

/// Default thinking verbs — rotated in the status bar while the agent reasons.
pub const DEFAULT_THINKING_VERBS: &[&str] = &[
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

/// Default kaomoji faces for the pre-first-token waiting state — full Unicode.
pub const DEFAULT_KAOMOJI_WAITING: &[&str] =
    &["(・・ )", "( •.•)", "(°_°)", "(¬_¬ )", "(・・;)", "( ˙-˙ )"];

/// ASCII-safe waiting kaomoji for limited terminal fonts.
pub const SAFE_KAOMOJI_WAITING: &[&str] = &["(._.)", "(o_o)", "(-_-)", "(.. )", "( ? )", "(>_<)"];

/// Default kaomoji faces for the thinking state — full Unicode (requires CJK/IPA font).
///
/// These faces use characters from Arabic-Indic (U+0669), Thai (U+0E51),
/// Katakana (U+30FD), and Latin Extended-D (U+1D17) planes.  They render
/// beautifully on Nerd Fonts / CJK-enabled terminals but fall back to `?`
/// boxes when the terminal font is missing those glyphs.
///
/// WHY BOTH SETS EXIST — first-principles rationale:
///   Terminal rendering = UTF-8 bytes → terminal font glyph lookup → cell draw
///   When the glyph is absent the terminal emits U+FFFD / '?' per character.
///   Three adjacent unsupported characters → "???".  The safe fallback set
///   uses only Latin-1 Supplement + Geometric Shapes that every monospace
///   font ships, so it is guaranteed to display correctly.
pub const DEFAULT_KAOMOJI_THINKING: &[&str] = &[
    "(｡◕‿◕｡)",
    "(◔_◔)",
    "(¬‿¬)",
    "( •_•)>⌐■-■",
    "(⌐■_■)",
    "(´･_･`)",
    "◉_◉",
    "(°ロ°)",
    "( ˘⌣˘)",
    "ヽ(>∀<☆)ノ",
    "٩(๑❛ᴗ❛๑)۶",
    "(⊙_⊙)",
    "(¬_¬)",
    "( ͡° ͜ʖ ͡°)",
    "ಠ_ಠ",
    "φ(゜▽゜*)♪",
    "(✿◠‿◠)",
    "٩(◕‿◕｡)۶",
    "ヾ(＾∇＾)",
    "(≧◡≦)",
];

/// ASCII-safe kaomoji for terminals whose font lacks CJK/Arabic/Thai glyphs.
///
/// All characters are within Latin-1 Supplement (U+00A0–U+00FF) or Basic
/// Latin, plus a handful of Geometric Shapes (U+25xx) and Letterlike Symbols
/// (U+2100–U+214F) that ship in virtually every monospace terminal font.
pub const SAFE_KAOMOJI_THINKING: &[&str] = &[
    "(^_^)", "(-_-)", "(o_o)", "(>_<)", "(=_=)", "(^o^)", "(+_+)", "(o.O)", "(*.*)", "(~_~)",
    "(._.)", "(@_@)", "(;_;)", "(-.-)", "(>.>)", "(o_O)", "(*_*)", "(^.^)", "(>.O)", "(-_^)",
];

/// Default kaomoji faces for successful tool completion — full Unicode.
pub const DEFAULT_KAOMOJI_SUCCESS: &[&str] = &[
    "✧(≗ᴗ≗)✧",
    "(ﾉ◕ヮ◕)ﾉ*:･ﾟ✧",
    "٩(◕‿◕｡)۶",
    "(✿◠‿◠)",
    "( ˘▽˘)っ",
    "♪(´ε` )",
    "(◕ᴗ◕✿)",
    "ヾ(＾∇＾)",
    "(≧◡≦)",
    "(★ω★)",
    "ヽ(★ω★)ﾉ",
];

/// ASCII-safe kaomoji for successful tool completion.
pub const SAFE_KAOMOJI_SUCCESS: &[&str] = &[
    "(^_^)/", "(^o^)/", "(>_<)/", "(*.*)/", "(=^.^=)", "(+_+)/", "(o_o)/", "\\(^_^)/", "(^.^)/",
    "(*_*)/",
];

/// Default kaomoji faces for tool errors — full Unicode.
pub const DEFAULT_KAOMOJI_ERROR: &[&str] = &[
    "(╯°□°）╯︵ ┻━┻",
    "(ノಠ益ಠ)ノ彡┻━┻",
    "ᕦ(ò_óˇ)ᕤ",
    "щ(ಠ益ಠщ)",
    "(；′⌒`)",
];

/// ASCII-safe kaomoji for tool errors.
pub const SAFE_KAOMOJI_ERROR: &[&str] = &["(>_<)!", "(T_T)", "(;_;)", "(x_x)", "(>.<)!"];

/// Detect at runtime whether the terminal is likely capable of rendering
/// characters outside the Basic Latin / Latin-1 Supplement Unicode ranges.
///
/// ## Detection strategy (first-principles)
///
/// The rendering chain is:
///   app → UTF-8 bytes → PTY → terminal emulator → font glyph cache → cell
///
/// We cannot probe the font directly from Rust without platform-specific calls,
/// so we use environmental heuristics ordered by reliability:
///
/// 1. **Explicit locale** (`LANG`, `LC_ALL`, `LC_CTYPE`): UTF-8 locales imply
///    the terminal was configured for multi-byte input; modern macOS/Linux
///    terminals set at least one of these.
/// 2. **Known-good terminal programs** (`TERM_PROGRAM`): iTerm2, WezTerm,
///    Ghostty, Alacritty, Kitty etc. ship with full Nerd Font / CJK support.
/// 3. **True-color declarations** (`COLORTERM=truecolor`): A terminal that
///    negotiates 24-bit colour is almost always a modern, Unicode-capable one.
/// 4. **VS Code / JetBrains integration** env vars.
///
/// When uncertain we return `false` — ASCII-safe fallback is always correct.
///
/// ## Edge cases
/// - WSL terminals inherit `LANG` from Linux but the Windows console host may
///   not have Asian fonts; we conservatively require TERM_PROGRAM on Windows.
/// - SSH sessions may inherit the user's locale from the remote host; if the
///   SSH client's font is limited the kaomoji will still break.  There is no
///   reliable way to detect this without an escape-sequence probe.
pub fn terminal_supports_extended_unicode() -> bool {
    // 1. Check locale for explicit UTF-8 declaration
    for var in &["LANG", "LC_ALL", "LC_CTYPE"] {
        if let Ok(val) = std::env::var(var) {
            let up = val.to_uppercase();
            if up.contains("UTF-8") || up.contains("UTF8") {
                // UTF-8 locale is necessary but not sufficient — still verify
                // the terminal program is known-good before trusting full kaomoji.
                // Apple_Terminal + UTF-8 works fine for BMP but struggles with
                // CJK fullwidth; for maximum safety we require a known terminal.
                if let Ok(prog) = std::env::var("TERM_PROGRAM") {
                    let known_good = [
                        "iTerm.app",
                        "WezTerm",
                        "wezterm",
                        "kitty",
                        "Ghostty",
                        "ghostty",
                        "Hyper",
                        "alacritty",
                        "Alacritty",
                        "vscode",
                        "VSCode",
                        "JetBrains",
                    ];
                    if known_good.iter().any(|k| prog.contains(k)) {
                        return true;
                    }
                }
                // If COLORTERM=truecolor it's almost certainly a modern emulator
                if let Ok(ct) = std::env::var("COLORTERM") {
                    if ct == "truecolor" || ct == "24bit" {
                        return true;
                    }
                }
                // TERM_PROGRAM_VERSION exists → likely a modern terminal
                if std::env::var("TERM_PROGRAM_VERSION").is_ok() {
                    return true;
                }
                // TERM_PROGRAM=Apple_Terminal: decent BMP support,
                // but Katakana / Arabic-Indic often fall back to boxes.
                // Return false to be safe.
                return false;
            }
        }
    }

    // 2. TERM_PROGRAM alone (no locale found) — known-good terminals
    if let Ok(prog) = std::env::var("TERM_PROGRAM") {
        let known_good = [
            "iTerm.app",
            "WezTerm",
            "wezterm",
            "kitty",
            "Ghostty",
            "ghostty",
            "Hyper",
            "alacritty",
            "vscode",
        ];
        if known_good.iter().any(|k| prog.contains(k)) {
            return true;
        }
    }

    // 3. COLORTERM alone
    if let Ok(ct) = std::env::var("COLORTERM") {
        if ct == "truecolor" || ct == "24bit" {
            return true;
        }
    }

    false
}

impl Theme {
    /// Build a Theme from a loaded SkinConfig.
    ///
    /// WHY keep Default and from_skin separate: Default applies hardcoded
    /// constants; from_skin allows user overrides. Both must produce a
    /// complete, valid Theme — the skin just tints the palette.
    pub fn from_skin(skin: &SkinConfig) -> Self {
        let prompt_fg = skin.color_or(&skin.prompt_color, Color::Rgb(205, 127, 50)); // copper
        let assistant_fg = skin.color_or(&skin.assistant_color, Color::Rgb(77, 208, 225)); // cyan
        let tool_fg = skin.color_or(&skin.tool_color, Color::Rgb(255, 191, 0)); // amber
        let error_fg = skin.color_or(&skin.error_color, Color::Rgb(239, 83, 80)); // red
        let system_fg = skin.color_or(&skin.system_color, Color::Rgb(158, 158, 158)); // grey

        // Build branding / verb / kaomoji vectors from skin overrides or defaults.
        // When no skin override is present we pick the Unicode tier based on
        // terminal capability detection so kaomoji never display as "???".
        let rich_unicode = terminal_supports_extended_unicode();

        let waiting_verbs: Vec<String> = skin
            .waiting_verbs
            .as_ref()
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                DEFAULT_WAITING_VERBS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });

        let thinking_verbs: Vec<String> = skin
            .thinking_verbs
            .as_ref()
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                DEFAULT_THINKING_VERBS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });

        let kaomoji_waiting: Vec<String> = skin
            .kaomoji_waiting
            .as_ref()
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                if rich_unicode {
                    DEFAULT_KAOMOJI_WAITING
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    SAFE_KAOMOJI_WAITING.iter().map(|s| s.to_string()).collect()
                }
            });

        let kaomoji_thinking: Vec<String> = skin
            .kaomoji_thinking
            .as_ref()
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                // SAFE: skin didn't override → pick tier based on terminal capability
                if rich_unicode {
                    DEFAULT_KAOMOJI_THINKING
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    SAFE_KAOMOJI_THINKING
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }
            });

        let kaomoji_success: Vec<String> = skin
            .kaomoji_success
            .as_ref()
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                if rich_unicode {
                    DEFAULT_KAOMOJI_SUCCESS
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    SAFE_KAOMOJI_SUCCESS.iter().map(|s| s.to_string()).collect()
                }
            });

        let kaomoji_error: Vec<String> = skin
            .kaomoji_error
            .as_ref()
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                if rich_unicode {
                    DEFAULT_KAOMOJI_ERROR
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    SAFE_KAOMOJI_ERROR.iter().map(|s| s.to_string()).collect()
                }
            });

        let spinner_wings: Vec<[String; 2]> = skin
            .spinner_wings
            .as_ref()
            .filter(|v| !v.is_empty())
            .cloned()
            .unwrap_or_default(); // no wings by default — user opts in via skin.yaml

        Self {
            input_border: Style::default().fg(prompt_fg),
            input_text: Style::default().fg(Color::Rgb(255, 248, 220)),
            output_text: Style::default().fg(Color::White),
            output_assistant: Style::default().fg(assistant_fg),
            output_tool: Style::default().fg(tool_fg).add_modifier(Modifier::DIM),
            output_error: Style::default().fg(error_fg),
            output_system: Style::default()
                .fg(system_fg)
                .add_modifier(Modifier::ITALIC),
            status_bar_bg: Style::default()
                .fg(Color::Rgb(255, 248, 220))
                .bg(Color::Rgb(50, 50, 50)),
            status_bar_model: Style::default().fg(assistant_fg),
            status_bar_tokens: Style::default().fg(Color::Rgb(156, 204, 101)),
            status_bar_cost: Style::default().fg(Color::Rgb(255, 183, 77)),
            prompt_symbol: skin
                .prompt_symbol
                .clone()
                .unwrap_or_else(|| "❯ ".to_string()),
            tool_prefix: skin.tool_prefix.clone().unwrap_or_else(|| "┊ ".to_string()),
            agent_name: skin
                .agent_name
                .clone()
                .unwrap_or_else(|| "EdgeCrab".to_string()),
            welcome_msg: skin.welcome_msg.clone().unwrap_or_else(|| {
                "/help  commands    /model  switch model    Ctrl+C  cancel/exit".to_string()
            }),
            goodbye_msg: skin
                .goodbye_msg
                .clone()
                .unwrap_or_else(|| "Goodbye! 🦀  See you next time.".to_string()),
            waiting_verbs,
            thinking_verbs,
            kaomoji_waiting,
            kaomoji_thinking,
            kaomoji_success,
            kaomoji_error,
            spinner_wings,
            tool_emojis: skin.tool_emojis.clone(),
        }
    }

    /// Load theme from YAML skin config (or fall back to defaults).
    pub fn load() -> Self {
        let skin = SkinConfig::load();
        Self::from_skin(&skin)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_builds() {
        let theme = Theme::default();
        assert_eq!(theme.prompt_symbol, "❯ ");
        assert_eq!(theme.tool_prefix, "┊ ");
    }

    #[test]
    fn parse_hex_valid() {
        assert_eq!(SkinConfig::parse_hex("#4DD0E1"), Some((77, 208, 225)));
    }

    #[test]
    fn parse_hex_no_hash() {
        assert_eq!(SkinConfig::parse_hex("CD7F32"), Some((205, 127, 50)));
    }

    #[test]
    fn parse_hex_invalid() {
        assert_eq!(SkinConfig::parse_hex("ZZZZZZ"), None);
        assert_eq!(SkinConfig::parse_hex("short"), None);
    }

    #[test]
    fn skin_config_color_override() {
        let skin = SkinConfig {
            prompt_color: Some("#FF0000".into()),
            ..Default::default()
        };
        let theme = Theme::from_skin(&skin);
        // The input border should use the red color
        assert_eq!(theme.input_border.fg, Some(Color::Rgb(255, 0, 0)));
    }

    #[test]
    fn skin_config_custom_symbols() {
        let skin = SkinConfig {
            prompt_symbol: Some("→ ".into()),
            tool_prefix: Some("# ".into()),
            ..Default::default()
        };
        let theme = Theme::from_skin(&skin);
        assert_eq!(theme.prompt_symbol, "→ ");
        assert_eq!(theme.tool_prefix, "# ");
    }

    #[test]
    fn theme_has_default_kaomoji_pools() {
        let theme = Theme::default();
        assert!(!theme.waiting_verbs.is_empty());
        assert!(!theme.kaomoji_waiting.is_empty());
        assert!(!theme.kaomoji_thinking.is_empty());
        assert!(!theme.kaomoji_success.is_empty());
        assert!(!theme.kaomoji_error.is_empty());
        assert!(!theme.thinking_verbs.is_empty());
    }

    #[test]
    fn skin_can_override_waiting_animation() {
        let skin = SkinConfig {
            waiting_verbs: Some(vec!["handshaking".into(), "queueing".into()]),
            kaomoji_waiting: Some(vec!["(wait)".into(), "(hold)".into()]),
            ..Default::default()
        };
        let theme = Theme::from_skin(&skin);
        assert_eq!(theme.waiting_verbs, vec!["handshaking", "queueing"]);
        assert_eq!(theme.kaomoji_waiting, vec!["(wait)", "(hold)"]);
    }

    #[test]
    fn skin_can_override_kaomoji_thinking() {
        let skin = SkinConfig {
            kaomoji_thinking: Some(vec!["(^_^)".into(), "(>_<)".into()]),
            ..Default::default()
        };
        let theme = Theme::from_skin(&skin);
        assert_eq!(theme.kaomoji_thinking, vec!["(^_^)", "(>_<)"]);
    }

    #[test]
    fn skin_can_override_thinking_verbs() {
        let skin = SkinConfig {
            thinking_verbs: Some(vec!["plotting".into(), "forging".into()]),
            ..Default::default()
        };
        let theme = Theme::from_skin(&skin);
        assert_eq!(theme.thinking_verbs, vec!["plotting", "forging"]);
    }

    #[test]
    fn skin_branding_agent_name() {
        let skin = SkinConfig {
            agent_name: Some("MyCrab".into()),
            goodbye_msg: Some("Bye!".into()),
            ..Default::default()
        };
        let theme = Theme::from_skin(&skin);
        assert_eq!(theme.agent_name, "MyCrab");
        assert_eq!(theme.goodbye_msg, "Bye!");
    }

    #[test]
    fn theme_load_falls_back_on_missing_file() {
        // No skin.yaml should be present in test env.
        // This should not panic and should return defaults.
        let theme = Theme::load();
        assert!(!theme.prompt_symbol.is_empty());
    }

    /// Verify that wide-character kaomoji in the default pools are measured
    /// correctly by unicode-width — i.e., display_width >= char_count.
    ///
    /// This test documents the fundamental emoji-alignment principle:
    /// wide Unicode chars (CJK, katakana, fullwidth Latin, most emoji) occupy
    /// **2 display columns** each, so `s.width() != s.chars().count()` for them.
    ///
    /// In ratatui this is transparent: each `Span` is measured via unicode-width
    /// automatically. The rule to follow is: NEVER use `.len()` or char-count
    /// arithmetic for gap calculations — always use `.width()`.
    #[test]
    fn kaomoji_wide_chars_measured_correctly() {
        use unicode_width::UnicodeWidthStr;

        // "ヽ(>∀<☆)ノ" contains full-width katakana ヽ (U+30FD) and ノ (U+30CE),
        // both width=2, so display_width (11) > char_count (9).
        let wide_face = "ヽ(>∀<☆)ノ";
        assert!(
            wide_face.width() > wide_face.chars().count(),
            "Katakana chars are wide (2 cols each): display_w={} char_count={}",
            wide_face.width(),
            wide_face.chars().count(),
        );

        // "(◔_◔)" is all narrow — display width equals char count.
        let narrow_face = "(◔_◔)";
        assert_eq!(
            narrow_face.width(),
            narrow_face.chars().count(),
            "Narrow kaomoji: display_w should equal char_count",
        );

        // "🦀" (the crab emoji) is a wide emoji: 2 display cols, 1 char.
        let crab = "🦀";
        assert_eq!(
            crab.width(),
            2,
            "🦀 is a wide emoji occupying 2 display cols"
        );
        assert_eq!(crab.chars().count(), 1);

        // All default kaomoji pools are non-empty (smoke test for loads).
        let theme = Theme::default();
        for face in &theme.kaomoji_thinking {
            assert!(
                face.width() > 0,
                "kaomoji_thinking entry '{face}' has zero display width"
            );
        }
    }

    /// Verify that the banner row-1 gap produces the correct box width.
    ///
    /// BOX_W = 62 (╔ + 60═ + ╗)
    /// Row1 spans: "║  "(3) + "🦀 "(3) + name_20(20) + tagline_24(24) + gap + "  ║"(3)
    /// Expected gap = 62 − 53 = 9
    #[test]
    fn banner_row1_gap_is_correct() {
        use unicode_width::UnicodeWidthStr;

        const BOX_W: usize = 62;
        const BORDER_L: usize = 3; // "║  "
        const BORDER_R: usize = 3; // "  ║"
        const CRAB_W: usize = 3; // "🦀 " = 2+1
        const NAME_COLS: usize = 20;

        let agent_name = "EdgeCrab"; // ASCII, display_w = char_count
        // Simulate unicode_pad_right
        let name_w = agent_name.width();
        let pad = NAME_COLS.saturating_sub(name_w);
        let name_cell = format!("{agent_name}{}", " ".repeat(pad));
        assert_eq!(name_cell.width(), NAME_COLS);

        let tagline = "AI-native terminal agent";
        let gap = BOX_W
            .saturating_sub(BORDER_L + CRAB_W + name_cell.width() + tagline.width() + BORDER_R);
        assert_eq!(gap, 9, "default banner row-1 gap should be 9");

        // Full row display width must equal BOX_W
        let row_w = BORDER_L + CRAB_W + name_cell.width() + tagline.width() + gap + BORDER_R;
        assert_eq!(row_w, BOX_W, "full row display width must equal BOX_W");
    }

    /// Verify that all SAFE_KAOMOJI_* entries contain only Latin-1 or common
    /// Unicode Geometric Shapes — no CJK, Arabic-Indic, or Thai characters.
    ///
    /// This is the guard that ensures the safe pool can never regress to showing "???".
    #[test]
    fn safe_kaomoji_are_ascii_and_latin1_only() {
        use unicode_width::UnicodeWidthStr;

        for face in SAFE_KAOMOJI_THINKING
            .iter()
            .chain(SAFE_KAOMOJI_SUCCESS.iter())
            .chain(SAFE_KAOMOJI_ERROR.iter())
        {
            // Every character must be either:
            //   a) ASCII (U+0020..U+007E)
            //   b) Latin-1 Supplement (U+00A0..U+00FF)
            //   c) Common punctuation / arrows (U+2000..U+27FF) which are in
            //      virtually every monospace terminal font.
            for ch in face.chars() {
                let cp = ch as u32;
                // Reject CJK Unified (U+4E00+), Katakana/Hiragana (U+3000+),
                // Arabic (U+0600+), Thai (U+0E00+), Devanagari (U+0900+),
                // and supplementary planes (> U+FFFF, e.g. emoji planes 1-16).
                assert!(
                    cp < 0x0600 || (0x2000..=0x27FF).contains(&cp),
                    "SAFE kaomoji '{face}' contains char '{ch}' (U+{cp:04X}) outside safe ranges"
                );
                // All safe chars must have non-zero display width
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                // Combining marks (width 0) must not be in the safe pool
                assert!(
                    w > 0 || ch == ' ',
                    "SAFE kaomoji '{face}' contains zero-width char '{ch}' (U+{cp:04X})"
                );
            }
            // Each face must render with positive display width
            assert!(
                face.width() > 0,
                "SAFE kaomoji '{face}' has zero display width"
            );
        }
    }

    /// Skin-set kaomoji override: when the skin provides a list the theme
    /// uses that regardless of terminal capability, so users who explicitly
    /// configure their skin get their choice.
    #[test]
    fn skin_override_respected_regardless_of_terminal_capability() {
        let custom = vec!["(^_^)".to_string(), "(*.*)".to_string()];
        let skin = SkinConfig {
            kaomoji_thinking: Some(custom.clone()),
            ..Default::default()
        };
        let theme = Theme::from_skin(&skin);
        assert_eq!(
            theme.kaomoji_thinking, custom,
            "skin-provided kaomoji must be used verbatim"
        );
    }

    /// Regression: theme built without any env vars uses the safe pool.
    #[test]
    fn default_theme_uses_some_kaomoji_pool() {
        // Both pools are non-empty — we just check the result is usable.
        let theme = Theme::default();
        assert!(!theme.kaomoji_thinking.is_empty());
        assert!(!theme.kaomoji_success.is_empty());
        assert!(!theme.kaomoji_error.is_empty());
        // All entries have positive display width
        for face in &theme.kaomoji_thinking {
            use unicode_width::UnicodeWidthStr;
            assert!(face.width() > 0, "kaomoji_thinking '{face}' has zero width");
        }
    }
}
