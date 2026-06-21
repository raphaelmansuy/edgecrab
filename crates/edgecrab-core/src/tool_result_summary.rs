//! One-line semantic summaries for pruned tool results (Hermes parity + spill path).
//!
//! Used by compression pre-pass and TUI shelf previews — single source of truth.

use serde_json::Value;

/// Duplicate tool output back-reference (Hermes parity).
pub const DUPLICATE_TOOL_OUTPUT: &str =
    "[Duplicate tool output — same content as a more recent call]";

const PRUNE_ALREADY_MARKERS: &[&str] = &[
    "[tool output pruned — reclaimed context window space]",
    DUPLICATE_TOOL_OUTPUT,
    "[tool_result_spill]",
];

fn arg_str_or(args: &Value, key: &str, default: &str) -> String {
    arg_str(args, key).unwrap_or_else(|| default.to_string())
}

/// Build a compact history line for a pruned tool result.
pub fn summarize_tool_result_for_history(
    tool_name: &str,
    tool_args_json: &str,
    tool_content: &str,
    is_error: bool,
) -> String {
    let args: Value =
        serde_json::from_str(tool_args_json).unwrap_or(Value::Object(Default::default()));
    let args_obj = args.as_object();

    let content = tool_content;
    let content_len = content.chars().count();
    let line_count = content.lines().count().max(1);

    if content.trim().starts_with("[tool_result_spill]") {
        let path_hint = infer_source_path_from_spill(content)
            .map(|p| format!(" ({p})"))
            .unwrap_or_default();
        return format!("[{tool_name}] spilled to artifact{path_hint} ({content_len} chars)");
    }

    if is_error {
        let line = extract_tool_error_text(content);
        let line = line
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or(&line);
        return format!("[{tool_name}] error: {}", truncate_chars(line, 120));
    }

    if let Ok(value) = serde_json::from_str::<Value>(content)
        && let Some(obj) = value.as_object()
        && let Some(summary) = summarize_structured_json(tool_name, obj)
    {
        if tool_name == "web_search" {
            let query = arg_str_or(&args, "query", "?");
            return format!("[web_search] query='{query}' — {summary}");
        }
        return summary;
    }

    match tool_name {
        "terminal" => summarize_terminal(&args, content, line_count),
        "read_file" => {
            let path = resolve_path_hint(&args, content);
            let offset = args_obj
                .and_then(|o| o.get("offset").or_else(|| o.get("line_start")))
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
            format!("[read_file] read {path} from line {offset} ({content_len} chars)")
        }
        "write_file" | "patch" => {
            let path = resolve_path_hint(&args, content);
            if tool_name == "patch" {
                let mode = arg_str_or(&args, "mode", "replace");
                format!("[patch] {mode} in {path} ({content_len} chars result)")
            } else {
                let lines = args_obj
                    .and_then(|o| o.get("content"))
                    .and_then(|v| v.as_str())
                    .map(|c| c.lines().count())
                    .unwrap_or(0);
                let lines_label = if lines > 0 {
                    format!("{lines} lines")
                } else {
                    format!("{content_len} chars")
                };
                format!("[write_file] wrote to {path} ({lines_label})")
            }
        }
        "search_files" => {
            let pattern = arg_str_or(&args, "pattern", "?");
            let path = arg_str_or(&args, "path", ".");
            let target = arg_str_or(&args, "target", "content");
            let count = serde_json::from_str::<Value>(content)
                .ok()
                .and_then(|v| v.get("total_count").and_then(|c| c.as_u64()))
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".into());
            format!("[search_files] {target} search for '{pattern}' in {path} -> {count} matches")
        }
        "web_search" => {
            let query = arg_str_or(&args, "query", "?");
            if let Ok(value) = serde_json::from_str::<Value>(content)
                && let Some(obj) = value.as_object()
                && let Some(summary) = summarize_structured_json("web_search", obj)
            {
                return format!("[web_search] query='{query}' — {summary}");
            }
            format!("[web_search] query='{query}' ({content_len} chars result)")
        }
        "web_extract" | "web_crawl" => {
            let url_desc = web_extract_url_desc(&args);
            format!("[{tool_name}] {url_desc} ({content_len} chars)")
        }
        "skill_view" | "skills_list" | "skill_manage" => {
            let name = arg_str_or(&args, "name", "?");
            format!("[{tool_name}] name={name} ({content_len} chars)")
        }
        "delegate_task" => {
            let goal = arg_str_or(&args, "goal", "?");
            format!(
                "[delegate_task] '{}' ({content_len} chars result)",
                truncate_chars(&goal, 60)
            )
        }
        "execute_code" => {
            let preview = arg_str(&args, "code")
                .unwrap_or_default()
                .replace('\n', " ");
            format!(
                "[execute_code] `{}` ({line_count} lines output)",
                truncate_chars(&preview, 60)
            )
        }
        "vision_analyze" | "browser_vision" => {
            let question = arg_str(&args, "question")
                .or_else(|| arg_str(&args, "prompt"))
                .unwrap_or_default();
            format!(
                "[{tool_name}] '{}' ({content_len} chars)",
                truncate_chars(&question, 50)
            )
        }
        "computer_use" => format!("[computer_use] ({content_len} chars)"),
        _ => {
            let mut hint = String::new();
            if let Some(obj) = args_obj {
                for (k, v) in obj.iter().take(2) {
                    let sv = truncate_chars(&v.to_string(), 40);
                    hint.push_str(&format!(" {k}={sv}"));
                }
            }
            format!("[{tool_name}]{hint} ({content_len} chars result)")
        }
    }
}

/// Shelf/TUI preview — structured JSON and terminal body extraction for display.
pub fn summarize_tool_result_preview(
    name: &str,
    tool_result: &str,
    is_error: bool,
) -> Option<String> {
    if tool_result.trim().starts_with("[tool_result_spill]") {
        if name == "computer_use" {
            for line in tool_result.lines() {
                if line.starts_with("--- BEGIN PREVIEW") {
                    continue;
                }
                if line.starts_with("--- END PREVIEW") {
                    break;
                }
                let trimmed = line.trim();
                if trimmed.is_empty()
                    || trimmed.starts_with("tool:")
                    || trimmed.starts_with("bytes:")
                {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    if let Some(msg) = val.get("message").and_then(|v| v.as_str()) {
                        return Some(truncate_chars(msg, 88));
                    }
                    if let Some(summary) = val
                        .get("text_summary")
                        .or_else(|| val.get("summary"))
                        .and_then(|v| v.as_str())
                    {
                        let first = summary.lines().next().unwrap_or(summary);
                        return Some(truncate_chars(first, 88));
                    }
                }
                if trimmed.contains("capture mode=") {
                    return Some(truncate_chars(trimmed, 88));
                }
            }
        }
        return Some(truncate_chars("[spilled — use read_file on artifact]", 88));
    }

    if is_error {
        let line = extract_tool_error_text(tool_result);
        let line = line
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or(line.trim());
        if line.is_empty() {
            return None;
        }
        return Some(truncate_chars(line, 88));
    }

    if name == "computer_use"
        && let Ok(value) = serde_json::from_str::<Value>(tool_result)
    {
        if let Some(msg) = value.get("message").and_then(|v| v.as_str()) {
            return Some(truncate_chars(msg.trim(), 88));
        }
        if value.get("_multimodal").and_then(|v| v.as_bool()) == Some(true) {
            let summary = value
                .get("text_summary")
                .and_then(|v| v.as_str())
                .unwrap_or("capture");
            let first = summary.lines().next().unwrap_or(summary);
            return Some(truncate_chars(first, 88));
        }
        if let Some(summary) = value.get("summary").and_then(|v| v.as_str()) {
            let first = summary.lines().next().unwrap_or(summary);
            return Some(truncate_chars(first, 88));
        }
    }

    if let Ok(value) = serde_json::from_str::<Value>(tool_result)
        && let Some(obj) = value.as_object()
        && let Some(summary) = summarize_structured_json(name, obj)
    {
        return Some(truncate_chars(&summary, 88));
    }

    if name == "terminal" {
        let mut lines = tool_result.lines();
        let _header = lines.next();
        if let Some(body) = lines
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("exit code:"))
        {
            return Some(truncate_chars(body, 88));
        }
        if let Some(exit_line) = tool_result
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("exit code:"))
        {
            return Some(truncate_chars(exit_line, 88));
        }
    }

    let summary = summarize_tool_result_for_history(name, "", tool_result, is_error);
    if summary.is_empty() {
        return None;
    }
    Some(truncate_chars(&summary, 88))
}

pub fn is_already_pruned_marker(content: &str) -> bool {
    PRUNE_ALREADY_MARKERS
        .iter()
        .any(|marker| content.contains(marker))
        || content.starts_with('[') && content.contains("] query='")
        || content.starts_with("[Duplicate tool output")
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Resolve a file path from tool args (aliases) or spilled result metadata.
fn resolve_path_hint(args: &Value, tool_content: &str) -> String {
    for key in ["path", "file_path", "file", "filename"] {
        if let Some(p) = arg_str(args, key)
            && !p.is_empty()
        {
            return p;
        }
    }
    infer_source_path_from_spill(tool_content).unwrap_or_else(|| "?".into())
}

fn infer_source_path_from_spill(content: &str) -> Option<String> {
    if !content.contains("[tool_result_spill]") {
        return None;
    }
    content.lines().find_map(|line| {
        line.strip_prefix("source_path:")
            .or_else(|| line.strip_prefix("source_path: "))
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(String::from)
    })
}

fn web_extract_url_desc(args: &Value) -> String {
    if let Some(url) = arg_str(args, "url") {
        return url;
    }
    if let Some(urls) = args.get("urls").and_then(|v| v.as_array()) {
        let first = urls.first().and_then(|v| v.as_str()).unwrap_or("?");
        if urls.len() > 1 {
            return format!("{first} (+{} more)", urls.len() - 1);
        }
        return first.to_string();
    }
    "?".into()
}

fn summarize_terminal(args: &Value, content: &str, line_count: usize) -> String {
    let cmd = arg_str(args, "command").unwrap_or_default();
    let cmd = truncate_chars(&cmd, 80);
    let exit_code = serde_json::from_str::<Value>(content)
        .ok()
        .and_then(|v| v.get("exit_code").and_then(|c| c.as_i64()))
        .or_else(|| {
            content
                .lines()
                .find_map(|l| l.strip_prefix("exit code:").map(|s| s.trim().parse().ok()))
                .flatten()
        })
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".into());
    format!("[terminal] ran `{cmd}` -> exit {exit_code}, {line_count} lines output")
}

fn summarize_structured_json(name: &str, obj: &serde_json::Map<String, Value>) -> Option<String> {
    match name {
        "web_search" => {
            let count = obj.get("results")?.as_array()?.len();
            let backend = obj.get("backend").and_then(|v| v.as_str()).unwrap_or("web");
            Some(edgecrab_tools::format_web_search_status_line(
                count,
                backend,
                obj.get("fallback_from").and_then(|v| v.as_str()),
                obj.get("skipped_tool_override").and_then(|v| v.as_str()),
            ))
        }
        "web_extract" | "web_crawl" => {
            let backend = obj.get("backend").and_then(|v| v.as_str()).unwrap_or("web");
            if let Some(results) = obj.get("results").and_then(|v| v.as_array()) {
                let success = results
                    .iter()
                    .filter(|e| e.get("success").and_then(|v| v.as_bool()).unwrap_or(false))
                    .count();
                return Some(format!("{success}/{} page(s) via {backend}", results.len()));
            }
            if obj.get("result").is_some() {
                return Some(format!("1 page via {backend}"));
            }
            None
        }
        "todo" | "manage_todo_list" => {
            let summary = obj.get("summary")?.as_object()?;
            let total = summary.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            let completed = summary
                .get("completed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let in_progress = summary
                .get("in_progress")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            Some(format!(
                "{completed}/{total} done, {in_progress} in progress"
            ))
        }
        "report_task_status" => {
            let status = obj
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("in_progress");
            let summary = obj
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let remaining = obj
                .get("remaining_steps")
                .and_then(|v| v.as_array())
                .map(|steps| steps.len())
                .unwrap_or(0);
            let label = match status {
                "completed" => "completed",
                "blocked" => "blocked",
                _ => "progress",
            };
            if summary.is_empty() {
                Some(label.to_string())
            } else if remaining > 0 {
                Some(format!("{label}: {summary} · {remaining} step(s) left"))
            } else {
                Some(format!("{label}: {summary}"))
            }
        }
        "delegate_task" => {
            let results = obj.get("results")?.as_array()?;
            let completed = results
                .iter()
                .filter(|entry| {
                    matches!(
                        entry.get("status").and_then(|v| v.as_str()),
                        Some("success" | "completed")
                    )
                })
                .count();
            let duration = obj
                .get("total_duration_seconds")
                .and_then(|v| v.as_f64())
                .map(|secs| format!(" in {secs:.2}s"))
                .unwrap_or_default();
            Some(format!(
                "{completed}/{} task(s) completed{duration}",
                results.len()
            ))
        }
        _ => None,
    }
}

fn extract_tool_error_text(result: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(result)
        && let Some(err) = v.get("error").and_then(|e| e.as_str())
    {
        return err.to_string();
    }
    result.to_string()
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .nth(max_chars.saturating_sub(3))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("{}...", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_search_summary_includes_query() {
        let args = r#"{"query":"best local LLM 2026"}"#;
        let body = r#"{"results":[{"title":"a"}],"backend":"exa"}"#;
        let s = summarize_tool_result_for_history("web_search", args, body, false);
        assert!(s.contains("best local LLM 2026"));
        assert!(!s.contains("reclaimed context window space"));
    }

    #[test]
    fn terminal_summary_includes_command() {
        let args = r#"{"command":"npm test"}"#;
        let body = "line1\nline2\nexit code: 0";
        let s = summarize_tool_result_for_history("terminal", args, body, false);
        assert!(s.contains("npm test"));
        assert!(s.contains("exit"));
    }

    #[test]
    fn ha03_read_file_summary_uses_spill_source_path_when_args_empty() {
        let body = "[tool_result_spill]\nsource_path: demo/games003/index.html\nnext: read_file";
        let s = summarize_tool_result_for_history("read_file", "{}", body, false);
        assert!(s.contains("games003"));
        assert!(!s.contains("read ?"));
    }

    #[test]
    fn infer_source_path_from_spill_stub() {
        let body = "[tool_result_spill]\nsource_path: src/main.rs\nnext: read_file";
        assert_eq!(
            infer_source_path_from_spill(body).as_deref(),
            Some("src/main.rs")
        );
    }
}
