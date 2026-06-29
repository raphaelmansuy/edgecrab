//! Offer-time skill filters — platform, environment, user-invocable (Hermes parity).
//!
//! Used by slash discovery and `skills_list` so macOS-only skills (`platforms: [macos]`)
//! are not incorrectly hidden on darwin hosts.

use std::path::Path;

/// Metadata needed to decide whether a skill appears in slash / list surfaces.
#[derive(Debug, Clone, Default)]
pub struct SkillOfferMeta {
    pub user_invocable: bool,
    pub platforms: Vec<String>,
    pub environments: Vec<String>,
}

impl SkillOfferMeta {
    pub fn should_offer(&self) -> bool {
        self.user_invocable
            && skill_matches_platform(&self.platforms)
            && skill_matches_environment(&self.environments)
    }
}

/// Current OS prefix — matches Python `sys.platform` (darwin / linux / win32).
pub fn current_os_platform_prefix() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "darwin"
    }
    #[cfg(target_os = "linux")]
    {
        return "linux";
    }
    #[cfg(target_os = "windows")]
    {
        return "win32";
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "unknown"
    }
}

fn normalize_platform_tag(tag: &str) -> String {
    match tag.trim().to_ascii_lowercase().as_str() {
        "macos" | "darwin" => "darwin".into(),
        "linux" => "linux".into(),
        "windows" | "win32" => "win32".into(),
        "termux" | "android" => "linux".into(),
        other => other.to_string(),
    }
}

fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
        || std::env::var("PREFIX")
            .map(|p| p.contains("com.termux"))
            .unwrap_or(false)
}

/// True when the skill is compatible with the current OS (agentskills.io `platforms` field).
pub fn skill_matches_platform(platforms: &[String]) -> bool {
    if platforms.is_empty() {
        return true;
    }
    let current = current_os_platform_prefix();
    let termux = is_termux();
    for platform in platforms {
        let mapped = normalize_platform_tag(platform);
        if current.starts_with(&mapped) || mapped == current {
            return true;
        }
        if termux && (mapped == "linux" || mapped == "termux" || mapped == "android") {
            return true;
        }
    }
    false
}

fn detect_environment(env: &str) -> bool {
    match env.trim().to_ascii_lowercase().as_str() {
        "kanban" => {
            std::env::var("EDGECRAB_KANBAN_TASK").is_ok()
                || std::env::var("EDGECRAB_KANBAN_BOARD").is_ok()
                || std::env::var("HERMES_KANBAN_TASK").is_ok()
                || std::env::var("HERMES_KANBAN_BOARD").is_ok()
        }
        "docker" => is_container(),
        "s6" => Path::new("/run/s6").is_dir() || Path::new("/package/admin/s6-overlay").is_dir(),
        // Unknown tags fail open (Hermes parity).
        _ => true,
    }
}

fn is_container() -> bool {
    if Path::new("/.dockerenv").exists() {
        return true;
    }
    std::fs::read_to_string("/proc/1/cgroup")
        .map(|s| s.contains("docker") || s.contains("kubepods") || s.contains("containerd"))
        .unwrap_or(false)
}

/// True when the skill is relevant to the current runtime (`environments` frontmatter).
pub fn skill_matches_environment(environments: &[String]) -> bool {
    if environments.is_empty() {
        return true;
    }
    for env in environments {
        let normalized = env.trim();
        if normalized.is_empty() {
            continue;
        }
        if detect_environment(normalized) {
            return true;
        }
    }
    false
}

fn parse_frontmatter_block(content: &str) -> Option<&str> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after = &trimmed[3..];
    let end = after.find("\n---")?;
    Some(after[..end].trim())
}

fn parse_inline_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let stripped = trimmed.trim_start_matches('[').trim_end_matches(']');
    stripped
        .split(',')
        .map(|item| item.trim().trim_matches(['\'', '"']).to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn parse_bool(value: &str, default: bool) -> bool {
    match value.trim().trim_matches(['\'', '"']) {
        v if v.eq_ignore_ascii_case("true")
            || v.eq_ignore_ascii_case("yes")
            || v.eq_ignore_ascii_case("on")
            || v == "1" =>
        {
            true
        }
        v if v.eq_ignore_ascii_case("false")
            || v.eq_ignore_ascii_case("no")
            || v.eq_ignore_ascii_case("off")
            || v == "0" =>
        {
            false
        }
        "" => default,
        _ => default,
    }
}

/// Parse offer-time fields from SKILL.md YAML frontmatter (lightweight — no full schema).
pub fn parse_offer_meta(content: &str) -> SkillOfferMeta {
    let mut meta = SkillOfferMeta {
        user_invocable: true,
        ..Default::default()
    };
    let Some(fm) = parse_frontmatter_block(content) else {
        return meta;
    };
    let mut list_key: Option<&str> = None;
    for line in fm.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("- ") {
            if let Some(key) = list_key {
                let item = rest.trim().trim_matches(['\'', '"']).to_string();
                if !item.is_empty() {
                    match key {
                        "platforms" => meta.platforms.push(item),
                        "environments" => meta.environments.push(item),
                        _ => {}
                    }
                }
            }
            continue;
        }
        list_key = None;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "user-invocable" | "user_invocable" => {
                meta.user_invocable = parse_bool(value, true);
            }
            "platforms" | "environments" => {
                if value.starts_with('[') {
                    let items = parse_inline_list(value);
                    if key == "platforms" {
                        meta.platforms = items;
                    } else {
                        meta.environments = items;
                    }
                } else if !value.is_empty() {
                    list_key = Some(key);
                    let item = value.trim_matches(['\'', '"']).to_string();
                    if key == "platforms" {
                        meta.platforms.push(item);
                    } else {
                        meta.environments.push(item);
                    }
                }
            }
            _ => {}
        }
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_platform_matches_darwin_host() {
        assert!(skill_matches_platform(&["macos".into()]));
        assert!(skill_matches_platform(&["darwin".into()]));
    }

    #[test]
    fn windows_only_hidden_on_macos() {
        #[cfg(target_os = "macos")]
        assert!(!skill_matches_platform(&["windows".into()]));
    }

    #[test]
    fn user_invocable_false_hides_from_offer() {
        let md = "---\nname: x\nuser-invocable: false\nplatforms: [macos]\n---\n";
        let meta = parse_offer_meta(md);
        assert!(!meta.should_offer());
    }

    #[test]
    fn empty_platforms_offers_everywhere() {
        let md = "---\nname: x\ndescription: y\n---\n";
        assert!(parse_offer_meta(md).should_offer());
    }

    #[test]
    fn unknown_environment_tag_fails_open() {
        assert!(skill_matches_environment(&["quantum-computer".into()]));
    }

    #[test]
    fn kanban_env_requires_signal() {
        assert!(!skill_matches_environment(&["kanban".into()]));
    }
}
