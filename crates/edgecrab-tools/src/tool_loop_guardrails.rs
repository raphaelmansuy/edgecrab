//! Per-turn tool-call loop guardrails — Hermes `tool_guardrails.py` parity + EdgeCrab extensions.
//!
//! Tracks repeated identical failures and idempotent no-progress loops within one
//! assistant turn. Warnings append guidance; hard-stop is opt-in via config later.

use std::collections::HashMap;

use serde_json::Value;

use crate::tool_argument_pipeline::canonical_tool_args_json;

/// Tools whose identical successful results indicate no progress when repeated.
const IDEMPOTENT_TOOLS: &[&str] = &[
    "read_file",
    "search_files",
    "file_search",
    "web_search",
    "web_extract",
    "session_search",
    "browser_snapshot",
    "browser_navigate",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardrailAction {
    Allow,
    Warn,
    Block,
    Halt,
}

#[derive(Debug, Clone)]
pub struct ToolGuardrailDecision {
    pub action: GuardrailAction,
    pub code: &'static str,
    pub message: String,
    pub tool_name: String,
    pub count: u32,
}

impl ToolGuardrailDecision {
    pub fn allow(tool_name: &str) -> Self {
        Self {
            action: GuardrailAction::Allow,
            code: "allow",
            message: String::new(),
            tool_name: tool_name.to_string(),
            count: 0,
        }
    }

    pub fn allows_execution(&self) -> bool {
        matches!(self.action, GuardrailAction::Allow | GuardrailAction::Warn)
    }
}

#[derive(Debug)]
pub struct ToolLoopGuardrailConfig {
    pub warnings_enabled: bool,
    pub hard_stop_enabled: bool,
    pub exact_failure_warn_after: u32,
    pub exact_failure_block_after: u32,
    pub no_progress_warn_after: u32,
    pub no_progress_block_after: u32,
    pub same_tool_failure_warn_after: u32,
    pub same_tool_failure_halt_after: u32,
}

impl Default for ToolLoopGuardrailConfig {
    fn default() -> Self {
        Self {
            warnings_enabled: true,
            hard_stop_enabled: true,
            exact_failure_warn_after: 2,
            exact_failure_block_after: 4,
            no_progress_warn_after: 2,
            no_progress_block_after: 4,
            same_tool_failure_warn_after: 3,
            same_tool_failure_halt_after: 6,
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct ToolSignature {
    tool_name: String,
    args_hash: String,
}

impl ToolSignature {
    fn from_call(tool_name: &str, args_json: &str) -> Self {
        let parsed = serde_json::from_str::<Value>(args_json).unwrap_or(Value::Null);
        let canonical = canonical_tool_args_json(&parsed);
        Self {
            tool_name: tool_name.to_string(),
            args_hash: hash_text(&canonical),
        }
    }
}

fn hash_text(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    format!("{digest:x}")
}

fn result_hash(result: &str) -> String {
    if let Ok(val) = serde_json::from_str::<Value>(result) {
        hash_text(&canonical_tool_args_json(&val))
    } else {
        hash_text(result)
    }
}

fn is_idempotent_tool(name: &str) -> bool {
    IDEMPOTENT_TOOLS.contains(&name)
}

pub fn classify_tool_failure(tool_name: &str, result: &str) -> bool {
    if result.trim().is_empty() {
        return false;
    }
    if tool_name == "terminal"
        && let Ok(val) = serde_json::from_str::<Value>(result)
        && let Some(code) = val.get("exit_code").and_then(|v| v.as_i64())
        && code != 0
    {
        return true;
    }
    let lower = result.get(..500).unwrap_or(result).to_ascii_lowercase();
    lower.contains("\"error\"")
        || lower.contains("\"failed\"")
        || result.starts_with("Error")
        || result.starts_with("BLOCKED:")
}

/// Per-turn controller — reset at each new assistant tool batch.
#[derive(Debug)]
pub struct ToolLoopGuardrailController {
    config: ToolLoopGuardrailConfig,
    exact_failure_counts: HashMap<ToolSignature, u32>,
    same_tool_failure_counts: HashMap<String, u32>,
    no_progress: HashMap<ToolSignature, (String, u32)>,
    halt_decision: Option<ToolGuardrailDecision>,
}

impl Default for ToolLoopGuardrailController {
    fn default() -> Self {
        Self::new(ToolLoopGuardrailConfig::default())
    }
}

impl ToolLoopGuardrailController {
    pub fn new(config: ToolLoopGuardrailConfig) -> Self {
        Self {
            config,
            exact_failure_counts: HashMap::new(),
            same_tool_failure_counts: HashMap::new(),
            no_progress: HashMap::new(),
            halt_decision: None,
        }
    }

    pub fn reset_for_turn(&mut self) {
        self.exact_failure_counts.clear();
        self.same_tool_failure_counts.clear();
        self.no_progress.clear();
        self.halt_decision = None;
    }

    pub fn take_halt_decision(&mut self) -> Option<ToolGuardrailDecision> {
        self.halt_decision.take()
    }

    pub fn before_call(&self, tool_name: &str, args_json: &str) -> ToolGuardrailDecision {
        let signature = ToolSignature::from_call(tool_name, args_json);
        if !self.config.hard_stop_enabled {
            return ToolGuardrailDecision::allow(tool_name);
        }
        let exact = self
            .exact_failure_counts
            .get(&signature)
            .copied()
            .unwrap_or(0);
        if exact >= self.config.exact_failure_block_after {
            return ToolGuardrailDecision {
                action: GuardrailAction::Block,
                code: "repeated_exact_failure_block",
                message: format!(
                    "Blocked {tool_name}: identical arguments failed {exact} times. \
                     Change strategy instead of retrying unchanged."
                ),
                tool_name: tool_name.to_string(),
                count: exact,
            };
        }
        if is_idempotent_tool(tool_name)
            && let Some((_, repeat)) = self.no_progress.get(&signature)
            && *repeat >= self.config.no_progress_block_after
        {
            return ToolGuardrailDecision {
                action: GuardrailAction::Block,
                code: "idempotent_no_progress_block",
                message: format!(
                    "Blocked {tool_name}: same result returned {repeat} times. \
                     Use the content already provided or change the query."
                ),
                tool_name: tool_name.to_string(),
                count: *repeat,
            };
        }
        ToolGuardrailDecision::allow(tool_name)
    }

    pub fn after_call(
        &mut self,
        tool_name: &str,
        args_json: &str,
        result: &str,
        failed: Option<bool>,
    ) -> ToolGuardrailDecision {
        let signature = ToolSignature::from_call(tool_name, args_json);
        let failed = failed.unwrap_or_else(|| classify_tool_failure(tool_name, result));

        if failed {
            let exact = self
                .exact_failure_counts
                .entry(signature.clone())
                .or_insert(0);
            *exact += 1;
            let exact_count = *exact;
            self.no_progress.remove(&signature);

            let same = self
                .same_tool_failure_counts
                .entry(tool_name.to_string())
                .or_insert(0);
            *same += 1;
            let same_count = *same;

            if self.config.hard_stop_enabled
                && same_count >= self.config.same_tool_failure_halt_after
            {
                let decision = ToolGuardrailDecision {
                    action: GuardrailAction::Halt,
                    code: "same_tool_failure_halt",
                    message: format!(
                        "Stopped {tool_name}: failed {same_count} times this turn. \
                         Diagnose the error and use a different approach."
                    ),
                    tool_name: tool_name.to_string(),
                    count: same_count,
                };
                self.halt_decision = Some(decision.clone());
                return decision;
            }

            if self.config.warnings_enabled && exact_count >= self.config.exact_failure_warn_after {
                return ToolGuardrailDecision {
                    action: GuardrailAction::Warn,
                    code: "repeated_exact_failure_warning",
                    message: format!(
                        "{tool_name} failed {exact_count} times with identical arguments — \
                         inspect the error before retrying unchanged."
                    ),
                    tool_name: tool_name.to_string(),
                    count: exact_count,
                };
            }

            if self.config.warnings_enabled
                && same_count >= self.config.same_tool_failure_warn_after
            {
                return ToolGuardrailDecision {
                    action: GuardrailAction::Warn,
                    code: "same_tool_failure_warning",
                    message: format!(
                        "{tool_name} failed {same_count} times this turn — likely a loop."
                    ),
                    tool_name: tool_name.to_string(),
                    count: same_count,
                };
            }

            return ToolGuardrailDecision::allow(tool_name);
        }

        self.exact_failure_counts.remove(&signature);
        self.same_tool_failure_counts.remove(tool_name);

        if !is_idempotent_tool(tool_name) {
            self.no_progress.remove(&signature);
            return ToolGuardrailDecision::allow(tool_name);
        }

        let hash = result_hash(result);
        let repeat = self
            .no_progress
            .get(&signature)
            .filter(|(h, _)| *h == hash)
            .map(|(_, c)| c + 1)
            .unwrap_or(1);
        self.no_progress.insert(signature, (hash, repeat));

        if self.config.warnings_enabled && repeat >= self.config.no_progress_warn_after {
            return ToolGuardrailDecision {
                action: GuardrailAction::Warn,
                code: "idempotent_no_progress_warning",
                message: format!(
                    "{tool_name} returned the same result {repeat} times — use existing output \
                     or change arguments."
                ),
                tool_name: tool_name.to_string(),
                count: repeat,
            };
        }

        ToolGuardrailDecision::allow(tool_name)
    }
}

pub fn append_guardrail_guidance(result: &str, decision: &ToolGuardrailDecision) -> String {
    if !matches!(
        decision.action,
        GuardrailAction::Warn | GuardrailAction::Halt
    ) || decision.message.is_empty()
    {
        return result.to_string();
    }
    let label = if decision.action == GuardrailAction::Halt {
        "Tool loop hard stop"
    } else {
        "Tool loop warning"
    };
    format!(
        "{result}\n\n[{label}: {}; count={}; {}]",
        decision.code, decision.count, decision.message
    )
}

pub fn guardrail_block_result(decision: &ToolGuardrailDecision) -> String {
    let action = match decision.action {
        GuardrailAction::Allow => "allow",
        GuardrailAction::Warn => "warn",
        GuardrailAction::Block => "block",
        GuardrailAction::Halt => "halt",
    };
    serde_json::json!({
        "error": decision.message,
        "guardrail": {
            "action": action,
            "code": decision.code,
            "count": decision.count,
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_on_repeated_exact_failure() {
        let mut ctrl = ToolLoopGuardrailController::new(ToolLoopGuardrailConfig::default());
        let args = r#"{"path":"x.rs"}"#;
        let d = ctrl.after_call("read_file", args, r#"{"error":"not found"}"#, None);
        assert_eq!(d.action, GuardrailAction::Allow);
        let d = ctrl.after_call("read_file", args, r#"{"error":"not found"}"#, None);
        assert_eq!(d.action, GuardrailAction::Warn);
        assert_eq!(d.code, "repeated_exact_failure_warning");
    }

    #[test]
    fn warns_on_idempotent_no_progress() {
        let mut ctrl = ToolLoopGuardrailController::new(ToolLoopGuardrailConfig::default());
        let args = r#"{"path":"a.txt"}"#;
        let body = "line1\nline2";
        ctrl.after_call("read_file", args, body, Some(false));
        let d = ctrl.after_call("read_file", args, body, Some(false));
        assert_eq!(d.action, GuardrailAction::Warn);
        assert_eq!(d.code, "idempotent_no_progress_warning");
    }

    #[test]
    fn hard_stop_blocks_identical_failure() {
        let mut cfg = ToolLoopGuardrailConfig::default();
        cfg.hard_stop_enabled = true;
        cfg.exact_failure_block_after = 2;
        let mut ctrl = ToolLoopGuardrailController::new(cfg);
        let args = r#"{"command":"false"}"#;
        ctrl.after_call("terminal", args, r#"{"exit_code":1}"#, None);
        ctrl.after_call("terminal", args, r#"{"exit_code":1}"#, None);
        let before = ctrl.before_call("terminal", args);
        assert_eq!(before.action, GuardrailAction::Block);
    }
}
