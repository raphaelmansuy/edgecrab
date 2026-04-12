//! API modes, platform identifiers, and constants.

use serde::{Deserialize, Serialize};

/// Default model when none is specified.
pub const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4-20250514";
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// API protocol variant — determines how requests/responses are shaped.
///
/// ```text
///   ChatCompletions   ── OpenAI / OpenRouter standard
///   AnthropicMessages ── Direct Anthropic API
///   CodexResponses    ── OpenAI Codex Responses API
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiMode {
    ChatCompletions,
    AnthropicMessages,
    CodexResponses,
}

impl ApiMode {
    /// Auto-detect API mode from base URL and model name.
    pub fn detect(base_url: &str, model: &str) -> Self {
        if base_url.contains("api.anthropic.com") {
            ApiMode::AnthropicMessages
        } else if base_url.contains("api.openai.com") && model.contains("codex") {
            ApiMode::CodexResponses
        } else {
            ApiMode::ChatCompletions
        }
    }
}

/// Platform the agent is running on — affects prompt hints and tool availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[default]
    Cli,
    Telegram,
    Discord,
    Slack,
    Whatsapp,
    Feishu,
    Wecom,
    Signal,
    Email,
    Matrix,
    Mattermost,
    DingTalk,
    Sms,
    Webhook,
    Api,
    HomeAssistant,
    Acp,
    /// Scheduled cron job — no interactive user present.
    Cron,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Platform::Cli => "cli",
            Platform::Telegram => "telegram",
            Platform::Discord => "discord",
            Platform::Slack => "slack",
            Platform::Whatsapp => "whatsapp",
            Platform::Feishu => "feishu",
            Platform::Wecom => "wecom",
            Platform::Signal => "signal",
            Platform::Email => "email",
            Platform::Matrix => "matrix",
            Platform::Mattermost => "mattermost",
            Platform::DingTalk => "dingtalk",
            Platform::Sms => "sms",
            Platform::Webhook => "webhook",
            Platform::Api => "api",
            Platform::HomeAssistant => "homeassistant",
            Platform::Acp => "acp",
            Platform::Cron => "cron",
        };
        write!(f, "{s}")
    }
}

/// Origin chat metadata for gateway-backed sessions.
///
/// WHY a named struct: `(String, String)` obscures which value is the platform
/// name and which is the chat identifier. A shared value type makes call sites
/// self-documenting across edgecrab-core, edgecrab-tools, and edgecrab-gateway.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OriginChat {
    pub platform: String,
    pub chat_id: String,
}

impl OriginChat {
    pub fn new(platform: impl Into<String>, chat_id: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            chat_id: chat_id.into(),
        }
    }

    pub fn session_key(&self) -> String {
        format!("{}:{}", self.platform, self.chat_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_mode_detect_anthropic() {
        assert_eq!(
            ApiMode::detect("https://api.anthropic.com/v1", "claude-4"),
            ApiMode::AnthropicMessages
        );
    }

    #[test]
    fn api_mode_detect_codex() {
        assert_eq!(
            ApiMode::detect("https://api.openai.com/v1", "codex-mini"),
            ApiMode::CodexResponses
        );
    }

    #[test]
    fn api_mode_detect_default() {
        assert_eq!(
            ApiMode::detect("https://openrouter.ai/api/v1", "anthropic/claude-4"),
            ApiMode::ChatCompletions
        );
    }

    #[test]
    fn platform_display() {
        assert_eq!(format!("{}", Platform::Cli), "cli");
        assert_eq!(format!("{}", Platform::Telegram), "telegram");
    }

    #[test]
    fn platform_serde_roundtrip() {
        for p in [
            Platform::Cli,
            Platform::Telegram,
            Platform::Discord,
            Platform::Slack,
            Platform::Feishu,
            Platform::Wecom,
        ] {
            let json = serde_json::to_string(&p).expect("serialize");
            let deser: Platform = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(p, deser);
        }
    }

    #[test]
    fn origin_chat_session_key() {
        let origin = OriginChat::new("telegram", "chat-123");
        assert_eq!(origin.session_key(), "telegram:chat-123");
    }

    #[test]
    fn origin_chat_serde_roundtrip() {
        let origin = OriginChat::new("discord", "chan-456");
        let json = serde_json::to_string(&origin).expect("serialize");
        let deser: OriginChat = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(origin, deser);
    }
}
