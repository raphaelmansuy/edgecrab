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
        // First principles: structured browser JSON or typed tool_error only.
        if let Some(ok) = edgecrab_tools::structured_browser_nav_succeeded(tool_result) {
            if ok {
                self.browser_nav_success = true;
            } else {
                self.browser_nav_failures = self.browser_nav_failures.saturating_add(1);
            }
            return;
        }
        if edgecrab_types::parse_tool_error_payload(tool_result).is_some() {
            self.browser_nav_failures = self.browser_nav_failures.saturating_add(1);
        }
        // Unstructured prose does not update storm counters.
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
        // Typed signals only: tool_error codes / structured chrome-error / storm debt.
        let payload = edgecrab_types::parse_tool_error_payload(tool_result);
        // Typed ToolError codes/categories only — not free-text `.contains("ssrf")`.
        let blocked = payload.as_ref().is_some_and(|p| {
            let code = p.code.to_ascii_lowercase();
            let cat = p.category.to_ascii_lowercase();
            matches!(
                code.as_str(),
                "ssrf_blocked"
                    | "ssrf"
                    | "scheme_not_allowed"
                    | "url_scheme_blocked"
                    | "capability_denied"
            ) || cat == "security"
                || code.starts_with("ssrf")
                || code.contains("scheme_not_allowed")
        });
        let structured_fail = edgecrab_tools::parse_structured_browser_result(tool_result)
            .is_some_and(|r| r.is_chrome_error || !r.ok);
        let failed_nav = self.browser_nav_failures > 0 && !self.browser_nav_success;
        if !blocked && !structured_fail && !failed_nav && payload.is_none() {
            return None;
        }
        self.preview_recovery_sent = true;
        if let Some(hint) = edgecrab_tools::dev_server::format_dev_server_ports_hint(known_ports) {
            return Some(format!(
                "[harness] browser verification unavailable — do not use file://, execute_code screenshots, \
                 or read ~/.edgecrab/config.yaml. {hint} Call browser_navigate to that URL, then \
                 browser_snapshot. Do not try other localhost ports. Do not write markdown \
                 verification reports."
            ));
        }
        let serve_dir = edgecrab_tools::recovery_catalog::infer_preview_serve_directory_from_text(
            // best-effort: recovery text itself carries no path; callers with messages
            // should prefer CallToolFirst JSON from browser_navigate errors.
            "",
        );
        let recipe =
            edgecrab_tools::recovery_catalog::preview_serve_then_navigate_recipe(&serve_dir);
        let command = recipe
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("python3 -m http.server 8000 --directory .");
        Some(format!(
            "[harness] browser verification unavailable — no session HTTP server recorded. \
             Do not guess localhost ports (8080/5050/5000/…). Call terminal with \
             `{command}` (background), then browser_navigate http://127.0.0.1:8000/ and \
             browser_snapshot. Ensure security.preview.enabled. Do not write markdown \
             verification reports."
        ))
    }

    /// After one failed loopback navigate with no session server, block further
    /// localhost port shopping until a port is recorded.
    pub fn maybe_loopback_port_shopping_block(
        &self,
        tool_name: &str,
        args_json: &str,
        session_ports: &[u16],
        serve_directory: &str,
    ) -> Option<String> {
        if tool_name != "browser_navigate" || !session_ports.is_empty() {
            return None;
        }
        if self.browser_nav_success || self.browser_nav_failures < 1 {
            return None;
        }
        let url = serde_json::from_str::<serde_json::Value>(args_json)
            .ok()
            .and_then(|v| v.get("url").and_then(|u| u.as_str()).map(str::to_string))?;
        if !is_loopback_http_url(&url) {
            return None;
        }
        let recipe =
            edgecrab_tools::recovery_catalog::preview_serve_then_navigate_recipe(serve_directory);
        let command = recipe
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("python3 -m http.server 8000 --directory .");
        Some(edgecrab_tools::tool_loop_guardrails::guardrail_block_result(
            &edgecrab_tools::tool_loop_guardrails::ToolGuardrailDecision {
                action: edgecrab_tools::tool_loop_guardrails::GuardrailAction::Block,
                code: "loopback_port_shopping_block",
                message: format!(
                    "Blocked localhost browser_navigate — no session HTTP server is recorded. \
                     Do not try other ports. Call terminal with `{command}`, then \
                     browser_navigate http://127.0.0.1:8000/ once."
                ),
                tool_name: tool_name.to_string(),
                count: self.browser_nav_failures,
            },
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
                           Call terminal with `python3 -m http.server 8000 --directory <demo-dir>`, \
                           then browser_navigate http://127.0.0.1:8000/ and browser_snapshot. \
                           Do not try other localhost ports. Do not script screenshots via execute_code."
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
                           no session HTTP server). Do not try other localhost ports. Start \
                           `python3 -m http.server 8000 --directory <demo-dir>`, confirm \
                           security.preview is enabled, then navigate to http://127.0.0.1:8000/ \
                           once. Use browser_snapshot for visual proof."
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
        // Failed browser_navigate (SSRF/preview block) must not count as perception —
        // otherwise terminal storms after a blocked preview never hard-stop (0aeef965).
        !self.window.iter().any(|(_, name)| {
            if !is_verification_tool_for_class(name, task_class)
                || !PERCEPTION_TOOLS.contains(&name.as_str())
            {
                return false;
            }
            if name == "browser_navigate" {
                return self.browser_nav_success;
            }
            true
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
        let recipe = match task_class {
            TaskClass::VisualUx => {
                "enable preview (/config), browser_navigate to the dev server URL, then \
                 browser_snapshot. Do not write markdown verification reports."
            }
            TaskClass::CodeChange => {
                "run compile/test gates (cargo test / npm test / pytest) before claiming done. \
                 Do not open a browser or spawn a static preview server for code_change tasks."
            }
            _ => "gather class-appropriate verification evidence before claiming done.",
        };
        Some(format!(
            "[harness] {act_count} mutation/discovery tools in the last {window}s without \
             verification — stop terminal/config debugging. For {class} tasks: {recipe}",
            window = self.window_secs,
            class = task_class_label(task_class),
        ))
    }
}

fn task_class_label(class: TaskClass) -> &'static str {
    match class {
        TaskClass::VisualUx => "visual_ux",
        TaskClass::Document => "document",
        TaskClass::CodeChange => "code_change",
        TaskClass::Research => "research",
        TaskClass::General => "general",
    }
}

fn is_loopback_http_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    match parsed.host_str() {
        Some("localhost") | Some("127.0.0.1") | Some("::1") => true,
        Some(h) => h.eq_ignore_ascii_case("localhost"),
        None => false,
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
        let fail = edgecrab_tools::StructuredBrowserResult::navigate_err(
            "http://127.0.0.1:8000/",
            "CDP connection refused",
        )
        .to_tool_result_json();
        adv.record_browser_navigate_result(&fail);
        let msg = adv
            .maybe_preview_recovery("browser_navigate", &fail, &[8000])
            .expect("cdp recovery");
        assert!(msg.contains("browser_navigate"));
        assert!(
            adv.maybe_preview_recovery("browser_navigate", "again", &[])
                .is_none()
        );
    }

    fn tool_error_json(code: &str, error: &str) -> String {
        serde_json::json!({
            "type": "tool_error",
            "category": "execution",
            "code": code,
            "code_num": 1006,
            "error": error,
            "retryable": true,
            "suppress_retry": false,
        })
        .to_string()
    }

    fn structured_nav_fail() -> String {
        edgecrab_tools::StructuredBrowserResult::navigate_err(
            "http://127.0.0.1:8000/",
            "Navigation error: net::ERR_CONNECTION_REFUSED",
        )
        .to_tool_result_json()
    }

    #[test]
    fn verification_theater_blocks_execute_code_after_browser_failures() {
        let mut adv = HarnessTurnAdvisory::new();
        let fail = structured_nav_fail();
        adv.record_browser_navigate_result(&fail);
        adv.record_browser_navigate_result(&fail);
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
        let fail = structured_nav_fail();
        adv.record_browser_navigate_result(&fail);
        adv.record_browser_navigate_result(&fail);
        assert!(
            adv.maybe_repeated_browser_nav_block("browser_navigate")
                .is_none()
        );
        adv.record_browser_navigate_result(&fail);
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
        let err = tool_error_json("ssrf_blocked", "URL blocked by SSRF policy");
        assert!(
            adv.maybe_preview_recovery("browser_navigate", &err, &[])
                .is_some()
        );
        assert!(
            adv.maybe_preview_recovery("browser_navigate", &err, &[])
                .is_none()
        );
    }

    #[test]
    fn ha20c_preview_recovery_includes_detected_ports() {
        let mut adv = HarnessTurnAdvisory::new();
        let err = tool_error_json("ssrf_blocked", "URL blocked by SSRF policy");
        let msg = adv
            .maybe_preview_recovery("browser_navigate", &err, &[8000])
            .expect("recovery");
        assert!(msg.contains("127.0.0.1:8000"));
    }

    #[test]
    fn empty_ports_preview_recovery_forbids_port_shopping() {
        let mut adv = HarnessTurnAdvisory::new();
        let err = tool_error_json("execution_failed", "connection refused");
        let msg = adv
            .maybe_preview_recovery("browser_navigate", &err, &[])
            .expect("recovery");
        assert!(msg.contains("http.server"));
        assert!(msg.contains("127.0.0.1:8000"));
        assert!(
            msg.contains("Do not guess") || msg.contains("do not guess"),
            "must forbid port guessing: {msg}"
        );
    }

    #[test]
    fn loopback_port_shopping_blocked_after_one_failure() {
        let mut adv = HarnessTurnAdvisory::new();
        assert!(
            adv.maybe_loopback_port_shopping_block(
                "browser_navigate",
                r#"{"url":"http://127.0.0.1:8080/"}"#,
                &[],
                "demo/game002",
            )
            .is_none()
        );
        adv.record_browser_navigate_result(&structured_nav_fail());
        let block = adv
            .maybe_loopback_port_shopping_block(
                "browser_navigate",
                r#"{"url":"http://127.0.0.1:5050/"}"#,
                &[],
                "demo/game002",
            )
            .expect("block port shopping");
        assert!(block.contains("loopback_port_shopping_block") || block.contains("Blocked"));
        assert!(block.contains("demo/game002") || block.contains("http.server"));
        // Once a session port exists, shopping block lifts.
        assert!(
            adv.maybe_loopback_port_shopping_block(
                "browser_navigate",
                r#"{"url":"http://127.0.0.1:8000/"}"#,
                &[8000],
                "demo/game002",
            )
            .is_none()
        );
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
        assert!(msg.contains("browser_navigate") || msg.contains("browser_snapshot"));
    }

    #[test]
    fn code_change_storm_does_not_prescribe_browser() {
        let mut adv = HarnessTurnAdvisory::new();
        for _ in 0..6 {
            adv.record_tool("terminal");
        }
        let msg = adv
            .maybe_iteration_storm_advisory(TaskClass::CodeChange)
            .expect("storm");
        assert!(msg.contains("code_change") || msg.contains("compile") || msg.contains("test"));
        assert!(
            !msg.contains("browser_navigate"),
            "CodeChange must not prescribe browser theater: {msg}"
        );
        assert!(!msg.contains("dev server"));
    }

    #[test]
    fn apply_harness_injects_preview_and_storm() {
        use edgecrab_types::Message;

        let mut harness = HarnessTurnAdvisory::new();
        let mut messages = vec![Message::user("make demo/games003/index.html beautiful UX")];
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
        // VisualUx perception = snapshot/vision (navigate alone is not verification).
        adv.record_tool("browser_snapshot");
        adv.record_tool("terminal");
        assert!(
            adv.maybe_iteration_storm_advisory(TaskClass::VisualUx)
                .is_none()
        );
    }
}
