//! # edgecrab-state
//!
//! Persistence layer: SQLite session database (FTS5), config manager,
//! memory store, skill store, cron scheduler.

#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod session_db;

pub use session_db::{
    DailyActivity, InsightsOverview, InsightsReport, ModelBreakdown, PlatformBreakdown,
    SearchResult, SessionDb, SessionExport, SessionRecord, SessionRichSummary, SessionStats,
    SessionSummary, ToolUsage,
};
