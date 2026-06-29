//! Skill config declarations — scan, show, and set `skills.config.*` in config.yaml.
//!
//! Hermes parity for `metadata.{hermes,edgecrab}.config` without a separate migrate CLI.

use std::collections::HashMap;
use std::path::Path;

use serde_yml::{Mapping, Value};

use super::context::SkillsScanContext;
use super::discovery::scan_skill_commands;
use super::preprocess::extract_skill_config_vars;

#[derive(Debug, Clone)]
pub struct SkillConfigEntry {
    pub skill_name: String,
    pub key: String,
    pub description: String,
    pub configured_value: Option<String>,
}

fn load_config_value(config_path: &Path) -> Value {
    if !config_path.is_file() {
        return Value::Mapping(Mapping::new());
    }
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|text| serde_yml::from_str(&text).ok())
        .unwrap_or(Value::Mapping(Mapping::new()))
}

fn resolve_dotpath(config: &Value, dotted_key: &str) -> Option<String> {
    let mut current = config;
    for part in dotted_key.split('.') {
        current = current.get(part)?;
    }
    match current {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn storage_key(logical_key: &str) -> String {
    format!("skills.config.{logical_key}")
}

/// Scan installed skills for declared config keys and current values.
pub fn scan_skill_config_entries(
    ctx: &SkillsScanContext,
    config_path: &Path,
) -> Vec<SkillConfigEntry> {
    let config = load_config_value(config_path);
    let commands = scan_skill_commands(ctx);
    let mut out = Vec::new();
    let mut seen_keys = HashMap::new();

    for info in commands.values() {
        let skill_md = info.skill_dir.join("SKILL.md");
        let Ok(content) = std::fs::read_to_string(&skill_md) else {
            continue;
        };
        for var in extract_skill_config_vars(&content) {
            if seen_keys.contains_key(&var.key) {
                continue;
            }
            seen_keys.insert(var.key.clone(), info.name.clone());
            let configured = resolve_dotpath(&config, &storage_key(&var.key));
            out.push(SkillConfigEntry {
                skill_name: info.name.clone(),
                key: var.key.clone(),
                description: var.description.clone(),
                configured_value: configured,
            });
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

pub fn format_skill_config_show(ctx: &SkillsScanContext, config_path: &Path) -> String {
    let entries = scan_skill_config_entries(ctx, config_path);
    if entries.is_empty() {
        return "No skill config declarations found in installed SKILL.md frontmatter.\n\
                Declare keys under metadata.edgecrab.config (or metadata.hermes.config)."
            .into();
    }
    let mut lines = vec!["Skill settings (skills.config.* in config.yaml):".to_string()];
    for entry in &entries {
        let value = entry
            .configured_value
            .as_deref()
            .filter(|v| !v.is_empty())
            .unwrap_or("(not set)");
        lines.push(format!(
            "  {} — {}  [from skill: {}]",
            entry.key, value, entry.skill_name
        ));
        if !entry.description.is_empty() && entry.description != entry.key {
            lines.push(format!("      {}", entry.description));
        }
    }
    lines.push(String::new());
    lines.push("Set: /skills config set <key> <value>  |  /skills config migrate".into());
    lines.join("\n")
}

pub fn format_skill_config_migrate(ctx: &SkillsScanContext, config_path: &Path) -> String {
    let entries = scan_skill_config_entries(ctx, config_path);
    let missing: Vec<_> = entries
        .iter()
        .filter(|e| {
            e.configured_value
                .as_ref()
                .is_none_or(|v| v.trim().is_empty())
        })
        .collect();
    if missing.is_empty() {
        return "All declared skill config keys are set.".into();
    }
    let mut lines = vec![format!("Unconfigured skill settings ({}):", missing.len())];
    for entry in missing {
        lines.push(format!("  {} — {}", entry.key, entry.description));
        lines.push(format!(
            "      Set: /skills config set {} <value>",
            entry.key
        ));
    }
    lines.join("\n")
}

fn ensure_mapping(value: &mut Value) -> &mut Mapping {
    if !value.is_mapping() {
        *value = Value::Mapping(Mapping::new());
    }
    value.as_mapping_mut().expect("mapping")
}

fn set_dotpath(root: &mut Value, dotted_key: &str, value: Value) {
    let parts: Vec<&str> = dotted_key.split('.').collect();
    if parts.is_empty() {
        return;
    }
    let mut current = root;
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        let map = ensure_mapping(current);
        if !map.contains_key(Value::from(*part)) {
            map.insert(Value::from(*part), Value::Mapping(Mapping::new()));
        }
        current = map
            .get_mut(Value::from(*part))
            .expect("nested key just inserted");
    }
    let map = ensure_mapping(current);
    map.insert(Value::from(parts[parts.len() - 1]), value);
}

/// Persist a skill config value under `skills.config.<key>`.
pub fn set_skill_config_value(
    config_path: &Path,
    logical_key: &str,
    raw_value: &str,
) -> Result<String, String> {
    let key = logical_key.trim();
    if key.is_empty() {
        return Err("config key cannot be empty".into());
    }
    let value = raw_value.trim();
    if value.is_empty() {
        return Err("config value cannot be empty".into());
    }
    let mut root = load_config_value(config_path);
    set_dotpath(
        &mut root,
        &storage_key(key),
        Value::String(value.to_string()),
    );
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
    }
    let yaml = serde_yml::to_string(&root).map_err(|e| format!("serialize failed: {e}"))?;
    std::fs::write(config_path, yaml).map_err(|e| format!("write failed: {e}"))?;
    Ok(format!("Set skills.config.{key} = {value}"))
}

/// Handle `/skills config [show|set|list]`.
pub fn handle_skills_config_subcommand(
    ctx: &SkillsScanContext,
    config_path: &Path,
    args: &str,
) -> Option<String> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let first = tokens.first()?.to_ascii_lowercase();
    if first != "config" && first != "settings" {
        return None;
    }
    let sub = tokens
        .get(1)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match sub.as_str() {
        "" | "show" | "list" => Some(format_skill_config_show(ctx, config_path)),
        "migrate" | "missing" | "unset" => Some(format_skill_config_migrate(ctx, config_path)),
        "set" => {
            let key = tokens.get(2)?;
            let value = tokens.get(3..)?.join(" ");
            if value.trim().is_empty() {
                return Some("Usage: /skills config set <key> <value>".into());
            }
            Some(
                set_skill_config_value(config_path, key, value.trim())
                    .unwrap_or_else(|e| format!("Config set failed: {e}")),
            )
        }
        "help" => Some(
            "Skill config commands:\n\
             /skills config — list declared settings + current values\n\
             /skills config migrate — list unset keys\n\
             /skills config set wiki.path ~/wiki — persist to config.yaml"
                .into(),
        ),
        other => Some(format!(
            "Unknown /skills config subcommand '{other}'. Try: /skills config show"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill_with_config(home: &Path, slug: &str, name: &str) {
        let dir = home.join("skills").join(slug);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: test\nmetadata:\n  edgecrab:\n    config:\n      - key: wiki.path\n        description: Wiki root\n        default: \"\"\n---\n"
            ),
        )
        .expect("write");
    }

    #[test]
    fn scan_finds_declared_keys() {
        let dir = TempDir::new().expect("tmpdir");
        write_skill_with_config(dir.path(), "wiki", "Wiki Skill");
        let ctx = SkillsScanContext::from_home(dir.path());
        let cfg = dir.path().join("config.yaml");
        let entries = scan_skill_config_entries(&ctx, &cfg);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "wiki.path");
    }

    #[test]
    fn set_persists_nested_yaml() {
        let dir = TempDir::new().expect("tmpdir");
        let cfg = dir.path().join("config.yaml");
        set_skill_config_value(&cfg, "wiki.path", "~/notes").expect("set");
        let text = std::fs::read_to_string(&cfg).expect("read");
        assert!(text.contains("wiki"));
        assert!(text.contains("~/notes"));
    }
}
