//! # edit_diff — single owner for local edit presentation (DRY · SOLID)
//!
//! Captures before-snapshots for `write_file` / `patch` / `apply_patch`, builds a
//! typed hunk model (`DiffLine` / `DiffHunk` / `EditPresentation`), and paints a
//! polished transcript card: dual gutters, insert/delete color, accurate +/−,
//! collapse caps, and consecutive-edit verb grouping.
//!
//! ## Ownership (SOLID)
//!
//! | Concern | Here? |
//! |---------|-------|
//! | Path jail + before-map | yes (`LocalEditSnapshot`) |
//! | TextDiff → hunks + stats | yes (`build_edit_presentation`) |
//! | Span chrome (gutter, paint) | yes (`render_*`) |
//! | Tool execution | no — tools crate only |
//! | Transcript storage | no — `OutputLine` in transcript.rs |
//!
//! Callers (`response_dispatch`, tests) only use the public capture/render API.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use edgecrab_tools::AppConfigRef;
use edgecrab_tools::path_utils::jail_write_path;
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use similar::{ChangeTag, TextDiff};

// ── Caps (first paint must stay cheap) ───────────────────────────────────────

/// Max files shown as full cards in one presentation.
const MAX_INLINE_DIFF_FILES: usize = 6;
/// Max painted content lines in the **expanded** body.
const MAX_EXPANDED_LINES: usize = 80;
/// Default collapsed body (header excluded): Grok-like glanceable card.
const MAX_COLLAPSED_LINES: usize = 8;
/// Context radius around each change for `similar`.
const CONTEXT_RADIUS: usize = 3;

// ── Semantic colors (edit chrome; map into skin later if needed) ─────────────

mod chrome {
    use super::*;

    pub fn gutter() -> Style {
        Style::default()
            .fg(Color::Rgb(55, 58, 70))
            .add_modifier(Modifier::DIM)
    }

    pub fn header_label() -> Style {
        Style::default()
            .fg(Color::Rgb(150, 160, 185))
            .add_modifier(Modifier::DIM)
    }

    pub fn path() -> Style {
        Style::default()
            .fg(Color::Rgb(255, 205, 110))
            .add_modifier(Modifier::BOLD)
    }

    pub fn action() -> Style {
        Style::default().fg(Color::Rgb(160, 175, 200))
    }

    pub fn plus() -> Style {
        Style::default()
            .fg(Color::Rgb(90, 210, 130))
            .add_modifier(Modifier::BOLD)
    }

    pub fn minus() -> Style {
        Style::default()
            .fg(Color::Rgb(240, 120, 120))
            .add_modifier(Modifier::BOLD)
    }

    pub fn hunk_meta() -> Style {
        Style::default().fg(Color::Rgb(110, 175, 230))
    }

    pub fn equal_text() -> Style {
        Style::default()
            .fg(Color::Rgb(120, 125, 140))
            .add_modifier(Modifier::DIM)
    }

    pub fn insert_text() -> Style {
        Style::default()
            .fg(Color::Rgb(180, 245, 195))
            .bg(Color::Rgb(18, 48, 32))
    }

    pub fn delete_text() -> Style {
        Style::default()
            .fg(Color::Rgb(255, 185, 185))
            .bg(Color::Rgb(52, 22, 26))
    }

    pub fn insert_marker() -> Style {
        Style::default()
            .fg(Color::Rgb(90, 210, 130))
            .bg(Color::Rgb(18, 48, 32))
            .add_modifier(Modifier::BOLD)
    }

    pub fn delete_marker() -> Style {
        Style::default()
            .fg(Color::Rgb(240, 120, 120))
            .bg(Color::Rgb(52, 22, 26))
            .add_modifier(Modifier::BOLD)
    }

    pub fn line_num() -> Style {
        Style::default()
            .fg(Color::Rgb(80, 88, 108))
            .add_modifier(Modifier::DIM)
    }

    pub fn sep() -> Style {
        Style::default()
            .fg(Color::Rgb(90, 100, 125))
            .add_modifier(Modifier::DIM)
    }

    pub fn group_header() -> Style {
        Style::default()
            .fg(Color::Rgb(255, 185, 70))
            .add_modifier(Modifier::BOLD)
    }
}

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub text: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub kind: DiffLineKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    /// Display path (relative when possible).
    pub path_display: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditStats {
    pub plus: usize,
    pub minus: usize,
    pub files: usize,
}

impl EditStats {
    pub fn is_empty(self) -> bool {
        self.plus == 0 && self.minus == 0
    }

    pub fn caption(self) -> String {
        match (self.plus, self.minus) {
            (0, 0) => "no changes".into(),
            (p, 0) => format!("+{p}"),
            (0, m) => format!("−{m}"),
            (p, m) => format!("+{p} −{m}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    Create,
    Update,
    Delete,
    Patch,
}

impl EditAction {
    pub fn label(self) -> &'static str {
        match self {
            EditAction::Create => "create",
            EditAction::Update => "update",
            EditAction::Delete => "delete",
            EditAction::Patch => "patch",
        }
    }
}

/// Full presentation model for one successful edit tool call.
#[derive(Debug, Clone)]
pub struct EditPresentation {
    pub path_display: String,
    pub action: EditAction,
    pub stats: EditStats,
    pub hunks: Vec<DiffHunk>,
    pub truncated: bool,
    /// Additional paths when multi-file (apply_patch).
    pub extra_paths: Vec<String>,
}

/// Rendered card ready for transcript push.
#[derive(Debug, Clone)]
pub struct EditDiffCard {
    /// Collapsed visual lines (header + up to [`MAX_COLLAPSED_LINES`] content).
    pub collapsed_lines: Vec<Vec<Span<'static>>>,
    /// Plain-text body for Ctrl+Shift+T expand (full capped card).
    pub expandable_body: Option<String>,
    pub presentation: EditPresentation,
}

/// Running card while an edit tool executes (before hunks are available).
pub fn build_editing_running_spans(path: &str) -> Vec<Span<'static>> {
    let display = if path.trim().is_empty() {
        "…".to_string()
    } else {
        path.to_string()
    };
    vec![
        Span::styled("  ┊ ".to_string(), chrome::gutter()),
        Span::styled("✎ ".to_string(), chrome::action()),
        Span::styled("Editing ".to_string(), chrome::header_label()),
        Span::styled(display, chrome::path()),
        Span::styled("…".to_string(), chrome::header_label()),
    ]
}

#[cfg(test)]
mod editing_running_tests {
    use super::*;

    #[test]
    fn editing_running_spans_include_path() {
        let spans = build_editing_running_spans("src/app.rs");
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("Editing"));
        assert!(text.contains("src/app.rs"));
    }
}

/// Accumulator for consecutive successful edit tools (verb group).
#[derive(Debug, Clone, Default)]
pub struct EditVerbGroup {
    pub files: Vec<String>,
    pub stats: EditStats,
    /// Transcript index of the group header line (rewritten on merge).
    pub header_line_idx: Option<usize>,
}

impl EditVerbGroup {
    pub fn push_file(&mut self, path: &str, stats: EditStats) {
        if !path.is_empty() && !self.files.iter().any(|f| f == path) {
            self.files.push(path.to_string());
        }
        // stats.files may already count multi-file patches — prefer unique path list.
        self.stats.plus = self.stats.plus.saturating_add(stats.plus);
        self.stats.minus = self.stats.minus.saturating_add(stats.minus);
        self.stats.files = self.files.len();
    }

    /// Spans for the group header: `✎ Edited 3 files  +42 −7`
    pub fn render_header_spans(&self) -> Vec<Span<'static>> {
        let n = self.files.len().max(1);
        let noun = if n == 1 { "file" } else { "files" };
        let stats = self.stats.caption();
        vec![
            Span::styled("  ┊ ".to_string(), chrome::gutter()),
            Span::styled("✎ ".to_string(), chrome::group_header()),
            Span::styled(
                format!("Edited {n} {noun}  {stats}"),
                chrome::group_header(),
            ),
        ]
    }

    /// Expandable file list for the group header.
    pub fn expandable_file_list(&self) -> String {
        let mut lines = vec![format!(
            "Edited {} file(s)  {}",
            self.files.len(),
            self.stats.caption()
        )];
        for (i, f) in self.files.iter().enumerate() {
            lines.push(format!("  {}. {f}", i + 1));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone)]
pub struct LocalEditSnapshot {
    cwd: PathBuf,
    paths: Vec<PathBuf>,
    before: BTreeMap<PathBuf, Option<String>>,
    /// Tool that triggered the snapshot (for action labeling).
    tool_name: String,
}

// ── Public API ───────────────────────────────────────────────────────────────

pub fn capture_local_edit_snapshot(tool_name: &str, args_json: &str) -> Option<LocalEditSnapshot> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let config = cli_preview_config();
    capture_local_edit_snapshot_with(tool_name, args_json, &cwd, &config)
}

/// Build presentation from a before-snapshot (reads disk for after-state).
#[cfg(test)]
pub fn build_edit_presentation(snapshot: &LocalEditSnapshot) -> Option<EditPresentation> {
    build_edit_presentation_from_snapshot(snapshot)
}

/// Render collapsed card + optional expand body (preferred entry for transcript).
pub fn render_edit_diff_card(
    tool_name: &str,
    _args_json: &str,
    is_error: bool,
    snapshot: Option<&LocalEditSnapshot>,
) -> Option<EditDiffCard> {
    if is_error || !is_edit_tool(tool_name) {
        return None;
    }
    let snapshot = snapshot?;
    let presentation = build_edit_presentation_from_snapshot(snapshot)?;
    Some(paint_card(&presentation))
}

/// Backward-compatible: collapsed span lines only.
#[cfg(test)]
pub fn render_edit_diff_lines(
    tool_name: &str,
    args_json: &str,
    is_error: bool,
    snapshot: Option<&LocalEditSnapshot>,
) -> Option<Vec<Vec<Span<'static>>>> {
    render_edit_diff_card(tool_name, args_json, is_error, snapshot).map(|card| card.collapsed_lines)
}

/// Format stats for tool_display / shelf captions (DRY with presentation).
pub fn format_edit_stats_caption(stats: EditStats) -> String {
    stats.caption()
}

/// Count +/− lines from raw apply_patch / unified-ish patch text (args, not snapshot).
///
/// Counts content lines starting with `+`/`-` while ignoring file headers (`+++`/`---`).
pub fn count_patch_line_stats(patch_text: &str) -> EditStats {
    let mut plus = 0usize;
    let mut minus = 0usize;
    for line in patch_text.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            plus += 1;
        } else if line.starts_with('-') {
            minus += 1;
        }
    }
    let files = extract_apply_patch_paths(patch_text)
        .len()
        .max(if plus + minus > 0 { 1 } else { 0 });
    EditStats { plus, minus, files }
}

// ── Snapshot capture ─────────────────────────────────────────────────────────

fn cli_preview_config() -> AppConfigRef {
    let app_config = edgecrab_core::AppConfig::load().unwrap_or_default();
    AppConfigRef {
        edgecrab_home: edgecrab_core::edgecrab_home(),
        file_allowed_roots: app_config.tools.file.allowed_roots,
        path_restrictions: app_config.security.path_restrictions,
        ..Default::default()
    }
}

fn capture_local_edit_snapshot_with(
    tool_name: &str,
    args_json: &str,
    cwd: &Path,
    config: &AppConfigRef,
) -> Option<LocalEditSnapshot> {
    let paths = resolve_local_edit_paths(tool_name, args_json, cwd, config);
    if paths.is_empty() {
        return None;
    }

    let before = paths
        .iter()
        .cloned()
        .map(|path| {
            let text = std::fs::read_to_string(&path).ok();
            (path, text)
        })
        .collect();

    Some(LocalEditSnapshot {
        cwd: cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf()),
        paths,
        before,
        tool_name: tool_name.to_string(),
    })
}

fn resolve_local_edit_paths(
    tool_name: &str,
    args_json: &str,
    cwd: &Path,
    config: &AppConfigRef,
) -> Vec<PathBuf> {
    if !is_edit_tool(tool_name) {
        return Vec::new();
    }

    let Ok(args) = serde_json::from_str::<serde_json::Value>(args_json) else {
        return Vec::new();
    };
    let Some(obj) = args.as_object() else {
        return Vec::new();
    };

    let mut raw_paths = Vec::new();
    match tool_name {
        "write_file" => {
            if let Some(path) = obj.get("path").and_then(|value| value.as_str()) {
                raw_paths.push(path.to_string());
            }
        }
        "patch" => {
            if let Some(patch_text) = obj.get("patch").and_then(|value| value.as_str()) {
                raw_paths.extend(extract_apply_patch_paths(patch_text));
            } else if let Some(path) = obj.get("path").and_then(|value| value.as_str()) {
                raw_paths.push(path.to_string());
            }
        }
        "apply_patch" => {
            if let Some(patch_text) = obj.get("patch").and_then(|value| value.as_str()) {
                raw_paths.extend(extract_apply_patch_paths(patch_text));
            }
        }
        _ => {}
    }

    let mut resolved = Vec::new();
    for raw_path in raw_paths {
        let Some(path) = resolve_preview_write_path(&raw_path, cwd, config) else {
            continue;
        };
        if !resolved.iter().any(|existing| existing == &path) {
            resolved.push(path);
        }
    }
    resolved
}

fn is_edit_tool(tool_name: &str) -> bool {
    matches!(tool_name, "write_file" | "patch" | "apply_patch")
}

fn resolve_preview_write_path(
    raw_path: &str,
    cwd: &Path,
    config: &AppConfigRef,
) -> Option<PathBuf> {
    let policy = config.file_path_policy(cwd);
    if let Ok(path) = jail_write_path(raw_path, &policy) {
        return Some(path);
    }

    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let candidate = if let Some(stripped) = raw_path.strip_prefix("/tmp/") {
        config.file_tools_tmp_dir().join(stripped)
    } else if raw_path == "/tmp" {
        config.file_tools_tmp_dir()
    } else if let Some(stripped) = raw_path.strip_prefix("tmp/files/") {
        config.file_tools_tmp_dir().join(stripped)
    } else if raw_path == "tmp/files" {
        config.file_tools_tmp_dir()
    } else {
        let raw = PathBuf::from(raw_path);
        if raw.is_absolute() {
            raw
        } else {
            cwd.join(raw)
        }
    };
    let normalized = normalize_path(&candidate);

    // Traversal guard: reject relative paths that escape the workspace root.
    // Exception: `/tmp/` prefixed paths are virtual-tmp references that have
    // already been remapped to file_tools_tmp_dir() above; on Windows they
    // are not considered absolute (no drive letter) so we must not reject them
    // here — the allowed_roots check below handles the permission boundary.
    let is_virtual_tmp = raw_path == "/tmp"
        || raw_path.starts_with("/tmp/")
        || raw_path == "tmp/files"
        || raw_path.starts_with("tmp/files/");
    if !is_virtual_tmp && !Path::new(raw_path).is_absolute() && !normalized.starts_with(&cwd) {
        return None;
    }

    let mut allowed_roots = vec![cwd.clone()];
    // Add both the raw path and the canonicalized path for file_tools_tmp_dir.
    // WHY both: normalize_path() produces non-UNC paths (no \\?\ prefix), while
    // canonicalize() on Windows returns \\?\-prefixed paths. starts_with() is
    // component-based so these two forms never compare as equal; having both
    // ensures candidates from either code path are accepted. The directory is
    // created by file_path_policy() earlier in this function so canonicalize
    // usually succeeds, but we push the raw path unconditionally for the
    // case where it doesn't exist yet.
    let tmp_dir = config.file_tools_tmp_dir();
    allowed_roots.push(tmp_dir.clone());
    if let Ok(root) = tmp_dir.canonicalize()
        && root != tmp_dir
    {
        allowed_roots.push(root);
    }
    for root in &config.file_allowed_roots {
        let resolved = if root.is_absolute() {
            root.clone()
        } else {
            cwd.join(root)
        };
        if let Ok(canonical) = resolved.canonicalize() {
            allowed_roots.push(canonical);
        }
    }

    if !allowed_roots
        .iter()
        .any(|root| normalized.starts_with(root))
    {
        return None;
    }

    for denied in &config.path_restrictions {
        let denied_root = if denied.is_absolute() {
            denied.clone()
        } else {
            cwd.join(denied)
        };
        if normalized.starts_with(normalize_path(&denied_root)) {
            return None;
        }
    }

    Some(normalized)
}

fn extract_apply_patch_paths(patch_text: &str) -> Vec<String> {
    let mut paths = Vec::new();

    for line in patch_text.lines().map(str::trim) {
        if let Some(path) = line.strip_prefix("*** Update File:") {
            paths.push(path.trim().to_string());
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Add File:") {
            paths.push(path.trim().to_string());
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Delete File:") {
            paths.push(path.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("*** Move File:")
            && let Some((old_path, new_path)) = rest.split_once("->")
        {
            paths.push(old_path.trim().to_string());
            paths.push(new_path.trim().to_string());
        }
    }

    paths
}

// ── Hunk model ───────────────────────────────────────────────────────────────

fn build_edit_presentation_from_snapshot(snapshot: &LocalEditSnapshot) -> Option<EditPresentation> {
    let mut hunks = Vec::new();
    let mut total_plus = 0usize;
    let mut total_minus = 0usize;
    let mut path_displays = Vec::new();
    let mut truncated = false;

    for path in &snapshot.paths {
        let before = snapshot.before.get(path).cloned().unwrap_or(None);
        let after = std::fs::read_to_string(path).ok();
        if before == after {
            continue;
        }

        let display = display_diff_path(path, &snapshot.cwd);
        path_displays.push(display.clone());

        let before_s = before.as_deref().unwrap_or("");
        let after_s = after.as_deref().unwrap_or("");
        let (file_hunks, plus, minus, file_truncated) =
            hunks_from_texts(&display, before_s, after_s);
        total_plus = total_plus.saturating_add(plus);
        total_minus = total_minus.saturating_add(minus);
        truncated |= file_truncated;

        if hunks.len() < MAX_INLINE_DIFF_FILES {
            hunks.extend(file_hunks);
        } else {
            truncated = true;
        }
    }

    if hunks.is_empty() && total_plus == 0 && total_minus == 0 {
        return None;
    }

    let primary = path_displays
        .first()
        .cloned()
        .unwrap_or_else(|| "file".into());
    let extra_paths = path_displays.into_iter().skip(1).collect::<Vec<_>>();
    let files = (1 + extra_paths.len()).max(1);

    let action = infer_action(snapshot, total_plus, total_minus);

    Some(EditPresentation {
        path_display: primary,
        action,
        stats: EditStats {
            plus: total_plus,
            minus: total_minus,
            files,
        },
        hunks,
        truncated,
        extra_paths,
    })
}

fn infer_action(snapshot: &LocalEditSnapshot, plus: usize, minus: usize) -> EditAction {
    match snapshot.tool_name.as_str() {
        "apply_patch" | "patch" => EditAction::Patch,
        "write_file" => {
            // Create if any tracked path had no before content.
            let all_new = snapshot
                .before
                .values()
                .all(|b| b.as_ref().map(|s| s.is_empty()).unwrap_or(true));
            let all_gone = plus == 0 && minus > 0;
            if all_gone {
                EditAction::Delete
            } else if all_new {
                EditAction::Create
            } else {
                EditAction::Update
            }
        }
        _ => EditAction::Update,
    }
}

/// Build hunks from before/after text. Returns (hunks, plus, minus, truncated).
fn hunks_from_texts(
    path_display: &str,
    before: &str,
    after: &str,
) -> (Vec<DiffHunk>, usize, usize, bool) {
    let diff = TextDiff::from_lines(before, after);
    let mut plus = 0usize;
    let mut minus = 0usize;
    let mut all_lines: Vec<DiffLine> = Vec::new();
    let mut old_ln: u32 = 1;
    let mut new_ln: u32 = 1;

    // Collect full change stream with line numbers, then group into context hunks.
    for change in diff.iter_all_changes() {
        let text = change.value().trim_end_matches('\n').to_string();
        match change.tag() {
            ChangeTag::Equal => {
                all_lines.push(DiffLine {
                    text,
                    old_line: Some(old_ln),
                    new_line: Some(new_ln),
                    kind: DiffLineKind::Equal,
                });
                old_ln = old_ln.saturating_add(1);
                new_ln = new_ln.saturating_add(1);
            }
            ChangeTag::Delete => {
                minus = minus.saturating_add(1);
                all_lines.push(DiffLine {
                    text,
                    old_line: Some(old_ln),
                    new_line: None,
                    kind: DiffLineKind::Delete,
                });
                old_ln = old_ln.saturating_add(1);
            }
            ChangeTag::Insert => {
                plus = plus.saturating_add(1);
                all_lines.push(DiffLine {
                    text,
                    old_line: None,
                    new_line: Some(new_ln),
                    kind: DiffLineKind::Insert,
                });
                new_ln = new_ln.saturating_add(1);
            }
        }
    }

    if plus == 0 && minus == 0 {
        return (Vec::new(), 0, 0, false);
    }

    // Group into hunks: keep CONTEXT_RADIUS equal lines around change clusters.
    let hunk_lines = trim_to_context_hunks(&all_lines, CONTEXT_RADIUS);
    let truncated = false; // line cap applied at paint time
    let hunk = DiffHunk {
        path_display: path_display.to_string(),
        lines: hunk_lines,
    };
    (vec![hunk], plus, minus, truncated)
}

/// Keep equal lines only within `radius` of an insert/delete; emit separators elsewhere.
fn trim_to_context_hunks(lines: &[DiffLine], radius: usize) -> Vec<DiffLine> {
    if lines.is_empty() {
        return Vec::new();
    }

    let is_change = |k: DiffLineKind| matches!(k, DiffLineKind::Insert | DiffLineKind::Delete);
    let mut keep = vec![false; lines.len()];

    for (i, line) in lines.iter().enumerate() {
        if is_change(line.kind) {
            let start = i.saturating_sub(radius);
            let end = (i + radius + 1).min(lines.len());
            for slot in keep.iter_mut().take(end).skip(start) {
                *slot = true;
            }
        }
    }

    // If no changes (shouldn't happen), keep nothing.
    if !keep.iter().any(|&k| k) {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if keep[i] {
            out.push(lines[i].clone());
            i += 1;
            continue;
        }
        // Skip a run of non-kept equals; insert a separator marker line.
        let mut j = i;
        while j < lines.len() && !keep[j] {
            j += 1;
        }
        let skipped = j - i;
        if skipped > 0 && !out.is_empty() && j < lines.len() {
            out.push(DiffLine {
                text: format!(
                    "… {skipped} unchanged line{}",
                    if skipped == 1 { "" } else { "s" }
                ),
                old_line: None,
                new_line: None,
                kind: DiffLineKind::Equal, // painted specially via text prefix
            });
        }
        i = j;
    }
    out
}

// ── Paint ────────────────────────────────────────────────────────────────────

fn paint_card(presentation: &EditPresentation) -> EditDiffCard {
    let full_lines = paint_full_lines(presentation, MAX_EXPANDED_LINES);
    let collapsed = paint_collapsed(presentation, &full_lines);

    let expandable_body = if full_lines.len() > collapsed.len() || presentation.truncated {
        Some(spans_to_plain(&full_lines))
    } else if !presentation.extra_paths.is_empty() {
        // Multi-file list still useful when the card is short.
        let mut body = spans_to_plain(&full_lines);
        body.push_str("\n\nfiles:\n");
        body.push_str(&presentation.path_display);
        body.push('\n');
        for p in &presentation.extra_paths {
            body.push_str(p);
            body.push('\n');
        }
        Some(body)
    } else {
        None
    };

    EditDiffCard {
        collapsed_lines: collapsed,
        expandable_body,
        presentation: presentation.clone(),
    }
}

fn paint_collapsed(
    presentation: &EditPresentation,
    full_lines: &[Vec<Span<'static>>],
) -> Vec<Vec<Span<'static>>> {
    // Always include header (first line); then up to MAX_COLLAPSED_LINES content.
    if full_lines.is_empty() {
        return vec![paint_header(presentation)];
    }
    let mut out = Vec::new();
    out.push(full_lines[0].clone());
    let content_budget = MAX_COLLAPSED_LINES;
    let rest: Vec<_> = full_lines
        .iter()
        .skip(1)
        .take(content_budget)
        .cloned()
        .collect();
    let omitted = full_lines.len().saturating_sub(1 + rest.len());
    out.extend(rest);
    if omitted > 0 || presentation.truncated {
        out.push(paint_more_line(omitted, presentation));
    }
    out
}

fn paint_full_lines(presentation: &EditPresentation, max_lines: usize) -> Vec<Vec<Span<'static>>> {
    let mut lines = Vec::new();
    lines.push(paint_header(presentation));

    let mut content_count = 0usize;
    let mut omitted = 0usize;

    for (hunk_idx, hunk) in presentation.hunks.iter().enumerate() {
        if content_count >= max_lines {
            omitted += hunk.lines.len();
            continue;
        }

        // File sub-header for multi-hunk / multi-file
        if presentation.hunks.len() > 1 || hunk_idx > 0 {
            if content_count < max_lines {
                lines.push(paint_file_subheader(&hunk.path_display));
                content_count += 1;
            } else {
                omitted += 1;
            }
        }

        for dl in &hunk.lines {
            if content_count >= max_lines {
                omitted += 1;
                continue;
            }
            lines.push(paint_diff_line(dl));
            content_count += 1;
        }
    }

    if omitted > 0 || presentation.truncated {
        lines.push(paint_more_line(omitted, presentation));
    }

    lines
}

fn paint_header(presentation: &EditPresentation) -> Vec<Span<'static>> {
    let stats = presentation.stats.caption();
    let mut spans = vec![
        Span::styled("  ┊ ".to_string(), chrome::gutter()),
        Span::styled("review ".to_string(), chrome::header_label()),
        Span::styled(presentation.path_display.clone(), chrome::path()),
        Span::styled(
            format!("  {}", presentation.action.label()),
            chrome::action(),
        ),
        Span::raw("  ".to_string()),
    ];

    // Color +/− independently when both present
    match (presentation.stats.plus, presentation.stats.minus) {
        (0, 0) => spans.push(Span::styled(stats, chrome::header_label())),
        (p, 0) => spans.push(Span::styled(format!("+{p}"), chrome::plus())),
        (0, m) => spans.push(Span::styled(format!("−{m}"), chrome::minus())),
        (p, m) => {
            spans.push(Span::styled(format!("+{p}"), chrome::plus()));
            spans.push(Span::styled(" ".to_string(), chrome::header_label()));
            spans.push(Span::styled(format!("−{m}"), chrome::minus()));
        }
    }

    if presentation.stats.files > 1 {
        spans.push(Span::styled(
            format!("  · {} files", presentation.stats.files),
            chrome::header_label(),
        ));
    }

    spans
}

fn paint_file_subheader(path: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled("  ┊ ".to_string(), chrome::gutter()),
        Span::styled("◆ ".to_string(), chrome::hunk_meta()),
        Span::styled(path.to_string(), chrome::path()),
    ]
}

fn paint_diff_line(dl: &DiffLine) -> Vec<Span<'static>> {
    // Separator rows produced by trim_to_context_hunks
    if dl.text.starts_with('…') && dl.old_line.is_none() && dl.new_line.is_none() {
        return vec![
            Span::styled("  ┊ ".to_string(), chrome::gutter()),
            Span::styled(dl.text.clone(), chrome::sep()),
        ];
    }

    let (marker, marker_style, text_style) = match dl.kind {
        DiffLineKind::Equal => (
            " ",
            Style::default().fg(Color::Rgb(70, 75, 90)),
            chrome::equal_text(),
        ),
        DiffLineKind::Insert => ("+", chrome::insert_marker(), chrome::insert_text()),
        DiffLineKind::Delete => ("−", chrome::delete_marker(), chrome::delete_text()),
    };

    let old_s = format_line_no(dl.old_line);
    let new_s = format_line_no(dl.new_line);

    vec![
        Span::styled("  ┊ ".to_string(), chrome::gutter()),
        Span::styled(format!("{old_s} "), chrome::line_num()),
        Span::styled(format!("{new_s} "), chrome::line_num()),
        Span::styled(format!("{marker} "), marker_style),
        Span::styled(dl.text.clone(), text_style),
    ]
}

fn format_line_no(n: Option<u32>) -> String {
    match n {
        Some(n) => format!("{n:>4}"),
        None => "    ".to_string(),
    }
}

fn paint_more_line(omitted: usize, presentation: &EditPresentation) -> Vec<Span<'static>> {
    let mut parts = Vec::new();
    if omitted > 0 {
        parts.push(format!(
            "+{omitted} more line{}",
            if omitted == 1 { "" } else { "s" }
        ));
    }
    if presentation.truncated {
        parts.push("truncated".into());
    }
    if !presentation.extra_paths.is_empty() && presentation.hunks.len() < presentation.stats.files {
        parts.push(format!(
            "+{} file(s)",
            presentation
                .stats
                .files
                .saturating_sub(presentation.hunks.len())
        ));
    }
    let text = if parts.is_empty() {
        "… more".to_string()
    } else {
        format!("… {}  · expand for full", parts.join(" · "))
    };
    vec![
        Span::styled("  ┊ ".to_string(), chrome::gutter()),
        Span::styled(text, chrome::hunk_meta()),
    ]
}

fn spans_to_plain(lines: &[Vec<Span<'static>>]) -> String {
    lines
        .iter()
        .map(|line| line.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Path helpers ─────────────────────────────────────────────────────────────

fn display_diff_path(path: &Path, cwd: &Path) -> String {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let s = path
        .strip_prefix(&cwd)
        .unwrap_or(path)
        .display()
        .to_string();
    // Normalize to forward slashes for consistent cross-platform display.
    s.replace('\\', "/")
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn preview_config(edgecrab_home: &Path) -> AppConfigRef {
        AppConfigRef {
            edgecrab_home: edgecrab_home.to_path_buf(),
            ..Default::default()
        }
    }

    fn line_text(lines: &[Vec<Span<'static>>]) -> String {
        spans_to_plain(lines)
    }

    #[test]
    fn resolve_local_edit_paths_for_apply_patch_tracks_all_affected_files() {
        let cwd = TempDir::new().expect("cwd");
        let edgecrab_home = TempDir::new().expect("edgecrab home");
        let patch = "\
*** Begin Patch
*** Update File: src/main.rs
@@
-old
+new
*** Add File: notes/todo.md
+hello
*** Delete File: src/old.rs
*** Move File: src/lib.rs -> src/core.rs
*** End Patch";

        let paths = resolve_local_edit_paths(
            "apply_patch",
            &serde_json::json!({ "patch": patch }).to_string(),
            cwd.path(),
            &preview_config(edgecrab_home.path()),
        );

        let rendered: Vec<String> = paths
            .iter()
            .map(|path| display_diff_path(path, cwd.path()))
            .collect();
        assert_eq!(
            rendered,
            vec![
                "src/main.rs",
                "notes/todo.md",
                "src/old.rs",
                "src/lib.rs",
                "src/core.rs"
            ]
        );
    }

    #[test]
    fn resolve_local_edit_paths_maps_tmp_through_file_tool_policy() {
        let cwd = TempDir::new().expect("cwd");
        let edgecrab_home = TempDir::new().expect("edgecrab home");

        let paths = resolve_local_edit_paths(
            "write_file",
            &serde_json::json!({ "path": "/tmp/report.md", "content": "hi" }).to_string(),
            cwd.path(),
            &preview_config(edgecrab_home.path()),
        );

        assert_eq!(paths.len(), 1);
        assert!(
            paths[0].ends_with("tmp/files/report.md"),
            "tmp preview path should target EdgeCrab tmp/files mirror: {}",
            paths[0].display()
        );
    }

    #[test]
    fn edit_hunks_from_write_snapshot() {
        let cwd = TempDir::new().expect("cwd");
        let edgecrab_home = TempDir::new().expect("edgecrab home");
        let file_path = cwd.path().join("main.rs");
        std::fs::write(&file_path, "fn main() {\n    println!(\"old\");\n}\n").expect("seed");

        let snapshot = capture_local_edit_snapshot_with(
            "write_file",
            &serde_json::json!({
                "path": "main.rs",
                "content": "fn main() {\n    println!(\"new\");\n}\n"
            })
            .to_string(),
            cwd.path(),
            &preview_config(edgecrab_home.path()),
        )
        .expect("snapshot");

        std::fs::write(&file_path, "fn main() {\n    println!(\"new\");\n}\n").expect("write new");

        let pres = build_edit_presentation(&snapshot).expect("presentation");
        assert_eq!(pres.stats.plus, 1);
        assert_eq!(pres.stats.minus, 1);
        assert_eq!(pres.action, EditAction::Update);
        assert!(!pres.hunks.is_empty());

        let has_insert = pres
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|l| l.kind == DiffLineKind::Insert && l.text.contains("new"));
        let has_delete = pres
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|l| l.kind == DiffLineKind::Delete && l.text.contains("old"));
        assert!(has_insert, "expected insert of new line");
        assert!(has_delete, "expected delete of old line");
    }

    #[test]
    fn edit_render_has_gutter_and_colors() {
        let cwd = TempDir::new().expect("cwd");
        let edgecrab_home = TempDir::new().expect("edgecrab home");
        let file_path = cwd.path().join("main.rs");
        std::fs::write(&file_path, "a\n").expect("seed");

        let snapshot = capture_local_edit_snapshot_with(
            "write_file",
            &serde_json::json!({ "path": "main.rs", "content": "b\n" }).to_string(),
            cwd.path(),
            &preview_config(edgecrab_home.path()),
        )
        .expect("snapshot");
        std::fs::write(&file_path, "b\n").expect("write");

        let card = render_edit_diff_card("write_file", "{}", false, Some(&snapshot)).expect("card");
        let joined = line_text(&card.collapsed_lines);
        assert!(joined.contains("review"), "header: {joined}");
        assert!(joined.contains("main.rs"), "path: {joined}");
        assert!(joined.contains('+') || joined.contains('−') || joined.contains('-'));

        // Insert lines must carry a non-default (green) style somewhere.
        let has_insert_style = card.collapsed_lines.iter().flatten().any(|span| {
            span.style.fg == Some(Color::Rgb(180, 245, 195))
                || span.style.fg == Some(Color::Rgb(90, 210, 130))
                || span.style.bg == Some(Color::Rgb(18, 48, 32))
        });
        assert!(has_insert_style, "expected insert paint styles");
    }

    #[test]
    fn edit_stats_match_hunk_counts() {
        let (hunks, plus, minus, _) =
            hunks_from_texts("f.rs", "one\ntwo\nthree\n", "one\nTWO\nthree\nfour\n");
        assert_eq!(plus, 2); // TWO + four
        assert_eq!(minus, 1); // two
        let counted_plus = hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.kind == DiffLineKind::Insert)
            .count();
        let counted_minus = hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.kind == DiffLineKind::Delete)
            .count();
        assert_eq!(counted_plus, plus);
        assert_eq!(counted_minus, minus);
        assert_eq!(
            EditStats {
                plus,
                minus,
                files: 1
            }
            .caption(),
            format!("+{plus} −{minus}")
        );
    }

    #[test]
    fn edit_caps_truncate_with_more_marker() {
        let before: String = (0..5).map(|i| format!("keep{i}\n")).collect();
        let after: String = (0..120).map(|i| format!("line{i}\n")).collect();
        let (hunks, plus, minus, _) = hunks_from_texts("big.txt", &before, &after);
        assert!(plus > 50);
        assert!(minus > 0 || plus > 0);

        let presentation = EditPresentation {
            path_display: "big.txt".into(),
            action: EditAction::Update,
            stats: EditStats {
                plus,
                minus,
                files: 1,
            },
            hunks,
            truncated: false,
            extra_paths: vec![],
        };
        let card = paint_card(&presentation);
        assert!(
            card.collapsed_lines.len() <= MAX_COLLAPSED_LINES + 3,
            "collapsed too tall: {}",
            card.collapsed_lines.len()
        );
        let joined = line_text(&card.collapsed_lines);
        assert!(
            joined.contains("more") || joined.contains("…") || card.expandable_body.is_some(),
            "expected truncation marker: {joined}"
        );
        if let Some(body) = &card.expandable_body {
            assert!(body.lines().count() > card.collapsed_lines.len());
        }
    }

    #[test]
    fn edit_path_jail_still_blocks() {
        let cwd = TempDir::new().expect("cwd");
        let edgecrab_home = TempDir::new().expect("edgecrab home");
        // Absolute path outside cwd and without allowed roots should fail.
        let paths = resolve_local_edit_paths(
            "write_file",
            &serde_json::json!({
                "path": "/etc/passwd",
                "content": "nope"
            })
            .to_string(),
            cwd.path(),
            &preview_config(edgecrab_home.path()),
        );
        assert!(
            paths.is_empty(),
            "path jail must block /etc/passwd, got {paths:?}"
        );
    }

    #[test]
    fn verb_group_merges_three_writes() {
        let mut group = EditVerbGroup::default();
        group.push_file(
            "a.rs",
            EditStats {
                plus: 10,
                minus: 2,
                files: 1,
            },
        );
        group.push_file(
            "b.rs",
            EditStats {
                plus: 5,
                minus: 1,
                files: 1,
            },
        );
        group.push_file(
            "c.rs",
            EditStats {
                plus: 3,
                minus: 0,
                files: 1,
            },
        );
        assert!(group.files.len() > 1);
        assert_eq!(group.files.len(), 3);
        assert_eq!(group.stats.plus, 18);
        assert_eq!(group.stats.minus, 3);
        let header = spans_to_plain(&[group.render_header_spans()]);
        assert!(header.contains("Edited 3 files"), "{header}");
        assert!(header.contains("+18"), "{header}");
        let list = group.expandable_file_list();
        assert!(list.contains("a.rs") && list.contains("b.rs") && list.contains("c.rs"));
    }

    #[test]
    fn expand_edit_card_roundtrip() {
        let cwd = TempDir::new().expect("cwd");
        let edgecrab_home = TempDir::new().expect("edgecrab home");
        let file_path = cwd.path().join("many.rs");
        let before: String = (0..40).map(|i| format!("L{i}\n")).collect();
        let after: String = (0..40)
            .map(|i| {
                if i % 3 == 0 {
                    format!("L{i}_changed\n")
                } else {
                    format!("L{i}\n")
                }
            })
            .collect();
        std::fs::write(&file_path, &before).expect("seed");

        let snapshot = capture_local_edit_snapshot_with(
            "write_file",
            &serde_json::json!({ "path": "many.rs", "content": after }).to_string(),
            cwd.path(),
            &preview_config(edgecrab_home.path()),
        )
        .expect("snapshot");
        std::fs::write(&file_path, &after).expect("write");

        let card = render_edit_diff_card("write_file", "{}", false, Some(&snapshot)).expect("card");
        let collapsed_n = card.collapsed_lines.len();
        assert!(collapsed_n <= MAX_COLLAPSED_LINES + 3);
        // With many changes, expand body should exist and be larger
        if let Some(body) = card.expandable_body {
            assert!(body.lines().count() >= collapsed_n);
        }
    }

    #[test]
    fn count_patch_line_stats_ignores_file_headers() {
        let patch = "\
--- a/foo.rs
+++ b/foo.rs
@@ -1,2 +1,3 @@
 context
-old
+new
+extra
";
        let stats = count_patch_line_stats(patch);
        assert_eq!(stats.plus, 2, "should not count +++ header");
        assert_eq!(stats.minus, 1, "should not count --- header");
    }

    #[test]
    fn render_edit_diff_lines_emits_review_block_for_file_changes() {
        let cwd = TempDir::new().expect("cwd");
        let edgecrab_home = TempDir::new().expect("edgecrab home");
        let file_path = cwd.path().join("main.rs");
        std::fs::write(&file_path, "fn main() {\n    println!(\"old\");\n}\n").expect("seed file");

        let snapshot = capture_local_edit_snapshot_with(
            "write_file",
            &serde_json::json!({
                "path": "main.rs",
                "content": "fn main() {\n    println!(\"new\");\n}\n"
            })
            .to_string(),
            cwd.path(),
            &preview_config(edgecrab_home.path()),
        )
        .expect("snapshot");

        std::fs::write(&file_path, "fn main() {\n    println!(\"new\");\n}\n").expect("write new");

        let lines =
            render_edit_diff_lines("write_file", "{}", false, Some(&snapshot)).expect("diff lines");
        let joined = line_text(&lines);

        assert!(joined.contains("review"), "{joined}");
        assert!(joined.contains("main.rs"), "{joined}");
        assert!(
            joined.contains("println!(\"old\")") || joined.contains("old"),
            "{joined}"
        );
        assert!(
            joined.contains("println!(\"new\")") || joined.contains("new"),
            "{joined}"
        );
        assert!(joined.contains('+') || joined.contains('−') || joined.contains('-'));
    }

    #[test]
    fn hunk_separator_for_distant_changes() {
        let before = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n";
        let after = "A\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nL\n";
        let (hunks, plus, minus, _) = hunks_from_texts("x.txt", before, after);
        assert_eq!(plus, 2);
        assert_eq!(minus, 2);
        let flat: Vec<&str> = hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(|l| l.text.as_str()))
            .collect();
        let has_sep = flat.iter().any(|t| t.starts_with('…'));
        assert!(has_sep, "expected unchanged separator, got {flat:?}");
    }
}
