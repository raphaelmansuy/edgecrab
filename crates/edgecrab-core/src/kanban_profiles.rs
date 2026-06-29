//! Kanban profile roster — Hermes `profiles.py` + decomposer routing subset.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use edgecrab_types::AgentError;

use crate::config::{AppConfig, KanbanConfig};
use serde_json::Value;

/// One profile entry for the decomposer prompt.
#[derive(Debug, Clone)]
pub struct KanbanProfileEntry {
    pub name: String,
    pub description: String,
    pub has_description: bool,
    pub description_auto: bool,
}

/// Normalize profile names (Hermes lowercase canonical form).
pub fn normalize_profile_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

/// EdgeCrab install root (`~/.edgecrab`), even when `EDGECRAB_HOME` points at a named profile.
pub fn install_root() -> PathBuf {
    install_root_from(crate::edgecrab_home())
}

pub fn install_root_from(home: PathBuf) -> PathBuf {
    if home
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "profiles")
    {
        return home
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(&home)
            .to_path_buf();
    }
    home
}

/// Effective home directory for a profile name.
pub fn profile_effective_home(install_root: &Path, name: &str) -> PathBuf {
    let canon = normalize_profile_name(name);
    if canon == "default" {
        install_root.to_path_buf()
    } else {
        install_root.join("profiles").join(&canon)
    }
}

/// Whether a profile name resolves to an existing home directory.
pub fn profile_exists(install_root: &Path, name: &str) -> bool {
    let canon = normalize_profile_name(name);
    if canon == "default" {
        return true;
    }
    profile_effective_home(install_root, &canon).is_dir()
}

fn read_active_profile(install_root: &Path) -> String {
    let path = install_root.join(".active_profile");
    fs::read_to_string(&path)
        .ok()
        .map(|s| normalize_profile_name(&s))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".into())
}

/// Active profile name from `~/.edgecrab/.active_profile`.
pub fn active_profile_name(install_root: &Path) -> String {
    read_active_profile(install_root)
}

/// Dashboard/API profile row — Hermes `GET /profiles`.
pub fn profiles_api_json(install_root: &Path) -> Value {
    use serde_json::json;
    let profiles: Vec<Value> = list_profile_roster(install_root)
        .iter()
        .map(|entry| {
            let model = load_config_for_profile(install_root, &entry.name)
                .map(|c| c.model.default_model)
                .unwrap_or_default();
            let desc = if entry.has_description {
                entry.description.clone()
            } else {
                String::new()
            };
            json!({
                "name": entry.name,
                "is_default": entry.name == "default",
                "model": model,
                "description": desc,
                "description_auto": entry.description_auto,
            })
        })
        .collect();
    json!({ "profiles": profiles })
}

/// Persist user-authored profile description (`profile.yaml`).
pub fn write_profile_description(
    install_root: &Path,
    profile_name: &str,
    description: &str,
) -> Result<String, AgentError> {
    write_profile_meta(install_root, profile_name, description, false)
}

/// Persist profile description and auto/manual flag (`profile.yaml`).
pub fn write_profile_meta(
    install_root: &Path,
    profile_name: &str,
    description: &str,
    description_auto: bool,
) -> Result<String, AgentError> {
    let canon = normalize_profile_name(profile_name);
    if !profile_exists(install_root, &canon) {
        return Err(AgentError::Validation(format!(
            "profile '{profile_name}' not found"
        )));
    }
    let dir = profile_effective_home(install_root, &canon);
    let path = dir.join("profile.yaml");
    let mut existing: serde_yml::Value = if path.is_file() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_yml::from_str::<serde_yml::Value>(&raw).ok())
            .and_then(|v: serde_yml::Value| if v.is_mapping() { Some(v) } else { None })
            .unwrap_or_else(|| serde_yml::Value::Mapping(serde_yml::Mapping::new()))
    } else {
        serde_yml::Value::Mapping(serde_yml::Mapping::new())
    };
    let Some(map) = existing.as_mapping_mut() else {
        return Err(AgentError::Validation("profile.yaml is not a mapping".into()));
    };
    let text = description.trim().to_string();
    map.insert(
        serde_yml::Value::String("description".into()),
        serde_yml::Value::String(text.clone()),
    );
    map.insert(
        serde_yml::Value::String("description_auto".into()),
        serde_yml::Value::Bool(description_auto),
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(AgentError::Io)?;
    }
    let yaml = serde_yml::to_string(&existing)
        .map_err(|e| AgentError::Config(format!("profile.yaml serialize: {e}")))?;
    fs::write(&path, yaml).map_err(AgentError::Io)?;
    Ok(text)
}

fn description_from_profile_yaml(profile_dir: &Path) -> Option<(String, bool)> {
    let raw = fs::read_to_string(profile_dir.join("profile.yaml")).ok()?;
    let val = serde_yml::from_str::<serde_yml::Value>(&raw).ok()?;
    let text = val.get("description")?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let auto = val
        .get("description_auto")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some((text.to_string(), auto))
}

fn description_from_soul(profile_dir: &Path) -> Option<String> {
    let raw = fs::read_to_string(profile_dir.join("SOUL.md")).ok()?;
    let first = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;
    (!first.is_empty()).then(|| first.chars().take(200).collect())
}

fn read_profile_description(profile_dir: &Path) -> (String, bool, bool) {
    if let Some((text, auto)) = description_from_profile_yaml(profile_dir) {
        return (text, true, auto);
    }
    if let Some(text) = description_from_soul(profile_dir) {
        return (text, true, false);
    }
    (String::new(), false, false)
}

fn collect_profile_names(install_root: &Path) -> HashSet<String> {
    let mut names = HashSet::from(["default".to_string()]);
    let profiles_dir = install_root.join("profiles");
    let Ok(entries) = fs::read_dir(&profiles_dir) else {
        return names;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        names.insert(normalize_profile_name(name));
    }
    names
}

/// List installed profiles with descriptions for decomposer routing.
pub fn list_profile_roster(install_root: &Path) -> Vec<KanbanProfileEntry> {
    let mut roster: Vec<KanbanProfileEntry> = collect_profile_names(install_root)
        .into_iter()
        .map(|name| {
            let home = profile_effective_home(install_root, &name);
            let (description, has_description, description_auto) = read_profile_description(&home);
            let display_desc = if has_description {
                description
            } else {
                format!("(no description; profile named '{name}')")
            };
            KanbanProfileEntry {
                name,
                description: display_desc,
                has_description,
                description_auto,
            }
        })
        .collect();
    roster.sort_by(|a, b| a.name.cmp(&b.name));
    roster
}

/// Valid assignee names on this machine.
pub fn valid_assignee_names(install_root: &Path) -> HashSet<String> {
    list_profile_roster(install_root)
        .into_iter()
        .map(|e| e.name)
        .collect()
}

pub fn format_roster_for_prompt(roster: &[KanbanProfileEntry]) -> String {
    if roster.is_empty() {
        return "  (no profiles installed — decomposer cannot route work)".into();
    }
    roster
        .iter()
        .map(|e| {
            let tag = if e.has_description { "" } else { " ⚠ undescribed" };
            format!("  - {}{}: {}", e.name, tag, e.description)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolve_named_profile(
    explicit: Option<&str>,
    install_root: &Path,
    fallback: impl FnOnce() -> String,
) -> String {
    let Some(name) = explicit.map(normalize_profile_name).filter(|s| !s.is_empty()) else {
        return fallback();
    };
    if profile_exists(install_root, &name) {
        name
    } else {
        fallback()
    }
}

/// Resolve orchestrator profile for root task after fan-out.
pub fn resolve_orchestrator_profile(cfg: &KanbanConfig, install_root: &Path) -> String {
    resolve_named_profile(
        cfg.orchestrator_profile.as_deref(),
        install_root,
        || read_active_profile(install_root),
    )
}

/// Resolve default assignee for unroutable / null decomposer picks.
pub fn resolve_default_assignee(cfg: &KanbanConfig, install_root: &Path) -> String {
    resolve_named_profile(
        cfg.default_assignee.as_deref(),
        install_root,
        || read_active_profile(install_root),
    )
}

/// Normalize LLM assignee choice; invalid names fall back to `default_assignee`.
pub fn normalize_assignee_choice(
    assignee: Option<&str>,
    default_assignee: &str,
    valid_names: &HashSet<String>,
) -> String {
    let Some(raw) = assignee.map(str::trim).filter(|s| !s.is_empty()) else {
        return default_assignee.to_string();
    };
    let chosen = normalize_profile_name(raw);
    if valid_names.contains(&chosen) {
        chosen
    } else {
        default_assignee.to_string()
    }
}

/// Load `config.yaml` for a named profile (worker spawn).
pub fn load_config_for_profile(
    install_root: &Path,
    profile_name: &str,
) -> Result<AppConfig, AgentError> {
    let home = profile_effective_home(install_root, profile_name);
    let path = home.join("config.yaml");
    if path.is_file() {
        AppConfig::load_from(&path)
    } else {
        Ok(AppConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize_profile_name("Work"), "work");
    }

    #[test]
    fn roster_includes_default_and_named() {
        let dir = TempDir::new().expect("tmpdir");
        let root = dir.path();
        fs::create_dir_all(root.join("profiles/research")).expect("mkdir");
        fs::write(
            root.join("profiles/research/profile.yaml"),
            "description: Research and analysis\n",
        )
        .expect("write");
        let roster = list_profile_roster(root);
        assert!(roster.iter().any(|e| e.name == "default"));
        assert!(roster.iter().any(|e| e.name == "research" && e.has_description));
    }

    #[test]
    fn invalid_assignee_falls_back() {
        let valid: HashSet<String> = ["default", "work"].into_iter().map(str::to_string).collect();
        assert_eq!(
            normalize_assignee_choice(Some("unknown"), "default", &valid),
            "default"
        );
        assert_eq!(
            normalize_assignee_choice(Some("Work"), "default", &valid),
            "work"
        );
    }
}
