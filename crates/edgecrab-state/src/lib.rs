//! # edgecrab-state
//!
//! Persistence layer: SQLite session database (FTS5), config manager,
//! memory store, skill store, cron scheduler.

#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod session_db;

pub mod kanban_db;
pub mod kanban_board;
pub mod kanban_rate_limit;
pub mod kanban_worker_context;

pub use kanban_board::{
    DEFAULT_BOARD, ensure_board, get_current_board, kanban_db_path_for_board, kanban_home,
    list_board_slugs, set_current_board,
};
pub use kanban_db::{
    KanbanComment, KanbanDb, KanbanDecomposeChild, KanbanEvent, KanbanNotifySub, KanbanRun,
    KanbanStatus, KanbanTask, DEFAULT_FAILURE_LIMIT, KANBAN_NOTIFY_TERMINAL_KINDS, kanban_db_path,
};
pub use kanban_worker_context::build_worker_context;
pub use session_db::{
    DailyActivity, HandoffRecord, InsightsOverview, InsightsReport, ModelBreakdown,
    ModelTransferRecord, PendingSessionHandoff, PlatformBreakdown, SearchResult, SessionDb,
    SessionExport, SessionHandoffStatus, SessionRecord, SessionRichSummary, SessionSearchHit,
    SessionStats, SessionSummary, StoredGoalState, StoredSubGoal, ToolUsage,
};
