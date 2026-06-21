//! Parse `harness.jsonl` for operator diagnostics (`edgecrab doctor harness`).

use std::fs;
use std::path::Path;

/// Metrics extracted from a harness JSONL log.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HarnessLogReport {
    pub tool_starts: usize,
    pub spill_events: usize,
    pub read_file_after_spill: usize,
    pub spill_without_read: usize,
    pub terminal_without_perception: usize,
    pub last_exit_reason: Option<String>,
    pub last_decision: Option<String>,
}

const PERCEPTION_TOOLS: &[&str] = &[
    "browser_navigate",
    "browser_snapshot",
    "browser_vision",
    "browser_click",
    "browser_console",
    "computer_use",
    "capture_screenshot",
    "analyze_image",
    "vision",
];

/// Analyze harness log text (tracing JSONL or plain formatted lines).
pub fn analyze_harness_log(content: &str) -> HarnessLogReport {
    let mut report = HarnessLogReport::default();
    let mut tool_names: Vec<String> = Vec::new();
    let mut saw_spill = false;
    let mut read_after_spill = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(event) = parse_harness_line(line) {
            if event.spill {
                report.spill_events += 1;
                saw_spill = true;
                read_after_spill = false;
            }
            if let Some(name) = event.tool_name {
                if saw_spill && name == "read_file" {
                    read_after_spill = true;
                    report.read_file_after_spill += 1;
                    saw_spill = false;
                }
                tool_names.push(name);
                if event.tool_start {
                    report.tool_starts += 1;
                }
            }
            if event.turn_complete {
                if let Some(reason) = event.exit_reason {
                    report.last_exit_reason = Some(reason);
                }
                if let Some(decision) = event.decision {
                    report.last_decision = Some(decision);
                }
            }
            continue;
        }

        // Plain-text fallback (tests + legacy logs)
        if line.contains("[tool_result_spill]") || line.contains("tool result spilled") {
            report.spill_events += 1;
            saw_spill = true;
            read_after_spill = false;
        }

        if (line.contains("harness: tool start") || line.contains("harness: tool complete"))
            && let Some(name) = extract_plain_field(line, "tool_name")
        {
            if saw_spill && name == "read_file" {
                read_after_spill = true;
                report.read_file_after_spill += 1;
                saw_spill = false;
            }
            tool_names.push(name);
            if line.contains("harness: tool start") {
                report.tool_starts += 1;
            }
        }

        if line.contains("harness: turn complete") {
            report.last_exit_reason = extract_plain_field(line, "exit_reason");
            report.last_decision = extract_plain_field(line, "decision");
        }
    }

    if saw_spill && !read_after_spill {
        report.spill_without_read += 1;
    }

    report.terminal_without_perception = count_terminal_without_perception(&tool_names);
    report
}

#[derive(Default)]
struct ParsedHarnessLine {
    spill: bool,
    tool_start: bool,
    tool_name: Option<String>,
    turn_complete: bool,
    exit_reason: Option<String>,
    decision: Option<String>,
}

fn parse_harness_line(line: &str) -> Option<ParsedHarnessLine> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let fields = value.get("fields")?;
    let message = fields.get("message")?.as_str()?;

    let mut event = ParsedHarnessLine::default();

    if message.contains("spill") || message.contains("tool_result_spill") {
        event.spill = true;
    }

    match message {
        "harness: tool start" => {
            event.tool_start = true;
            event.tool_name = json_string_field(fields, "tool_name");
        }
        "harness: tool complete" => {
            event.tool_name = json_string_field(fields, "tool_name");
        }
        "harness: turn complete" => {
            event.turn_complete = true;
            event.exit_reason = json_string_field(fields, "exit_reason");
            event.decision = json_string_field(fields, "decision");
        }
        _ => {}
    }

    if event.spill || event.tool_start || event.tool_name.is_some() || event.turn_complete {
        Some(event)
    } else {
        None
    }
}

fn json_string_field(fields: &serde_json::Value, key: &str) -> Option<String> {
    fields
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn extract_plain_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == ',' || c == '"')
        .unwrap_or(rest.len());
    let value = rest[..end].trim_matches('"').trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn count_terminal_without_perception(tool_names: &[String]) -> usize {
    let mut window: Vec<&str> = Vec::new();
    let mut gaps = 0usize;
    for name in tool_names {
        window.push(name.as_str());
        if window.len() > 12 {
            window.remove(0);
        }
        if name != "terminal" && name != "run_process" {
            continue;
        }
        let has_perception = window.iter().any(|n| PERCEPTION_TOOLS.contains(n));
        if !has_perception && window.iter().filter(|n| **n == "terminal").count() >= 5 {
            gaps += 1;
            window.clear();
        }
    }
    gaps
}

/// Load and analyze `harness.jsonl` from a log directory.
pub fn analyze_harness_file(log_dir: &Path) -> std::io::Result<HarnessLogReport> {
    let path = log_dir.join(crate::logging::HARNESS_JSON_LOG_NAME);
    let content = fs::read_to_string(path)?;
    Ok(analyze_harness_log(&content))
}

/// Render a human-readable harness doctor report.
pub fn format_harness_report(report: &HarnessLogReport) -> String {
    let mut lines = vec![
        "Harness log analysis".into(),
        format!("  tool starts: {}", report.tool_starts),
        format!("  spill events: {}", report.spill_events),
        format!("  spill-without-read (tail): {}", report.spill_without_read),
        format!("  read_file after spill: {}", report.read_file_after_spill),
        format!(
            "  terminal storms w/o perception: {}",
            report.terminal_without_perception
        ),
    ];
    if let Some(reason) = report.last_exit_reason.as_deref() {
        lines.push(format!("  last exit_reason: {reason}"));
    }
    if let Some(decision) = report.last_decision.as_deref() {
        lines.push(format!("  last decision: {decision}"));
    }
    if report.spill_without_read > 0 {
        lines.push(
            "  ⚠ spill stub seen without follow-up read_file — model may be blind to artifacts."
                .into(),
        );
    }
    if report.terminal_without_perception > 0 {
        lines.push(
            "  ⚠ terminal-heavy window without browser/vision — visual tasks may lack evidence."
                .into(),
        );
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ha25_reports_spill_without_read() {
        let log = r#"
INFO edgecrab::harness tool_name=read_file harness: tool start
INFO spilled tool result spilled to artifact
INFO edgecrab::harness tool_name=write_file harness: tool start
INFO edgecrab::harness exit_reason=interrupted decision=incomplete harness: turn complete
"#;
        let report = analyze_harness_log(log);
        assert!(report.spill_events >= 1);
        assert_eq!(report.spill_without_read, 1);
        assert_eq!(report.last_exit_reason.as_deref(), Some("interrupted"));
    }

    #[test]
    fn ha25_read_after_spill_clears_gap() {
        let log = r#"
INFO [tool_result_spill] artifact: .edgecrab-artifacts/s/read_file_001.md
INFO edgecrab::harness tool_name=read_file harness: tool start
"#;
        let report = analyze_harness_log(log);
        assert_eq!(report.read_file_after_spill, 1);
        assert_eq!(report.spill_without_read, 0);
    }

    #[test]
    fn ha44_parses_tracing_jsonl() {
        let log = r#"{"fields":{"message":"harness: tool start","tool_name":"terminal"}}
{"fields":{"message":"harness: tool start","tool_name":"write_file"}}
{"fields":{"message":"harness: turn complete","exit_reason":"interrupted","decision":"interrupted"}}"#;
        let report = analyze_harness_log(log);
        assert_eq!(report.tool_starts, 2);
        assert_eq!(report.last_exit_reason.as_deref(), Some("interrupted"));
        assert_eq!(report.last_decision.as_deref(), Some("interrupted"));
    }

    #[test]
    fn terminal_storm_detected_without_perception() {
        let mut names = Vec::new();
        for _ in 0..6 {
            names.push("terminal".into());
        }
        assert!(count_terminal_without_perception(&names) >= 1);
    }

    #[test]
    fn perception_tool_clears_terminal_storm() {
        let names = vec![
            "terminal".into(),
            "terminal".into(),
            "browser_navigate".into(),
            "terminal".into(),
        ];
        assert_eq!(count_terminal_without_perception(&names), 0);
    }
}
