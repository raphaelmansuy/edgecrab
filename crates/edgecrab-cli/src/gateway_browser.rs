use crate::fuzzy_selector::FuzzyItem;
use crate::gateway_catalog::{PlatformDiagnostic, PlatformState};
use crate::gateway_presentation::{format_gateway_delivery_line, gateway_platform_next_step};
use edgecrab_core::AppConfig;
use edgecrab_gateway::channel_directory::load_directory;

#[derive(Clone)]
pub(crate) struct GatewayPlatformEntry {
    pub(crate) diagnostic: PlatformDiagnostic,
    pub(crate) title: String,
    pub(crate) tag: String,
    pub(crate) summary: String,
    pub(crate) detail_view: String,
}

impl FuzzyItem for GatewayPlatformEntry {
    fn primary(&self) -> &str {
        &self.title
    }

    fn secondary(&self) -> &str {
        &self.summary
    }

    fn tag(&self) -> &str {
        &self.tag
    }
}

pub(crate) fn build_gateway_platform_entries(
    config: &AppConfig,
    diagnostics: &[PlatformDiagnostic],
) -> Vec<GatewayPlatformEntry> {
    let directory = load_directory();

    diagnostics
        .iter()
        .map(|diagnostic| {
            let known_targets = directory
                .platforms
                .get(diagnostic.id)
                .map(|entries| entries.len())
                .unwrap_or(0);
            let home_channel = gateway_home_channel(config, diagnostic.id);
            let delivery = format_gateway_delivery_line(diagnostic.id);
            let next_step = gateway_platform_next_step(diagnostic)
                .unwrap_or_else(|| "Configured and ready for delivery.".into());
            let title = format!("{}  {}", state_glyph(diagnostic.state), diagnostic.name);
            let tag = diagnostic.state.label().replace(' ', "-");
            let summary = format!(
                "{} | {} | {}",
                diagnostic.detail,
                delivery,
                if known_targets > 0 {
                    format!("{known_targets} known route(s)")
                } else {
                    "no known routes yet".into()
                }
            );

            let mut detail_lines = vec![
                format!("Platform: {}", diagnostic.name),
                format!("State:    {}", diagnostic.state.label()),
                format!("Status:   {}", diagnostic.detail),
                format!("Delivery: {}", delivery),
            ];
            if let Some(home_channel) = home_channel {
                detail_lines.push(format!("Home:     {home_channel}"));
            }
            if known_targets > 0 {
                detail_lines.push(format!("Routes:   {known_targets} discovered target(s)"));
            }
            if !diagnostic.missing_required.is_empty() {
                detail_lines.push(format!(
                    "Needs:    {}",
                    diagnostic.missing_required.join(", ")
                ));
            }
            detail_lines.push(String::new());
            detail_lines.push("Actions".into());
            detail_lines.push("  Enter  primary setup field".into());
            detail_lines.push("  Space  enable or disable this platform".into());
            if supports_allowlist(diagnostic.id) {
                detail_lines.push("  A      edit allowlist / allowed users".into());
            }
            if supports_home_channel(diagnostic.id) {
                detail_lines.push("  H      edit home channel".into());
            }
            detail_lines.push("  B      edit gateway bind address".into());
            detail_lines.push("  R      refresh runtime and diagnostics".into());
            detail_lines.push("  X      restart the gateway to apply changes".into());
            detail_lines.push(String::new());
            detail_lines.push(format!("Next: {next_step}"));
            detail_lines
                .push("Runtime: `/gateway restart` is available directly from the TUI.".into());

            GatewayPlatformEntry {
                diagnostic: diagnostic.clone(),
                title,
                tag,
                summary,
                detail_view: detail_lines.join("\n"),
            }
        })
        .collect()
}

pub(crate) fn supports_allowlist(platform_id: &str) -> bool {
    matches!(
        platform_id,
        "telegram"
            | "discord"
            | "slack"
            | "signal"
            | "whatsapp"
            | "sms"
            | "matrix"
            | "mattermost"
            | "dingtalk"
            | "homeassistant"
            | "feishu"
            | "wecom"
    ) || platform_id == "email"
}

pub(crate) fn supports_home_channel(platform_id: &str) -> bool {
    matches!(platform_id, "telegram" | "discord" | "slack")
}

fn gateway_home_channel(config: &AppConfig, platform_id: &str) -> Option<String> {
    match platform_id {
        "telegram" => config.gateway.telegram.home_channel.clone(),
        "discord" => config.gateway.discord.home_channel.clone(),
        "slack" => config.gateway.slack.home_channel.clone(),
        _ => None,
    }
}

fn state_glyph(state: PlatformState) -> &'static str {
    match state {
        PlatformState::Ready => "✓",
        PlatformState::Available => "○",
        PlatformState::Incomplete => "!",
        PlatformState::NotConfigured => "·",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_catalog::collect_platform_diagnostics;

    #[test]
    fn browser_entries_surface_delivery_and_actions() {
        let mut config = AppConfig::default();
        config.gateway.enable_platform("telegram");
        let diagnostics = collect_platform_diagnostics(&config);
        let entries = build_gateway_platform_entries(&config, &diagnostics);
        let telegram = entries
            .iter()
            .find(|entry| entry.diagnostic.id == "telegram")
            .expect("telegram entry");
        assert!(telegram.summary.contains("live edits"));
        assert!(telegram.detail_view.contains("Enter  primary setup field"));
        assert!(
            telegram
                .detail_view
                .contains("Space  enable or disable this platform")
        );
        assert!(telegram.detail_view.contains("X      restart the gateway"));
    }
}
