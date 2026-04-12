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
use edgecrab_core::{Agent, IsolatedAgentOptions};
use edgecrab_types::{AgentError, OriginChat, Platform};
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
    /// Isolated per-chat agent runtime.
    pub agent: Arc<Agent>,
    /// Last time this session was active (for idle expiry)
    pub last_activity: Instant,
}

impl GatewaySession {
    fn new(agent: Arc<Agent>) -> Self {
        Self {
            agent,
            last_activity: Instant::now(),
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
    /// If the session exists, returns it. Otherwise clones the shared gateway
    /// agent runtime into a fresh isolated per-chat session.
    pub async fn resolve(
        &self,
        key: &SessionKey,
        base_agent: &Arc<Agent>,
        origin_chat: OriginChat,
    ) -> Result<Arc<RwLock<GatewaySession>>, AgentError> {
        if let Some(existing) = self.sessions.get(key) {
            return Ok(existing.clone());
        }

        let child = Arc::new(
            base_agent
                .fork_isolated(IsolatedAgentOptions {
                    platform: Some(key.platform),
                    origin_chat: Some(origin_chat),
                    ..IsolatedAgentOptions::default()
                })
                .await?,
        );
        let created = Arc::new(RwLock::new(GatewaySession::new(child)));
        Ok(self
            .sessions
            .entry(key.clone())
            .or_insert_with(|| created.clone())
            .clone())
    }

    pub fn get(&self, key: &SessionKey) -> Option<Arc<RwLock<GatewaySession>>> {
        self.sessions.get(key).map(|entry| entry.clone())
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
            let removed = self.sessions.remove(&key).map(|(_, session)| session);
            if let Some(session) = removed {
                let agent = session.read().await.agent.clone();
                agent.finalize_session().await;
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

    /// Finalize and remove all tracked sessions.
    pub async fn finalize_all(&self) -> usize {
        let sessions = self
            .sessions
            .iter()
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>();
        let mut finalized = 0usize;
        for key in sessions {
            let removed = self.sessions.remove(&key).map(|(_, session)| session);
            if let Some(session) = removed {
                let agent = session.read().await.agent.clone();
                agent.finalize_session().await;
                finalized += 1;
            }
        }
        finalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_core::AgentBuilder;
    use edgequake_llm::MockProvider;

    fn base_agent() -> Arc<Agent> {
        Arc::new(
            AgentBuilder::new("mock")
                .provider(Arc::new(MockProvider::new()))
                .build()
                .expect("build agent"),
        )
    }

    #[tokio::test]
    async fn resolve_creates_new_session() {
        let mgr = SessionManager::new(Duration::from_secs(3600));
        let key = SessionKey::new(Platform::Telegram, "user1", Some("ch1"));
        let agent = base_agent();

        let _session = mgr
            .resolve(
                &key,
                &agent,
                OriginChat::new(Platform::Telegram.to_string(), "ch1".to_string()),
            )
            .await
            .expect("resolve session");
        assert_eq!(mgr.session_count(), 1);
    }

    #[tokio::test]
    async fn resolve_returns_same_session() {
        let mgr = SessionManager::new(Duration::from_secs(3600));
        let key = SessionKey::new(Platform::Discord, "user1", None);
        let agent = base_agent();

        let s1 = mgr
            .resolve(
                &key,
                &agent,
                OriginChat::new(Platform::Discord.to_string(), "user1".to_string()),
            )
            .await
            .expect("resolve 1");
        let s2 = mgr
            .resolve(
                &key,
                &agent,
                OriginChat::new(Platform::Discord.to_string(), "user1".to_string()),
            )
            .await
            .expect("resolve 2");

        // Same Arc — pointer equality
        assert!(Arc::ptr_eq(&s1, &s2));
        assert_eq!(mgr.session_count(), 1);
    }

    #[tokio::test]
    async fn different_keys_different_sessions() {
        let mgr = SessionManager::new(Duration::from_secs(3600));
        let k1 = SessionKey::new(Platform::Telegram, "user1", None);
        let k2 = SessionKey::new(Platform::Telegram, "user2", None);
        let k3 = SessionKey::new(Platform::Discord, "user1", None);
        let agent = base_agent();

        mgr.resolve(
            &k1,
            &agent,
            OriginChat::new(Platform::Telegram.to_string(), "user1".to_string()),
        )
        .await
        .expect("resolve k1");
        mgr.resolve(
            &k2,
            &agent,
            OriginChat::new(Platform::Telegram.to_string(), "user2".to_string()),
        )
        .await
        .expect("resolve k2");
        mgr.resolve(
            &k3,
            &agent,
            OriginChat::new(Platform::Discord.to_string(), "user1".to_string()),
        )
        .await
        .expect("resolve k3");
        assert_eq!(mgr.session_count(), 3);
    }

    #[test]
    fn remove_session() {
        let mgr = SessionManager::new(Duration::from_secs(3600));
        let key = SessionKey::new(Platform::Telegram, "user1", None);
        assert!(!mgr.remove(&key));
        assert_eq!(mgr.session_count(), 0);
    }

    #[tokio::test]
    async fn cleanup_expired_removes_old_sessions() {
        // Use a very short timeout
        let mgr = SessionManager::new(Duration::from_millis(10));
        let key = SessionKey::new(Platform::Telegram, "user1", None);
        let agent = base_agent();
        mgr.resolve(
            &key,
            &agent,
            OriginChat::new(Platform::Telegram.to_string(), "user1".to_string()),
        )
        .await
        .expect("resolve session");

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
        let agent = base_agent();
        mgr.resolve(
            &key,
            &agent,
            OriginChat::new(Platform::Telegram.to_string(), "user1".to_string()),
        )
        .await
        .expect("resolve session");

        let evicted = mgr.cleanup_expired().await;
        assert_eq!(evicted, 0);
        assert_eq!(mgr.session_count(), 1);
    }
}
