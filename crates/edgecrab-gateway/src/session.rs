//! # Session manager — per-user conversation state
//!
//! WHY DashMap: Gateway sessions are accessed concurrently from multiple
//! platform adapter tasks. DashMap provides lock-free concurrent reads
//! and sharded writes, avoiding a single global mutex bottleneck.
//!
//! ```text
//!   SessionManager
//!     ├── resolve()    → get-or-create session for IncomingMessage
//!     ├── cleanup()    → expire idle sessions (background task)
//!     └── sessions     → DashMap<SessionKey, GatewaySession>
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use edgecrab_types::{Message, Platform};
use tokio::sync::RwLock;

/// Composite key identifying a unique gateway session.
///
/// WHY channel_id in key: A user might have different conversations
/// in different channels/groups. Each channel gets its own session.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SessionKey {
    pub platform: Platform,
    pub user_id: String,
    pub channel_id: Option<String>,
}

impl SessionKey {
    pub fn new(platform: Platform, user_id: &str, channel_id: Option<&str>) -> Self {
        Self {
            platform,
            user_id: user_id.to_string(),
            channel_id: channel_id.map(String::from),
        }
    }
}

/// Per-session state stored in the gateway.
pub struct GatewaySession {
    /// Unique session identifier (persisted to state_db)
    pub session_id: String,
    /// Conversation history for this session
    pub history: Vec<Message>,
    /// Last time this session was active (for idle expiry)
    pub last_activity: Instant,
    /// Model override for this session (if set via /model)
    pub model_override: Option<String>,
}

impl GatewaySession {
    fn new(session_id: String) -> Self {
        Self {
            session_id,
            history: Vec::new(),
            last_activity: Instant::now(),
            model_override: None,
        }
    }

    /// Touch the session to update last_activity.
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
}

/// Manages all active gateway sessions.
pub struct SessionManager {
    sessions: DashMap<SessionKey, Arc<RwLock<GatewaySession>>>,
    idle_timeout: Duration,
}

impl SessionManager {
    pub fn new(idle_timeout: Duration) -> Self {
        Self {
            sessions: DashMap::new(),
            idle_timeout,
        }
    }

    /// Get or create a session for the given key.
    ///
    /// If the session exists, returns it. Otherwise creates a new one
    /// with a fresh UUID session_id.
    pub fn resolve(&self, key: &SessionKey) -> Arc<RwLock<GatewaySession>> {
        self.sessions
            .entry(key.clone())
            .or_insert_with(|| {
                let session_id = uuid::Uuid::new_v4().to_string();
                Arc::new(RwLock::new(GatewaySession::new(session_id)))
            })
            .clone()
    }

    /// Remove a specific session (e.g., on /new or /reset command).
    pub fn remove(&self, key: &SessionKey) -> bool {
        self.sessions.remove(key).is_some()
    }

    /// Remove all idle sessions older than the configured timeout.
    ///
    /// Returns the number of sessions evicted.
    pub async fn cleanup_expired(&self) -> usize {
        let mut evicted = 0;
        let timeout = self.idle_timeout;

        // Collect keys to evict (can't hold DashMap ref across await)
        let expired_keys: Vec<SessionKey> = self
            .sessions
            .iter()
            .filter_map(|entry| {
                let session = entry.value().try_read().ok()?;
                if session.last_activity.elapsed() > timeout {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();

        for key in expired_keys {
            if self.sessions.remove(&key).is_some() {
                evicted += 1;
            }
        }

        if evicted > 0 {
            tracing::info!(evicted, "cleaned up idle gateway sessions");
        }
        evicted
    }

    /// Number of active sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_creates_new_session() {
        let mgr = SessionManager::new(Duration::from_secs(3600));
        let key = SessionKey::new(Platform::Telegram, "user1", Some("ch1"));

        let _session = mgr.resolve(&key);
        assert_eq!(mgr.session_count(), 1);
    }

    #[test]
    fn resolve_returns_same_session() {
        let mgr = SessionManager::new(Duration::from_secs(3600));
        let key = SessionKey::new(Platform::Discord, "user1", None);

        let s1 = mgr.resolve(&key);
        let s2 = mgr.resolve(&key);

        // Same Arc — pointer equality
        assert!(Arc::ptr_eq(&s1, &s2));
        assert_eq!(mgr.session_count(), 1);
    }

    #[test]
    fn different_keys_different_sessions() {
        let mgr = SessionManager::new(Duration::from_secs(3600));
        let k1 = SessionKey::new(Platform::Telegram, "user1", None);
        let k2 = SessionKey::new(Platform::Telegram, "user2", None);
        let k3 = SessionKey::new(Platform::Discord, "user1", None);

        mgr.resolve(&k1);
        mgr.resolve(&k2);
        mgr.resolve(&k3);
        assert_eq!(mgr.session_count(), 3);
    }

    #[test]
    fn remove_session() {
        let mgr = SessionManager::new(Duration::from_secs(3600));
        let key = SessionKey::new(Platform::Telegram, "user1", None);
        mgr.resolve(&key);
        assert_eq!(mgr.session_count(), 1);

        assert!(mgr.remove(&key));
        assert_eq!(mgr.session_count(), 0);
        assert!(!mgr.remove(&key)); // already gone
    }

    #[tokio::test]
    async fn cleanup_expired_removes_old_sessions() {
        // Use a very short timeout
        let mgr = SessionManager::new(Duration::from_millis(10));
        let key = SessionKey::new(Platform::Telegram, "user1", None);
        mgr.resolve(&key);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(50)).await;

        let evicted = mgr.cleanup_expired().await;
        assert_eq!(evicted, 1);
        assert_eq!(mgr.session_count(), 0);
    }

    #[tokio::test]
    async fn cleanup_keeps_active_sessions() {
        let mgr = SessionManager::new(Duration::from_secs(3600));
        let key = SessionKey::new(Platform::Telegram, "user1", None);
        mgr.resolve(&key);

        let evicted = mgr.cleanup_expired().await;
        assert_eq!(evicted, 0);
        assert_eq!(mgr.session_count(), 1);
    }
}
