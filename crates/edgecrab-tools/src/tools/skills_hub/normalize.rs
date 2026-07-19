//! Canonical skill identifier normalization (Pi / OpenClaw / Hermes aliases).

/// Normalize a raw install/search identifier into a hub-canonical form.
///
/// Handles:
/// - `git:owner/repo[@ref][/path]` / `git:https://…`
/// - `npm:package[@version]`
/// - `@owner/slug` → `clawhub:slug` (OpenClaw)
/// - `skills-sh:` ↔ `skills.sh:`
/// - path separators
pub fn normalize_identifier(raw: &str) -> String {
    let trimmed = raw.trim().replace('\\', "/");
    if trimmed.is_empty() {
        return trimmed;
    }

    // OpenClaw ClawHub owner-scoped: @owner/slug → clawhub:slug
    if let Some(rest) = trimmed.strip_prefix('@') {
        let slug = rest.rsplit('/').next().unwrap_or(rest).trim();
        if !slug.is_empty() && !slug.contains(':') {
            return format!("clawhub:{slug}");
        }
    }

    // Pi / OpenClaw git: prefix
    if let Some(rest) = trimmed.strip_prefix("git:") {
        return normalize_git_spec(rest);
    }

    // npm: kept as-is (handled by npm_pack)
    if trimmed.to_ascii_lowercase().starts_with("npm:") {
        return format!("npm:{}", trimmed[4..].trim());
    }

    // skills-sh ↔ skills.sh
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("skills-sh:") {
        let original_rest = &trimmed[trimmed.len() - rest.len()..];
        return format!("skills.sh:{original_rest}");
    }
    if let Some(rest) = lower.strip_prefix("skills-sh/") {
        let original_rest = &trimmed[trimmed.len() - rest.len()..];
        return format!("skills.sh:{original_rest}");
    }

    // well-known: keep scheme intact
    if lower.starts_with("well-known:") {
        return trimmed;
    }

    trimmed
}

fn normalize_git_spec(rest: &str) -> String {
    let rest = rest.trim();
    if rest.starts_with("http://") || rest.starts_with("https://") {
        return github_url_to_identifier(rest).unwrap_or_else(|| rest.to_string());
    }
    // git:github.com/owner/repo → owner/repo
    let rest = rest
        .strip_prefix("github.com/")
        .or_else(|| rest.strip_prefix("www.github.com/"))
        .unwrap_or(rest);
    // Drop @ref for Contents API default-branch fetch (path after @ref/…)
    let (repo_path, _ref) = split_git_ref(rest);
    repo_path.trim_matches('/').to_string()
}

fn split_git_ref(spec: &str) -> (String, Option<String>) {
    // owner/repo@main/path → (owner/repo/path, Some(main)) — keep path after ref
    if let Some(at) = spec.find('@') {
        let before = &spec[..at];
        let after = &spec[at + 1..];
        // ref may contain slashes for branches; skill path usually after last known segment.
        // Hermes/OpenClaw: git:owner/repo@ref — no extra path, or owner/repo/path without @.
        if before.matches('/').count() >= 1 {
            // If after looks like ref only (no further skill path convention), drop ref.
            // Convention: owner/repo@ref → owner/repo
            // owner/repo/path — no @
            let parts: Vec<&str> = after.splitn(2, '/').collect();
            if parts.len() == 1 {
                return (before.to_string(), Some(parts[0].to_string()));
            }
            // owner/repo@branch/with/slashes — treat entire after as ref (no path)
            return (before.to_string(), Some(after.to_string()));
        }
    }
    (spec.to_string(), None)
}

fn github_url_to_identifier(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let stripped = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let stripped = stripped.strip_suffix(".git").unwrap_or(stripped);
    // tree/main/path or blob/main/path
    let parts: Vec<&str> = stripped.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let owner = parts[0];
    let repo = parts[1];
    if parts.len() >= 4 && (parts[2] == "tree" || parts[2] == "blob") {
        let path = parts[4..].join("/");
        if path.is_empty() {
            return Some(format!("{owner}/{repo}"));
        }
        return Some(format!("{owner}/{repo}/{path}"));
    }
    if parts.len() > 2 {
        let path = parts[2..].join("/");
        return Some(format!("{owner}/{repo}/{path}"));
    }
    Some(format!("{owner}/{repo}"))
}

// Provider filters live in `catalog.rs` (HUB_CATALOG SoT).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openclaw_at_owner_slug() {
        assert_eq!(normalize_identifier("@alice/my-skill"), "clawhub:my-skill");
    }

    #[test]
    fn git_owner_repo() {
        assert_eq!(
            normalize_identifier("git:owner/repo/skills/foo"),
            "owner/repo/skills/foo"
        );
    }

    #[test]
    fn git_https_url() {
        assert_eq!(
            normalize_identifier("git:https://github.com/openai/skills/tree/main/skills/.curated/foo"),
            "openai/skills/skills/.curated/foo"
        );
    }

    #[test]
    fn skills_sh_alias() {
        assert_eq!(
            normalize_identifier("skills-sh:a/b/c"),
            "skills.sh:a/b/c"
        );
    }

    #[test]
    fn npm_kept() {
        assert_eq!(normalize_identifier("npm:pi-skills"), "npm:pi-skills");
    }

}
