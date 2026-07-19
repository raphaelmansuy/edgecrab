//! Import skills from peer agent homes (Claude Code, Codex, Pi, OpenClaw, agentskills).

use std::path::{Path, PathBuf};

use super::local_bundle::build_local_skill_bundle;
use super::{InstallGate, InstallOutcome, install_skill};

/// Peer agent skill home aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerSkillHome {
    Claude,
    Codex,
    Pi,
    Agents,
    OpenClaw,
}

impl PeerSkillHome {
    /// Canonical CLI/TUI aliases (import-from picker).
    pub const ALL: &'static [Self] = &[
        Self::Claude,
        Self::Codex,
        Self::Pi,
        Self::Agents,
        Self::OpenClaw,
    ];

    pub const ALIASES: &'static [&'static str] =
        &["claude", "codex", "pi", "agents", "openclaw"];

    pub fn parse(alias: &str) -> Option<Self> {
        match alias.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" | "claudecode" => Some(Self::Claude),
            "codex" | "openai-codex" => Some(Self::Codex),
            "pi" | "pi-agent" => Some(Self::Pi),
            "agents" | "agentskills" => Some(Self::Agents),
            "openclaw" | "claw" => Some(Self::OpenClaw),
            _ => None,
        }
    }

    pub fn alias(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::Agents => "agents",
            Self::OpenClaw => "openclaw",
        }
    }

    pub fn default_path(&self) -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        match self {
            Self::Claude => home.join(".claude").join("skills"),
            Self::Codex => home.join(".codex").join("skills"),
            Self::Pi => home.join(".pi").join("agent").join("skills"),
            Self::Agents => home.join(".agents").join("skills"),
            Self::OpenClaw => home.join(".openclaw").join("skills"),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Pi => "Pi",
            Self::Agents => "agentskills",
            Self::OpenClaw => "OpenClaw",
        }
    }
}

/// Preset external_dirs entries for discovery (read-only) without import.
pub fn peer_external_dir_presets() -> Vec<String> {
    vec![
        "~/.claude/skills".into(),
        "~/.codex/skills".into(),
        "~/.pi/agent/skills".into(),
        "~/.agents/skills".into(),
        "~/.openclaw/skills".into(),
    ]
}

/// Resolve import root from alias or filesystem path.
pub fn resolve_import_root(spec: &str) -> Result<(String, PathBuf), String> {
    let spec = spec.trim();
    if let Some(peer) = PeerSkillHome::parse(spec) {
        return Ok((peer.label().into(), peer.default_path()));
    }
    let expanded = shellexpand::tilde(spec).into_owned();
    let path = PathBuf::from(&expanded);
    if path.is_dir() {
        Ok((expanded, path))
    } else {
        Err(format!(
            "Import source '{spec}' not found. Use claude|codex|pi|agents|openclaw or a directory path."
        ))
    }
}

/// Find skill directories (contain SKILL.md) under root, skipping hidden `.system` only when empty of skills.
pub fn discover_skill_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    discover_skill_dirs_inner(root, root, &mut out);
    out.sort();
    out
}

fn discover_skill_dirs_inner(_root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    if dir.join("SKILL.md").is_file() {
        out.push(dir.to_path_buf());
        return; // do not recurse into skill package
    }
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') && name != ".curated" && name != ".system" && name != ".experimental"
        {
            continue;
        }
        discover_skill_dirs_inner(_root, &path, out);
    }
}

/// Import all skills from a peer home / path through quarantine → scan → gate.
pub fn import_skills_from(
    spec: &str,
    skills_dir: &Path,
    gate: InstallGate,
) -> Result<ImportReport, String> {
    let (label, root) = resolve_import_root(spec)?;
    if !root.is_dir() {
        return Err(format!(
            "{} skills directory not found: {}",
            label,
            root.display()
        ));
    }

    let dirs = discover_skill_dirs(&root);
    if dirs.is_empty() {
        return Ok(ImportReport {
            label,
            root: root.display().to_string(),
            installed: Vec::new(),
            skipped: Vec::new(),
            errors: vec!["No SKILL.md directories found.".into()],
        });
    }

    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    let mut errors = Vec::new();

    for dir in dirs {
        match build_local_skill_bundle(&dir, None) {
            Ok(bundle) => {
                let name = bundle.name.clone();
                match install_skill(&bundle, skills_dir, gate) {
                    Ok(msg) => installed.push(ImportItem {
                        name,
                        message: msg,
                    }),
                    Err(e) => {
                        if e.contains("already") || e.to_ascii_lowercase().contains("exists") {
                            skipped.push(format!("{name}: {e}"));
                        } else {
                            errors.push(format!("{name}: {e}"));
                        }
                    }
                }
            }
            Err(e) => errors.push(format!("{}: {e}", dir.display())),
        }
    }

    Ok(ImportReport {
        label,
        root: root.display().to_string(),
        installed,
        skipped,
        errors,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportItem {
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportReport {
    pub label: String,
    pub root: String,
    pub installed: Vec<ImportItem>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

impl ImportReport {
    pub fn format(&self) -> String {
        let mut out = format!(
            "Import from {} ({})\n  installed: {}  skipped: {}  errors: {}\n",
            self.label,
            self.root,
            self.installed.len(),
            self.skipped.len(),
            self.errors.len()
        );
        for item in &self.installed {
            out.push_str(&format!("  ✓ {} — {}\n", item.name, item.message));
        }
        for s in &self.skipped {
            out.push_str(&format!("  · skipped: {s}\n"));
        }
        for e in &self.errors {
            out.push_str(&format!("  ✗ {e}\n"));
        }
        out
    }
}

/// Convenience: import and return outcomes for scripting surfaces.
#[allow(dead_code)]
pub fn import_skills_from_as_outcomes(
    spec: &str,
    skills_dir: &Path,
    gate: InstallGate,
) -> Result<Vec<InstallOutcome>, String> {
    let report = import_skills_from(spec, skills_dir, gate)?;
    Ok(report
        .installed
        .into_iter()
        .map(|i| InstallOutcome {
            message: i.message,
            skill_name: i.name,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome as TestHome;
    use tempfile::TempDir;

    #[test]
    fn discovers_nested_skills() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(a.join("SKILL.md"), "# A\n").unwrap();
        let nested = dir.path().join(".curated").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("SKILL.md"), "# B\n").unwrap();
        let found = discover_skill_dirs(dir.path());
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn import_uses_quarantine_pipeline() {
        let home = TestHome::new();
        let skills_dir = home.path().join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        let peer = TempDir::new().unwrap();
        let skill = peer.path().join("peer-skill");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: peer-skill\ndescription: test\n---\n# Peer\n",
        )
        .unwrap();

        let report = import_skills_from(peer.path().to_str().unwrap(), &skills_dir, InstallGate::default())
            .unwrap();
        assert_eq!(report.installed.len(), 1);
        assert!(skills_dir.join("peer-skill").join("SKILL.md").is_file());
        // quarantine should be empty or cleaned after success
        let q = home.path().join("skills").join(".hub").join("quarantine");
        if q.is_dir() {
            let leftovers: Vec<_> = std::fs::read_dir(&q).unwrap().flatten().collect();
            assert!(leftovers.is_empty(), "quarantine should be cleaned after commit");
        }
    }
}
