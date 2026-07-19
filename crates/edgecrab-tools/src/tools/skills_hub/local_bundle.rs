//! Build a SkillBundle from a local path (always for quarantine install).

use std::collections::HashMap;
use std::path::Path;

use super::SkillBundle;

/// Build a skill bundle from a local file (SKILL.md) or directory containing SKILL.md.
pub fn build_local_skill_bundle(
    source_path: &Path,
    name_override: Option<&str>,
) -> Result<SkillBundle, String> {
    let skill_name = name_override
        .map(str::to_string)
        .unwrap_or_else(|| derive_skill_name(source_path));

    if skill_name.is_empty()
        || skill_name.contains('/')
        || skill_name.contains('\\')
        || skill_name.contains("..")
    {
        return Err(format!(
            "Derived skill name '{skill_name}' is unsafe; provide an explicit name"
        ));
    }

    let mut files = HashMap::new();
    if source_path.is_file() {
        let content = std::fs::read_to_string(source_path)
            .map_err(|e| format!("Failed to read {}: {e}", source_path.display()))?;
        files.insert("SKILL.md".into(), content);
    } else if source_path.is_dir() {
        let skill_md = source_path.join("SKILL.md");
        if !skill_md.is_file() {
            return Err(format!("No SKILL.md found in {}", source_path.display()));
        }
        collect_local_skill_files(source_path, source_path, &mut files)?;
    } else {
        return Err(format!("Path not found: {}", source_path.display()));
    }

    Ok(SkillBundle {
        name: skill_name,
        files,
        source: "local".into(),
        identifier: source_path.display().to_string(),
        trust_level: "community".into(),
    })
}

fn derive_skill_name(source_path: &Path) -> String {
    if source_path.is_file() {
        return source_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_string();
    }
    source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill")
        .to_string()
}

fn collect_local_skill_files(
    root: &Path,
    dir: &Path,
    files: &mut HashMap<String, String>,
) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("Failed to read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_local_skill_files(root, &path, files)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
            files.insert(rel, content);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn builds_from_directory() {
        let dir = TempDir::new().unwrap();
        let skill = dir.path().join("demo");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: demo\n---\n# Demo\n").unwrap();
        let bundle = build_local_skill_bundle(&skill, None).unwrap();
        assert_eq!(bundle.name, "demo");
        assert!(bundle.files.contains_key("SKILL.md"));
        assert_eq!(bundle.trust_level, "community");
    }
}
