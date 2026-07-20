//! Harness loop policy — single owner for guardrail config and visual-storm blocks (DRY / SOLID).

use edgecrab_tools::tool_loop_guardrails::{ToolLoopGuardrailConfig, ToolLoopGuardrailController};
use edgecrab_types::Message;

use crate::config::HarnessConfig;
use crate::harness_advisory::HarnessTurnAdvisory;
use crate::task_class::{TaskClass, classify_from_messages};

/// Resolve per-turn tool-loop guardrail settings from harness config.
///
/// Default: hard stops **on** — usefulness over infinite retry loops on cheap models.
pub fn resolve_guardrail_config(harness: &HarnessConfig) -> ToolLoopGuardrailConfig {
    if !harness.guardrails_hard_stop {
        return ToolLoopGuardrailConfig {
            hard_stop_enabled: false,
            ..ToolLoopGuardrailConfig::default()
        };
    }
    ToolLoopGuardrailConfig {
        hard_stop_enabled: true,
        exact_failure_block_after: 4,
        same_tool_failure_halt_after: 6,
        ..ToolLoopGuardrailConfig::default()
    }
}

const STORM_BLOCK_TOOLS: &[&str] = &["terminal", "run_process", "execute_code"];

/// Block act-without-perceive storms on visual tasks (terminal, shell, sandbox code).
///
/// Preview-server starts (`python -m http.server`, `npx serve`, …) are exempt —
/// browser perception requires a listening HTTP server first (game002 deadlock).
pub fn visual_storm_block_result(
    advisory: &HarnessTurnAdvisory,
    messages: &[Message],
    tool_name: &str,
) -> Option<String> {
    visual_storm_block_result_with_args(advisory, messages, tool_name, "")
}

/// Like [`visual_storm_block_result`] but inspects tool args for preview-server exemption.
pub fn visual_storm_block_result_with_args(
    advisory: &HarnessTurnAdvisory,
    messages: &[Message],
    tool_name: &str,
    args_json: &str,
) -> Option<String> {
    if !STORM_BLOCK_TOOLS.contains(&tool_name) {
        return None;
    }
    // Capability law: serve before perceive — never block the serve step.
    if matches!(tool_name, "terminal" | "run_process" | "execute_code")
        && let Some(cmd) = edgecrab_tools::dev_server::command_from_tool_args_json(args_json)
        && edgecrab_tools::dev_server::is_preview_server_command(&cmd)
    {
        return None;
    }
    // execute_code: also scan raw args for embedded http.server snippets.
    if tool_name == "execute_code"
        && edgecrab_tools::dev_server::is_preview_server_command(args_json)
    {
        return None;
    }
    let class = classify_from_messages(messages);
    if class != TaskClass::VisualUx || !advisory.is_act_storm_without_perception(class) {
        return None;
    }
    Some(
        edgecrab_tools::tool_loop_guardrails::guardrail_block_result(
            &edgecrab_tools::tool_loop_guardrails::ToolGuardrailDecision {
                action: edgecrab_tools::tool_loop_guardrails::GuardrailAction::Block,
                code: "visual_storm_act_block",
                message: "Blocked shell/code debugging — visual task needs browser evidence. \
                       To preview: call terminal with \
                       `python3 -m http.server 8000 --directory <demo-dir>` \
                       (preview-server starts are allowed), then \
                       browser_navigate http://127.0.0.1:8000/ and browser_snapshot. \
                       Do not try other localhost ports. Do not ls/cat/grep-storm."
                    .into(),
                tool_name: tool_name.to_string(),
                count: advisory.act_tool_count_in_window() as u32,
            },
        ),
    )
}

/// If the guardrail controller recorded a halt, return an operator-facing steer message.
pub fn consume_guardrail_halt_message(
    guardrail: &mut ToolLoopGuardrailController,
) -> Option<String> {
    guardrail.take_halt_decision().map(|d| {
        format!(
            "[harness] Tool loop halted ({}, {}× {}): {} \
             Change strategy — do not retry the same failing tool.",
            d.code, d.count, d.tool_name, d.message
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::Message;

    #[test]
    fn guardrails_hard_stop_default_on() {
        let cfg = resolve_guardrail_config(&HarnessConfig::default());
        assert!(cfg.hard_stop_enabled);
    }

    #[test]
    fn visual_storm_blocks_terminal_after_threshold() {
        let mut adv = HarnessTurnAdvisory::new();
        for _ in 0..6 {
            adv.record_tool("terminal");
        }
        let messages = vec![Message::user("make demo/games003/index.html beautiful UX")];
        assert!(
            visual_storm_block_result_with_args(
                &adv,
                &messages,
                "terminal",
                r#"{"command":"ls -la"}"#
            )
            .is_some()
        );
        assert!(visual_storm_block_result(&adv, &messages, "write_file").is_none());
    }

    #[test]
    fn visual_storm_blocks_execute_code_after_threshold() {
        let mut adv = HarnessTurnAdvisory::new();
        for _ in 0..6 {
            adv.record_tool("execute_code");
        }
        let messages = vec![Message::user("make demo/games003/index.html beautiful UX")];
        assert!(
            visual_storm_block_result_with_args(
                &adv,
                &messages,
                "execute_code",
                r#"{"code":"print('hi')"}"#
            )
            .is_some()
        );
    }

    #[test]
    fn visual_storm_allows_http_server_after_write_storm() {
        let mut adv = HarnessTurnAdvisory::new();
        for _ in 0..6 {
            adv.record_tool("write_file");
        }
        let messages = vec![Message::user(
            "build a 3D html game in demo/game002/index.html",
        )];
        assert!(
            visual_storm_block_result_with_args(
                &adv,
                &messages,
                "terminal",
                r#"{"command":"python3 -m http.server 8000 --directory demo/game002","background":true}"#
            )
            .is_none(),
            "preview-server start must be exempt from visual storm block"
        );
        assert!(
            visual_storm_block_result_with_args(
                &adv,
                &messages,
                "terminal",
                r#"{"command":"ls -la demo/game002"}"#
            )
            .is_some(),
            "non-preview terminal must still block under storm"
        );
    }
}
