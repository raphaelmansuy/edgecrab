//! # patch, apply_patch — File editing tools
//!
//! ## patch
//! Apply a targeted edit to a file by replacing an exact string match.
//! More precise than write_file for small, localized changes.
//!
//! ## apply_patch
//! Apply a V4A multi-operation patch — mirrors hermes-agent's patch_parser.py.
//!
//! V4A format:
//! ```text
//! *** Begin Patch
//! *** Update File: path/to/file.py
//! @@ context hint @@
//!  context line (space prefix)
//! -removed line
//! +added line
//! *** Add File: path/to/new.py
//! +new file content
//! *** Delete File: path/to/old.py
//! *** Move File: old/path.py -> new/path.py
//! *** End Patch
//! ```

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Format apply_patch errors as a single visible line.
///
/// WHY (FP18): TUI activity feed shows only the first line of a tool result.
/// Multi-line errors with `\n` mean the user and model see `"Errors:"` but
/// no error details — the body is below the fold. Semicolon-joined errors fit
/// on one visible line in both TUI summary and LLM tool-result context.
fn format_patch_errors(errors: &[String]) -> String {
    if errors.is_empty() {
        "apply_patch failed and was rolled back (no error detail)".into()
    } else if errors.len() == 1 {
        format!("apply_patch failed and was rolled back: {}", errors[0])
    } else {
        format!(
            "apply_patch failed and was rolled back ({} errors): {}",
            errors.len(),
            errors.join("; ")
        )
    }
}

use edgecrab_types::{ToolError, ToolSchema};

use crate::edit_contract::{
    DEFAULT_MAX_MUTATION_PAYLOAD_BYTES, DEFAULT_MAX_MUTATION_PAYLOAD_KIB,
    enforce_apply_patch_payload_limit_with_max, enforce_patch_payload_limit_with_max,
};
use crate::fuzzy_match::fuzzy_find_and_replace;
use crate::path_utils::jail_read_path;
use crate::registry::{ToolContext, ToolHandler};
use crate::tools::checkpoint::ensure_checkpoint;

pub struct PatchTool;

#[derive(Deserialize)]
struct ReplaceArgs {
    path: String,
    /// String to find (8-strategy fuzzy matching is applied if exact fails)
    old_string: String,
    /// Replacement string
    new_string: String,
    /// Replace all occurrences (default: false — require unique match)
    #[serde(default)]
    replace_all: bool,
}

#[derive(Deserialize)]
struct PatchArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    old_string: Option<String>,
    #[serde(default)]
    new_string: Option<String>,
    #[serde(default)]
    replace_all: bool,
    #[serde(default)]
    patch: Option<String>,
}

fn should_use_v4a_mode(args: &PatchArgs) -> bool {
    match args.mode.as_deref() {
        Some("patch") => true,
        Some("replace") => false,
        Some(_) => false,
        None => args.patch.is_some() && args.old_string.is_none() && args.new_string.is_none(),
    }
}

async fn execute_replace_patch(args: ReplaceArgs, ctx: &ToolContext) -> Result<String, ToolError> {
    enforce_patch_payload_limit_with_max(
        "patch",
        &args.path,
        args.old_string.len() + args.new_string.len(),
        ctx.config.max_write_payload_bytes(),
    )?;

    // Auto-checkpoint before mutation
    ensure_checkpoint(ctx, &format!("before patch: {}", args.path));

    let path_policy = ctx.config.file_path_policy(&ctx.cwd);
    let resolved = jail_read_path(&args.path, &path_policy)?;

    crate::read_tracker::guard_file_freshness(&ctx.session_id, "patch", &args.path, &resolved)?;

    let content = tokio::fs::read_to_string(&resolved)
        .await
        .map_err(|e| ToolError::Other(format!("Cannot read '{}': {}", args.path, e)))?;

    // R17: Capture file metadata immediately after reading so we can detect
    // any external modification in the window between read and write (TOCTOU).
    let pre_write_snap = tokio::fs::metadata(&resolved)
        .await
        .ok()
        .map(|m| (m.len(), m.modified().ok()));

    // 8-strategy fuzzy replacement (mirrors hermes fuzzy_match.py)
    let (new_content, count) = match fuzzy_find_and_replace(
        &content,
        &args.old_string,
        &args.new_string,
        args.replace_all,
    ) {
        Ok(result) => result,
        Err(msg) => {
            // R15: old_string not found — record current snapshot so the model
            // can retry with a corrected old_string without needing read_file.
            let _ = crate::read_tracker::record_file_snapshot(&ctx.session_id, &resolved);

            // Embed a 600-char preview of the actual file content so the model
            // can see what to match against when composing the retry.
            const PREVIEW_LIMIT: usize = 600;
            let preview = crate::safe_truncate(&content, PREVIEW_LIMIT);
            let truncated = preview.len() < content.len();
            let trunc_note = if truncated {
                format!(
                    "\n[...truncated — file has {} total bytes; \
                     read_file gives full content if needed.]",
                    content.len()
                )
            } else {
                String::new()
            };

            return Err(ToolError::ContentMismatch {
                tool: "patch".into(),
                path: args.path.clone(),
                message: format!(
                    "{msg}\n\
                     Snapshot recorded — retry patch with the corrected old_string \
                     (no read_file needed).\n\
                     \n\
                     Current file content (preview):\n\
                     ---\n\
                     {preview}{trunc_note}\n\
                     ---"
                ),
            });
        }
    };

    // R17: Atomic TOCTOU re-check — confirm the file has not been modified by
    // another process in the window between our read and the upcoming write.
    if let Some((size_at_read, mtime_at_read)) = pre_write_snap {
        if let Ok(current) = tokio::fs::metadata(&resolved).await {
            let changed = current.len() != size_at_read
                || current.modified().ok() != mtime_at_read;
            if changed {
                return Err(ToolError::ContentMismatch {
                    tool: "patch".into(),
                    path: args.path.clone(),
                    message: format!(
                        "'{}' was modified by another process between read and write (TOCTOU). \
                         Re-read the file with read_file and retry the patch.",
                        args.path
                    ),
                });
            }
        }
    }

    let before_bytes = content.len();
    let after_bytes = new_content.len();

    tokio::fs::write(&resolved, &new_content)
        .await
        .map_err(|e| ToolError::Other(format!("Cannot write '{}': {}", args.path, e)))?;

    let _ = crate::read_tracker::record_file_snapshot(&ctx.session_id, &resolved);

    // R18: Structured JSON result (FP57) — enables the display layer and the
    // conversation loop to branch on machine-readable fields instead of parsing prose.
    let result = serde_json::json!({
        "ok": true,
        "replacements": count,
        "before_bytes": before_bytes,
        "after_bytes": after_bytes,
        "path": args.path,
    });
    Ok(result.to_string())
}

#[async_trait]
impl ToolHandler for PatchTool {
    fn name(&self) -> &'static str {
        "patch"
    }

    fn toolset(&self) -> &'static str {
        "file"
    }

    fn emoji(&self) -> &'static str {
        "🩹"
    }

    fn path_arguments(&self) -> &'static [&'static str] {
        &["path"]
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "patch".into(),
            description: format!(
                "Patch files in two modes. Replace mode: targeted find-and-replace \
                 using path + old_string + new_string with 8-strategy fuzzy matching \
                 (exact → line-trimmed → whitespace-norm → indent-flexible → \
                 escape-norm → trimmed-boundary → block-anchor → context-aware). \
                 Patch mode: pass a V4A multi-file patch block in the patch field \
                 (hermes-compatible). Replace-mode hard limit: combined old_string + \
                 new_string payload must stay at or under {DEFAULT_MAX_MUTATION_PAYLOAD_BYTES} \
                 bytes ({DEFAULT_MAX_MUTATION_PAYLOAD_KIB} KiB) per call."
            ),
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["replace", "patch"],
                        "description": "replace = targeted find-and-replace, patch = V4A multi-file patch block"
                    },
                    "path": {
                        "type": ["string", "null"],
                        "description": "File path relative to working directory. Required for replace mode; use null for patch mode."
                    },
                    "old_string": {
                        "type": ["string", "null"],
                        "description": "String to find and replace. Required for replace mode; use null for patch mode."
                    },
                    "new_string": {
                        "type": ["string", "null"],
                        "description": "String to replace old_string with. Required for replace mode; use empty string to delete text and null for patch mode."
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace all occurrences. Set explicitly to true or false."
                    },
                    "patch": {
                        "type": ["string", "null"],
                        "description": "V4A patch block for patch mode, starting with '*** Begin Patch'. Use null for replace mode."
                    }
                },
                "required": ["mode", "path", "old_string", "new_string", "replace_all", "patch"]
            }),
            strict: Some(true),
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: PatchArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "patch".into(),
            message: e.to_string(),
        })?;

        if should_use_v4a_mode(&args) {
            let patch_text = args
                .patch
                .as_deref()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "patch".into(),
                    message: "patch mode requires the 'patch' field with a V4A patch block".into(),
                })?;
            return execute_v4a_patch(patch_text, ctx).await;
        }

        let replace_args = ReplaceArgs {
            path: args.path.ok_or_else(|| ToolError::InvalidArgs {
                tool: "patch".into(),
                message: "replace mode requires 'path'. If you intended a V4A patch block, call apply_patch or patch(mode='patch', patch=...)".into(),
            })?,
            old_string: args.old_string.ok_or_else(|| ToolError::InvalidArgs {
                tool: "patch".into(),
                message: "replace mode requires 'old_string'. If you intended a V4A patch block, call apply_patch or patch(mode='patch', patch=...)".into(),
            })?,
            new_string: args.new_string.ok_or_else(|| ToolError::InvalidArgs {
                tool: "patch".into(),
                message: "replace mode requires 'new_string'. Use empty string to delete text. If you intended a V4A patch block, call apply_patch or patch(mode='patch', patch=...)".into(),
            })?,
            replace_all: args.replace_all,
        };

        execute_replace_patch(replace_args, ctx).await
    }
}

inventory::submit!(&PatchTool as &dyn ToolHandler);

// ─── V4A multi-operation patch ────────────────────────────────────────────

/// Operation type decoded from a `*** Foo File:` header.
#[derive(Debug, PartialEq, Clone)]
enum V4AOpKind {
    Update,
    Add,
    Delete,
    Move { new_path: String },
}

/// A single line in a V4A hunk: context (' '), removed ('-'), or added ('+').
#[derive(Debug, Clone)]
struct HunkLine {
    prefix: char, // ' ', '-', '+'
    content: String,
}

/// A hunk within an Update operation.
#[derive(Debug, Default, Clone)]
struct Hunk {
    context_hint: Option<String>,
    lines: Vec<HunkLine>,
}

/// A parsed V4A operation.
#[derive(Debug, Clone)]
struct V4AOp {
    kind: V4AOpKind,
    file_path: String,
    hunks: Vec<Hunk>,
}

/// Parse a V4A patch block into a list of operations.
///
/// Returns `(ops, None)` on success, `([], Some(error))` on fatal parse error.
fn parse_v4a(patch: &str) -> (Vec<V4AOp>, Option<String>) {
    let lines: Vec<&str> = patch.lines().collect();
    let mut ops: Vec<V4AOp> = Vec::new();

    // Find begin/end markers (flexible: allow missing Begin marker)
    let start = lines
        .iter()
        .position(|l| l.contains("Begin Patch"))
        .map(|i| i + 1)
        .unwrap_or(0);
    let end = lines
        .iter()
        .position(|l| l.contains("End Patch"))
        .unwrap_or(lines.len());

    let patch_lines = &lines[start..end];

    let mut current_op: Option<V4AOp> = None;
    let mut current_hunk: Option<Hunk> = None;

    for raw in patch_lines {
        let line = *raw;

        // ── File operation markers ───────────────────────────────────
        if let Some(rest) = strip_op_prefix(line, "Update File:") {
            flush_op(&mut current_op, &mut current_hunk, &mut ops);
            current_op = Some(V4AOp {
                kind: V4AOpKind::Update,
                file_path: rest.trim().to_string(),
                hunks: Vec::new(),
            });
            current_hunk = None;
            continue;
        }
        if let Some(rest) = strip_op_prefix(line, "Add File:") {
            flush_op(&mut current_op, &mut current_hunk, &mut ops);
            current_op = Some(V4AOp {
                kind: V4AOpKind::Add,
                file_path: rest.trim().to_string(),
                hunks: Vec::new(),
            });
            current_hunk = Some(Hunk::default());
            continue;
        }
        if let Some(rest) = strip_op_prefix(line, "Delete File:") {
            flush_op(&mut current_op, &mut current_hunk, &mut ops);
            ops.push(V4AOp {
                kind: V4AOpKind::Delete,
                file_path: rest.trim().to_string(),
                hunks: Vec::new(),
            });
            current_op = None;
            current_hunk = None;
            continue;
        }
        if let Some(rest) = strip_op_prefix(line, "Move File:") {
            flush_op(&mut current_op, &mut current_hunk, &mut ops);
            if let Some((old, new)) = rest.split_once("->") {
                ops.push(V4AOp {
                    kind: V4AOpKind::Move {
                        new_path: new.trim().to_string(),
                    },
                    file_path: old.trim().to_string(),
                    hunks: Vec::new(),
                });
            }
            current_op = None;
            current_hunk = None;
            continue;
        }

        // ── Hunk header (@@ ... @@) ──────────────────────────────────
        if line.starts_with("@@") {
            if let Some(ref mut op) = current_op {
                if let Some(h) = current_hunk.take() {
                    if !h.lines.is_empty() {
                        op.hunks.push(h);
                    }
                }
                let hint = line
                    .trim_start_matches('@')
                    .trim_end_matches('@')
                    .trim_start_matches('@') // handle @@...@@
                    .trim()
                    .to_string();
                current_hunk = Some(Hunk {
                    context_hint: if hint.is_empty() { None } else { Some(hint) },
                    lines: Vec::new(),
                });
            }
            continue;
        }

        // ── Hunk lines ───────────────────────────────────────────────
        if current_op.is_some() && !line.is_empty() {
            let hunk = current_hunk.get_or_insert_with(Hunk::default);
            let (prefix, content) = if let Some(c) = line.strip_prefix('+') {
                ('+', c.to_string())
            } else if let Some(c) = line.strip_prefix('-') {
                ('-', c.to_string())
            } else if let Some(c) = line.strip_prefix(' ') {
                (' ', c.to_string())
            } else if line.starts_with('\\') {
                // "\\ No newline at end of file" — skip
                continue;
            } else {
                // Implicit context line
                (' ', line.to_string())
            };
            hunk.lines.push(HunkLine { prefix, content });
        }
    }

    // Flush out any trailing op
    flush_op(&mut current_op, &mut current_hunk, &mut ops);

    (ops, None)
}

/// Flush the current in-progress operation into the ops list.
fn flush_op(current_op: &mut Option<V4AOp>, current_hunk: &mut Option<Hunk>, ops: &mut Vec<V4AOp>) {
    if let Some(mut op) = current_op.take() {
        if let Some(h) = current_hunk.take() {
            if !h.lines.is_empty() {
                op.hunks.push(h);
            }
        }
        ops.push(op);
    }
}

/// Strip a `*** <marker>` prefix from a line (case-insensitive for the keyword).
fn strip_op_prefix<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let trimmed = line.trim_start_matches('*').trim();
    trimmed.strip_prefix(marker)
}

/// Apply a single V4A Update hunk to `content`, returning the new content.
///
/// Strategy: build a search string from (context + removed) lines and a
/// replacement from (context + added) lines, then do exact-string replacement.
/// If the exact match fails, try a whitespace-normalized fuzzy match.
fn apply_update_hunk(content: &str, hunk: &Hunk) -> Result<String, String> {
    let mut search_lines: Vec<&str> = Vec::new();
    let mut replace_lines: Vec<&str> = Vec::new();

    for hl in &hunk.lines {
        match hl.prefix {
            ' ' => {
                search_lines.push(&hl.content);
                replace_lines.push(&hl.content);
            }
            '-' => {
                search_lines.push(&hl.content);
            }
            '+' => {
                replace_lines.push(&hl.content);
            }
            _ => {}
        }
    }

    if search_lines.is_empty() {
        // Pure-addition hunk with no context.  Use context hint to locate
        // insertion point (after the line containing the hint), or append.
        let insert_text = replace_lines.join("\n");
        if let Some(ref hint) = hunk.context_hint {
            if let Some(pos) = content.find(hint.as_str()) {
                let eol = content[pos..]
                    .find('\n')
                    .map(|o| pos + o + 1)
                    .unwrap_or(content.len());
                return Ok(format!(
                    "{}{}\n{}",
                    &content[..eol],
                    insert_text,
                    &content[eol..]
                ));
            }
        }
        return Ok(format!("{content}\n{insert_text}"));
    }

    let search = search_lines.join("\n");
    let replacement = replace_lines.join("\n");

    // 1. Exact match
    let count = content.matches(search.as_str()).count();
    if count == 1 {
        return Ok(content.replacen(&search, &replacement, 1));
    }
    if count > 1 {
        // Multiple matches: use context hint to choose the closest occurrence.
        if let Some(hint) = hunk.context_hint.as_deref() {
            if let Some(hint_pos) = content.find(hint) {
                let occurrences: Vec<usize> = content
                    .match_indices(search.as_str())
                    .map(|(idx, _)| idx)
                    .collect();
                if !occurrences.is_empty() {
                    let chosen = occurrences
                        .iter()
                        .copied()
                        .find(|idx| *idx >= hint_pos)
                        .or_else(|| occurrences.last().copied())
                        .expect("occurrence exists");
                    let before = &content[..chosen];
                    let after = &content[chosen + search.len()..];
                    return Ok(format!("{before}{replacement}{after}"));
                }
            }
        }
        return Err(format!(
            "Hunk search pattern matched {} times — add more context lines to make it unique",
            count
        ));
    }

    // 2. Whitespace-trimmed fuzzy match (handles trailing-space differences)
    let norm_search: Vec<&str> = search_lines.iter().map(|l| l.trim_end()).collect();
    let norm_content_lines: Vec<&str> = content.lines().collect();

    for start_idx in 0..=norm_content_lines.len().saturating_sub(norm_search.len()) {
        let window: Vec<&str> = norm_content_lines[start_idx..start_idx + norm_search.len()]
            .iter()
            .map(|l| l.trim_end())
            .collect();
        if window == norm_search {
            // Found a fuzzy match — reconstruct content with replacement
            let before = norm_content_lines[..start_idx].join("\n");
            let after = norm_content_lines[start_idx + norm_search.len()..].join("\n");
            let norm_replace: Vec<&str> = replace_lines.iter().map(|l| l.trim_end()).collect();
            let middle = norm_replace.join("\n");
            let sep_before = if before.is_empty() { "" } else { "\n" };
            let sep_after = if after.is_empty() { "" } else { "\n" };
            return Ok(format!("{before}{sep_before}{middle}{sep_after}{after}"));
        }
    }

    Err(format!(
        "Hunk search pattern not found in file. Pattern:\n{}",
        search_lines.join("\n")
    ))
}

pub struct ApplyPatchTool;

/// Resolved operation with jailed absolute paths.
#[derive(Debug, Clone)]
struct PreparedOp {
    op: V4AOp,
    source: PathBuf,
    target: Option<PathBuf>,
}

/// Restore backed-up filesystem state for transactional rollback.
async fn restore_backups(backups: &BTreeMap<PathBuf, Option<Vec<u8>>>) {
    for (path, original) in backups {
        match original {
            Some(bytes) => {
                if let Some(parent) = path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::write(path, bytes).await;
            }
            None => {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
    }
}

async fn execute_v4a_patch(patch_text: &str, ctx: &ToolContext) -> Result<String, ToolError> {
    enforce_apply_patch_payload_limit_with_max(patch_text, ctx.config.max_write_payload_bytes())?;

    let (ops, parse_err) = parse_v4a(patch_text);
    if let Some(e) = parse_err {
        return Err(ToolError::InvalidArgs {
            tool: "apply_patch".into(),
            message: e,
        });
    }
    if ops.is_empty() {
        return Err(ToolError::InvalidArgs {
            tool: "apply_patch".into(),
            message: "No V4A operations found. Ensure the patch starts with '*** Begin Patch'."
                .into(),
        });
    }

    // Auto-checkpoint before any mutations
    ensure_checkpoint(ctx, "before apply_patch");

    // Validate + resolve all paths upfront (fail-fast before touching any file)
    let path_policy = ctx.config.file_path_policy(&ctx.cwd);
    let mut prepared: Vec<PreparedOp> = Vec::new();
    for op in &ops {
        match &op.kind {
            V4AOpKind::Update | V4AOpKind::Delete => {
                let source = jail_read_path(&op.file_path, &path_policy)
                    .map_err(|e| ToolError::Other(e.to_string()))?;
                prepared.push(PreparedOp {
                    op: op.clone(),
                    source,
                    target: None,
                });
            }
            V4AOpKind::Add => {
                let source = crate::path_utils::jail_write_path(&op.file_path, &path_policy)
                    .map_err(|e| ToolError::Other(e.to_string()))?;
                if source.exists() {
                    return Err(ToolError::Other(format!(
                        "Add File '{}' failed: destination already exists",
                        op.file_path
                    )));
                }
                prepared.push(PreparedOp {
                    op: op.clone(),
                    source,
                    target: None,
                });
            }
            V4AOpKind::Move { new_path } => {
                let source = jail_read_path(&op.file_path, &path_policy)
                    .map_err(|e| ToolError::Other(e.to_string()))?;
                let target = crate::path_utils::jail_write_path(new_path, &path_policy)
                    .map_err(|e| ToolError::Other(e.to_string()))?;
                if target.exists() {
                    return Err(ToolError::Other(format!(
                        "Move File '{}' -> '{}' failed: destination already exists",
                        op.file_path, new_path
                    )));
                }
                prepared.push(PreparedOp {
                    op: op.clone(),
                    source,
                    target: Some(target),
                });
            }
        }
    }

    for prepared_op in &prepared {
        match &prepared_op.op.kind {
            V4AOpKind::Update | V4AOpKind::Delete | V4AOpKind::Move { .. } => {
                crate::read_tracker::guard_file_freshness(
                    &ctx.session_id,
                    "apply_patch",
                    &prepared_op.op.file_path,
                    &prepared_op.source,
                )?;
            }
            V4AOpKind::Add => {}
        }
    }

    // Snapshot original bytes for every touched path so failures can rollback.
    let mut backups: BTreeMap<PathBuf, Option<Vec<u8>>> = BTreeMap::new();
    for p in &prepared {
        backups
            .entry(p.source.clone())
            .or_insert_with(|| std::fs::read(&p.source).ok());
        if let Some(target) = &p.target {
            backups
                .entry(target.clone())
                .or_insert_with(|| std::fs::read(target).ok());
        }
    }

    let mut files_modified: Vec<String> = Vec::new();
    let mut files_created: Vec<String> = Vec::new();
    let mut files_deleted: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for p in &prepared {
        let op = &p.op;
        let resolved = &p.source;

        match &op.kind {
            V4AOpKind::Update => {
                let content = match tokio::fs::read_to_string(resolved).await {
                    Ok(c) => c,
                    Err(e) => {
                        errors.push(format!("Cannot read '{}': {}", op.file_path, e));
                        break;
                    }
                };
                let mut new_content = content;
                let mut hunk_ok = true;
                for hunk in &op.hunks {
                    match apply_update_hunk(&new_content, hunk) {
                        Ok(updated) => new_content = updated,
                        Err(e) => {
                            errors.push(format!("Update '{}': {}", op.file_path, e));
                            hunk_ok = false;
                            break;
                        }
                    }
                }
                if hunk_ok {
                    if let Err(e) = tokio::fs::write(resolved, &new_content).await {
                        errors.push(format!("Cannot write '{}': {}", op.file_path, e));
                        break;
                    } else {
                        files_modified.push(op.file_path.clone());
                    }
                } else {
                    break;
                }
            }

            V4AOpKind::Add => {
                let content_lines: Vec<String> = op
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.prefix == '+')
                    .map(|l| l.content.clone())
                    .collect();
                let content = content_lines.join("\n");

                if let Some(parent) = resolved.parent() {
                    if let Err(e) = tokio::fs::create_dir_all(parent).await {
                        errors.push(format!("Cannot create dirs for '{}': {}", op.file_path, e));
                        break;
                    }
                }
                if let Err(e) = tokio::fs::write(resolved, &content).await {
                    errors.push(format!("Cannot write '{}': {}", op.file_path, e));
                    break;
                } else {
                    files_created.push(op.file_path.clone());
                }
            }

            V4AOpKind::Delete => match tokio::fs::remove_file(resolved).await {
                Ok(()) => files_deleted.push(op.file_path.clone()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    files_deleted.push(op.file_path.clone());
                }
                Err(e) => {
                    errors.push(format!("Cannot delete '{}': {}", op.file_path, e));
                    break;
                }
            },

            V4AOpKind::Move { new_path } => {
                let new_resolved = p.target.as_ref().expect("move target prepared");
                if let Some(parent) = new_resolved.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                if let Err(e) = tokio::fs::rename(resolved, new_resolved).await {
                    errors.push(format!(
                        "Cannot move '{}' → '{}': {}",
                        op.file_path, new_path, e
                    ));
                    break;
                } else {
                    files_modified.push(format!("{} → {}", op.file_path, new_path));
                }
            }
        }
    }

    let mut summary_parts: Vec<String> = Vec::new();
    if !files_modified.is_empty() {
        summary_parts.push(format!("Modified: {}", files_modified.join(", ")));
    }
    if !files_created.is_empty() {
        summary_parts.push(format!("Created: {}", files_created.join(", ")));
    }
    if !files_deleted.is_empty() {
        summary_parts.push(format!("Deleted: {}", files_deleted.join(", ")));
    }

    if errors.is_empty() {
        for prepared_op in &prepared {
            match &prepared_op.op.kind {
                V4AOpKind::Update | V4AOpKind::Add => {
                    let _ = crate::read_tracker::record_file_snapshot(
                        &ctx.session_id,
                        &prepared_op.source,
                    );
                }
                V4AOpKind::Delete => {
                    crate::read_tracker::clear_file_snapshot(&ctx.session_id, &prepared_op.source);
                }
                V4AOpKind::Move { .. } => {
                    crate::read_tracker::clear_file_snapshot(&ctx.session_id, &prepared_op.source);
                    if let Some(target) = &prepared_op.target {
                        let _ = crate::read_tracker::record_file_snapshot(&ctx.session_id, target);
                    }
                }
            }
        }
        // R18: Structured JSON result (FP57).
        let result = serde_json::json!({
            "ok": true,
            "modified": files_modified,
            "created": files_created,
            "deleted": files_deleted,
        });
        Ok(result.to_string())
    } else {
        restore_backups(&backups).await;
        Err(ToolError::Other(format_patch_errors(&errors)))
    }
}

#[async_trait]
impl ToolHandler for ApplyPatchTool {
    fn name(&self) -> &'static str {
        "apply_patch"
    }

    fn toolset(&self) -> &'static str {
        "file"
    }

    fn emoji(&self) -> &'static str {
        "📋"
    }

    fn path_arguments(&self) -> &'static [&'static str] {
        &["path"]
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "apply_patch".into(),
            description: format!(
                "\
Apply a V4A multi-file patch atomically. Supports Update, Add, Delete, and Move operations \
on multiple files in a single call. Use this for complex refactors spanning many files. \
Hard patch limit: {DEFAULT_MAX_MUTATION_PAYLOAD_BYTES} bytes ({DEFAULT_MAX_MUTATION_PAYLOAD_KIB} KiB) per call. \
Split larger refactors into multiple focused apply_patch calls.\n\n\
Format:\n\
```\n\
*** Begin Patch\n\
*** Update File: path/to/file.py\n\
@@ optional context hint @@\n\
 context line (space prefix)\n\
-removed line\n\
+added line\n\
*** Add File: path/to/new.py\n\
+new file content\n\
*** Delete File: path/to/old.py\n\
*** Move File: old/path.py -> new/path.py\n\
*** End Patch\n\
```"
            ),
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "patch": {
                        "type": "string",
                        "description": "V4A patch block starting with '*** Begin Patch' and ending with '*** End Patch'"
                    }
                },
                "required": ["patch"]
            }),
            strict: Some(true),
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let patch_text =
            args.get("patch")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "apply_patch".into(),
                    message: "missing 'patch' field".into(),
                })?;
        execute_v4a_patch(patch_text, ctx).await
    }

    fn parallel_safe(&self) -> bool {
        false // file mutation
    }
}

inventory::submit!(&ApplyPatchTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::file_read::ReadFileTool;
    use tempfile::TempDir;

    fn ctx_in(dir: &std::path::Path) -> ToolContext {
        let mut ctx = ToolContext::test_context();
        ctx.cwd = dir.to_path_buf();
        ctx
    }

    #[tokio::test]
    async fn patch_exact_match() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(
            dir.path().join("code.rs"),
            "fn main() {\n    println!(\"old\");\n}\n",
        )
        .expect("write");

        let ctx = ctx_in(dir.path());
        let result = PatchTool
            .execute(
                json!({
                    "path": "code.rs",
                    "old_string": "println!(\"old\")",
                    "new_string": "println!(\"new\")"
                }),
                &ctx,
            )
            .await
            .expect("patch");

        assert!(result.contains("\"ok\":true") || result.contains("\"ok\": true"), "expected JSON ok result, got: {result}");
        let content = std::fs::read_to_string(dir.path().join("code.rs")).expect("read");
        assert!(content.contains("println!(\"new\")"));
        assert!(!content.contains("println!(\"old\")"));
    }

    #[tokio::test]
    async fn patch_no_match_errors() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("code.rs"), "fn main() {}").expect("write");

        let ctx = ctx_in(dir.path());
        let result = PatchTool
            .execute(
                json!({
                    "path": "code.rs",
                    "old_string": "nonexistent string",
                    "new_string": "replacement"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn patch_rejects_stale_replace_after_external_edit() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(
            dir.path().join("stale.rs"),
            "fn main() {\n    println!(\"old\");\n}\n",
        )
        .expect("write");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "patch-stale-guard".into();

        ReadFileTool
            .execute(json!({"path": "stale.rs", "line_numbers": false}), &ctx)
            .await
            .expect("read");

        std::fs::write(
            dir.path().join("stale.rs"),
            "fn main() {\n    println!(\"new\");\n}\n",
        )
        .expect("modify");

        let err = execute_replace_patch(
            ReplaceArgs {
                path: "stale.rs".into(),
                old_string: "println!(\"old\");".into(),
                new_string: "println!(\"patched\");".into(),
                replace_all: false,
            },
            &ctx,
        )
        .await
        .expect_err("stale patch should be rejected");

        assert!(err.to_string().contains("modified since you last read it"));
    }

    #[test]
    fn patch_schema_is_strict_and_uses_nullable_mode_specific_fields() {
        let schema = PatchTool.schema();
        assert_eq!(schema.strict, Some(true));
        assert_eq!(schema.parameters["type"], "object");
        assert_eq!(schema.parameters["additionalProperties"], false);
        assert_eq!(
            schema.parameters["required"],
            json!(["mode", "path", "old_string", "new_string", "replace_all", "patch"])
        );
        assert_eq!(
            schema.parameters["properties"]["path"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(
            schema.parameters["properties"]["patch"]["type"],
            json!(["string", "null"])
        );
    }

    #[test]
    fn apply_patch_schema_is_strict() {
        let schema = ApplyPatchTool.schema();
        assert_eq!(schema.strict, Some(true));
        assert_eq!(schema.parameters["type"], "object");
        assert_eq!(schema.parameters["additionalProperties"], false);
        assert_eq!(schema.parameters["required"], json!(["patch"]));
        assert_eq!(schema.parameters["properties"]["patch"]["type"], "string");
    }

    #[tokio::test]
    async fn patch_ambiguous_match_errors() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("dup.txt"), "aaa\naaa\n").expect("write");

        let ctx = ctx_in(dir.path());
        let result = PatchTool
            .execute(
                json!({
                    "path": "dup.txt",
                    "old_string": "aaa",
                    "new_string": "bbb"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result
            .expect_err("should fail for ambiguous match")
            .to_string();
        assert!(
            err.contains("2 occurrences") || err.contains("2 times"),
            "Got: {}",
            err
        );
    }

    #[tokio::test]
    async fn patch_maps_absolute_tmp_into_edgecrab_temp_root() {
        let dir = TempDir::new().expect("workspace");
        let edgecrab_home = TempDir::new().expect("edgecrab_home");
        let mapped = edgecrab_home.path().join("tmp/files/summary.md");
        std::fs::create_dir_all(mapped.parent().expect("tmp parent")).expect("create tmp parent");
        std::fs::write(&mapped, "old value\n").expect("write mapped tmp");

        let mut ctx = ctx_in(dir.path());
        ctx.config.edgecrab_home = edgecrab_home.path().to_path_buf();

        PatchTool
            .execute(
                json!({
                    "path": "/tmp/summary.md",
                    "old_string": "old value",
                    "new_string": "new value"
                }),
                &ctx,
            )
            .await
            .expect("patch virtual tmp");

        let content = std::fs::read_to_string(&mapped).expect("read mapped tmp");
        assert!(content.contains("new value"));
    }

    #[tokio::test]
    async fn patch_rejects_oversized_payloads() {
        let dir = TempDir::new().expect("workspace");
        std::fs::write(dir.path().join("code.rs"), "fn main() {}\n").expect("seed");
        let ctx = ctx_in(dir.path());
        let oversized = "x".repeat(DEFAULT_MAX_MUTATION_PAYLOAD_BYTES);

        let result = PatchTool
            .execute(
                json!({
                    "path": "code.rs",
                    "old_string": oversized,
                    "new_string": "y"
                }),
                &ctx,
            )
            .await;

        let err = result.expect_err("oversized patch must be rejected");
        assert!(
            err.to_string()
                .contains("Large single-call edit payloads are unreliable")
        );
    }

    #[tokio::test]
    async fn patch_accepts_v4a_payload_without_mode() {
        let dir = TempDir::new().expect("workspace");
        std::fs::write(
            dir.path().join("code.rs"),
            "fn main() {\n    println!(\"old\");\n}\n",
        )
        .expect("seed");
        let ctx = ctx_in(dir.path());
        let patch = concat!(
            "*** Begin Patch\n",
            "*** Update File: code.rs\n",
            " fn main() {\n",
            "-    println!(\"old\");\n",
            "+    println!(\"new\");\n",
            " }\n",
            "*** End Patch",
        );

        let result = PatchTool.execute(json!({"patch": patch}), &ctx).await;

        // R18: apply_patch now returns JSON {"ok":true,"modified":[...],"created":[...],"deleted":[...]}
        let out = result.expect("v4a patch via patch tool");
        let v: serde_json::Value = serde_json::from_str(&out).expect("JSON result");
        assert_eq!(v["ok"], serde_json::Value::Bool(true));
        let content = std::fs::read_to_string(dir.path().join("code.rs")).expect("read");
        assert!(content.contains("println!(\"new\")"));
    }

    #[tokio::test]
    async fn patch_accepts_v4a_payload_with_stray_path() {
        let dir = TempDir::new().expect("workspace");
        std::fs::write(dir.path().join("code.rs"), "const N: i32 = 1;\n").expect("seed");
        let ctx = ctx_in(dir.path());
        let patch = concat!(
            "*** Begin Patch\n",
            "*** Update File: code.rs\n",
            "-const N: i32 = 1;\n",
            "+const N: i32 = 2;\n",
            "*** End Patch",
        );

        let result = PatchTool
            .execute(json!({"path": "code.rs", "patch": patch}), &ctx)
            .await;

        // R18: apply_patch now returns JSON {"ok":true,...}
        let out = result.expect("legacy mixed patch args");
        let v: serde_json::Value = serde_json::from_str(&out).expect("JSON result");
        assert_eq!(v["ok"], serde_json::Value::Bool(true));
        let content = std::fs::read_to_string(dir.path().join("code.rs")).expect("read");
        assert!(content.contains("const N: i32 = 2;"));
    }

    // ─── V4A parse / apply unit tests ────────────────────────────────────

    #[test]
    fn v4a_parse_update_hunk() {
        let patch = "*** Begin Patch\n*** Update File: foo.rs\n context\n-old\n+new\n*** End Patch";
        let (ops, err) = parse_v4a(patch);
        assert!(err.is_none());
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0].kind, V4AOpKind::Update));
        assert_eq!(ops[0].file_path, "foo.rs");
        assert_eq!(ops[0].hunks.len(), 1);
        assert_eq!(ops[0].hunks[0].lines.len(), 3);
    }

    #[test]
    fn v4a_parse_add_delete_move() {
        let patch = concat!(
            "*** Begin Patch\n",
            "*** Add File: new.txt\n",
            "+hello\n",
            "*** Delete File: old.txt\n",
            "*** Move File: a.rs -> b.rs\n",
            "*** End Patch",
        );
        let (ops, err) = parse_v4a(patch);
        assert!(err.is_none(), "parse error: {:?}", err);
        assert_eq!(ops.len(), 3);
        assert!(matches!(ops[0].kind, V4AOpKind::Add));
        assert!(matches!(ops[1].kind, V4AOpKind::Delete));
        assert!(matches!(ops[2].kind, V4AOpKind::Move { .. }));
        if let V4AOpKind::Move { new_path } = &ops[2].kind {
            assert_eq!(new_path, "b.rs");
        }
    }

    #[test]
    fn v4a_apply_update_hunk_exact() {
        let content = "fn foo() {\n    let x = 1;\n    let y = 2;\n}\n";
        let hunk = Hunk {
            context_hint: None,
            lines: vec![
                HunkLine {
                    prefix: ' ',
                    content: "    let x = 1;".to_string(),
                },
                HunkLine {
                    prefix: '-',
                    content: "    let y = 2;".to_string(),
                },
                HunkLine {
                    prefix: '+',
                    content: "    let y = 99;".to_string(),
                },
            ],
        };
        let result = apply_update_hunk(content, &hunk).expect("apply");
        assert!(result.contains("let y = 99;"), "Got: {result}");
        assert!(!result.contains("let y = 2;"), "Got: {result}");
    }

    #[test]
    fn v4a_apply_update_hunk_fuzzy() {
        // trailing whitespace on context line — fuzzy should still match
        let content = "fn foo() {  \n    old_value\n}\n";
        let hunk = Hunk {
            context_hint: None,
            lines: vec![
                HunkLine {
                    prefix: ' ',
                    content: "fn foo() {".to_string(),
                },
                HunkLine {
                    prefix: '-',
                    content: "    old_value".to_string(),
                },
                HunkLine {
                    prefix: '+',
                    content: "    new_value".to_string(),
                },
            ],
        };
        let result = apply_update_hunk(content, &hunk).expect("fuzzy apply");
        assert!(result.contains("new_value"), "Got: {result}");
    }

    #[test]
    fn v4a_apply_update_hunk_uses_context_hint_for_ambiguous_match() {
        let content = "section a\nvalue=1\nsection b\nvalue=1\n";
        let hunk = Hunk {
            context_hint: Some("section b".to_string()),
            lines: vec![
                HunkLine {
                    prefix: '-',
                    content: "value=1".to_string(),
                },
                HunkLine {
                    prefix: '+',
                    content: "value=2".to_string(),
                },
            ],
        };

        let result = apply_update_hunk(content, &hunk).expect("hint apply");
        assert_eq!(result, "section a\nvalue=1\nsection b\nvalue=2\n");
    }

    #[tokio::test]
    async fn apply_patch_add_file() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let patch = "*** Begin Patch\n*** Add File: hello.txt\n+Hello, World!\n*** End Patch";
        let result = ApplyPatchTool
            .execute(json!({ "patch": patch }), &ctx)
            .await
            .expect("apply_patch add");

        let v: serde_json::Value = serde_json::from_str(&result).expect("JSON result");
        assert_eq!(v["ok"], true, "Got: {result}");
        assert!(
            v["created"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "Expected non-empty 'created' array, got: {result}"
        );
        let content = std::fs::read_to_string(dir.path().join("hello.txt")).expect("read");
        assert_eq!(content.trim(), "Hello, World!");
    }

    #[tokio::test]
    async fn apply_patch_delete_file() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("remove_me.txt"), "bye").expect("write");
        let ctx = ctx_in(dir.path());

        let patch = "*** Begin Patch\n*** Delete File: remove_me.txt\n*** End Patch";
        let result = ApplyPatchTool
            .execute(json!({ "patch": patch }), &ctx)
            .await
            .expect("apply_patch delete");

        let v: serde_json::Value = serde_json::from_str(&result).expect("JSON result");
        assert_eq!(v["ok"], true, "Got: {result}");
        assert!(
            v["deleted"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "Expected non-empty 'deleted' array, got: {result}"
        );
        assert!(!dir.path().join("remove_me.txt").exists());
    }

    #[tokio::test]
    async fn apply_patch_move_file() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("old.txt"), "data").expect("write");
        let ctx = ctx_in(dir.path());

        let patch = "*** Begin Patch\n*** Move File: old.txt -> new.txt\n*** End Patch";
        let result = ApplyPatchTool
            .execute(json!({ "patch": patch }), &ctx)
            .await
            .expect("apply_patch move");

        let v: serde_json::Value = serde_json::from_str(&result).expect("JSON result");
        assert_eq!(v["ok"], true, "Got: {result}");
        assert!(!dir.path().join("old.txt").exists());
        assert!(dir.path().join("new.txt").exists());
    }

    #[tokio::test]
    async fn apply_patch_add_file_refuses_existing_destination() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("existing.txt"), "old").expect("write");
        let ctx = ctx_in(dir.path());

        let patch = "*** Begin Patch\n*** Add File: existing.txt\n+new\n*** End Patch";
        let result = ApplyPatchTool
            .execute(json!({ "patch": patch }), &ctx)
            .await;
        assert!(result.is_err(), "Add should fail when file exists");

        let content = std::fs::read_to_string(dir.path().join("existing.txt")).expect("read");
        assert_eq!(content, "old");
    }

    #[tokio::test]
    async fn apply_patch_move_refuses_existing_destination() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("src.txt"), "src").expect("write src");
        std::fs::write(dir.path().join("dst.txt"), "dst").expect("write dst");
        let ctx = ctx_in(dir.path());

        let patch = "*** Begin Patch\n*** Move File: src.txt -> dst.txt\n*** End Patch";
        let result = ApplyPatchTool
            .execute(json!({ "patch": patch }), &ctx)
            .await;
        assert!(result.is_err(), "Move should fail when destination exists");

        assert!(dir.path().join("src.txt").exists());
        let dst_content = std::fs::read_to_string(dir.path().join("dst.txt")).expect("read dst");
        assert_eq!(dst_content, "dst");
    }

    #[tokio::test]
    async fn apply_patch_update_file() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(
            dir.path().join("greet.py"),
            "def greet():\n    print('hello')\n    return True\n",
        )
        .expect("write");
        let ctx = ctx_in(dir.path());

        let patch = concat!(
            "*** Begin Patch\n",
            "*** Update File: greet.py\n",
            " def greet():\n",
            "-    print('hello')\n",
            "+    print('world')\n",
            "     return True\n",
            "*** End Patch",
        );
        let result = ApplyPatchTool
            .execute(json!({ "patch": patch }), &ctx)
            .await
            .expect("apply_patch update");

        let v: serde_json::Value = serde_json::from_str(&result).expect("JSON result");
        assert_eq!(v["ok"], true, "Got: {result}");
        assert!(
            v["modified"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "Expected non-empty 'modified' array, got: {result}"
        );
        let content = std::fs::read_to_string(dir.path().join("greet.py")).expect("read");
        assert!(content.contains("print('world')"), "Got: {content}");
        assert!(!content.contains("print('hello')"), "Got: {content}");
    }

    #[tokio::test]
    async fn apply_patch_is_transactional_rolls_back_on_error() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("a.txt"), "old\n").expect("write a");
        std::fs::write(dir.path().join("b.txt"), "keep\n").expect("write b");
        let ctx = ctx_in(dir.path());

        // First update would succeed, second update fails -> entire patch rolled back.
        let patch = concat!(
            "*** Begin Patch\n",
            "*** Update File: a.txt\n",
            "-old\n",
            "+new\n",
            "*** Update File: b.txt\n",
            "-missing_line\n",
            "+replacement\n",
            "*** End Patch",
        );

        let result = ApplyPatchTool
            .execute(json!({ "patch": patch }), &ctx)
            .await;
        assert!(result.is_err(), "Patch should fail");

        let a_after = std::fs::read_to_string(dir.path().join("a.txt")).expect("read a");
        let b_after = std::fs::read_to_string(dir.path().join("b.txt")).expect("read b");
        assert_eq!(a_after, "old\n", "a.txt must be rolled back");
        assert_eq!(b_after, "keep\n", "b.txt must remain unchanged");
    }

    #[tokio::test]
    async fn apply_patch_add_file_maps_absolute_tmp_into_edgecrab_temp_root() {
        let dir = TempDir::new().expect("workspace");
        let edgecrab_home = TempDir::new().expect("edgecrab_home");
        let mut ctx = ctx_in(dir.path());
        ctx.config.edgecrab_home = edgecrab_home.path().to_path_buf();

        let patch = "*** Begin Patch\n*** Add File: /tmp/generated.txt\n+generated\n*** End Patch";
        ApplyPatchTool
            .execute(json!({ "patch": patch }), &ctx)
            .await
            .expect("apply_patch virtual tmp add");

        let content = std::fs::read_to_string(edgecrab_home.path().join("tmp/files/generated.txt"))
            .expect("read mapped tmp file");
        assert_eq!(content, "generated");
    }

    #[tokio::test]
    async fn apply_patch_rejects_oversized_payloads() {
        let dir = TempDir::new().expect("workspace");
        let ctx = ctx_in(dir.path());
        let oversized_body = "x".repeat(DEFAULT_MAX_MUTATION_PAYLOAD_BYTES);
        let patch = format!(
            "*** Begin Patch\n*** Add File: huge.txt\n+{}\n*** End Patch",
            oversized_body
        );

        let result = ApplyPatchTool
            .execute(json!({ "patch": patch }), &ctx)
            .await;

        let err = result.expect_err("oversized apply_patch must be rejected");
        assert!(
            err.to_string()
                .contains("Split the refactor into multiple focused apply_patch calls")
        );
        assert!(!dir.path().join("huge.txt").exists());
    }
}
