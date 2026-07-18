//! SQLite session database with WAL mode, FTS5 search, and jitter-retry
//! write contention handling.
//!
//! # Architecture
//!
//! ```text
//!  ┌─────────────┐     ┌────────────────────────────┐
//!  │  CLI / GW    │────▶│  SessionDb                 │
//!  │  (readers)   │     │  Arc<Mutex<Connection>>     │
//!  └─────────────┘     │                            │
//!                       │  WAL mode ─── readers      │
//!  ┌─────────────┐     │  don't block writers        │
//!  │  Agent loop  │────▶│                            │
//!  │  (writer)    │     │  BEGIN IMMEDIATE + jitter   │
//!  └─────────────┘     │  retry breaks convoy        │
//!                       └────────────────────────────┘
//! ```
//!
//! Multiple EdgeCrab processes (gateway + CLI + worktree agents) share
//! one `state.db`. SQLite's built-in busy handler uses deterministic
//! sleep causing convoy effects. We keep timeout short and retry with
//! random jitter (20-150ms) to naturally stagger competing writers.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::Rng;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use edgecrab_types::{AgentError, Message, Role};

/// Schema version — incremented on breaking schema changes.
const SCHEMA_VERSION: u32 = 13;

// Write-contention constants
const WRITE_MAX_RETRIES: u32 = 15;
const WRITE_RETRY_MIN_MS: u64 = 20;
const WRITE_RETRY_MAX_MS: u64 = 150;
const CHECKPOINT_EVERY_N_WRITES: u32 = 50;

// ── Public types ──────────────────────────────────────────────────────

/// Full session record for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub source: String,
    pub user_id: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    /// Stable cache-law prefix (1h tier). Restored with [`Self::system_prompt`] for 3-tier wire.
    #[serde(default)]
    pub stable_system_prompt: Option<String>,
    /// Semi-stable skills index (5m tier).
    #[serde(default)]
    pub semi_stable_system_prompt: Option<String>,
    pub parent_session_id: Option<String>,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub end_reason: Option<String>,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    /// Last full-request prompt-side token pressure (Hermes parity for restore UI).
    #[serde(default)]
    pub last_prompt_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub title: Option<String>,
}

/// Lightweight session summary for list views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub source: String,
    pub model: Option<String>,
    pub started_at: f64,
    pub message_count: i64,
    pub title: Option<String>,
}

/// FTS5 search result with BM25 score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub session_id: String,
    pub role: String,
    pub snippet: String,
    pub score: f64,
}

// ── Historical Insights types ──────────────────────────────────────────

/// Complete insights report matching hermes-agent's InsightsEngine output.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InsightsReport {
    /// Number of days covered by this report.
    pub days: u32,
    pub overview: InsightsOverview,
    pub models: Vec<ModelBreakdown>,
    pub platforms: Vec<PlatformBreakdown>,
    pub top_tools: Vec<ToolUsage>,
    pub daily_activity: Vec<DailyActivity>,
}

/// High-level aggregate stats.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InsightsOverview {
    pub total_sessions: u64,
    pub total_messages: u64,
    pub total_tool_calls: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_write_tokens: u64,
    pub total_reasoning_tokens: u64,
    pub estimated_total_cost_usd: f64,
}

/// Per-model usage breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelBreakdown {
    pub model: String,
    pub sessions: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost_usd: f64,
}

/// Per-platform/source session counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformBreakdown {
    pub source: String,
    pub sessions: u64,
    pub tool_calls: u64,
}

/// Tool usage frequency entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUsage {
    pub name: String,
    pub count: u64,
}

/// Daily session count for activity sparklines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyActivity {
    pub day: String,
    pub sessions: u64,
}

/// Rich session summary with first-message preview (for list display).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRichSummary {
    pub id: String,
    pub source: String,
    pub model: Option<String>,
    pub started_at: f64,
    pub message_count: i64,
    pub title: Option<String>,
    pub preview: String,
    pub last_active: f64,
}

/// Full-text session search result with rich session metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSearchHit {
    pub session: SessionRichSummary,
    pub role: String,
    pub snippet: String,
    pub score: f64,
}

/// Full session export (session record + messages) for JSONL backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExport {
    pub session: SessionRecord,
    pub messages: Vec<Message>,
}

/// Aggregate session statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub total_sessions: i64,
    pub total_messages: i64,
    pub by_source: Vec<(String, i64)>,
    pub db_size_bytes: i64,
}

/// One subgoal row for persistent goal storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSubGoal {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

/// Active goal snapshot loaded from SQLite.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredGoalState {
    pub goal_text: Option<String>,
    pub subgoals: Vec<StoredSubGoal>,
    pub status: String,
    pub turns_used: u32,
    pub max_turns: u32,
    pub paused_reason: Option<String>,
    pub last_verdict: Option<String>,
    pub last_reason: Option<String>,
    pub consecutive_parse_failures: u32,
    /// JSON-serialized [`edgecrab_types::GoalContract`] (optional evidence contract).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_json: Option<String>,
}

/// One recorded model transfer for a session (`/transfer-model`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTransferRecord {
    pub session_id: String,
    pub from_model: String,
    pub to_model: String,
    pub brief: String,
    pub ts: f64,
}

/// @deprecated alias — use [`ModelTransferRecord`].
pub type HandoffRecord = ModelTransferRecord;

/// Cross-platform session handoff state (`/handoff <platform>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHandoffStatus {
    /// `pending` | `running` | `completed` | `failed`
    pub state: String,
    pub platform: Option<String>,
    pub error: Option<String>,
}

/// Minimal session row for the gateway handoff watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSessionHandoff {
    pub session_id: String,
    pub platform: String,
    pub title: Option<String>,
}

// ── SessionDb ─────────────────────────────────────────────────────────

pub struct SessionDb {
    conn: Arc<Mutex<Connection>>,
    write_count: Mutex<u32>,
}

impl SessionDb {
    /// Open (or create) the database at `path`, configure WAL, create
    /// schema and FTS5 virtual table with sync triggers.
    pub fn open(path: &Path) -> Result<Self, AgentError> {
        let conn = Connection::open(path).map_err(|e| AgentError::Database(e.to_string()))?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| AgentError::Database(e.to_string()))?;

        Self::init_schema(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            write_count: Mutex::new(0),
        })
    }

    /// Open an in-memory database (for testing).
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, AgentError> {
        let conn = Connection::open_in_memory().map_err(|e| AgentError::Database(e.to_string()))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| AgentError::Database(e.to_string()))?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            write_count: Mutex::new(0),
        })
    }

    // ── Schema ────────────────────────────────────────────────────────

    fn init_schema(conn: &Connection) -> Result<(), AgentError> {
        conn.execute_batch(include_str!("schema.sql"))
            .map_err(|e| AgentError::Database(format!("schema init: {e}")))?;

        // Check / insert schema version
        let version: Option<u32> = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .ok();

        match version {
            None => {
                // Idempotent: older schema.sql copies may lack newer columns.
                Self::migrate_to_v13(conn)?;
                conn.execute(
                    "INSERT INTO schema_version (version) VALUES (?1)",
                    params![SCHEMA_VERSION],
                )
                .map_err(|e| AgentError::Database(e.to_string()))?;
            }
            Some(v) if v < SCHEMA_VERSION => {
                if v < 7 {
                    Self::migrate_to_v7(conn)?;
                }
                if v < 8 {
                    Self::migrate_to_v8(conn)?;
                }
                if v < 9 {
                    Self::migrate_to_v9(conn)?;
                }
                if v < 10 {
                    Self::migrate_to_v10(conn)?;
                }
                if v < 11 {
                    Self::migrate_to_v11(conn)?;
                }
                if v < 12 {
                    Self::migrate_to_v12(conn)?;
                }
                if v < 13 {
                    Self::migrate_to_v13(conn)?;
                }
                conn.execute(
                    "UPDATE schema_version SET version = ?1",
                    params![SCHEMA_VERSION],
                )
                .map_err(|e| AgentError::Database(e.to_string()))?;
            }
            _ => {}
        }
        Ok(())
    }

    fn migrate_to_v7(conn: &Connection) -> Result<(), AgentError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS session_goals (
                session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                goal_text TEXT NOT NULL,
                created_at REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS session_subgoals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES session_goals(session_id) ON DELETE CASCADE,
                text TEXT NOT NULL,
                done INTEGER NOT NULL DEFAULT 0,
                position INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_session_subgoals_session
                ON session_subgoals(session_id, position);",
        )
        .map_err(|e| AgentError::Database(format!("migrate v7: {e}")))?;
        Ok(())
    }

    fn migrate_to_v8(conn: &Connection) -> Result<(), AgentError> {
        conn.execute_batch(
            "ALTER TABLE session_goals ADD COLUMN status TEXT NOT NULL DEFAULT 'active';
             ALTER TABLE session_goals ADD COLUMN turns_used INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE session_goals ADD COLUMN max_turns INTEGER NOT NULL DEFAULT 20;
             ALTER TABLE session_goals ADD COLUMN paused_reason TEXT;
             ALTER TABLE session_goals ADD COLUMN last_verdict TEXT;
             ALTER TABLE session_goals ADD COLUMN last_reason TEXT;
             ALTER TABLE session_goals ADD COLUMN consecutive_parse_failures INTEGER NOT NULL DEFAULT 0;",
        )
        .map_err(|e| AgentError::Database(format!("migrate v8: {e}")))?;
        Ok(())
    }

    fn migrate_to_v9(conn: &Connection) -> Result<(), AgentError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS model_transfers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                from_model TEXT NOT NULL,
                to_model TEXT NOT NULL,
                brief TEXT NOT NULL,
                ts REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_model_transfers_session ON model_transfers(session_id, ts);",
        )
        .map_err(|e| AgentError::Database(format!("migrate v9: {e}")))?;
        Ok(())
    }

    fn migrate_to_v10(conn: &Connection) -> Result<(), AgentError> {
        Self::reconcile_model_transfers_table(conn)?;
        Self::ensure_sessions_column(conn, "handoff_state", "TEXT")?;
        Self::ensure_sessions_column(conn, "handoff_platform", "TEXT")?;
        Self::ensure_sessions_column(conn, "handoff_error", "TEXT")?;
        Ok(())
    }

    fn migrate_to_v11(conn: &Connection) -> Result<(), AgentError> {
        Self::ensure_sessions_column(conn, "last_prompt_tokens", "INTEGER NOT NULL DEFAULT 0")?;
        Ok(())
    }

    fn migrate_to_v12(conn: &Connection) -> Result<(), AgentError> {
        Self::ensure_sessions_column(conn, "stable_system_prompt", "TEXT")?;
        Self::ensure_sessions_column(conn, "semi_stable_system_prompt", "TEXT")?;
        Ok(())
    }

    fn migrate_to_v13(conn: &Connection) -> Result<(), AgentError> {
        if Self::table_exists(conn, "session_goals")
            && !Self::column_exists(conn, "session_goals", "contract_json")
        {
            conn.execute_batch("ALTER TABLE session_goals ADD COLUMN contract_json TEXT;")
                .map_err(|e| AgentError::Database(format!("migrate v13: {e}")))?;
        }
        Ok(())
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |row| row.get::<_, i64>(0).map(|count| count > 0),
        )
        .unwrap_or(false)
    }

    fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name=?2",
            params![table, column],
            |row| row.get::<_, i64>(0).map(|count| count > 0),
        )
        .unwrap_or(false)
    }

    fn ensure_sessions_column(
        conn: &Connection,
        column: &str,
        sql_type: &str,
    ) -> Result<(), AgentError> {
        if Self::column_exists(conn, "sessions", column) {
            return Ok(());
        }
        let sql = format!("ALTER TABLE sessions ADD COLUMN {column} {sql_type}");
        conn.execute(&sql, [])
            .map_err(|e| AgentError::Database(format!("migrate v10 add {column}: {e}")))?;
        Ok(())
    }

    /// Idempotent v10 table reconciliation.
    ///
    /// `schema.sql` may create an empty `model_transfers` before migrations run on
    /// legacy DBs that still have `handoffs` — merge then drop instead of RENAME.
    fn reconcile_model_transfers_table(conn: &Connection) -> Result<(), AgentError> {
        let handoffs = Self::table_exists(conn, "handoffs");
        let model_transfers = Self::table_exists(conn, "model_transfers");

        match (handoffs, model_transfers) {
            (true, false) => {
                conn.execute_batch("ALTER TABLE handoffs RENAME TO model_transfers;")
                    .map_err(|e| AgentError::Database(format!("migrate v10 rename: {e}")))?;
            }
            (true, true) => {
                conn.execute_batch(
                    "INSERT INTO model_transfers (session_id, from_model, to_model, brief, ts)
                     SELECT session_id, from_model, to_model, brief, ts FROM handoffs;
                     DROP TABLE handoffs;",
                )
                .map_err(|e| AgentError::Database(format!("migrate v10 merge handoffs: {e}")))?;
            }
            (false, false) => {
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS model_transfers (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                        from_model TEXT NOT NULL,
                        to_model TEXT NOT NULL,
                        brief TEXT NOT NULL,
                        ts REAL NOT NULL
                    );
                    CREATE INDEX IF NOT EXISTS idx_model_transfers_session
                        ON model_transfers(session_id, ts);",
                )
                .map_err(|e| {
                    AgentError::Database(format!("migrate v10 create model_transfers: {e}"))
                })?;
            }
            (false, true) => {}
        }
        Ok(())
    }

    pub fn save_session(&self, session: &SessionRecord) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            Self::upsert_session_header(conn, session, session.message_count)?;
            Ok(())
        })
    }

    /// Upsert a session header without deleting the row.
    ///
    /// WHY not INSERT OR REPLACE: SQLite implements REPLACE as DELETE + INSERT,
    /// which fires ON DELETE CASCADE on `session_goals` / `session_subgoals`.
    fn upsert_session_header(
        conn: &Connection,
        session: &SessionRecord,
        message_count: i64,
    ) -> Result<(), rusqlite::Error> {
        conn.execute(
            "INSERT INTO sessions
             (id, source, user_id, model, system_prompt, stable_system_prompt,
              semi_stable_system_prompt, parent_session_id,
              started_at, ended_at, end_reason, message_count, tool_call_count,
              input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
              reasoning_tokens, last_prompt_tokens, estimated_cost_usd, title)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)
             ON CONFLICT(id) DO UPDATE SET
              source = excluded.source,
              user_id = excluded.user_id,
              model = excluded.model,
              system_prompt = excluded.system_prompt,
              stable_system_prompt = excluded.stable_system_prompt,
              semi_stable_system_prompt = excluded.semi_stable_system_prompt,
              parent_session_id = excluded.parent_session_id,
              started_at = sessions.started_at,
              ended_at = excluded.ended_at,
              end_reason = excluded.end_reason,
              message_count = excluded.message_count,
              tool_call_count = excluded.tool_call_count,
              input_tokens = excluded.input_tokens,
              output_tokens = excluded.output_tokens,
              cache_read_tokens = excluded.cache_read_tokens,
              cache_write_tokens = excluded.cache_write_tokens,
              reasoning_tokens = excluded.reasoning_tokens,
              last_prompt_tokens = excluded.last_prompt_tokens,
              estimated_cost_usd = excluded.estimated_cost_usd,
              title = excluded.title",
            params![
                session.id,
                session.source,
                session.user_id,
                session.model,
                session.system_prompt,
                session.stable_system_prompt,
                session.semi_stable_system_prompt,
                session.parent_session_id,
                session.started_at,
                session.ended_at,
                session.end_reason,
                message_count,
                session.tool_call_count,
                session.input_tokens,
                session.output_tokens,
                session.cache_read_tokens,
                session.cache_write_tokens,
                session.reasoning_tokens,
                session.last_prompt_tokens,
                session.estimated_cost_usd,
                session.title,
            ],
        )?;
        Ok(())
    }

    /// Atomically replace the session header and full message list in one
    /// transaction so callers never observe a half-persisted turn.
    pub fn save_session_with_messages(
        &self,
        session: &SessionRecord,
        messages: &[Message],
        timestamp: f64,
    ) -> Result<(), AgentError> {
        let message_count = messages.len() as i64;
        self.execute_write(|conn| {
            Self::upsert_session_header(conn, session, message_count)?;
            conn.execute(
                "DELETE FROM messages WHERE session_id = ?1",
                params![session.id],
            )?;
            for (i, msg) in messages.iter().enumerate() {
                let tool_calls_json = msg
                    .tool_calls
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                // Prefer per-message created_at (018 P6 forensics); else monotonic
                // offset so ORDER BY timestamp matches conversation order.
                let msg_ts = msg
                    .created_at
                    .unwrap_or(timestamp + (i as f64) * 0.001);
                conn.execute(
                    "INSERT INTO messages
                     (session_id, role, content, tool_call_id, tool_calls, tool_name, timestamp,
                      finish_reason, reasoning)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        session.id,
                        msg.role.as_str(),
                        msg.text_content(),
                        msg.tool_call_id.as_deref(),
                        tool_calls_json,
                        msg.name.as_deref(),
                        msg_ts,
                        msg.finish_reason.as_deref(),
                        msg.reasoning.as_deref(),
                    ],
                )?;
            }
            Ok(())
        })
    }

    pub fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, source, user_id, model, system_prompt,
                        stable_system_prompt, semi_stable_system_prompt, parent_session_id,
                        started_at, ended_at, end_reason, message_count, tool_call_count,
                        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                        reasoning_tokens, COALESCE(last_prompt_tokens, 0), estimated_cost_usd, title
                 FROM sessions WHERE id = ?1",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let result = stmt
            .query_row(params![id], Self::session_record_from_row)
            .ok();

        Ok(result)
    }

    fn session_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
        Ok(SessionRecord {
            id: row.get(0)?,
            source: row.get(1)?,
            user_id: row.get(2)?,
            model: row.get(3)?,
            system_prompt: row.get(4)?,
            stable_system_prompt: row.get(5)?,
            semi_stable_system_prompt: row.get(6)?,
            parent_session_id: row.get(7)?,
            started_at: row.get(8)?,
            ended_at: row.get(9)?,
            end_reason: row.get(10)?,
            message_count: row.get(11)?,
            tool_call_count: row.get(12)?,
            input_tokens: row.get(13)?,
            output_tokens: row.get(14)?,
            cache_read_tokens: row.get(15)?,
            cache_write_tokens: row.get(16)?,
            reasoning_tokens: row.get(17)?,
            last_prompt_tokens: row.get(18)?,
            estimated_cost_usd: row.get(19)?,
            title: row.get(20)?,
        })
    }

    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, source, model, started_at, message_count, title
                 FROM sessions ORDER BY started_at DESC LIMIT ?1",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    model: row.get(2)?,
                    started_at: row.get(3)?,
                    message_count: row.get(4)?,
                    title: row.get(5)?,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| AgentError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn delete_session(&self, id: &str) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            conn.execute(
                "DELETE FROM session_subgoals WHERE session_id = ?1",
                params![id],
            )?;
            conn.execute(
                "DELETE FROM session_goals WHERE session_id = ?1",
                params![id],
            )?;
            conn.execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
            conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    /// Update a session's display title (user-facing label for `/title`).
    pub fn update_session_title(&self, id: &str, title: &str) -> Result<(), AgentError> {
        let cleaned = Self::sanitize_title(title)?;
        self.execute_write(|conn| {
            // Enforce uniqueness: only non-NULL titles must be unique
            if let Some(ref t) = cleaned {
                let conflict: Option<String> = conn
                    .query_row(
                        "SELECT id FROM sessions WHERE title = ?1 AND id != ?2",
                        params![t, id],
                        |row| row.get(0),
                    )
                    .ok();
                if conflict.is_some() {
                    return Err(rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                        Some(format!("Title '{}' is already in use", t)),
                    ));
                }
            }
            conn.execute(
                "UPDATE sessions SET title = ?1 WHERE id = ?2",
                params![cleaned, id],
            )?;
            Ok(())
        })
    }

    /// Update the model recorded for a session (after `/transfer-model` or `/model`).
    pub fn update_session_model(&self, id: &str, model: &str) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            conn.execute(
                "UPDATE sessions SET model = ?1 WHERE id = ?2",
                params![model, id],
            )?;
            Ok(())
        })
    }

    /// Persist a model transfer event for `/insights`.
    pub fn record_model_transfer(
        &self,
        session_id: &str,
        from_model: &str,
        to_model: &str,
        brief: &str,
        ts: f64,
    ) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            conn.execute(
                "INSERT INTO model_transfers (session_id, from_model, to_model, brief, ts)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![session_id, from_model, to_model, brief, ts],
            )?;
            Ok(())
        })
    }

    /// @deprecated — use [`record_model_transfer`].
    pub fn record_handoff(
        &self,
        session_id: &str,
        from_model: &str,
        to_model: &str,
        brief: &str,
        ts: f64,
    ) -> Result<(), AgentError> {
        self.record_model_transfer(session_id, from_model, to_model, brief, ts)
    }

    /// List model transfers for a session, oldest first.
    pub fn list_model_transfers(
        &self,
        session_id: &str,
    ) -> Result<Vec<ModelTransferRecord>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(format!("lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare(
                "SELECT session_id, from_model, to_model, brief, ts
                 FROM model_transfers WHERE session_id = ?1 ORDER BY ts ASC, id ASC",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(ModelTransferRecord {
                    session_id: row.get(0)?,
                    from_model: row.get(1)?,
                    to_model: row.get(2)?,
                    brief: row.get(3)?,
                    ts: row.get(4)?,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(e.to_string()))?);
        }
        Ok(out)
    }

    /// @deprecated — use [`list_model_transfers`].
    pub fn list_handoffs(&self, session_id: &str) -> Result<Vec<ModelTransferRecord>, AgentError> {
        self.list_model_transfers(session_id)
    }

    // ── Cross-platform session handoff (CLI → gateway) ───────────────────

    pub fn request_session_handoff(
        &self,
        session_id: &str,
        platform: &str,
    ) -> Result<bool, AgentError> {
        self.execute_write_with_result(|conn| {
            let updated = conn.execute(
                "UPDATE sessions SET handoff_state = 'pending', handoff_platform = ?1, handoff_error = NULL
                 WHERE id = ?2 AND (handoff_state IS NULL OR handoff_state IN ('completed', 'failed'))",
                params![platform, session_id],
            )?;
            Ok(updated > 0)
        })
    }

    pub fn get_session_handoff_status(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionHandoffStatus>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(format!("lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare(
                "SELECT handoff_state, handoff_platform, handoff_error FROM sessions WHERE id = ?1",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![session_id])
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let row = match rows.next() {
            Ok(Some(row)) => row,
            Ok(None) => return Ok(None),
            Err(e) => return Err(AgentError::Database(e.to_string())),
        };
        let state: Option<String> = row
            .get(0)
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let Some(state) = state else {
            return Ok(None);
        };
        Ok(Some(SessionHandoffStatus {
            state,
            platform: row
                .get(1)
                .map_err(|e| AgentError::Database(e.to_string()))?,
            error: row
                .get(2)
                .map_err(|e| AgentError::Database(e.to_string()))?,
        }))
    }

    pub fn list_pending_session_handoffs(&self) -> Result<Vec<PendingSessionHandoff>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(format!("lock poisoned: {e}")))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, handoff_platform, title FROM sessions
                 WHERE handoff_state = 'pending' ORDER BY started_at ASC",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PendingSessionHandoff {
                    session_id: row.get(0)?,
                    platform: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    title: row.get(2)?,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn claim_session_handoff(&self, session_id: &str) -> Result<bool, AgentError> {
        self.execute_write_with_result(|conn| {
            let updated = conn.execute(
                "UPDATE sessions SET handoff_state = 'running' WHERE id = ?1 AND handoff_state = 'pending'",
                params![session_id],
            )?;
            Ok(updated > 0)
        })
    }

    pub fn complete_session_handoff(&self, session_id: &str) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            conn.execute(
                "UPDATE sessions SET handoff_state = 'completed', handoff_error = NULL WHERE id = ?1",
                params![session_id],
            )?;
            Ok(())
        })
    }

    pub fn fail_session_handoff(&self, session_id: &str, error: &str) -> Result<(), AgentError> {
        let truncated = if error.chars().count() > 500 {
            error.chars().take(500).collect::<String>()
        } else {
            error.to_string()
        };
        self.execute_write(|conn| {
            conn.execute(
                "UPDATE sessions SET handoff_state = 'failed', handoff_error = ?1 WHERE id = ?2",
                params![truncated, session_id],
            )?;
            Ok(())
        })
    }

    pub fn rebind_session_routing(
        &self,
        session_id: &str,
        source: &str,
        user_id: &str,
    ) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            conn.execute(
                "UPDATE sessions SET source = ?1, user_id = ?2 WHERE id = ?3",
                params![source, user_id, session_id],
            )?;
            Ok(())
        })
    }

    // ── Title hygiene (matches hermes-agent) ──────────────────────────

    /// Maximum length for session titles.
    pub const MAX_TITLE_LENGTH: usize = 100;

    /// Sanitize a session title: strip control chars, zero-width chars,
    /// collapse whitespace, enforce max length.  Returns `Ok(None)` for
    /// empty/whitespace-only input.
    pub fn sanitize_title(title: &str) -> Result<Option<String>, AgentError> {
        if title.is_empty() {
            return Ok(None);
        }
        // Remove ASCII control characters (0x00-0x08, 0x0B, 0x0C, 0x0E-0x1F, 0x7F)
        let cleaned: String = title
            .chars()
            .filter(|c| {
                !matches!(*c as u32,
                    0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F |
                    // Zero-width chars
                    0x200B..=0x200F | 0xFEFF | 0xFFFC | 0xFFF9..=0xFFFB |
                    // Directional overrides
                    0x202A..=0x202E | 0x2060..=0x2069
                )
            })
            .collect();

        // Collapse whitespace runs + strip
        let collapsed: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            return Ok(None);
        }
        if collapsed.len() > Self::MAX_TITLE_LENGTH {
            return Err(AgentError::Validation(format!(
                "Title too long ({} chars, max {})",
                collapsed.len(),
                Self::MAX_TITLE_LENGTH
            )));
        }
        Ok(Some(collapsed))
    }

    /// Get a session by exact title match.
    pub fn get_session_by_title(&self, title: &str) -> Result<Option<SessionRecord>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let result = conn
            .query_row(
                "SELECT id, source, user_id, model, system_prompt,
                        stable_system_prompt, semi_stable_system_prompt, parent_session_id,
                        started_at, ended_at, end_reason, message_count, tool_call_count,
                        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                        reasoning_tokens, COALESCE(last_prompt_tokens, 0), estimated_cost_usd, title
                 FROM sessions WHERE title = ?1",
                params![title],
                Self::session_record_from_row,
            )
            .ok();
        Ok(result)
    }

    /// Resolve a session ID prefix or title to a full session ID.
    ///
    /// 1. Exact ID match
    /// 2. Unique prefix match on ID
    /// 3. Exact title match (with lineage: "my project" finds "my project #3")
    pub fn resolve_session(&self, id_or_title: &str) -> Result<Option<String>, AgentError> {
        // 1. Exact ID
        if self.get_session(id_or_title)?.is_some() {
            return Ok(Some(id_or_title.to_string()));
        }

        // 2. Prefix match (escape LIKE wildcards)
        let escaped = id_or_title
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| AgentError::Database(e.to_string()))?;
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM sessions WHERE id LIKE ?1 ESCAPE '\\' ORDER BY started_at DESC LIMIT 2",
                )
                .map_err(|e| AgentError::Database(e.to_string()))?;
            let matches: Vec<String> = stmt
                .query_map(params![format!("{escaped}%")], |row| row.get(0))
                .map_err(|e| AgentError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
            if matches.len() == 1 {
                return Ok(Some(matches[0].clone()));
            }
        }

        // 3. Title match (with lineage)
        self.resolve_session_by_title(id_or_title)
    }

    /// Resolve a title to a session ID, preferring the latest in a lineage.
    ///
    /// If "my project" exists AND "my project #3" exists, returns #3.
    pub fn resolve_session_by_title(&self, title: &str) -> Result<Option<String>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;

        // Search for numbered variants: "title #2", "title #3", etc.
        let escaped = title
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let mut stmt = conn
            .prepare(
                "SELECT id, title FROM sessions WHERE title LIKE ?1 ESCAPE '\\' ORDER BY started_at DESC LIMIT 1",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let numbered: Option<String> = stmt
            .query_row(params![format!("{escaped} #%")], |row| row.get(0))
            .ok();
        if let Some(id) = numbered {
            return Ok(Some(id));
        }

        // Exact title match
        let exact: Option<String> = conn
            .query_row(
                "SELECT id FROM sessions WHERE title = ?1",
                params![title],
                |row| row.get(0),
            )
            .ok();
        Ok(exact)
    }

    /// Generate the next title in a lineage.
    ///
    /// "my session" → "my session #2", "my session #2" → "my session #3"
    pub fn next_title_in_lineage(&self, base_title: &str) -> Result<String, AgentError> {
        // Strip existing " #N" suffix to find the true base
        let base = if let Some(idx) = base_title.rfind(" #") {
            let suffix = &base_title[idx + 2..];
            if suffix.chars().all(|c| c.is_ascii_digit()) {
                &base_title[..idx]
            } else {
                base_title
            }
        } else {
            base_title
        };

        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let escaped = base
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let mut stmt = conn
            .prepare("SELECT title FROM sessions WHERE title = ?1 OR title LIKE ?2 ESCAPE '\\'")
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let existing: Vec<String> = stmt
            .query_map(params![base, format!("{escaped} #%")], |row| row.get(0))
            .map_err(|e| AgentError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        if existing.is_empty() {
            return Ok(base.to_string());
        }

        // Find the highest number
        let mut max_num: u32 = 1; // The unnumbered original counts as #1
        for t in &existing {
            if let Some(idx) = t.rfind(" #")
                && let Ok(n) = t[idx + 2..].parse::<u32>()
            {
                max_num = max_num.max(n);
            }
        }

        Ok(format!("{base} #{}", max_num + 1))
    }

    // ── Session lifecycle ─────────────────────────────────────────────

    /// Mark a session as ended with the given reason.
    pub fn end_session(&self, id: &str, reason: &str) -> Result<(), AgentError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        self.execute_write(|conn| {
            conn.execute(
                "UPDATE sessions SET ended_at = ?1, end_reason = ?2 WHERE id = ?3",
                params![now, reason, id],
            )?;
            Ok(())
        })
    }

    /// Clear ended_at and end_reason so a session can be resumed.
    pub fn reopen_session(&self, id: &str) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            conn.execute(
                "UPDATE sessions SET ended_at = NULL, end_reason = NULL WHERE id = ?1",
                params![id],
            )?;
            Ok(())
        })
    }

    // ── Filtered listing ──────────────────────────────────────────────

    /// List sessions filtered by source platform.
    pub fn list_sessions_by_source(
        &self,
        source: &str,
        limit: usize,
    ) -> Result<Vec<SessionSummary>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, source, model, started_at, message_count, title
                 FROM sessions WHERE source = ?1 ORDER BY started_at DESC LIMIT ?2",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![source, limit as i64], |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    model: row.get(2)?,
                    started_at: row.get(3)?,
                    message_count: row.get(4)?,
                    title: row.get(5)?,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| AgentError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    /// Rich session listing with first-message preview, for display.
    pub fn list_sessions_rich(
        &self,
        source: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SessionRichSummary>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let base_sql = "SELECT s.id, s.source, s.model, s.started_at, s.message_count, s.title,
                        COALESCE(
                            (SELECT SUBSTR(REPLACE(REPLACE(m.content, X'0A', ' '), X'0D', ' '), 1, 63)
                             FROM messages m
                             WHERE m.session_id = s.id AND m.role = 'user' AND m.content IS NOT NULL
                             ORDER BY m.timestamp, m.id LIMIT 1),
                            ''
                        ) AS preview,
                        COALESCE(
                            (SELECT MAX(m2.timestamp) FROM messages m2 WHERE m2.session_id = s.id),
                            s.started_at
                        ) AS last_active
                 FROM sessions s";

        let parse_row = |row: &rusqlite::Row| -> rusqlite::Result<SessionRichSummary> {
            Ok(SessionRichSummary {
                id: row.get(0)?,
                source: row.get(1)?,
                model: row.get(2)?,
                started_at: row.get(3)?,
                message_count: row.get(4)?,
                title: row.get(5)?,
                preview: row.get::<_, String>(6).unwrap_or_default(),
                last_active: row.get(7)?,
            })
        };

        let mut result = Vec::new();
        if let Some(src) = source {
            let sql = format!("{base_sql} WHERE s.source = ?1 ORDER BY s.started_at DESC LIMIT ?2");
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| AgentError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(params![src, limit as i64], parse_row)
                .map_err(|e| AgentError::Database(e.to_string()))?;
            for row in rows {
                result.push(row.map_err(|e| AgentError::Database(e.to_string()))?);
            }
        } else {
            let sql = format!("{base_sql} ORDER BY s.started_at DESC LIMIT ?1");
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| AgentError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(params![limit as i64], parse_row)
                .map_err(|e| AgentError::Database(e.to_string()))?;
            for row in rows {
                result.push(row.map_err(|e| AgentError::Database(e.to_string()))?);
            }
        };

        Ok(result)
    }

    // ── Prune ─────────────────────────────────────────────────────────

    /// Delete ended sessions older than `days`. Returns count of deleted sessions.
    /// Only prunes sessions with `ended_at IS NOT NULL` (active sessions are safe).
    pub fn prune_sessions(
        &self,
        older_than_days: u32,
        source: Option<&str>,
    ) -> Result<usize, AgentError> {
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
            - (older_than_days as f64 * 86400.0);

        let source_owned = source.map(String::from);
        self.execute_write_with_result(|conn| {
            let session_ids: Vec<String> = if let Some(ref src) = source_owned {
                let mut stmt = conn.prepare(
                    "SELECT id FROM sessions WHERE started_at < ?1 AND ended_at IS NOT NULL AND source = ?2"
                )?;
                stmt.query_map(params![cutoff, src], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect()
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id FROM sessions WHERE started_at < ?1 AND ended_at IS NOT NULL"
                )?;
                stmt.query_map(params![cutoff], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect()
            };
            let count = session_ids.len();
            for sid in &session_ids {
                conn.execute("DELETE FROM messages WHERE session_id = ?1", params![sid])?;
                conn.execute("DELETE FROM sessions WHERE id = ?1", params![sid])?;
            }
            Ok(count)
        })
    }

    // ── Export ─────────────────────────────────────────────────────────

    /// Export a single session with all its messages as a JSON-serializable struct.
    pub fn export_session_jsonl(&self, id: &str) -> Result<Option<SessionExport>, AgentError> {
        let session = match self.get_session(id)? {
            Some(s) => s,
            None => return Ok(None),
        };
        let messages = self.get_messages(id)?;
        Ok(Some(SessionExport { session, messages }))
    }

    /// Export all sessions (optionally filtered by source) for JSONL backup.
    pub fn export_all_jsonl(&self, source: Option<&str>) -> Result<Vec<SessionExport>, AgentError> {
        let sessions = if let Some(src) = source {
            self.list_sessions_by_source(src, 100_000)?
        } else {
            self.list_sessions(100_000)?
        };
        let mut result = Vec::new();
        for summary in &sessions {
            if let Some(export) = self.export_session_jsonl(&summary.id)? {
                result.push(export);
            }
        }
        Ok(result)
    }

    // ── Session statistics ────────────────────────────────────────────

    /// Return aggregate statistics matching hermes-agent's `sessions stats`.
    pub fn session_statistics(&self) -> Result<SessionStats, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let (total_sessions, total_messages): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(message_count), 0) FROM sessions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let mut stmt = conn
            .prepare("SELECT source, COUNT(*) FROM sessions GROUP BY source ORDER BY COUNT(*) DESC")
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let by_source: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| AgentError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        // Database file size (best-effort)
        let db_size_bytes = conn
            .query_row(
                "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0);

        Ok(SessionStats {
            total_sessions,
            total_messages,
            by_source,
            db_size_bytes,
        })
    }

    // ── Message CRUD ──────────────────────────────────────────────────

    pub fn save_message(
        &self,
        session_id: &str,
        msg: &Message,
        timestamp: f64,
    ) -> Result<(), AgentError> {
        let tool_calls_json = msg
            .tool_calls
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(AgentError::Serde)?;

        self.execute_write(|conn| {
            conn.execute(
                "INSERT INTO messages
                 (session_id, role, content, tool_call_id, tool_calls, tool_name, timestamp,
                  finish_reason, reasoning)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    session_id,
                    msg.role.as_str(),
                    msg.text_content(),
                    msg.tool_call_id.as_deref(),
                    tool_calls_json,
                    msg.name.as_deref(),
                    timestamp,
                    msg.finish_reason.as_deref(),
                    msg.reasoning.as_deref(),
                ],
            )?;
            // Update session message count
            conn.execute(
                "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
                params![session_id],
            )?;
            Ok(())
        })
    }

    pub fn get_messages(&self, session_id: &str) -> Result<Vec<Message>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT role, content, tool_call_id, tool_calls, finish_reason, reasoning, tool_name,
                        timestamp
                 FROM messages WHERE session_id = ?1 ORDER BY timestamp ASC, id ASC",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let role_str: String = row.get(0)?;
                let content: Option<String> = row.get(1)?;
                let tool_call_id: Option<String> = row.get(2)?;
                let tool_calls_json: Option<String> = row.get(3)?;
                let finish_reason: Option<String> = row.get(4)?;
                let reasoning: Option<String> = row.get(5)?;
                let tool_name: Option<String> = row.get(6)?;
                let timestamp: f64 = row.get(7)?;

                Ok((
                    role_str,
                    content,
                    tool_call_id,
                    tool_calls_json,
                    finish_reason,
                    reasoning,
                    tool_name,
                    timestamp,
                ))
            })
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let mut messages = Vec::new();
        for row in rows {
            let (
                role_str,
                content,
                tool_call_id,
                tool_calls_json,
                finish_reason,
                reasoning,
                tool_name,
                timestamp,
            ) = row.map_err(|e| AgentError::Database(e.to_string()))?;

            let role = match role_str.as_str() {
                "system" => Role::System,
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::User,
            };

            let tool_calls = tool_calls_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(AgentError::Serde)?;

            let mut msg = match role {
                Role::System => Message::system(content.as_deref().unwrap_or_default()),
                Role::User => Message::user(content.as_deref().unwrap_or_default()),
                Role::Assistant => Message::assistant(content.as_deref().unwrap_or_default()),
                Role::Tool => Message::tool_result(
                    tool_call_id.as_deref().unwrap_or_default(),
                    tool_name.as_deref().unwrap_or_default(),
                    content.as_deref().unwrap_or_default(),
                ),
            };
            msg.tool_calls = tool_calls;
            msg.tool_call_id = tool_call_id;
            msg.finish_reason = finish_reason;
            msg.reasoning = reasoning;
            msg.created_at = Some(timestamp);

            messages.push(msg);
        }
        Ok(messages)
    }

    /// Replace all persisted messages for a session in one transaction.
    pub fn replace_messages(
        &self,
        session_id: &str,
        messages: &[Message],
        timestamp: f64,
    ) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            conn.execute(
                "DELETE FROM messages WHERE session_id = ?1",
                params![session_id],
            )?;
            for (i, msg) in messages.iter().enumerate() {
                let tool_calls_json = msg
                    .tool_calls
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                let msg_ts = msg
                    .created_at
                    .unwrap_or(timestamp + (i as f64) * 0.001);
                conn.execute(
                    "INSERT INTO messages
                     (session_id, role, content, tool_call_id, tool_calls, tool_name, timestamp,
                      finish_reason, reasoning)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        session_id,
                        msg.role.as_str(),
                        msg.text_content(),
                        msg.tool_call_id.as_deref(),
                        tool_calls_json,
                        msg.name.as_deref(),
                        msg_ts,
                        msg.finish_reason.as_deref(),
                        msg.reasoning.as_deref(),
                    ],
                )?;
            }
            conn.execute(
                "UPDATE sessions SET message_count = ?1 WHERE id = ?2",
                params![messages.len() as i64, session_id],
            )?;
            Ok(())
        })
    }

    // ── FTS5 Search ───────────────────────────────────────────────────

    /// Full-text search across all sessions using FTS5 with BM25 ranking.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;

        // Escape special FTS5 chars to prevent injection
        let safe_query = Self::escape_fts5_query(query);

        let mut stmt = conn
            .prepare(
                "SELECT m.session_id, m.role,
                        snippet(messages_fts, 0, '<b>', '</b>', '...', 32),
                        rank
                 FROM messages_fts
                 JOIN messages m ON m.id = messages_fts.rowid
                 WHERE messages_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![safe_query, limit as i64], |row| {
                Ok(SearchResult {
                    session_id: row.get(0)?,
                    role: row.get(1)?,
                    snippet: row.get(2)?,
                    score: row.get(3)?,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| AgentError::Database(e.to_string()))?);
        }
        Ok(results)
    }

    /// Full-text search returning one ranked hit per session with session metadata.
    pub fn search_sessions_rich(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SessionSearchHit>, AgentError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let safe_query = Self::escape_fts5_query(query);
        let mut stmt = conn
            .prepare(
                "WITH hits AS (
                    SELECT m.id AS message_rowid,
                           m.session_id,
                           m.role,
                           rank,
                           ROW_NUMBER() OVER (
                               PARTITION BY m.session_id
                               ORDER BY rank, m.id
                           ) AS rn
                    FROM messages_fts
                    JOIN messages m ON m.id = messages_fts.rowid
                    WHERE messages_fts MATCH ?1
                 )
                 SELECT s.id, s.source, s.model, s.started_at, s.message_count, s.title,
                        COALESCE(
                            (SELECT SUBSTR(REPLACE(REPLACE(m0.content, X'0A', ' '), X'0D', ' '), 1, 63)
                             FROM messages m0
                             WHERE m0.session_id = s.id AND m0.role = 'user' AND m0.content IS NOT NULL
                             ORDER BY m0.timestamp, m0.id LIMIT 1),
                            ''
                        ) AS preview,
                        COALESCE(
                            (SELECT MAX(m2.timestamp) FROM messages m2 WHERE m2.session_id = s.id),
                            s.started_at
                        ) AS last_active,
                        h.role,
                        snippet(messages_fts, 0, '<b>', '</b>', '...', 32) AS snippet,
                        h.rank
                 FROM hits h
                 JOIN sessions s ON s.id = h.session_id
                 JOIN messages_fts ON messages_fts.rowid = h.message_rowid
                 WHERE h.rn = 1
                 ORDER BY h.rank
                 LIMIT ?2",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![safe_query, limit as i64], |row| {
                Ok(SessionSearchHit {
                    session: SessionRichSummary {
                        id: row.get(0)?,
                        source: row.get(1)?,
                        model: row.get(2)?,
                        started_at: row.get(3)?,
                        message_count: row.get(4)?,
                        title: row.get(5)?,
                        preview: row.get::<_, String>(6).unwrap_or_default(),
                        last_active: row.get(7)?,
                    },
                    role: row.get(8)?,
                    snippet: row.get::<_, String>(9).unwrap_or_default(),
                    score: row.get(10)?,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let mut hits = Vec::new();
        for row in rows {
            hits.push(row.map_err(|e| AgentError::Database(e.to_string()))?);
        }

        Ok(hits)
    }

    /// Escape an FTS5 query to prevent syntax errors from user input.
    /// Wraps each token in double-quotes to treat them as literal terms.
    fn escape_fts5_query(query: &str) -> String {
        query
            .split_whitespace()
            .map(|token| {
                let escaped = token.replace('"', "\"\"");
                format!("\"{escaped}\"")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    // ── Session splitting (compression) ───────────────────────────────

    /// Create a child session linked to `parent_id` (used when context
    /// compression triggers a session split).
    pub fn split_session(
        &self,
        parent_id: &str,
        new_id: &str,
        source: &str,
        model: Option<&str>,
        started_at: f64,
    ) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            conn.execute(
                "INSERT INTO sessions (id, source, model, parent_session_id, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![new_id, source, model, parent_id, started_at],
            )?;
            // Mark parent as ended
            conn.execute(
                "UPDATE sessions SET ended_at = ?1, end_reason = 'compression' WHERE id = ?2",
                params![started_at, parent_id],
            )?;
            Ok(())
        })
    }

    // ── Write contention helper ───────────────────────────────────────

    /// Execute a write transaction with `BEGIN IMMEDIATE` and jitter retry.
    ///
    /// `BEGIN IMMEDIATE` acquires the WAL write lock at transaction start
    /// (not at commit time), so contention surfaces immediately. On
    /// "database is locked", sleep random 20-150ms and retry — breaking
    /// the convoy pattern that SQLite's deterministic backoff creates.
    fn execute_write<F>(&self, f: F) -> Result<(), AgentError>
    where
        F: Fn(&Connection) -> Result<(), rusqlite::Error>,
    {
        self.execute_write_with_result(|conn| {
            f(conn)?;
            Ok(())
        })
    }

    /// Like `execute_write` but returns a value from the transaction closure.
    fn execute_write_with_result<T, F>(&self, f: F) -> Result<T, AgentError>
    where
        F: Fn(&Connection) -> Result<T, rusqlite::Error>,
    {
        let mut rng = rand::rng();

        for attempt in 0..WRITE_MAX_RETRIES {
            let conn = self
                .conn
                .lock()
                .map_err(|e| AgentError::Database(e.to_string()))?;

            if let Err(e) = conn.execute_batch("BEGIN IMMEDIATE") {
                if Self::is_locked(&e) && attempt < WRITE_MAX_RETRIES - 1 {
                    drop(conn);
                    let jitter_ms = rng.random_range(WRITE_RETRY_MIN_MS..WRITE_RETRY_MAX_MS);
                    std::thread::sleep(Duration::from_millis(jitter_ms));
                    continue;
                }
                return Err(AgentError::Database(e.to_string()));
            }

            match f(&conn) {
                Ok(val) => {
                    conn.execute_batch("COMMIT")
                        .map_err(|e| AgentError::Database(e.to_string()))?;
                    drop(conn);
                    self.maybe_checkpoint();
                    return Ok(val);
                }
                Err(e) if Self::is_locked(&e) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    drop(conn);
                    if attempt < WRITE_MAX_RETRIES - 1 {
                        let jitter_ms = rng.random_range(WRITE_RETRY_MIN_MS..WRITE_RETRY_MAX_MS);
                        std::thread::sleep(Duration::from_millis(jitter_ms));
                        continue;
                    }
                    return Err(AgentError::Database(e.to_string()));
                }
                Err(e) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(AgentError::Database(e.to_string()));
                }
            }
        }
        Err(AgentError::Database(format!(
            "Write failed after {WRITE_MAX_RETRIES} retries"
        )))
    }

    fn is_locked(e: &rusqlite::Error) -> bool {
        let msg = e.to_string().to_lowercase();
        msg.contains("locked") || msg.contains("busy")
    }

    /// Best-effort PASSIVE WAL checkpoint every N writes.
    fn maybe_checkpoint(&self) {
        if let Ok(mut count) = self.write_count.lock() {
            *count += 1;
            if *count % CHECKPOINT_EVERY_N_WRITES == 0
                && let Ok(conn) = self.conn.lock()
            {
                let _ = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)");
            }
        }
    }

    /// Graceful close — checkpoint WAL before dropping connection.
    pub fn close(&self) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)");
        }
    }

    /// Query historical insights for the last `days` days.
    ///
    /// Returns an `InsightsReport` with session counts, token totals,
    /// cost estimates, per-model and per-platform breakdowns, top tools,
    /// and daily activity — mirroring hermes-agent's `InsightsEngine.generate()`.
    pub fn query_insights(&self, days: u32) -> Result<InsightsReport, AgentError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
            - (days as f64 * 86400.0);

        // ── Overview ──────────────────────────────────────────────────
        let overview_row = conn
            .query_row(
                "SELECT
               COUNT(*) as sessions,
               COALESCE(SUM(message_count), 0) as messages,
               COALESCE(SUM(tool_call_count), 0) as tool_calls,
               COALESCE(SUM(input_tokens), 0) as input_tokens,
               COALESCE(SUM(output_tokens), 0) as output_tokens,
               COALESCE(SUM(cache_read_tokens), 0) as cache_read,
               COALESCE(SUM(cache_write_tokens), 0) as cache_write,
               COALESCE(SUM(reasoning_tokens), 0) as reasoning,
               COALESCE(SUM(estimated_cost_usd), 0.0) as total_cost
             FROM sessions WHERE started_at >= ?",
                params![cutoff],
                |row| {
                    Ok(InsightsOverview {
                        total_sessions: row.get(0)?,
                        total_messages: row.get(1)?,
                        total_tool_calls: row.get(2)?,
                        total_input_tokens: row.get(3)?,
                        total_output_tokens: row.get(4)?,
                        total_cache_read_tokens: row.get(5)?,
                        total_cache_write_tokens: row.get(6)?,
                        total_reasoning_tokens: row.get(7)?,
                        estimated_total_cost_usd: row.get(8)?,
                    })
                },
            )
            .unwrap_or_else(|_| InsightsOverview::default());

        // ── Per-model breakdown ───────────────────────────────────────
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(model, 'unknown') as model,
                    COUNT(*) as sessions,
                    COALESCE(SUM(input_tokens), 0) as input,
                    COALESCE(SUM(output_tokens), 0) as output,
                    COALESCE(SUM(estimated_cost_usd), 0.0) as cost
             FROM sessions WHERE started_at >= ?
             GROUP BY model ORDER BY sessions DESC LIMIT 10",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let models: Vec<ModelBreakdown> = stmt
            .query_map(params![cutoff], |row| {
                Ok(ModelBreakdown {
                    model: row.get(0)?,
                    sessions: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    estimated_cost_usd: row.get(4)?,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        // ── Per-platform (source) breakdown ────────────────────────────
        let mut stmt2 = conn
            .prepare(
                "SELECT COALESCE(source, 'unknown') as source,
                    COUNT(*) as sessions,
                    COALESCE(SUM(tool_call_count), 0) as tool_calls
             FROM sessions WHERE started_at >= ?
             GROUP BY source ORDER BY sessions DESC",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let platforms: Vec<PlatformBreakdown> = stmt2
            .query_map(params![cutoff], |row| {
                Ok(PlatformBreakdown {
                    source: row.get(0)?,
                    sessions: row.get(1)?,
                    tool_calls: row.get(2)?,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        // ── Top tools (from assistant message tool_calls JSON) ─────────
        let mut tool_counts: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        {
            let mut tstmt = conn
                .prepare(
                    "SELECT m.tool_calls FROM messages m
                 JOIN sessions s ON s.id = m.session_id
                 WHERE s.started_at >= ? AND m.role = 'assistant' AND m.tool_calls IS NOT NULL",
                )
                .map_err(|e| AgentError::Database(e.to_string()))?;
            let rows: Vec<String> = tstmt
                .query_map(params![cutoff], |row| row.get(0))
                .map_err(|e| AgentError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
            for raw in rows {
                if let Ok(calls) = serde_json::from_str::<serde_json::Value>(&raw)
                    && let Some(arr) = calls.as_array()
                {
                    for call in arr {
                        if let Some(name) = call
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                        {
                            *tool_counts.entry(name.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
        let mut top_tools: Vec<ToolUsage> = tool_counts
            .into_iter()
            .map(|(name, count)| ToolUsage { name, count })
            .collect();
        top_tools.sort_by_key(|tool| std::cmp::Reverse(tool.count));
        top_tools.truncate(10);

        // ── Daily activity (last 14 days) ──────────────────────────────
        let daily_cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
            - (14.0 * 86400.0);
        let mut dstmt = conn
            .prepare(
                "SELECT date(started_at, 'unixepoch') as day, COUNT(*) as sessions
             FROM sessions WHERE started_at >= ?
             GROUP BY day ORDER BY day ASC LIMIT 14",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let daily_activity: Vec<DailyActivity> = dstmt
            .query_map(params![daily_cutoff], |row| {
                Ok(DailyActivity {
                    day: row.get(0)?,
                    sessions: row.get(1)?,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(InsightsReport {
            days,
            overview: overview_row,
            models,
            platforms,
            top_tools,
            daily_activity,
        })
    }

    // ── Persistent goals ───────────────────────────────────────────────

    /// Ensure a minimal `sessions` row exists so FK-dependent goal rows can attach.
    ///
    /// Slash commands like `/goal` can run before the first chat turn persists
    /// the session header; without this stub, `session_goals` inserts fail with
    /// `FOREIGN KEY constraint failed`.
    pub fn ensure_session_row(
        &self,
        session_id: &str,
        source: &str,
        user_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<(), AgentError> {
        if session_id.trim().is_empty() {
            return Err(AgentError::Config(
                "session_id is required for session persistence".into(),
            ));
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        self.execute_write(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO sessions (id, source, user_id, model, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![session_id, source, user_id, model, now],
            )?;
            Ok(())
        })
    }

    pub fn goals_active(&self, session_id: &str) -> Result<StoredGoalState, AgentError> {
        if session_id.trim().is_empty() {
            return Ok(StoredGoalState::default());
        }
        type GoalRow = (
            String,
            String,
            i64,
            i64,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
        );
        let conn = self
            .conn
            .lock()
            .map_err(|_| AgentError::Database("database lock poisoned".into()))?;
        let goal_row: Option<GoalRow> = conn
            .query_row(
                "SELECT goal_text, status, turns_used, max_turns, paused_reason,
                        last_verdict, last_reason, consecutive_parse_failures, contract_json
                 FROM session_goals WHERE session_id = ?1",
                params![session_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .ok();
        let Some((
            goal_text,
            status,
            turns_used,
            max_turns,
            paused_reason,
            last_verdict,
            last_reason,
            consecutive_parse_failures,
            contract_json,
        )) = goal_row
        else {
            return Ok(StoredGoalState::default());
        };
        let mut stmt = conn
            .prepare(
                "SELECT id, text, done FROM session_subgoals
                 WHERE session_id = ?1 ORDER BY position ASC",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;
        let subgoals = stmt
            .query_map(params![session_id], |row| {
                Ok(StoredSubGoal {
                    id: row.get::<_, i64>(0)? as u64,
                    text: row.get(1)?,
                    done: row.get::<_, i64>(2)? != 0,
                })
            })
            .map_err(|e| AgentError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(StoredGoalState {
            goal_text: Some(goal_text),
            subgoals,
            status,
            turns_used: turns_used.max(0) as u32,
            max_turns: max_turns.max(1) as u32,
            paused_reason,
            last_verdict,
            last_reason,
            consecutive_parse_failures: consecutive_parse_failures.max(0) as u32,
            contract_json,
        })
    }

    pub fn goals_set(
        &self,
        session_id: &str,
        text: &str,
        max_turns: u32,
    ) -> Result<(), AgentError> {
        self.goals_set_with_contract(session_id, text, max_turns, None)
    }

    /// Set goal text with optional JSON contract (018 GoalContract).
    pub fn goals_set_with_contract(
        &self,
        session_id: &str,
        text: &str,
        max_turns: u32,
        contract_json: Option<&str>,
    ) -> Result<(), AgentError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(AgentError::Config("goal text must not be empty".into()));
        }
        if session_id.trim().is_empty() {
            return Err(AgentError::Config(
                "session_id is required for goal operations".into(),
            ));
        }
        let now = chrono::Utc::now().timestamp() as f64;
        let budget = max_turns.max(1) as i64;
        let contract = contract_json
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        self.execute_write(|conn| {
            conn.execute(
                "INSERT INTO session_goals (
                    session_id, goal_text, created_at, status, turns_used, max_turns,
                    paused_reason, last_verdict, last_reason, consecutive_parse_failures,
                    contract_json
                 ) VALUES (?1, ?2, ?3, 'active', 0, ?4, NULL, NULL, NULL, 0, ?5)
                 ON CONFLICT(session_id) DO UPDATE SET
                    goal_text = excluded.goal_text,
                    status = 'active',
                    turns_used = 0,
                    max_turns = excluded.max_turns,
                    paused_reason = NULL,
                    last_verdict = NULL,
                    last_reason = NULL,
                    consecutive_parse_failures = 0,
                    contract_json = excluded.contract_json",
                params![session_id, trimmed, now, budget, contract],
            )?;
            conn.execute(
                "DELETE FROM session_subgoals WHERE session_id = ?1",
                params![session_id],
            )?;
            Ok(())
        })
    }

    pub fn goals_clear(&self, session_id: &str) -> Result<(), AgentError> {
        if session_id.trim().is_empty() {
            return Ok(());
        }
        self.execute_write(|conn| {
            conn.execute(
                "DELETE FROM session_subgoals WHERE session_id = ?1",
                params![session_id],
            )?;
            conn.execute(
                "DELETE FROM session_goals WHERE session_id = ?1",
                params![session_id],
            )?;
            Ok(())
        })
    }

    pub fn goals_push_subgoal(&self, session_id: &str, text: &str) -> Result<(), AgentError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(AgentError::Config("subgoal text must not be empty".into()));
        }
        self.execute_write(|conn| {
            let has_goal: bool = conn
                .query_row(
                    "SELECT 1 FROM session_goals WHERE session_id = ?1",
                    params![session_id],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if !has_goal {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                    Some("set a top-level goal with /goal before adding subgoals".into()),
                ));
            }
            let next_pos: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(position), 0) + 1 FROM session_subgoals WHERE session_id = ?1",
                    params![session_id],
                    |row| row.get(0),
                )
                .unwrap_or(1);
            conn.execute(
                "INSERT INTO session_subgoals (session_id, text, done, position)
                 VALUES (?1, ?2, 0, ?3)",
                params![session_id, trimmed, next_pos],
            )?;
            Ok(())
        })
    }

    pub fn goals_complete_subgoal(
        &self,
        session_id: &str,
    ) -> Result<Option<StoredSubGoal>, AgentError> {
        self.execute_write_with_result(|conn| {
            let target: Option<(i64, String)> = conn
                .query_row(
                    "SELECT id, text FROM session_subgoals
                     WHERE session_id = ?1 AND done = 0
                     ORDER BY position DESC LIMIT 1",
                    params![session_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();
            let Some((id, text)) = target else {
                return Ok(None);
            };
            conn.execute(
                "UPDATE session_subgoals SET done = 1 WHERE id = ?1",
                params![id],
            )?;
            Ok(Some(StoredSubGoal {
                id: id as u64,
                text,
                done: true,
            }))
        })
    }

    pub fn goals_clear_subgoals(&self, session_id: &str) -> Result<u32, AgentError> {
        if session_id.trim().is_empty() {
            return Ok(0);
        }
        self.execute_write_with_result(|conn| {
            let has_goal: bool = conn
                .query_row(
                    "SELECT 1 FROM session_goals WHERE session_id = ?1",
                    params![session_id],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if !has_goal {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                    Some("set a top-level goal with /goal before managing subgoals".into()),
                ));
            }
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM session_subgoals WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )?;
            conn.execute(
                "DELETE FROM session_subgoals WHERE session_id = ?1",
                params![session_id],
            )?;
            Ok(count.max(0) as u32)
        })
    }

    pub fn goals_remove_subgoal(
        &self,
        session_id: &str,
        index_1based: usize,
    ) -> Result<String, AgentError> {
        if session_id.trim().is_empty() {
            return Err(AgentError::Config("session_id is required".into()));
        }
        self.execute_write_with_result(|conn| {
            let has_goal: bool = conn
                .query_row(
                    "SELECT 1 FROM session_goals WHERE session_id = ?1",
                    params![session_id],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if !has_goal {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                    Some("set a top-level goal with /goal before managing subgoals".into()),
                ));
            }
            let mut stmt = conn.prepare(
                "SELECT id, text FROM session_subgoals
                 WHERE session_id = ?1 ORDER BY position ASC",
            )?;
            let rows: Vec<(i64, String)> = stmt
                .query_map(params![session_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            if index_1based == 0 || index_1based > rows.len() {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                    Some(format!("index out of range (1..{})", rows.len())),
                ));
            }
            let (id, text) = rows[index_1based - 1].clone();
            conn.execute("DELETE FROM session_subgoals WHERE id = ?1", params![id])?;
            Ok(text)
        })
    }

    pub fn goals_pause(&self, session_id: &str, reason: &str) -> Result<(), AgentError> {
        if session_id.trim().is_empty() {
            return Ok(());
        }
        self.execute_write(|conn| {
            conn.execute(
                "UPDATE session_goals SET status = 'paused', paused_reason = ?2
                 WHERE session_id = ?1",
                params![session_id, reason],
            )?;
            Ok(())
        })
    }

    pub fn goals_resume(&self, session_id: &str, reset_budget: bool) -> Result<(), AgentError> {
        if session_id.trim().is_empty() {
            return Ok(());
        }
        self.execute_write(|conn| {
            if reset_budget {
                conn.execute(
                    "UPDATE session_goals
                     SET status = 'active', paused_reason = NULL, turns_used = 0
                     WHERE session_id = ?1",
                    params![session_id],
                )?;
            } else {
                conn.execute(
                    "UPDATE session_goals SET status = 'active', paused_reason = NULL
                     WHERE session_id = ?1",
                    params![session_id],
                )?;
            }
            Ok(())
        })
    }

    pub fn goals_save_loop_state(
        &self,
        session_id: &str,
        state: &StoredGoalState,
    ) -> Result<(), AgentError> {
        if session_id.trim().is_empty() {
            return Ok(());
        }
        self.execute_write(|conn| {
            conn.execute(
                "UPDATE session_goals SET
                    status = ?2,
                    turns_used = ?3,
                    max_turns = ?4,
                    paused_reason = ?5,
                    last_verdict = ?6,
                    last_reason = ?7,
                    consecutive_parse_failures = ?8,
                    contract_json = ?9
                 WHERE session_id = ?1",
                params![
                    session_id,
                    state.status,
                    state.turns_used as i64,
                    state.max_turns as i64,
                    state.paused_reason,
                    state.last_verdict,
                    state.last_reason,
                    state.consecutive_parse_failures as i64,
                    state.contract_json,
                ],
            )?;
            Ok(())
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> SessionDb {
        SessionDb::open_in_memory().expect("in-memory db")
    }

    fn sample_session(id: &str) -> SessionRecord {
        SessionRecord {
            id: id.to_string(),
            source: "cli".to_string(),
            user_id: None,
            model: Some("mock/test".to_string()),
            system_prompt: None,
            stable_system_prompt: None,
            semi_stable_system_prompt: None,
            parent_session_id: None,
            started_at: 1720000000.0,
            ended_at: None,
            end_reason: None,
            message_count: 0,
            tool_call_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            last_prompt_tokens: 0,
            estimated_cost_usd: None,
            title: Some("Test session".to_string()),
        }
    }

    #[test]
    fn session_crud() {
        let db = test_db();
        let session = sample_session("s1");
        db.save_session(&session).expect("save");

        let loaded = db.get_session("s1").expect("get").expect("found");
        assert_eq!(loaded.id, "s1");
        assert_eq!(loaded.source, "cli");
        assert_eq!(loaded.title.as_deref(), Some("Test session"));

        let list = db.list_sessions(10).expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "s1");

        db.delete_session("s1").expect("delete");
        assert!(db.get_session("s1").expect("get").is_none());
    }

    #[test]
    fn message_crud() {
        let db = test_db();
        db.save_session(&sample_session("s1"))
            .expect("save session");

        let msg = Message::user("Hello, agent!");
        db.save_message("s1", &msg, 1720000001.0).expect("save msg");

        let reply = Message::assistant("Hi there!");
        db.save_message("s1", &reply, 1720000002.0)
            .expect("save reply");

        let messages = db.get_messages("s1").expect("get messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].text_content(), "Hello, agent!");
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].text_content(), "Hi there!");

        // Verify message_count incremented
        let session = db.get_session("s1").expect("get").expect("found");
        assert_eq!(session.message_count, 2);
    }

    #[test]
    fn tool_message_roundtrip_preserves_tool_name() {
        let db = test_db();
        db.save_session(&sample_session("tool-session"))
            .expect("save session");

        let msg = Message::tool_result("call_123", "session_search", "search complete");
        db.save_message("tool-session", &msg, 1720000003.0)
            .expect("save tool msg");

        let messages = db.get_messages("tool-session").expect("get messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::Tool);
        assert_eq!(messages[0].name.as_deref(), Some("session_search"));
        assert_eq!(messages[0].tool_call_id.as_deref(), Some("call_123"));
        assert_eq!(messages[0].text_content(), "search complete");
    }

    #[test]
    fn fts5_search() {
        let db = test_db();
        db.save_session(&sample_session("s1")).expect("save");

        db.save_message("s1", &Message::user("Rust ownership model"), 1.0)
            .expect("msg1");
        db.save_message("s1", &Message::assistant("Borrow checker explanation"), 2.0)
            .expect("msg2");
        db.save_message("s1", &Message::user("Python garbage collection"), 3.0)
            .expect("msg3");

        let results = db.search("Rust", 10).expect("search");
        assert!(!results.is_empty(), "Should find 'Rust' in messages");
        assert_eq!(results[0].session_id, "s1");
    }

    #[test]
    fn rich_session_search_returns_ranked_unique_sessions() {
        let db = test_db();

        let mut s1 = sample_session("s1");
        s1.title = Some("Rust ownership deep dive".into());
        db.save_session(&s1).expect("save s1");
        db.save_message("s1", &Message::user("Rust ownership model"), 1.0)
            .expect("msg1");
        db.save_message("s1", &Message::assistant("Borrow checker explanation"), 2.0)
            .expect("msg2");

        let mut s2 = sample_session("s2");
        s2.title = Some("Python reference guide".into());
        db.save_session(&s2).expect("save s2");
        db.save_message("s2", &Message::user("Python uses reference counting"), 3.0)
            .expect("msg3");
        db.save_message("s2", &Message::assistant("Rust differs here"), 4.0)
            .expect("msg4");

        let hits = db.search_sessions_rich("Rust", 10).expect("rich search");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].session.id, "s1");
        assert!(hits[0].snippet.contains("Rust"));
        assert_eq!(hits[0].session.preview, "Rust ownership model");
        assert_eq!(hits[1].session.id, "s2");
    }

    #[test]
    fn save_session_with_messages_is_atomic_and_keeps_message_count_consistent() {
        let db = test_db();
        let mut session = sample_session("atomic");
        session.message_count = 999;
        let messages = vec![Message::user("hello"), Message::assistant("world")];

        db.save_session_with_messages(&session, &messages, 42.0)
            .expect("atomic save");

        let loaded = db.get_session("atomic").expect("get").expect("found");
        assert_eq!(loaded.message_count, 2);

        let loaded_messages = db.get_messages("atomic").expect("get messages");
        assert_eq!(loaded_messages.len(), 2);
        assert_eq!(loaded_messages[0].text_content(), "hello");
        assert_eq!(loaded_messages[1].text_content(), "world");
    }

    #[test]
    fn message_timestamps_preserve_per_message_created_at() {
        let db = test_db();
        let session = sample_session("ts-order");
        let mut m0 = Message::user("first");
        m0.created_at = Some(100.0);
        let mut m1 = Message::assistant("second");
        m1.created_at = Some(101.5);
        let mut m2 = Message::tool_result("c1", "write_file", r#"{"path":"x.html"}"#);
        m2.created_at = Some(102.25);
        db.save_session_with_messages(&session, &[m0, m1, m2], 999.0)
            .expect("save");
        let loaded = db.get_messages("ts-order").expect("load");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].created_at, Some(100.0));
        assert_eq!(loaded[1].created_at, Some(101.5));
        assert_eq!(loaded[2].created_at, Some(102.25));
        // Mid-turn re-save with a new wall clock must keep prior stamps.
        db.save_session_with_messages(&session, &loaded, 5000.0)
            .expect("re-save");
        let again = db.get_messages("ts-order").expect("reload");
        assert_eq!(again[0].created_at, Some(100.0));
        assert_eq!(again[2].created_at, Some(102.25));
    }

    #[test]
    fn rich_session_search_ignores_empty_query() {
        let db = test_db();
        db.save_session(&sample_session("s1")).expect("save");
        let hits = db
            .search_sessions_rich("   ", 10)
            .expect("empty search should not fail");
        assert!(hits.is_empty());
    }

    #[test]
    fn goals_persist_and_isolate_sessions() {
        let db = test_db();
        db.save_session(&sample_session("goal-a")).expect("save a");
        db.save_session(&sample_session("goal-b")).expect("save b");
        db.goals_set("goal-a", "Goal A", 20).expect("set a");
        db.goals_set("goal-b", "Goal B", 20).expect("set b");
        db.goals_push_subgoal("goal-a", "step 1").expect("push");
        assert_eq!(
            db.goals_active("goal-a")
                .expect("active a")
                .goal_text
                .as_deref(),
            Some("Goal A")
        );
        assert_eq!(
            db.goals_active("goal-a").expect("active a").subgoals.len(),
            1
        );
        db.goals_clear("goal-a").expect("clear");
        assert!(
            db.goals_active("goal-a")
                .expect("active a")
                .goal_text
                .is_none()
        );
        assert_eq!(
            db.goals_active("goal-b")
                .expect("active b")
                .goal_text
                .as_deref(),
            Some("Goal B")
        );
    }

    #[test]
    fn goals_complete_subgoal_marks_latest_undone() {
        let db = test_db();
        db.save_session(&sample_session("goal-s")).expect("save");
        db.goals_set("goal-s", "Ship", 20).expect("set");
        db.goals_push_subgoal("goal-s", "one").expect("push");
        db.goals_push_subgoal("goal-s", "two").expect("push");
        let done = db
            .goals_complete_subgoal("goal-s")
            .expect("done")
            .expect("marked");
        assert_eq!(done.text, "two");
        let state = db.goals_active("goal-s").expect("active");
        assert!(!state.subgoals[0].done);
        assert!(state.subgoals[1].done);
    }

    #[test]
    fn goals_contract_json_round_trips() {
        let db = test_db();
        db.save_session(&sample_session("goal-c")).expect("save");
        let contract = r#"{"verification":"cargo test --workspace","constraints":"keep API"}"#;
        db.goals_set_with_contract("goal-c", "Ship auth", 20, Some(contract))
            .expect("set with contract");
        let state = db.goals_active("goal-c").expect("active");
        assert_eq!(state.goal_text.as_deref(), Some("Ship auth"));
        let raw = state.contract_json.expect("contract_json");
        assert!(raw.contains("cargo test --workspace"));
        assert!(raw.contains("keep API"));
    }

    #[test]
    fn goals_set_requires_session_row_without_stub() {
        let db = test_db();
        let err = db
            .goals_set("orphan-session", "No parent row", 20)
            .expect_err("FK should fail without sessions row");
        assert!(
            err.to_string().contains("FOREIGN KEY") || err.to_string().contains("constraint"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ensure_session_row_allows_goals_before_first_chat() {
        let db = test_db();
        db.ensure_session_row("fresh-session", "cli", None, Some("mock/test"))
            .expect("stub session");
        db.goals_set("fresh-session", "Demo goal", 20)
            .expect("goal after stub");
        assert_eq!(
            db.goals_active("fresh-session")
                .expect("active")
                .goal_text
                .as_deref(),
            Some("Demo goal")
        );
    }

    #[test]
    fn goals_survive_save_session_with_messages() {
        let db = test_db();
        db.ensure_session_row("persist-goals", "cli", None, Some("mock/test"))
            .expect("stub");
        db.goals_set("persist-goals", "Keep me across saves", 20)
            .expect("set goal");
        let session = sample_session("persist-goals");
        let messages = vec![Message::user("hello"), Message::assistant("world")];
        db.save_session_with_messages(&session, &messages, 42.0)
            .expect("save turn");
        assert_eq!(
            db.goals_active("persist-goals")
                .expect("active")
                .goal_text
                .as_deref(),
            Some("Keep me across saves")
        );
    }

    #[test]
    fn fts5_empty_query() {
        let db = test_db();
        db.save_session(&sample_session("s1")).expect("save");
        db.save_message("s1", &Message::user("hello world"), 1.0)
            .expect("msg");

        // An empty FTS5 query is either an error or returns no results —
        // the caller (session_search tool) must avoid calling search("", _).
        // We only assert that it does NOT panic; panicking here would be a
        // regression because it previously crashed the owning thread.
        let result = db.search("", 10);
        // Acceptable outcomes: Ok([]) or Err(_) — never a panic.
        if let Ok(rows) = result {
            // SQLite FTS5 may return all rows for an empty query, or none —
            // either is fine. The critical invariant is no panic.
            let _ = rows.len();
        }
    }

    #[test]
    fn session_split() {
        let db = test_db();
        db.save_session(&sample_session("parent"))
            .expect("save parent");

        db.split_session("parent", "child", "cli", Some("mock/test"), 1720001000.0)
            .expect("split");

        let child = db.get_session("child").expect("get").expect("found");
        assert_eq!(child.parent_session_id.as_deref(), Some("parent"));

        let parent = db.get_session("parent").expect("get").expect("found");
        assert!(parent.ended_at.is_some());
        assert_eq!(parent.end_reason.as_deref(), Some("compression"));
    }

    #[test]
    fn nonexistent_session_returns_none() {
        let db = test_db();
        assert!(db.get_session("nope").expect("get").is_none());
    }

    #[test]
    fn escape_fts5_special_chars() {
        // Hyphens and special chars should be quoted to prevent FTS5 errors
        let escaped = SessionDb::escape_fts5_query("hello-world AND test");
        assert!(escaped.contains("\"hello-world\""));
        assert!(escaped.contains("\"AND\""));
    }

    /// Verify that `query_insights` returns sensible aggregate data when
    /// the database is seeded with known sessions and messages.
    #[test]
    fn query_insights_aggregates_sessions() {
        let db = test_db();

        // Seed two sessions with different models and token counts.
        let mut s1 = sample_session("i1");
        s1.source = "cli".to_string();
        s1.model = Some("anthropic/claude-3-5-sonnet".to_string());
        s1.message_count = 4;
        s1.tool_call_count = 2;
        s1.input_tokens = 1000;
        s1.output_tokens = 500;
        s1.title = Some("Insight session 1".to_string());
        s1.started_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time is after UNIX_EPOCH")
            .as_secs_f64()
            - 3600.0; // 1 hour ago
        db.save_session(&s1).expect("save s1");

        let mut s2 = sample_session("i2");
        s2.source = "telegram".to_string();
        s2.model = Some("openai/gpt-4o".to_string());
        s2.message_count = 2;
        s2.tool_call_count = 1;
        s2.input_tokens = 200;
        s2.output_tokens = 150;
        s2.title = Some("Insight session 2".to_string());
        s2.started_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time is after UNIX_EPOCH")
            .as_secs_f64()
            - 7200.0; // 2 hours ago
        db.save_session(&s2).expect("save s2");

        let report = db.query_insights(30).expect("insights");

        // Overview aggregates
        assert_eq!(report.days, 30);
        assert_eq!(
            report.overview.total_sessions, 2,
            "should count both sessions"
        );
        assert_eq!(report.overview.total_messages, 6, "4 + 2 messages");
        assert_eq!(report.overview.total_tool_calls, 3, "2 + 1 tool calls");
        assert_eq!(
            report.overview.total_input_tokens, 1200,
            "1000 + 200 input tokens"
        );
        assert_eq!(
            report.overview.total_output_tokens, 650,
            "500 + 150 output tokens"
        );

        // Per-model breakdown: should have two entries
        assert_eq!(report.models.len(), 2, "two distinct models");
        let model_names: Vec<&str> = report.models.iter().map(|m| m.model.as_str()).collect();
        assert!(
            model_names.contains(&"anthropic/claude-3-5-sonnet"),
            "claude model present"
        );
        assert!(
            model_names.contains(&"openai/gpt-4o"),
            "gpt-4o model present"
        );

        // Per-platform breakdown: cli and telegram
        assert_eq!(report.platforms.len(), 2, "two distinct sources");
        let sources: Vec<&str> = report.platforms.iter().map(|p| p.source.as_str()).collect();
        assert!(sources.contains(&"cli"), "cli source present");
        assert!(sources.contains(&"telegram"), "telegram source present");

        // Daily activity should have at least one entry (today)
        assert!(
            !report.daily_activity.is_empty(),
            "daily activity not empty"
        );
    }

    /// `query_insights` with zero sessions in range returns zeroed overview.
    #[test]
    fn query_insights_no_sessions_returns_zeroed_overview() {
        let db = test_db();
        let report = db.query_insights(30).expect("insights");
        assert_eq!(report.overview.total_sessions, 0);
        assert_eq!(report.overview.total_messages, 0);
        assert!(report.models.is_empty());
        assert!(report.platforms.is_empty());
    }

    #[test]
    fn session_handoff_state_machine() {
        let db = test_db();
        let session = sample_session("handoff-cli-session");
        db.save_session(&session).expect("save session");

        assert!(
            db.request_session_handoff(&session.id, "telegram")
                .expect("request")
        );
        let status = db
            .get_session_handoff_status(&session.id)
            .expect("status")
            .expect("some");
        assert_eq!(status.state, "pending");
        assert_eq!(status.platform.as_deref(), Some("telegram"));

        assert!(db.claim_session_handoff(&session.id).expect("claim"));
        db.complete_session_handoff(&session.id).expect("complete");
        let done = db
            .get_session_handoff_status(&session.id)
            .expect("status")
            .expect("some");
        assert_eq!(done.state, "completed");
    }

    #[test]
    fn migrate_v10_merges_handoffs_when_model_transfers_preexists() {
        let conn = Connection::open_in_memory().expect("in-memory");
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE sessions (
                 id TEXT PRIMARY KEY,
                 source TEXT NOT NULL,
                 started_at REAL NOT NULL
             );
             CREATE TABLE handoffs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id TEXT NOT NULL,
                 from_model TEXT NOT NULL,
                 to_model TEXT NOT NULL,
                 brief TEXT NOT NULL,
                 ts REAL NOT NULL
             );
             INSERT INTO sessions (id, source, started_at)
             VALUES ('sess-1', 'cli', 1.0);
             INSERT INTO handoffs (session_id, from_model, to_model, brief, ts)
             VALUES ('sess-1', 'a/m1', 'b/m2', 'legacy brief', 42.0);
             CREATE TABLE model_transfers (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id TEXT NOT NULL,
                 from_model TEXT NOT NULL,
                 to_model TEXT NOT NULL,
                 brief TEXT NOT NULL,
                 ts REAL NOT NULL
             );",
        )
        .expect("seed legacy");

        SessionDb::reconcile_model_transfers_table(&conn).expect("reconcile");

        let brief: String = conn
            .query_row(
                "SELECT brief FROM model_transfers WHERE session_id = 'sess-1'",
                [],
                |row| row.get(0),
            )
            .expect("row");
        assert_eq!(brief, "legacy brief");
        let handoffs_still_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='handoffs'",
                [],
                |row| row.get::<_, i64>(0).map(|count| count > 0),
            )
            .unwrap_or(false);
        assert!(!handoffs_still_exists);
    }

    #[test]
    fn record_and_list_model_transfers() {
        let db = test_db();
        let session = sample_session("handoff-session");
        db.save_session(&session).expect("save session");
        db.record_handoff(
            &session.id,
            "anthropic/claude-opus-4.6",
            "copilot/gpt-5-mini",
            "Implement session handoff feature.",
            1_700_000_000.0,
        )
        .expect("record handoff");
        let rows = db.list_handoffs(&session.id).expect("list handoffs");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].from_model, "anthropic/claude-opus-4.6");
        assert_eq!(rows[0].to_model, "copilot/gpt-5-mini");
        assert!(rows[0].brief.contains("session handoff"));
    }

    #[test]
    fn update_session_model_persists() {
        let db = test_db();
        let session = sample_session("model-update");
        db.save_session(&session).expect("save");
        db.update_session_model(&session.id, "copilot/gpt-5-mini")
            .expect("update model");
        let loaded = db.get_session(&session.id).expect("get").expect("row");
        assert_eq!(loaded.model.as_deref(), Some("copilot/gpt-5-mini"));
    }
}
