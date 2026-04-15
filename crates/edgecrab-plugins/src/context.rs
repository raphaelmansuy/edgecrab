//! # Context Engine Plugin Discovery
//!
//! Discovers and loads context engine plugins from `~/.edgecrab/plugins/context_engine/`.
//! Each plugin directory must contain a `manifest.yaml` with name, description,
//! command, and args fields.
//!
//! ## Design Note
//!
//! This module is intentionally kept trait-free: it only handles filesystem
//! discovery and manifest parsing. The `PluginContextEngine` adapter (which
//! actually *implements* `ContextEngine`) lives in `edgecrab-core` to avoid
//! a circular dependency (`edgecrab-plugins` → `edgecrab-core` → `edgecrab-plugins`).

use std::path::PathBuf;

/// Manifest data for a context engine plugin — command + args to spawn.
#[derive(Debug, Clone)]
pub struct ContextEngineManifest {
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
}

/// Scan `~/.edgecrab/plugins/context_engine/` for available engines.
///
/// Returns a list of `(name, description, available)` tuples.
pub fn discover_context_engines() -> Vec<(String, String, bool)> {
    let dir = context_engine_plugins_dir();
    let mut engines = Vec::new();

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return engines,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("manifest.yaml");
        if !manifest_path.exists() {
            continue;
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();

        let (description, available) = match std::fs::read_to_string(&manifest_path) {
            Ok(content) => {
                let desc = extract_yaml_field(&content, "description")
                    .unwrap_or_else(|| format!("Context engine: {name}"));
                let command = extract_yaml_field(&content, "command");
                (desc, command.is_some())
            }
            Err(_) => (format!("Context engine: {name}"), false),
        };

        engines.push((name, description, available));
    }

    engines
}

/// Find and parse the manifest for a named context engine plugin.
///
/// Returns `None` when the plugin directory or manifest does not exist,
/// or when the manifest is missing a `command` field.
pub fn find_context_engine_manifest(name: &str) -> Option<ContextEngineManifest> {
    let dir = context_engine_plugins_dir().join(name);
    if !dir.is_dir() {
        return None;
    }

    let content = std::fs::read_to_string(dir.join("manifest.yaml")).ok()?;
    let command = extract_yaml_field(&content, "command")?;
    let description = extract_yaml_field(&content, "description")
        .unwrap_or_else(|| format!("Context engine: {name}"));

    // Parse `args:` as a simple inline YAML list: ["-m", "my_engine"]
    let args = extract_yaml_list(&content, "args");

    Some(ContextEngineManifest {
        name: name.to_string(),
        description,
        command,
        args,
    })
}

/// Base directory for context engine plugins.
fn context_engine_plugins_dir() -> PathBuf {
    let home = std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".edgecrab")
        });
    home.join("plugins").join("context_engine")
}

/// Simple YAML field extractor (avoids pulling in a full YAML parser just for this).
fn extract_yaml_field(content: &str, field: &str) -> Option<String> {
    let prefix = format!("{field}:");
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let value = rest.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() && !value.starts_with('[') {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Extract an inline YAML list value: `args: ["-m", "engine"]` → `["-m", "engine"]`.
fn extract_yaml_list(content: &str, field: &str) -> Vec<String> {
    let prefix = format!("{field}:");
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let rest = rest.trim();
            if rest.starts_with('[') && rest.ends_with(']') {
                let inner = &rest[1..rest.len() - 1];
                return inner
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_yaml_field_basic() {
        let yaml = r#"
name: my-engine
description: "A test context engine"
command: python
args: ["-m", "my_engine"]
"#;
        assert_eq!(
            extract_yaml_field(yaml, "name"),
            Some("my-engine".to_string())
        );
        assert_eq!(
            extract_yaml_field(yaml, "description"),
            Some("A test context engine".to_string())
        );
        assert_eq!(
            extract_yaml_field(yaml, "command"),
            Some("python".to_string())
        );
        assert_eq!(extract_yaml_field(yaml, "nonexistent"), None);
    }

    #[test]
    fn extract_yaml_list_inline() {
        let yaml = r#"args: ["-m", "my_engine"]"#;
        let args = extract_yaml_list(yaml, "args");
        assert_eq!(args, vec!["-m", "my_engine"]);
    }

    #[test]
    fn extract_yaml_list_missing() {
        let yaml = r#"command: python"#;
        assert!(extract_yaml_list(yaml, "args").is_empty());
    }

    #[test]
    fn discover_returns_empty_for_missing_dir() {
        // In test environments, the plugins/context_engine dir won't exist
        let engines = discover_context_engines();
        // Should not panic, just return empty or whatever is there
        let _ = engines;
    }

    #[test]
    fn find_manifest_returns_none_for_missing() {
        let result = find_context_engine_manifest("__nonexistent_engine__");
        assert!(result.is_none());
    }
}
