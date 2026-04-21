//! # Gateway configuration
//!
//! WHY separate from AppConfig: Gateway-specific settings (platform
//! credentials, adapter toggles, delivery preferences) don't belong
//! in the generic agent config. This keeps concerns separated.

use std::time::Duration;

use edgecrab_core::config::{GroupPolicy, UnauthorizedDmBehavior};
use serde::Deserialize;

/// Progressive streaming and progress-notification settings for the gateway.
///
/// WHY a nested struct: These settings only apply to the async dispatch loop —
/// they're logically separate from server/session settings and map cleanly
/// to a `gateway.streaming:` YAML block.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GatewayStreamingConfig {
    /// Enable progressive delivery for edit-capable platforms (Telegram, Discord, Slack).
    ///
    /// When `true` (default), the gateway uses `Agent::chat_streaming()` and
    /// progressively edits the first reply message as tokens arrive.
    /// When `false`, the full response is sent as a single message (old behaviour).
    pub enabled: bool,
    /// Minimum pause between consecutive edits, in milliseconds (default: 300 ms).
    ///
    /// Prevents flooding the Telegram/Discord edit endpoint and triggering
    /// 429 Too Many Requests responses. Mirrors hermes-agent's `edit_interval`.
    pub edit_interval_ms: u64,
    /// Minimum characters buffered before triggering an intermediate edit (default: 40).
    ///
    /// Balances responsiveness against API call count.
    pub buffer_threshold: usize,
    /// Cursor appended to partial messages to signal "still typing" (default: " ▉").
    pub cursor: String,
    /// Send a tool-progress notification when a tool starts executing (default: true).
    ///
    /// The notification is a short, separate message (e.g. "🔧 web_search…").
    /// On edit-capable platforms this can be the same message being updated;
    /// on non-edit platforms it is a standalone status message.
    pub tool_progress: bool,
    /// Show the agent's reasoning/thinking block before the final answer (default: false).
    ///
    /// When `true`, reasoning text is forwarded to the platform before the
    /// response tokens start. Useful for debugging; generally off for production.
    pub show_reasoning: bool,
}

impl Default for GatewayStreamingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            edit_interval_ms: 300,
            buffer_threshold: 40,
            cursor: " ▉".into(),
            tool_progress: true,
            show_reasoning: false,
        }
    }
}

impl GatewayStreamingConfig {
    /// Edit interval as a `Duration`.
    pub fn edit_interval(&self) -> Duration {
        Duration::from_millis(self.edit_interval_ms)
    }
}

/// Gateway-specific configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    /// Host to bind the health/API server to
    pub host: String,
    /// Port for the health/API server
    pub port: u16,
    /// Session idle timeout in seconds (default: 1 hour)
    pub session_idle_timeout_secs: u64,
    /// Session cleanup interval in seconds (default: 5 minutes)
    pub cleanup_interval_secs: u64,
    /// Default model for gateway sessions
    pub default_model: String,
    /// Whether to enable the webhook adapter (always-on)
    pub webhook_enabled: bool,
    /// Default group chat policy for platforms without an explicit override.
    pub group_policy: GroupPolicy,
    /// Behavior when an unauthorized user sends a direct message.
    pub unauthorized_dm_behavior: UnauthorizedDmBehavior,
    /// Streaming and tool-progress settings.
    pub streaming: GatewayStreamingConfig,
    /// How to handle a second user message that arrives while the agent is
    /// already processing a prior message for the same session.
    pub second_message_mode: SecondMessageMode,
}

/// Governs how a second incoming message is handled when the agent is already
/// running for that session.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SecondMessageMode {
    /// Queue the message and deliver it after the current response finishes.
    /// This is the default and preserves the original FIFO behaviour.
    #[default]
    Queue,
    /// Inject the message as a `Redirect` `SteeringEvent` into the running
    /// agent loop so it can course-correct mid-turn.
    Steer,
    /// Cancel the running agent immediately and start a fresh turn with the
    /// new message.
    Interrupt,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
            session_idle_timeout_secs: 3600,
            cleanup_interval_secs: 300,
            default_model: "anthropic/claude-sonnet-4-20250514".into(),
            webhook_enabled: true,
            group_policy: GroupPolicy::default(),
            unauthorized_dm_behavior: UnauthorizedDmBehavior::default(),
            streaming: GatewayStreamingConfig::default(),
            second_message_mode: SecondMessageMode::default(),
        }
    }
}

impl GatewayConfig {
    /// Session idle timeout as a Duration.
    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.session_idle_timeout_secs)
    }

    /// Cleanup interval as a Duration.
    pub fn cleanup_interval(&self) -> Duration {
        Duration::from_secs(self.cleanup_interval_secs)
    }

    /// Bind address as "host:port".
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let cfg = GatewayConfig::default();
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.idle_timeout(), Duration::from_secs(3600));
        assert_eq!(cfg.bind_addr(), "127.0.0.1:8080");
    }

    #[test]
    fn custom_config() {
        let cfg = GatewayConfig {
            host: "0.0.0.0".into(),
            port: 9090,
            session_idle_timeout_secs: 600,
            ..Default::default()
        };
        assert_eq!(cfg.bind_addr(), "0.0.0.0:9090");
        assert_eq!(cfg.idle_timeout(), Duration::from_secs(600));
    }
}
