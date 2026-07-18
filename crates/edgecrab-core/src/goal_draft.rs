//! `/goal draft` — expand a one-liner into a completion contract (Hermes mechanism).
//!
//! Fail-open: if the aux model is unavailable or returns garbage, callers use free-form
//! `goal_set` on the original text.

use std::sync::Arc;

use edgequake_llm::LLMProvider;

use crate::config::GoalJudgeConfig;

const DRAFT_SYSTEM: &str = "\
You expand a user's goal into a structured completion contract.\n\
Reply ONLY with lines in this exact shape (omit empty fields):\n\
outcome: <what done means>\n\
verification: <shell command or check that proves done>\n\
constraints: <what not to break>\n\
boundaries: <scope limits>\n\
stop_when: <when to stop early>\n\
Then a blank line and a one-line goal summary without field prefixes.\n\
Prefer a concrete verification command (e.g. cargo test, npm test, pytest).";

/// Draft a contract-bearing goal string. On any failure returns `Err` so callers fail-open.
pub async fn draft_goal_contract_text(
    objective: &str,
    provider: &Arc<dyn LLMProvider>,
    _model: &str,
    _judge_cfg: &GoalJudgeConfig,
) -> Result<String, String> {
    let objective = objective.trim();
    if objective.is_empty() {
        return Err("empty objective".into());
    }

    let user = format!("Objective:\n{objective}\n\nExpand into a completion contract.");
    let messages = vec![
        edgequake_llm::ChatMessage::system(DRAFT_SYSTEM),
        edgequake_llm::ChatMessage::user(&user),
    ];
    let response = provider
        .chat(&messages, None)
        .await
        .map_err(|e| e.to_string())?;
    let text = response.content.trim();
    if text.is_empty() {
        return Err("empty draft response".into());
    }

    // Accept if it looks like contract fields OR a usable goal paragraph.
    let lower = text.to_ascii_lowercase();
    if lower.contains("verification:") || lower.contains("outcome:") {
        // Prefer: keep model output; parse_goal_with_contract will strip fields.
        // Prepend a clean goal line if the model only emitted fields.
        let (goal_text, contract) = edgecrab_types::parse_goal_with_contract(text);
        if goal_text.trim().is_empty() && !contract.is_empty() {
            let summary = objective.lines().next().unwrap_or(objective).trim();
            return Ok(format!(
                "{summary}\noutcome: {}\nverification: {}\nconstraints: {}\nboundaries: {}\nstop_when: {}",
                contract.outcome.trim(),
                contract.verification.trim(),
                contract.constraints.trim(),
                contract.boundaries.trim(),
                contract.stop_when.trim(),
            ));
        }
        return Ok(text.to_string());
    }

    // Soft accept: model returned prose without fields — use as free-form goal text.
    Ok(format!("{objective}\noutcome: {}", text.lines().next().unwrap_or(text)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_roundtrip_from_draft_shape() {
        let raw = "Ship the API\nverification: cargo test -p api\noutcome: tests green\n";
        let (goal, contract) = edgecrab_types::parse_goal_with_contract(raw);
        assert!(goal.contains("Ship the API"));
        assert_eq!(contract.verification.trim(), "cargo test -p api");
    }
}
