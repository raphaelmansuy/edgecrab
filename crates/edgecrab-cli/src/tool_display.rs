//! # tool_display — Tool call display helpers
//!
//! Centralises all tool-name-aware display logic:
//! - emoji mapping (`tool_emoji`)
//! - status-bar icon (`tool_icon`)
//! - action verb (`tool_action_verb`)
//! - argument preview (`extract_tool_preview`)
//! - full ratatui span builders (`build_tool_done_line`, `build_tool_running_line`)
//! - combined status-bar preview (`tool_status_preview`)
//! - `DisplayWidths` — single source of truth for column budgets
//!
//! None of these functions depend on `App`; they are pure string/span transforms.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

// ── DisplayWidths — single source of truth for column budgets ───────────────

/// Column budget calculator that adapts all display widths to the terminal size.
///
/// Follows a proportional allocation scheme:
/// - **name**: fixed 12–18 cols (capped)
/// - **preview**: ~30% of remaining width after chrome
/// - **result**: ~35% of remaining width after chrome
/// - **duration**: fixed 8 cols
/// - **chrome** (bar, emoji, spaces): fixed 6 cols
#[derive(Debug, Clone, Copy)]
pub struct DisplayWidths {
    /// Tool label column width (e.g. "search", "write")
    pub name: usize,
    /// Argument preview column width
    pub preview: usize,
    /// Result preview column width
    pub result: usize,
    /// Verbose-mode content width (args/result lines)
    pub verbose_content: usize,
    /// Status bar tool preview width
    pub status_preview: usize,
}

impl DisplayWidths {
    /// Default widths for an 80-column terminal (backward-compatible).
    pub const DEFAULT: Self = Self {
        name: 18,
        preview: 44,
        result: 52,
        verbose_content: 108,
        status_preview: 45,
    };

    /// Compute proportional column budgets from terminal width.
    pub fn from_terminal_width(w: usize) -> Self {
        if w <= 80 {
            return Self::DEFAULT;
        }

        // Fixed chrome per line: "  ┊ " (4) + emoji (3) + name (18) + "-> " (3) + duration (8) = 36
        const FIXED_CHROME: usize = 36;
        let content_budget = w.saturating_sub(FIXED_CHROME);

        // Split content budget between preview (46%) and result (54%)
        // — matches the 44:52 ratio of DEFAULT at 80 cols.
        let preview = (content_budget * 46 / 100).max(20);
        let result = (content_budget * 54 / 100).max(20);

        let name = 18usize;

        // Verbose: nearly full width minus indentation (14 cols for "     label    ")
        let verbose_content = w.saturating_sub(14).max(40);

        // Status bar preview: ~40% of terminal width, capped
        let status_preview = (w * 40 / 100).clamp(30, 80);

        Self {
            name,
            preview,
            result,
            verbose_content,
            status_preview,
        }
    }
}

// ── Tool category — semantic grouping for display colors ─────────────────────

/// Semantic category for a tool name used to select display colors.
///
/// Having distinct color families lets users instantly distinguish file edits
/// from shell commands, web fetches from memory writes, etc. — without reading
/// the label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Search,
    WebBrowser,
    FileRead,
    FileWrite,
    Terminal,
    Memory,
    Plan,
    Ai,
    Mcp,
    Ha,
    Other,
}

impl ToolCategory {
    /// Foreground color for the tool name column (done and running lines).
    pub fn name_color(self) -> Color {
        match self {
            ToolCategory::Search => Color::Rgb(80, 210, 230), // cyan
            ToolCategory::WebBrowser => Color::Rgb(64, 188, 212), // teal
            ToolCategory::FileRead => Color::Rgb(150, 165, 195), // slate
            ToolCategory::FileWrite => Color::Rgb(255, 185, 50), // amber
            ToolCategory::Terminal => Color::Rgb(255, 145, 60), // orange
            ToolCategory::Memory => Color::Rgb(110, 195, 135), // sage green
            ToolCategory::Plan => Color::Rgb(140, 170, 255),  // periwinkle
            ToolCategory::Ai => Color::Rgb(185, 145, 240),    // violet
            ToolCategory::Mcp => Color::Rgb(130, 165, 210),   // steel blue
            ToolCategory::Ha => Color::Rgb(100, 195, 145),    // green
            ToolCategory::Other => Color::Rgb(170, 180, 205), // gray
        }
    }
}

/// Classify a tool name into a display category for color selection.
pub fn tool_category(name: &str) -> ToolCategory {
    if name == "web_search" || name == "search_files" || name == "session_search" {
        return ToolCategory::Search;
    }
    if name.starts_with("web_") || name.starts_with("browser_") {
        return ToolCategory::WebBrowser;
    }
    if name == "read_file" {
        return ToolCategory::FileRead;
    }
    if name == "write_file"
        || name == "patch"
        || name == "apply_patch"
        || name.contains("delete")
        || name.contains("move_file")
    {
        return ToolCategory::FileWrite;
    }
    if name == "terminal" || name == "execute_code" || name.starts_with("process") {
        return ToolCategory::Terminal;
    }
    if name == "memory" {
        return ToolCategory::Memory;
    }
    if name == "todo" || name == "manage_todo_list" {
        return ToolCategory::Plan;
    }
    if name.contains("vision")
        || name.contains("tts")
        || name.contains("speech")
        || name.contains("transcrib")
        || name.contains("image")
        || name == "delegate_task"
    {
        return ToolCategory::Ai;
    }
    if name.starts_with("mcp_") {
        return ToolCategory::Mcp;
    }
    if name.starts_with("ha_") {
        return ToolCategory::Ha;
    }
    ToolCategory::Other
}

// ── Duration formatting ───────────────────────────────────────────────────────

/// Format a completion duration for the tool line timing column.
///
/// Targets a consistent 5-character display width so timing glances are fast:
/// `"  1ms"`, `"999ms"`, `" 1.0s"`, `"10.0s"`, `"1m05s"`
fn format_duration_aligned(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms:>3}ms") // "  1ms" .. "999ms" (5 chars)
    } else if ms < 10_000 {
        format!(" {:.1}s", ms as f64 / 1000.0) // " 1.0s" (5 chars)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0) // "10.0s" (5 chars)
    } else {
        let secs = ms / 1000;
        let mins = secs / 60;
        format!("{mins}m{:02}s", secs % 60) // "1m05s" (5 chars)
    }
}

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

fn quoted_preview(text: &str, max_cols: usize) -> String {
    unicode_trunc(&format!("\"{}\"", oneline(text)), max_cols)
}

fn url_domain(url: &str) -> String {
    url.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

fn extract_patch_targets(patch_text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for line in patch_text.lines() {
        let path = line
            .strip_prefix("*** Update File: ")
            .or_else(|| line.strip_prefix("*** Add File: "))
            .or_else(|| line.strip_prefix("*** Delete File: "))
            .or_else(|| line.strip_prefix("*** Move to: "))
            .map(str::trim);
        if let Some(path) = path.filter(|path| !path.is_empty()) {
            if !targets.iter().any(|existing| existing == path) {
                targets.push(path.to_string());
            }
        }
    }
    targets
}

fn line_range_suffix(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let line_start = obj.get("line_start").and_then(|v| v.as_u64());
    let line_end = obj.get("line_end").and_then(|v| v.as_u64());
    match (line_start, line_end) {
        (Some(start), Some(end)) if start == end => format!(":{start}"),
        (Some(start), Some(end)) => format!(":{start}-{end}"),
        (Some(start), None) => format!(":{start}"),
        _ => String::new(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TodoDisplayItem {
    id: Option<String>,
    title: String,
    status: String,
}

fn normalize_todo_status(status: &str) -> String {
    match status.trim() {
        "pending" => "not-started".into(),
        "in_progress" => "in-progress".into(),
        "not-started" | "in-progress" | "completed" | "cancelled" => status.trim().into(),
        _ => "not-started".into(),
    }
}

fn extract_todo_items(obj: &serde_json::Map<String, serde_json::Value>) -> Vec<TodoDisplayItem> {
    let Some(items) = obj
        .get("items")
        .or_else(|| obj.get("todos"))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|item| {
            let item = item.as_object()?;
            let title = item
                .get("title")
                .or_else(|| item.get("content"))
                .and_then(|v| v.as_str())
                .map(oneline)
                .filter(|text| !text.is_empty())?;
            let id = item
                .get("id")
                .and_then(|v| match v {
                    serde_json::Value::String(text) => Some(text.clone()),
                    serde_json::Value::Number(num) => Some(num.to_string()),
                    _ => None,
                })
                .filter(|text| !text.is_empty());
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .map(normalize_todo_status)
                .unwrap_or_else(|| "not-started".into());
            Some(TodoDisplayItem { id, title, status })
        })
        .collect()
}

fn todo_preview(obj: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let items = extract_todo_items(obj);
    if items.is_empty() {
        return Some("review current plan".into());
    }

    let action = if obj.get("merge").and_then(|v| v.as_bool()).unwrap_or(false) {
        "update"
    } else {
        "set"
    };
    let first = unicode_trunc(&items[0].title, 20);
    Some(if items.len() == 1 {
        format!("{action} 1 task · {first}")
    } else {
        format!(
            "{action} {} tasks · {first} +{}",
            items.len(),
            items.len() - 1
        )
    })
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
fn extract_generic_preview(
    obj: &serde_json::Map<String, serde_json::Value>,
    max_cols: usize,
) -> String {
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
                return unicode_trunc(&format!("{key}: {preview}"), max_cols);
            }
        }
    }

    for (key, val) in obj {
        if SKIP.contains(&key.as_str()) {
            continue;
        }
        if let Some(preview) = json_value_preview(val) {
            return unicode_trunc(&format!("{key}: {preview}"), max_cols);
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
        "process" | "process_start" | "process_list" | "process_kill" | "process_wait"
        | "process_logs" | "process_write" => "proc".into(),
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
        "browser_console" => "console".into(),
        "browser_wait_for" => "wait".into(),
        "browser_select" => "select".into(),
        "browser_hover" => "hover".into(),
        "browser_close" => "close".into(),
        "todo" | "manage_todo_list" => "plan".into(),
        "session_search" => "recall".into(),
        "memory" => "memory".into(),
        "skills_list" => "skills".into(),
        "skill_view" => "skill".into(),
        "generate_image" | "image_generate" => "create".into(),
        "text_to_speech" => "speak".into(),
        "transcribe_audio" => "transcribe".into(),
        "vision_analyze" => "vision".into(),
        "mixture_of_agents" | "moa" => "reason".into(),
        "send_message" => "send".into(),
        "cronjob" | "cron" => "cron".into(),
        "execute_code" => "exec".into(),
        "delegate_task" => "delegate".into(),
        "clarify" => "clarify".into(),
        "checkpoint" => "checkpoint".into(),
        "pdf_to_markdown" => "pdf".into(),
        "ha_list_entities" => "ha entities".into(),
        "ha_get_state" => "ha state".into(),
        "ha_list_services" => "ha services".into(),
        "ha_call_service" => "ha call".into(),
        "honcho_conclude" => "honcho save".into(),
        "honcho_search" => "honcho search".into(),
        "honcho_list" => "honcho list".into(),
        "honcho_remove" => "honcho remove".into(),
        "honcho_profile" => "honcho profile".into(),
        "honcho_context" => "honcho ask".into(),
        "mcp_list_tools" => "mcp tools".into(),
        "mcp_call_tool" => "mcp call".into(),
        "mcp_list_resources" => "mcp resources".into(),
        "mcp_read_resource" => "mcp read".into(),
        "mcp_list_prompts" => "mcp prompts".into(),
        "mcp_get_prompt" => "mcp prompt".into(),
        _ => tool_name.replace('_', " "),
    }
}

pub fn extract_tool_preview(tool_name: &str, args_json: &str) -> String {
    extract_tool_preview_width(tool_name, args_json, DisplayWidths::DEFAULT.preview)
}

pub fn extract_tool_preview_width(
    tool_name: &str,
    args_json: &str,
    max_preview_cols: usize,
) -> String {
    let Some(obj) = args_object(args_json) else {
        return String::new();
    };

    let preview = match tool_name {
        "web_search" => obj.get("query").and_then(|v| v.as_str()).map(oneline),
        "web_extract" => obj.get("urls").and_then(|v| match v {
            serde_json::Value::Array(urls) if !urls.is_empty() => {
                let first = urls.first()?.as_str()?;
                let domain = url_domain(first);
                Some(if urls.len() > 1 {
                    format!("{domain} +{}", urls.len() - 1)
                } else {
                    domain
                })
            }
            serde_json::Value::String(url) if !url.is_empty() => Some(url_domain(url)),
            _ => None,
        }),
        "web_crawl" | "browser_navigate" => obj.get("url").and_then(|v| v.as_str()).map(url_domain),
        "terminal" => obj.get("command").and_then(|v| v.as_str()).map(oneline),
        "read_file" => obj.get("path").and_then(|v| v.as_str()).map(|path| {
            let suffix = line_range_suffix(&obj);
            format!("{path}{suffix}")
        }),
        "write_file" => obj.get("path").and_then(|v| v.as_str()).map(oneline),
        "patch" => obj
            .get("path")
            .and_then(|v| v.as_str())
            .map(oneline)
            .or_else(|| {
                obj.get("patch")
                    .and_then(|v| v.as_str())
                    .map(extract_patch_targets)
                    .filter(|targets| !targets.is_empty())
                    .map(|targets| {
                        if targets.len() == 1 {
                            targets[0].clone()
                        } else {
                            format!("{} file(s): {}", targets.len(), targets[0])
                        }
                    })
            }),
        "apply_patch" => obj
            .get("patch")
            .and_then(|v| v.as_str())
            .map(extract_patch_targets)
            .filter(|targets| !targets.is_empty())
            .map(|targets| {
                if targets.len() == 1 {
                    targets[0].clone()
                } else {
                    format!("{} file(s): {}", targets.len(), targets[0])
                }
            }),
        "search_files" => {
            let Some(pattern) = obj.get("pattern").and_then(|v| v.as_str()).map(oneline) else {
                return String::new();
            };
            let target = obj
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("content");
            let include = obj
                .get("include")
                .or_else(|| obj.get("file_glob"))
                .and_then(|v| v.as_str())
                .filter(|value| !value.is_empty());
            Some(match include {
                Some(glob) if target == "files" => format!("{pattern} in {glob}"),
                Some(glob) => format!("{pattern} @ {glob}"),
                None => pattern,
            })
        }
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
        "todo" | "manage_todo_list" => todo_preview(&obj),
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
                .map(|msg| format!("{target}: {}", quoted_preview(msg, 34)))
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
        "browser_snapshot" => Some(
            if obj.get("full").and_then(|v| v.as_bool()).unwrap_or(false) {
                "full page".into()
            } else {
                "interactive view".into()
            },
        ),
        "browser_click" | "browser_hover" => obj.get("ref").and_then(|v| v.as_str()).map(oneline),
        "browser_type" => {
            let target = obj.get("ref").and_then(|v| v.as_str()).unwrap_or("?");
            obj.get("text")
                .and_then(|v| v.as_str())
                .map(|text| format!("{target} {}", quoted_preview(text, 28)))
        }
        "browser_scroll" => obj
            .get("direction")
            .and_then(|v| v.as_str())
            .map(|direction| {
                let amount = obj.get("amount").and_then(|v| v.as_u64()).unwrap_or(500);
                format!("{direction} {amount}px")
            }),
        "browser_press" => obj.get("key").and_then(|v| v.as_str()).map(oneline),
        "browser_console" => Some(
            if obj.get("clear").and_then(|v| v.as_bool()).unwrap_or(false) {
                "read + clear".into()
            } else {
                "read".into()
            },
        ),
        "browser_get_images" => Some("extract page images".into()),
        "browser_vision" => obj
            .get("question")
            .and_then(|v| v.as_str())
            .map(|question| quoted_preview(question, 40))
            .or_else(|| Some("analyze page".into())),
        "browser_wait_for" => obj
            .get("text")
            .and_then(|v| v.as_str())
            .map(|text| format!("text {}", quoted_preview(text, 28)))
            .or_else(|| {
                obj.get("selector")
                    .and_then(|v| v.as_str())
                    .map(|selector| format!("selector {selector}"))
            }),
        "browser_select" => {
            let target = obj.get("ref").and_then(|v| v.as_str()).unwrap_or("?");
            obj.get("option")
                .and_then(|v| v.as_str())
                .map(|option| format!("{target} -> {}", quoted_preview(option, 28)))
        }
        "generate_image" | "image_generate" => obj
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(|prompt| quoted_preview(prompt, 44)),
        "text_to_speech" => obj
            .get("text")
            .and_then(|v| v.as_str())
            .map(|text| quoted_preview(text, 40)),
        "transcribe_audio" => obj
            .get("audio_path")
            .or_else(|| obj.get("path"))
            .and_then(|v| v.as_str())
            .map(oneline),
        "vision_analyze" => obj
            .get("prompt")
            .or_else(|| obj.get("question"))
            .and_then(|v| v.as_str())
            .map(|question| quoted_preview(question, 40))
            .or_else(|| {
                obj.get("image_source")
                    .and_then(|v| v.as_str())
                    .map(oneline)
            }),
        "delegate_task" => match obj.get("tasks") {
            Some(serde_json::Value::Array(tasks)) => {
                Some(format!("{} parallel task(s)", tasks.len()))
            }
            _ => obj.get("goal").and_then(|v| v.as_str()).map(oneline),
        },
        "clarify" => obj
            .get("question")
            .and_then(|v| v.as_str())
            .map(|question| quoted_preview(question, 40)),
        "checkpoint" => {
            let action = obj
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let name = obj
                .get("name")
                .or_else(|| obj.get("checkpoint"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            match (action.is_empty(), name.is_empty()) {
                (false, false) => Some(format!("{action} {name}")),
                (false, true) => Some(action.to_string()),
                (true, false) => Some(name.to_string()),
                (true, true) => None,
            }
        }
        "pdf_to_markdown" => obj.get("path").and_then(|v| v.as_str()).map(oneline),
        "ha_list_entities" => obj
            .get("domain")
            .and_then(|v| v.as_str())
            .map(oneline)
            .or_else(|| Some("all domains".into())),
        "ha_get_state" => obj.get("entity_id").and_then(|v| v.as_str()).map(oneline),
        "ha_list_services" => obj
            .get("domain")
            .and_then(|v| v.as_str())
            .map(oneline)
            .or_else(|| Some("all domains".into())),
        "ha_call_service" => {
            let domain = obj.get("domain").and_then(|v| v.as_str()).unwrap_or("?");
            let service = obj.get("service").and_then(|v| v.as_str()).unwrap_or("?");
            Some(format!("{domain}.{service}"))
        }
        "honcho_conclude" => obj
            .get("entry")
            .and_then(|v| v.as_str())
            .map(|entry| quoted_preview(entry, 40)),
        "honcho_search" | "honcho_context" => obj
            .get("query")
            .or_else(|| obj.get("question"))
            .and_then(|v| v.as_str())
            .map(|query| quoted_preview(query, 40)),
        "honcho_remove" => obj.get("id").and_then(|v| v.as_str()).map(oneline),
        "honcho_profile" | "honcho_list" => Some("read".into()),
        "mcp_list_tools" | "mcp_list_resources" | "mcp_list_prompts" => obj
            .get("server")
            .or_else(|| obj.get("server_name"))
            .and_then(|v| v.as_str())
            .map(oneline),
        "mcp_call_tool" => {
            let server = obj
                .get("server")
                .or_else(|| obj.get("server_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let tool = obj
                .get("tool")
                .or_else(|| obj.get("tool_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            Some(format!("{server}/{tool}"))
        }
        "mcp_read_resource" => obj
            .get("uri")
            .or_else(|| obj.get("resource"))
            .and_then(|v| v.as_str())
            .map(oneline),
        "mcp_get_prompt" => obj
            .get("prompt")
            .or_else(|| obj.get("prompt_name"))
            .and_then(|v| v.as_str())
            .map(oneline),
        _ => None,
    };

    preview
        .filter(|text| !text.trim().is_empty())
        .map(|text| unicode_trunc(&text, max_preview_cols))
        .unwrap_or_else(|| extract_generic_preview(&obj, max_preview_cols))
}

pub fn tool_signature(tool_name: &str, args_json: &str) -> String {
    let preview = extract_tool_preview(tool_name, args_json);
    if preview.is_empty() {
        tool_name.to_ascii_lowercase()
    } else {
        format!("{}::{preview}", tool_name.to_ascii_lowercase())
    }
}

#[allow(dead_code)]
pub fn build_tool_verbose_lines(
    tool_name: &str,
    args_json: &str,
    result_preview: Option<&str>,
    is_error: bool,
) -> Vec<Vec<Span<'static>>> {
    build_tool_verbose_lines_width(
        tool_name,
        args_json,
        result_preview,
        is_error,
        DisplayWidths::DEFAULT.verbose_content,
    )
}

// ── Verbose-mode helpers ─────────────────────────────────────────────────────

/// Build a structured one-line summary for verbose-mode args display.
///
/// Replaces the previous raw JSON dump (`"args  {json}"`) with a human-readable
/// preview using the same rich tool-aware extract logic as the compact display.
/// Falls back to top-3 key-value pairs for unknown tools, never to raw JSON.
fn build_verbose_args_summary(tool_name: &str, args_json: &str, max_width: usize) -> String {
    // Prefer the rich tool-aware preview (same as compact line but with more budget).
    let preview = extract_tool_preview_width(tool_name, args_json, max_width.saturating_sub(2));
    if !preview.is_empty() {
        return preview;
    }
    // Fallback: render top-3 key-value pairs (skip known large-payload keys).
    const SKIP_LARGE: &[&str] = &["content", "code", "patch", "data", "body", "html"];
    if let Some(obj) = args_object(args_json) {
        let pairs: Vec<String> = obj
            .iter()
            .filter(|(k, _)| !SKIP_LARGE.contains(&k.as_str()))
            .take(3)
            .filter_map(|(k, v)| {
                json_value_preview(v).map(|pv| format!("{k}: {}", unicode_trunc(&pv, 28)))
            })
            .collect();
        if !pairs.is_empty() {
            return unicode_trunc(&pairs.join(" · "), max_width);
        }
    }
    String::new()
}

/// Build a content-stat hint for verbose mode when a tool carries large payload.
/// Returns `None` for tools without notable content.
fn build_verbose_content_stat(tool_name: &str, args_json: &str) -> Option<String> {
    let obj = args_object(args_json)?;
    match tool_name {
        "write_file" => {
            let content = obj.get("content").and_then(|v| v.as_str())?;
            let line_count = content.lines().count();
            let bytes = content.len();
            let size = if bytes >= 1024 {
                format!("{:.1}k", bytes as f64 / 1024.0)
            } else {
                format!("{bytes}b")
            };
            Some(format!("content: <{line_count} lines, {size}>"))
        }
        "apply_patch" | "patch" => {
            let patch = obj.get("patch").and_then(|v| v.as_str())?;
            let targets = extract_patch_targets(patch);
            let total = targets.len();
            if total == 0 {
                return None;
            }
            let adds: usize = patch.lines().filter(|l| l.starts_with("+++")).count();
            let dels: usize = patch.lines().filter(|l| l.starts_with("---")).count();
            Some(format!("patch: {total} file(s) · +{adds} −{dels} hunks"))
        }
        "terminal" | "execute_code" => {
            let is_bg = obj
                .get("background")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let timeout = obj.get("timeout").and_then(|v| v.as_u64());
            let mut parts: Vec<String> = Vec::new();
            if is_bg {
                parts.push("background".into());
            }
            if let Some(t) = timeout {
                parts.push(format!("timeout: {t}s"));
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" · "))
            }
        }
        _ => None,
    }
}

pub fn build_tool_verbose_lines_width(
    tool_name: &str,
    args_json: &str,
    result_preview: Option<&str>,
    is_error: bool,
    verbose_width: usize,
) -> Vec<Vec<Span<'static>>> {
    if matches!(tool_name, "todo" | "manage_todo_list") {
        return build_todo_verbose_lines(args_json, result_preview, is_error, verbose_width);
    }

    let mut lines = Vec::new();
    let category = tool_category(tool_name);
    let label = unicode_trunc(&tool_label(tool_name), 9);
    let indent = Span::styled(
        "     ",
        Style::default()
            .fg(Color::Rgb(48, 52, 62))
            .add_modifier(Modifier::DIM),
    );
    let label_style = Style::default()
        .fg(category.name_color())
        .add_modifier(Modifier::DIM);

    // Structured summary — no more raw JSON dumps.
    let args_summary = build_verbose_args_summary(tool_name, args_json, verbose_width);
    lines.push(vec![
        indent.clone(),
        Span::styled(unicode_pad_right(&label, 9), label_style),
        Span::styled(
            args_summary,
            Style::default()
                .fg(Color::Rgb(115, 128, 150))
                .add_modifier(Modifier::DIM),
        ),
    ]);

    // For tools with large payload, add a content-stat hint line.
    if let Some(stat) = build_verbose_content_stat(tool_name, args_json) {
        lines.push(vec![
            indent.clone(),
            Span::styled(
                unicode_pad_right("", 9),
                Style::default()
                    .fg(Color::Rgb(80, 92, 112))
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(
                unicode_trunc(&stat, verbose_width),
                Style::default()
                    .fg(Color::Rgb(90, 102, 125))
                    .add_modifier(Modifier::DIM),
            ),
        ]);
    }

    if let Some(result) = result_preview
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        // Use the same rich-result formatter as the compact done-line so
        // verbose mode benefits from per-tool formatting with a wider budget.
        let rich = format_tool_result(tool_name, result, verbose_width);
        let display = if rich.is_empty() {
            unicode_trunc(result, verbose_width)
        } else {
            rich
        };
        lines.push(vec![
            indent,
            Span::styled(
                unicode_pad_right("result", 9),
                Style::default()
                    .fg(Color::Rgb(100, 112, 135))
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(
                display,
                if is_error {
                    Style::default()
                        .fg(Color::Rgb(255, 120, 120))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(148, 208, 168))
                },
            ),
        ]);
    }
    lines
}

fn build_todo_verbose_lines(
    args_json: &str,
    result_preview: Option<&str>,
    is_error: bool,
    verbose_width: usize,
) -> Vec<Vec<Span<'static>>> {
    let mut lines = Vec::new();
    let obj = args_object(args_json).unwrap_or_default();
    let items = extract_todo_items(&obj);
    let mode = if obj.get("merge").and_then(|v| v.as_bool()).unwrap_or(false) {
        "merge update"
    } else if items.is_empty() {
        "read current plan"
    } else {
        "replace plan"
    };
    let summary = result_preview
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(mode);

    lines.push(vec![
        Span::styled(
            "     ",
            Style::default()
                .fg(Color::Rgb(52, 56, 66))
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(
            unicode_pad_right("plan", 9),
            Style::default()
                .fg(Color::Rgb(108, 118, 138))
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(
            unicode_trunc(summary, verbose_width),
            if is_error {
                Style::default().fg(Color::Rgb(235, 170, 170))
            } else {
                Style::default().fg(Color::Rgb(156, 208, 188))
            },
        ),
    ]);

    for item in items.iter().take(6) {
        let (badge, badge_style, text_style) = match item.status.as_str() {
            "completed" => (
                "[x]",
                Style::default().fg(Color::Rgb(104, 196, 129)),
                Style::default().fg(Color::Rgb(168, 215, 184)),
            ),
            "in-progress" => (
                "[>]",
                Style::default().fg(Color::Rgb(77, 208, 225)),
                Style::default().fg(Color::Rgb(176, 220, 236)),
            ),
            "cancelled" => (
                "[-]",
                Style::default().fg(Color::Rgb(220, 120, 120)),
                Style::default()
                    .fg(Color::Rgb(175, 145, 145))
                    .add_modifier(Modifier::DIM),
            ),
            _ => (
                "[ ]",
                Style::default().fg(Color::Rgb(150, 160, 180)),
                Style::default().fg(Color::Rgb(205, 212, 224)),
            ),
        };
        let title = if let Some(id) = &item.id {
            format!("{id}. {}", item.title)
        } else {
            item.title.clone()
        };
        lines.push(vec![
            Span::styled(
                "     ",
                Style::default()
                    .fg(Color::Rgb(52, 56, 66))
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(format!("{badge} "), badge_style),
            Span::styled(
                unicode_trunc(&title, verbose_width.saturating_sub(16)),
                text_style,
            ),
        ]);
    }

    if items.len() > 6 {
        lines.push(vec![
            Span::styled(
                "     ",
                Style::default()
                    .fg(Color::Rgb(52, 56, 66))
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled(
                format!("+{} more task(s)", items.len() - 6),
                Style::default()
                    .fg(Color::Rgb(120, 132, 152))
                    .add_modifier(Modifier::DIM),
            ),
        ]);
    }

    lines
}

// ── Span builders ─────────────────────────────────────────────────────────────

/// Build a rich tool-completion display line for the output area.
///
/// Format (separate Span values so ratatui width-accounting is correct):
///   ┊ [emoji]  [tool name · name_w cols]  [key: value preview]   [timing]
///
/// Column widths adapt to `widths` (computed from terminal width).
#[allow(dead_code)]
pub fn build_tool_done_line(
    tool_name: &str,
    args_json: &str,
    result_preview: Option<&str>,
    duration_ms: u64,
    is_error: bool,
    emoji_overrides: &std::collections::HashMap<String, String>,
) -> Vec<Span<'static>> {
    build_tool_done_line_width(
        tool_name,
        args_json,
        result_preview,
        duration_ms,
        is_error,
        emoji_overrides,
        &DisplayWidths::DEFAULT,
    )
}

pub fn build_tool_done_line_width(
    tool_name: &str,
    args_json: &str,
    result_preview: Option<&str>,
    duration_ms: u64,
    is_error: bool,
    emoji_overrides: &std::collections::HashMap<String, String>,
    widths: &DisplayWidths,
) -> Vec<Span<'static>> {
    let preview = extract_tool_preview_width(tool_name, args_json, widths.preview);
    let result_preview = result_preview.unwrap_or("").trim();

    let dur = format_duration_aligned(duration_ms);

    let emoji: &str = if is_error {
        "❌"
    } else {
        emoji_overrides
            .get(tool_name)
            .map(|s| s.as_str())
            .unwrap_or_else(|| tool_emoji(tool_name))
    };

    // FIX: truncate label before padding so names never blow the column budget.
    let label = unicode_trunc(&tool_label(tool_name), widths.name);
    let name_padded = unicode_pad_right(&label, widths.name);

    // FIX: right-pad preview to a fixed column width so the result separator
    // always appears at the same horizontal position on every tool line.
    let preview_col = format!(" {}", unicode_pad_right(&preview, widths.preview));

    // Apply per-tool rich formatting so the result column shows the most useful
    // signal (exit code, item count, title…) rather than a raw truncated string.
    let rich_result = if result_preview.is_empty() {
        String::new()
    } else {
        format_tool_result(tool_name, result_preview, widths.result)
    };
    let result_part = if rich_result.is_empty() {
        String::new()
    } else {
        format!("  {} {}", if is_error { "✗" } else { "→" }, rich_result)
    };

    // Semantic colors: each tool category gets a distinct hue so users can
    // visually distinguish file edits, terminal commands, web searches, etc.
    let category = tool_category(tool_name);
    let bar_style = Style::default()
        .fg(Color::Rgb(55, 58, 70))
        .add_modifier(Modifier::DIM);
    let emoji_style = if is_error {
        Style::default()
            .fg(Color::Rgb(255, 80, 80))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(category.name_color())
    };
    let name_style = if is_error {
        Style::default().fg(Color::Rgb(255, 105, 105))
    } else {
        Style::default().fg(category.name_color())
    };
    let preview_style = Style::default()
        .fg(Color::Rgb(90, 102, 125))
        .add_modifier(Modifier::DIM);
    let result_style = if is_error {
        Style::default()
            .fg(Color::Rgb(255, 125, 125))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(148, 208, 168))
    };
    let dur_style = Style::default()
        .fg(Color::Rgb(72, 79, 98))
        .add_modifier(Modifier::DIM);

    vec![
        Span::styled("  ┊ ", bar_style),
        Span::styled(emoji.to_string(), emoji_style),
        Span::styled(format!(" {name_padded}"), name_style),
        Span::styled(preview_col, preview_style),
        Span::styled(result_part, result_style),
        Span::styled(format!("  {dur}"), dur_style),
    ]
}

/// Build the in-flight "running" placeholder spans for the output area.
///
/// Visual design:
///   ┊  ⌕  web search   query: "rust async"  ···
#[allow(dead_code)]
pub fn build_tool_running_line(
    tool_name: &str,
    args_json: &str,
    detail: Option<&str>,
    emoji_overrides: &std::collections::HashMap<String, String>,
) -> Vec<Span<'static>> {
    build_tool_running_line_width(
        tool_name,
        args_json,
        detail,
        emoji_overrides,
        &DisplayWidths::DEFAULT,
    )
}

pub fn build_tool_running_line_width(
    tool_name: &str,
    args_json: &str,
    detail: Option<&str>,
    emoji_overrides: &std::collections::HashMap<String, String>,
    widths: &DisplayWidths,
) -> Vec<Span<'static>> {
    build_tool_running_line_width_elapsed(tool_name, args_json, detail, None, emoji_overrides, widths)
}

/// Like `build_tool_running_line_width` but also shows elapsed time in the placeholder.
///
/// FP49: After 3s, the `  ···` tail becomes `  ···  Xs` so the output-area placeholder
/// provides temporal feedback without requiring the user to look at the status bar.
pub fn build_tool_running_line_width_elapsed(
    tool_name: &str,
    args_json: &str,
    detail: Option<&str>,
    elapsed_secs: Option<u64>,
    emoji_overrides: &std::collections::HashMap<String, String>,
    widths: &DisplayWidths,
) -> Vec<Span<'static>> {
    let preview = extract_tool_preview_width(tool_name, args_json, widths.preview);
    let emoji: &str = emoji_overrides
        .get(tool_name)
        .map(|s| s.as_str())
        .unwrap_or_else(|| tool_emoji(tool_name));
    let category = tool_category(tool_name);
    // FIX: truncate label + right-pad preview for column alignment (mirrors done-line).
    let label = unicode_trunc(&tool_label(tool_name), widths.name);
    let name_padded = unicode_pad_right(&label, widths.name);
    let preview_col = format!(" {}", unicode_pad_right(&preview, widths.preview));
    let detail_width = widths.result.max(20);
    let detail_part = detail
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
        .map(|detail| format!("  {}", unicode_trunc(detail, detail_width)))
        .unwrap_or_default();

    // FP49: Show elapsed time in the running placeholder after 3s.
    let elapsed_part = match elapsed_secs {
        Some(secs) if secs >= 3 => format!("  {secs}s"),
        _ => String::new(),
    };

    let bar_style = Style::default()
        .fg(Color::Rgb(55, 58, 70))
        .add_modifier(Modifier::DIM);
    let indicator_style = Style::default().fg(category.name_color());
    let name_style = Style::default().fg(category.name_color());
    let preview_style = Style::default()
        .fg(Color::Rgb(90, 102, 125))
        .add_modifier(Modifier::DIM);
    let running_style = Style::default()
        .fg(category.name_color())
        .add_modifier(Modifier::DIM);
    let elapsed_style = Style::default()
        .fg(Color::Rgb(100, 112, 135))
        .add_modifier(Modifier::DIM);

    vec![
        Span::styled("  ┊ ", bar_style),
        Span::styled(emoji.to_string(), indicator_style),
        Span::styled(format!(" {name_padded}"), name_style),
        Span::styled(preview_col, preview_style),
        Span::styled(detail_part, preview_style),
        Span::styled("  ···".to_string(), running_style),
        Span::styled(elapsed_part, elapsed_style),
    ]
}

// ── Sub-agent display lines ───────────────────────────────────────────────────
//
// Visual contract (mirrors build_tool_done_line_width):
//
//   ┊ →  [1/3]  goal preview...            write_file src/...    3.2s ···
//   ┊ ✅  [1/3]  River poem (first line…)  · 1 call  gpt-5-mi  11.9s
//   ┊ ❌  [2/3]  Error: context exceeded   · 3 calls claude-3   4.2s
//
// All columns use the same `DisplayWidths` as root tool lines so the two families
// share a visual grid on the same terminal width.

/// Build an in-flight "running" placeholder line for a delegated sub-agent.
///
/// Shown from `SubAgentStart` until `SubAgentFinish` replaces it in-place.
/// `last_detail` is the most recent tool call or reasoning hint from the child.
pub fn build_subagent_running_line_width(
    task_index: usize,
    task_count: usize,
    goal: &str,
    last_detail: Option<&str>,
    elapsed_secs: u64,
    widths: &DisplayWidths,
) -> Vec<Span<'static>> {
    let badge = format!("[{}/{}]", task_index + 1, task_count);
    // Budget: bar(4) + icon(3) + badge(8) + elapsed(6) + dots(5) = 26 chrome chars.
    // Remaining split between goal and detail.
    let goal_width = widths.preview;
    let detail_width = widths.result.saturating_sub(16).max(16);

    let goal_col = unicode_pad_right(&unicode_trunc(goal, goal_width), goal_width);
    let detail_part = last_detail
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| format!("  {}", unicode_trunc(d, detail_width)))
        .unwrap_or_default();

    let elapsed_str = format_duration_aligned(elapsed_secs * 1_000);

    let bar_style = Style::default()
        .fg(Color::Rgb(55, 60, 70))
        .add_modifier(Modifier::DIM);
    let icon_style = Style::default().fg(Color::Rgb(95, 175, 255)); // blue → "in progress"
    let badge_style = Style::default()
        .fg(Color::Rgb(115, 128, 150))
        .add_modifier(Modifier::DIM);
    let goal_style = Style::default().fg(Color::Rgb(185, 198, 218));
    let detail_style = Style::default()
        .fg(Color::Rgb(90, 102, 125))
        .add_modifier(Modifier::DIM);
    let running_style = Style::default()
        .fg(Color::Rgb(95, 175, 255))
        .add_modifier(Modifier::DIM);
    let dur_style = Style::default()
        .fg(Color::Rgb(72, 79, 98))
        .add_modifier(Modifier::DIM);

    vec![
        Span::styled("  ┊ ", bar_style),
        Span::styled("→ ", icon_style),
        Span::styled(format!("{badge} "), badge_style),
        Span::styled(goal_col, goal_style),
        Span::styled(detail_part, detail_style),
        Span::styled("  ···".to_string(), running_style),
        Span::styled(format!("  {elapsed_str}"), dur_style),
    ]
}

/// Build a completed sub-agent line (replaces the running placeholder in-place).
///
/// Structured columns mirror `build_tool_done_line_width`:
/// `  ┊  ✅  [1/3]  summary text…   · N calls  model  duration`
#[allow(clippy::too_many_arguments)]
pub fn build_subagent_done_line_width(
    task_index: usize,
    task_count: usize,
    is_error: bool,
    duration_ms: u64,
    api_calls: u32,
    model: Option<&str>,
    summary: &str,
    widths: &DisplayWidths,
) -> Vec<Span<'static>> {
    let badge = format!("[{}/{}]", task_index + 1, task_count);
    let dur = format_duration_aligned(duration_ms);

    // Show first non-blank line of summary (or goal if summary empty).
    let summary_first = summary
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("completed");

    // Stat pill: "· N calls  model-short"
    let calls_str = if api_calls == 1 {
        "· 1 call".to_string()
    } else {
        format!("· {api_calls} calls")
    };
    let model_short = model
        .unwrap_or("")
        .split('/')
        .next_back()
        .unwrap_or("")
        .chars()
        .take(10)
        .collect::<String>();

    // Chrome: bar(4) + icon(3) + badge(8) + stats_pill + dur(7) ≈ 30 chars.
    // Stat pill itself: calls_str(9) + " " + model_short(10) = ~20 chars.
    let summary_budget = widths.preview + widths.name;
    let summary_col = unicode_trunc(summary_first, summary_budget);

    let bar_style = Style::default()
        .fg(Color::Rgb(55, 60, 70))
        .add_modifier(Modifier::DIM);
    let icon_style = if is_error {
        Style::default()
            .fg(Color::Rgb(255, 80, 80))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(104, 196, 129))
    };
    let badge_style = Style::default()
        .fg(Color::Rgb(115, 128, 150))
        .add_modifier(Modifier::DIM);
    let summary_style = if is_error {
        Style::default()
            .fg(Color::Rgb(255, 125, 125))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(200, 210, 226))
    };
    let stats_style = Style::default()
        .fg(Color::Rgb(100, 112, 135))
        .add_modifier(Modifier::DIM);
    let model_style = Style::default().fg(Color::Rgb(185, 145, 240)); // violet = Ai category
    let dur_style = Style::default()
        .fg(Color::Rgb(72, 79, 98))
        .add_modifier(Modifier::DIM);

    let icon: &str = if is_error { "❌" } else { "✅" };
    let model_part = if model_short.is_empty() {
        String::new()
    } else {
        format!("  {model_short}")
    };

    vec![
        Span::styled("  ┊ ", bar_style),
        Span::styled(format!("{icon} "), icon_style),
        Span::styled(format!("{badge} "), badge_style),
        Span::styled(summary_col, summary_style),
        Span::styled(format!("  {calls_str}"), stats_style),
        Span::styled(model_part, model_style),
        Span::styled(format!("  {dur}"), dur_style),
    ]
}

/// Legacy wrapper kept for any remaining direct callers.
/// New code should use [`build_subagent_running_line_width`] or
/// [`build_subagent_done_line_width`] directly.
#[allow(dead_code)]
pub fn build_subagent_event_line(
    task_index: usize,
    task_count: usize,
    _label: &str,
    detail: &str,
    tone: &str,
) -> Vec<Span<'static>> {
    let widths = &DisplayWidths::DEFAULT;
    match tone {
        "success" => build_subagent_done_line_width(
            task_index, task_count, false, 0, 0, None, detail, widths,
        ),
        "error" => {
            build_subagent_done_line_width(task_index, task_count, true, 0, 0, None, detail, widths)
        }
        _ => build_subagent_running_line_width(task_index, task_count, detail, None, 0, widths),
    }
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
    if n.contains("todo") || n.contains("task") || n.contains("plan") {
        return "planning";
    }
    if n.contains("skill") || n.contains("session_search") {
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
    if n.contains("todo") || n.contains("task") || n.contains("plan") {
        return "☑";
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
    tool_status_preview_width(tool_name, args_json, DisplayWidths::DEFAULT.status_preview)
}

pub fn tool_status_preview_width(tool_name: &str, args_json: &str, max_cols: usize) -> String {
    let display_name = tool_label(tool_name);
    let preview = extract_tool_preview(tool_name, args_json);
    let full = if preview.is_empty() {
        display_name
    } else {
        format!("{display_name} · {preview}")
    };
    unicode_trunc(&full, max_cols)
}

// ── Context pressure gauge ──────────────────────────────────────────────────

/// Build a compact context pressure gauge for the status bar.
///
/// Returns styled spans like: `ctx [▰▰▰▱▱] 52%`
///
/// Color tiers:
/// - Cyan: < 50% (safe)
/// - Yellow: 50–75% (warning)
/// - Red: > 75% (critical)
pub fn build_context_gauge(used_tokens: u64, context_window: u64) -> Vec<Span<'static>> {
    if context_window == 0 {
        return Vec::new();
    }
    let pct = ((used_tokens as f64 / context_window as f64) * 100.0).min(100.0);
    let filled = ((pct / 100.0) * 5.0).round() as usize;
    let empty = 5usize.saturating_sub(filled);

    let color = if pct < 50.0 {
        Color::Rgb(77, 208, 225) // cyan
    } else if pct < 75.0 {
        Color::Rgb(255, 200, 50) // yellow
    } else {
        Color::Rgb(239, 83, 80) // red
    };

    let gauge_str = format!("{}{}", "▰".repeat(filled), "▱".repeat(empty),);

    vec![
        Span::styled(
            " ctx [",
            Style::default()
                .fg(Color::Rgb(100, 105, 120))
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(gauge_str, Style::default().fg(color)),
        Span::styled(
            format!("] {:.0}% ", pct),
            Style::default()
                .fg(Color::Rgb(100, 105, 120))
                .add_modifier(Modifier::DIM),
        ),
    ]
}

// ── Tool result formatting ────────────────────────────────────────────────────

/// Parse `key=value` from a structured bracket-header such as:
///
/// ```text
/// [terminal_result status=success backend=local cwd=/tmp exit_code=0]
/// ```
///
/// Handles values that run up to the next space, `]`, or end of string.
fn parse_header_attr<'a>(header: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=");
    let start = header.find(needle.as_str())? + needle.len();
    let rest = &header[start..];
    let end = rest.find([' ', ']']).unwrap_or(rest.len());
    Some(&rest[..end]).filter(|v| !v.is_empty())
}

/// Format a `terminal` tool result for compact display.
///
/// The terminal tool prefixes output with a structured header line:
/// ```text
/// [terminal_result status=success backend=local cwd=/proj exit_code=0]
/// stdout line 1
/// stdout line 2
/// ```
/// This function renders it as `✓ 0  first-output-line` or `✗ N  first-error-line`.
fn format_terminal_result(result: &str, max_cols: usize) -> String {
    let mut lines_iter = result.lines();
    let header = lines_iter.next().unwrap_or("");

    let exit_code: i32 = parse_header_attr(header, "exit_code")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);
    let ok = exit_code == 0;
    let badge = if ok {
        format!("✓ {exit_code}")
    } else {
        format!("✗ {exit_code}")
    };

    let output_first = lines_iter
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");

    if output_first.is_empty() || badge.width() + 2 >= max_cols {
        return unicode_trunc(&badge, max_cols);
    }
    let detail_budget = max_cols.saturating_sub(badge.width() + 2);
    unicode_trunc(
        &format!("{}  {}", badge, unicode_trunc(output_first, detail_budget)),
        max_cols,
    )
}

/// Format an `execute_code` JSON result for compact display.
///
/// JSON shape: `{"status":"success","output":"…","tool_calls_made":N,"duration_seconds":N}`
///   or `{"status":"error","error":"…","output":"…"}`
fn format_execute_code_result(val: &serde_json::Value, max_cols: usize) -> String {
    let status = val
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let ok = status == "success";
    let badge = if ok { "✓" } else { "✗" };

    // Prefer error message for failures; output for success.
    let content = if !ok {
        val.get("error")
            .or_else(|| val.get("output"))
            .and_then(|v| v.as_str())
    } else {
        val.get("output")
            .or_else(|| val.get("error"))
            .and_then(|v| v.as_str())
    }
    .unwrap_or("")
    .trim();

    let first_line = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");

    if first_line.is_empty() {
        return unicode_trunc(&format!("{badge} {status}"), max_cols);
    }
    let badge_part = format!("{badge} {status}");
    let detail_budget = max_cols.saturating_sub(badge_part.width() + 2);
    if detail_budget < 4 {
        return unicode_trunc(&badge_part, max_cols);
    }
    unicode_trunc(
        &format!(
            "{}  {}",
            badge_part,
            unicode_trunc(&oneline(first_line), detail_budget)
        ),
        max_cols,
    )
}

/// Format a `web_search` JSON result: `N results · first-title`.
fn format_web_search_result(val: &serde_json::Value, max_cols: usize) -> String {
    let results = val
        .get("results")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let count = results.len();
    if count == 0 {
        return unicode_trunc("no results", max_cols);
    }
    let count_part = if count == 1 {
        "1 result".to_string()
    } else {
        format!("{count} results")
    };
    let first_title = results
        .first()
        .and_then(|item| item.get("title").or_else(|| item.get("name")))
        .and_then(|v| v.as_str())
        .map(oneline)
        .unwrap_or_default();
    if first_title.is_empty() || count_part.width() + 4 >= max_cols {
        return unicode_trunc(&count_part, max_cols);
    }
    let title_budget = max_cols.saturating_sub(count_part.width() + 4);
    if title_budget < 4 {
        return unicode_trunc(&count_part, max_cols);
    }
    unicode_trunc(
        &format!(
            "{count_part}  · {}",
            unicode_trunc(&first_title, title_budget)
        ),
        max_cols,
    )
}

/// Format a `web_extract` JSON result: `N.Nk chars · page-title`.
fn format_web_extract_result(val: &serde_json::Value, max_cols: usize) -> String {
    let result_node = val.get("result");
    let title = result_node
        .and_then(|r| r.get("title").or_else(|| r.get("name")))
        .and_then(|v| v.as_str())
        .map(oneline);
    let char_count = result_node
        .and_then(|r| r.get("content").or_else(|| r.get("text")))
        .and_then(|v| v.as_str())
        .map(|s| s.len())
        .or_else(|| {
            val.get("results").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        item.get("content")
                            .or_else(|| item.get("text"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.len())
                    })
                    .sum::<usize>()
            })
        })
        .unwrap_or(0);

    let size_part = if char_count >= 1_000 {
        format!("{:.1}k chars", char_count as f64 / 1_000.0)
    } else if char_count > 0 {
        format!("{char_count} chars")
    } else {
        "extracted".to_string()
    };

    match title.filter(|t| !t.is_empty()) {
        Some(t) if size_part.width() + 4 < max_cols => {
            let title_budget = max_cols.saturating_sub(size_part.width() + 4);
            if title_budget < 4 {
                unicode_trunc(&size_part, max_cols)
            } else {
                unicode_trunc(
                    &format!("{size_part}  · {}", unicode_trunc(&t, title_budget)),
                    max_cols,
                )
            }
        }
        _ => unicode_trunc(&size_part, max_cols),
    }
}

/// Format a `web_crawl` JSON result: `N pages crawled`.
fn format_web_crawl_result(val: &serde_json::Value, max_cols: usize) -> String {
    let count = val
        .get("pages")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .or_else(|| {
            val.get("page_count")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
        })
        .unwrap_or(0);
    if count == 0 {
        unicode_trunc("crawled", max_cols)
    } else if count == 1 {
        unicode_trunc("1 page crawled", max_cols)
    } else {
        unicode_trunc(&format!("{count} pages crawled"), max_cols)
    }
}

/// Format a `session_search` JSON result: `N results`.
fn format_session_search_result(val: &serde_json::Value, max_cols: usize) -> String {
    let count = val
        .get("results")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .or_else(|| {
            val.get("sessions")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
        })
        .unwrap_or(0);
    if count == 0 {
        unicode_trunc("no results", max_cols)
    } else if count == 1 {
        unicode_trunc("1 result", max_cols)
    } else {
        unicode_trunc(&format!("{count} results"), max_cols)
    }
}

/// Format a `ha_*` tool JSON result: `✓ state` / `✗ error`.
fn format_ha_result(val: &serde_json::Value, max_cols: usize) -> String {
    let is_failure = val
        .get("success")
        .and_then(|v| v.as_bool())
        .map(|v| !v)
        .unwrap_or(false)
        || val.get("error").is_some();
    if is_failure {
        let err = val
            .get("error")
            .and_then(|v| v.as_str())
            .map(oneline)
            .unwrap_or_else(|| "failed".to_string());
        unicode_trunc(&format!("✗ {err}"), max_cols)
    } else {
        let state = val.get("state").and_then(|v| v.as_str()).unwrap_or("ok");
        unicode_trunc(&format!("✓ {state}"), max_cols)
    }
}

/// Format a tool result string for the **compact done-line** and the **verbose result line**.
///
/// Applies per-tool rich formatting (exit codes, counts, titles) and falls back to
/// first-line truncation for tools without a dedicated formatter.
///
/// # Design
///
/// - One central function (`format_tool_result`) replaces all direct `unicode_trunc(result, …)` calls.
/// - Per-tool helpers are private; callers only know `format_tool_result`.
/// - Adding a new tool formatter requires only a new `match` arm — no call-site changes.
pub fn format_tool_result(tool_name: &str, result: &str, max_cols: usize) -> String {
    let result_trimmed = result.trim();
    if result_trimmed.is_empty() {
        return String::new();
    }

    // ── 1. terminal — structured header line ────────────────────────────
    // Only apply the structured parser when the result actually carries
    // the `[terminal_result …]` header produced by the tool's execute()
    // implementation.  Test mocks and background-task proxies may pass
    // plain human-readable strings; those fall through to the default path.
    if tool_name == "terminal" && result_trimmed.starts_with("[terminal_result ") {
        return format_terminal_result(result_trimmed, max_cols);
    }

    // ── 2. JSON-returning tools ─────────────────────────────────────────
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(result_trimmed) {
        match tool_name {
            "execute_code" => return format_execute_code_result(&val, max_cols),
            "web_search" => return format_web_search_result(&val, max_cols),
            "web_extract" => return format_web_extract_result(&val, max_cols),
            "web_crawl" => return format_web_crawl_result(&val, max_cols),
            "session_search" => return format_session_search_result(&val, max_cols),
            _ if tool_name.starts_with("ha_") => return format_ha_result(&val, max_cols),
            _ => {}
        }
    }

    // ── 3. Plain-text tools with known formats ──────────────────────────
    match tool_name {
        "write_file" => {
            // "Wrote N bytes to 'path'"
            // Show `✓ N bytes` — concise success signal with size.
            if let Some(rest) = result_trimmed.strip_prefix("Wrote ") {
                // rest = "N bytes to 'path'"
                let end = rest.find(" to ").unwrap_or(rest.len());
                let size_part = rest[..end].trim();
                if !size_part.is_empty() {
                    return unicode_trunc(&format!("✓ {size_part}"), max_cols);
                }
            }
            return unicode_trunc(&oneline(result_trimmed), max_cols);
        }
        "apply_patch" if result_trimmed.contains("succeeded") => {
            // "apply_patch succeeded. Modified: src/a.rs; Created: src/b.rs"
            let detail = result_trimmed
                .find(". ")
                .map(|i| &result_trimmed[i + 2..])
                .unwrap_or(result_trimmed)
                .trim();
            // Count comma-separated items in each section.
            let count_section = |label: &str, text: &str| -> usize {
                text.split(label)
                    .nth(1)
                    .map(|s| s.split(';').next().unwrap_or(""))
                    .map(|s| s.split(',').filter(|t| !t.trim().is_empty()).count())
                    .unwrap_or(0)
            };
            let total = count_section("Modified: ", detail)
                + count_section("Created: ", detail)
                + count_section("Deleted: ", detail);
            let summary = if total == 0 {
                "✓ ok".to_string()
            } else if total == 1 {
                "✓ 1 file".to_string()
            } else {
                format!("✓ {total} files")
            };
            return unicode_trunc(&summary, max_cols);
        }
        "read_file" => {
            // Result is the raw file content.  Show line count + first meaningful line.
            let n = result_trimmed.lines().count();
            let count_part = if n == 1 {
                "1 line".to_string()
            } else {
                format!("{n} lines")
            };
            let first_code = result_trimmed
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with('#'))
                .unwrap_or("");
            if first_code.is_empty() || count_part.width() + 2 >= max_cols {
                return unicode_trunc(&count_part, max_cols);
            }
            let detail_budget = max_cols.saturating_sub(count_part.width() + 2);
            return unicode_trunc(
                &format!(
                    "{}  {}",
                    count_part,
                    unicode_trunc(&oneline(first_code), detail_budget)
                ),
                max_cols,
            );
        }
        "search_files" => {
            // Grep output — count non-blank, non-separator lines as "matches".
            let match_count = result_trimmed
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with("---"))
                .count();
            return if match_count == 0 {
                unicode_trunc("no matches", max_cols)
            } else if match_count == 1 {
                unicode_trunc("1 match", max_cols)
            } else {
                unicode_trunc(&format!("{match_count} matches"), max_cols)
            };
        }
        "memory"
            if result_trimmed.starts_with("Added")
                || result_trimmed.starts_with("Updated")
                || result_trimmed.starts_with("Removed")
                || result_trimmed.contains("success") =>
        {
            // "Added to user memory file." etc.
            return unicode_trunc("✓ saved", max_cols);
        }
        "todo" | "manage_todo_list" => {
            return unicode_trunc("✓ plan updated", max_cols);
        }
        "generate_image" | "image_generate" => {
            // Usually an image path or URL — show just the filename.
            let fname = result_trimmed
                .lines()
                .next()
                .unwrap_or(result_trimmed)
                .trim()
                .split('/')
                .next_back()
                .unwrap_or(result_trimmed);
            return unicode_trunc(&oneline(fname), max_cols);
        }
        "text_to_speech" => {
            let fname = result_trimmed
                .lines()
                .next()
                .unwrap_or(result_trimmed)
                .trim()
                .split('/')
                .next_back()
                .unwrap_or(result_trimmed);
            return unicode_trunc(&oneline(fname), max_cols);
        }
        _ => {}
    }

    // ── 4. Default: first non-empty line, collapsed whitespace, truncated ──
    let first_line = result_trimmed
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or(result_trimmed);
    unicode_trunc(&oneline(first_line), max_cols)
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
    fn test_extract_tool_preview_summarizes_apply_patch_targets() {
        let patch = "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** Add File: src/lib.rs\n+hello\n*** End Patch\n";
        let preview = extract_tool_preview(
            "apply_patch",
            &format!(
                r#"{{"patch":{}}}"#,
                serde_json::to_string(patch).expect("json")
            ),
        );
        assert!(preview.contains("2 file(s): src/main.rs"), "got: {preview}");
    }

    #[test]
    fn test_extract_tool_preview_formats_browser_wait() {
        let preview = extract_tool_preview(
            "browser_wait_for",
            r#"{"selector":".results","timeout":10}"#,
        );
        assert_eq!(preview, "selector .results");
    }

    #[test]
    fn test_extract_tool_preview_formats_send_message_with_quote() {
        let preview = extract_tool_preview(
            "send_message",
            r#"{"target":"telegram:#ops","message":"deploy is complete"}"#,
        );
        assert!(preview.contains("telegram:#ops"), "got: {preview}");
        assert!(preview.contains("\"deploy is complete\""), "got: {preview}");
    }

    #[test]
    fn test_extract_tool_preview_supports_manage_todo_list_alias() {
        let preview = extract_tool_preview(
            "manage_todo_list",
            r#"{"items":[{"id":1,"title":"Audit","status":"in-progress"}],"merge":true}"#,
        );
        assert_eq!(preview, "update 1 task · Audit");
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
        assert_eq!(tool_action_verb("manage_todo_list"), "planning");
        assert_eq!(tool_action_verb("mcp_call"), "calling");
        assert_eq!(tool_action_verb("unknown_op"), "running");
    }

    #[test]
    fn test_build_tool_verbose_lines_renders_todo_plan_board() {
        let lines = build_tool_verbose_lines(
            "manage_todo_list",
            r#"{"items":[{"id":1,"title":"Audit Hermes display","status":"completed"},{"id":2,"title":"Improve todo renderer","status":"in-progress"},{"id":3,"title":"Reassess UX","status":"not-started"}],"merge":true}"#,
            Some("1/3 done, 1 in progress"),
            false,
        );
        assert_eq!(lines.len(), 4);
        let joined = lines
            .iter()
            .map(|line| {
                line.iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(joined[0].contains("1/3 done, 1 in progress"));
        assert!(joined[1].contains("[x]"));
        assert!(joined[2].contains("[>]"));
        assert!(joined[3].contains("[ ]"));
    }

    #[test]
    fn test_tool_icon_known() {
        assert_eq!(tool_icon("web_search"), "⌕");
        assert_eq!(tool_icon("bash_exec"), "$");
        assert_eq!(tool_icon("write_file"), "✎");
        assert_eq!(tool_icon("read_file"), "≡"); // matches "read" check
        assert_eq!(tool_icon("mcp_call"), "◎");
        assert_eq!(tool_icon("manage_todo_list"), "☑");
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
        // The result column now shows the rich format (✓ N bytes) rather than raw text.
        assert!(
            joined.contains("src/main.rs"),
            "should contain path in args preview"
        );
        assert!(
            joined.contains("✓ 42 bytes"),
            "result should use rich write_file format, got: {joined}"
        );
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

    // ── DisplayWidths tests ─────────────────────────────────────────

    #[test]
    fn test_display_widths_default_for_narrow() {
        let w = DisplayWidths::from_terminal_width(80);
        assert_eq!(w.name, DisplayWidths::DEFAULT.name);
        assert_eq!(w.preview, DisplayWidths::DEFAULT.preview);
    }

    #[test]
    fn test_display_widths_scales_for_wide_terminal() {
        let w = DisplayWidths::from_terminal_width(160);
        assert!(
            w.preview > 44,
            "preview should be wider than default 44, got {}",
            w.preview
        );
        assert!(
            w.result > 52,
            "result should be wider than default 52, got {}",
            w.result
        );
        assert!(
            w.verbose_content > 108,
            "verbose should be wider than 108, got {}",
            w.verbose_content
        );
    }

    #[test]
    fn test_display_widths_very_narrow() {
        let w = DisplayWidths::from_terminal_width(40);
        assert_eq!(w.name, DisplayWidths::DEFAULT.name);
        assert!(w.preview >= 20);
        assert!(w.result >= 20);
    }

    // ── Context gauge tests ─────────────────────────────────────────

    #[test]
    fn test_context_gauge_zero_window() {
        let spans = build_context_gauge(1000, 0);
        assert!(spans.is_empty());
    }

    #[test]
    fn test_context_gauge_low_usage() {
        let spans = build_context_gauge(20_000, 128_000);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("▰"), "should have filled chars");
        assert!(text.contains("▱"), "should have empty chars");
        assert!(text.contains("16%"), "should show percentage, got: {text}");
    }

    #[test]
    fn test_context_gauge_high_usage() {
        let spans = build_context_gauge(100_000, 128_000);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("78%"), "should show percentage, got: {text}");
    }

    #[test]
    fn test_context_gauge_full() {
        let spans = build_context_gauge(128_000, 128_000);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("▰▰▰▰▰"), "should be full");
        assert!(text.contains("100%"), "should show 100%, got: {text}");
    }

    // ── Width-adaptive tool line tests ──────────────────────────────

    #[test]
    fn test_build_tool_done_line_width_uses_wider_result() {
        let wide = DisplayWidths::from_terminal_width(160);
        let long_result = "a".repeat(100);
        let spans = build_tool_done_line_width(
            "web_search",
            r#"{"query":"test"}"#,
            Some(&long_result),
            100,
            false,
            &std::collections::HashMap::new(),
            &wide,
        );
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        // With wider widths, more of the result should be visible
        let result_segment = text.split("→ ").nth(1).unwrap_or("");
        assert!(
            result_segment.len() > 52,
            "wider terminal should show more result"
        );
    }

    #[test]
    fn test_extract_tool_preview_width_respects_limit() {
        let long_query = "a".repeat(200);
        let args = format!(r#"{{"query": "{long_query}"}}"#);
        let narrow = extract_tool_preview_width("web_search", &args, 30);
        assert!(
            narrow.width() <= 30,
            "narrow preview too wide: {}",
            narrow.width()
        );
        let wide = extract_tool_preview_width("web_search", &args, 80);
        assert!(
            wide.width() <= 80,
            "wide preview too wide: {}",
            wide.width()
        );
        assert!(
            wide.width() > narrow.width(),
            "wide should show more than narrow"
        );
    }

    // ── format_tool_result tests ────────────────────────────────────

    #[test]
    fn test_format_tool_result_empty() {
        assert_eq!(format_tool_result("terminal", "", 80), "");
        assert_eq!(format_tool_result("web_search", "   ", 80), "");
    }

    #[test]
    fn test_format_tool_result_terminal_success() {
        let result =
            "[terminal_result status=success backend=local cwd=/proj exit_code=0]\ncargo build\n";
        let out = format_tool_result("terminal", result, 80);
        assert!(out.contains("✓ 0"), "should show ✓ exit 0, got: {out}");
        assert!(
            out.contains("cargo build"),
            "should show stdout, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_terminal_failure() {
        let result = "[terminal_result status=error backend=local cwd=/proj exit_code=1]\nerror: linker not found\n";
        let out = format_tool_result("terminal", result, 80);
        assert!(out.contains("✗ 1"), "should show ✗ exit 1, got: {out}");
        assert!(out.contains("linker"), "should show error line, got: {out}");
    }

    #[test]
    fn test_format_tool_result_terminal_no_output() {
        // Only the header, no stdout — show exit code only.
        let result = "[terminal_result status=success backend=local cwd=/tmp exit_code=0]";
        let out = format_tool_result("terminal", result, 80);
        assert!(out.contains("✓ 0"), "should show ✓ exit 0, got: {out}");
    }

    #[test]
    fn test_parse_header_attr_exit_code() {
        let header = "[terminal_result status=success backend=local cwd=/tmp exit_code=42]";
        assert_eq!(parse_header_attr(header, "exit_code"), Some("42"));
        assert_eq!(parse_header_attr(header, "status"), Some("success"));
        assert_eq!(parse_header_attr(header, "missing"), None);
    }

    #[test]
    fn test_format_tool_result_execute_code_success() {
        let result = r#"{"status":"success","output":"Hello world\n","tool_calls_made":0,"duration_seconds":0.12}"#;
        let out = format_tool_result("execute_code", result, 80);
        assert!(out.contains("✓"), "should have success badge, got: {out}");
        assert!(
            out.contains("Hello world"),
            "should show output, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_execute_code_error() {
        let result = r#"{"status":"error","error":"NameError: name 'x' not defined","output":""}"#;
        let out = format_tool_result("execute_code", result, 80);
        assert!(out.contains("✗"), "should have failure badge, got: {out}");
        assert!(out.contains("NameError"), "should show error, got: {out}");
    }

    #[test]
    fn test_format_tool_result_web_search_with_results() {
        let result = r#"{"success":true,"query":"rust async","backend":"brave","results":[{"title":"Async in Rust","url":"https://example.com"},{"title":"Tokio guide","url":"https://tokio.rs"}]}"#;
        let out = format_tool_result("web_search", result, 80);
        assert!(out.contains("2 results"), "should show count, got: {out}");
        assert!(
            out.contains("Async in Rust"),
            "should show first title, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_web_search_no_results() {
        let result = r#"{"success":true,"query":"xyz404","backend":"brave","results":[]}"#;
        let out = format_tool_result("web_search", result, 80);
        assert_eq!(out, "no results");
    }

    #[test]
    fn test_format_tool_result_web_extract() {
        let result = r#"{"success":true,"backend":"native","result":{"title":"Rust Programming Language","content":"The Rust programming language is blazingly fast..."}}"#;
        let out = format_tool_result("web_extract", result, 80);
        assert!(out.contains("chars"), "should show char count, got: {out}");
        assert!(
            out.contains("Rust Programming Language"),
            "should show title, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_web_crawl() {
        let result = r#"{"success":true,"pages":[{"url":"https://a.com"},{"url":"https://b.com"},{"url":"https://c.com"}]}"#;
        let out = format_tool_result("web_crawl", result, 80);
        assert!(
            out.contains("3 pages"),
            "should show page count, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_read_file_counts_lines() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let out = format_tool_result("read_file", content, 80);
        assert!(out.contains("3 lines"), "should show 3 lines, got: {out}");
        assert!(
            out.contains("fn main"),
            "should show first code line, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_read_file_single_line() {
        let out = format_tool_result("read_file", "hello world", 80);
        assert!(out.contains("1 line"), "should say 1 line, got: {out}");
    }

    #[test]
    fn test_format_tool_result_search_files_counts_matches() {
        let result = "src/main.rs:5:fn main() {\nsrc/lib.rs:12:pub fn foo() {\n";
        let out = format_tool_result("search_files", result, 80);
        assert!(
            out.contains("2 matches"),
            "should show 2 matches, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_search_files_no_matches() {
        let out = format_tool_result("search_files", "   ", 80);
        // empty result → empty string (not "0 matches")
        assert!(out.is_empty() || out.contains("no matches"), "got: {out}");
    }

    #[test]
    fn test_format_tool_result_write_file_extracts_size() {
        let out = format_tool_result("write_file", "Wrote 1234 bytes to 'src/main.rs'", 80);
        assert!(out.contains("✓"), "should have success badge, got: {out}");
        assert!(out.contains("1234 bytes"), "should show size, got: {out}");
    }

    #[test]
    fn test_format_tool_result_apply_patch_counts_files() {
        let result =
            "apply_patch succeeded. Modified: src/main.rs, src/lib.rs; Created: src/new.rs";
        let out = format_tool_result("apply_patch", result, 80);
        assert!(out.contains("✓"), "should have success badge, got: {out}");
        assert!(out.contains("3 files"), "should show 3 files, got: {out}");
    }

    #[test]
    fn test_format_tool_result_apply_patch_single_file() {
        let result = "apply_patch succeeded. Modified: src/main.rs";
        let out = format_tool_result("apply_patch", result, 80);
        assert!(out.contains("✓ 1 file"), "should show 1 file, got: {out}");
    }

    #[test]
    fn test_format_tool_result_session_search() {
        let result = r#"{"success":true,"results":[{"session_id":"abc"},{"session_id":"def"}]}"#;
        let out = format_tool_result("session_search", result, 80);
        assert!(
            out.contains("2 results"),
            "should show 2 results, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_ha_call_service_success() {
        let result = r#"{"success":true,"state":"on"}"#;
        let out = format_tool_result("ha_call_service", result, 80);
        assert!(out.contains("✓"), "should have success badge, got: {out}");
    }

    #[test]
    fn test_format_tool_result_ha_call_service_failure() {
        let result = r#"{"success":false,"error":"entity not found"}"#;
        let out = format_tool_result("ha_call_service", result, 80);
        assert!(out.contains("✗"), "should have failure badge, got: {out}");
        assert!(
            out.contains("entity not found"),
            "should show error, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_memory_saved() {
        let out = format_tool_result("memory", "Added to user memory file.", 80);
        assert_eq!(out, "✓ saved");
    }

    #[test]
    fn test_format_tool_result_todo_plan_updated() {
        let out = format_tool_result("todo", "TodoList updated.", 80);
        assert_eq!(out, "✓ plan updated");
        let out2 = format_tool_result("manage_todo_list", "ok", 80);
        assert_eq!(out2, "✓ plan updated");
    }

    #[test]
    fn test_format_tool_result_unknown_tool_falls_back() {
        // Unknown tool: should return first meaningful line, not empty.
        let out = format_tool_result("my_custom_tool", "result: all clear", 80);
        assert!(
            !out.is_empty(),
            "unknown tool should produce non-empty output"
        );
        assert!(
            out.contains("result: all clear"),
            "should show first line, got: {out}"
        );
    }

    #[test]
    fn test_format_tool_result_respects_max_cols() {
        // Very narrow budget — must never exceed.
        let result = "[terminal_result status=success backend=local cwd=/longpath exit_code=0]\nsome quite long output line here";
        let out = format_tool_result("terminal", result, 15);
        assert!(
            out.width() <= 15,
            "result should fit in 15 cols, got {} cols: '{out}'",
            out.width()
        );
    }

    #[test]
    fn test_format_tool_result_done_line_uses_rich_format() {
        // Verify the done-line plumbing uses format_tool_result (not raw truncation).
        let spans = build_tool_done_line_width(
            "terminal",
            r#"{"command":"cargo build"}"#,
            Some(
                "[terminal_result status=success backend=local cwd=/proj exit_code=0]\nFinished release target",
            ),
            500,
            false,
            &std::collections::HashMap::new(),
            &DisplayWidths::DEFAULT,
        );
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("✓ 0"),
            "done-line should use rich terminal format, got: {text}"
        );
        assert!(
            text.contains("Finished release"),
            "done-line should show first output line, got: {text}"
        );
    }

    #[test]
    fn test_format_tool_result_verbose_line_uses_rich_format() {
        // Verify verbose lines also use format_tool_result.
        let lines = build_tool_verbose_lines_width(
            "write_file",
            r#"{"path":"src/main.rs","content":"fn main() {}"}"#,
            Some("Wrote 12 bytes to 'src/main.rs'"),
            false,
            DisplayWidths::DEFAULT.verbose_content,
        );
        let all_text: String = lines
            .iter()
            .flat_map(|line| line.iter().map(|span| span.content.as_ref()))
            .collect();
        assert!(
            all_text.contains("✓ 12 bytes"),
            "verbose result should show rich write_file format, got: {all_text}"
        );
    }
}
