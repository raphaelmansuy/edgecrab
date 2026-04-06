//! # banner — ASCII art banner for the CLI
//!
//! WHY banner: Visual identity when starting the agent. Matches
//! hermes-agent's branding style but with EdgeCrab identity.
//! Uses pure box-drawing + block characters — no emoji (avoids
//! terminal-width alignment issues in raw stdout mode).

use ratatui::style::{Color, Style};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Full ASCII art banner shown at startup.
/// Pure box-drawing characters — safe across all terminal emulators.
#[allow(dead_code)]
pub const BANNER: &str = concat!(
    "\
╔══════════════════════════════════════════════════════════════╗\n\
║                                                              ║\n\
║  ███████╗██████╗  ██████╗ ███████╗ ██████╗██████╗  ██╗      ║\n\
║  ██╔════╝██╔══██╗██╔════╝ ██╔════╝██╔════╝██╔══██╗ ██║      ║\n\
║  █████╗  ██║  ██║██║  ███╗█████╗  ██║     ██████╔╝ ██║      ║\n\
║  ██╔══╝  ██║  ██║██║   ██║██╔══╝  ██║     ██╔══██╗ ██║      ║\n\
║  ███████╗██████╔╝╚██████╔╝███████╗╚██████╗██║  ██║ ███████╗ ║\n\
║  ╚══════╝╚═════╝  ╚═════╝ ╚══════╝ ╚═════╝╚═╝  ╚═╝╚══════╝ ║\n\
║                                                              ║\n\
║  AI-native terminal agent                     v",
    env!("CARGO_PKG_VERSION"),
    "    ║\n\
║  /help  commands   /model  switch model                      ║\n\
║                                                              ║\n\
╚══════════════════════════════════════════════════════════════╝"
);

/// Short one-line banner for minimal/pipe mode.
#[allow(dead_code)]
pub const BANNER_SHORT: &str = concat!(
    "EdgeCrab v",
    env!("CARGO_PKG_VERSION"),
    " | AI-native terminal agent"
);

/// Style for the banner border.
#[allow(dead_code)]
pub fn banner_style() -> Style {
    Style::default().fg(Color::Rgb(205, 127, 50)) // bronze
}

/// Style for the banner title text.
#[allow(dead_code)]
pub fn title_style() -> Style {
    Style::default().fg(Color::Rgb(255, 215, 0)) // gold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_is_not_empty() {
        assert!(!BANNER.is_empty());
        assert!(
            BANNER.contains("EDGECR")
                || BANNER.contains("EdgeCrab")
                || BANNER.contains("terminal agent")
        );
    }

    #[test]
    fn short_banner_has_version() {
        assert!(BANNER_SHORT.contains(VERSION));
    }

    #[test]
    fn banner_has_no_emoji() {
        // Emoji break alignment in box-drawing and vary in width across terminals.
        // In the TUI (ratatui), emoji in Spans is fine because ratatui uses
        // unicode-width internally. But the banner is printed to raw stdout
        // before the TUI starts, so we keep it purely ASCII/box-drawing.
        for ch in BANNER.chars() {
            // Allow box-drawing (U+2500..U+257F) and block elements (U+2580..U+259F)
            // but reject anything in the emoji ranges.
            let cp = ch as u32;
            let is_emoji_range = (0x1F600..=0x1F64F).contains(&cp)  // emoticons
                || (0x1F300..=0x1F5FF).contains(&cp)  // misc symbols & pictographs
                || (0x1F680..=0x1F6FF).contains(&cp)  // transport & map
                || (0x1F900..=0x1F9FF).contains(&cp)  // supplemental symbols
                || (0x2600..=0x26FF).contains(&cp)    // misc symbols
                || (0x2700..=0x27BF).contains(&cp); // dingbats
            assert!(
                !is_emoji_range,
                "BANNER contains emoji codepoint U+{cp:04X}"
            );
        }
    }
}
