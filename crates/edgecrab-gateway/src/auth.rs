//! # Gateway authorization — centralized access control
//!
//! Single source of truth for:
//! - Platform → env var mapping (DRY, used by both setup and runtime)
//! - User authorization (secure-by-default: deny when no allowlist configured)
//! - Group policy enforcement
//! - Unauthorized DM behavior dispatch
//! - Security posture analysis for config review screens
//!
//! Design: Open/Closed — new platforms only need an entry in `platform_env_prefix()`.

use edgecrab_core::config::{GroupPolicy, UnauthorizedDmBehavior};
use edgecrab_types::Platform;
use tracing;

use crate::pairing::PairingStore;
use crate::platform::ChatType;

/// Map a platform to its env-var prefix for allowlist/auth variables.
///
/// This is the **single source of truth** for all `{PLATFORM}_ALLOWED_USERS`,
/// `{PLATFORM}_ALLOW_ALL_USERS` env var lookups. Adding a new platform
/// requires only one new arm here.
pub fn platform_env_prefix(platform: Platform) -> Option<&'static str> {
    match platform {
        Platform::Telegram => Some("TELEGRAM"),
        Platform::Discord => Some("DISCORD"),
        Platform::Slack => Some("SLACK"),
        Platform::Whatsapp => Some("WHATSAPP"),
        Platform::Signal => Some("SIGNAL"),
        Platform::Email => Some("EMAIL"),
        Platform::Sms => Some("SMS"),
        Platform::Matrix => Some("MATRIX"),
        Platform::Mattermost => Some("MATTERMOST"),
        Platform::DingTalk => Some("DINGTALK"),
        Platform::Feishu => Some("FEISHU"),
        Platform::Wecom => Some("WECOM"),
        Platform::HomeAssistant => Some("HA"),
        Platform::BlueBubbles => Some("BLUEBUBBLES"),
        Platform::Weixin => Some("WEIXIN"),
        // These platforms have no user allowlist concept:
        Platform::Webhook | Platform::Api | Platform::Cli | Platform::Acp | Platform::Cron => None,
    }
}

/// Result of an authorization check with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthResult {
    /// Access granted.
    Allowed(AuthReason),
    /// Access denied.
    Denied(AuthReason),
}

/// Why the authorization decision was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthReason {
    /// Platform bypasses auth entirely (Webhook, HomeAssistant, etc.)
    PlatformBypass,
    /// `GATEWAY_ALLOW_ALL_USERS=true`
    GlobalAllowAll,
    /// `{PLATFORM}_ALLOW_ALL_USERS=true`
    PlatformAllowAll,
    /// User found in pairing store (approved via `/approve`)
    PairingApproved,
    /// User found in per-platform or global allowlist
    Allowlisted,
    /// Explicit opt-in `GATEWAY_ALLOW_ALL_USERS=true` with no allowlists
    ExplicitOpenAccess,
    /// No allowlists configured and no explicit open-access → default deny
    NoAllowlistDeny,
    /// User not found in any allowlist
    NotInAllowlist,
    /// Group messages are disabled by policy
    GroupPolicyDeny,
}

impl AuthResult {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed(_))
    }
}

/// Check whether an incoming message should be processed.
///
/// Authorization rules (first match wins):
/// 1. System platforms (Webhook, HomeAssistant, Cron) → ALLOW (bypass)
/// 2. Group policy check → DENY if group messages are disabled
/// 3. `GATEWAY_ALLOW_ALL_USERS=true|1|yes` → ALLOW everyone
/// 4. `{PLATFORM}_ALLOW_ALL_USERS=true|1|yes` → ALLOW everyone on that platform
/// 5. Pairing store → ALLOW if user is approved
/// 6. `GATEWAY_ALLOWED_USERS` / `{PLATFORM}_ALLOWED_USERS` → ALLOW if listed
/// 7. If **no** allowlist configured → DENY (secure by default)
///
/// This replaces the old `is_user_authorized()` which defaulted to ALLOW
/// when no allowlists were configured (insecure).
pub fn check_authorization(
    platform: Platform,
    user_id: &str,
    chat_type: ChatType,
    group_policy: GroupPolicy,
    pairing_store: Option<&PairingStore>,
) -> AuthResult {
    // 1. System platform bypass
    if matches!(
        platform,
        Platform::Webhook | Platform::HomeAssistant | Platform::Cron | Platform::Api
    ) {
        return AuthResult::Allowed(AuthReason::PlatformBypass);
    }

    // 1b. WhatsApp self-chat bypass: in self-chat mode the bridge only
    // delivers messages from the account owner to themselves, so every
    // message that reaches us is already implicitly authorised.
    if platform == Platform::Whatsapp
        && std::env::var("WHATSAPP_MODE").as_deref() == Ok("self-chat")
    {
        return AuthResult::Allowed(AuthReason::PlatformBypass);
    }

    // 1c. Generic per-platform self-chat bypass.
    //
    // Any chat platform can be put into "self-chat" (personal-bot) mode by
    // setting `{PREFIX}_SELF_CHAT=true` in the environment.  In that mode the
    // operator guarantees that only their own messages will reach the bot
    // (e.g. they are the sole user of a personal Telegram bot, Signal "Note
    // to Self" linked-device messages, or a private Discord bot).
    //
    // Signal: set SIGNAL_SELF_CHAT=true — the signal.rs adapter already
    //   filters to source_device=1 (primary/linked-device self-messages).
    // Telegram/Discord/Slack/etc.: set {PREFIX}_SELF_CHAT=true when running
    //   as a single-owner personal bot.
    if let Some(prefix) = platform_env_prefix(platform)
        && is_env_truthy(&format!("{prefix}_SELF_CHAT"))
    {
        return AuthResult::Allowed(AuthReason::PlatformBypass);
    }

    // 2. Group policy check (before auth, groups are a separate policy axis)
    if chat_type == ChatType::Group || chat_type == ChatType::Channel {
        match group_policy {
            GroupPolicy::Disabled => return AuthResult::Denied(AuthReason::GroupPolicyDeny),
            // MentionOnly is checked at the adapter level (platform-specific @mention parsing).
            // If we reach here, the message was already @mention-filtered or the adapter
            // doesn't support mention filtering, so we proceed to user auth.
            GroupPolicy::MentionOnly | GroupPolicy::AllowedOnly | GroupPolicy::Open => {}
        }
    }

    // 3. Global allow-all override
    if is_env_truthy("GATEWAY_ALLOW_ALL_USERS") {
        return AuthResult::Allowed(AuthReason::GlobalAllowAll);
    }

    // 4. Per-platform allow-all override
    if let Some(prefix) = platform_env_prefix(platform) {
        let var = format!("{prefix}_ALLOW_ALL_USERS");
        if is_env_truthy(&var) {
            return AuthResult::Allowed(AuthReason::PlatformAllowAll);
        }
    }

    // 5. Pairing store check
    if let Some(store) = pairing_store {
        let platform_name = platform.to_string();
        if store.is_approved(&platform_name, user_id) {
            return AuthResult::Allowed(AuthReason::PairingApproved);
        }
    }

    // 6. Collect allowlists from env vars
    let global_list = std::env::var("GATEWAY_ALLOWED_USERS").unwrap_or_default();
    let platform_list = platform_env_prefix(platform)
        .map(|prefix| std::env::var(format!("{prefix}_ALLOWED_USERS")).unwrap_or_default())
        .unwrap_or_default();

    let has_global = !global_list.trim().is_empty();
    let has_platform = !platform_list.trim().is_empty();

    if !has_global && !has_platform {
        // 7. No allowlists configured → DENY (secure by default).
        // Operators must explicitly set GATEWAY_ALLOW_ALL_USERS=true for open access.
        return AuthResult::Denied(AuthReason::NoAllowlistDeny);
    }

    // Check membership in allowlists
    let is_listed = global_list
        .split(',')
        .chain(platform_list.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .any(|allowed| allowed == user_id || allowed == "*");

    if is_listed {
        AuthResult::Allowed(AuthReason::Allowlisted)
    } else {
        AuthResult::Denied(AuthReason::NotInAllowlist)
    }
}

/// Determine what to do when an unauthorized user sends a DM.
pub fn unauthorized_dm_response(
    behavior: UnauthorizedDmBehavior,
    chat_type: ChatType,
    platform: Platform,
    user_id: &str,
    user_name: &str,
    pairing_store: &PairingStore,
) -> Option<String> {
    // Never send pairing codes to groups — silent deny
    if chat_type != ChatType::Dm {
        return None;
    }

    match behavior {
        UnauthorizedDmBehavior::Ignore => {
            tracing::info!(
                platform = ?platform,
                user = %user_id,
                "auth audit: unauthorized DM silently ignored"
            );
            None
        }
        UnauthorizedDmBehavior::Reject => {
            tracing::info!(
                platform = ?platform,
                user = %user_id,
                "auth audit: unauthorized DM rejected"
            );
            Some("⛔ Unauthorized. Contact the bot administrator.".into())
        }
        UnauthorizedDmBehavior::Pair => {
            let platform_name = platform.to_string();
            match pairing_store.generate_code(&platform_name, user_id, user_name) {
                Some(code) => {
                    tracing::info!(
                        platform = ?platform,
                        user = %user_id,
                        "auth audit: pairing code generated for unauthorized user"
                    );
                    Some(format!(
                        "👋 I don't recognize you yet.\n\n\
                         Your pairing code: `{code}`\n\n\
                         Ask the bot owner to run:\n\
                         `edgecrab gateway approve {platform_name} {code}`\n\n\
                         This code expires in 1 hour."
                    ))
                }
                None => {
                    tracing::warn!(
                        platform = ?platform,
                        user = %user_id,
                        "auth audit: pairing code rate-limited or locked out"
                    );
                    None
                }
            }
        }
    }
}

// ─── Security Posture Analysis ────────────────────────────────────────

/// Access control mode for a platform, derived from current env/config state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    /// No restrictions — anyone can use the bot
    Open,
    /// Restricted to specific user IDs
    Allowlisted,
    /// Only self-chat (WhatsApp self-chat mode)
    SelfOnly,
    /// No access configured (will be denied by default)
    Unconfigured,
}

impl AccessMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Allowlisted => "allowlist",
            Self::SelfOnly => "self-only",
            Self::Unconfigured => "deny-all",
        }
    }

    pub fn is_secure(self) -> bool {
        !matches!(self, Self::Open)
    }
}

/// Per-platform security posture for display in config review.
#[derive(Debug, Clone)]
pub struct PlatformSecurityPosture {
    pub platform: String,
    pub access_mode: AccessMode,
    pub group_policy: GroupPolicy,
    pub allowlisted_count: usize,
    pub paired_count: usize,
    pub warnings: Vec<String>,
}

/// Analyze security posture for a specific platform.
pub fn analyze_platform_security(
    platform_id: &str,
    group_policy: GroupPolicy,
    pairing_store: Option<&PairingStore>,
) -> PlatformSecurityPosture {
    let prefix = match platform_id {
        "telegram" => "TELEGRAM",
        "discord" => "DISCORD",
        "slack" => "SLACK",
        "whatsapp" => "WHATSAPP",
        "signal" => "SIGNAL",
        "email" => "EMAIL",
        "sms" => "SMS",
        "matrix" => "MATRIX",
        "mattermost" => "MATTERMOST",
        "dingtalk" => "DINGTALK",
        "feishu" => "FEISHU",
        "wecom" => "WECOM",
        "homeassistant" => "HA",
        _ => "",
    };

    let mut warnings = Vec::new();

    // Determine access mode
    let global_allow_all = is_env_truthy("GATEWAY_ALLOW_ALL_USERS");
    let platform_allow_all = if !prefix.is_empty() {
        is_env_truthy(&format!("{prefix}_ALLOW_ALL_USERS"))
    } else {
        false
    };

    let global_list = std::env::var("GATEWAY_ALLOWED_USERS").unwrap_or_default();
    let platform_list = if !prefix.is_empty() {
        std::env::var(format!("{prefix}_ALLOWED_USERS")).unwrap_or_default()
    } else {
        String::new()
    };

    let allowlisted_count = global_list
        .split(',')
        .chain(platform_list.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .len();

    let paired_count = pairing_store
        .map(|store| store.list_approved(Some(platform_id)).len())
        .unwrap_or(0);

    let access_mode = if global_allow_all || platform_allow_all {
        warnings.push(format!(
            "⚠ {platform_id} has OPEN access — anyone can use the bot!"
        ));
        AccessMode::Open
    } else if platform_id == "whatsapp" {
        // Check if self-chat mode
        let mode = std::env::var("WHATSAPP_MODE").unwrap_or_default();
        if mode == "self-chat" {
            AccessMode::SelfOnly
        } else if allowlisted_count > 0 || paired_count > 0 {
            AccessMode::Allowlisted
        } else {
            AccessMode::Unconfigured
        }
    } else if !prefix.is_empty() && is_env_truthy(&format!("{prefix}_SELF_CHAT")) {
        // Generic per-platform self-chat / personal-bot mode
        AccessMode::SelfOnly
    } else if allowlisted_count > 0 || paired_count > 0 {
        AccessMode::Allowlisted
    } else {
        warnings.push(format!(
            "⚠ {platform_id} has no access restrictions — all users will be denied by default."
        ));
        warnings.push(format!("  Run: edgecrab gateway configure {platform_id}"));
        AccessMode::Unconfigured
    };

    // Group policy warnings
    if group_policy == GroupPolicy::Open {
        warnings.push(format!(
            "⚠ {platform_id} group policy is OPEN — any authorized user in any group can trigger the bot."
        ));
    }

    PlatformSecurityPosture {
        platform: match platform_id {
            "telegram" => "Telegram".into(),
            "discord" => "Discord".into(),
            "slack" => "Slack".into(),
            "whatsapp" => "WhatsApp".into(),
            "signal" => "Signal".into(),
            "email" => "Email".into(),
            "sms" => "SMS".into(),
            "matrix" => "Matrix".into(),
            "mattermost" => "Mattermost".into(),
            "dingtalk" => "DingTalk".into(),
            "feishu" => "Feishu".into(),
            "wecom" => "WeCom".into(),
            "homeassistant" => "Home Asst".into(),
            _ => platform_id.to_string(),
        },
        access_mode,
        group_policy,
        allowlisted_count,
        paired_count,
        warnings,
    }
}

/// Format a security posture table for display.
pub fn format_security_review(postures: &[PlatformSecurityPosture]) -> String {
    let mut lines = Vec::new();
    let mut has_warnings = false;

    lines.push(String::new());
    lines.push("  ┌─────────────┬──────────────┬─────────────┬─────────────────────┐".into());
    lines.push("  │ Platform    │ Access       │ Groups      │ Users               │".into());
    lines.push("  ├─────────────┼──────────────┼─────────────┼─────────────────────┤".into());

    for p in postures {
        let access_icon = if p.access_mode.is_secure() {
            "✓"
        } else {
            "⚠"
        };
        let access = format!("{} {}", access_icon, p.access_mode.label());

        let users = if p.allowlisted_count > 0 || p.paired_count > 0 {
            let mut parts = Vec::new();
            if p.allowlisted_count > 0 {
                parts.push(format!("{} listed", p.allowlisted_count));
            }
            if p.paired_count > 0 {
                parts.push(format!("{} paired", p.paired_count));
            }
            parts.join(", ")
        } else if p.access_mode == AccessMode::SelfOnly {
            "self-chat only".into()
        } else if p.access_mode == AccessMode::Open {
            "unrestricted".into()
        } else {
            "none".into()
        };

        lines.push(format!(
            "  │ {:<11} │ {:<12} │ {:<11} │ {:<19} │",
            p.platform, access, p.group_policy, users
        ));

        if !p.warnings.is_empty() {
            has_warnings = true;
        }
    }

    lines.push("  └─────────────┴──────────────┴─────────────┴─────────────────────┘".into());

    if has_warnings {
        lines.push(String::new());
        for p in postures {
            for w in &p.warnings {
                lines.push(format!("  {w}"));
            }
        }
    }

    lines.join("\n")
}

// ─── Helpers ──────────────────────────────────────────────────────────

fn is_env_truthy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|v| matches!(v.to_ascii_lowercase().trim(), "true" | "1" | "yes"))
}

// ─── Platform ID Format Validation ────────────────────────────────────

/// Validate a user ID against the expected format for a platform.
/// Returns `None` if valid, or a warning message if the format looks wrong.
pub fn validate_user_id_format(platform_id: &str, user_id: &str) -> Option<String> {
    let trimmed = user_id.trim();
    if trimmed.is_empty() {
        return Some("User ID cannot be empty".into());
    }

    match platform_id {
        "telegram" => {
            if !trimmed.chars().all(|c| c.is_ascii_digit()) {
                Some(format!(
                    "Telegram user IDs are numeric (e.g., 123456789), got: {trimmed}"
                ))
            } else {
                None
            }
        }
        "discord" => {
            // Discord snowflakes: 17-20 digit numbers
            if !trimmed.chars().all(|c| c.is_ascii_digit()) || trimmed.len() < 17 {
                Some(format!(
                    "Discord user IDs are 17-20 digit snowflakes (e.g., 123456789012345678), got: {trimmed}"
                ))
            } else {
                None
            }
        }
        "slack" => {
            // Slack member IDs: U or W followed by alphanumeric
            if !trimmed.starts_with('U') && !trimmed.starts_with('W') {
                Some(format!(
                    "Slack member IDs start with U or W (e.g., U0123456789), got: {trimmed}"
                ))
            } else {
                None
            }
        }
        "signal" | "sms" | "whatsapp" => {
            // E.164 phone numbers: + followed by 7-15 digits
            if !trimmed.starts_with('+') || !trimmed[1..].chars().all(|c| c.is_ascii_digit()) {
                Some(format!(
                    "Use E.164 phone format (e.g., +15551234567), got: {trimmed}"
                ))
            } else {
                None
            }
        }
        "matrix" => {
            // Matrix user IDs: @user:server
            if !trimmed.starts_with('@') || !trimmed.contains(':') {
                Some(format!(
                    "Matrix user IDs use @user:server format (e.g., @bot:matrix.org), got: {trimmed}"
                ))
            } else {
                None
            }
        }
        "email" => {
            if !trimmed.contains('@') {
                Some(format!(
                    "Email addresses must contain @ (e.g., user@example.com), got: {trimmed}"
                ))
            } else {
                None
            }
        }
        _ => None, // No validation for other platforms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_env_prefix_covers_all_chat_platforms() {
        assert_eq!(platform_env_prefix(Platform::Telegram), Some("TELEGRAM"));
        assert_eq!(platform_env_prefix(Platform::Discord), Some("DISCORD"));
        assert_eq!(platform_env_prefix(Platform::Slack), Some("SLACK"));
        assert_eq!(platform_env_prefix(Platform::Whatsapp), Some("WHATSAPP"));
        assert_eq!(platform_env_prefix(Platform::Signal), Some("SIGNAL"));
        assert_eq!(platform_env_prefix(Platform::Email), Some("EMAIL"));
        assert_eq!(platform_env_prefix(Platform::Sms), Some("SMS"));
        assert_eq!(platform_env_prefix(Platform::Matrix), Some("MATRIX"));
        assert_eq!(
            platform_env_prefix(Platform::Mattermost),
            Some("MATTERMOST")
        );
        assert_eq!(platform_env_prefix(Platform::DingTalk), Some("DINGTALK"));
        assert_eq!(platform_env_prefix(Platform::Feishu), Some("FEISHU"));
        assert_eq!(platform_env_prefix(Platform::Wecom), Some("WECOM"));
        assert_eq!(platform_env_prefix(Platform::HomeAssistant), Some("HA"));
    }

    #[test]
    fn non_chat_platforms_have_no_prefix() {
        assert_eq!(platform_env_prefix(Platform::Webhook), None);
        assert_eq!(platform_env_prefix(Platform::Api), None);
        assert_eq!(platform_env_prefix(Platform::Cli), None);
        assert_eq!(platform_env_prefix(Platform::Acp), None);
        assert_eq!(platform_env_prefix(Platform::Cron), None);
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn default_deny_when_no_allowlists() {
        // Ensure no interfering env vars are set for this test
        let _cleanup = TestEnvGuard::new(&[
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "TELEGRAM_ALLOW_ALL_USERS",
            "TELEGRAM_ALLOWED_USERS",
        ]);

        let result = check_authorization(
            Platform::Telegram,
            "123456",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Denied(AuthReason::NoAllowlistDeny));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn global_allow_all_grants_access() {
        let _cleanup = TestEnvGuard::new(&[
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "TELEGRAM_ALLOW_ALL_USERS",
            "TELEGRAM_ALLOWED_USERS",
        ]);
        unsafe { std::env::set_var("GATEWAY_ALLOW_ALL_USERS", "true") };

        let result = check_authorization(
            Platform::Telegram,
            "123456",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Allowed(AuthReason::GlobalAllowAll));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn platform_allowlist_grants_access() {
        let _cleanup = TestEnvGuard::new(&[
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "TELEGRAM_ALLOW_ALL_USERS",
            "TELEGRAM_ALLOWED_USERS",
        ]);
        unsafe { std::env::set_var("TELEGRAM_ALLOWED_USERS", "123456,789012") };

        let allowed = check_authorization(
            Platform::Telegram,
            "123456",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(allowed, AuthResult::Allowed(AuthReason::Allowlisted));

        let denied = check_authorization(
            Platform::Telegram,
            "999999",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(denied, AuthResult::Denied(AuthReason::NotInAllowlist));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn group_policy_disabled_denies_groups() {
        let _cleanup = TestEnvGuard::new(&[
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "TELEGRAM_ALLOW_ALL_USERS",
            "TELEGRAM_ALLOWED_USERS",
        ]);
        unsafe { std::env::set_var("TELEGRAM_ALLOWED_USERS", "123456") };

        let result = check_authorization(
            Platform::Telegram,
            "123456",
            ChatType::Group,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Denied(AuthReason::GroupPolicyDeny));

        // Same user in DM should be allowed
        let dm_result = check_authorization(
            Platform::Telegram,
            "123456",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(dm_result, AuthResult::Allowed(AuthReason::Allowlisted));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn group_policy_open_allows_authorized_users_in_groups() {
        let _cleanup = TestEnvGuard::new(&[
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "TELEGRAM_ALLOW_ALL_USERS",
            "TELEGRAM_ALLOWED_USERS",
        ]);
        unsafe { std::env::set_var("TELEGRAM_ALLOWED_USERS", "123456") };

        let result = check_authorization(
            Platform::Telegram,
            "123456",
            ChatType::Group,
            GroupPolicy::Open,
            None,
        );
        assert_eq!(result, AuthResult::Allowed(AuthReason::Allowlisted));
    }

    #[test]
    fn webhook_bypasses_auth() {
        let result = check_authorization(
            Platform::Webhook,
            "",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Allowed(AuthReason::PlatformBypass));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn whatsapp_self_chat_bypasses_auth_without_allowlist() {
        // WHATSAPP_MODE=self-chat should allow messages through even when no
        // WHATSAPP_ALLOWED_USERS is configured — the bridge itself restricts
        // delivery to the account owner.
        let _cleanup = TestEnvGuard::new(&[
            "WHATSAPP_MODE",
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "WHATSAPP_ALLOW_ALL_USERS",
            "WHATSAPP_ALLOWED_USERS",
        ]);
        unsafe { std::env::set_var("WHATSAPP_MODE", "self-chat") };

        let result = check_authorization(
            Platform::Whatsapp,
            "1234567890",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Allowed(AuthReason::PlatformBypass));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn whatsapp_bot_mode_still_requires_allowlist() {
        // When WHATSAPP_MODE is NOT self-chat (e.g. "bot"), normal auth rules apply.
        let _cleanup = TestEnvGuard::new(&[
            "WHATSAPP_MODE",
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "WHATSAPP_ALLOW_ALL_USERS",
            "WHATSAPP_ALLOWED_USERS",
        ]);
        unsafe { std::env::set_var("WHATSAPP_MODE", "bot") };

        let result = check_authorization(
            Platform::Whatsapp,
            "1234567890",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Denied(AuthReason::NoAllowlistDeny));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn signal_self_chat_bypasses_auth_without_allowlist() {
        let _cleanup = TestEnvGuard::new(&[
            "SIGNAL_SELF_CHAT",
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "SIGNAL_ALLOW_ALL_USERS",
            "SIGNAL_ALLOWED_USERS",
        ]);
        unsafe { std::env::set_var("SIGNAL_SELF_CHAT", "true") };

        let result = check_authorization(
            Platform::Signal,
            "+15551234567",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Allowed(AuthReason::PlatformBypass));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn telegram_self_chat_bypasses_auth_without_allowlist() {
        let _cleanup = TestEnvGuard::new(&[
            "TELEGRAM_SELF_CHAT",
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "TELEGRAM_ALLOW_ALL_USERS",
            "TELEGRAM_ALLOWED_USERS",
        ]);
        unsafe { std::env::set_var("TELEGRAM_SELF_CHAT", "true") };

        let result = check_authorization(
            Platform::Telegram,
            "123456789",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Allowed(AuthReason::PlatformBypass));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn signal_normal_mode_still_requires_allowlist() {
        // Without SIGNAL_SELF_CHAT=true, normal auth rules apply.
        let _cleanup = TestEnvGuard::new(&[
            "SIGNAL_SELF_CHAT",
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "SIGNAL_ALLOW_ALL_USERS",
            "SIGNAL_ALLOWED_USERS",
        ]);
        // Ensure var is not set
        unsafe { std::env::remove_var("SIGNAL_SELF_CHAT") };

        let result = check_authorization(
            Platform::Signal,
            "+15551234567",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Denied(AuthReason::NoAllowlistDeny));
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn pairing_store_grants_access() {
        let temp = tempfile::tempdir().expect("temp dir");
        unsafe { std::env::set_var("EDGECRAB_HOME", temp.path()) };

        let store = PairingStore::new();
        // Generate and approve a code
        let code = store
            .generate_code("telegram", "123456", "alice")
            .expect("should generate code");
        store.approve_code("telegram", &code);

        let _cleanup = TestEnvGuard::new(&[
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "TELEGRAM_ALLOW_ALL_USERS",
            "TELEGRAM_ALLOWED_USERS",
        ]);

        let result = check_authorization(
            Platform::Telegram,
            "123456",
            ChatType::Dm,
            GroupPolicy::Disabled,
            Some(&store),
        );
        assert_eq!(result, AuthResult::Allowed(AuthReason::PairingApproved));

        unsafe { std::env::remove_var("EDGECRAB_HOME") };
    }

    #[test]
    #[serial_test::serial(edgecrab_gateway_env)]
    fn wildcard_allowlist_grants_access() {
        let _cleanup = TestEnvGuard::new(&[
            "GATEWAY_ALLOW_ALL_USERS",
            "GATEWAY_ALLOWED_USERS",
            "TELEGRAM_ALLOW_ALL_USERS",
            "TELEGRAM_ALLOWED_USERS",
        ]);
        unsafe { std::env::set_var("TELEGRAM_ALLOWED_USERS", "*") };

        let result = check_authorization(
            Platform::Telegram,
            "anyone",
            ChatType::Dm,
            GroupPolicy::Disabled,
            None,
        );
        assert_eq!(result, AuthResult::Allowed(AuthReason::Allowlisted));
    }

    #[test]
    fn validate_telegram_user_id() {
        assert!(validate_user_id_format("telegram", "123456789").is_none());
        assert!(validate_user_id_format("telegram", "abc").is_some());
    }

    #[test]
    fn validate_signal_phone_number() {
        assert!(validate_user_id_format("signal", "+15551234567").is_none());
        assert!(validate_user_id_format("signal", "5551234567").is_some());
    }

    #[test]
    fn validate_matrix_mxid() {
        assert!(validate_user_id_format("matrix", "@bot:matrix.org").is_none());
        assert!(validate_user_id_format("matrix", "bot").is_some());
    }

    #[test]
    fn validate_discord_snowflake() {
        assert!(validate_user_id_format("discord", "123456789012345678").is_none());
        assert!(validate_user_id_format("discord", "12345").is_some());
    }

    /// RAII guard that removes env vars on drop.
    struct TestEnvGuard {
        keys: Vec<&'static str>,
    }

    impl TestEnvGuard {
        fn new(keys: &[&'static str]) -> Self {
            for key in keys {
                unsafe { std::env::remove_var(key) };
            }
            Self {
                keys: keys.to_vec(),
            }
        }
    }

    impl Drop for TestEnvGuard {
        fn drop(&mut self) {
            for key in &self.keys {
                unsafe { std::env::remove_var(key) };
            }
        }
    }
}
