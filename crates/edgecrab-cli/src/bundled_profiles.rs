use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy)]
pub struct BundledProfileFile {
    pub relative_path: &'static str,
    pub content: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct BundledProfileTemplate {
    pub name: &'static str,
    pub files: &'static [BundledProfileFile],
}

pub const BUNDLED_PROFILES: &[BundledProfileTemplate] = &[
    BundledProfileTemplate {
        name: "work",
        files: &[
            BundledProfileFile {
                relative_path: "config.yaml",
                content: include_str!("../bundled-profiles/work/config.yaml"),
            },
            BundledProfileFile {
                relative_path: "SOUL.md",
                content: include_str!("../bundled-profiles/work/SOUL.md"),
            },
            BundledProfileFile {
                relative_path: "memories/USER.md",
                content: include_str!("../bundled-profiles/work/memories/USER.md"),
            },
            BundledProfileFile {
                relative_path: "memories/MEMORY.md",
                content: include_str!("../bundled-profiles/work/memories/MEMORY.md"),
            },
        ],
    },
    BundledProfileTemplate {
        name: "research",
        files: &[
            BundledProfileFile {
                relative_path: "config.yaml",
                content: include_str!("../bundled-profiles/research/config.yaml"),
            },
            BundledProfileFile {
                relative_path: "SOUL.md",
                content: include_str!("../bundled-profiles/research/SOUL.md"),
            },
            BundledProfileFile {
                relative_path: "memories/USER.md",
                content: include_str!("../bundled-profiles/research/memories/USER.md"),
            },
            BundledProfileFile {
                relative_path: "memories/MEMORY.md",
                content: include_str!("../bundled-profiles/research/memories/MEMORY.md"),
            },
        ],
    },
    BundledProfileTemplate {
        name: "homelab",
        files: &[
            BundledProfileFile {
                relative_path: "config.yaml",
                content: include_str!("../bundled-profiles/homelab/config.yaml"),
            },
            BundledProfileFile {
                relative_path: "SOUL.md",
                content: include_str!("../bundled-profiles/homelab/SOUL.md"),
            },
            BundledProfileFile {
                relative_path: "memories/USER.md",
                content: include_str!("../bundled-profiles/homelab/memories/USER.md"),
            },
            BundledProfileFile {
                relative_path: "memories/MEMORY.md",
                content: include_str!("../bundled-profiles/homelab/memories/MEMORY.md"),
            },
        ],
    },
];

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BundledProfileSyncReport {
    pub created: Vec<String>,
    pub skipped_existing: Vec<String>,
}

impl BundledProfileSyncReport {
    pub fn summary(&self) -> String {
        if self.created.is_empty() {
            "No bundled profile changes".into()
        } else {
            format!("Seeded bundled profiles: {}", self.created.join(", "))
        }
    }
}

pub fn sync_bundled_profiles<F>(
    edgecrab_home: &Path,
    mut bootstrap_profile_home: F,
) -> Result<BundledProfileSyncReport>
where
    F: FnMut(&Path) -> Result<()>,
{
    let profiles_root = edgecrab_home.join("profiles");
    fs::create_dir_all(&profiles_root)
        .with_context(|| format!("Creating {}", profiles_root.display()))?;

    let mut report = BundledProfileSyncReport::default();

    for template in BUNDLED_PROFILES {
        let dest = profiles_root.join(template.name);
        if dest.exists() {
            report.skipped_existing.push(template.name.to_string());
            continue;
        }

        bootstrap_profile_home(&dest)?;
        for file in template.files {
            let path = dest.join(file.relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Creating {}", parent.display()))?;
            }
            fs::write(&path, file.content)
                .with_context(|| format!("Writing {}", path.display()))?;
        }
        report.created.push(template.name.to_string());
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sync_bundled_profiles_creates_missing_templates_once() {
        let temp = TempDir::new().expect("tempdir");
        let report = sync_bundled_profiles(temp.path(), |dest| {
            fs::create_dir_all(dest)?;
            Ok(())
        })
        .expect("sync");

        assert!(report.created.iter().any(|name| name == "work"));
        assert!(
            temp.path()
                .join("profiles")
                .join("work")
                .join("SOUL.md")
                .exists()
        );

        let second = sync_bundled_profiles(temp.path(), |dest| {
            fs::create_dir_all(dest)?;
            Ok(())
        })
        .expect("sync");
        assert!(second.created.is_empty());
        assert!(second.skipped_existing.iter().any(|name| name == "work"));
    }
}
