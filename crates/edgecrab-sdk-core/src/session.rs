//! Session persistence.
//!
//! [`SdkSession`] wraps [`SessionDb`] with a stable API for querying
//! conversation history and full-text search.

use std::path::Path;
use std::sync::Arc;

use edgecrab_state::{SessionDb, SessionSearchHit, SessionStats, SessionSummary};
use edgecrab_types::Message;

use crate::error::SdkError;

/// Stable wrapper around the [`SessionDb`] SQLite store.
///
/// Provides read/query access to conversation sessions.
pub struct SdkSession {
    db: Arc<SessionDb>,
}

impl SdkSession {
    /// Open (or create) a session database at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SdkError> {
        let db = SessionDb::open(path.as_ref())
            .map_err(|e| SdkError::Config(format!("Failed to open session DB: {e}")))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// List recent sessions, ordered by start time descending.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>, SdkError> {
        self.db.list_sessions(limit).map_err(SdkError::Agent)
    }

    /// Full-text search across all session messages.
    pub fn search_sessions(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SessionSearchHit>, SdkError> {
        self.db
            .search_sessions_rich(query, limit)
            .map_err(SdkError::Agent)
    }

    /// Get all messages for a specific session.
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<Message>, SdkError> {
        self.db.get_messages(session_id).map_err(SdkError::Agent)
    }

    /// Delete a session by ID.
    pub fn delete_session(&self, id: &str) -> Result<(), SdkError> {
        self.db.delete_session(id).map_err(SdkError::Agent)
    }

    /// Rename a session (set its title).
    pub fn rename_session(&self, id: &str, title: &str) -> Result<(), SdkError> {
        self.db
            .update_session_title(id, title)
            .map_err(SdkError::Agent)
    }

    /// Prune sessions older than `older_than_days` days.
    /// Optionally filter by source (e.g. "cli", "gateway").
    /// Returns the number of sessions deleted.
    pub fn prune_sessions(
        &self,
        older_than_days: u32,
        source: Option<&str>,
    ) -> Result<usize, SdkError> {
        self.db
            .prune_sessions(older_than_days, source)
            .map_err(SdkError::Agent)
    }

    /// Get aggregate statistics about the session store.
    pub fn stats(&self) -> Result<SessionStats, SdkError> {
        self.db.session_statistics().map_err(SdkError::Agent)
    }

    /// Get the inner `Arc<SessionDb>` for passing to the agent builder.
    pub fn db_arc(&self) -> Arc<SessionDb> {
        Arc::clone(&self.db)
    }
}

impl From<Arc<SessionDb>> for SdkSession {
    fn from(db: Arc<SessionDb>) -> Self {
        Self { db }
    }
}
