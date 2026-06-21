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
pub fn visual_storm_block_result(
    advisory: &HarnessTurnAdvisory,
    messages: &[Message],
    tool_name: &str,
) -> Option<String> {
    if !STORM_BLOCK_TOOLS.contains(&tool_name) {
        return None;
    }
    let class = classify_from_messages(messages);
    if class != TaskClass::VisualUx || !advisory.is_act_storm_without_perception(class) {
        return None;
    }
    Some(edgecrab_tools::tool_loop_guardrails::guardrail_block_result(
        &edgecrab_tools::tool_loop_guardrails::ToolGuardrailDecision {
            action: edgecrab_tools::tool_loop_guardrails::GuardrailAction::Block,
            code: "visual_storm_act_block",
            message: "Blocked shell/code tool — visual task needs browser evidence, not terminal debugging. \
                       Run /config set security.preview.enabled true if needed, start a dev server, \
                       then browser_navigate + browser_snapshot."
                .into(),
            tool_name: tool_name.to_string(),
            count: advisory.act_tool_count_in_window() as u32,
        },
    ))
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
        let messages = vec![Message::user("make demo/games003 beautiful UX")];
        assert!(visual_storm_block_result(&adv, &messages, "terminal").is_some());
        assert!(visual_storm_block_result(&adv, &messages, "write_file").is_none());
    }

    #[test]
    fn visual_storm_blocks_execute_code_after_threshold() {
        let mut adv = HarnessTurnAdvisory::new();
        for _ in 0..6 {
            adv.record_tool("execute_code");
        }
        let messages = vec![Message::user("make demo/games003 beautiful UX")];
        assert!(visual_storm_block_result(&adv, &messages, "execute_code").is_some());
    }
}
