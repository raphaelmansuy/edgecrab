use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use super::context::SkillsScanContext;
use super::discovery::scan_skill_commands;
use super::slug::{normalize_command_token, slugify};
use super::usage::bump_use;
use crate::tools::skills::load_skill_prompt_bundle;

#[derive(Debug, Clone, serde::Deserialize)]
struct BundleFile {
    name: Option<String>,
    description: Option<String>,
    skills: Vec<String>,
    #[serde(default)]
    instruction: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SkillBundleDef {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub skills: Vec<String>,
    pub instruction: String,
    pub path: PathBuf,
}

struct BundleCache {
    mtime: SystemTime,
    bundles: HashMap<String, SkillBundleDef>,
}

static BUNDLE_CACHE: OnceLock<Mutex<Option<BundleCache>>> = OnceLock::new();

fn bundle_cache() -> &'static Mutex<Option<BundleCache>> {
    BUNDLE_CACHE.get_or_init(|| Mutex::new(None))
}

fn bundles_max_mtime(dir: &Path, files: &[PathBuf]) -> SystemTime {
    let mut latest = dir.metadata().and_then(|m| m.modified()).ok();
    for file in files {
        if let Ok(meta) = file.metadata()
            && let Ok(modified) = meta.modified()
            && latest.as_ref().is_none_or(|l| modified > *l)
        {
            latest = Some(modified);
        }
    }
    latest.unwrap_or(SystemTime::UNIX_EPOCH)
}

fn iter_bundle_files(dir: &Path) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        {
            files.push(path);
        }
    }
    files.sort();
    files
}

fn load_bundle_file(path: &Path) -> Option<SkillBundleDef> {
    let raw = std::fs::read_to_string(path).ok()?;
    let data: BundleFile = serde_yml::from_str(&raw).ok()?;
    let stem = path.file_stem()?.to_str()?;
    let name = data.name.unwrap_or_else(|| stem.to_string());
    if name.trim().is_empty() || data.skills.is_empty() {
        return None;
    }
    let skills: Vec<String> = data
        .skills
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if skills.is_empty() {
        return None;
    }
    let slug = slugify(&name);
    if slug.is_empty() {
        return None;
    }
    Some(SkillBundleDef {
        description: data
            .description
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| format!("Load {} skills as a bundle", skills.len())),
        name,
        slug: slug.clone(),
        skills,
        instruction: data.instruction.unwrap_or_default(),
        path: path.to_path_buf(),
    })
}

fn scan_bundles_internal(ctx: &SkillsScanContext) -> HashMap<String, SkillBundleDef> {
    let dir = ctx.bundles_dir();
    let files = iter_bundle_files(&dir);
    let mut out = HashMap::new();
    for file in files {
        if let Some(bundle) = load_bundle_file(&file) {
            if out.contains_key(&bundle.slug) {
                tracing::warn!(
                    slug = %bundle.slug,
                    path = %file.display(),
                    "duplicate bundle slug; keeping first"
                );
                continue;
            }
            out.insert(bundle.slug.clone(), bundle);
        }
    }
    let mtime = bundles_max_mtime(&dir, &iter_bundle_files(&dir));
    if let Ok(mut guard) = bundle_cache().lock() {
        *guard = Some(BundleCache {
            mtime,
            bundles: out.clone(),
        });
    }
    out
}

pub fn get_skill_bundles(ctx: &SkillsScanContext) -> HashMap<String, SkillBundleDef> {
    let dir = ctx.bundles_dir();
    let files = iter_bundle_files(&dir);
    let current_mtime = bundles_max_mtime(&dir, &files);
    if let Ok(guard) = bundle_cache().lock()
        && let Some(cache) = guard.as_ref()
        && cache.mtime == current_mtime
    {
        return cache.bundles.clone();
    }
    scan_bundles_internal(ctx)
}

pub fn resolve_bundle_command_key(
    bundles: &HashMap<String, SkillBundleDef>,
    command: &str,
) -> Option<String> {
    let token = normalize_command_token(command);
    bundles.contains_key(&token).then_some(token)
}

pub fn reload_bundles(
    ctx: &SkillsScanContext,
) -> (
    HashMap<String, SkillBundleDef>,
    crate::skills::discovery::SkillsReloadDiff,
) {
    let before = get_skill_bundles(ctx);
    let before_desc: HashMap<_, _> = before
        .iter()
        .map(|(k, v)| (k.clone(), v.description.clone()))
        .collect();
    if let Ok(mut guard) = bundle_cache().lock() {
        *guard = None;
    }
    let after = scan_bundles_internal(ctx);
    let after_desc: HashMap<_, _> = after
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
    let diff = crate::skills::discovery::SkillsReloadDiff {
        added,
        removed,
        unchanged,
        total: after.len(),
    };
    (after, diff)
}

pub fn list_bundles(ctx: &SkillsScanContext) -> Vec<SkillBundleDef> {
    let mut bundles: Vec<_> = get_skill_bundles(ctx).into_values().collect();
    bundles.sort_by(|a, b| a.slug.cmp(&b.slug));
    bundles
}

pub fn format_bundles_list(ctx: &SkillsScanContext) -> String {
    let bundles = list_bundles(ctx);
    if bundles.is_empty() {
        return format!(
            "No skill bundles installed.\n\
             Create YAML files under {} (see Hermes skill-bundles docs).",
            ctx.bundles_dir().display()
        );
    }
    let mut lines = vec![format!("Skill bundles ({}):", bundles.len())];
    for bundle in bundles {
        lines.push(format!(
            "  /{} — {} ({} skills)",
            bundle.slug,
            bundle.description,
            bundle.skills.len()
        ));
        lines.push(format!("    skills: {}", bundle.skills.join(", ")));
    }
    lines.join("\n")
}

pub fn build_bundle_invocation_message(
    ctx: &SkillsScanContext,
    slug: &str,
    user_instruction: &str,
    session_id: Option<&str>,
) -> Option<String> {
    let bundles = get_skill_bundles(ctx);
    let bundle = bundles.get(slug)?;
    let commands = scan_skill_commands(ctx);
    let mut blocks = Vec::new();
    let mut loaded = Vec::new();
    let mut missing = Vec::new();
    let mut seen = HashSet::new();

    for skill_id in &bundle.skills {
        let id = skill_id.trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        let lookup = commands
            .get(&slugify(id))
            .map(|i| i.name.clone())
            .unwrap_or_else(|| id.to_string());
        if let Some(body) = load_skill_prompt_bundle(
            &ctx.edgecrab_home,
            &ctx.external_skill_dirs,
            &lookup,
            session_id,
        ) {
            bump_use(&ctx.edgecrab_home, &lookup);
            blocks.push(format!(
                "[Loaded as part of the \"{}\" skill bundle.]\n\n{}",
                bundle.name, body
            ));
            loaded.push(lookup);
        } else {
            missing.push(id.to_string());
        }
    }
    if blocks.is_empty() {
        return None;
    }
    let mut parts = vec![format!(
        "[IMPORTANT: The user invoked the \"{}\" skill bundle (/{}) — \
         loading {} skills together. Treat every skill below as active guidance.]",
        bundle.name,
        bundle.slug,
        loaded.len()
    )];
    if !loaded.is_empty() {
        parts.push(format!("Skills loaded: {}", loaded.join(", ")));
    }
    if !bundle.instruction.is_empty() {
        parts.push(bundle.instruction.clone());
    }
    parts.extend(blocks);
    if !missing.is_empty() {
        parts.push(format!(
            "[Note: these bundle skills were not found and were skipped: {}]",
            missing.join(", ")
        ));
    }
    if !user_instruction.trim().is_empty() {
        parts.push(format!(
            "The user provided the following instruction alongside the bundle invocation: {}",
            user_instruction.trim()
        ));
    }
    let _ = loaded;
    Some(parts.join("\n\n"))
}

pub fn invalidate_bundle_cache() {
    if let Ok(mut guard) = bundle_cache().lock() {
        *guard = None;
    }
}

pub fn bundle_path_for(ctx: &SkillsScanContext, name: &str) -> Result<PathBuf, String> {
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(format!("Bundle name {name:?} normalizes to an empty slug"));
    }
    Ok(ctx.bundles_dir().join(format!("{slug}.yaml")))
}

pub fn save_bundle(
    ctx: &SkillsScanContext,
    name: &str,
    skills: &[String],
    description: &str,
    instruction: &str,
    overwrite: bool,
) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Bundle name is required".into());
    }
    let cleaned: Vec<String> = skills
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if cleaned.is_empty() {
        return Err("Bundle must reference at least one skill".into());
    }
    let path = bundle_path_for(ctx, name)?;
    if path.exists() && !overwrite {
        return Err(format!(
            "Bundle already exists at {} (use overwrite)",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut payload = serde_yml::Mapping::new();
    payload.insert(serde_yml::Value::from("name"), serde_yml::Value::from(name));
    if !description.trim().is_empty() {
        payload.insert(
            serde_yml::Value::from("description"),
            serde_yml::Value::from(description.trim()),
        );
    }
    if !instruction.trim().is_empty() {
        payload.insert(
            serde_yml::Value::from("instruction"),
            serde_yml::Value::from(instruction.trim()),
        );
    }
    let skill_list: Vec<serde_yml::Value> = cleaned
        .iter()
        .cloned()
        .map(serde_yml::Value::from)
        .collect();
    payload.insert(
        serde_yml::Value::from("skills"),
        serde_yml::Value::Sequence(skill_list),
    );
    let yaml = serde_yml::to_string(&payload).map_err(|e| e.to_string())?;
    std::fs::write(&path, yaml).map_err(|e| e.to_string())?;
    invalidate_bundle_cache();
    Ok(path)
}

pub fn delete_bundle(ctx: &SkillsScanContext, name: &str) -> Result<PathBuf, String> {
    let path = bundle_path_for(ctx, name)?;
    if !path.is_file() {
        return Err(format!("No bundle at {}", path.display()));
    }
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    invalidate_bundle_cache();
    Ok(path)
}

/// Handle `/bundles create|delete|list` — returns `None` when caller should list.
pub fn handle_bundles_subcommand(ctx: &SkillsScanContext, args: &str) -> Option<String> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let first = *tokens.first()?;
    match first.to_ascii_lowercase().as_str() {
        "" | "list" | "ls" => None,
        "create" | "add" | "new" => {
            let Some(name) = tokens.get(1) else {
                return Some(
                    "Usage: /bundles create <name> <skill1> [skill2 ...]\n\
                     Example: /bundles create backend-dev github-code-review test-driven-development"
                        .into(),
                );
            };
            let skills: Vec<String> = tokens.iter().skip(2).map(|s| (*s).to_string()).collect();
            match save_bundle(ctx, name, &skills, "", "", false) {
                Ok(path) => Some(format!("Created bundle '{name}' at {}", path.display())),
                Err(e) => Some(format!("Create failed: {e}")),
            }
        }
        "delete" | "remove" | "rm" => {
            let Some(name) = tokens.get(1) else {
                return Some("Usage: /bundles delete <name>".into());
            };
            match delete_bundle(ctx, name) {
                Ok(path) => Some(format!("Deleted bundle at {}", path.display())),
                Err(e) => Some(format!("Delete failed: {e}")),
            }
        }
        other => Some(format!(
            "Unknown /bundles subcommand '{other}'. Try: /bundles, /bundles create, /bundles delete"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_delete_bundle() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let ctx = SkillsScanContext::from_home(dir.path());
        let path =
            save_bundle(&ctx, "demo", &["skill-a".into()], "desc", "instr", false).expect("save");
        assert!(path.is_file());
        let deleted = delete_bundle(&ctx, "demo").expect("delete");
        assert_eq!(deleted, path);
    }

    #[test]
    fn loads_bundle_yaml() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let bundles_dir = dir.path().join("skill-bundles");
        std::fs::create_dir_all(&bundles_dir).expect("mkdir");
        std::fs::write(
            bundles_dir.join("backend.yaml"),
            "name: backend-dev\nskills:\n  - demo-skill\ninstruction: Use TDD\n",
        )
        .expect("write");
        let skill_dir = dir.path().join("skills").join("demo-skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir skill");
        std::fs::write(skill_dir.join("SKILL.md"), "# Demo\n").expect("write skill");

        let ctx = SkillsScanContext::from_home(dir.path());
        let bundles = get_skill_bundles(&ctx);
        assert!(bundles.contains_key("backend-dev"));
        let msg = build_bundle_invocation_message(&ctx, "backend-dev", "fix auth", None)
            .expect("message");
        assert!(msg.contains("backend-dev"));
        assert!(msg.contains("fix auth"));
    }
}
