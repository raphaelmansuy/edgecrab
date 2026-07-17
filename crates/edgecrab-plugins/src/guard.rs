use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::types::TrustLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    fn weight(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreatCategory {
    Exfiltration,
    Injection,
    Destructive,
    Persistence,
    Network,
    Obfuscation,
    Execution,
    Traversal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanFinding {
    pub pattern_id: String,
    pub severity: Severity,
    pub category: ThreatCategory,
    pub file: PathBuf,
    pub line: usize,
    pub excerpt: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanVerdict {
    Safe,
    Caution,
    Dangerous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub plugin_name: String,
    pub source: String,
    pub trust_level: TrustLevel,
    pub verdict: ScanVerdict,
    pub findings: Vec<ScanFinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerdictResult {
    pub allowed: bool,
    pub forced: bool,
}

// Threat needles live in `edgecrab_security::threat_patterns` (gap 031 SoT).

pub fn scan_plugin_bundle(
    plugin_dir: &Path,
    plugin_name: &str,
    source: &str,
    trust_level: TrustLevel,
) -> std::io::Result<ScanResult> {
    let mut findings = Vec::new();
    scan_dir(plugin_dir, plugin_dir, &mut findings)?;
    let verdict = findings
        .iter()
        .map(|finding| finding.severity.weight())
        .max()
        .map(|max| {
            if max >= Severity::High.weight() {
                ScanVerdict::Dangerous
            } else {
                ScanVerdict::Caution
            }
        })
        .unwrap_or(ScanVerdict::Safe);
    Ok(ScanResult {
        plugin_name: plugin_name.to_string(),
        source: source.to_string(),
        trust_level,
        verdict,
        findings,
    })
}

fn scan_dir(root: &Path, dir: &Path, findings: &mut Vec<ScanFinding>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            scan_dir(root, &path, findings)?;
            continue;
        }
        if !is_scan_candidate(&path) {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let relative = path
            .strip_prefix(root)
            .map(PathBuf::from)
            .unwrap_or_else(|_| path.clone());
        for (line_idx, line) in content.lines().enumerate() {
            let result = edgecrab_security::threat_patterns::scan(
                line,
                edgecrab_security::threat_patterns::ScanContext::Install,
            );
            for f in result.findings {
                findings.push(ScanFinding {
                    pattern_id: f.pattern_id.to_string(),
                    severity: map_severity(f.severity),
                    category: map_category(f.category),
                    file: relative.clone(),
                    line: line_idx + 1,
                    excerpt: line.trim().to_string(),
                    description: f.description.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn map_severity(s: edgecrab_security::threat_patterns::ThreatSeverity) -> Severity {
    match s {
        edgecrab_security::threat_patterns::ThreatSeverity::Low => Severity::Low,
        edgecrab_security::threat_patterns::ThreatSeverity::Medium => Severity::Medium,
        edgecrab_security::threat_patterns::ThreatSeverity::High => Severity::High,
        edgecrab_security::threat_patterns::ThreatSeverity::Critical => Severity::Critical,
    }
}

fn map_category(c: edgecrab_security::threat_patterns::ThreatCategory) -> ThreatCategory {
    match c {
        edgecrab_security::threat_patterns::ThreatCategory::Exfiltration => {
            ThreatCategory::Exfiltration
        }
        edgecrab_security::threat_patterns::ThreatCategory::Injection
        | edgecrab_security::threat_patterns::ThreatCategory::Brainworm => {
            ThreatCategory::Injection
        }
        edgecrab_security::threat_patterns::ThreatCategory::Destructive => {
            ThreatCategory::Destructive
        }
        edgecrab_security::threat_patterns::ThreatCategory::Persistence => {
            ThreatCategory::Persistence
        }
        edgecrab_security::threat_patterns::ThreatCategory::Network => ThreatCategory::Network,
        edgecrab_security::threat_patterns::ThreatCategory::Obfuscation => {
            ThreatCategory::Obfuscation
        }
        edgecrab_security::threat_patterns::ThreatCategory::Execution => ThreatCategory::Execution,
        edgecrab_security::threat_patterns::ThreatCategory::Traversal => ThreatCategory::Traversal,
    }
}

fn is_scan_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("md" | "txt" | "toml" | "json" | "yaml" | "yml" | "py" | "js" | "ts" | "rhai" | "sh")
    )
}

pub fn should_allow_install(
    trust_level: TrustLevel,
    scan: &ScanResult,
    allow_caution: bool,
    force: bool,
) -> VerdictResult {
    match scan.verdict {
        ScanVerdict::Safe => VerdictResult {
            allowed: true,
            forced: false,
        },
        ScanVerdict::Caution => {
            let allowed = force || allow_caution || matches!(trust_level, TrustLevel::Official);
            VerdictResult {
                allowed,
                forced: allowed && scan.verdict != ScanVerdict::Safe,
            }
        }
        ScanVerdict::Dangerous => VerdictResult {
            allowed: force && matches!(trust_level, TrustLevel::Official | TrustLevel::Trusted),
            forced: force,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner_marks_dangerous_when_critical_pattern_found() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("plugin.py"),
            "print('x')\nwith open('~/.edgecrab/.env') as fh:\n    print(fh.read())\n",
        )
        .expect("write plugin");

        let result = scan_plugin_bundle(temp.path(), "demo", "local", TrustLevel::Unverified)
            .expect("scan succeeds");
        assert_eq!(result.verdict, ScanVerdict::Dangerous);
        assert_eq!(result.findings[0].pattern_id, "edgecrab_env_access");
    }

    #[test]
    fn caution_result_can_be_forced() {
        let scan = ScanResult {
            plugin_name: "demo".into(),
            source: "local".into(),
            trust_level: TrustLevel::Community,
            verdict: ScanVerdict::Caution,
            findings: Vec::new(),
        };
        assert!(should_allow_install(TrustLevel::Community, &scan, false, true).allowed);
    }
}
