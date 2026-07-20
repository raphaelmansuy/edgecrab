//! First-class follow / browse mode for the transcript viewport (026 Wave E).

#![allow(dead_code)]

/// Whether the viewport sticks to the live stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FollowMode {
    /// Glued to the bottom — new content scrolls into view.
    #[default]
    Following,
    /// User scrolled away — new content must not yank the viewport.
    Browsing,
}

impl FollowMode {
    pub fn is_following(self) -> bool {
        matches!(self, Self::Following)
    }

    pub fn is_browsing(self) -> bool {
        matches!(self, Self::Browsing)
    }

    /// Status / scrollbar cue when browsing.
    pub fn browse_hint(self, ascii: bool) -> Option<&'static str> {
        if self.is_browsing() {
            Some(if ascii { " follow:G " } else { " ↓ follow " })
        } else {
            None
        }
    }

    /// Scroll away from bottom → Browsing.
    pub fn on_scroll_away(&mut self) {
        *self = Self::Browsing;
    }

    /// At bottom / `G` / send message → Following.
    pub fn reengage(&mut self) {
        *self = Self::Following;
    }

    /// After scroll: if offset is 0 (bottom), re-engage; else browse.
    pub fn sync_from_offset(&mut self, scroll_offset: u16) {
        if scroll_offset == 0 {
            self.reengage();
        } else {
            self.on_scroll_away();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_and_reengage() {
        let mut m = FollowMode::Following;
        m.on_scroll_away();
        assert!(m.is_browsing());
        assert!(m.browse_hint(false).is_some());
        m.reengage();
        assert!(m.is_following());
        m.sync_from_offset(5);
        assert!(m.is_browsing());
        m.sync_from_offset(0);
        assert!(m.is_following());
    }
}
