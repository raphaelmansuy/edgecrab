//! Goal judge — auxiliary LLM call that decides whether a standing goal is done.
//!
//! Mirrors hermes-agent `hermes_cli/goals.py::judge_goal`. Fail-open on errors.

use std::sync::Arc;

use edgequake_llm::LLMProvider;

use crate::config::GoalJudgeConfig;
use crate::goals::GoalState;

pub const DEFAULT_JUDGE_MAX_TOKENS: u32 = 4096;
pub const DEFAULT_MAX_CONSECUTIVE_PARSE_FAILURES: u32 = 3;
const JUDGE_RESPONSE_SNIPPET_CHARS: usize = 4000;

const JUDGE_SYSTEM_PROMPT: &str = "\
You are a strict judge evaluating whether an autonomous agent has \
achieved a user's stated goal. You receive the goal text and the \
agent's most recent response. Your only job is to decide whether \
the goal is fully satisfied based on that response.\n\n\
A goal is DONE only when:\n\
- The response explicitly confirms the goal was completed, OR\n\
- The response clearly shows the final deliverable was produced, OR\n\
- The response explains the goal is unachievable / blocked / needs \
user input (treat this as DONE with reason describing the block).\n\n\
When a completion contract is present, DONE additionally requires \
concrete evidence that the verification criterion was met \
(command result, test output, file excerpt, or browser evidence) \
— never accept vibes or generic \"looks done\" claims.\n\n\
Otherwise the goal is NOT done — CONTINUE.\n\n\
Reply ONLY with a single JSON object on one line:\n\
{\"done\": <true|false>, \"reason\": \"<one-sentence rationale>\", \"wait\": <true|false>}\n\
Set wait=true when the agent is blocked on a long-running background process \
and should park (not busy-loop) until the next user turn.";

const JUDGE_USER_PROMPT: &str = "\
Goal:\n{goal}\n\n\
Agent's most recent response:\n{response}\n\n\
Current time: {current_time}\n\n\
Is the goal satisfied?";

const JUDGE_USER_PROMPT_WITH_SUBGOALS: &str = "\
Goal:\n{goal}\n\n\
Additional criteria the user added mid-loop (all must also be \
satisfied for the goal to be DONE):\n{subgoals_block}\n\n\
Agent's most recent response:\n{response}\n\n\
Current time: {current_time}\n\n\
Decision: For each numbered criterion above, find concrete \
evidence in the agent's response that the criterion is \
satisfied. Do not accept generic phrases like 'all requirements \
met' or 'implying it was done' — require specific evidence (a \
file contents excerpt, an output line, a command result). If \
ANY criterion lacks specific evidence in the response, the goal \
is NOT done — return CONTINUE.\n\n\
Is the goal AND every additional criterion satisfied?";

const JUDGE_USER_PROMPT_WITH_CONTRACT: &str = "\
Goal:\n{goal}\n\n\
Completion contract (evidence-based done):\n{contract_block}\n\n\
{extra_criteria}\
Agent's most recent response:\n{response}\n\n\
Current time: {current_time}\n\n\
Decision: Mark DONE only when the verification criterion is met \
with concrete evidence in the response (command/test output, file \
excerpt, or browser/screenshot evidence). Reject vibes. Respect \
constraints and boundaries. If stop_when applies, mark DONE with \
a block reason.\n\n\
Is the goal satisfied under the contract?";

/// Parsed judge verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalJudgeVerdict {
    pub done: bool,
    pub reason: String,
    pub parse_failed: bool,
    /// Park Ralph continuation (background work / wait-for-process).
    pub wait: bool,
}

/// Run the goal judge against the last assistant response.
pub async fn run_goal_judge(
    provider: &Arc<dyn LLMProvider>,
    _model: &str,
    goal: &str,
    last_response: &str,
    state: &GoalState,
    judge_cfg: &GoalJudgeConfig,
) -> GoalJudgeVerdict {
    if goal.trim().is_empty() {
        return GoalJudgeVerdict {
            done: false,
            reason: "empty goal".into(),
            parse_failed: false,
            wait: false,
        };
    }
    if last_response.trim().is_empty() {
        return GoalJudgeVerdict {
            done: false,
            reason: "empty response (nothing to evaluate)".into(),
            parse_failed: false,
            wait: false,
        };
    }

    let now = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string();
    let active_subgoals: Vec<String> = state
        .subgoals
        .iter()
        .map(|s| s.text.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    let user_prompt = if !state.contract.is_empty() {
        let contract_block = format_contract_block(&state.contract);
        let extra_criteria = if active_subgoals.is_empty() {
            String::new()
        } else {
            let subgoals_block = active_subgoals
                .iter()
                .enumerate()
                .map(|(i, text)| format!("- {}. {text}", i + 1))
                .collect::<Vec<_>>()
                .join("\n");
            format!("Additional subgoal criteria (all required):\n{subgoals_block}\n\n")
        };
        JUDGE_USER_PROMPT_WITH_CONTRACT
            .replace("{goal}", &truncate(goal, 2000))
            .replace("{contract_block}", &truncate(&contract_block, 2000))
            .replace("{extra_criteria}", &extra_criteria)
            .replace(
                "{response}",
                &truncate(last_response, JUDGE_RESPONSE_SNIPPET_CHARS),
            )
            .replace("{current_time}", &now)
    } else if active_subgoals.is_empty() {
        JUDGE_USER_PROMPT
            .replace("{goal}", &truncate(goal, 2000))
            .replace(
                "{response}",
                &truncate(last_response, JUDGE_RESPONSE_SNIPPET_CHARS),
            )
            .replace("{current_time}", &now)
    } else {
        let subgoals_block = active_subgoals
            .iter()
            .enumerate()
            .map(|(i, text)| format!("- {}. {text}", i + 1))
            .collect::<Vec<_>>()
            .join("\n");
        JUDGE_USER_PROMPT_WITH_SUBGOALS
            .replace("{goal}", &truncate(goal, 2000))
            .replace("{subgoals_block}", &truncate(&subgoals_block, 2000))
            .replace(
                "{response}",
                &truncate(last_response, JUDGE_RESPONSE_SNIPPET_CHARS),
            )
            .replace("{current_time}", &now)
    };

    let max_tokens = judge_cfg.max_tokens.max(1) as usize;
    let messages = vec![
        edgequake_llm::ChatMessage::system(JUDGE_SYSTEM_PROMPT),
        edgequake_llm::ChatMessage::user(&user_prompt),
    ];
    let options = edgequake_llm::CompletionOptions {
        max_tokens: Some(max_tokens),
        temperature: Some(0.0),
        ..Default::default()
    };

    let response = match provider
        .chat_with_tools(&messages, &[], None, Some(&options))
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            tracing::info!(
                error = %err,
                "goal judge: API call failed — falling through to continue"
            );
            return GoalJudgeVerdict {
                done: false,
                reason: format!("judge error: {err}"),
                parse_failed: false,
                wait: false,
            };
        }
    };

    let raw = response.content.trim().to_string();
    parse_judge_response(&raw)
}

/// Resolve `(provider, model)` for the goal judge (mirrors shadow_judge resolution).
pub fn resolve_goal_judge_provider_and_model(
    judge_cfg: &GoalJudgeConfig,
    auxiliary_model: Option<&str>,
    main_provider: Arc<dyn LLMProvider>,
    main_model: &str,
) -> (Arc<dyn LLMProvider>, String) {
    crate::auxiliary_model::resolve_side_task_provider_and_model(
        judge_cfg.model.as_deref(),
        auxiliary_model,
        main_provider,
        main_model,
        "goal judge",
    )
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let end = text
        .char_indices()
        .nth(limit)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len());
    format!("{}… [truncated]", &text[..end])
}

fn format_contract_block(contract: &edgecrab_types::GoalContract) -> String {
    let mut lines = Vec::new();
    if !contract.outcome.trim().is_empty() {
        lines.push(format!("- outcome: {}", contract.outcome.trim()));
    }
    if !contract.verification.trim().is_empty() {
        lines.push(format!("- verification: {}", contract.verification.trim()));
    }
    if !contract.constraints.trim().is_empty() {
        lines.push(format!("- constraints: {}", contract.constraints.trim()));
    }
    if !contract.boundaries.trim().is_empty() {
        lines.push(format!("- boundaries: {}", contract.boundaries.trim()));
    }
    if !contract.stop_when.trim().is_empty() {
        lines.push(format!("- stop_when: {}", contract.stop_when.trim()));
    }
    if lines.is_empty() {
        "(empty contract)".into()
    } else {
        lines.join("\n")
    }
}

/// Tools whose successful results may satisfy a goal contract verification criterion.
///
/// `report_task_status` is intentionally excluded — model-authored status is not proof.
const CONTRACT_EVIDENCE_TOOLS: &[&str] = &[
    "terminal",
    "run_process",
    "execute_code",
    "browser_navigate",
    "browser_snapshot",
    "browser_screenshot",
    "vision_analyze",
];

/// Code-is-law evidence: scan **tool results** for structured proof.
///
/// - `terminal` / `run_process`: `[terminal_result … exit_code=0]`; harness backend
///   always counts; agent runs need needle in body and must not be echo-gaming.
/// - Browser/vision: `StructuredBrowserResult.ok` when verification names perception.
/// - Free-text `.contains(needle)` on tool prose is never evidence (018 F2).
/// - Cheerleading / `report_task_status` alone never satisfy.
pub fn contract_evidence_in_messages(
    contract: &edgecrab_types::GoalContract,
    messages: &[edgecrab_types::Message],
) -> bool {
    if !contract.requires_verification_evidence() {
        return true;
    }
    let needle = contract.verification.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return true;
    }
    for msg in messages {
        if msg.role != edgecrab_types::Role::Tool {
            continue;
        }
        let Some(name) = msg.name.as_deref() else {
            continue;
        };
        if !CONTRACT_EVIDENCE_TOOLS.contains(&name) {
            continue;
        }
        let content = msg.text_content();
        if edgecrab_types::parse_tool_error_payload(&content).is_some() {
            continue;
        }
        if matches!(name, "terminal" | "run_process") {
            let Some(parsed) = edgecrab_tools::parse_terminal_result(&content) else {
                continue;
            };
            if parsed.exit_code != 0 {
                continue;
            }
            // Harness-run verification already executed the contract command.
            if parsed.backend == "harness" {
                return true;
            }
            if crate::contract_verify::looks_like_echo_gaming(parsed.body, &needle) {
                continue;
            }
            if parsed.body.to_ascii_lowercase().contains(&needle) {
                return true;
            }
            continue;
        }
        if matches!(
            name,
            "browser_navigate"
                | "browser_snapshot"
                | "browser_screenshot"
                | "browser_vision"
                | "vision_analyze"
        ) {
            let structured_ok =
                edgecrab_tools::structured_browser_nav_succeeded(&content).unwrap_or(false);
            if !structured_ok {
                continue;
            }
            // Verification must name a perception criterion — not arbitrary prose match.
            if needle.contains("browser")
                || needle.contains("snapshot")
                || needle.contains("screenshot")
                || needle.contains("vision")
                || needle.contains(name)
            {
                return true;
            }
            continue;
        }
        // execute_code: never free-text needle (code contracts use terminal/harness).
    }
    false
}

/// Parse judge JSON. Fail-open to `(done=false, reason, parse_failed=true)`.
pub fn parse_judge_response(raw: &str) -> GoalJudgeVerdict {
    if raw.trim().is_empty() {
        return GoalJudgeVerdict {
            done: false,
            reason: "judge returned empty response".into(),
            parse_failed: true,
            wait: false,
        };
    }

    let mut text = raw.trim().to_string();
    if text.starts_with("```") {
        text = text.trim_matches('`').to_string();
        if let Some(nl) = text.find('\n') {
            text = text[nl + 1..].to_string();
        }
    }

    let data = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .or_else(|| extract_json_object(&text));

    let Some(data) = data.filter(|v| v.is_object()) else {
        return GoalJudgeVerdict {
            done: false,
            reason: format!("judge reply was not JSON: {:?}", truncate(raw, 200)),
            parse_failed: true,
            wait: false,
        };
    };

    let done_val = &data["done"];
    let done = if let Some(s) = done_val.as_str() {
        matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "true" | "yes" | "1" | "done"
        )
    } else {
        done_val.as_bool().unwrap_or(false)
    };
    let reason = data["reason"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("no reason provided")
        .to_string();
    let wait = data["wait"].as_bool().unwrap_or(false);

    GoalJudgeVerdict {
        done,
        reason,
        parse_failed: false,
        wait,
    }
}

fn extract_json_object(text: &str) -> Option<serde_json::Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')? + 1;
    serde_json::from_str(&text[start..end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_done_json() {
        let v = parse_judge_response(r#"{"done": true, "reason": "Shipped feature"}"#);
        assert!(v.done);
        assert_eq!(v.reason, "Shipped feature");
        assert!(!v.parse_failed);
    }

    #[test]
    fn parse_continue_json() {
        let v = parse_judge_response(r#"{"done": false, "reason": "Still working"}"#);
        assert!(!v.done);
        assert!(!v.parse_failed);
    }

    #[test]
    fn parse_empty_is_parse_failure() {
        let v = parse_judge_response("");
        assert!(!v.done);
        assert!(v.parse_failed);
    }

    #[test]
    fn parse_strips_markdown_fence() {
        let v = parse_judge_response("```json\n{\"done\": true, \"reason\": \"ok\"}\n```");
        assert!(v.done);
        assert!(!v.parse_failed);
    }

    #[test]
    fn parse_prose_wrapped_json() {
        let v = parse_judge_response(
            "Here is my verdict: {\"done\": false, \"reason\": \"needs tests\"}",
        );
        assert!(!v.done);
        assert_eq!(v.reason, "needs tests");
    }

    #[test]
    fn contract_evidence_requires_tool_result_not_prose() {
        let contract = edgecrab_types::GoalContract {
            verification: "cargo test -p edgecrab-core".into(),
            ..Default::default()
        };
        let prose_only = vec![edgecrab_types::Message::assistant(
            "Ran cargo test -p edgecrab-core — all green.",
        )];
        assert!(!contract_evidence_in_messages(&contract, &prose_only));
        let with_tool = vec![
            edgecrab_types::Message::assistant("running"),
            edgecrab_types::Message::tool_result(
                "t1",
                "terminal",
                "[terminal_result status=success backend=local cwd=/tmp exit_code=0]\n\
                 cargo test -p edgecrab-core\nok",
            ),
        ];
        assert!(contract_evidence_in_messages(&contract, &with_tool));
    }

    #[test]
    fn contract_evidence_rejects_failed_terminal_even_with_needle() {
        let contract = edgecrab_types::GoalContract {
            verification: "cargo test".into(),
            ..Default::default()
        };
        let failed = vec![edgecrab_types::Message::tool_result(
            "t1",
            "terminal",
            "[terminal_result status=error backend=local cwd=/tmp exit_code=1]\n\
             cargo test\nFAILED",
        )];
        assert!(!contract_evidence_in_messages(&contract, &failed));
    }

    #[test]
    fn contract_evidence_rejects_report_task_status_alone() {
        let contract = edgecrab_types::GoalContract {
            verification: "cargo test".into(),
            ..Default::default()
        };
        let status = vec![edgecrab_types::Message::tool_result(
            "r1",
            "report_task_status",
            r#"{"status":"completed","summary":"cargo test passed","evidence":["cargo test"]}"#,
        )];
        assert!(!contract_evidence_in_messages(&contract, &status));
    }

    #[test]
    fn contract_evidence_rejects_headerless_terminal_blob() {
        let contract = edgecrab_types::GoalContract {
            verification: "cargo test".into(),
            ..Default::default()
        };
        let blob = vec![edgecrab_types::Message::tool_result(
            "t1",
            "terminal",
            "cargo test\nok",
        )];
        assert!(!contract_evidence_in_messages(&contract, &blob));
    }

    #[test]
    fn contract_evidence_harness_backend_counts_without_needle() {
        let contract = edgecrab_types::GoalContract {
            verification: "cargo test -p edgecrab-core".into(),
            ..Default::default()
        };
        let msgs = vec![edgecrab_types::Message::tool_result(
            "t1",
            "terminal",
            "[terminal_result status=success backend=harness cwd=/tmp exit_code=0]\nok\n",
        )];
        assert!(contract_evidence_in_messages(&contract, &msgs));
    }

    #[test]
    fn contract_evidence_rejects_browser_prose_needle() {
        let contract = edgecrab_types::GoalContract {
            verification: "browser_snapshot shows chess board".into(),
            ..Default::default()
        };
        let prose = vec![edgecrab_types::Message::tool_result(
            "s1",
            "browser_snapshot",
            "browser_snapshot shows chess board with pieces",
        )];
        assert!(!contract_evidence_in_messages(&contract, &prose));

        let structured = edgecrab_tools::StructuredBrowserResult::snapshot_ok(
            "http://127.0.0.1:8000/",
            "Board @e1 @e2",
            Some(4),
        )
        .to_tool_result_text();
        let ok = vec![edgecrab_types::Message::tool_result(
            "s1",
            "browser_snapshot",
            &structured,
        )];
        assert!(contract_evidence_in_messages(&contract, &ok));
    }
}
