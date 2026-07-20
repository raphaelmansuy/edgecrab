//! Per-section shelf disclosure — Hermes `domain/details.ts` parity for ratatui.
//!
//! `/details` controls thinking / tools / subagents / activity independently of
//! transcript `/verbose` policy.

use std::collections::HashMap;
use std::fmt;

use edgecrab_core::ShelfDetailsConfig;

/// Visibility mode for a shelf section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShelfDetailsMode {
    Hidden,
    #[default]
    Collapsed,
    Expanded,
}

impl ShelfDetailsMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "hidden" | "hide" | "off" => Some(Self::Hidden),
            "collapsed" | "collapse" | "compact" => Some(Self::Collapsed),
            "expanded" | "expand" | "full" | "on" => Some(Self::Expanded),
            _ => None,
        }
    }

    /// Map shelf disclosure → card [`crate::stream_presentation::DisplayMode`] (026 C4).
    /// Hidden sections still finish as Collapsed cards in the transcript.
    pub fn to_card_display_mode(self) -> crate::stream_presentation::DisplayMode {
        match self {
            Self::Hidden | Self::Collapsed => crate::stream_presentation::DisplayMode::Collapsed,
            Self::Expanded => crate::stream_presentation::DisplayMode::Expanded,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Collapsed => "collapsed",
            Self::Expanded => "expanded",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Hidden => Self::Collapsed,
            Self::Collapsed => Self::Expanded,
            Self::Expanded => Self::Hidden,
        }
    }
}

/// Shelf regions — maps to Hermes `SECTION_NAMES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShelfSection {
    Thinking,
    Tools,
    Subagents,
    Activity,
}

impl ShelfSection {
    pub const ALL: [Self; 4] = [Self::Thinking, Self::Tools, Self::Subagents, Self::Activity];

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "thinking" | "think" | "reasoning" => Some(Self::Thinking),
            "tools" | "tool" => Some(Self::Tools),
            "subagents" | "subagent" | "agents" | "delegate" => Some(Self::Subagents),
            "activity" | "feed" | "notices" => Some(Self::Activity),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Thinking => "thinking",
            Self::Tools => "tools",
            Self::Subagents => "subagents",
            Self::Activity => "activity",
        }
    }
}

impl fmt::Display for ShelfSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How a section should render in the shelf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionRender {
    Skip,
    Summary,
    Full,
}

impl From<ShelfDetailsMode> for SectionRender {
    fn from(mode: ShelfDetailsMode) -> Self {
        match mode {
            ShelfDetailsMode::Hidden => Self::Skip,
            ShelfDetailsMode::Collapsed => Self::Summary,
            ShelfDetailsMode::Expanded => Self::Full,
        }
    }
}

/// Session-scoped shelf disclosure state.
#[derive(Clone, Debug, Default)]
pub struct ShelfDetailsState {
    pub global: ShelfDetailsMode,
    /// When true, `/details <mode>` applies globally (Hermes `commandOverride`).
    pub command_override: bool,
    section_overrides: HashMap<ShelfSection, ShelfDetailsMode>,
}

impl ShelfDetailsState {
    /// Hermes `sectionMode()` — explicit override → default → global.
    pub fn effective_mode(&self, section: ShelfSection) -> ShelfDetailsMode {
        if let Some(mode) = self.section_overrides.get(&section) {
            return *mode;
        }
        if self.command_override {
            return self.global;
        }
        match section {
            ShelfSection::Thinking | ShelfSection::Tools => ShelfDetailsMode::Expanded,
            ShelfSection::Activity => ShelfDetailsMode::Hidden,
            ShelfSection::Subagents => self.global,
        }
    }

    pub fn section_render(&self, section: ShelfSection) -> SectionRender {
        self.effective_mode(section).into()
    }

    /// Live reasoning snippet auto-expands thinking without mutating `/details` config.
    pub fn effective_thinking_render(&self, has_reasoning_snippet: bool) -> SectionRender {
        if has_reasoning_snippet {
            return SectionRender::Full;
        }
        self.section_render(ShelfSection::Thinking)
    }

    /// True when every shelf section resolves to hidden (Hermes quiet-mode backstop).
    pub fn all_sections_hidden(&self) -> bool {
        ShelfSection::ALL
            .iter()
            .all(|&s| self.effective_mode(s) == ShelfDetailsMode::Hidden)
    }

    pub fn has_section_override(&self, section: ShelfSection) -> bool {
        self.section_overrides.contains_key(&section)
    }

    /// Hermes section default before global override.
    pub fn default_mode(&self, section: ShelfSection) -> ShelfDetailsMode {
        match section {
            ShelfSection::Thinking | ShelfSection::Tools => ShelfDetailsMode::Expanded,
            ShelfSection::Activity => ShelfDetailsMode::Hidden,
            ShelfSection::Subagents => self.global,
        }
    }

    pub fn set_global_mode(&mut self, mode: ShelfDetailsMode) {
        self.global = mode;
        self.command_override = true;
        self.section_overrides.clear();
    }

    pub fn cycle_global_mode(&mut self) {
        self.global = self.global.cycle();
        self.command_override = true;
        self.section_overrides.clear();
    }

    pub fn set_section_mode(&mut self, section: ShelfSection, mode: ShelfDetailsMode) {
        self.section_overrides.insert(section, mode);
    }

    pub fn cycle_section_mode(&mut self, section: ShelfSection) {
        let next = self.effective_mode(section).cycle();
        self.section_overrides.insert(section, next);
    }

    pub fn reset_section(&mut self, section: ShelfSection) {
        self.section_overrides.remove(&section);
    }

    /// Initial list cursor — highlights current global mode row.
    pub fn panel_cursor_for_global(&self) -> usize {
        match self.global {
            ShelfDetailsMode::Hidden => 0,
            ShelfDetailsMode::Collapsed => 1,
            ShelfDetailsMode::Expanded => 2,
        }
    }

    pub fn mode_blurb(mode: ShelfDetailsMode) -> &'static str {
        match mode {
            ShelfDetailsMode::Hidden => {
                "Section omitted from the live shelf (errors may still backstop)."
            }
            ShelfDetailsMode::Collapsed => "One summary line in the shelf — calm, at-a-glance.",
            ShelfDetailsMode::Expanded => {
                "Full tree rows — tool tails, delegate churn, activity feed."
            }
        }
    }

    pub fn section_blurb(section: ShelfSection) -> &'static str {
        match section {
            ShelfSection::Thinking => "Phase line: thinking, awaiting, preparing tools.",
            ShelfSection::Tools => "In-flight tools, stdout tail preview, background processes.",
            ShelfSection::Subagents => "Delegate tree, tool sparkline, recent tool tail.",
            ShelfSection::Activity => "Compression, approval, long-run charms, notices.",
        }
    }

    pub fn handle_command(&mut self, raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("status") {
            return self.format_status();
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2
            && let Some(section) = ShelfSection::parse(parts[0])
        {
            let action = parts[1];
            if matches!(action, "reset" | "clear" | "default") {
                self.section_overrides.remove(&section);
                return format!("Shelf section `{section}` reset to default.");
            }
            if let Some(mode) = ShelfDetailsMode::parse(action) {
                self.section_overrides.insert(section, mode);
                return format!("Shelf section `{section}` → {mode}.", mode = mode.label());
            }
            return SECTION_USAGE.into();
        }

        let word = parts[0];
        if matches!(word, "cycle" | "toggle" | "next") {
            self.global = self.global.cycle();
            self.command_override = true;
            self.section_overrides.clear();
            return format!("Shelf details → {} (all sections).", self.global.label());
        }
        if let Some(mode) = ShelfDetailsMode::parse(word) {
            self.global = mode;
            self.command_override = true;
            self.section_overrides.clear();
            return format!("Shelf details → {} (all sections).", self.global.label());
        }

        GLOBAL_USAGE.into()
    }

    fn format_status(&self) -> String {
        let mut lines = vec![format!("Shelf details: global={}", self.global.label())];
        for section in ShelfSection::ALL {
            let eff = self.effective_mode(section);
            let tag = if self.section_overrides.contains_key(&section) {
                "override"
            } else {
                "effective"
            };
            lines.push(format!("  {section}: {eff} ({tag})", eff = eff.label()));
        }
        lines.join("\n")
    }

    /// Load persisted disclosure from `config.yaml` (`display.shelf_details`).
    pub fn from_config(cfg: &ShelfDetailsConfig) -> Self {
        let mut state = Self::default();
        if let Some(mode) = ShelfDetailsMode::parse(&cfg.mode) {
            state.global = mode;
        }
        state.command_override = cfg.command_override;
        for (key, val) in &cfg.sections {
            if let (Some(section), Some(mode)) =
                (ShelfSection::parse(key), ShelfDetailsMode::parse(val))
            {
                state.section_overrides.insert(section, mode);
            }
        }
        state
    }

    /// Serialize for persistence (Hermes `config.set details_mode*` parity).
    pub fn to_config(&self) -> ShelfDetailsConfig {
        let mut sections = HashMap::new();
        for section in ShelfSection::ALL {
            if let Some(mode) = self.section_overrides.get(&section) {
                sections.insert(section.name().into(), mode.label().into());
            }
        }
        ShelfDetailsConfig {
            mode: self.global.label().into(),
            command_override: self.command_override,
            sections,
        }
    }
}

const GLOBAL_USAGE: &str = "Usage: /details [hidden|collapsed|expanded|cycle|status]  or  /details <thinking|tools|subagents|activity> [hidden|collapsed|expanded|reset]";

const SECTION_USAGE: &str = "Usage: /details <section> [hidden|collapsed|expanded|reset]  (sections: thinking, tools, subagents, activity)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermes_defaults_thinking_tools_expanded_activity_hidden() {
        let state = ShelfDetailsState::default();
        assert_eq!(
            state.effective_mode(ShelfSection::Thinking),
            ShelfDetailsMode::Expanded
        );
        assert_eq!(
            state.effective_mode(ShelfSection::Tools),
            ShelfDetailsMode::Expanded
        );
        assert_eq!(
            state.effective_mode(ShelfSection::Activity),
            ShelfDetailsMode::Hidden
        );
    }

    #[test]
    fn global_command_override_applies_to_all() {
        let mut state = ShelfDetailsState::default();
        state.handle_command("hidden");
        assert_eq!(
            state.effective_mode(ShelfSection::Thinking),
            ShelfDetailsMode::Hidden
        );
        assert_eq!(
            state.effective_mode(ShelfSection::Tools),
            ShelfDetailsMode::Hidden
        );
    }

    #[test]
    fn section_override_beats_global() {
        let mut state = ShelfDetailsState::default();
        state.handle_command("hidden");
        state.handle_command("activity expanded");
        assert_eq!(
            state.effective_mode(ShelfSection::Activity),
            ShelfDetailsMode::Expanded
        );
    }

    #[test]
    fn section_reset_restores_default() {
        let mut state = ShelfDetailsState::default();
        state.handle_command("activity expanded");
        state.handle_command("activity reset");
        assert_eq!(
            state.effective_mode(ShelfSection::Activity),
            ShelfDetailsMode::Hidden
        );
    }

    #[test]
    fn config_roundtrip_preserves_overrides() {
        let mut state = ShelfDetailsState::default();
        state.handle_command("hidden");
        state.handle_command("activity expanded");
        let cfg = state.to_config();
        let loaded = ShelfDetailsState::from_config(&cfg);
        assert_eq!(loaded.global, ShelfDetailsMode::Hidden);
        assert!(loaded.command_override);
        assert_eq!(
            loaded.effective_mode(ShelfSection::Activity),
            ShelfDetailsMode::Expanded
        );
    }
}
