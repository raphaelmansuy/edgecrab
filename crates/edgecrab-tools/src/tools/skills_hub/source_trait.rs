//! SkillSource trait + source_id inventory (DRY router documentation).
//!
//! Concrete HTTP adapters remain in `sources.rs` / `mod.rs`; this module owns
//! the stable `source_id` list and provider/tap routing helpers.

use async_trait::async_trait;

use super::{SkillBundle, SkillMeta};

/// Hermes ∪ EdgeCrab registry source identifiers.
pub const ALL_SOURCE_IDS: &[&str] = &[
    "official",
    "hermes-index",
    "skills-sh",
    "well-known",
    "url",
    "github",
    "clawhub",
    "claude-marketplace",
    "lobehub",
    "browse-sh",
    "agentskills.io",
    "npm",
    "local",
];

/// Abstract skill registry source (SOLID: open for new registries).
///
/// Fetch-only sources (`url`, `local`, `npm`) may return an empty `search` result.
/// Inspect/scan stays on the façade (`preview_install_scan`) — not on this trait.
#[async_trait]
pub trait SkillSource: Send + Sync {
    fn source_id(&self) -> &'static str;
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta>;
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String>;
    fn trust_level_for(&self, _identifier: &str) -> &'static str {
        "community"
    }
}

/// Human catalog lines for `/skills sources`.
pub fn source_id_catalog_lines() -> Vec<String> {
    vec![
        "official — local/embedded optional-skills (official/<cat>/<name>)".into(),
        "hermes-index / unified-index — cached multi-source index".into(),
        "skills-sh — https://skills.sh (skills.sh:owner/repo/skill)".into(),
        "well-known — /.well-known/skills/index.json (well-known:<base>/<name>)".into(),
        "url — direct https://…/SKILL.md".into(),
        "github — owner/repo[/path] + curated + taps".into(),
        "clawhub — ClawHub / OpenClaw (@owner/slug → clawhub:slug)".into(),
        "claude-marketplace — .claude-plugin/marketplace.json".into(),
        "lobehub — chat-agents.lobehub.com".into(),
        "browse-sh — browse.sh API".into(),
        "agentskills.io — federation well-known hubs".into(),
        "npm — Pi-style npm:package (pack + find SKILL.md)".into(),
        "local — filesystem path (always quarantined)".into(),
        "Peer aliases: git:owner/repo[/path] | import-from claude|codex|pi|agents|openclaw".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_hermes_sources_listed() {
        for id in [
            "official",
            "skills-sh",
            "well-known",
            "url",
            "github",
            "clawhub",
            "claude-marketplace",
            "lobehub",
            "browse-sh",
        ] {
            assert!(
                ALL_SOURCE_IDS.contains(&id),
                "missing hermes source_id {id}"
            );
        }
    }
}
