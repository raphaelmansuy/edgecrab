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
const SCHEMA_VERSION: u32 = 6;

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
                conn.execute(
                    "INSERT INTO schema_version (version) VALUES (?1)",
                    params![SCHEMA_VERSION],
                )
                .map_err(|e| AgentError::Database(e.to_string()))?;
            }
            Some(v) if v < SCHEMA_VERSION => {
                // Future: run migrations here
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

    // ── Session CRUD ──────────────────────────────────────────────────

    pub fn save_session(&self, session: &SessionRecord) -> Result<(), AgentError> {
        self.execute_write(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO sessions
                 (id, source, user_id, model, system_prompt, parent_session_id,
                  started_at, ended_at, end_reason, message_count, tool_call_count,
                  input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                  reasoning_tokens, estimated_cost_usd, title)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                params![
                    session.id,
                    session.source,
                    session.user_id,
                    session.model,
                    session.system_prompt,
                    session.parent_session_id,
                    session.started_at,
                    session.ended_at,
                    session.end_reason,
                    session.message_count,
                    session.tool_call_count,
                    session.input_tokens,
                    session.output_tokens,
                    session.cache_read_tokens,
                    session.cache_write_tokens,
                    session.reasoning_tokens,
                    session.estimated_cost_usd,
                    session.title,
                ],
            )?;
            Ok(())
        })
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
            conn.execute(
                "INSERT OR REPLACE INTO sessions
                 (id, source, user_id, model, system_prompt, parent_session_id,
                  started_at, ended_at, end_reason, message_count, tool_call_count,
                  input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                  reasoning_tokens, estimated_cost_usd, title)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                params![
                    session.id,
                    session.source,
                    session.user_id,
                    session.model,
                    session.system_prompt,
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
                    session.estimated_cost_usd,
                    session.title,
                ],
            )?;
            conn.execute(
                "DELETE FROM messages WHERE session_id = ?1",
                params![session.id],
            )?;
            for msg in messages {
                let tool_calls_json = msg
                    .tool_calls
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
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
                        timestamp,
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
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        started_at, ended_at, end_reason, message_count, tool_call_count,
                        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                        reasoning_tokens, estimated_cost_usd, title
                 FROM sessions WHERE id = ?1",
            )
            .map_err(|e| AgentError::Database(e.to_string()))?;

        let result = stmt
            .query_row(params![id], |row| {
                Ok(SessionRecord {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    user_id: row.get(2)?,
                    model: row.get(3)?,
                    system_prompt: row.get(4)?,
                    parent_session_id: row.get(5)?,
                    started_at: row.get(6)?,
                    ended_at: row.get(7)?,
                    end_reason: row.get(8)?,
                    message_count: row.get(9)?,
                    tool_call_count: row.get(10)?,
                    input_tokens: row.get(11)?,
                    output_tokens: row.get(12)?,
                    cache_read_tokens: row.get(13)?,
                    cache_write_tokens: row.get(14)?,
                    reasoning_tokens: row.get(15)?,
                    estimated_cost_usd: row.get(16)?,
                    title: row.get(17)?,
                })
            })
            .ok();

        Ok(result)
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
                "SELECT id, source, user_id, model, system_prompt, parent_session_id,
                        started_at, ended_at, end_reason, message_count, tool_call_count,
                        input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                        reasoning_tokens, estimated_cost_usd, title
                 FROM sessions WHERE title = ?1",
                params![title],
                |row| {
                    Ok(SessionRecord {
                        id: row.get(0)?,
                        source: row.get(1)?,
                        user_id: row.get(2)?,
                        model: row.get(3)?,
                        system_prompt: row.get(4)?,
                        parent_session_id: row.get(5)?,
                        started_at: row.get(6)?,
                        ended_at: row.get(7)?,
                        end_reason: row.get(8)?,
                        message_count: row.get(9)?,
                        tool_call_count: row.get(10)?,
                        input_tokens: row.get(11)?,
                        output_tokens: row.get(12)?,
                        cache_read_tokens: row.get(13)?,
                        cache_write_tokens: row.get(14)?,
                        reasoning_tokens: row.get(15)?,
                        estimated_cost_usd: row.get(16)?,
                        title: row.get(17)?,
                    })
                },
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
                "SELECT role, content, tool_call_id, tool_calls, finish_reason, reasoning, tool_name
                 FROM messages WHERE session_id = ?1 ORDER BY timestamp ASC",
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

                Ok((
                    role_str,
                    content,
                    tool_call_id,
                    tool_calls_json,
                    finish_reason,
                    reasoning,
                    tool_name,
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
            for msg in messages {
                let tool_calls_json = msg
                    .tool_calls
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
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
    fn rich_session_search_ignores_empty_query() {
        let db = test_db();
        db.save_session(&sample_session("s1")).expect("save");
        let hits = db
            .search_sessions_rich("   ", 10)
            .expect("empty search should not fail");
        assert!(hits.is_empty());
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
}
