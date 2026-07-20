//! Typed scrollback presentation (026 Wave B).
//!
//! `RenderEntry` is the uniform block model. `OutputLine` remains a thin
//! transcript adapter for one release — expand toggles map to [`DisplayMode`].

// Public API surface for dispatch + future shelf paint; keep unused constructors.
#![allow(dead_code)]

pub mod entries;
pub mod render;

pub use entries::{EntryId, RenderEntry};
// Re-exports for harness / future shelf paint (keep even if binary path unused).
#[allow(unused_imports)]
pub use entries::{CardStatus, RenderEntryKind, ToolEntryArgs, VerbGroupEntry, next_entry_id};
#[allow(unused_imports)]
pub use render::{RenderOpts, render_entry_lines, render_entry_plain};
