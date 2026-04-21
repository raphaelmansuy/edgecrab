//! # edgecrab-types
//!
//! Shared types for the EdgeCrab agent ecosystem.
//! This is the leaf crate — no internal dependencies.
//!
//! ```text
//!   edgecrab-types  ←  (all other crates depend on this)
//!     ├── message.rs    — Message, Role, Content, ContentPart
//!     ├── tool.rs       — ToolCall, FunctionCall, ToolSchema
//!     ├── usage.rs      — Usage, Cost, billing normalization
//!     ├── config.rs     — ApiMode, Platform, constants
//!     ├── trajectory.rs — Trajectory, reasoning extraction
//!     └── error.rs      — AgentError, ToolError
//! ```

#![deny(clippy::unwrap_used)]

pub mod config;
pub mod error;
pub mod harness;
pub mod message;
pub mod tool;
pub mod trajectory;
pub mod usage;

pub use config::{ApiMode, DEFAULT_MODEL, OPENROUTER_BASE_URL, OriginChat, Platform};
pub use error::{AgentError, ToolError, ToolErrorRecord, ToolErrorResponse};
pub use harness::{
    CompletionDecision, ExitReason, ReportedTaskStatus, RunOutcome, TaskStatusKind,
    VerificationSummary,
};
pub use message::{Content, ContentPart, ImageUrl, Message, Role};
pub use tool::{FunctionCall, ToolCall, ToolSchema};
pub use trajectory::Trajectory;
pub use usage::{Cost, Usage};

/// Crate-level Result alias
pub type Result<T> = std::result::Result<T, AgentError>;

// ─── Termux / Android detection ──────────────────────────────────────

/// Returns `true` if running inside Termux on Android.
///
/// Detection checks:
/// 1. `TERMUX_VERSION` env var is set
/// 2. `PREFIX` env var contains `com.termux/files/usr`
pub fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
        || std::env::var("PREFIX")
            .map(|p| p.contains("com.termux/files/usr"))
            .unwrap_or(false)
}

/// Cached result of [`is_termux()`]. Env vars don't change mid-process,
/// so we evaluate once at first access.
pub static IS_TERMUX: std::sync::LazyLock<bool> = std::sync::LazyLock::new(is_termux);
