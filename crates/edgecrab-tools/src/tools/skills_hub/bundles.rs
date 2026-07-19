//! Skill bundles — multi-skill install groups (Wave C; product-light stub).
//!
//! Hermes `hermes bundles` is a separate product surface. EdgeCrab exposes a
//! minimal JSON bundle file format so CLI/slash have a landing place without
//! committing to a full bundle marketplace.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundleManifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Install identifiers (same forms as `/skills install`).
    pub skills: Vec<String>,
}

/// Load a bundle manifest from JSON (`.json`) or a simple newline list (`.txt`).
pub fn load_bundle_manifest(path: &Path) -> Result<SkillBundleManifest, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read bundle: {e}"))?;
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        return serde_json::from_str(&text).map_err(|e| format!("invalid bundle JSON: {e}"));
    }
    let skills: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    if skills.is_empty() {
        return Err("bundle file has no skill identifiers".into());
    }
    Ok(SkillBundleManifest {
        name: path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "bundle".into()),
        description: String::new(),
        skills,
    })
}

pub fn format_bundle_help() -> String {
    "Skill bundles (Wave C):\n\
       edgecrab skills bundles show <file.json|.txt>\n\
       edgecrab skills bundles install <file.json|.txt> [--force] [--trust]\n\
     Manifest JSON: {\"name\",\"description\",\"skills\":[\"owner/repo/path\", ...]}\n\
     Full Hermes-style bundle marketplace is intentionally out of scope until product confirms."
        .into()
}
