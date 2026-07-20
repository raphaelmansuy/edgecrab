//! Typed scrollback entries (026 Wave B).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::stream_presentation::{DisplayMode, FinishedThinkingCard, ToolCardKind, VerbGroupKind};

static NEXT_ENTRY_ID: AtomicU64 = AtomicU64::new(1);

/// Opaque id linking a transcript [`crate::transcript::OutputLine`] to a render entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId(pub u64);

pub fn next_entry_id() -> EntryId {
    EntryId(NEXT_ENTRY_ID.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStatus {
    Generating,
    Running,
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderEntryKind {
    Thinking {
        duration: Option<Duration>,
        running: bool,
    },
    Tool {
        name: String,
        kind: ToolCardKind,
        status: CardStatus,
        caption: String,
        duration: Option<Duration>,
        muted: bool,
    },
    AgentMessage,
    VerbGroup {
        kind: VerbGroupKind,
        count: usize,
        running: bool,
        items: Vec<String>,
    },
    User,
    Footer,
}

/// One scrollback card with uniform disclosure.
#[derive(Debug, Clone)]
pub struct RenderEntry {
    pub id: EntryId,
    pub kind: RenderEntryKind,
    pub mode: DisplayMode,
    pub header: String,
    pub body: String,
    /// Soft-error / failed tool — never muted.
    pub is_error: bool,
}

impl RenderEntry {
    pub fn thinking_running(text: &str, mode: DisplayMode) -> Self {
        Self {
            id: next_entry_id(),
            kind: RenderEntryKind::Thinking {
                duration: None,
                running: true,
            },
            mode,
            header: "Thinking…".into(),
            body: text.to_string(),
            is_error: false,
        }
    }

    pub fn from_finished_thinking(card: &FinishedThinkingCard) -> Self {
        Self {
            id: next_entry_id(),
            kind: RenderEntryKind::Thinking {
                duration: Some(card.duration),
                running: false,
            },
            mode: card.mode,
            header: card.collapsed_label(),
            body: card.text.clone(),
            is_error: false,
        }
    }

    pub fn tool(args: ToolEntryArgs) -> Self {
        let caption = args.caption;
        let muted = !args.is_error
            && matches!(args.status, CardStatus::Success)
            && matches!(args.mode, DisplayMode::Collapsed);
        Self {
            id: next_entry_id(),
            kind: RenderEntryKind::Tool {
                name: args.name,
                kind: args.kind,
                status: args.status,
                caption: caption.clone(),
                duration: args.duration,
                muted,
            },
            mode: args.mode,
            header: caption,
            body: args.body,
            is_error: args.is_error,
        }
    }

    pub fn agent(text: impl Into<String>, mode: DisplayMode) -> Self {
        Self {
            id: next_entry_id(),
            kind: RenderEntryKind::AgentMessage,
            mode,
            header: String::new(),
            body: text.into(),
            is_error: false,
        }
    }

    pub fn verb_group(entry: VerbGroupEntry) -> Self {
        let header = if entry.running {
            entry.kind.running_label(entry.count)
        } else {
            entry.kind.done_label(entry.count)
        };
        let body = {
            let mut lines = vec![header.clone()];
            for (i, item) in entry.items.iter().enumerate() {
                lines.push(format!("  {}. {item}", i + 1));
            }
            lines.join("\n")
        };
        Self {
            id: next_entry_id(),
            kind: RenderEntryKind::VerbGroup {
                kind: entry.kind,
                count: entry.count,
                running: entry.running,
                items: entry.items,
            },
            mode: DisplayMode::Collapsed,
            header,
            body,
            is_error: false,
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self {
            id: next_entry_id(),
            kind: RenderEntryKind::User,
            mode: DisplayMode::Expanded,
            header: String::new(),
            body: text.into(),
            is_error: false,
        }
    }

    pub fn footer(text: impl Into<String>) -> Self {
        Self {
            id: next_entry_id(),
            kind: RenderEntryKind::Footer,
            mode: DisplayMode::Collapsed,
            header: text.into(),
            body: String::new(),
            is_error: false,
        }
    }

    pub fn set_mode(&mut self, mode: DisplayMode) {
        self.mode = mode;
        if let RenderEntryKind::Tool {
            status, muted, ..
        } = &mut self.kind
        {
            *muted = !self.is_error
                && matches!(*status, CardStatus::Success)
                && matches!(mode, DisplayMode::Collapsed);
        }
    }

    pub fn cycle_mode(&mut self) {
        self.set_mode(match self.mode {
            DisplayMode::Collapsed => DisplayMode::Truncated,
            DisplayMode::Truncated => DisplayMode::Expanded,
            DisplayMode::Expanded => DisplayMode::Collapsed,
        });
    }

    pub fn expand(&mut self) {
        self.set_mode(DisplayMode::Expanded);
    }

    pub fn collapse(&mut self) {
        self.set_mode(DisplayMode::Collapsed);
    }

    pub fn is_thinking(&self) -> bool {
        matches!(self.kind, RenderEntryKind::Thinking { .. })
    }

    pub fn is_muted(&self) -> bool {
        match &self.kind {
            RenderEntryKind::Tool { muted, .. } => *muted,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerbGroupEntry {
    pub kind: VerbGroupKind,
    pub count: usize,
    pub running: bool,
    pub items: Vec<String>,
}

/// Arguments for [`RenderEntry::tool`] (avoids clippy too_many_arguments).
#[derive(Debug, Clone)]
pub struct ToolEntryArgs {
    pub name: String,
    pub kind: ToolCardKind,
    pub status: CardStatus,
    pub caption: String,
    pub body: String,
    pub mode: DisplayMode,
    pub duration: Option<Duration>,
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn thinking_modes_cycle() {
        let card = FinishedThinkingCard {
            text: "a\nb\nc\nd\ne\nf\ng".into(),
            duration: Duration::from_millis(1200),
            mode: DisplayMode::Collapsed,
        };
        let mut e = RenderEntry::from_finished_thinking(&card);
        assert_eq!(e.mode, DisplayMode::Collapsed);
        e.cycle_mode();
        assert_eq!(e.mode, DisplayMode::Truncated);
        e.expand();
        assert_eq!(e.mode, DisplayMode::Expanded);
        assert!(e.is_thinking());
    }

    #[test]
    fn success_tool_muted_when_collapsed() {
        let e = RenderEntry::tool(ToolEntryArgs {
            name: "read_file".into(),
            kind: ToolCardKind::Read,
            status: CardStatus::Success,
            caption: "⊙ Read foo.rs".into(),
            body: "contents".into(),
            mode: DisplayMode::Collapsed,
            duration: Some(Duration::from_millis(40)),
            is_error: false,
        });
        assert!(e.is_muted());
    }
}
