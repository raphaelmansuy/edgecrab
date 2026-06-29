use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::context::SkillsScanContext;
use super::slug::{normalize_command_token, slugify};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCommandInfo {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub skill_dir: PathBuf,
}

fn expand_path_with_env(path_str: &str) -> PathBuf {
    let mut result = path_str.to_string();
    if result.starts_with('~')
        && let Some(home) = dirs::home_dir()
    {
        result = result.replacen('~', &home.to_string_lossy(), 1);
    }
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let value = std::env::var(var_name).unwrap_or_default();
            result = format!(
                "{}{}{}",
                &result[..start],
                value,
                &result[start + end + 1..]
            );
        } else {
            break;
        }
    }
    PathBuf::from(result)
}

fn resolve_skill_directories(base_dir: &Path, external_dirs: &[String]) -> Vec<PathBuf> {
    let mut dirs = vec![base_dir.to_path_buf()];
    for dir_str in external_dirs {
        let expanded = expand_path_with_env(dir_str);
        if expanded.is_dir() {
            dirs.push(expanded);
        }
    }
    dirs
}

fn parse_frontmatter_description(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_first = &trimmed[3..];
    let end_pos = after_first.find("\n---")?;
    let frontmatter = &after_first[..end_pos];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("description:") {
            let desc = rest.trim().trim_matches(['\'', '"']);
            if !desc.is_empty() {
                return Some(desc.to_string());
            }
        }
    }
    None
}

fn parse_frontmatter_name(content: &str, fallback: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return fallback.to_string();
    }
    let after_first = &trimmed[3..];
    if let Some(end_pos) = after_first.find("\n---") {
        let frontmatter = &after_first[..end_pos];
        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("name:") {
                let name = rest.trim().trim_matches(['\'', '"']);
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }
    fallback.to_string()
}

fn first_body_line(content: &str) -> Option<String> {
    let body = if content.trim_start().starts_with("---") {
        let after_first = &content.trim_start()[3..];
        after_first
            .find("\n---")
            .map(|end| &after_first[end + 4..])
            .unwrap_or(content)
    } else {
        content
    };
    for line in body.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            return Some(line.chars().take(80).collect());
        }
    }
    None
}

fn scan_dir(
    scan_dir: &Path,
    ctx: &SkillsScanContext,
    seen_names: &mut HashSet<String>,
    out: &mut HashMap<String, SkillCommandInfo>,
) {
    if !scan_dir.is_dir() {
        return;
    }
    let mut stack = vec![scan_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if dir_name.starts_with('.') || dir_name == ".archive" {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                let content = match std::fs::read_to_string(&skill_md) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let name = parse_frontmatter_name(&content, &dir_name);
                if seen_names.contains(&name) {
                    continue;
                }
                if ctx.disabled_skills.contains(&name.to_ascii_lowercase())
                    || ctx.disabled_skills.contains(&dir_name.to_ascii_lowercase())
                {
                    continue;
                }
                let offer = super::filters::parse_offer_meta(&content);
                if !offer.should_offer() {
                    continue;
                }
                let slug = slugify(&name);
                if slug.is_empty() {
                    continue;
                }
                let description = parse_frontmatter_description(&content)
                    .or_else(|| first_body_line(&content))
                    .unwrap_or_else(|| format!("Invoke the {name} skill"));
                seen_names.insert(name.clone());
                out.insert(
                    slug.clone(),
                    SkillCommandInfo {
                        slug,
                        name,
                        description,
                        skill_dir: path,
                    },
                );
            } else {
                stack.push(path);
            }
        }
    }
}

/// Scan installed skills and return slash slug → info (Hermes `scan_skill_commands`).
pub fn scan_skill_commands(ctx: &SkillsScanContext) -> HashMap<String, SkillCommandInfo> {
    let skills_base = ctx.skills_dir();
    let roots = resolve_skill_directories(&skills_base, &ctx.external_skill_dirs);
    let mut out = HashMap::new();
    let mut seen = HashSet::new();
    for root in roots {
        scan_dir(&root, ctx, &mut seen, &mut out);
    }
    out
}

pub fn resolve_skill_command_key(
    commands: &HashMap<String, SkillCommandInfo>,
    command: &str,
) -> Option<String> {
    let token = normalize_command_token(command);
    if token.is_empty() {
        return None;
    }
    commands.contains_key(&token).then_some(token)
}

pub fn list_installed_skill_slugs(ctx: &SkillsScanContext) -> Vec<String> {
    let mut slugs: Vec<_> = scan_skill_commands(ctx).into_keys().collect();
    slugs.sort();
    slugs
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsReloadDiff {
    pub added: Vec<(String, String)>,
    pub removed: Vec<(String, String)>,
    pub unchanged: Vec<String>,
    pub total: usize,
}

pub fn reload_skills(
    before: &HashMap<String, SkillCommandInfo>,
    ctx: &SkillsScanContext,
) -> (HashMap<String, SkillCommandInfo>, SkillsReloadDiff) {
    super::bundles::invalidate_bundle_cache();
    let before_desc: HashMap<_, _> = before
        .iter()
        .map(|(k, v)| (k.clone(), v.description.clone()))
        .collect();
    let after_map = scan_skill_commands(ctx);
    let after_desc: HashMap<_, _> = after_map
        .iter()
        .map(|(k, v)| (k.clone(), v.description.clone()))
        .collect();

    let added: Vec<_> = after_desc
        .iter()
        .filter(|(k, _)| !before_desc.contains_key(*k))
        .map(|(k, d)| (k.clone(), d.clone()))
        .collect();
    let removed: Vec<_> = before_desc
        .iter()
        .filter(|(k, _)| !after_desc.contains_key(*k))
        .map(|(k, d)| (k.clone(), d.clone()))
        .collect();
    let unchanged: Vec<_> = after_desc
        .keys()
        .filter(|k| before_desc.contains_key(*k))
        .cloned()
        .collect();

    let diff = SkillsReloadDiff {
        added,
        removed,
        unchanged,
        total: after_map.len(),
    };
    (after_map, diff)
}

pub fn format_reload_diff(diff: &SkillsReloadDiff) -> String {
    let mut lines = vec![format!("Skills rescanned — {} slash commands.", diff.total)];
    if !diff.added.is_empty() {
        lines.push(format!("Added ({}):", diff.added.len()));
        for (name, desc) in &diff.added {
            lines.push(format!("  + /{name} — {desc}"));
        }
    }
    if !diff.removed.is_empty() {
        lines.push(format!("Removed ({}):", diff.removed.len()));
        for (name, desc) in &diff.removed {
            lines.push(format!("  - /{name} — {desc}"));
        }
    }
    if diff.added.is_empty() && diff.removed.is_empty() {
        lines.push("No changes.".into());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn scan_finds_nested_skill() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("coding").join("demo-skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Demo Skill\ndescription: A demo\n---\n\nBody",
        )
        .expect("write");

        let ctx = SkillsScanContext::from_home(dir.path());
        let cmds = scan_skill_commands(&ctx);
        assert!(cmds.contains_key("demo-skill"));
        assert_eq!(cmds["demo-skill"].name, "Demo Skill");
    }

    #[test]
    fn scan_skips_non_user_invocable() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("hidden");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: hidden\nuser-invocable: false\ndescription: x\n---\n",
        )
        .expect("write");
        let ctx = SkillsScanContext::from_home(dir.path());
        assert!(!scan_skill_commands(&ctx).contains_key("hidden"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn scan_includes_macos_platform_skill() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("mac-only");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: mac-only\nplatforms: [macos]\ndescription: x\n---\n",
        )
        .expect("write");
        let ctx = SkillsScanContext::from_home(dir.path());
        assert!(scan_skill_commands(&ctx).contains_key("mac-only"));
    }
}
