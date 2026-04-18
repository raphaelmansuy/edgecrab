use crate::gateway_catalog::{PlatformDiagnostic, PlatformState};
use edgecrab_core::AppConfig;
#[cfg(test)]
use edgecrab_gateway::channel_directory::{ChannelDirectory, load_directory};

#[cfg(test)]
pub type PendingPairingSummary = (String, String, String, String, u64);
#[cfg(test)]
pub type ApprovedPairingSummary = (String, String, String);

struct DeliveryProfile {
    experience: &'static str,
    attachments: &'static str,
    routing: &'static str,
    max_chars: usize,
}

pub fn render_gateway_home_channel_summary(config: &AppConfig) -> String {
    let entries = [
        (
            "telegram",
            config.gateway.platform_enabled("telegram") || config.gateway.telegram.enabled,
            config.gateway.telegram.home_channel.as_deref(),
        ),
        (
            "discord",
            config.gateway.platform_enabled("discord") || config.gateway.discord.enabled,
            config.gateway.discord.home_channel.as_deref(),
        ),
        (
            "slack",
            config.gateway.platform_enabled("slack") || config.gateway.slack.enabled,
            config.gateway.slack.home_channel.as_deref(),
        ),
    ];

    let mut lines = Vec::new();
    for (platform, enabled, home_channel) in entries {
        if enabled || home_channel.is_some() {
            lines.push(format!(
                "  {platform:<9} {}",
                home_channel.unwrap_or("(not set)")
            ));
        }
    }

    if lines.is_empty() {
        "Gateway homes:\n  No supported home-channel platform is configured yet.\n  Set one with: /sethome <platform> <channel>".into()
    } else {
        format!(
            "Gateway homes:\n{}\n  Set with: /sethome <platform> <channel>  or /sethome <channel> when exactly one supported platform is enabled.",
            lines.join("\n")
        )
    }
}

#[cfg(test)]
pub fn render_gateway_platforms_panel(
    config: &AppConfig,
    diagnostics: &[PlatformDiagnostic],
    gateway_status: Option<&crate::gateway_cmd::GatewayStatus>,
    pending_pairings: &[PendingPairingSummary],
    approved_pairings: &[ApprovedPairingSummary],
) -> String {
    let directory = load_directory();
    let mut ready = Vec::new();
    let mut available = Vec::new();
    let mut attention = Vec::new();
    let mut dormant = Vec::new();

    for diagnostic in diagnostics {
        match diagnostic.state {
            PlatformState::Ready => ready.push(diagnostic),
            PlatformState::Available => available.push(diagnostic),
            PlatformState::Incomplete => attention.push(diagnostic),
            PlatformState::NotConfigured => dormant.push(diagnostic),
        }
    }

    let mut text = String::from("Gateway control\n");
    text.push_str(&format_gateway_runtime_summary(
        config,
        diagnostics,
        gateway_status,
    ));
    text.push_str("\n\n");
    text.push_str(&render_gateway_home_channel_summary(config));
    text.push_str("\n\n");
    text.push_str(&format_gateway_pairing_summary(
        pending_pairings,
        approved_pairings,
    ));

    append_gateway_platform_group(&mut text, "Ready now", &ready, &directory);
    append_gateway_platform_group(&mut text, "Needs attention", &attention, &directory);
    append_gateway_platform_group(&mut text, "Ready to enable", &available, &directory);
    append_gateway_platform_group(&mut text, "Not configured yet", &dormant, &directory);

    text.push_str("\n\nNext steps\n");
    if gateway_status.is_some_and(|status| status.running) {
        text.push_str("  edgecrab gateway restart     apply config changes cleanly\n");
        text.push_str("  edgecrab gateway stop        stop the background gateway\n");
    } else {
        text.push_str("  edgecrab gateway start       launch the background gateway\n");
    }
    text.push_str("  edgecrab gateway status      inspect runtime health and logs\n");
    text.push_str("  edgecrab gateway configure   open the curated setup flow\n");
    text.push_str("  /sethome <platform> <channel>  route proactive messages where they belong");

    text
}

#[cfg(test)]
fn format_gateway_runtime_summary(
    config: &AppConfig,
    diagnostics: &[PlatformDiagnostic],
    gateway_status: Option<&crate::gateway_cmd::GatewayStatus>,
) -> String {
    let process = match gateway_status {
        Some(status) if status.running => match status.pid {
            Some(pid) => format!("running (pid {pid})"),
            None => "running".into(),
        },
        Some(status) if status.stale_pid => "stopped (stale pid cleaned)".into(),
        Some(_) => "stopped".into(),
        None => "unknown".into(),
    };
    let active_routes = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.active)
        .count();
    let attention_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.state == PlatformState::Incomplete)
        .count();
    let log_path = gateway_status
        .map(|status| status.log_path.display().to_string())
        .unwrap_or_else(|| {
            edgecrab_core::edgecrab_home()
                .join("logs")
                .join("gateway.log")
                .display()
                .to_string()
        });

    format!(
        "Runtime\n  Process: {process}\n  Bind: http://{}:{}\n  Active routes: {active_routes}\n  Needs attention: {attention_count}\n  Log: {log_path}",
        config.gateway.host, config.gateway.port
    )
}

#[cfg(test)]
fn format_gateway_pairing_summary(
    pending_pairings: &[PendingPairingSummary],
    approved_pairings: &[ApprovedPairingSummary],
) -> String {
    let mut text = format!(
        "Pairing\n  Pending DM approvals: {}\n  Approved identities: {}",
        pending_pairings.len(),
        approved_pairings.len()
    );

    if !pending_pairings.is_empty() {
        let mut pending = pending_pairings.to_vec();
        pending.sort_by_key(|entry| std::cmp::Reverse(entry.4));
        for (platform, code, user_id, user_name, age_minutes) in pending.into_iter().take(3) {
            let label = if user_name.trim().is_empty() {
                user_id
            } else {
                format!("{user_name} ({user_id})")
            };
            text.push_str(&format!(
                "\n  Pending: {platform}/{label}  code {code}  {age_minutes}m old"
            ));
        }
    }

    text
}

#[cfg(test)]
fn append_gateway_platform_group(
    text: &mut String,
    heading: &str,
    diagnostics: &[&PlatformDiagnostic],
    directory: &ChannelDirectory,
) {
    if diagnostics.is_empty() {
        return;
    }

    text.push_str(&format!("\n\n{heading}\n"));
    for diagnostic in diagnostics {
        text.push_str(&format!("{}\n", format_gateway_platform_line(diagnostic)));
        text.push_str(&format!(
            "    Delivery: {}\n",
            format_gateway_delivery_line(diagnostic.id)
        ));
        let discovered = discovered_target_count(directory, diagnostic.id);
        if discovered > 0 {
            text.push_str(&format!("    Targets: {discovered} known route(s)\n"));
        }
        if let Some(next_step) = gateway_platform_next_step(diagnostic) {
            text.push_str(&format!("    Next: {next_step}\n"));
        }
    }
    text.truncate(text.trim_end_matches('\n').len());
}

#[cfg(test)]
fn format_gateway_platform_line(diagnostic: &PlatformDiagnostic) -> String {
    let marker = match diagnostic.state {
        PlatformState::Ready => "✓",
        PlatformState::Available => "○",
        PlatformState::Incomplete => "!",
        PlatformState::NotConfigured => "·",
    };
    format!("  {marker} {:<12} {}", diagnostic.name, diagnostic.detail)
}

pub(crate) fn format_gateway_delivery_line(platform_id: &str) -> String {
    let profile = delivery_profile(platform_id);
    format!(
        "{}; {}; {}; {} chars max",
        profile.experience, profile.attachments, profile.routing, profile.max_chars
    )
}

#[cfg(test)]
fn discovered_target_count(directory: &ChannelDirectory, platform_id: &str) -> usize {
    directory
        .platforms
        .get(platform_id)
        .map(|entries| entries.len())
        .unwrap_or(0)
}

pub(crate) fn gateway_platform_next_step(diagnostic: &PlatformDiagnostic) -> Option<String> {
    match diagnostic.state {
        PlatformState::Incomplete => Some(match diagnostic.id {
            "whatsapp" => {
                "Run `edgecrab whatsapp`, finish QR pairing, then restart the gateway.".into()
            }
            "api_server" => "Set `API_SERVER_ENABLED=true` to activate the API surface.".into(),
            _ if !diagnostic.missing_required.is_empty() => format!(
                "Set {} and rerun `edgecrab gateway configure`.",
                diagnostic.missing_required.join(", ")
            ),
            _ => "Complete setup in `edgecrab gateway configure`.".into(),
        }),
        PlatformState::Available => Some(match diagnostic.id {
            "telegram" | "discord" | "slack" => {
                "Enable it in config, then set a home channel if you want proactive delivery."
                    .into()
            }
            _ => "Enable it in config to make it live in the gateway.".into(),
        }),
        PlatformState::NotConfigured => {
            Some("Wire credentials only if this surface matters.".into())
        }
        PlatformState::Ready => None,
    }
}

fn delivery_profile(platform_id: &str) -> DeliveryProfile {
    match platform_id {
        "telegram" => DeliveryProfile {
            experience: "live edits + typing keepalive",
            attachments: "images, files, native voice notes",
            routing: "DMs, groups, forum topics",
            max_chars: 4096,
        },
        "discord" => DeliveryProfile {
            experience: "live edits + typing keepalive",
            attachments: "images, files, native voice uploads",
            routing: "channels, threads, DMs",
            max_chars: 2000,
        },
        "slack" => DeliveryProfile {
            experience: "live edits",
            attachments: "images and files",
            routing: "channels and threads",
            max_chars: 39_000,
        },
        "feishu" => DeliveryProfile {
            experience: "live edits",
            attachments: "images and files",
            routing: "users, chats, threads",
            max_chars: 8000,
        },
        "whatsapp" => DeliveryProfile {
            experience: "final-message delivery",
            attachments: "images, files, voice notes",
            routing: "DMs and chats",
            max_chars: 65_536,
        },
        "signal" => DeliveryProfile {
            experience: "final-message delivery",
            attachments: "images and files",
            routing: "DMs and groups",
            max_chars: 8000,
        },
        "email" => DeliveryProfile {
            experience: "full-message delivery",
            attachments: "files and inline text",
            routing: "inboxes and threads",
            max_chars: 50_000,
        },
        "sms" => DeliveryProfile {
            experience: "single text delivery",
            attachments: "text only",
            routing: "phone numbers",
            max_chars: 1600,
        },
        "matrix" => DeliveryProfile {
            experience: "final-message delivery",
            attachments: "images and files",
            routing: "rooms and threads",
            max_chars: 4000,
        },
        "mattermost" => DeliveryProfile {
            experience: "final-message delivery",
            attachments: "images and files",
            routing: "channels and DMs",
            max_chars: 4000,
        },
        "dingtalk" => DeliveryProfile {
            experience: "final-message delivery",
            attachments: "images only",
            routing: "1:1 and group chats",
            max_chars: 6000,
        },
        "wecom" => DeliveryProfile {
            experience: "final-message delivery",
            attachments: "images and files",
            routing: "users and groups",
            max_chars: 4000,
        },
        "homeassistant" => DeliveryProfile {
            experience: "full-message delivery",
            attachments: "text only",
            routing: "configured notify surfaces",
            max_chars: 10_000,
        },
        "api_server" => DeliveryProfile {
            experience: "client-controlled delivery",
            attachments: "JSON payloads",
            routing: "OpenAI-compatible HTTP clients",
            max_chars: 100_000,
        },
        "webhook" => DeliveryProfile {
            experience: "web callback delivery",
            attachments: "text only",
            routing: "HTTP webhook consumers",
            max_chars: 65_536,
        },
        _ => DeliveryProfile {
            experience: "final-message delivery",
            attachments: "text only",
            routing: "platform-native routes",
            max_chars: 4000,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_catalog::collect_platform_diagnostics;

    #[test]
    fn delivery_line_highlights_live_editing_platforms() {
        assert!(format_gateway_delivery_line("telegram").contains("live edits"));
        assert!(format_gateway_delivery_line("discord").contains("typing keepalive"));
        assert!(format_gateway_delivery_line("whatsapp").contains("final-message delivery"));
    }

    #[test]
    fn panel_includes_delivery_semantics() {
        let mut config = AppConfig::default();
        config.gateway.enable_platform("telegram");
        let diagnostics = collect_platform_diagnostics(&config);
        let panel = render_gateway_platforms_panel(&config, &diagnostics, None, &[], &[]);
        assert!(panel.contains("Delivery:"));
        assert!(panel.contains("DMs, groups, forum topics"));
        assert!(panel.contains("4096 chars max"));
    }
}
