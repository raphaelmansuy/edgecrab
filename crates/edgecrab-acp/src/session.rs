//! # ACP Session Manager — maps session IDs to agent state
//!
//! WHY sessions: Each editor window (VS Code, Zed, JetBrains) gets its own
//! session with its own conversation history, working directory, and cancel
//! token. The session manager is the single source of truth.
//!
//! ```text
//! ┌─────────────┐     ┌──────────────────────────┐
//! │ Editor A     │────►│ Session "abc-123"         │
//! │ (cwd: /proj) │     │  history: [...messages]   │
//! └─────────────┘     │  model: "claude-opus-4.6" │
//!                      │  cancel: CancellationToken │
//! ┌─────────────┐     └──────────────────────────┘
//! │ Editor B     │────►│ Session "def-456"         │
//! │ (cwd: /lib)  │     │  history: [...]           │
//! └─────────────┘     └──────────────────────────┘
//! ```

use dashmap::DashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// State for a single ACP session.
#[derive(Debug)]
pub struct AcpSession {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    pub history: Vec<serde_json::Value>,
    pub cancel: CancellationToken,
}

impl AcpSession {
    fn new(session_id: String, cwd: String) -> Self {
        Self {
            session_id,
            cwd,
            model: String::new(),
            history: Vec::new(),
            cancel: CancellationToken::new(),
        }
    }
}

/// Thread-safe session manager using `DashMap` for lock-free concurrent access.
pub struct SessionManager {
    sessions: DashMap<String, Arc<tokio::sync::RwLock<AcpSession>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    /// Create a new session and return its ID.
    pub fn create_session(&self, cwd: &str) -> String {
        let id = Uuid::new_v4().to_string();
        let session = AcpSession::new(id.clone(), cwd.to_string());
        self.sessions
            .insert(id.clone(), Arc::new(tokio::sync::RwLock::new(session)));
        id
    }

    /// Get a session by ID (returns a cloned Arc for concurrent access).
    pub fn get_session(&self, session_id: &str) -> Option<Arc<tokio::sync::RwLock<AcpSession>>> {
        self.sessions.get(session_id).map(|v| Arc::clone(v.value()))
    }

    /// Remove a session.
    pub fn remove_session(&self, session_id: &str) -> bool {
        self.sessions.remove(session_id).is_some()
    }

    /// Fork a session: deep-copy history into a new session.
    pub async fn fork_session(&self, session_id: &str, cwd: &str) -> Option<String> {
        let original = self.get_session(session_id)?;
        let reader = original.read().await;

        let new_id = Uuid::new_v4().to_string();
        let mut new_session = AcpSession::new(new_id.clone(), cwd.to_string());
        new_session.model.clone_from(&reader.model);
        new_session.history = reader.history.clone();
        drop(reader);

        self.sessions.insert(
            new_id.clone(),
            Arc::new(tokio::sync::RwLock::new(new_session)),
        );
        Some(new_id)
    }

    /// List all sessions with lightweight info.
    pub async fn list_sessions(&self) -> Vec<crate::protocol::SessionInfo> {
        let mut result = Vec::with_capacity(self.sessions.len());
        for entry in self.sessions.iter() {
            let session = entry.value().read().await;
            result.push(crate::protocol::SessionInfo {
                session_id: session.session_id.clone(),
                cwd: session.cwd.clone(),
                model: session.model.clone(),
                history_len: session.history.len(),
            });
        }
        result
    }

    /// Update the working directory for a session.
    pub async fn update_cwd(&self, session_id: &str, cwd: &str) -> bool {
        if let Some(session) = self.get_session(session_id) {
            let mut writer = session.write().await;
            writer.cwd = cwd.to_string();
            true
        } else {
            false
        }
    }

    /// Number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether the manager has no sessions.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_get_session() {
        let mgr = SessionManager::new();
        let id = mgr.create_session("/project");
        assert!(!id.is_empty());
        assert!(mgr.get_session(&id).is_some());
        assert!(mgr.get_session("nonexistent").is_none());
    }

    #[test]
    fn remove_session() {
        let mgr = SessionManager::new();
        let id = mgr.create_session(".");
        assert!(mgr.remove_session(&id));
        assert!(!mgr.remove_session(&id)); // already removed
        assert!(mgr.get_session(&id).is_none());
    }

    #[tokio::test]
    async fn fork_session_copies_history() {
        let mgr = SessionManager::new();
        let id = mgr.create_session("/a");

        // Add history to original
        {
            let session = mgr.get_session(&id).expect("session");
            let mut w = session.write().await;
            w.history
                .push(serde_json::json!({"role": "user", "content": "hello"}));
            w.model = "test-model".to_string();
        }

        let fork_id = mgr.fork_session(&id, "/b").await.expect("fork");
        assert_ne!(id, fork_id);

        let forked = mgr.get_session(&fork_id).expect("forked session");
        let r = forked.read().await;
        assert_eq!(r.cwd, "/b");
        assert_eq!(r.model, "test-model");
        assert_eq!(r.history.len(), 1);
    }

    #[tokio::test]
    async fn fork_nonexistent_returns_none() {
        let mgr = SessionManager::new();
        assert!(mgr.fork_session("nope", ".").await.is_none());
    }

    #[tokio::test]
    async fn list_sessions_returns_all() {
        let mgr = SessionManager::new();
        mgr.create_session("/a");
        mgr.create_session("/b");
        let list = mgr.list_sessions().await;
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn update_cwd() {
        let mgr = SessionManager::new();
        let id = mgr.create_session("/old");
        assert!(mgr.update_cwd(&id, "/new").await);

        let session = mgr.get_session(&id).expect("session");
        let r = session.read().await;
        assert_eq!(r.cwd, "/new");
    }

    #[test]
    fn len_and_is_empty() {
        let mgr = SessionManager::new();
        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);
        mgr.create_session(".");
        assert!(!mgr.is_empty());
        assert_eq!(mgr.len(), 1);
    }
}
