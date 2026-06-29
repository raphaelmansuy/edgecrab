//! AST-style deep audit for skill Python files — opt-in diagnostic, not a security gate.
//!
//! Hermes parity (`tools/skills_ast_audit.py`): flags dynamic import / attribute access
//! patterns for human review. Findings are hints, not verdicts.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstFinding {
    pub file: String,
    pub line: usize,
    pub pattern_id: String,
    pub description: String,
}

const IGNORED_DIRS: &[&str] = &["__pycache__", ".venv", "venv", "node_modules", ".git"];

/// Scan a `.py` file or recursively scan all Python under a directory.
pub fn ast_scan_path(path: &Path) -> Vec<AstFinding> {
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()).unwrap_or("") != "py" {
            return Vec::new();
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let rel = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown.py".into());
        return scan_python_source(&content, &rel);
    }

    if !path.is_dir() {
        return Vec::new();
    }

    let mut out = Vec::new();
    collect_py_findings(path, path, &mut out);
    out.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.line.cmp(&b.line)));
    out
}

fn collect_py_findings(dir: &Path, root: &Path, out: &mut Vec<AstFinding>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            if IGNORED_DIRS.iter().any(|d| name == *d) {
                continue;
            }
            collect_py_findings(&path, root, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("py") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if let Ok(content) = std::fs::read_to_string(&path) {
                out.extend(scan_python_source(&content, &rel));
            }
        }
    }
}

fn scan_python_source(content: &str, rel_path: &str) -> Vec<AstFinding> {
    let mut findings = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }

        if line.contains("importlib.import_module") {
            findings.push(AstFinding {
                file: rel_path.to_string(),
                line: line_no,
                pattern_id: "dynamic_import".into(),
                description: "importlib.import_module() — loads arbitrary modules at runtime"
                    .into(),
            });
        }
        if line.contains("__import__(") {
            findings.push(AstFinding {
                file: rel_path.to_string(),
                line: line_no,
                pattern_id: "dynamic_import_computed".into(),
                description: "__import__(…) — dynamic module load".into(),
            });
        }
        if line.contains("getattr(") {
            findings.push(AstFinding {
                file: rel_path.to_string(),
                line: line_no,
                pattern_id: "dynamic_getattr".into(),
                description: "getattr(…) — dynamic attribute access".into(),
            });
        }
        if line.contains("__dict__[") {
            findings.push(AstFinding {
                file: rel_path.to_string(),
                line: line_no,
                pattern_id: "dict_access".into(),
                description: "__dict__[…] — dynamic attribute access".into(),
            });
        }
        if trimmed.starts_with("import importlib") || trimmed.contains(" import importlib") {
            findings.push(AstFinding {
                file: rel_path.to_string(),
                line: line_no,
                pattern_id: "importlib_import".into(),
                description: "import importlib — enables dynamic module loading".into(),
            });
        }
        if trimmed.starts_with("from importlib") {
            findings.push(AstFinding {
                file: rel_path.to_string(),
                line: line_no,
                pattern_id: "importlib_import".into(),
                description: "from importlib import … — enables dynamic module loading".into(),
            });
        }
    }
    findings
}

pub fn format_ast_report(findings: &[AstFinding], skill_name: &str) -> String {
    let header = if skill_name.is_empty() {
        "AST deep scan".to_string()
    } else {
        format!("AST deep scan: {skill_name}")
    };
    if findings.is_empty() {
        return format!("{header}\n  No dynamic import/access patterns detected.\n");
    }

    let mut lines = vec![header, format!("  {} finding(s):", findings.len())];
    let mut current: Option<&str> = None;
    for f in findings {
        if current != Some(f.file.as_str()) {
            current = Some(&f.file);
            lines.push(format!("  {}", f.file));
        }
        lines.push(format!(
            "    L{}  {}  — {}",
            f.line, f.pattern_id, f.description
        ));
    }
    lines.push(String::new());
    lines.push("  Note: diagnostic hints for human review, not security verdicts.".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_importlib_in_py_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("runner.py");
        let mut f = std::fs::File::create(&path).expect("create");
        writeln!(f, "import importlib").expect("write");
        writeln!(f, "importlib.import_module(name)").expect("write");

        let findings = ast_scan_path(&path);
        assert!(findings.iter().any(|f| f.pattern_id == "importlib_import"));
        assert!(findings.iter().any(|f| f.pattern_id == "dynamic_import"));
    }

    #[test]
    fn format_empty_ast_report() {
        let text = format_ast_report(&[], "demo");
        assert!(text.contains("No dynamic import"));
    }
}
