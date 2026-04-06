use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use edgecrab_tools::AppConfigRef;
use edgecrab_tools::path_utils::jail_write_path;
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use similar::TextDiff;

const MAX_INLINE_DIFF_FILES: usize = 6;
const MAX_INLINE_DIFF_LINES: usize = 80;

#[derive(Debug, Clone)]
pub struct LocalEditSnapshot {
    cwd: PathBuf,
    paths: Vec<PathBuf>,
    before: BTreeMap<PathBuf, Option<String>>,
}

pub fn capture_local_edit_snapshot(tool_name: &str, args_json: &str) -> Option<LocalEditSnapshot> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let config = cli_preview_config();
    capture_local_edit_snapshot_with(tool_name, args_json, &cwd, &config)
}

pub fn render_edit_diff_lines(
    tool_name: &str,
    _args_json: &str,
    is_error: bool,
    snapshot: Option<&LocalEditSnapshot>,
) -> Option<Vec<Vec<Span<'static>>>> {
    if is_error || !is_edit_tool(tool_name) {
        return None;
    }
    let snapshot = snapshot?;
    let diff = diff_from_snapshot(snapshot)?;
    let mut lines = Vec::new();
    lines.push(render_header_line("review diff"));
    lines.extend(summarize_rendered_diff_sections(&diff));
    if lines.len() <= 1 { None } else { Some(lines) }
}

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
        "write_file" | "patch" => {
            if let Some(path) = obj.get("path").and_then(|value| value.as_str()) {
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
    let is_virtual_tmp = raw_path == "/tmp" || raw_path.starts_with("/tmp/");
    if !is_virtual_tmp && !Path::new(raw_path).is_absolute() && !normalized.starts_with(&cwd) {
        return None;
    }

    let mut allowed_roots = vec![cwd.clone()];
    // Always permit file_tools_tmp_dir as a preview target even if the
    // directory has not been created yet (canonicalize would fail on a
    // non-existent path). The starts_with comparison below uses the same
    // non-canonical base as `candidate`, so no canonicalization is needed.
    let tmp_dir = config.file_tools_tmp_dir();
    if let Ok(root) = tmp_dir.canonicalize() {
        allowed_roots.push(root);
    } else {
        allowed_roots.push(tmp_dir);
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
        if let Some(rest) = line.strip_prefix("*** Move File:") {
            if let Some((old_path, new_path)) = rest.split_once("->") {
                paths.push(old_path.trim().to_string());
                paths.push(new_path.trim().to_string());
            }
        }
    }

    paths
}

fn diff_from_snapshot(snapshot: &LocalEditSnapshot) -> Option<String> {
    let mut chunks = Vec::new();

    for path in &snapshot.paths {
        let before = snapshot.before.get(path).cloned().unwrap_or(None);
        let after = std::fs::read_to_string(path).ok();
        if before == after {
            continue;
        }

        let display_path = display_diff_path(path, &snapshot.cwd);
        let diff = TextDiff::from_lines(
            before.as_deref().unwrap_or(""),
            after.as_deref().unwrap_or(""),
        )
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{display_path}"), &format!("b/{display_path}"))
        .to_string();
        if !diff.trim().is_empty() {
            chunks.push(diff);
        }
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join(""))
    }
}

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

fn split_unified_diff_sections(diff: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = Vec::new();

    for line in diff.lines() {
        if line.starts_with("--- ") && !current.is_empty() {
            sections.push(current.join("\n"));
            current.clear();
        }
        current.push(line.to_string());
    }

    if !current.is_empty() {
        sections.push(current.join("\n"));
    }

    sections
}

fn summarize_rendered_diff_sections(diff: &str) -> Vec<Vec<Span<'static>>> {
    let sections = split_unified_diff_sections(diff);
    let mut rendered = Vec::new();
    let mut omitted_files = 0usize;
    let mut omitted_lines = 0usize;

    for (idx, section) in sections.iter().enumerate() {
        if idx >= MAX_INLINE_DIFF_FILES {
            omitted_files += 1;
            omitted_lines += render_inline_unified_diff(section).len();
            continue;
        }

        let section_lines = render_inline_unified_diff(section);
        let remaining_budget = MAX_INLINE_DIFF_LINES.saturating_sub(rendered.len());
        if remaining_budget == 0 {
            omitted_files += 1;
            omitted_lines += section_lines.len();
            continue;
        }

        if section_lines.len() <= remaining_budget {
            rendered.extend(section_lines);
            continue;
        }

        rendered.extend(section_lines.into_iter().take(remaining_budget));
        omitted_files += 1 + sections.len().saturating_sub(idx + 1);
        omitted_lines += render_inline_unified_diff(section)
            .len()
            .saturating_sub(remaining_budget);
        for leftover in sections.iter().skip(idx + 1) {
            omitted_lines += render_inline_unified_diff(leftover).len();
        }
        break;
    }

    if omitted_files > 0 || omitted_lines > 0 {
        rendered.push(render_summary_line(omitted_files, omitted_lines));
    }

    rendered
}

fn render_inline_unified_diff(diff: &str) -> Vec<Vec<Span<'static>>> {
    let mut rendered = Vec::new();
    let mut from_file: Option<String> = None;

    for raw_line in diff.lines() {
        if let Some(rest) = raw_line.strip_prefix("--- ") {
            from_file = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("+++ ") {
            let to_file = rest.trim().to_string();
            rendered.push(render_file_line(
                from_file.as_deref().unwrap_or("a/?"),
                &to_file,
            ));
            continue;
        }
        if raw_line.starts_with("@@") {
            rendered.push(render_diff_line(
                raw_line.to_string(),
                Style::default().fg(Color::Rgb(120, 190, 255)),
            ));
            continue;
        }
        if raw_line.starts_with('+') {
            rendered.push(render_diff_line(
                raw_line.to_string(),
                Style::default().fg(Color::Rgb(120, 220, 140)),
            ));
            continue;
        }
        if raw_line.starts_with('-') {
            rendered.push(render_diff_line(
                raw_line.to_string(),
                Style::default().fg(Color::Rgb(245, 130, 130)),
            ));
            continue;
        }
        if raw_line.starts_with(' ') {
            rendered.push(render_diff_line(
                raw_line.to_string(),
                Style::default()
                    .fg(Color::Rgb(125, 130, 145))
                    .add_modifier(Modifier::DIM),
            ));
            continue;
        }
        if !raw_line.is_empty() {
            rendered.push(render_diff_line(raw_line.to_string(), Style::default()));
        }
    }

    rendered
}

fn render_header_line(label: &str) -> Vec<Span<'static>> {
    let gutter_style = Style::default()
        .fg(Color::Rgb(60, 60, 72))
        .add_modifier(Modifier::DIM);
    let label_style = Style::default()
        .fg(Color::Rgb(160, 170, 190))
        .add_modifier(Modifier::DIM);
    vec![
        Span::styled("  ┊ ".to_string(), gutter_style),
        Span::styled(label.to_string(), label_style),
    ]
}

fn render_file_line(from_file: &str, to_file: &str) -> Vec<Span<'static>> {
    let gutter_style = Style::default()
        .fg(Color::Rgb(60, 60, 72))
        .add_modifier(Modifier::DIM);
    let file_style = Style::default().fg(Color::Rgb(255, 210, 120));
    vec![
        Span::styled("  ┊ ".to_string(), gutter_style),
        Span::styled(format!("{from_file} -> {to_file}"), file_style),
    ]
}

fn render_diff_line(text: String, style: Style) -> Vec<Span<'static>> {
    let gutter_style = Style::default()
        .fg(Color::Rgb(60, 60, 72))
        .add_modifier(Modifier::DIM);
    vec![
        Span::styled("  ┊ ".to_string(), gutter_style),
        Span::styled(text, style),
    ]
}

fn render_summary_line(omitted_files: usize, omitted_lines: usize) -> Vec<Span<'static>> {
    let text = format!(
        "... omitted {omitted_lines} diff line(s) across {omitted_files} additional file(s)/section(s)"
    );
    render_diff_line(text, Style::default().fg(Color::Rgb(120, 190, 255)))
}

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
        let joined = lines
            .iter()
            .map(|line| {
                line.iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(joined.contains("review diff"));
        assert!(joined.contains("a/main.rs -> b/main.rs"));
        assert!(joined.contains("-    println!(\"old\");"));
        assert!(joined.contains("+    println!(\"new\");"));
    }
}
