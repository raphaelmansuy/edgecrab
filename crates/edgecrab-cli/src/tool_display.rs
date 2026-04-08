//! # tool_display — Tool call display helpers
//!
//! Centralises all tool-name-aware display logic:
//! - emoji mapping (`tool_emoji`)
//! - status-bar icon (`tool_icon`)
//! - action verb (`tool_action_verb`)
//! - argument preview (`extract_tool_preview`)
//! - full ratatui span builders (`build_tool_done_line`, `build_tool_running_line`)
//! - combined status-bar preview (`tool_status_preview`)
//!
//! None of these functions depend on `App`; they are pure string/span transforms.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

// ── Internal Unicode helpers (mirrored from app.rs) ─────────────────────────

fn unicode_pad_right(s: &str, target_display_cols: usize) -> String {
    let w = s.width();
    if w >= target_display_cols {
        return s.to_string();
    }
    format!("{}{}", s, " ".repeat(target_display_cols - w))
}

fn unicode_trunc(s: &str, max_cols: usize) -> String {
    let w = s.width();
    if w <= max_cols {
        return s.to_string();
    }
    let budget = max_cols.saturating_sub(3);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
        if used + cw > budget {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push_str("...");
    out
}

fn oneline(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_args_json(args_json: &str) -> Option<serde_json::Value> {
    serde_json::from_str(args_json).ok()
}

fn args_object(args_json: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    parse_args_json(args_json)?.as_object().cloned()
}

// ── tool_emoji ───────────────────────────────────────────────────────────────

/// Map a tool name to a display emoji using keyword pattern matching.
/// Generic: no per-tool lookup table — works for any tool name.
fn tool_emoji(tool_name: &str) -> &'static str {
    let n = tool_name;
    if n.contains("search") || n.contains("grep") || n.contains("find") {
        return "🔍";
    }
    if n.contains("web") || n.contains("browser") || n.contains("navigate") || n.contains("crawl") {
        return "🌐";
    }
    if n.contains("read") {
        return "📖";
    }
    if n.contains("write") || n.contains("create") {
        return "✍️";
    }
    if n.contains("patch") || n.contains("edit") || n.contains("update") {
        return "🔧";
    }
    if n.contains("delete") || n.contains("remove") {
        return "🗑️";
    }
    if n.contains("file") {
        return "📄";
    }
    if n.contains("terminal") || n.contains("bash") || n.contains("exec") || n.contains("cmd") {
        return "💻";
    }
    if n.contains("memory") {
        return "🧠";
    }
    if n.contains("cron") || n.contains("schedule") {
        return "⏰";
    }
    if n.contains("delegate") || n.contains("agent") {
        return "🤖";
    }
    if n.contains("skill") {
        return "📚";
    }
    if n.contains("session") {
        return "🗂️";
    }
    if n.contains("todo") || n.contains("task") {
        return "📋";
    }
    if n.contains("speech") || n.contains("tts") || n.contains("audio") {
        return "🔊";
    }
    if n.contains("vision") || n.contains("image") || n.contains("photo") {
        return "👁️";
    }
    if n.contains("mcp") {
        return "◎";
    }
    if n.contains("checkpoint") {
        return "🏁";
    }
    if n.contains("clarify") {
        return "❓";
    }
    if n.contains("honcho") {
        return "🧩";
    }
    "⚙️"
}

// ── JSON preview helpers ──────────────────────────────────────────────────────

/// Render a JSON value as a short display string, collapsing whitespace.
/// Returns `None` if the value is null, an object, or empty.
fn json_value_preview(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) if !s.is_empty() => {
            let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
            Some(collapsed)
        }
        serde_json::Value::Array(arr) if !arr.is_empty() => {
            let first = arr
                .first()
                .and_then(|v| v.as_str())
                .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
                .unwrap_or_else(|| arr.len().to_string());
            let extra = if arr.len() > 1 {
                format!(" +{}", arr.len() - 1)
            } else {
                String::new()
            };
            Some(format!("{first}{extra}"))
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Generic parameter preview for any tool call.
///
/// Strategy (no per-tool lookup):
///  1. Try keys in a priority order that covers the most "meaningful" args.
///  2. Show `key: value` so the display is self-documenting for any tool.
///  3. Fall back to the first non-trivial key-value pair in the object.
fn extract_generic_preview(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    const PRIORITY: &[&str] = &[
        "query",
        "url",
        "path",
        "command",
        "action",
        "name",
        "key",
        "text",
        "content",
        "goal",
        "prompt",
        "question",
        "label",
        "tool_name",
        "code",
        "message",
        "selector",
        "input",
        "job_id",
        "skill_name",
    ];
    const SKIP: &[&str] = &[
        "timeout",
        "max_tokens",
        "temperature",
        "stream",
        "verbose",
        "debug",
        "format",
        "output_format",
    ];

    for &key in PRIORITY {
        if let Some(val) = obj.get(key) {
            if let Some(preview) = json_value_preview(val) {
                return unicode_trunc(&format!("{key}: {preview}"), 44);
            }
        }
    }

    for (key, val) in obj {
        if SKIP.contains(&key.as_str()) {
            continue;
        }
        if let Some(preview) = json_value_preview(val) {
            return unicode_trunc(&format!("{key}: {preview}"), 44);
        }
    }

    String::new()
}

pub fn tool_label(tool_name: &str) -> String {
    match tool_name {
        "web_search" => "search".into(),
        "web_extract" => "fetch".into(),
        "web_crawl" => "crawl".into(),
        "terminal" => "$".into(),
        "process" => "proc".into(),
        "read_file" => "read".into(),
        "write_file" => "write".into(),
        "patch" | "apply_patch" => "patch".into(),
        "search_files" => "grep".into(),
        "browser_navigate" => "navigate".into(),
        "browser_snapshot" => "snapshot".into(),
        "browser_click" => "click".into(),
        "browser_type" => "type".into(),
        "browser_scroll" => "scroll".into(),
        "browser_back" => "back".into(),
        "browser_press" => "press".into(),
        "browser_get_images" => "images".into(),
        "browser_vision" => "vision".into(),
        "todo" => "plan".into(),
        "session_search" => "recall".into(),
        "memory" => "memory".into(),
        "skills_list" => "skills".into(),
        "skill_view" => "skill".into(),
        "image_generate" => "create".into(),
        "text_to_speech" => "speak".into(),
        "vision_analyze" => "vision".into(),
        "mixture_of_agents" | "moa" => "reason".into(),
        "send_message" => "send".into(),
        "cronjob" | "cron" => "cron".into(),
        "execute_code" => "exec".into(),
        "delegate_task" => "delegate".into(),
        _ => tool_name.replace('_', " "),
    }
}

pub fn extract_tool_preview(tool_name: &str, args_json: &str) -> String {
    let Some(obj) = args_object(args_json) else {
        return String::new();
    };

    let preview = match tool_name {
        "web_search" => obj.get("query").and_then(|v| v.as_str()).map(oneline),
        "web_extract" => obj.get("urls").and_then(|v| match v {
            serde_json::Value::Array(urls) if !urls.is_empty() => {
                let first = urls.first()?.as_str()?;
                let domain = first
                    .trim()
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or(first);
                Some(if urls.len() > 1 {
                    format!("{domain} +{}", urls.len() - 1)
                } else {
                    domain.to_string()
                })
            }
            serde_json::Value::String(url) if !url.is_empty() => Some(
                url.trim()
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or(url)
                    .to_string(),
            ),
            _ => None,
        }),
        "web_crawl" | "browser_navigate" => obj.get("url").and_then(|v| v.as_str()).map(|url| {
            url.trim()
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or(url)
                .to_string()
        }),
        "terminal" => obj.get("command").and_then(|v| v.as_str()).map(oneline),
        "process" => {
            let action = obj
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let sid = obj
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| edgecrab_core::safe_truncate(s, 12).to_string())
                .unwrap_or_default();
            let detail = match action {
                "list" => "list".to_string(),
                "poll" | "log" | "wait" | "kill" | "write" | "submit" if !sid.is_empty() => {
                    format!("{action} {sid}")
                }
                _ if !action.is_empty() => action.to_string(),
                _ => String::new(),
            };
            (!detail.is_empty()).then_some(detail)
        }
        "todo" => match obj.get("todos") {
            None => Some("read task list".into()),
            Some(serde_json::Value::Array(items)) => Some(
                if obj.get("merge").and_then(|v| v.as_bool()).unwrap_or(false) {
                    format!("update {} task(s)", items.len())
                } else {
                    format!("{} task(s)", items.len())
                },
            ),
            _ => None,
        },
        "session_search" => obj
            .get("query")
            .and_then(|v| v.as_str())
            .map(|q| format!("\"{}\"", oneline(q))),
        "memory" => {
            let action = obj
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let target = obj
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            match action {
                "add" => obj
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|content| format!("+{target}: \"{}\"", oneline(content))),
                "replace" | "remove" => {
                    obj.get("old_text").and_then(|v| v.as_str()).map(|content| {
                        format!(
                            "{}{}: \"{}\"",
                            if action == "replace" { "~" } else { "-" },
                            target,
                            oneline(content)
                        )
                    })
                }
                _ if !action.is_empty() => Some(action.to_string()),
                _ => None,
            }
        }
        "send_message" => {
            let target = obj.get("target").and_then(|v| v.as_str()).unwrap_or("?");
            obj.get("message")
                .and_then(|v| v.as_str())
                .map(|msg| format!("{target}: \"{}\"", oneline(msg)))
        }
        "cronjob" | "cron" => {
            let action = obj
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if action == "create" {
                obj.get("name")
                    .and_then(|v| v.as_str())
                    .or_else(|| obj.get("prompt").and_then(|v| v.as_str()))
                    .map(oneline)
            } else if !action.is_empty() {
                Some(action.to_string())
            } else {
                None
            }
        }
        "execute_code" => obj
            .get("code")
            .and_then(|v| v.as_str())
            .map(|code| oneline(code.lines().next().unwrap_or_default())),
        "delegate_task" => match obj.get("tasks") {
            Some(serde_json::Value::Array(tasks)) => {
                Some(format!("{} parallel task(s)", tasks.len()))
            }
            _ => obj.get("goal").and_then(|v| v.as_str()).map(oneline),
        },
        _ => None,
    };

    preview
        .filter(|text| !text.trim().is_empty())
        .map(|text| unicode_trunc(&text, 44))
        .unwrap_or_else(|| extract_generic_preview(&obj))
}

pub fn tool_signature(tool_name: &str, args_json: &str) -> String {
    let preview = extract_tool_preview(tool_name, args_json);
    if preview.is_empty() {
        tool_name.to_ascii_lowercase()
    } else {
        format!("{}::{preview}", tool_name.to_ascii_lowercase())
    }
}

pub fn build_tool_verbose_lines(
    tool_name: &str,
    args_json: &str,
    result_preview: Option<&str>,
    is_error: bool,
) -> Vec<Vec<Span<'static>>> {
    let mut lines = Vec::new();
    let label = tool_label(tool_name);
    let args_line = unicode_trunc(&format!("args  {args_json}"), 108);
    lines.push(vec![
        Span::styled(
            "     ",
            Style::default()
                .fg(Color::Rgb(52, 56, 66))
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(
            unicode_pad_right(&label, 9),
            Style::default()
                .fg(Color::Rgb(108, 118, 138))
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(
            args_line,
            Style::default()
                .fg(Color::Rgb(120, 132, 152))
                .add_modifier(Modifier::DIM),
        ),
    ]);
    if let Some(result) = result_preview
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        lines.push(vec![
            Span::styled(
                "     ",
                Style::default()
                    .fg(Color::Rgb(52, 56, 66))
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(
                unicode_pad_right("result", 9),
                Style::default()
                    .fg(Color::Rgb(108, 118, 138))
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(
                unicode_trunc(result, 108),
                if is_error {
                    Style::default().fg(Color::Rgb(235, 170, 170))
                } else {
                    Style::default().fg(Color::Rgb(148, 198, 164))
                },
            ),
        ]);
    }
    lines
}

// ── Span builders ─────────────────────────────────────────────────────────────

/// Build a rich tool-completion display line for the output area.
///
/// Format (separate Span values so ratatui width-accounting is correct):
///   ┊ [emoji]  [tool name · 18 cols]  [key: value preview]   [timing]
pub fn build_tool_done_line(
    tool_name: &str,
    args_json: &str,
    result_preview: Option<&str>,
    duration_ms: u64,
    is_error: bool,
    emoji_overrides: &std::collections::HashMap<String, String>,
) -> Vec<Span<'static>> {
    let preview = extract_tool_preview(tool_name, args_json);
    let result_preview = result_preview.unwrap_or("").trim();

    let dur = if duration_ms >= 1000 {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    } else {
        format!("{duration_ms}ms")
    };

    let emoji: &str = if is_error {
        "❌"
    } else {
        emoji_overrides
            .get(tool_name)
            .map(|s| s.as_str())
            .unwrap_or_else(|| tool_emoji(tool_name))
    };

    let label = tool_label(tool_name);
    let name_padded = unicode_pad_right(&label, 18);

    let preview_part = if preview.is_empty() {
        String::new()
    } else {
        format!(" {preview}")
    };
    let result_part = if result_preview.is_empty() {
        String::new()
    } else {
        format!("  {}", unicode_trunc(result_preview, 54))
    };

    let bar_style = Style::default()
        .fg(Color::Rgb(60, 60, 72))
        .add_modifier(Modifier::DIM);
    let emoji_style = if is_error {
        Style::default().fg(Color::Rgb(239, 83, 80))
    } else {
        Style::default().fg(Color::Rgb(255, 191, 0))
    };
    let name_style = if is_error {
        Style::default().fg(Color::Rgb(239, 83, 80))
    } else {
        Style::default().fg(Color::Rgb(180, 190, 210))
    };
    let preview_style = Style::default()
        .fg(Color::Rgb(110, 120, 140))
        .add_modifier(Modifier::DIM);
    let result_style = if is_error {
        Style::default().fg(Color::Rgb(235, 170, 170))
    } else {
        Style::default().fg(Color::Rgb(150, 210, 170))
    };
    let dur_style = Style::default()
        .fg(Color::Rgb(90, 95, 115))
        .add_modifier(Modifier::DIM);

    vec![
        Span::styled("  ┊ ", bar_style),
        Span::styled(emoji.to_string(), emoji_style),
        Span::styled(format!(" {name_padded}"), name_style),
        Span::styled(preview_part, preview_style),
        Span::styled(result_part, result_style),
        Span::styled(format!("  {dur}"), dur_style),
    ]
}

/// Build the in-flight "running" placeholder spans for the output area.
///
/// Visual design:
///   ┊  ⌕  web search   query: "rust async"  ···
pub fn build_tool_running_line(
    tool_name: &str,
    args_json: &str,
    detail: Option<&str>,
    emoji_overrides: &std::collections::HashMap<String, String>,
) -> Vec<Span<'static>> {
    let preview = extract_tool_preview(tool_name, args_json);
    let emoji: &str = emoji_overrides
        .get(tool_name)
        .map(|s| s.as_str())
        .unwrap_or_else(|| tool_emoji(tool_name));
    let label = tool_label(tool_name);
    let name_padded = unicode_pad_right(&label, 18);
    let preview_part = if preview.is_empty() {
        String::new()
    } else {
        format!(" {preview}")
    };
    let detail_part = detail
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
        .map(|detail| format!("  {}", unicode_trunc(detail, 54)))
        .unwrap_or_default();

    let bar_style = Style::default()
        .fg(Color::Rgb(60, 60, 72))
        .add_modifier(Modifier::DIM);
    let indicator_style = Style::default().fg(Color::Rgb(77, 208, 225));
    let name_style = Style::default().fg(Color::Rgb(150, 160, 180));
    let preview_style = Style::default()
        .fg(Color::Rgb(100, 110, 130))
        .add_modifier(Modifier::DIM);
    let running_style = Style::default()
        .fg(Color::Rgb(77, 208, 225))
        .add_modifier(Modifier::DIM);

    vec![
        Span::styled("  ┊ ", bar_style),
        Span::styled(emoji.to_string(), indicator_style),
        Span::styled(format!(" {name_padded}"), name_style),
        Span::styled(preview_part, preview_style),
        Span::styled(detail_part, preview_style),
        Span::styled("  ···".to_string(), running_style),
    ]
}

/// Build a compact delegated-child progress line for the transcript.
pub fn build_subagent_event_line(
    task_index: usize,
    task_count: usize,
    label: &str,
    detail: &str,
    tone: &str,
) -> Vec<Span<'static>> {
    let badge = format!("[{}/{}]", task_index + 1, task_count);
    let (icon, icon_style, detail_style) = match tone {
        "success" => (
            "✅",
            Style::default().fg(Color::Rgb(104, 196, 129)),
            Style::default().fg(Color::Rgb(170, 210, 180)),
        ),
        "error" => (
            "❌",
            Style::default().fg(Color::Rgb(239, 83, 80)),
            Style::default().fg(Color::Rgb(235, 170, 170)),
        ),
        _ => (
            "🔀",
            Style::default().fg(Color::Rgb(95, 170, 255)),
            Style::default().fg(Color::Rgb(170, 190, 220)),
        ),
    };

    vec![
        Span::styled(
            "  │ ",
            Style::default()
                .fg(Color::Rgb(55, 60, 70))
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(format!("{icon} "), icon_style),
        Span::styled(
            format!("{} ", unicode_pad_right(&badge, 6)),
            Style::default()
                .fg(Color::Rgb(120, 130, 150))
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(
            unicode_pad_right(&unicode_trunc(label, 28), 28),
            Style::default().fg(Color::Rgb(210, 220, 235)),
        ),
        Span::styled(unicode_trunc(detail, 56), detail_style),
    ]
}

// ── Status-bar helpers ────────────────────────────────────────────────────────

/// Map a tool name to an action verb shown in the status bar during execution.
pub fn tool_action_verb(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    let n = n.as_str();
    if n.contains("search")
        || n.contains("browse")
        || n.contains("web_extract")
        || n.contains("crawl")
    {
        return "searching";
    }
    if n.contains("navigate") || n.contains("browser") {
        return "browsing";
    }
    if n.contains("terminal")
        || n.contains("exec")
        || n.contains("bash")
        || n.contains("shell")
        || n.contains("process")
    {
        return "executing";
    }
    if n.contains("read") || n.contains("cat") {
        return "reading";
    }
    if n.contains("write") || n.contains("create") {
        return "writing";
    }
    if n.contains("patch") || n.contains("edit") || n.contains("update") {
        return "patching";
    }
    if n.contains("memory") || n.contains("store") {
        return "remembering";
    }
    if n.contains("delegate") || n.contains("spawn") || n.contains("agent") {
        return "delegating";
    }
    if n.contains("vision") || n.contains("image") || n.contains("photo") {
        return "analyzing";
    }
    if n.contains("tts") || n.contains("speech") || n.contains("audio") || n.contains("transcrib") {
        return "processing";
    }
    if n.contains("mcp") {
        return "calling";
    }
    if n.contains("clarify") {
        return "asking";
    }
    if n.contains("skill") || n.contains("session_search") || n.contains("todo") {
        return "fetching";
    }
    "running"
}

/// Map a tool name to a compact monospace icon for the status bar.
pub fn tool_icon(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    let n = n.as_str();
    if n.contains("web") || n.contains("search") || n.contains("browse") {
        return "⌕";
    }
    if n.contains("terminal") || n.contains("bash") || n.contains("shell") || n.contains("run_cmd")
    {
        return "$";
    }
    if n.contains("write") || n.contains("patch") || n.contains("edit") || n.contains("create") {
        return "✎";
    }
    if n.contains("read") || n.contains("file") || n.contains("cat") {
        return "≡";
    }
    if n.contains("memory") || n.contains("store") {
        return "⊞";
    }
    if n.contains("delegate") || n.contains("spawn") || n.contains("agent") {
        return "⊛";
    }
    if n.contains("git") {
        return "⑂";
    }
    if n.contains("mcp") {
        return "◎";
    }
    if n.contains("image") || n.contains("photo") || n.contains("vision") {
        return "◫";
    }
    "⚙"
}

/// Short preview for the live status bar during tool execution.
/// Format: `tool name · key: value` (single line, truncated).
pub fn tool_status_preview(tool_name: &str, args_json: &str) -> String {
    let display_name = tool_label(tool_name);
    let preview = extract_tool_preview(tool_name, args_json);
    let full = if preview.is_empty() {
        display_name
    } else {
        format!("{display_name} · {preview}")
    };
    unicode_trunc(&full, 45)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_emoji_known_tools() {
        assert_eq!(tool_emoji("web_search"), "🔍");
        assert_eq!(tool_emoji("read_file"), "📖");
        assert_eq!(tool_emoji("write_file"), "✍️");
        assert_eq!(tool_emoji("bash_exec"), "💻");
        assert_eq!(tool_emoji("memory_store"), "🧠");
        assert_eq!(tool_emoji("mcp_call"), "◎");
        // unknown tool falls back to gear
        assert_eq!(tool_emoji("frobnicate"), "⚙️");
    }

    #[test]
    fn test_extract_tool_preview_uses_specialized_terminal_summary() {
        let preview =
            extract_tool_preview("terminal", r#"{"command":"cargo test -p edgecrab-cli"}"#);
        assert_eq!(preview, "cargo test -p edgecrab-cli");
    }

    #[test]
    fn test_tool_signature_tracks_preview_not_raw_json_order() {
        let left = tool_signature("read_file", r#"{"path":"src/main.rs","offset":0}"#);
        let right = tool_signature("read_file", r#"{"offset":25,"path":"src/main.rs"}"#);
        assert_eq!(left, right);
    }

    #[test]
    fn test_tool_emoji_search_variants() {
        assert_eq!(tool_emoji("grep_files"), "🔍");
        assert_eq!(tool_emoji("find_in_dir"), "🔍");
        assert_eq!(tool_emoji("web_browser_navigate"), "🌐");
    }

    #[test]
    fn test_extract_tool_preview_empty_json() {
        assert_eq!(extract_tool_preview("any_tool", "{}"), "");
        assert_eq!(extract_tool_preview("any_tool", "null"), "");
        assert_eq!(extract_tool_preview("any_tool", ""), "");
    }

    #[test]
    fn test_extract_tool_preview_priority_key() {
        let args = r#"{"query": "rust async", "verbose": true}"#;
        let preview = extract_tool_preview("web_search", args);
        assert!(
            !preview.contains("query:"),
            "expected concise preview in '{preview}'"
        );
        assert!(
            preview.contains("rust async"),
            "expected 'rust async' in '{preview}'"
        );
    }

    #[test]
    fn test_extract_tool_preview_fallback_key() {
        // No priority key matches — should fall back to 'result'
        let args = r#"{"result": "done"}"#;
        let preview = extract_tool_preview("unknown_tool", args);
        assert!(
            preview.contains("result:"),
            "expected 'result:' in '{preview}'"
        );
    }

    #[test]
    fn test_extract_tool_preview_truncation() {
        let long_val = "a".repeat(200);
        let args = format!(r#"{{"query": "{long_val}"}}"#);
        let preview = extract_tool_preview("web_search", &args);
        // Must not exceed 44 display cols (unicode_trunc limit in extract)
        assert!(
            preview.width() <= 44,
            "preview too wide: {}",
            preview.width()
        );
        assert!(preview.ends_with("..."), "should have ellipsis: {preview}");
    }

    #[test]
    fn test_tool_action_verb_known() {
        assert_eq!(tool_action_verb("web_search"), "searching");
        assert_eq!(tool_action_verb("bash_exec"), "executing");
        assert_eq!(tool_action_verb("read_file"), "reading");
        assert_eq!(tool_action_verb("write_file"), "writing");
        assert_eq!(tool_action_verb("mcp_call"), "calling");
        assert_eq!(tool_action_verb("unknown_op"), "running");
    }

    #[test]
    fn test_tool_icon_known() {
        assert_eq!(tool_icon("web_search"), "⌕");
        assert_eq!(tool_icon("bash_exec"), "$");
        assert_eq!(tool_icon("write_file"), "✎");
        assert_eq!(tool_icon("read_file"), "≡"); // matches "read" check
        assert_eq!(tool_icon("mcp_call"), "◎");
        assert_eq!(tool_icon("unknown_op"), "⚙"); // no pattern matches → gear fallback
    }

    #[test]
    fn test_tool_status_preview_format() {
        let args = r#"{"query": "hello"}"#;
        let preview = tool_status_preview("web_search", args);
        assert!(preview.contains("search"), "should contain tool label");
        assert!(preview.contains("hello"), "should contain query value");
    }

    #[test]
    fn test_build_tool_done_line_includes_result_preview() {
        let spans = build_tool_done_line(
            "write_file",
            r#"{"path":"src/main.rs"}"#,
            Some("Wrote 42 bytes to 'src/main.rs'"),
            250,
            false,
            &std::collections::HashMap::new(),
        );
        let joined = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(joined.contains("path: src/main.rs"));
        assert!(joined.contains("Wrote 42 bytes"));
    }

    #[test]
    fn test_unicode_trunc() {
        assert_eq!(unicode_trunc("hello", 10), "hello");
        let long = "a".repeat(50);
        let truncated = unicode_trunc(&long, 10);
        assert!(truncated.width() <= 10);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_unicode_pad_right() {
        let padded = unicode_pad_right("hi", 10);
        assert_eq!(padded.width(), 10);
        // Already at target width — no padding
        let exact = unicode_pad_right("1234567890", 10);
        assert_eq!(exact, "1234567890");
    }
}
