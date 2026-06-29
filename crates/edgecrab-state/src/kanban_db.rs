//! SQLite-backed Kanban board — Hermes `kanban.db` parity (Phase 1–4).
//!
//! Durable task cards with claim leases, parent deps, task runs, and failure circuit.

use crate::kanban_board::{kanban_db_path_for_board, kanban_home};
use crate::kanban_rate_limit;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use edgecrab_types::AgentError;

const DEFAULT_CLAIM_TTL_SECS: i64 = 15 * 60;
/// Hermes `DEFAULT_FAILURE_LIMIT` — auto-block after N consecutive non-successes.
pub const DEFAULT_FAILURE_LIMIT: u32 = 2;

/// Task lifecycle states (Hermes subset for MVP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KanbanStatus {
    Triage,
    Todo,
    Doing,
    Done,
    Blocked,
    Archived,
}

impl KanbanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Triage => "triage",
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Archived => "archived",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "triage" => Some(Self::Triage),
            "todo" | "ready" => Some(Self::Todo),
            "doing" | "running" => Some(Self::Doing),
            "done" | "complete" | "completed" => Some(Self::Done),
            "blocked" | "block" => Some(Self::Blocked),
            "archived" | "archive" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// One child task in a decompose fan-out (Hermes decompose graph node).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanDecomposeChild {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    /// Indices into the sibling `children` slice — data deps within the graph.
    #[serde(default)]
    pub parents: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanComment {
    pub id: i64,
    pub task_id: String,
    pub author: String,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanRun {
    pub id: i64,
    pub task_id: String,
    pub status: String,
    pub worker_id: Option<String>,
    pub claim_expires: Option<i64>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub outcome: Option<String>,
    pub summary: Option<String>,
    pub error: Option<String>,
}

/// Parent task blocking promotion to dispatchable `todo`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentBlocker {
    pub id: String,
    pub title: String,
    pub status: String,
}

/// Kanban task lifecycle event (gateway notifier tails these).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanEvent {
    pub id: i64,
    pub task_id: String,
    pub kind: String,
    pub payload: Option<String>,
    pub created_at: i64,
}

/// Gateway chat subscription for terminal kanban notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanNotifySub {
    pub task_id: String,
    pub platform: String,
    pub chat_id: String,
    pub thread_id: String,
    pub user_id: Option<String>,
    pub created_at: i64,
    pub last_event_id: i64,
}

/// Terminal event kinds delivered by the gateway kanban notifier.
pub const KANBAN_NOTIFY_TERMINAL_KINDS: &[&str] = &[
    "completed",
    "blocked",
    "gave_up",
    "spawn_auto_blocked",
    "crashed",
    "timed_out",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanTask {
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub status: String,
    pub priority: i32,
    pub worker_id: Option<String>,
    pub claim_expires: Option<i64>,
    pub result: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub consecutive_failures: i32,
    #[serde(default)]
    pub last_failure_error: Option<String>,
    #[serde(default)]
    pub max_retries: Option<i32>,
    #[serde(default)]
    pub current_run_id: Option<i64>,
    #[serde(default)]
    pub max_runtime_seconds: Option<i32>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub scheduled_at: Option<i64>,
}

pub fn kanban_db_path(home: Option<&Path>) -> PathBuf {
    kanban_db_path_for_board(&kanban_home(home), None)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn kanban_conflict_error(message: &str, blockers: &[ParentBlocker]) -> AgentError {
    AgentError::Validation(format!(
        "KANBAN_CONFLICT:{}",
        serde_json::json!({ "error": message, "blockers": blockers })
    ))
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<KanbanTask> {
    Ok(KanbanTask {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        status: row.get(3)?,
        priority: row.get(4)?,
        worker_id: row.get(5)?,
        claim_expires: row.get(6)?,
        result: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        consecutive_failures: row.get(10).unwrap_or(0),
        last_failure_error: row.get(11).ok(),
        max_retries: row.get(12).ok(),
        current_run_id: row.get(13).ok(),
        max_runtime_seconds: row.get(14).ok(),
        assignee: row.get(15).ok(),
        scheduled_at: row.get(16).ok(),
    })
}

const SELECT_COLS: &str = "id, title, body, status, priority, worker_id, claim_expires, result, \
    created_at, updated_at, consecutive_failures, last_failure_error, max_retries, current_run_id, \
    max_runtime_seconds, assignee, scheduled_at";

pub struct KanbanDb {
    conn: Mutex<Connection>,
}

impl KanbanDb {
    pub fn open(path: &Path) -> Result<Arc<Self>, AgentError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(AgentError::Io)?;
        }
        let conn = Connection::open(path).map_err(|e| AgentError::Database(e.to_string()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| AgentError::Database(format!("kanban db pragma: {e}")))?;
        let db = Arc::new(Self {
            conn: Mutex::new(conn),
        });
        db.init_schema()?;
        db.migrate_schema()?;
        Ok(db)
    }

    pub fn open_default(home: Option<&Path>) -> Result<Arc<Self>, AgentError> {
        Self::open(&kanban_db_path(home))
    }

    fn init_schema(&self) -> Result<(), AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kanban_tasks (
                id            TEXT PRIMARY KEY,
                title         TEXT NOT NULL,
                body          TEXT,
                status        TEXT NOT NULL DEFAULT 'todo',
                priority      INTEGER NOT NULL DEFAULT 0,
                worker_id     TEXT,
                claim_expires INTEGER,
                result        TEXT,
                created_at    INTEGER NOT NULL,
                updated_at    INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_kanban_tasks_status ON kanban_tasks(status);
            CREATE INDEX IF NOT EXISTS idx_kanban_tasks_updated ON kanban_tasks(updated_at DESC);
            CREATE TABLE IF NOT EXISTS kanban_task_comments (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id    TEXT NOT NULL,
                author     TEXT NOT NULL,
                body       TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_kanban_comments_task
                ON kanban_task_comments(task_id, created_at);
            CREATE TABLE IF NOT EXISTS kanban_task_links (
                parent_id TEXT NOT NULL,
                child_id  TEXT NOT NULL,
                PRIMARY KEY (parent_id, child_id)
            );
            CREATE INDEX IF NOT EXISTS idx_kanban_links_child ON kanban_task_links(child_id);",
        )
        .map_err(|e| AgentError::Database(format!("kanban schema init: {e}")))?;
        Ok(())
    }

    fn migrate_schema(&self) -> Result<(), AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut cols = std::collections::HashSet::new();
        {
            let mut stmt = conn
                .prepare("PRAGMA table_info(kanban_tasks)")
                .map_err(|e| AgentError::Database(format!("kanban pragma: {e}")))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| AgentError::Database(format!("kanban pragma rows: {e}")))?;
            for row in rows {
                cols.insert(row.map_err(|e| AgentError::Database(format!("kanban pragma map: {e}")))?);
            }
        }
        let add_col = |name: &str, ddl: &str| -> Result<(), AgentError> {
            if !cols.contains(name) {
                conn.execute(ddl, [])
                    .map_err(|e| AgentError::Database(format!("kanban migrate {name}: {e}")))?;
            }
            Ok(())
        };
        add_col(
            "consecutive_failures",
            "ALTER TABLE kanban_tasks ADD COLUMN consecutive_failures INTEGER NOT NULL DEFAULT 0",
        )?;
        add_col(
            "last_failure_error",
            "ALTER TABLE kanban_tasks ADD COLUMN last_failure_error TEXT",
        )?;
        add_col("max_retries", "ALTER TABLE kanban_tasks ADD COLUMN max_retries INTEGER")?;
        add_col(
            "current_run_id",
            "ALTER TABLE kanban_tasks ADD COLUMN current_run_id INTEGER",
        )?;
        add_col(
            "max_runtime_seconds",
            "ALTER TABLE kanban_tasks ADD COLUMN max_runtime_seconds INTEGER",
        )?;
        add_col("assignee", "ALTER TABLE kanban_tasks ADD COLUMN assignee TEXT")?;
        add_col(
            "scheduled_at",
            "ALTER TABLE kanban_tasks ADD COLUMN scheduled_at INTEGER",
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_kanban_tasks_assignee_status
             ON kanban_tasks(assignee, status);",
        )
        .map_err(|e| AgentError::Database(format!("kanban migrate assignee index: {e}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kanban_task_runs (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id       TEXT NOT NULL,
                status        TEXT NOT NULL,
                worker_id     TEXT,
                claim_expires INTEGER,
                started_at    INTEGER NOT NULL,
                ended_at      INTEGER,
                outcome       TEXT,
                summary       TEXT,
                error         TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_kanban_runs_task
                ON kanban_task_runs(task_id, started_at);
            CREATE TABLE IF NOT EXISTS kanban_task_events (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id    TEXT NOT NULL,
                kind       TEXT NOT NULL,
                payload    TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_kanban_events_task
                ON kanban_task_events(task_id, created_at);
            CREATE TABLE IF NOT EXISTS kanban_notify_subs (
                task_id       TEXT NOT NULL,
                platform      TEXT NOT NULL,
                chat_id       TEXT NOT NULL,
                thread_id     TEXT NOT NULL DEFAULT '',
                user_id       TEXT,
                created_at    INTEGER NOT NULL,
                last_event_id INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (task_id, platform, chat_id, thread_id)
            );
            CREATE INDEX IF NOT EXISTS idx_kanban_notify_task
                ON kanban_notify_subs(task_id);",
        )
        .map_err(|e| AgentError::Database(format!("kanban migrate tables: {e}")))?;
        Ok(())
    }

    fn append_event(&self, task_id: &str, kind: &str, payload: Option<&str>) -> Result<(), AgentError> {
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "INSERT INTO kanban_task_events (task_id, kind, payload, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![task_id, kind, payload, now],
        )
        .map_err(|e| AgentError::Database(format!("kanban event: {e}")))?;
        Ok(())
    }

    /// Fetch lifecycle events with `id > since_id` (dashboard tail / REST poll).
    pub fn list_events_after(
        &self,
        since_id: i64,
        limit: usize,
    ) -> Result<(i64, Vec<KanbanEvent>), AgentError> {
        let limit = limit.clamp(1, 500);
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, kind, payload, created_at FROM kanban_task_events
                 WHERE id > ?1 ORDER BY id ASC LIMIT ?2",
            )
            .map_err(|e| AgentError::Database(format!("kanban events prepare: {e}")))?;
        let rows = stmt
            .query_map(params![since_id, limit as i64], |row| {
                Ok(KanbanEvent {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    kind: row.get(2)?,
                    payload: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("kanban events query: {e}")))?;
        let mut out = Vec::new();
        let mut cursor = since_id;
        for row in rows {
            let ev = row.map_err(|e| AgentError::Database(format!("kanban events map: {e}")))?;
            cursor = cursor.max(ev.id);
            out.push(ev);
        }
        Ok((cursor, out))
    }

    fn has_sticky_block(&self, task_id: &str) -> Result<bool, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let kind: Option<String> = conn
            .query_row(
                "SELECT kind FROM kanban_task_events
                 WHERE task_id = ?1 AND kind IN ('blocked', 'unblocked')
                 ORDER BY id DESC LIMIT 1",
                params![task_id],
                |row| row.get(0),
            )
            .ok();
        Ok(kind.as_deref() == Some("blocked"))
    }

    fn effective_failure_limit(&self, task: &KanbanTask, failure_limit: u32) -> u32 {
        task.max_retries
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(failure_limit)
    }

    fn start_run(
        &self,
        task_id: &str,
        worker_id: &str,
        claim_expires: i64,
    ) -> Result<i64, AgentError> {
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "INSERT INTO kanban_task_runs (task_id, status, worker_id, claim_expires, started_at)
             VALUES (?1, 'doing', ?2, ?3, ?4)",
            params![task_id, worker_id, claim_expires, now],
        )
        .map_err(|e| AgentError::Database(format!("kanban run start: {e}")))?;
        let run_id = conn.last_insert_rowid();
        conn.execute(
            "UPDATE kanban_tasks SET current_run_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![run_id, now, task_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban run pointer: {e}")))?;
        Ok(run_id)
    }

    fn end_run(
        &self,
        run_id: i64,
        status: &str,
        outcome: Option<&str>,
        summary: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), AgentError> {
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "UPDATE kanban_task_runs SET status = ?1, outcome = ?2, summary = ?3,
             error = ?4, ended_at = ?5, claim_expires = NULL
             WHERE id = ?6 AND ended_at IS NULL",
            params![status, outcome, summary, error, now, run_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban run end: {e}")))?;
        Ok(())
    }

    fn close_current_run(
        &self,
        task_id: &str,
        status: &str,
        outcome: Option<&str>,
        summary: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), AgentError> {
        if let Some(task) = self.get_task(task_id)?
            && let Some(run_id) = task.current_run_id
        {
            self.end_run(run_id, status, outcome, summary, error)?;
        }
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "UPDATE kanban_tasks SET current_run_id = NULL, updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban clear run: {e}")))?;
        Ok(())
    }

    /// Record a non-success and maybe trip the failure circuit breaker.
    pub fn record_task_failure(
        &self,
        task_id: &str,
        error: &str,
        failure_limit: u32,
    ) -> Result<bool, AgentError> {
        let task = self
            .get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("unknown task {task_id}")))?;
        let limit = self.effective_failure_limit(&task, failure_limit);
        let failures = task.consecutive_failures + 1;
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let tripped = failures >= limit as i32;
        if tripped {
            conn.execute(
                "UPDATE kanban_tasks SET status = 'blocked', worker_id = NULL,
                 claim_expires = NULL, consecutive_failures = ?1, last_failure_error = ?2,
                 updated_at = ?3 WHERE id = ?4",
                params![failures, error, now, task_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban failure block: {e}")))?;
            drop(conn);
            let payload = serde_json::json!({ "error": error }).to_string();
            self.append_event(task_id, "gave_up", Some(&payload))?;
        } else {
            conn.execute(
                "UPDATE kanban_tasks SET status = 'todo', worker_id = NULL,
                 claim_expires = NULL, consecutive_failures = ?1, last_failure_error = ?2,
                 updated_at = ?3 WHERE id = ?4",
                params![failures, error, now, task_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban failure release: {e}")))?;
        }
        Ok(tripped)
    }

    /// Promote todo/blocked tasks when parent deps are satisfied (Hermes `recompute_ready` subset).
    pub fn recompute_ready(&self, failure_limit: u32) -> Result<usize, AgentError> {
        let candidates = self.list_tasks(None, 500)?;
        let mut promoted = 0usize;
        for task in candidates {
            if !matches!(task.status.as_str(), "todo" | "blocked") {
                continue;
            }
            if task.status == "blocked" && self.has_sticky_block(&task.id)? {
                continue;
            }
            let parents = self.parent_ids(&task.id)?;
            let parents_done = parents.iter().all(|pid| {
                self.get_task(pid)
                    .ok()
                    .flatten()
                    .is_some_and(|p| p.status == "done" || p.status == "archived")
            });
            if !parents_done {
                continue;
            }
            if task.status == "blocked" {
                let limit = self.effective_failure_limit(&task, failure_limit);
                if task.consecutive_failures >= limit as i32 {
                    continue;
                }
            }
            let now = now_secs();
            let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
            let updated = conn
                .execute(
                    "UPDATE kanban_tasks SET status = 'todo', updated_at = ?1
                     WHERE id = ?2 AND status IN ('todo', 'blocked')",
                    params![now, task.id],
                )
                .map_err(|e| AgentError::Database(format!("kanban promote: {e}")))?;
            if updated > 0 {
                promoted += 1;
                drop(conn);
                self.append_event(&task.id, "promoted", None)?;
            }
        }
        Ok(promoted)
    }

    pub fn list_task_runs(&self, task_id: &str, limit: usize) -> Result<Vec<KanbanRun>, AgentError> {
        let limit = limit.clamp(1, 100);
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, status, worker_id, claim_expires, started_at, ended_at,
                        outcome, summary, error
                 FROM kanban_task_runs WHERE task_id = ?1
                 ORDER BY started_at DESC LIMIT ?2",
            )
            .map_err(|e| AgentError::Database(format!("kanban runs prepare: {e}")))?;
        let rows = stmt
            .query_map(params![task_id, limit as i64], |row| {
                Ok(KanbanRun {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    status: row.get(2)?,
                    worker_id: row.get(3)?,
                    claim_expires: row.get(4)?,
                    started_at: row.get(5)?,
                    ended_at: row.get(6)?,
                    outcome: row.get(7)?,
                    summary: row.get(8)?,
                    error: row.get(9)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("kanban runs query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(format!("kanban runs map: {e}")))?);
        }
        Ok(out)
    }

    /// Gateway callback when a worker chat fails without completing the task.
    pub fn handle_worker_failure(
        &self,
        task_id: &str,
        worker_id: &str,
        error: &str,
        failure_limit: u32,
    ) -> Result<bool, AgentError> {
        if kanban_rate_limit::is_rate_limit_error(error) {
            self.requeue_rate_limited(task_id, worker_id, error)?;
            return Ok(false);
        }
        let _ = self.close_current_run(task_id, "failed", Some("failed"), None, Some(error));
        let _ = self.release_task(task_id, Some(worker_id));
        self.record_task_failure(task_id, error, failure_limit)
    }

    /// Requeue after quota wall without tripping failure counter (Hermes rate-limit path).
    pub fn requeue_rate_limited(
        &self,
        task_id: &str,
        worker_id: &str,
        error: &str,
    ) -> Result<(), AgentError> {
        self.close_current_run(task_id, "rate_limited", Some("rate_limited"), None, Some(error))?;
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "UPDATE kanban_tasks SET status = 'todo', worker_id = NULL, claim_expires = NULL,
             last_failure_error = ?1, updated_at = ?2 WHERE id = ?3",
            params![error, now, task_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban rate limit requeue: {e}")))?;
        drop(conn);
        let payload = serde_json::json!({ "error": error }).to_string();
        self.append_event(task_id, "rate_limited", Some(&payload))?;
        let _ = worker_id;
        Ok(())
    }

    /// Most recent ended run for respawn guard checks.
    pub fn latest_ended_run(&self, task_id: &str) -> Result<Option<KanbanRun>, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, status, worker_id, claim_expires, started_at, ended_at,
                        outcome, summary, error
                 FROM kanban_task_runs WHERE task_id = ?1 AND ended_at IS NOT NULL
                 ORDER BY ended_at DESC LIMIT 1",
            )
            .map_err(|e| AgentError::Database(format!("kanban latest run prepare: {e}")))?;
        let mut rows = stmt
            .query_map(params![task_id], |row| {
                Ok(KanbanRun {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    status: row.get(2)?,
                    worker_id: row.get(3)?,
                    claim_expires: row.get(4)?,
                    started_at: row.get(5)?,
                    ended_at: row.get(6)?,
                    outcome: row.get(7)?,
                    summary: row.get(8)?,
                    error: row.get(9)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("kanban latest run query: {e}")))?;
        if let Some(row) = rows.next() {
            return row
                .map(Some)
                .map_err(|e| AgentError::Database(format!("kanban latest run map: {e}")));
        }
        Ok(None)
    }

    /// Set or clear future dispatch time (Unix seconds). `None` clears the gate.
    pub fn set_scheduled_at(&self, task_id: &str, at: Option<i64>) -> Result<(), AgentError> {
        if self.get_task(task_id)?.is_none() {
            return Err(AgentError::Validation(format!("task '{task_id}' not found")));
        }
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "UPDATE kanban_tasks SET scheduled_at = ?1, updated_at = ?2 WHERE id = ?3",
            params![at, now, task_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban scheduled_at: {e}")))?;
        drop(conn);
        let payload = at.map(|ts| serde_json::json!({ "scheduled_at": ts }).to_string());
        self.append_event(task_id, "scheduled", payload.as_deref())?;
        Ok(())
    }

    pub fn create_task(
        &self,
        title: &str,
        body: Option<&str>,
        priority: i32,
    ) -> Result<KanbanTask, AgentError> {
        self.create_task_with_runtime(title, body, priority, None)
    }

    pub fn create_task_with_runtime(
        &self,
        title: &str,
        body: Option<&str>,
        priority: i32,
        max_runtime_seconds: Option<i32>,
    ) -> Result<KanbanTask, AgentError> {
        self.create_task_full(title, body, priority, max_runtime_seconds, None)
    }

    pub fn create_task_with_assignee(
        &self,
        title: &str,
        body: Option<&str>,
        priority: i32,
        max_runtime_seconds: Option<i32>,
        assignee: Option<&str>,
    ) -> Result<KanbanTask, AgentError> {
        self.create_task_full(title, body, priority, max_runtime_seconds, assignee)
    }

    fn create_task_full(
        &self,
        title: &str,
        body: Option<&str>,
        priority: i32,
        max_runtime_seconds: Option<i32>,
        assignee: Option<&str>,
    ) -> Result<KanbanTask, AgentError> {
        let id = format!("kb-{}", &Uuid::new_v4().simple().to_string()[..12]);
        let now = now_secs();
        let assignee = assignee.map(str::trim).filter(|s| !s.is_empty());
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "INSERT INTO kanban_tasks (id, title, body, status, priority, max_runtime_seconds, assignee, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'todo', ?4, ?5, ?6, ?7, ?7)",
            params![id, title.trim(), body, priority, max_runtime_seconds, assignee, now],
        )
        .map_err(|e| AgentError::Database(format!("kanban create: {e}")))?;
        drop(conn);
        self.get_task(&id)?
            .ok_or_else(|| AgentError::Validation("kanban create: row missing after insert".into()))
    }

    /// Create a task in the triage column (Hermes parking column for decomposer input).
    pub fn create_triage_task(
        &self,
        title: &str,
        body: Option<&str>,
        priority: i32,
    ) -> Result<KanbanTask, AgentError> {
        self.create_triage_task_with_assignee(title, body, priority, None)
    }

    pub fn create_triage_task_with_assignee(
        &self,
        title: &str,
        body: Option<&str>,
        priority: i32,
        assignee: Option<&str>,
    ) -> Result<KanbanTask, AgentError> {
        let id = format!("kb-{}", &Uuid::new_v4().simple().to_string()[..12]);
        let now = now_secs();
        let assignee = assignee.map(str::trim).filter(|s| !s.is_empty());
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "INSERT INTO kanban_tasks (id, title, body, status, priority, assignee, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'triage', ?4, ?5, ?6, ?6)",
            params![id, title.trim(), body, priority, assignee, now],
        )
        .map_err(|e| AgentError::Database(format!("kanban triage create: {e}")))?;
        drop(conn);
        self.append_event(&id, "created", Some(r#"{"column":"triage"}"#))?;
        self.get_task(&id)?
            .ok_or_else(|| AgentError::Validation("kanban triage create: row missing".into()))
    }

    /// Promote a triage task to todo with a tightened spec (Hermes specify / fanout=false).
    pub fn specify_triage_task(
        &self,
        task_id: &str,
        title: &str,
        body: Option<&str>,
        assignee: Option<&str>,
        author: Option<&str>,
    ) -> Result<bool, AgentError> {
        let Some(root) = self.get_task(task_id)? else {
            return Ok(false);
        };
        if root.status != "triage" {
            return Ok(false);
        }
        let assignee_val = if root.assignee.is_some() {
            None
        } else {
            assignee.map(str::trim).filter(|s| !s.is_empty())
        };
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = if let Some(a) = assignee_val {
            conn.execute(
                "UPDATE kanban_tasks SET title = ?1, body = ?2, status = 'todo', assignee = ?3, updated_at = ?4
                 WHERE id = ?5 AND status = 'triage'",
                params![title.trim(), body, a, now, task_id],
            )
        } else {
            conn.execute(
                "UPDATE kanban_tasks SET title = ?1, body = ?2, status = 'todo', updated_at = ?3
                 WHERE id = ?4 AND status = 'triage'",
                params![title.trim(), body, now, task_id],
            )
        }
        .map_err(|e| AgentError::Database(format!("kanban specify: {e}")))?;
        drop(conn);
        if updated == 0 {
            return Ok(false);
        }
        if let Some(author) = author.filter(|a| !a.trim().is_empty()) {
            let _ = self.add_comment(task_id, author, "Specified — promoted triage → todo");
        }
        self.append_event(task_id, "specified", None)?;
        let _ = self.recompute_ready(DEFAULT_FAILURE_LIMIT);
        Ok(true)
    }

    /// Apply `kanban.default_assignee` before dispatch (Hermes auto-assign).
    pub fn apply_default_assignee(&self, task_id: &str, assignee: &str) -> Result<bool, AgentError> {
        let assignee = assignee.trim();
        if assignee.is_empty() {
            return Ok(false);
        }
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = conn
            .execute(
                "UPDATE kanban_tasks SET assignee = ?1, updated_at = ?2
                 WHERE id = ?3 AND (assignee IS NULL OR assignee = '')",
                params![assignee, now, task_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban default assignee: {e}")))?;
        drop(conn);
        if updated > 0 {
            let payload = serde_json::json!({
                "assignee": assignee,
                "source": "kanban.default_assignee",
            })
            .to_string();
            self.append_event(task_id, "assigned", Some(&payload))?;
        }
        Ok(updated > 0)
    }

    fn validate_decompose_children(children: &[KanbanDecomposeChild]) -> Result<(), AgentError> {
        if children.is_empty() {
            return Err(AgentError::Validation("decompose requires at least one child".into()));
        }
        for (idx, child) in children.iter().enumerate() {
            if child.title.trim().is_empty() {
                return Err(AgentError::Validation(format!("child[{idx}].title is required")));
            }
            for p in &child.parents {
                if *p >= children.len() {
                    return Err(AgentError::Validation(format!(
                        "child[{idx}].parents contains invalid index {p}"
                    )));
                }
                if *p == idx {
                    return Err(AgentError::Validation(format!(
                        "child[{idx}] cannot list itself as a parent"
                    )));
                }
            }
        }
        let mut in_deg = vec![0usize; children.len()];
        let mut adj = vec![Vec::new(); children.len()];
        for (idx, child) in children.iter().enumerate() {
            for p in &child.parents {
                adj[*p].push(idx);
                in_deg[idx] += 1;
            }
        }
        let mut queue: Vec<usize> = in_deg
            .iter()
            .enumerate()
            .filter_map(|(i, &d)| (d == 0).then_some(i))
            .collect();
        let mut seen = 0usize;
        while let Some(node) = queue.pop() {
            seen += 1;
            for nb in &adj[node] {
                in_deg[*nb] -= 1;
                if in_deg[*nb] == 0 {
                    queue.push(*nb);
                }
            }
        }
        if seen != children.len() {
            return Err(AgentError::Validation(
                "cyclic dependency detected in decomposed children list".into(),
            ));
        }
        Ok(())
    }

    /// Fan a triage task into child tasks; root stays alive until all children complete.
    pub fn decompose_triage_task(
        &self,
        task_id: &str,
        root_assignee: Option<&str>,
        children: &[KanbanDecomposeChild],
        author: Option<&str>,
    ) -> Result<Option<Vec<String>>, AgentError> {
        Self::validate_decompose_children(children)?;
        let Some(root) = self.get_task(task_id)? else {
            return Ok(None);
        };
        if root.status != "triage" {
            return Ok(None);
        }

        let now = now_secs();
        let author = author.unwrap_or("decomposer");
        let mut child_ids = Vec::with_capacity(children.len());

        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| AgentError::Database(format!("kanban decompose txn: {e}")))?;

        for child in children {
            let new_id = format!("kb-{}", &Uuid::new_v4().simple().to_string()[..12]);
            tx.execute(
                "INSERT INTO kanban_tasks (id, title, body, status, priority, assignee, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'todo', 0, ?4, ?5, ?5)",
                params![
                    new_id,
                    child.title.trim(),
                    child.body.as_deref(),
                    child.assignee.as_deref(),
                    now
                ],
            )
            .map_err(|e| AgentError::Database(format!("kanban decompose child: {e}")))?;
            child_ids.push(new_id);
        }

        for (idx, child) in children.iter().enumerate() {
            for p in &child.parents {
                tx.execute(
                    "INSERT OR IGNORE INTO kanban_task_links (parent_id, child_id) VALUES (?1, ?2)",
                    params![child_ids[*p], child_ids[idx]],
                )
                .map_err(|e| AgentError::Database(format!("kanban decompose link: {e}")))?;
            }
        }

        for cid in &child_ids {
            tx.execute(
                "INSERT OR IGNORE INTO kanban_task_links (parent_id, child_id) VALUES (?1, ?2)",
                params![cid, task_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban decompose root link: {e}")))?;
        }

        let root_assignee = root_assignee.map(str::trim).filter(|s| !s.is_empty());
        let updated = if let Some(a) = root_assignee {
            tx.execute(
                "UPDATE kanban_tasks SET status = 'todo', assignee = ?1, updated_at = ?2
                 WHERE id = ?3 AND status = 'triage'",
                params![a, now, task_id],
            )
        } else {
            tx.execute(
                "UPDATE kanban_tasks SET status = 'todo', updated_at = ?1
                 WHERE id = ?2 AND status = 'triage'",
                params![now, task_id],
            )
        }
        .map_err(|e| AgentError::Database(format!("kanban decompose root: {e}")))?;
        if updated == 0 {
            return Ok(None);
        }

        tx.commit()
            .map_err(|e| AgentError::Database(format!("kanban decompose commit: {e}")))?;
        drop(conn);

        let payload = serde_json::json!({ "child_ids": child_ids }).to_string();
        self.append_event(task_id, "decomposed", Some(&payload))?;
        let comment = format!(
            "Decomposed into {}. Root will wake when all children complete.",
            child_ids.join(", ")
        );
        let _ = self.add_comment(task_id, author, &comment);
        let _ = self.recompute_ready(DEFAULT_FAILURE_LIMIT);
        Ok(Some(child_ids))
    }

    pub fn get_task(&self, id: &str) -> Result<Option<KanbanTask>, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let sql = format!("SELECT {SELECT_COLS} FROM kanban_tasks WHERE id = ?1");
        let mut stmt = conn.prepare(&sql).map_err(|e| AgentError::Database(format!("kanban get prepare: {e}")))?;
        let mut rows = stmt.query(params![id]).map_err(|e| AgentError::Database(format!("kanban get query: {e}")))?;
        if let Some(row) = rows.next().map_err(|e| AgentError::Database(format!("kanban get row: {e}")))? {
            return map_row(row).map(Some).map_err(|e| AgentError::Database(format!("kanban get map: {e}")));
        }
        Ok(None)
    }

    pub fn list_tasks(
        &self,
        status: Option<KanbanStatus>,
        limit: usize,
    ) -> Result<Vec<KanbanTask>, AgentError> {
        let limit = limit.clamp(1, 200);
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let (sql, status_param) = match status {
            Some(s) => (
                format!(
                    "SELECT {SELECT_COLS} FROM kanban_tasks WHERE status = ?1
                     ORDER BY priority DESC, updated_at DESC LIMIT ?2"
                ),
                Some(s.as_str().to_string()),
            ),
            None => (
                format!(
                    "SELECT {SELECT_COLS} FROM kanban_tasks
                     ORDER BY priority DESC, updated_at DESC LIMIT ?1"
                ),
                None,
            ),
        };
        let mut stmt = conn.prepare(&sql).map_err(|e| AgentError::Database(format!("kanban list prepare: {e}")))?;
        let rows = if let Some(st) = status_param {
            stmt.query_map(params![st, limit as i64], map_row)
        } else {
            stmt.query_map(params![limit as i64], map_row)
        }
        .map_err(|e| AgentError::Database(format!("kanban list query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(format!("kanban list map: {e}")))?);
        }
        Ok(out)
    }

    pub fn claim_task(
        &self,
        task_id: &str,
        worker_id: &str,
        ttl_secs: i64,
    ) -> Result<KanbanTask, AgentError> {
        let now = now_secs();
        let expires = now + ttl_secs.max(1);
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = conn
            .execute(
                "UPDATE kanban_tasks SET status = 'doing', worker_id = ?1,
                 claim_expires = ?2, updated_at = ?3
                 WHERE id = ?4 AND status = 'todo'
                 AND (scheduled_at IS NULL OR scheduled_at <= ?3)
                 AND (worker_id IS NULL OR claim_expires IS NULL OR claim_expires < ?3)
                 AND NOT EXISTS (
                     SELECT 1 FROM kanban_task_links l
                     INNER JOIN kanban_tasks p ON p.id = l.parent_id
                     WHERE l.child_id = ?4 AND p.status NOT IN ('done', 'archived')
                 )",
                params![worker_id, expires, now, task_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban claim: {e}")))?;
        if updated == 0 {
            return Err(AgentError::Validation(format!(
                "task '{task_id}' is not claimable (wrong status, deps pending, or held by another worker)"
            )));
        }
        drop(conn);
        self.start_run(task_id, worker_id, expires)?;
        self.get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' missing after claim")))
    }

    pub fn release_task(&self, task_id: &str, worker_id: Option<&str>) -> Result<KanbanTask, AgentError> {
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = if let Some(w) = worker_id {
            conn.execute(
                "UPDATE kanban_tasks SET status = 'todo', worker_id = NULL,
                 claim_expires = NULL, updated_at = ?1
                 WHERE id = ?2 AND worker_id = ?3",
                params![now, task_id, w],
            )
        } else {
            conn.execute(
                "UPDATE kanban_tasks SET status = 'todo', worker_id = NULL,
                 claim_expires = NULL, updated_at = ?1
                 WHERE id = ?2",
                params![now, task_id],
            )
        }
        .map_err(|e| AgentError::Database(format!("kanban release: {e}")))?;
        if updated == 0 {
            return Err(AgentError::Validation(format!(
                "task '{task_id}' not released (not found or worker mismatch)"
            )));
        }
        drop(conn);
        self.close_current_run(task_id, "released", Some("released"), None, None)?;
        self.get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' missing after release")))
    }

    pub fn complete_task(
        &self,
        task_id: &str,
        worker_id: Option<&str>,
        result: Option<&str>,
    ) -> Result<KanbanTask, AgentError> {
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = if let Some(w) = worker_id {
            conn.execute(
                "UPDATE kanban_tasks SET status = 'done', result = ?1,
                 worker_id = NULL, claim_expires = NULL, updated_at = ?2
                 WHERE id = ?3 AND (worker_id = ?4 OR worker_id IS NULL)",
                params![result, now, task_id, w],
            )
        } else {
            conn.execute(
                "UPDATE kanban_tasks SET status = 'done', result = ?1,
                 worker_id = NULL, claim_expires = NULL, updated_at = ?2
                 WHERE id = ?3",
                params![result, now, task_id],
            )
        }
        .map_err(|e| AgentError::Database(format!("kanban complete: {e}")))?;
        if updated == 0 {
            return Err(AgentError::Validation(format!(
                "task '{task_id}' not completed (not found or worker mismatch)"
            )));
        }
        drop(conn);
        self.close_current_run(
            task_id,
            "done",
            Some("completed"),
            result,
            None,
        )?;
        let completed_payload = result.map(|r| {
            let first_line = r.lines().next().unwrap_or(r);
            let summary: String = first_line.chars().take(400).collect();
            serde_json::json!({
                "summary": summary,
                "result_len": r.len(),
            })
            .to_string()
        });
        self.append_event(
            task_id,
            "completed",
            completed_payload.as_deref(),
        )?;
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "UPDATE kanban_tasks SET consecutive_failures = 0, last_failure_error = NULL,
             updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban complete reset failures: {e}")))?;
        drop(conn);
        let _ = self.recompute_ready(DEFAULT_FAILURE_LIMIT);
        self.get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' missing after complete")))
    }

    pub fn heartbeat_task(&self, task_id: &str, worker_id: &str, ttl_secs: i64) -> Result<(), AgentError> {
        let now = now_secs();
        let expires = now + ttl_secs.max(1);
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = conn
            .execute(
                "UPDATE kanban_tasks SET claim_expires = ?1, updated_at = ?2
                 WHERE id = ?3 AND worker_id = ?4 AND status = 'doing'",
                params![expires, now, task_id, worker_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban heartbeat: {e}")))?;
        if updated == 0 {
            return Err(AgentError::Validation(format!(
                "heartbeat failed for task '{task_id}' (not held by worker)"
            )));
        }
        Ok(())
    }

    pub fn parent_ids(&self, task_id: &str) -> Result<Vec<String>, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut stmt = conn
            .prepare("SELECT parent_id FROM kanban_task_links WHERE child_id = ?1 ORDER BY parent_id")
            .map_err(|e| AgentError::Database(format!("kanban parents prepare: {e}")))?;
        let rows = stmt
            .query_map(params![task_id], |row| row.get::<_, String>(0))
            .map_err(|e| AgentError::Database(format!("kanban parents query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(format!("kanban parents map: {e}")))?);
        }
        Ok(out)
    }

    /// Parents that prevent promoting a task to dispatchable `todo`.
    pub fn parents_blocking_todo(&self, task_id: &str) -> Result<Vec<ParentBlocker>, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.title, t.status FROM kanban_tasks t
                 INNER JOIN kanban_task_links l ON l.parent_id = t.id
                 WHERE l.child_id = ?1 AND t.status NOT IN ('done', 'archived')
                 ORDER BY t.id",
            )
            .map_err(|e| AgentError::Database(format!("kanban parent blockers prepare: {e}")))?;
        let rows = stmt
            .query_map(params![task_id], |row| {
                Ok(ParentBlocker {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    status: row.get(2)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("kanban parent blockers query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(format!("kanban parent blockers map: {e}")))?);
        }
        Ok(out)
    }

    pub fn child_ids(&self, task_id: &str) -> Result<Vec<String>, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut stmt = conn
            .prepare("SELECT child_id FROM kanban_task_links WHERE parent_id = ?1 ORDER BY child_id")
            .map_err(|e| AgentError::Database(format!("kanban children prepare: {e}")))?;
        let rows = stmt
            .query_map(params![task_id], |row| row.get::<_, String>(0))
            .map_err(|e| AgentError::Database(format!("kanban children query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(format!("kanban children map: {e}")))?);
        }
        Ok(out)
    }

    fn would_cycle(&self, parent_id: &str, child_id: &str) -> Result<bool, AgentError> {
        if parent_id == child_id {
            return Ok(true);
        }
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![child_id.to_string()];
        while let Some(node) = stack.pop() {
            if node == parent_id {
                return Ok(true);
            }
            if !seen.insert(node.clone()) {
                continue;
            }
            stack.extend(self.child_ids(&node)?);
        }
        Ok(false)
    }

    pub fn link_tasks(&self, parent_id: &str, child_id: &str) -> Result<(), AgentError> {
        if self.get_task(parent_id)?.is_none() || self.get_task(child_id)?.is_none() {
            return Err(AgentError::Validation(format!(
                "unknown task in link {parent_id} -> {child_id}"
            )));
        }
        if self.would_cycle(parent_id, child_id)? {
            return Err(AgentError::Validation(format!(
                "linking {parent_id} -> {child_id} would create a cycle"
            )));
        }
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "INSERT OR IGNORE INTO kanban_task_links (parent_id, child_id) VALUES (?1, ?2)",
            params![parent_id, child_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban link: {e}")))?;
        Ok(())
    }

    /// Todo tasks whose parent dependencies are all `done`/`archived` and schedule has elapsed.
    pub fn list_claimable_tasks(&self, limit: usize) -> Result<Vec<KanbanTask>, AgentError> {
        let limit = limit.clamp(1, 200);
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let sql = format!(
            "SELECT {SELECT_COLS} FROM kanban_tasks t
             WHERE t.status = 'todo'
             AND (t.scheduled_at IS NULL OR t.scheduled_at <= ?1)
             AND NOT EXISTS (
                 SELECT 1 FROM kanban_task_links l
                 INNER JOIN kanban_tasks p ON p.id = l.parent_id
                 WHERE l.child_id = t.id AND p.status NOT IN ('done', 'archived')
             )
             ORDER BY t.priority DESC, t.updated_at ASC
             LIMIT ?2"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AgentError::Database(format!("kanban claimable prepare: {e}")))?;
        let rows = stmt
            .query_map(params![now, limit as i64], map_row)
            .map_err(|e| AgentError::Database(format!("kanban claimable query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(format!("kanban claimable map: {e}")))?);
        }
        Ok(out)
    }

    pub fn count_doing_tasks(&self) -> Result<usize, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM kanban_tasks WHERE status = 'doing'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AgentError::Database(format!("kanban doing count: {e}")))?;
        Ok(n as usize)
    }

    /// Count in-flight tasks per assignee profile (Hermes per-profile cap).
    pub fn count_doing_by_assignee(&self) -> Result<std::collections::HashMap<String, usize>, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT assignee, COUNT(*) FROM kanban_tasks
                 WHERE status = 'doing' AND assignee IS NOT NULL
                 GROUP BY assignee",
            )
            .map_err(|e| AgentError::Database(format!("kanban doing-by-assignee prepare: {e}")))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize)))
            .map_err(|e| AgentError::Database(format!("kanban doing-by-assignee query: {e}")))?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (assignee, n) = row.map_err(|e| AgentError::Database(format!("kanban doing-by-assignee map: {e}")))?;
            out.insert(assignee, n);
        }
        Ok(out)
    }

    /// Archive a task — terminal state; unblocks dependents like `done`.
    pub fn archive_task(&self, task_id: &str) -> Result<KanbanTask, AgentError> {
        let task = self
            .get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' not found")))?;
        if task.status == "archived" {
            return Ok(task);
        }
        if task.status == "doing" {
            self.close_current_run(
                task_id,
                "reclaimed",
                Some("reclaimed"),
                Some("task archived with run still active"),
                None,
            )?;
        }
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = conn
            .execute(
                "UPDATE kanban_tasks SET status = 'archived', worker_id = NULL,
                 claim_expires = NULL, updated_at = ?1
                 WHERE id = ?2 AND status != 'archived'",
                params![now, task_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban archive: {e}")))?;
        if updated == 0 {
            return Err(AgentError::Validation(format!("task '{task_id}' not archived")));
        }
        drop(conn);
        self.append_event(task_id, "archived", None)?;
        let _ = self.recompute_ready(DEFAULT_FAILURE_LIMIT);
        self.get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' missing after archive")))
    }

    /// Hard-delete a task and cascade related rows (Hermes `delete_task`).
    pub fn delete_task(&self, task_id: &str) -> Result<bool, AgentError> {
        if self.get_task(task_id)?.is_none() {
            return Ok(false);
        }
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "DELETE FROM kanban_task_links WHERE parent_id = ?1 OR child_id = ?1",
            params![task_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban delete links: {e}")))?;
        conn.execute("DELETE FROM kanban_task_comments WHERE task_id = ?1", params![task_id])
            .map_err(|e| AgentError::Database(format!("kanban delete comments: {e}")))?;
        conn.execute("DELETE FROM kanban_task_events WHERE task_id = ?1", params![task_id])
            .map_err(|e| AgentError::Database(format!("kanban delete events: {e}")))?;
        conn.execute("DELETE FROM kanban_task_runs WHERE task_id = ?1", params![task_id])
            .map_err(|e| AgentError::Database(format!("kanban delete runs: {e}")))?;
        conn.execute("DELETE FROM kanban_notify_subs WHERE task_id = ?1", params![task_id])
            .map_err(|e| AgentError::Database(format!("kanban delete subs: {e}")))?;
        let deleted = conn
            .execute("DELETE FROM kanban_tasks WHERE id = ?1", params![task_id])
            .map_err(|e| AgentError::Database(format!("kanban delete task: {e}")))?;
        drop(conn);
        if deleted == 0 {
            return Ok(false);
        }
        let _ = self.recompute_ready(DEFAULT_FAILURE_LIMIT);
        Ok(true)
    }

    pub fn reclaim_stale_claims(&self) -> Result<usize, AgentError> {
        self.reclaim_stale_claims_with_limit(DEFAULT_FAILURE_LIMIT)
    }

    pub fn reclaim_stale_claims_with_limit(&self, failure_limit: u32) -> Result<usize, AgentError> {
        let now = now_secs();
        let stale: Vec<(String, Option<String>, Option<i64>)> = {
            let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
            let mut stmt = conn
                .prepare(
                    "SELECT id, worker_id, current_run_id FROM kanban_tasks
                     WHERE status = 'doing' AND claim_expires IS NOT NULL AND claim_expires < ?1",
                )
                .map_err(|e| AgentError::Database(format!("kanban reclaim prepare: {e}")))?;
            let rows = stmt
                .query_map(params![now], |row| {
                    Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
                })
                .map_err(|e| AgentError::Database(format!("kanban reclaim query: {e}")))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(|e| AgentError::Database(format!("kanban reclaim map: {e}")))?);
            }
            out
        };
        let mut reclaimed = 0usize;
        for (task_id, worker_id, run_id) in stale {
            if let Some(rid) = run_id {
                let _ = self.end_run(rid, "reclaimed", Some("reclaimed"), Some("stale claim"), None);
            }
            let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
            conn.execute(
                "UPDATE kanban_tasks SET status = 'todo', worker_id = NULL,
                 claim_expires = NULL, current_run_id = NULL, updated_at = ?1
                 WHERE id = ?2 AND status = 'doing'",
                params![now, task_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban reclaim: {e}")))?;
            drop(conn);
            let wid = worker_id.as_deref().unwrap_or("unknown");
            let _ = self.record_task_failure(&task_id, &format!("stale claim (worker {wid})"), failure_limit);
            reclaimed += 1;
        }
        Ok(reclaimed)
    }

    /// Terminate in-flight workers that exceeded per-task ``max_runtime_seconds``.
    pub fn enforce_max_runtime(&self, failure_limit: u32) -> Result<usize, AgentError> {
        self.enforce_max_runtime_with(failure_limit, |_: &str| {})
    }

    /// Like [`Self::enforce_max_runtime`], invoking `on_overdue` for each task before DB release.
    pub fn enforce_max_runtime_with<F>(
        &self,
        failure_limit: u32,
        mut on_overdue: F,
    ) -> Result<usize, AgentError>
    where
        F: FnMut(&str),
    {
        let now = now_secs();
        let overdue: Vec<(String, i64, i32, i64)> = {
            let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
            let mut stmt = conn
                .prepare(
                    "SELECT t.id, t.current_run_id, t.max_runtime_seconds, r.started_at
                     FROM kanban_tasks t
                     JOIN kanban_task_runs r ON r.id = t.current_run_id
                     WHERE t.status = 'doing' AND t.max_runtime_seconds IS NOT NULL
                     AND r.started_at IS NOT NULL",
                )
                .map_err(|e| AgentError::Database(format!("kanban runtime prepare: {e}")))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i32>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .map_err(|e| AgentError::Database(format!("kanban runtime query: {e}")))?;
            let mut out = Vec::new();
            for row in rows {
                let (task_id, run_id, limit, started) =
                    row.map_err(|e| AgentError::Database(format!("kanban runtime map: {e}")))?;
                if now.saturating_sub(started) >= i64::from(limit.max(1)) {
                    out.push((task_id, run_id, limit, started));
                }
            }
            out
        };
        let mut timed_out = 0usize;
        for (task_id, run_id, limit, started) in overdue {
            on_overdue(&task_id);
            let elapsed = now.saturating_sub(started);
            let error = format!("elapsed {elapsed}s > limit {limit}s");
            let _ = self.end_run(
                run_id,
                "timed_out",
                Some("timed_out"),
                None,
                Some(&error),
            );
            let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
            let updated = conn
                .execute(
                    "UPDATE kanban_tasks SET status = 'todo', worker_id = NULL,
                     claim_expires = NULL, current_run_id = NULL, updated_at = ?1
                     WHERE id = ?2 AND status = 'doing'",
                    params![now, task_id],
                )
                .map_err(|e| AgentError::Database(format!("kanban runtime release: {e}")))?;
            drop(conn);
            if updated == 0 {
                continue;
            }
            let payload = serde_json::json!({
                "elapsed_seconds": elapsed,
                "limit_seconds": limit,
            })
            .to_string();
            self.append_event(&task_id, "timed_out", Some(&payload))?;
            let _ = self.record_task_failure(&task_id, &error, failure_limit);
            timed_out += 1;
        }
        Ok(timed_out)
    }

    pub fn add_comment(
        &self,
        task_id: &str,
        author: &str,
        body: &str,
    ) -> Result<KanbanComment, AgentError> {
        let author = author.trim();
        let body = body.trim();
        if body.is_empty() {
            return Err(AgentError::Validation("comment body is required".into()));
        }
        if author.is_empty() {
            return Err(AgentError::Validation("comment author is required".into()));
        }
        if self.get_task(task_id)?.is_none() {
            return Err(AgentError::Validation(format!("unknown task {task_id}")));
        }
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "INSERT INTO kanban_task_comments (task_id, author, body, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![task_id, author, body, now],
        )
        .map_err(|e| AgentError::Database(format!("kanban comment insert: {e}")))?;
        let id = conn.last_insert_rowid();
        drop(conn);
        Ok(KanbanComment {
            id,
            task_id: task_id.to_string(),
            author: author.to_string(),
            body: body.to_string(),
            created_at: now,
        })
    }

    pub fn list_comments(&self, task_id: &str) -> Result<Vec<KanbanComment>, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, author, body, created_at
                 FROM kanban_task_comments WHERE task_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| AgentError::Database(format!("kanban comments prepare: {e}")))?;
        let rows = stmt
            .query_map(params![task_id], |row| {
                Ok(KanbanComment {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    author: row.get(2)?,
                    body: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("kanban comments query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(format!("kanban comments map: {e}")))?);
        }
        Ok(out)
    }

    pub fn block_task(
        &self,
        task_id: &str,
        worker_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<KanbanTask, AgentError> {
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = if let Some(w) = worker_id {
            conn.execute(
                "UPDATE kanban_tasks SET status = 'blocked', worker_id = NULL,
                 claim_expires = NULL, result = ?1, updated_at = ?2
                 WHERE id = ?3 AND status IN ('todo', 'doing')
                 AND (worker_id = ?4 OR worker_id IS NULL)",
                params![reason, now, task_id, w],
            )
        } else {
            conn.execute(
                "UPDATE kanban_tasks SET status = 'blocked', worker_id = NULL,
                 claim_expires = NULL, result = ?1, updated_at = ?2
                 WHERE id = ?3 AND status IN ('todo', 'doing')",
                params![reason, now, task_id],
            )
        }
        .map_err(|e| AgentError::Database(format!("kanban block: {e}")))?;
        if updated == 0 {
            return Err(AgentError::Validation(format!(
                "task '{task_id}' not blocked (wrong status, worker mismatch, or missing)"
            )));
        }
        drop(conn);
        self.close_current_run(
            task_id,
            "blocked",
            Some("blocked"),
            reason,
            None,
        )?;
        if worker_id.is_some() {
            let payload = reason.map(|r| serde_json::json!({ "reason": r }).to_string());
            self.append_event(
                task_id,
                "blocked",
                payload.as_deref(),
            )?;
        }
        self.get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' missing after block")))
    }

    pub fn unblock_task(&self, task_id: &str) -> Result<KanbanTask, AgentError> {
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = conn
            .execute(
                "UPDATE kanban_tasks SET status = 'todo', worker_id = NULL,
                 claim_expires = NULL, updated_at = ?1
                 WHERE id = ?2 AND status = 'blocked'",
                params![now, task_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban unblock: {e}")))?;
        if updated == 0 {
            return Err(AgentError::Validation(format!(
                "task '{task_id}' not unblocked (not found or not blocked)"
            )));
        }
        drop(conn);
        self.close_current_run(task_id, "reclaimed", Some("reclaimed"), None, None)?;
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "UPDATE kanban_tasks SET consecutive_failures = 0, last_failure_error = NULL,
             current_run_id = NULL, updated_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban unblock reset: {e}")))?;
        drop(conn);
        self.append_event(task_id, "unblocked", None)?;
        let _ = self.recompute_ready(DEFAULT_FAILURE_LIMIT);
        self.get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' missing after unblock")))
    }

    /// Assign or reassign a task (Hermes `assign_task` subset).
    pub fn set_task_assignee(
        &self,
        task_id: &str,
        assignee: Option<&str>,
    ) -> Result<(), AgentError> {
        let task = self
            .get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' not found")))?;
        if task.status == "doing" && task.worker_id.is_some() {
            return Err(AgentError::Validation(format!(
                "cannot reassign {task_id}: currently doing (claimed). \
                 Wait for completion or reclaim the stale lock first."
            )));
        }
        let new_assignee = assignee.map(str::trim).filter(|s| !s.is_empty());
        let changed = task.assignee.as_deref() != new_assignee;
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        if changed {
            conn.execute(
                "UPDATE kanban_tasks SET assignee = ?1, consecutive_failures = 0,
                 last_failure_error = NULL, updated_at = ?2 WHERE id = ?3",
                params![new_assignee, now, task_id],
            )
        } else {
            conn.execute(
                "UPDATE kanban_tasks SET assignee = ?1, updated_at = ?2 WHERE id = ?3",
                params![new_assignee, now, task_id],
            )
        }
        .map_err(|e| AgentError::Database(format!("kanban assign: {e}")))?;
        drop(conn);
        let payload = serde_json::json!({ "assignee": new_assignee }).to_string();
        self.append_event(task_id, "assigned", Some(&payload))?;
        Ok(())
    }

    /// Direct status write for dashboard drag-drop / PATCH (non-terminal verbs).
    pub fn set_task_status(&self, task_id: &str, status: &str) -> Result<KanbanTask, AgentError> {
        let task = self
            .get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' not found")))?;
        if task.status == status {
            return Ok(task);
        }
        if status == "todo" {
            let blockers = self.parents_blocking_todo(task_id)?;
            if !blockers.is_empty() {
                let names: Vec<String> = blockers
                    .iter()
                    .map(|p| format!("'{}' ({}, status={})", p.title, p.id, p.status))
                    .collect();
                return Err(kanban_conflict_error(
                    &format!(
                        "Cannot move to 'todo': blocked by parent(s) not done — {}",
                        names.join(", ")
                    ),
                    &blockers,
                ));
            }
        }
        if task.status == "doing" && status != "doing" {
            self.close_current_run(task_id, "reclaimed", Some("reclaimed"), None, None)?;
        }
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = conn
            .execute(
                "UPDATE kanban_tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![status, now, task_id],
            )
            .map_err(|e| AgentError::Database(format!("kanban set status: {e}")))?;
        if updated == 0 {
            return Err(AgentError::Validation(format!("task '{task_id}' not found")));
        }
        drop(conn);
        let payload = serde_json::json!({ "status": status }).to_string();
        self.append_event(task_id, "status", Some(&payload))?;
        let _ = self.recompute_ready(DEFAULT_FAILURE_LIMIT);
        self.get_task(task_id)?
            .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' missing after status set")))
    }

    /// Update task priority and emit reprioritized event.
    pub fn set_task_priority(&self, task_id: &str, priority: i32) -> Result<(), AgentError> {
        if self.get_task(task_id)?.is_none() {
            return Err(AgentError::Validation(format!("task '{task_id}' not found")));
        }
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "UPDATE kanban_tasks SET priority = ?1, updated_at = ?2 WHERE id = ?3",
            params![priority, now, task_id],
        )
        .map_err(|e| AgentError::Database(format!("kanban priority: {e}")))?;
        drop(conn);
        let payload = serde_json::json!({ "priority": priority }).to_string();
        self.append_event(task_id, "reprioritized", Some(&payload))?;
        Ok(())
    }

    /// Edit task title and/or body.
    pub fn edit_task(
        &self,
        task_id: &str,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<(), AgentError> {
        if self.get_task(task_id)?.is_none() {
            return Err(AgentError::Validation(format!("task '{task_id}' not found")));
        }
        if let Some(t) = title
            && t.trim().is_empty()
        {
            return Err(AgentError::Validation("title cannot be empty".into()));
        }
        let now = now_secs();
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        match (title, body) {
            (Some(t), Some(b)) => {
                conn.execute(
                    "UPDATE kanban_tasks SET title = ?1, body = ?2, updated_at = ?3 WHERE id = ?4",
                    params![t.trim(), b, now, task_id],
                )
            }
            (Some(t), None) => {
                conn.execute(
                    "UPDATE kanban_tasks SET title = ?1, updated_at = ?2 WHERE id = ?3",
                    params![t.trim(), now, task_id],
                )
            }
            (None, Some(b)) => {
                conn.execute(
                    "UPDATE kanban_tasks SET body = ?1, updated_at = ?2 WHERE id = ?3",
                    params![b, now, task_id],
                )
            }
            (None, None) => return Ok(()),
        }
        .map_err(|e| AgentError::Database(format!("kanban edit: {e}")))?;
        Ok(())
    }

    /// Register a gateway chat for terminal-state notifications (idempotent).
    pub fn add_notify_sub(
        &self,
        task_id: &str,
        platform: &str,
        chat_id: &str,
        thread_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<(), AgentError> {
        let now = now_secs();
        let thread = thread_id.unwrap_or("");
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute(
            "INSERT OR IGNORE INTO kanban_notify_subs
             (task_id, platform, chat_id, thread_id, user_id, created_at, last_event_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![task_id, platform, chat_id, thread, user_id, now],
        )
        .map_err(|e| AgentError::Database(format!("kanban notify sub: {e}")))?;
        Ok(())
    }

    pub fn list_notify_subs(&self) -> Result<Vec<KanbanNotifySub>, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT task_id, platform, chat_id, thread_id, user_id, created_at, last_event_id
                 FROM kanban_notify_subs",
            )
            .map_err(|e| AgentError::Database(format!("kanban notify list prepare: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(KanbanNotifySub {
                    task_id: row.get(0)?,
                    platform: row.get(1)?,
                    chat_id: row.get(2)?,
                    thread_id: row.get(3)?,
                    user_id: row.get(4)?,
                    created_at: row.get(5)?,
                    last_event_id: row.get(6)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("kanban notify list query: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| AgentError::Database(format!("kanban notify list map: {e}")))?);
        }
        Ok(out)
    }

    fn unseen_events_for_sub(
        &self,
        conn: &Connection,
        task_id: &str,
        platform: &str,
        chat_id: &str,
        thread_id: &str,
        kinds: &[&str],
    ) -> Result<(i64, Vec<KanbanEvent>), AgentError> {
        let row = conn
            .query_row(
                "SELECT last_event_id FROM kanban_notify_subs
                 WHERE task_id = ?1 AND platform = ?2 AND chat_id = ?3 AND thread_id = ?4",
                params![task_id, platform, chat_id, thread_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|e| AgentError::Database(format!("kanban notify cursor: {e}")))?;
        let Some(cursor) = row else {
            return Ok((0, Vec::new()));
        };
        let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT id, task_id, kind, payload, created_at FROM kanban_task_events
             WHERE task_id = ?1 AND id > ?2 AND kind IN ({placeholders})
             ORDER BY id ASC"
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(task_id.to_string()),
            Box::new(cursor),
        ];
        for kind in kinds {
            params_vec.push(Box::new(kind.to_string()));
        }
        let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AgentError::Database(format!("kanban notify events prepare: {e}")))?;
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(KanbanEvent {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    kind: row.get(2)?,
                    payload: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| AgentError::Database(format!("kanban notify events query: {e}")))?;
        let mut out = Vec::new();
        let mut max_id = cursor;
        for row in rows {
            let ev = row.map_err(|e| AgentError::Database(format!("kanban notify events map: {e}")))?;
            max_id = max_id.max(ev.id);
            out.push(ev);
        }
        Ok((max_id, out))
    }

    /// Atomically claim unseen terminal events for one subscription.
    pub fn claim_unseen_events_for_sub(
        &self,
        task_id: &str,
        platform: &str,
        chat_id: &str,
        thread_id: Option<&str>,
        kinds: &[&str],
    ) -> Result<(i64, i64, Vec<KanbanEvent>), AgentError> {
        let thread = thread_id.unwrap_or("");
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        conn.execute("BEGIN IMMEDIATE", [])
            .map_err(|e| AgentError::Database(format!("kanban notify txn: {e}")))?;
        let result = (|| {
            let old_row = conn
                .query_row(
                    "SELECT last_event_id FROM kanban_notify_subs
                     WHERE task_id = ?1 AND platform = ?2 AND chat_id = ?3 AND thread_id = ?4",
                    params![task_id, platform, chat_id, thread],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|e| AgentError::Database(format!("kanban notify claim cursor: {e}")))?;
            let Some(old_cursor) = old_row else {
                return Ok((0, 0, Vec::new()));
            };
            let (new_cursor, events) = self.unseen_events_for_sub(
                &conn, task_id, platform, chat_id, thread, kinds,
            )?;
            if events.is_empty() {
                return Ok((old_cursor, old_cursor, events));
            }
            let updated = conn
                .execute(
                    "UPDATE kanban_notify_subs SET last_event_id = ?1
                     WHERE task_id = ?2 AND platform = ?3 AND chat_id = ?4 AND thread_id = ?5
                     AND last_event_id = ?6",
                    params![new_cursor, task_id, platform, chat_id, thread, old_cursor],
                )
                .map_err(|e| AgentError::Database(format!("kanban notify advance: {e}")))?;
            if updated == 0 {
                return Ok((old_cursor, old_cursor, Vec::new()));
            }
            Ok((old_cursor, new_cursor, events))
        })();
        match &result {
            Ok(_) => {
                conn.execute("COMMIT", [])
                    .map_err(|e| AgentError::Database(format!("kanban notify commit: {e}")))?;
            }
            Err(_) => {
                let _ = conn.execute("ROLLBACK", []);
            }
        }
        result
    }

    /// Undo a notification claim when delivery fails (CAS guard).
    pub fn rewind_notify_cursor(
        &self,
        task_id: &str,
        platform: &str,
        chat_id: &str,
        thread_id: Option<&str>,
        claimed_cursor: i64,
        old_cursor: i64,
    ) -> Result<bool, AgentError> {
        let thread = thread_id.unwrap_or("");
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let updated = conn
            .execute(
                "UPDATE kanban_notify_subs SET last_event_id = ?1
                 WHERE task_id = ?2 AND platform = ?3 AND chat_id = ?4 AND thread_id = ?5
                 AND last_event_id = ?6",
                params![old_cursor, task_id, platform, chat_id, thread, claimed_cursor],
            )
            .map_err(|e| AgentError::Database(format!("kanban notify rewind: {e}")))?;
        Ok(updated > 0)
    }

    /// Drop subscriptions when a task reaches a final status.
    pub fn remove_notify_subs_for_task(&self, task_id: &str) -> Result<usize, AgentError> {
        let conn = self.conn.lock().map_err(|_| AgentError::Database("kanban db lock poisoned".into()))?;
        let n = conn
            .execute("DELETE FROM kanban_notify_subs WHERE task_id = ?1", params![task_id])
            .map_err(|e| AgentError::Database(format!("kanban notify remove: {e}")))?;
        Ok(n)
    }

    pub fn default_claim_ttl_secs() -> i64 {
        DEFAULT_CLAIM_TTL_SECS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (TempDir, Arc<KanbanDb>) {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("kanban.db");
        let db = KanbanDb::open(&path).expect("open");
        (dir, db)
    }

    #[test]
    fn create_claim_complete_roundtrip() {
        let (_dir, db) = test_db();
        let task = db
            .create_task("Refactor auth", Some("migrate handlers"), 1)
            .expect("create");
        assert_eq!(task.status, "todo");

        let claimed = db
            .claim_task(&task.id, "worker-a", 900)
            .expect("claim");
        assert_eq!(claimed.status, "doing");
        assert_eq!(claimed.worker_id.as_deref(), Some("worker-a"));

        db.heartbeat_task(&task.id, "worker-a", 900).expect("heartbeat");

        let done = db
            .complete_task(&task.id, Some("worker-a"), Some("shipped"))
            .expect("complete");
        assert_eq!(done.status, "done");
        assert_eq!(done.result.as_deref(), Some("shipped"));
    }

    #[test]
    fn double_claim_fails() {
        let (_dir, db) = test_db();
        let task = db.create_task("Task", None, 0).expect("create");
        db.claim_task(&task.id, "w1", 900).expect("claim1");
        assert!(db.claim_task(&task.id, "w2", 900).is_err());
        db.release_task(&task.id, Some("w1")).expect("release");
        db.claim_task(&task.id, "w2", 900).expect("claim2");
    }

    #[test]
    fn list_filters_by_status() {
        let (_dir, db) = test_db();
        db.create_task("A", None, 0).expect("a");
        let b = db.create_task("B", None, 0).expect("b");
        db.claim_task(&b.id, "w", 900).expect("claim");
        let todo = db
            .list_tasks(Some(KanbanStatus::Todo), 10)
            .expect("todo list");
        assert_eq!(todo.len(), 1);
        let doing = db
            .list_tasks(Some(KanbanStatus::Doing), 10)
            .expect("doing list");
        assert_eq!(doing.len(), 1);
    }

    #[test]
    fn link_blocks_child_until_parent_done() {
        let (_dir, db) = test_db();
        let parent = db.create_task("Parent", None, 0).expect("parent");
        let child = db.create_task("Child", None, 0).expect("child");
        db.link_tasks(&parent.id, &child.id).expect("link");
        assert!(db.claim_task(&child.id, "w", 900).is_err());
        db.complete_task(&parent.id, None, Some("ok")).expect("done");
        db.claim_task(&child.id, "w", 900).expect("claim child");
    }

    #[test]
    fn block_unblock_and_comments() {
        let (_dir, db) = test_db();
        let task = db.create_task("Needs input", None, 0).expect("create");
        db.claim_task(&task.id, "w1", 900).expect("claim");
        let blocked = db
            .block_task(&task.id, Some("w1"), Some("waiting on API key"))
            .expect("block");
        assert_eq!(blocked.status, "blocked");

        let comment = db
            .add_comment(&task.id, "w1", "Still blocked")
            .expect("comment");
        assert_eq!(comment.body, "Still blocked");
        let comments = db.list_comments(&task.id).expect("list comments");
        assert_eq!(comments.len(), 1);

        let unblocked = db.unblock_task(&task.id).expect("unblock");
        assert_eq!(unblocked.status, "todo");
    }

    #[test]
    fn enforce_max_runtime_times_out_long_run() {
        let (_dir, db) = test_db();
        let task = db
            .create_task_with_runtime("Slow job", None, 0, Some(1))
            .expect("create");
        db.claim_task(&task.id, "w1", 900).expect("claim");
        std::thread::sleep(std::time::Duration::from_secs(2));
        let n = db.enforce_max_runtime(2).expect("enforce");
        assert_eq!(n, 1);
        let t = db.get_task(&task.id).expect("get").expect("row");
        assert_eq!(t.status, "todo");
    }

    #[test]
    fn notify_sub_claim_and_rewind() {
        let (_dir, db) = test_db();
        let task = db.create_task("Notify me", None, 0).expect("create");
        db.add_notify_sub(&task.id, "telegram", "chat-1", None, Some("user-1"))
            .expect("sub");
        db.claim_task(&task.id, "w1", 900).expect("claim");
        db.complete_task(&task.id, Some("w1"), Some("shipped"))
            .expect("complete");
        let (old, new, events) = db
            .claim_unseen_events_for_sub(
                &task.id,
                "telegram",
                "chat-1",
                None,
                KANBAN_NOTIFY_TERMINAL_KINDS,
            )
            .expect("claim");
        assert_eq!(old, 0);
        assert!(new > 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "completed");
        let (_, _, again) = db
            .claim_unseen_events_for_sub(
                &task.id,
                "telegram",
                "chat-1",
                None,
                KANBAN_NOTIFY_TERMINAL_KINDS,
            )
            .expect("claim2");
        assert!(again.is_empty());
    }

    #[test]
    fn decompose_creates_children_and_promotes_root() {
        let (_dir, db) = test_db();
        let tid = db.create_triage_task("ship a feature", None, 0).expect("triage");
        assert_eq!(db.get_task(&tid.id).expect("get").expect("row").status, "triage");

        let children = vec![
            KanbanDecomposeChild {
                title: "research".into(),
                body: Some("look at prior art".into()),
                assignee: Some("work".into()),
                parents: vec![],
            },
            KanbanDecomposeChild {
                title: "build it".into(),
                body: Some("write code".into()),
                assignee: Some("work".into()),
                parents: vec![0],
            },
        ];
        let child_ids = db
            .decompose_triage_task(&tid.id, Some("default"), &children, Some("decomposer"))
            .expect("decompose")
            .expect("some ids");
        assert_eq!(child_ids.len(), 2);

        let root = db.get_task(&tid.id).expect("get").expect("root");
        assert_eq!(root.status, "todo");
        let c0 = db.get_task(&child_ids[0]).expect("get").expect("c0");
        assert_eq!(c0.status, "todo");
        db.claim_task(&child_ids[0], "w1", 900).expect("claim first");
        assert!(db.claim_task(&child_ids[1], "w2", 900).is_err());
    }

    #[test]
    fn decompose_rejects_cycle() {
        let (_dir, db) = test_db();
        let tid = db.create_triage_task("x", None, 0).expect("triage");
        let children = vec![
            KanbanDecomposeChild {
                title: "a".into(),
                body: None,
                assignee: None,
                parents: vec![1],
            },
            KanbanDecomposeChild {
                title: "b".into(),
                body: None,
                assignee: None,
                parents: vec![0],
            },
        ];
        assert!(db
            .decompose_triage_task(&tid.id, None, &children, None)
            .is_err());
    }

    #[test]
    fn specify_promotes_triage_to_todo() {
        let (_dir, db) = test_db();
        let t = db.create_triage_task("vague idea", None, 0).expect("triage");
        assert!(db
            .specify_triage_task(
                &t.id,
                "Concrete title",
                Some("spec body"),
                Some("work"),
                Some("user"),
            )
            .expect("spec"));
        let row = db.get_task(&t.id).expect("get").expect("row");
        assert_eq!(row.status, "todo");
        assert_eq!(row.title, "Concrete title");
    }

    #[test]
    fn failure_limit_trips_circuit_breaker() {
        let (_dir, db) = test_db();
        let task = db.create_task("Flaky", None, 0).expect("create");
        let mut tripped = false;
        for i in 0..2 {
            db.claim_task(&task.id, "w1", 900).expect("claim");
            tripped = db
                .record_task_failure(&task.id, &format!("fail {i}"), 2)
                .expect("record");
        }
        assert!(tripped);
        let t = db.get_task(&task.id).expect("get").expect("row");
        assert_eq!(t.status, "blocked");
        assert_eq!(t.consecutive_failures, 2);
        let runs = db.list_task_runs(&task.id, 5).expect("runs");
        assert!(runs.len() >= 2);
    }

    #[test]
    fn unblock_resets_failures() {
        let (_dir, db) = test_db();
        let task = db.create_task("Recover", None, 0).expect("create");
        for _ in 0..2 {
            db.claim_task(&task.id, "w1", 900).expect("claim");
            let _ = db.record_task_failure(&task.id, "oops", 2);
        }
        let unblocked = db.unblock_task(&task.id).expect("unblock");
        assert_eq!(unblocked.status, "todo");
        assert_eq!(unblocked.consecutive_failures, 0);
    }

    #[test]
    fn list_events_after_tails_incrementally() {
        let (_dir, db) = test_db();
        let task = db.create_task("Eventful", None, 0).expect("create");
        db.claim_task(&task.id, "w1", 900).expect("claim");
        db.complete_task(&task.id, Some("w1"), Some("done"))
            .expect("complete");

        let (cursor, batch) = db.list_events_after(0, 50).expect("tail");
        assert!(cursor > 0);
        assert!(!batch.is_empty());
        assert!(batch.iter().any(|e| e.kind == "completed"));

        let (cursor2, batch2) = db.list_events_after(cursor, 50).expect("tail2");
        assert_eq!(cursor2, cursor);
        assert!(batch2.is_empty());
    }

    #[test]
    fn archive_unblocks_dependents_like_done() {
        let (_dir, db) = test_db();
        let parent = db.create_task("Parent", None, 0).expect("parent");
        let child = db.create_task("Child", None, 0).expect("child");
        db.link_tasks(&parent.id, &child.id).expect("link");
        assert!(db.claim_task(&child.id, "w", 900).is_err());
        db.archive_task(&parent.id).expect("archive");
        db.claim_task(&child.id, "w", 900).expect("claim child");
    }

    #[test]
    fn delete_task_cascades() {
        let (_dir, db) = test_db();
        let task = db.create_task("Gone", None, 0).expect("create");
        db.add_comment(&task.id, "me", "note").expect("comment");
        assert!(db.delete_task(&task.id).expect("delete"));
        assert!(db.get_task(&task.id).expect("get").is_none());
        assert!(db.list_comments(&task.id).expect("comments").is_empty());
    }

    #[test]
    fn scheduled_at_defers_claim_until_elapsed() {
        let (_dir, db) = test_db();
        let task = db.create_task("Later", None, 0).expect("create");
        let future = now_secs() + 3600;
        db.set_scheduled_at(&task.id, Some(future)).expect("schedule");
        assert!(db.claim_task(&task.id, "w", 900).is_err());
        db.set_scheduled_at(&task.id, Some(now_secs() - 1)).expect("unschedule");
        db.claim_task(&task.id, "w", 900).expect("claim");
    }
}
