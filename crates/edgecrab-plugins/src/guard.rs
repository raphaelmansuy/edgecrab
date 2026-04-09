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

struct ThreatPattern {
    needle: &'static str,
    pattern_id: &'static str,
    severity: Severity,
    category: ThreatCategory,
    description: &'static str,
}

const THREAT_PATTERNS: &[ThreatPattern] = &[
    ThreatPattern {
        needle: "~/.edgecrab/.env",
        pattern_id: "edgecrab_env_access",
        severity: Severity::Critical,
        category: ThreatCategory::Exfiltration,
        description: "references the EdgeCrab environment file",
    },
    ThreatPattern {
        needle: "os.getenv(",
        pattern_id: "python_getenv_secret",
        severity: Severity::High,
        category: ThreatCategory::Exfiltration,
        description: "reads process environment variables",
    },
    ThreatPattern {
        needle: "process.env",
        pattern_id: "node_process_env",
        severity: Severity::High,
        category: ThreatCategory::Exfiltration,
        description: "reads Node.js environment variables",
    },
    ThreatPattern {
        needle: "ignore previous instructions",
        pattern_id: "prompt_injection_ignore",
        severity: Severity::Critical,
        category: ThreatCategory::Injection,
        description: "contains a prompt-injection override",
    },
    ThreatPattern {
        needle: "system prompt override",
        pattern_id: "sys_prompt_override",
        severity: Severity::Critical,
        category: ThreatCategory::Injection,
        description: "attempts to override the system prompt",
    },
    ThreatPattern {
        needle: "rm -rf /",
        pattern_id: "destructive_root_rm",
        severity: Severity::Critical,
        category: ThreatCategory::Destructive,
        description: "contains a destructive root deletion command",
    },
    ThreatPattern {
        needle: "chmod 777",
        pattern_id: "insecure_perms",
        severity: Severity::Medium,
        category: ThreatCategory::Destructive,
        description: "sets world-writable permissions",
    },
    ThreatPattern {
        needle: "crontab",
        pattern_id: "persistence_cron",
        severity: Severity::Medium,
        category: ThreatCategory::Persistence,
        description: "installs cron persistence",
    },
    ThreatPattern {
        needle: "launchctl load",
        pattern_id: "macos_launchd",
        severity: Severity::Medium,
        category: ThreatCategory::Persistence,
        description: "loads a launchd persistence job",
    },
    ThreatPattern {
        needle: "socket.connect((",
        pattern_id: "python_socket_connect",
        severity: Severity::High,
        category: ThreatCategory::Network,
        description: "opens an outbound socket connection",
    },
    ThreatPattern {
        needle: "curl | bash",
        pattern_id: "curl_pipe_shell",
        severity: Severity::Critical,
        category: ThreatCategory::Obfuscation,
        description: "pipes remote content directly into a shell",
    },
    ThreatPattern {
        needle: "base64 -d |",
        pattern_id: "base64_decode_pipe",
        severity: Severity::High,
        category: ThreatCategory::Obfuscation,
        description: "decodes base64 into execution",
    },
    ThreatPattern {
        needle: "subprocess.run(",
        pattern_id: "python_subprocess",
        severity: Severity::Medium,
        category: ThreatCategory::Execution,
        description: "spawns a subprocess from Python",
    },
    ThreatPattern {
        needle: "child_process.exec(",
        pattern_id: "node_child_process",
        severity: Severity::High,
        category: ThreatCategory::Execution,
        description: "spawns a subprocess from Node.js",
    },
    ThreatPattern {
        needle: "../..",
        pattern_id: "path_traversal",
        severity: Severity::Medium,
        category: ThreatCategory::Traversal,
        description: "contains a path traversal sequence",
    },
];

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
            let lowered = line.to_ascii_lowercase();
            for pattern in THREAT_PATTERNS {
                if lowered.contains(&pattern.needle.to_ascii_lowercase()) {
                    findings.push(ScanFinding {
                        pattern_id: pattern.pattern_id.to_string(),
                        severity: pattern.severity,
                        category: pattern.category,
                        file: relative.clone(),
                        line: line_idx + 1,
                        excerpt: line.trim().to_string(),
                        description: pattern.description.to_string(),
                    });
                }
            }
        }
    }
    Ok(())
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
