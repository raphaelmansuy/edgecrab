//! Deterministic harness gates for the agent control plane.
//!
//! First principle: completion must depend on **structured facts** (JSON tool
//! errors, exit codes, mutation debt), not NLP heuristics on assistant prose.
//!
//! Single responsibility: collect gate evidence. [`HarnessSnapshot::assess`] is
//! pure; oracle subprocesses run only in [`build_harness_snapshot`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use edgecrab_types::{Message, Role, parse_tool_error_payload};

use crate::mutations::{FILE_MUTATING_TOOLS, MutationRecord, MutationTurnState, file_mutation_result_landed};

/// Immutable gate state consumed by completion policy (pure assessment).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarnessSnapshot {
    /// Paths still marked failed in [`MutationTurnState`] (no later success cleared them).
    pub unresolved_mutation_failures: Vec<UnresolvedMutationFailure>,
    /// The chronologically last file-mutation tool result was a structured error.
    pub terminal_mutation_tool_error: Option<TerminalMutationToolError>,
    /// Post-mutation oracle failures (exit code ≠ 0).
    pub oracle_failures: Vec<OracleGateFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedMutationFailure {
    pub path: String,
    pub tool: String,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalMutationToolError {
    pub tool: String,
    pub code: String,
    pub code_num: u16,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleGateFailure {
    pub path: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stderr_preview: String,
}

/// Input for [`build_harness_snapshot`].
pub struct HarnessBuildInput<'a> {
    pub messages: &'a [Message],
    pub mutation_turn: &'a MutationTurnState,
    pub cwd: &'a Path,
    pub post_mutation_oracles: bool,
}

/// Collect deterministic gate facts at end-of-turn (may run subprocess oracles).
pub fn build_harness_snapshot(input: HarnessBuildInput<'_>) -> HarnessSnapshot {
    let unresolved_mutation_failures = input
        .mutation_turn
        .peek_failed()
        .into_iter()
        .map(|(path, (tool, preview))| UnresolvedMutationFailure {
            path,
            tool,
            preview,
        })
        .collect();

    let terminal_mutation_tool_error = terminal_mutation_tool_error(input.messages);

    let oracle_failures = if input.post_mutation_oracles {
        let paths = mutated_paths_for_oracles(input.mutation_turn.peek_success_records());
        run_post_mutation_oracles(input.cwd, &paths)
    } else {
        Vec::new()
    };

    HarnessSnapshot {
        unresolved_mutation_failures,
        terminal_mutation_tool_error,
        oracle_failures,
    }
}

impl HarnessSnapshot {
    /// True when any deterministic gate blocks declaring the run complete.
    pub fn blocks_completion(&self) -> bool {
        !self.unresolved_mutation_failures.is_empty()
            || self.terminal_mutation_tool_error.is_some()
            || !self.oracle_failures.is_empty()
    }

    /// User-facing completion denial (no LLM judgment).
    pub fn completion_block_reason(&self) -> Option<String> {
        if let Some(err) = &self.terminal_mutation_tool_error {
            return Some(format!(
                "Incomplete — last file mutation tool failed ({}, code {}): {}",
                err.tool, err.code_num, err.message
            ));
        }
        if !self.unresolved_mutation_failures.is_empty() {
            let n = self.unresolved_mutation_failures.len();
            return Some(format!(
                "Incomplete — {n} file mutation(s) failed and were not recovered this turn."
            ));
        }
        if let Some(oracle) = self.oracle_failures.first() {
            return Some(format!(
                "Incomplete — post-mutation gate failed for `{}` ({})",
                oracle.path, oracle.command
            ));
        }
        None
    }

    /// Optional footer appended after mutation verifier (gate evidence, not mutation log).
    pub fn render_gate_footer(&self) -> Option<String> {
        if !self.blocks_completion() {
            return None;
        }
        let mut lines = vec![
            "─── harness-gates (deterministic) ─────────────────".to_string(),
        ];
        if let Some(err) = &self.terminal_mutation_tool_error {
            lines.push(format!(
                "  ✗ last mutation tool `{}` → {} ({})",
                err.tool, err.code, err.message
            ));
        }
        for item in &self.unresolved_mutation_failures {
            lines.push(format!(
                "  ✗ unresolved {} on `{}` — {}",
                item.tool, item.path, item.preview
            ));
        }
        for oracle in &self.oracle_failures {
            lines.push(format!(
                "  ✗ oracle `{}` exit {:?} — {}",
                oracle.command,
                oracle.exit_code,
                oracle.stderr_preview
            ));
        }
        lines.push("───────────────────────────────────────────────────".to_string());
        Some(lines.join("\n"))
    }
}

/// Walk tool history backwards; the latest file-mutation tool result wins.
pub fn terminal_mutation_tool_error(messages: &[Message]) -> Option<TerminalMutationToolError> {
    for msg in messages.iter().rev() {
        if msg.role != Role::Tool {
            continue;
        }
        let name = msg.name.as_deref()?;
        if !FILE_MUTATING_TOOLS.contains(&name) {
            continue;
        }
        let content = msg.text_content();
        if let Some(payload) = parse_tool_error_payload(&content) {
            return Some(TerminalMutationToolError {
                tool: name.to_string(),
                code: payload.code,
                code_num: payload.code_num,
                message: payload.error,
            });
        }
        if file_mutation_result_landed(name, &content) {
            return None;
        }
        // Non-JSON legacy error string — still a hard failure, but keep structured path primary.
        if content.starts_with("Tool error:") {
            return Some(TerminalMutationToolError {
                tool: name.to_string(),
                code: "legacy_tool_error".into(),
                code_num: 1099,
                message: content.lines().next().unwrap_or("tool error").to_string(),
            });
        }
        return None;
    }
    None
}

fn mutated_paths_for_oracles(records: Vec<MutationRecord>) -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for rec in records {
        if seen.insert(rec.path.clone()) {
            paths.push(rec.path);
        }
    }
    paths
}

fn run_post_mutation_oracles(cwd: &Path, paths: &[String]) -> Vec<OracleGateFailure> {
    let mut failures = Vec::new();
    for path in paths {
        let Some(command) = oracle_command_for_path(path) else {
            continue;
        };
        let full_path = cwd.join(path);
        if let Some(failure) = run_oracle(path, &full_path, command) {
            failures.push(failure);
        }
    }
    failures
}

fn oracle_command_for_path(path: &str) -> Option<&'static str> {
    let path = Path::new(path);
    match path.extension().and_then(|e| e.to_str()) {
        Some("js") | Some("mjs") | Some("cjs") => Some("node --check"),
        _ => None,
    }
}

fn run_oracle(display_path: &str, full_path: &PathBuf, command: &str) -> Option<OracleGateFailure> {
    if !full_path.is_file() {
        return None;
    }
    let output = Command::new("node")
        .arg("--check")
        .arg(full_path)
        .output()
        .ok()?;
    if output.status.success() {
        return None;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let preview = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("syntax check failed")
        .chars()
        .take(200)
        .collect::<String>();
    Some(OracleGateFailure {
        path: display_path.to_string(),
        command: command.to_string(),
        exit_code: output.status.code(),
        stderr_preview: preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::Message;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn terminal_error_when_last_patch_failed() {
        let ok = serde_json::json!({"ok": true, "replacements": 1}).to_string();
        let err = serde_json::json!({
            "type": "tool_error",
            "category": "arguments",
            "code": "invalid_arguments",
            "code_num": 1002,
            "error": "Invalid arguments for patch",
            "retryable": false,
            "suppress_retry": true
        })
        .to_string();
        let messages = vec![
            Message::tool_result("t1", "patch", &ok),
            Message::tool_result("t2", "patch", &err),
        ];
        let terminal = terminal_mutation_tool_error(&messages).expect("should block");
        assert_eq!(terminal.tool, "patch");
        assert_eq!(terminal.code_num, 1002);
    }

    #[test]
    fn terminal_ok_when_last_patch_succeeded_after_failure() {
        let err = serde_json::json!({
            "type": "tool_error",
            "category": "arguments",
            "code": "invalid_arguments",
            "code_num": 1002,
            "error": "Invalid arguments for patch",
            "retryable": false,
            "suppress_retry": true
        })
        .to_string();
        let ok = serde_json::json!({"ok": true, "replacements": 1}).to_string();
        let messages = vec![
            Message::tool_result("t1", "patch", &err),
            Message::tool_result("t2", "patch", &ok),
        ];
        assert!(terminal_mutation_tool_error(&messages).is_none());
    }

    #[test]
    fn oracle_catches_js_syntax_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("broken.js");
        fs::write(&path, "const x = ;\n").expect("write");
        let turn = MutationTurnState::new();
        turn.push_success(crate::mutations::MutationRecord {
            path: "broken.js".into(),
            kind: crate::mutations::MutationKind::Modify,
            lines_added: 1,
            lines_removed: 0,
        });
        let snapshot = build_harness_snapshot(HarnessBuildInput {
            messages: &[],
            mutation_turn: &turn,
            cwd: dir.path(),
            post_mutation_oracles: true,
        });
        assert_eq!(snapshot.oracle_failures.len(), 1);
        assert!(snapshot.blocks_completion());
    }

    #[test]
    fn oracle_passes_valid_js() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("ok.js"), "const x = 1;\n").expect("write");
        let turn = MutationTurnState::new();
        turn.push_success(crate::mutations::MutationRecord {
            path: "ok.js".into(),
            kind: crate::mutations::MutationKind::Modify,
            lines_added: 1,
            lines_removed: 0,
        });
        let snapshot = build_harness_snapshot(HarnessBuildInput {
            messages: &[],
            mutation_turn: &turn,
            cwd: dir.path(),
            post_mutation_oracles: true,
        });
        assert!(snapshot.oracle_failures.is_empty());
        assert!(!snapshot.blocks_completion());
    }

    #[test]
    fn prose_deferred_intent_does_not_affect_snapshot() {
        let snapshot = HarnessSnapshot::default();
        assert!(!snapshot.blocks_completion());
    }
}
