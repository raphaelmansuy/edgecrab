//! Progressive subdirectory AGENTS.md discovery (Hermes `subdirectory_hints.py` parity).
//!
//! As the agent navigates into subdirectories via tool calls, discover project
//! context files and append them to the **tool result** — never the system
//! prompt — so Anthropic prefix caches stay warm.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::prompt_builder::{ThreatSeverity, scan_for_injection};

const HINT_FILENAMES: &[&str] = &[
    "AGENTS.md",
    "agents.md",
    "CLAUDE.md",
    "claude.md",
    ".cursorrules",
];

const MAX_HINT_CHARS: usize = 8_000;
const PATH_ARG_KEYS: &[&str] = &["path", "file_path", "workdir"];
const COMMAND_TOOLS: &[&str] = &["terminal"];
const MAX_ANCESTOR_WALK: usize = 5;

/// Track which directories already contributed hints this session.
pub struct SubdirectoryHintTracker {
    working_dir: PathBuf,
    loaded_dirs: HashSet<PathBuf>,
}

impl SubdirectoryHintTracker {
    pub fn new(working_dir: impl Into<PathBuf>) -> Self {
        let working_dir = working_dir
            .into()
            .canonicalize()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let mut loaded_dirs = HashSet::new();
        loaded_dirs.insert(working_dir.clone());
        Self {
            working_dir,
            loaded_dirs,
        }
    }

    /// Check tool-call args for new directories; return text to append to the tool result.
    pub fn check_tool_call(
        &mut self,
        tool_name: &str,
        tool_args: &serde_json::Value,
    ) -> Option<String> {
        let dirs = self.extract_directories(tool_name, tool_args);
        if dirs.is_empty() {
            return None;
        }
        let mut all_hints = Vec::new();
        for dir in dirs {
            if let Some(hint) = self.load_hints_for_directory(&dir) {
                all_hints.push(hint);
            }
        }
        if all_hints.is_empty() {
            return None;
        }
        Some(format!("\n\n{}", all_hints.join("\n\n")))
    }

    fn extract_directories(&self, tool_name: &str, args: &serde_json::Value) -> Vec<PathBuf> {
        let mut candidates = HashSet::new();
        if let Some(obj) = args.as_object() {
            for key in PATH_ARG_KEYS {
                if let Some(val) = obj.get(*key).and_then(|v| v.as_str())
                    && !val.trim().is_empty()
                {
                    self.add_path_candidate(val, &mut candidates);
                }
            }
            if COMMAND_TOOLS.contains(&tool_name)
                && let Some(cmd) = obj.get("command").and_then(|v| v.as_str())
            {
                self.extract_paths_from_command(cmd, &mut candidates);
            }
        }
        candidates.into_iter().collect()
    }

    fn add_path_candidate(&self, raw_path: &str, candidates: &mut HashSet<PathBuf>) {
        let mut p = PathBuf::from(raw_path.trim());
        if let Some(stripped) = raw_path.strip_prefix("~/")
            && let Some(home) = dirs::home_dir()
        {
            p = home.join(stripped);
        }
        if !p.is_absolute() {
            p = self.working_dir.join(&p);
        }
        let resolved = p.canonicalize().unwrap_or_else(|_| {
            p.parent()
                .and_then(|parent| parent.canonicalize().ok())
                .map(|parent| parent.join(p.file_name().unwrap_or_default()))
                .unwrap_or(p)
        });
        let mut dir = if resolved.extension().is_some() || resolved.is_file() {
            resolved
                .parent()
                .unwrap_or(resolved.as_path())
                .to_path_buf()
        } else {
            resolved
        };
        for _ in 0..MAX_ANCESTOR_WALK {
            if self.loaded_dirs.contains(&dir) {
                break;
            }
            if self.is_valid_subdir(&dir) {
                candidates.insert(dir.clone());
            }
            let parent = match dir.parent() {
                Some(parent) if parent != dir => parent.to_path_buf(),
                _ => break,
            };
            dir = parent;
        }
    }

    fn extract_paths_from_command(&self, cmd: &str, candidates: &mut HashSet<PathBuf>) {
        for token in cmd.split_whitespace() {
            if token.starts_with('-') {
                continue;
            }
            if !token.contains('/') && !token.contains('.') {
                continue;
            }
            if token.starts_with("http://")
                || token.starts_with("https://")
                || token.starts_with("git@")
            {
                continue;
            }
            self.add_path_candidate(token, candidates);
        }
    }

    fn is_valid_subdir(&self, path: &Path) -> bool {
        if !path.is_dir() || self.loaded_dirs.contains(path) {
            return false;
        }
        path.starts_with(&self.working_dir)
    }

    fn load_hints_for_directory(&mut self, directory: &Path) -> Option<String> {
        self.loaded_dirs.insert(directory.to_path_buf());
        if !directory.starts_with(&self.working_dir) {
            return None;
        }
        for filename in HINT_FILENAMES {
            let hint_path = directory.join(filename);
            if !hint_path.is_file() {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&hint_path) else {
                continue;
            };
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let threats = scan_for_injection(trimmed);
            let content = if threats
                .iter()
                .any(|t| matches!(t.severity, ThreatSeverity::High))
            {
                format!("[BLOCKED: high-severity injection patterns in {filename}]")
            } else {
                trimmed.to_string()
            };
            let content = if content.chars().count() > MAX_HINT_CHARS {
                let end = content
                    .char_indices()
                    .nth(MAX_HINT_CHARS)
                    .map(|(i, _)| i)
                    .unwrap_or(content.len());
                format!(
                    "{}\n\n[...truncated {filename}: {} chars total]",
                    &content[..end],
                    content.chars().count()
                )
            } else {
                content
            };
            let rel = hint_path
                .strip_prefix(&self.working_dir)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| hint_path.display().to_string());
            return Some(format!(
                "[Subdirectory context discovered: {rel}]\n{content}"
            ));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn discovers_agents_md_on_first_subdir_read() {
        let dir = TempDir::new().expect("tmpdir");
        let root = dir.path();
        let sub = root.join("backend");
        std::fs::create_dir_all(&sub).expect("mkdir");
        std::fs::write(sub.join("AGENTS.md"), "# Backend rules\nUse async.").expect("write");
        std::fs::write(sub.join("main.rs"), "fn main() {}").expect("write");

        let mut tracker = SubdirectoryHintTracker::new(root);
        let args = serde_json::json!({"path": "backend/main.rs"});
        let hint = tracker.check_tool_call("read_file", &args).expect("hint");
        assert!(hint.contains("Backend rules"));
        assert!(hint.contains("Subdirectory context discovered"));

        // Second access is a no-op (already loaded).
        assert!(tracker.check_tool_call("read_file", &args).is_none());
    }

    #[test]
    fn rejects_paths_outside_working_dir() {
        let dir = TempDir::new().expect("tmpdir");
        let mut tracker = SubdirectoryHintTracker::new(dir.path());
        let args = serde_json::json!({"path": "/tmp/outside/file.rs"});
        assert!(tracker.check_tool_call("read_file", &args).is_none());
    }
}
