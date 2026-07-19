//! Web hub API surface (Hermes `/api/skills/hub/*`) — documented for parity.
//!
//! EdgeCrab's primary surface is CLI + TUI + gateway slash commands. A full
//! dashboard HTTP API is a product decision (Wave C). This module lists the
//! Hermes-shaped routes so doctor/docs can report them as *agent-CLI out of
//! scope* rather than silently claiming parity.

/// Hermes-compatible route catalog (not mounted by default).
pub const WEB_HUB_ROUTES: &[&str] = &[
    "GET  /api/skills/hub/search",
    "GET  /api/skills/hub/browse",
    "GET  /api/skills/hub/inspect",
    "POST /api/skills/hub/install",
    "GET  /api/skills/hub/sources",
    "GET  /api/skills/hub/lock",
];

pub fn format_web_hub_status() -> String {
    let mut out = String::from(
        "Web hub APIs (Wave C): not mounted in the agent CLI by default.\n\
         Use CLI/TUI/slash equivalents instead:\n",
    );
    for route in WEB_HUB_ROUTES {
        out.push_str("  ");
        out.push_str(route);
        out.push('\n');
    }
    out.push_str(
        "Product opt-in: wire these onto gateway `api_server` if a dashboard is required.\n",
    );
    out
}
