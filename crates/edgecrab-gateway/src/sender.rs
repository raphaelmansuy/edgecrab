//! # GatewaySenderBridge — bridges delivery router to the GatewaySender trait
//!
//! WHY this bridge: `edgecrab-tools` defines the `GatewaySender` trait so that
//! tools like `send_message` and the cron delivery system can dispatch outbound
//! messages without depending on `edgecrab-gateway`. This module implements that
//! trait using the gateway's `DeliveryRouter`, keeping the dependency arrow
//! pointing in one direction (gateway depends on tools, not vice-versa).
//!
//! ```text
//!   tick_due_jobs (cron)
//!     └── GatewaySenderBridge (this module)
//!           └── DeliveryRouter
//!                 └── PlatformAdapter (Telegram/Discord/Slack/...)
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use edgecrab_state::SessionDb;
use edgecrab_tools::registry::GatewaySender;
use edgecrab_types::Platform;

use crate::channel_directory::{load_directory, resolve_channel_name};
use crate::delivery::DeliveryRouter;
use crate::mirror::mirror_to_session;
use crate::platform::MessageMetadata;

// ─── GatewaySenderBridge ─────────────────────────────────────────────

/// Wraps an `Arc<DeliveryRouter>` and implements the `GatewaySender` trait.
///
/// Used by the cron scheduler to deliver job output to external platforms
/// without creating a circular crate dependency.
pub struct GatewaySenderBridge {
    router: Arc<DeliveryRouter>,
    state_db: Option<Arc<SessionDb>>,
}

impl GatewaySenderBridge {
    pub fn new(router: Arc<DeliveryRouter>, state_db: Option<Arc<SessionDb>>) -> Self {
        Self { router, state_db }
    }
}

#[async_trait]
impl GatewaySender for GatewaySenderBridge {
    /// Send a message to `recipient` (channel/user ID) on `platform`.
    async fn send_message(
        &self,
        platform: &str,
        recipient: &str,
        message: &str,
    ) -> Result<(), String> {
        let platform_name = platform.trim().to_ascii_lowercase();
        let p = parse_platform(platform)
            .ok_or_else(|| format!("GatewaySenderBridge: unknown platform '{platform}'"))?;
        let target = resolve_target(p, &platform_name, recipient)?;

        let metadata = MessageMetadata {
            channel_id: Some(target.channel_id.clone()),
            thread_id: target.thread_id.clone(),
            ..Default::default()
        };

        self.router
            .deliver(message, p, &metadata)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(db) = &self.state_db {
            let _ = mirror_to_session(
                db,
                &platform_name,
                &target.channel_id,
                message,
                "gateway",
                target.thread_id.as_deref(),
            );
        }

        Ok(())
    }

    /// List the platforms currently registered in the delivery router.
    async fn list_targets(&self) -> Result<Vec<String>, String> {
        let mut targets: Vec<String> = self
            .router
            .list_platforms()
            .into_iter()
            .map(|p| p.to_string())
            .collect();

        let directory = load_directory();
        for (platform, entries) in directory.platforms {
            for entry in entries {
                let label = if entry.name.is_empty() {
                    entry.id
                } else {
                    entry.name
                };
                targets.push(format!("{platform}:{label}"));
            }
        }

        targets.sort();
        targets.dedup();
        Ok(targets)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedTarget {
    pub(crate) channel_id: String,
    pub(crate) thread_id: Option<String>,
}

// ─── Platform string parsing ─────────────────────────────────────────

/// Convert a lowercase platform name string to the typed `Platform` enum.
///
/// WHY separate function: `Platform` is defined in `edgecrab-types` without
/// a `FromStr` impl to keep that crate minimal. We localise the mapping here
/// where it is used.
pub(crate) fn parse_platform(s: &str) -> Option<Platform> {
    match s.to_lowercase().as_str() {
        "telegram" => Some(Platform::Telegram),
        "discord" => Some(Platform::Discord),
        "slack" => Some(Platform::Slack),
        "whatsapp" => Some(Platform::Whatsapp),
        "feishu" => Some(Platform::Feishu),
        "wecom" => Some(Platform::Wecom),
        "signal" => Some(Platform::Signal),
        "email" => Some(Platform::Email),
        "matrix" => Some(Platform::Matrix),
        "mattermost" => Some(Platform::Mattermost),
        "dingtalk" => Some(Platform::DingTalk),
        "sms" => Some(Platform::Sms),
        "webhook" => Some(Platform::Webhook),
        "homeassistant" | "home_assistant" => Some(Platform::HomeAssistant),
        "api" | "api_server" => Some(Platform::Api),
        _ => None,
    }
}

pub(crate) fn resolve_target(
    platform: Platform,
    platform_name: &str,
    recipient: &str,
) -> Result<ResolvedTarget, String> {
    let trimmed = recipient.trim();
    if trimmed.is_empty() {
        let channel_id = resolve_home_channel(platform).ok_or_else(|| {
            format!(
                "No home channel configured for {platform_name}. Set {}_HOME_CHANNEL or pass an explicit recipient.",
                platform_name.to_ascii_uppercase()
            )
        })?;
        if let Some(target) = parse_explicit_target(platform, &channel_id) {
            return Ok(target);
        }
        return Ok(ResolvedTarget {
            channel_id,
            thread_id: None,
        });
    }

    if let Some(target) = parse_explicit_target(platform, trimmed) {
        return Ok(target);
    }

    if let Some(resolved) = resolve_channel_name(platform_name, trimmed) {
        if let Some(target) = parse_explicit_target(platform, &resolved) {
            return Ok(target);
        }
        return Ok(ResolvedTarget {
            channel_id: resolved,
            thread_id: None,
        });
    }

    if looks_like_named_target(trimmed) {
        return Err(format!(
            "Unknown {platform_name} target '{trimmed}'. Call send_message(action='list') first to see available targets."
        ));
    }

    Ok(ResolvedTarget {
        channel_id: trimmed.to_string(),
        thread_id: None,
    })
}

fn looks_like_named_target(value: &str) -> bool {
    value.starts_with('#') || value.contains('/') || value.contains(" (") || value.contains(' ')
}

fn parse_explicit_target(platform: Platform, target: &str) -> Option<ResolvedTarget> {
    let trimmed = target.trim();
    match platform {
        Platform::Telegram => parse_numeric_thread_target(trimmed),
        Platform::Feishu => {
            parse_prefixed_thread_target(trimmed, &["oc_", "ou_", "on_", "chat_", "open_"])
        }
        Platform::Wecom => parse_wecom_target(trimmed),
        _ => {
            if let Some(target) = parse_numeric_thread_target(trimmed) {
                return Some(target);
            }
            if looks_like_named_target(trimmed) {
                return None;
            }
            Some(ResolvedTarget {
                channel_id: trimmed.to_string(),
                thread_id: None,
            })
        }
    }
}

fn parse_wecom_target(target: &str) -> Option<ResolvedTarget> {
    let normalized = normalize_wecom_target(target);
    if normalized.is_empty() || looks_like_named_target(&normalized) {
        return None;
    }
    Some(ResolvedTarget {
        channel_id: normalized,
        thread_id: None,
    })
}

fn normalize_wecom_target(target: &str) -> String {
    let mut value = target.trim();
    if let Some(stripped) = strip_ascii_prefix(value, "wecom:") {
        value = stripped;
    }
    if let Some(stripped) = strip_ascii_prefix(value, "user:") {
        value = stripped;
    } else if let Some(stripped) = strip_ascii_prefix(value, "group:") {
        value = stripped;
    }
    value.trim().to_string()
}

fn strip_ascii_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

fn parse_numeric_thread_target(target: &str) -> Option<ResolvedTarget> {
    if let Some((chat_id, thread_id)) = target.split_once(':') {
        if is_numeric_id(chat_id) && thread_id.chars().all(|c| c.is_ascii_digit()) {
            return Some(ResolvedTarget {
                channel_id: chat_id.to_string(),
                thread_id: Some(thread_id.to_string()),
            });
        }
    }
    if is_numeric_id(target) {
        return Some(ResolvedTarget {
            channel_id: target.to_string(),
            thread_id: None,
        });
    }
    None
}

fn parse_prefixed_thread_target(target: &str, prefixes: &[&str]) -> Option<ResolvedTarget> {
    let (chat_id, thread_id) = match target.split_once(':') {
        Some((chat_id, thread_id)) => (chat_id, Some(thread_id)),
        None => (target, None),
    };
    if prefixes.iter().any(|prefix| chat_id.starts_with(prefix)) {
        return Some(ResolvedTarget {
            channel_id: chat_id.to_string(),
            thread_id: thread_id
                .filter(|thread_id| !thread_id.is_empty())
                .map(str::to_string),
        });
    }
    None
}

fn is_numeric_id(value: &str) -> bool {
    let stripped = value.strip_prefix('-').unwrap_or(value);
    !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit())
}

fn resolve_home_channel(platform: Platform) -> Option<String> {
    let key = match platform {
        Platform::Telegram => "TELEGRAM_HOME_CHANNEL",
        Platform::Discord => "DISCORD_HOME_CHANNEL",
        Platform::Slack => "SLACK_HOME_CHANNEL",
        Platform::Whatsapp => "WHATSAPP_HOME_CHANNEL",
        Platform::Feishu => "FEISHU_HOME_CHANNEL",
        Platform::Wecom => "WECOM_HOME_CHANNEL",
        Platform::Signal => "SIGNAL_HOME_CHANNEL",
        Platform::Email => "EMAIL_HOME_CHANNEL",
        Platform::Matrix => "MATRIX_HOME_CHANNEL",
        Platform::Mattermost => "MATTERMOST_HOME_CHANNEL",
        Platform::DingTalk => "DINGTALK_HOME_CHANNEL",
        Platform::Sms => "SMS_HOME_CHANNEL",
        Platform::Webhook => "WEBHOOK_HOME_CHANNEL",
        Platform::HomeAssistant => "HOMEASSISTANT_HOME_CHANNEL",
        Platform::Api => "API_HOME_CHANNEL",
        Platform::Cli | Platform::Cron | Platform::Acp => return None,
    };
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_platform_known() {
        assert_eq!(parse_platform("telegram"), Some(Platform::Telegram));
        assert_eq!(parse_platform("DISCORD"), Some(Platform::Discord));
        assert_eq!(parse_platform("slack"), Some(Platform::Slack));
        assert_eq!(parse_platform("feishu"), Some(Platform::Feishu));
        assert_eq!(parse_platform("wecom"), Some(Platform::Wecom));
        assert_eq!(parse_platform("email"), Some(Platform::Email));
        assert_eq!(
            parse_platform("homeassistant"),
            Some(Platform::HomeAssistant)
        );
        assert_eq!(
            parse_platform("home_assistant"),
            Some(Platform::HomeAssistant)
        );
        assert_eq!(parse_platform("api"), Some(Platform::Api));
        assert_eq!(parse_platform("api_server"), Some(Platform::Api));
    }

    #[test]
    fn parse_platform_unknown() {
        assert_eq!(parse_platform("unknown_platform"), None);
        assert_eq!(parse_platform(""), None);
        assert_eq!(parse_platform("cron"), None); // Cron is not a delivery target
    }

    #[test]
    fn parse_numeric_thread_target_accepts_telegram_topic() {
        let target = parse_explicit_target(Platform::Telegram, "-100123:17").expect("target");
        assert_eq!(target.channel_id, "-100123");
        assert_eq!(target.thread_id.as_deref(), Some("17"));
    }

    #[test]
    fn parse_name_like_target_is_not_treated_as_explicit() {
        assert!(parse_explicit_target(Platform::Slack, "#engineering").is_none());
        assert!(parse_explicit_target(Platform::Discord, "My Server/general").is_none());
    }

    #[test]
    fn resolve_target_uses_raw_ids_for_email_and_signal() {
        let email = resolve_target(Platform::Email, "email", "alice@example.com").expect("email");
        assert_eq!(email.channel_id, "alice@example.com");

        let signal = resolve_target(Platform::Signal, "signal", "+15551234567").expect("signal");
        assert_eq!(signal.channel_id, "+15551234567");
    }

    #[test]
    fn resolve_target_normalizes_wecom_aliases() {
        let wecom = resolve_target(Platform::Wecom, "wecom", "wecom:user:Alice").expect("wecom");
        assert_eq!(wecom.channel_id, "Alice");

        let group = resolve_target(Platform::Wecom, "wecom", "group:chat-1").expect("group");
        assert_eq!(group.channel_id, "chat-1");
    }
}
