use std::path::{Path, PathBuf};

use edgecrab_types::Platform;

use crate::error::PluginError;
use crate::skill::manifest::{SkillManifest, parse_skill_manifest};
use crate::skill::platform::skill_matches_platform;
use crate::skill::readiness::resolve_skill_readiness;
use crate::types::{SkillReadinessStatus, SkillSource};

const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".github",
    ".quarantine",
    ".hub",
    "node_modules",
    "target",
    "__pycache__",
];

#[derive(Debug, Clone)]
pub struct LoadedSkill {
    pub path: PathBuf,
    pub source: SkillSource,
    pub manifest: SkillManifest,
    pub readiness: SkillReadinessStatus,
    pub platform_visible: bool,
}

pub fn scan_skills_dir(
    root: &Path,
    source: SkillSource,
    disabled: &[String],
    _platform: Platform,
) -> Result<Vec<LoadedSkill>, PluginError> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for skill_file in candidate_skill_files(root, 0)? {
        let manifest = parse_skill_manifest(&skill_file)?;
        if disabled.iter().any(|candidate| candidate == &manifest.name) {
            continue;
        }
        let required_env = manifest
            .required_environment_variables
            .iter()
            .map(|entry| entry.name.clone())
            .chain(
                manifest
                    .collect_secrets
                    .iter()
                    .map(|entry| entry.env_var.clone()),
            )
            .collect::<Vec<_>>();
        out.push(LoadedSkill {
            path: skill_file,
            source,
            platform_visible: skill_matches_platform(&manifest.platforms),
            readiness: resolve_skill_readiness(&required_env),
            manifest,
        });
    }

    out.sort_by(|left, right| left.manifest.name.cmp(&right.manifest.name));
    Ok(out)
}

fn candidate_skill_files(root: &Path, depth: usize) -> Result<Vec<PathBuf>, PluginError> {
    if depth > 2 {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let root_skill = root.join("SKILL.md");
    if root_skill.is_file() {
        out.push(root_skill);
    }
    if depth >= 2 {
        return Ok(out);
    }

    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if EXCLUDED_DIRS
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(name))
        {
            continue;
        }
        let file = path.join("SKILL.md");
        if file.is_file() {
            out.push(file);
        }
        out.extend(candidate_skill_files(&path, depth + 1)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_detects_root_and_depth_two_skills_but_not_depth_three() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("SKILL.md"),
            skill_doc("root-skill", "Root"),
        )
        .expect("write root");
        std::fs::create_dir_all(temp.path().join("one/two/three")).expect("dirs");
        std::fs::write(
            temp.path().join("one/SKILL.md"),
            skill_doc("one-skill", "One"),
        )
        .expect("write one");
        std::fs::write(
            temp.path().join("one/two/SKILL.md"),
            skill_doc("two-skill", "Two"),
        )
        .expect("write two");
        std::fs::write(
            temp.path().join("one/two/three/SKILL.md"),
            skill_doc("three-skill", "Three"),
        )
        .expect("write three");

        let loaded = scan_skills_dir(temp.path(), SkillSource::User, &[], Platform::Cli)
            .expect("skills load");

        let names = loaded
            .into_iter()
            .map(|skill| skill.manifest.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"root-skill".to_string()));
        assert!(names.contains(&"one-skill".to_string()));
        assert!(names.contains(&"two-skill".to_string()));
        assert!(!names.contains(&"three-skill".to_string()));
    }

    fn skill_doc(name: &str, title: &str) -> String {
        format!("---\nname: {name}\ndescription: {title}\n---\n\n# {title}\n\nBody.\n")
    }
}
