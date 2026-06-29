use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config_ref::resolve_edgecrab_home;

/// Merge global `skills.disabled` with optional platform-specific overrides.
pub fn merge_disabled_skills(
    global: &[String],
    platform_disabled: &HashMap<String, Vec<String>>,
    platform: Option<&str>,
) -> Vec<String> {
    let mut out = global.to_vec();
    if let Some(platform) = platform
        && let Some(list) = platform_disabled.get(platform)
    {
        out.extend(list.iter().cloned());
    }
    out
}

/// Scan-time context for skill slash commands and bundles.
#[derive(Debug, Clone)]
pub struct SkillsScanContext {
    pub edgecrab_home: PathBuf,
    pub external_skill_dirs: Vec<String>,
    pub disabled_skills: HashSet<String>,
    /// When false (gateway/messaging), missing env vars get a non-interactive setup hint.
    pub interactive: bool,
}

impl SkillsScanContext {
    pub fn from_home(edgecrab_home: &Path) -> Self {
        Self {
            edgecrab_home: edgecrab_home.to_path_buf(),
            external_skill_dirs: Vec::new(),
            disabled_skills: HashSet::new(),
            interactive: true,
        }
    }

    pub fn with_interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    pub fn with_external_dirs(mut self, dirs: &[String]) -> Self {
        self.external_skill_dirs = dirs.to_vec();
        self
    }

    pub fn with_disabled(mut self, disabled: &[String]) -> Self {
        self.disabled_skills = disabled.iter().map(|s| s.to_ascii_lowercase()).collect();
        self
    }

    /// Build scan context with global + platform-specific disabled skill names.
    pub fn from_config(
        edgecrab_home: &Path,
        external_dirs: &[String],
        global_disabled: &[String],
        platform_disabled: &HashMap<String, Vec<String>>,
        platform: Option<&str>,
    ) -> Self {
        let disabled = merge_disabled_skills(global_disabled, platform_disabled, platform);
        Self::from_home(edgecrab_home)
            .with_external_dirs(external_dirs)
            .with_disabled(&disabled)
    }

    pub fn default_home() -> Self {
        Self::from_home(&resolve_edgecrab_home())
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.edgecrab_home.join("skills")
    }

    pub fn bundles_dir(&self) -> PathBuf {
        self.edgecrab_home.join("skill-bundles")
    }
}

impl Default for SkillsScanContext {
    fn default() -> Self {
        Self::default_home()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn merge_disabled_includes_platform_overrides() {
        let mut platform_disabled = HashMap::new();
        platform_disabled.insert("telegram".into(), vec!["secret".into()]);
        let merged =
            merge_disabled_skills(&["global".into()], &platform_disabled, Some("telegram"));
        assert!(merged.contains(&"global".into()));
        assert!(merged.contains(&"secret".into()));
        let cli_only = merge_disabled_skills(&["global".into()], &platform_disabled, Some("cli"));
        assert!(!cli_only.contains(&"secret".into()));
    }
}
