//! Harness turn advisories — iteration storm, perception recovery (spec 015 P0.6 / P1.9).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::task_class::{TaskClass, classify_from_messages, is_verification_tool_for_class};

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

const ACT_STORM_TOOLS: &[&str] = &[
    "terminal",
    "run_process",
    "write_file",
    "patch",
    "apply_patch",
    "execute_code",
];

/// Tools that substitute for browser verification when preview/CDP is unavailable.
const VERIFICATION_THEATER_TOOLS: &[&str] = &["execute_code", "tool_search"];

/// Sliding-window tracker for act-without-perceive iteration storms.
#[derive(Debug)]
pub struct HarnessTurnAdvisory {
    window: VecDeque<(Instant, String)>,
    window_secs: u64,
    storm_threshold: usize,
    preview_recovery_sent: bool,
    last_storm_advisory: Option<Instant>,
    browser_nav_failures: u32,
    browser_nav_success: bool,
}

impl Default for HarnessTurnAdvisory {
    fn default() -> Self {
        Self::new()
    }
}

impl HarnessTurnAdvisory {
    pub fn new() -> Self {
        Self {
            window: VecDeque::new(),
            window_secs: 60,
            storm_threshold: 5,
            preview_recovery_sent: false,
            last_storm_advisory: None,
            browser_nav_failures: 0,
            browser_nav_success: false,
        }
    }

    pub fn browser_nav_failures(&self) -> u32 {
        self.browser_nav_failures
    }

    pub fn browser_nav_succeeded(&self) -> bool {
        self.browser_nav_success
    }

    pub fn record_browser_navigate_result(&mut self, tool_result: &str) {
        let lower = tool_result.to_ascii_lowercase();
        let failed = lower.contains("\"success\":false")
            || lower.contains("\"is_error\":true")
            || lower.contains("tool error")
            || lower.contains("connection refused")
            || lower.contains("cdp")
            || lower.contains("chrome")
            || lower.contains("headless")
            || lower.contains("blocked")
            || lower.contains("ssrf")
            || lower.contains("scheme not allowed");
        if failed {
            self.browser_nav_failures = self.browser_nav_failures.saturating_add(1);
            return;
        }
        if lower.contains("\"success\":true") || lower.contains("navigated") {
            self.browser_nav_success = true;
        }
    }

    pub fn record_tool(&mut self, tool_name: &str) {
        let now = Instant::now();
        self.window.push_back((now, tool_name.to_string()));
        let cutoff = now - Duration::from_secs(self.window_secs);
        while self.window.front().is_some_and(|(t, _)| *t < cutoff) {
            self.window.pop_front();
        }
    }

    /// One-shot user message when `browser_navigate` fails SSRF/scheme (HA-16 ops).
    pub fn maybe_preview_recovery(
        &mut self,
        tool_name: &str,
        tool_result: &str,
        known_ports: &[u16],
    ) -> Option<String> {
        if self.preview_recovery_sent || tool_name != "browser_navigate" {
            return None;
        }
        let lower = tool_result.to_ascii_lowercase();
        let blocked = lower.contains("blocked")
            || lower.contains("ssrf")
            || lower.contains("scheme not allowed")
            || lower.contains("file://");
        let cdp_or_connect = lower.contains("cdp")
            || lower.contains("chrome")
            || lower.contains("headless")
            || lower.contains("connection refused")
            || lower.contains("websocket");
        let failed_nav = self.browser_nav_failures > 0 && !self.browser_nav_success;
        if !blocked && !cdp_or_connect && !failed_nav {
            return None;
        }
        self.preview_recovery_sent = true;
        let port_line = edgecrab_tools::dev_server::format_dev_server_ports_hint(known_ports)
            .map(|h| format!("\n{h}"))
            .unwrap_or_default();
        Some(format!(
            "[harness] browser verification unavailable — do not use file://, execute_code screenshots, \
             or read ~/.edgecrab/config.yaml. Start a dev server on an allowed preview port, then \
             browser_navigate to http://127.0.0.1:PORT/ and browser_snapshot. \
             If Chrome/CDP is down: run a local server (python3 -m http.server 8000) and ensure \
             security.preview.enabled is true in the active profile.{port_line} \
             Do not write markdown verification reports."
        ))
    }

    /// Block execute_code / tool_search loops after repeated browser_navigate failures on visual tasks.
    pub fn maybe_verification_theater_block(
        &self,
        task_class: TaskClass,
        tool_name: &str,
    ) -> Option<String> {
        if !matches!(task_class, TaskClass::VisualUx) {
            return None;
        }
        if !VERIFICATION_THEATER_TOOLS.contains(&tool_name) {
            return None;
        }
        if self.browser_nav_success || self.browser_nav_failures < 2 {
            return None;
        }
        Some(
            edgecrab_tools::tool_loop_guardrails::guardrail_block_result(
                &edgecrab_tools::tool_loop_guardrails::ToolGuardrailDecision {
                    action: edgecrab_tools::tool_loop_guardrails::GuardrailAction::Block,
                    code: "verification_theater_block",
                    message:
                        "Blocked verification workaround — browser_navigate failed repeatedly. \
                           Start http.server on port 8000 (or dev_server), open \
                           http://127.0.0.1:8000/... via browser_navigate, then browser_snapshot. \
                           Do not script screenshots via execute_code."
                            .into(),
                    tool_name: tool_name.to_string(),
                    count: self.browser_nav_failures,
                },
            ),
        )
    }

    /// Hard-block further `browser_navigate` after repeated failures (stops SSRF/CDP retry loops).
    pub fn maybe_repeated_browser_nav_block(&self, tool_name: &str) -> Option<String> {
        if tool_name != "browser_navigate" {
            return None;
        }
        if self.browser_nav_success || self.browser_nav_failures < 3 {
            return None;
        }
        Some(edgecrab_tools::tool_loop_guardrails::guardrail_block_result(
            &edgecrab_tools::tool_loop_guardrails::ToolGuardrailDecision {
                action: edgecrab_tools::tool_loop_guardrails::GuardrailAction::Block,
                code: "browser_nav_loop_block",
                message: "Blocked repeated browser_navigate — prior attempts failed (SSRF, CDP, or \
                           dev server not running). Start `python3 -m http.server 8000` in the \
                           project directory, confirm security.preview is enabled, then navigate to \
                           http://127.0.0.1:8000/ once. Use browser_snapshot for visual proof."
                    .into(),
                tool_name: tool_name.to_string(),
                count: self.browser_nav_failures,
            },
        ))
    }

    pub fn act_tool_count_in_window(&self) -> usize {
        self.window
            .iter()
            .filter(|(_, name)| ACT_STORM_TOOLS.contains(&name.as_str()))
            .count()
    }

    fn window_lacks_perception(&self, task_class: TaskClass) -> bool {
        !self.window.iter().any(|(_, name)| {
            is_verification_tool_for_class(name, task_class)
                && PERCEPTION_TOOLS.contains(&name.as_str())
        })
    }

    /// True when act tools exceeded threshold with no perception (for hard blocks).
    pub fn is_act_storm_without_perception(&self, task_class: TaskClass) -> bool {
        if !matches!(task_class, TaskClass::VisualUx | TaskClass::CodeChange) {
            return false;
        }
        self.act_tool_count_in_window() >= self.storm_threshold
            && self.window_lacks_perception(task_class)
    }

    /// Warn when many act-class tools fire without perception (HA-20e).
    pub fn maybe_iteration_storm_advisory(&mut self, task_class: TaskClass) -> Option<String> {
        if !matches!(task_class, TaskClass::VisualUx | TaskClass::CodeChange) {
            return None;
        }
        if self
            .last_storm_advisory
            .is_some_and(|t| t.elapsed() < Duration::from_secs(120))
        {
            return None;
        }
        if !self.is_act_storm_without_perception(task_class) {
            return None;
        }
        let act_count = self.act_tool_count_in_window();
        self.last_storm_advisory = Some(Instant::now());
        tracing::warn!(
            act_tools = act_count,
            window_secs = self.window_secs,
            ?task_class,
            "harness: iteration storm without perception evidence"
        );
        Some(format!(
            "[harness] {act_count} mutation/discovery tools in the last {window}s without \
             verification — stop terminal/config debugging. For {class} tasks: enable preview \
             (/config), browser_navigate to the dev server URL, then browser_snapshot. Do not write \
             markdown verification reports.",
            window = self.window_secs,
            class = task_class_label(task_class),
        ))
    }
}

fn task_class_label(class: TaskClass) -> &'static str {
    match class {
        TaskClass::VisualUx => "visual_ux",
        TaskClass::CodeChange => "code_change",
        TaskClass::Research => "research",
        TaskClass::General => "general",
    }
}

/// Record tools from a turn and inject one-shot harness user messages (P0.6 / P1.9).
pub fn apply_harness_advisories(
    harness: &mut HarnessTurnAdvisory,
    messages: &mut Vec<edgecrab_types::Message>,
    tool_names: &[&str],
    browser_navigate_results: &[&str],
    known_dev_ports: &[u16],
) {
    for name in tool_names {
        harness.record_tool(name);
    }
    for result in browser_navigate_results {
        harness.record_browser_navigate_result(result);
        if let Some(recovery) =
            harness.maybe_preview_recovery("browser_navigate", result, known_dev_ports)
        {
            messages.push(edgecrab_types::Message::user(&recovery));
            break;
        }
    }
    let class = classify_from_messages(messages);
    if let Some(storm) = harness.maybe_iteration_storm_advisory(class) {
        messages.push(edgecrab_types::Message::user(&storm));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ha16_preview_recovery_on_cdp_failure() {
        let mut adv = HarnessTurnAdvisory::new();
        adv.record_browser_navigate_result(r#"{"success":false,"error":"CDP connection refused"}"#);
        let msg = adv
            .maybe_preview_recovery(
                "browser_navigate",
                r#"{"success":false,"error":"CDP connection refused"}"#,
                &[8000],
            )
            .expect("cdp recovery");
        assert!(msg.contains("browser_navigate"));
        assert!(
            adv.maybe_preview_recovery("browser_navigate", "again", &[])
                .is_none()
        );
    }

    #[test]
    fn verification_theater_blocks_execute_code_after_browser_failures() {
        let mut adv = HarnessTurnAdvisory::new();
        adv.record_browser_navigate_result(r#"{"success":false}"#);
        adv.record_browser_navigate_result(r#"{"success":false}"#);
        assert!(
            adv.maybe_verification_theater_block(TaskClass::VisualUx, "execute_code")
                .is_some()
        );
        assert!(
            adv.maybe_verification_theater_block(TaskClass::VisualUx, "write_file")
                .is_none()
        );
    }

    #[test]
    fn repeated_browser_nav_blocked_after_three_failures() {
        let mut adv = HarnessTurnAdvisory::new();
        adv.record_browser_navigate_result(r#"{"success":false}"#);
        adv.record_browser_navigate_result(r#"{"success":false}"#);
        assert!(
            adv.maybe_repeated_browser_nav_block("browser_navigate")
                .is_none()
        );
        adv.record_browser_navigate_result(r#"{"success":false}"#);
        let block = adv
            .maybe_repeated_browser_nav_block("browser_navigate")
            .expect("block");
        assert!(block.contains("browser_nav_loop_block") || block.contains("Blocked"));
        assert!(
            adv.maybe_repeated_browser_nav_block("browser_snapshot")
                .is_none()
        );
    }

    #[test]
    fn ha16_preview_recovery_once_per_session() {
        let mut adv = HarnessTurnAdvisory::new();
        let err = r#"{"error":"URL blocked by SSRF policy"}"#;
        assert!(
            adv.maybe_preview_recovery("browser_navigate", err, &[])
                .is_some()
        );
        assert!(
            adv.maybe_preview_recovery("browser_navigate", err, &[])
                .is_none()
        );
    }

    #[test]
    fn ha20c_preview_recovery_includes_detected_ports() {
        let mut adv = HarnessTurnAdvisory::new();
        let err = r#"{"error":"URL blocked by SSRF policy"}"#;
        let msg = adv
            .maybe_preview_recovery("browser_navigate", err, &[8000])
            .expect("recovery");
        assert!(msg.contains("127.0.0.1:8000"));
    }

    #[test]
    fn ha20e_storm_after_five_terminals_without_perception() {
        let mut adv = HarnessTurnAdvisory::new();
        for _ in 0..6 {
            adv.record_tool("terminal");
        }
        let msg = adv
            .maybe_iteration_storm_advisory(TaskClass::VisualUx)
            .expect("storm");
        assert!(msg.contains("without verification"));
    }

    #[test]
    fn apply_harness_injects_preview_and_storm() {
        use edgecrab_types::Message;

        let mut harness = HarnessTurnAdvisory::new();
        let mut messages = vec![Message::user("make demo/games003 beautiful UX")];
        let tool_names: Vec<&str> = std::iter::repeat("terminal").take(6).collect();
        apply_harness_advisories(
            &mut harness,
            &mut messages,
            &tool_names,
            &[r#"{"error":"URL blocked"}"#],
            &[8000],
        );
        let joined: String = messages
            .iter()
            .map(|m| m.text_content())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("/config") || joined.contains("preview"));
        assert!(joined.contains("without verification"));
    }

    #[test]
    fn perception_tool_clears_storm() {
        let mut adv = HarnessTurnAdvisory::new();
        for _ in 0..4 {
            adv.record_tool("terminal");
        }
        adv.record_tool("browser_navigate");
        adv.record_tool("terminal");
        assert!(
            adv.maybe_iteration_storm_advisory(TaskClass::VisualUx)
                .is_none()
        );
    }
}
