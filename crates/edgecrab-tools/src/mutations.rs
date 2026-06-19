//! Per-turn file mutation tracking and footer rendering.
//!
//! Mirrors hermes-agent's file-mutation verifier (failure advisory) and extends
//! it with a ground-truth success log (`files-mutated this turn`) for TTY,
//! gateway, and next-turn model context.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

/// Maximum successful mutation records retained per turn before collapsing.
pub const MAX_MUTATION_RECORDS: usize = 256;

/// Maximum failed paths shown in the failure advisory footer.
pub const MAX_FAILED_SHOWN: usize = 10;

/// Tools that mutate files on disk.
pub const FILE_MUTATING_TOOLS: &[&str] = &["write_file", "patch", "apply_patch"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    Add,
    Modify,
    Delete,
}

impl MutationKind {
    pub fn glyph(self) -> char {
        match self {
            Self::Add => 'A',
            Self::Modify => 'M',
            Self::Delete => 'D',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationRecord {
    pub path: String,
    pub kind: MutationKind,
    pub lines_added: u32,
    pub lines_removed: u32,
}

#[derive(Debug, Clone)]
struct FailedMutation {
    tool: String,
    error_preview: String,
}

#[derive(Debug, Default)]
struct TurnInner {
    records: Vec<MutationRecord>,
    failed: HashMap<String, FailedMutation>,
}

/// Per-conversation-turn mutation state (success log + failure tracker).
#[derive(Debug, Clone, Default)]
pub struct MutationTurnState {
    inner: Arc<Mutex<TurnInner>>,
}

impl MutationTurnState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.records.clear();
            guard.failed.clear();
        }
    }

    pub fn push_success(&self, record: MutationRecord) {
        if let Ok(mut guard) = self.inner.lock()
            && guard.records.len() < MAX_MUTATION_RECORDS
        {
            guard.records.push(record);
        }
    }

    pub fn drain_success(&self) -> Vec<MutationRecord> {
        if let Ok(mut guard) = self.inner.lock() {
            std::mem::take(&mut guard.records)
        } else {
            Vec::new()
        }
    }

    /// Peek successful mutation records without draining (harness gate input).
    pub fn peek_success_records(&self) -> Vec<MutationRecord> {
        if let Ok(guard) = self.inner.lock() {
            guard.records.clone()
        } else {
            Vec::new()
        }
    }

    /// Peek unresolved per-path mutation failures without clearing state.
    pub fn peek_failed(&self) -> HashMap<String, (String, String)> {
        if let Ok(guard) = self.inner.lock() {
            guard
                .failed
                .iter()
                .map(|(path, info)| {
                    (
                        path.clone(),
                        (info.tool.clone(), info.error_preview.clone()),
                    )
                })
                .collect()
        } else {
            HashMap::new()
        }
    }

    pub fn record_tool_outcome(&self, tool_name: &str, args: &Value, result: &str, is_error: bool) {
        if !FILE_MUTATING_TOOLS.contains(&tool_name) {
            return;
        }
        let targets = extract_file_mutation_targets(tool_name, args);
        if targets.is_empty() {
            return;
        }
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        let landed = file_mutation_result_landed(tool_name, result);
        if is_error && !landed {
            let preview = extract_error_preview(result, 180);
            for path in targets {
                guard.failed.entry(path).or_insert(FailedMutation {
                    tool: tool_name.to_string(),
                    error_preview: preview.clone(),
                });
            }
        } else {
            for path in targets {
                guard.failed.remove(&path);
            }
        }
    }

    pub fn take_failed(&self) -> HashMap<String, (String, String)> {
        if let Ok(mut guard) = self.inner.lock() {
            guard
                .failed
                .drain()
                .map(|(path, info)| (path, (info.tool, info.error_preview)))
                .collect()
        } else {
            HashMap::new()
        }
    }

    pub fn render_turn_footer(&self) -> String {
        let records = self.drain_success();
        let failed = self.take_failed();
        let mut parts = Vec::new();
        if let Some(success) = render_success_footer(&records) {
            parts.push(success);
        }
        if let Some(failure) = render_failure_footer(&failed) {
            parts.push(failure);
        }
        parts.join("\n\n")
    }
}

/// Return true when a tool result proves the write landed on disk.
pub fn file_mutation_result_landed(tool_name: &str, result: &str) -> bool {
    if !FILE_MUTATING_TOOLS.contains(&tool_name) {
        return false;
    }
    let Ok(data) = serde_json::from_str::<Value>(result.trim()) else {
        return false;
    };
    if !data.is_object() || data.get("error").is_some() {
        return false;
    }
    match tool_name {
        "write_file" => data.get("bytes").is_some() || data.get("ok") == Some(&Value::Bool(true)),
        "patch" | "apply_patch" => data.get("ok") == Some(&Value::Bool(true)),
        _ => false,
    }
}

/// Extract target paths from write_file / patch / apply_patch arguments.
pub fn extract_file_mutation_targets(tool_name: &str, args: &Value) -> Vec<String> {
    match tool_name {
        "write_file" => args
            .get("path")
            .and_then(Value::as_str)
            .map(|p| vec![p.to_string()])
            .unwrap_or_default(),
        "patch" => {
            let mode = args
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("replace");
            if mode == "replace" {
                return args
                    .get("path")
                    .and_then(Value::as_str)
                    .map(|p| vec![p.to_string()])
                    .unwrap_or_default();
            }
            if mode == "patch" {
                return parse_v4a_patch_paths(
                    args.get("patch").and_then(Value::as_str).unwrap_or(""),
                );
            }
            Vec::new()
        }
        "apply_patch" => {
            parse_v4a_patch_paths(args.get("patch").and_then(Value::as_str).unwrap_or(""))
        }
        _ => Vec::new(),
    }
}

fn parse_v4a_patch_paths(body: &str) -> Vec<String> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut paths = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("*** Update File:")
            .or_else(|| trimmed.strip_prefix("*** Add File:"))
            .or_else(|| trimmed.strip_prefix("*** Delete File:"));
        if let Some(path) = rest {
            let path = path.trim();
            if !path.is_empty() {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

pub fn extract_error_preview(result: &str, max_len: usize) -> String {
    let mut text = result.trim().to_string();
    if let Ok(data) = serde_json::from_str::<Value>(&text)
        && let Some(err) = data.get("error").and_then(Value::as_str)
    {
        text = err.to_string();
    }
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() <= max_len {
        collapsed
    } else {
        let boundary = collapsed
            .char_indices()
            .take_while(|(i, _)| *i < max_len.saturating_sub(1))
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}…", &collapsed[..boundary])
    }
}

pub fn render_success_footer(records: &[MutationRecord]) -> Option<String> {
    if records.is_empty() {
        return None;
    }
    let mut lines = vec!["─── files-mutated this turn ───────────────────────".to_string()];
    let show = records.len().min(MAX_MUTATION_RECORDS);
    for rec in records.iter().take(show) {
        lines.push(format_mutation_line(rec));
    }
    let extra = records.len().saturating_sub(show);
    if extra > 0 {
        lines.push(format!("  … + {extra} more"));
    }
    lines.push("───────────────────────────────────────────────────".to_string());
    Some(lines.join("\n"))
}

pub fn render_failure_footer(failed: &HashMap<String, (String, String)>) -> Option<String> {
    if failed.is_empty() {
        return None;
    }
    let mut lines = vec![format!(
        "⚠️ File-mutation verifier: {} file(s) were NOT modified this turn despite any \
         wording above that may suggest otherwise. Run `git status` or `read_file` to confirm.",
        failed.len()
    )];
    let mut shown = 0usize;
    for (path, (tool, preview)) in failed {
        if shown >= MAX_FAILED_SHOWN {
            break;
        }
        if preview.is_empty() {
            lines.push(format!("  • {path} — [{tool}] failed"));
        } else {
            lines.push(format!("  • {path} — [{tool}] {preview}"));
        }
        shown += 1;
    }
    let remaining = failed.len().saturating_sub(shown);
    if remaining > 0 {
        lines.push(format!("  • … and {remaining} more"));
    }
    Some(lines.join("\n"))
}

fn format_mutation_line(rec: &MutationRecord) -> String {
    format!(
        "{}  {:<40} +{} −{}",
        rec.kind.glyph(),
        truncate_path(&rec.path, 40),
        rec.lines_added,
        rec.lines_removed
    )
}

fn truncate_path(path: &str, max_cols: usize) -> String {
    if path.chars().count() <= max_cols {
        return path.to_string();
    }
    let tail: String = path
        .chars()
        .rev()
        .take(max_cols.saturating_sub(1))
        .collect();
    let tail: String = tail.chars().rev().collect();
    format!("…{tail}")
}

/// Render success footer with optional terminal width (compact / Termux).
pub fn render_success_footer_width(
    records: &[MutationRecord],
    max_cols: Option<usize>,
) -> Option<String> {
    let base = render_success_footer(records)?;
    let Some(cols) = max_cols else {
        return Some(base);
    };
    if cols >= 60 {
        return Some(base);
    }
    let mut lines = vec!["─── files-mutated ───".to_string()];
    let show = records.len().min(8);
    for rec in records.iter().take(show) {
        let path = truncate_path(&rec.path, cols.saturating_sub(12));
        lines.push(format!(
            "{} {} +{} −{}",
            rec.kind.glyph(),
            path,
            rec.lines_added,
            rec.lines_removed
        ));
    }
    let extra = records.len().saturating_sub(show);
    if extra > 0 {
        lines.push(format!("… +{extra} more"));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_success_footer_is_none() {
        assert!(render_success_footer(&[]).is_none());
    }

    #[test]
    fn single_add_record() {
        let footer = render_success_footer(&[MutationRecord {
            path: "path/to/new.rs".into(),
            kind: MutationKind::Add,
            lines_added: 12,
            lines_removed: 0,
        }])
        .expect("footer");
        assert!(footer.contains("files-mutated"));
        assert!(footer.contains("A"));
        assert!(footer.contains("+12"));
        assert!(footer.contains("new.rs"));
    }

    #[test]
    fn mixed_kinds_and_overflow_collapse() {
        let mut records = Vec::new();
        for i in 0..260 {
            records.push(MutationRecord {
                path: format!("file{i}.rs"),
                kind: MutationKind::Modify,
                lines_added: 1,
                lines_removed: 0,
            });
        }
        let footer = render_success_footer(&records).expect("footer");
        assert!(footer.contains("+ 4 more") || footer.contains("+ 260 more"));
    }

    #[test]
    fn extract_write_file_path() {
        let args = json!({"path": "/tmp/a.md", "content": "x"});
        assert_eq!(
            extract_file_mutation_targets("write_file", &args),
            vec!["/tmp/a.md"]
        );
    }

    #[test]
    fn extract_patch_v4a_paths() {
        let body =
            "*** Begin Patch\n*** Update File: /tmp/a.md\n*** Add File: /tmp/b.md\n*** End Patch\n";
        let args = json!({"mode": "patch", "patch": body});
        assert_eq!(
            extract_file_mutation_targets("patch", &args),
            vec!["/tmp/a.md", "/tmp/b.md"]
        );
    }

    #[test]
    fn failure_footer_lists_paths() {
        let mut failed = HashMap::new();
        failed.insert(
            "/tmp/a.md".into(),
            ("patch".into(), "Could not find old_string".into()),
        );
        let footer = render_failure_footer(&failed).expect("footer");
        assert!(footer.contains("NOT modified"));
        assert!(footer.contains("/tmp/a.md"));
    }

    #[test]
    fn record_tool_outcome_success_clears_failure() {
        let state = MutationTurnState::new();
        let args = json!({"path": "/tmp/a.md", "old_string": "x", "new_string": "y"});
        state.record_tool_outcome("patch", &args, r#"{"error":"not found"}"#, true);
        assert_eq!(state.take_failed().len(), 1);
        state.record_tool_outcome(
            "patch",
            &args,
            r#"{"ok":true,"before_lines":1,"after_lines":2}"#,
            false,
        );
        assert!(state.take_failed().is_empty());
    }

    #[test]
    fn landed_detects_write_and_patch() {
        assert!(file_mutation_result_landed(
            "write_file",
            r#"{"ok":true,"bytes":10,"lines":2}"#
        ));
        assert!(file_mutation_result_landed(
            "patch",
            r#"{"ok":true,"before_lines":1,"after_lines":2}"#
        ));
        assert!(!file_mutation_result_landed("patch", r#"{"error":"x"}"#));
    }

    #[test]
    fn compact_width_footer() {
        let records = vec![MutationRecord {
            path: "crates/edgecrab-core/src/agent.rs".into(),
            kind: MutationKind::Modify,
            lines_added: 3,
            lines_removed: 1,
        }];
        let footer = render_success_footer_width(&records, Some(40)).expect("footer");
        assert!(footer.contains("files-mutated"));
        assert!(!footer.contains("────────────────"));
    }
}
