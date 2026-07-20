//! Optional syntax highlighting for edit hunks (026 Wave F / 024 W5).
//!
//! Default is a noop. Enable `--features edit-syntax` for syntect-backed HL.
//! Caps: 2 MiB / 50k lines; unhighlighted first paint is always safe.

#![allow(dead_code)]

/// Trait so presentation can swap highlighters without coupling to syntect.
pub trait EditHighlighter: Send + Sync {
    /// Highlight a source snippet. Returns plain text lines when highlighting
    /// is skipped (too large, unknown language, or noop).
    fn highlight(&self, source: &str, language_hint: Option<&str>) -> Vec<String>;
}

/// Always returns the source split into lines — first-paint safe.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopEditHighlighter;

impl EditHighlighter for NoopEditHighlighter {
    fn highlight(&self, source: &str, _language_hint: Option<&str>) -> Vec<String> {
        if exceeds_caps(source) {
            return vec!["…(diff too large to highlight)".into()];
        }
        source.lines().map(|l| l.to_string()).collect()
    }
}

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINES: usize = 50_000;

pub fn exceeds_caps(source: &str) -> bool {
    source.len() > MAX_BYTES || source.lines().count() > MAX_LINES
}

/// Build the active highlighter for this build.
pub fn default_highlighter() -> Box<dyn EditHighlighter> {
    #[cfg(feature = "edit-syntax")]
    {
        Box::new(SyntectEditHighlighter::default())
    }
    #[cfg(not(feature = "edit-syntax"))]
    {
        Box::new(NoopEditHighlighter)
    }
}

#[cfg(feature = "edit-syntax")]
#[derive(Debug, Default)]
pub struct SyntectEditHighlighter;

#[cfg(feature = "edit-syntax")]
impl EditHighlighter for SyntectEditHighlighter {
    fn highlight(&self, source: &str, language_hint: Option<&str>) -> Vec<String> {
        // Syntect is optional; until wired, fall back to noop with language tag.
        let _ = language_hint;
        NoopEditHighlighter.highlight(source, language_hint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_splits_lines() {
        let h = NoopEditHighlighter;
        let lines = h.highlight("a\nb\n", Some("rs"));
        assert_eq!(lines, vec!["a", "b"]);
    }

    #[test]
    fn caps_trip() {
        assert!(exceeds_caps(&"x".repeat(MAX_BYTES + 1)));
    }
}
