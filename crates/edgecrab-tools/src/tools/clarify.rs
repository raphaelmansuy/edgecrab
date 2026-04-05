//! # clarify — Ask the user for clarification
//!
//! WHY clarify: When the agent is uncertain, it should ask instead of
//! guessing. This tool surfaces a question in the TUI. Supports two modes:
//! 1. Multiple-choice — provide up to MAX_CLARIFY_CHOICES predefined answers.
//! 2. Open-ended — omit `choices` entirely; user types a free-form response.
//!
//! Only available in interactive mode (Platform != Cron).
//! Mirrors hermes-agent's `clarify_tool.py` schema exactly.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{Platform, ToolError, ToolSchema};

use crate::registry::{ClarifyRequest, MAX_CLARIFY_CHOICES, ToolContext, ToolHandler};

pub struct ClarifyTool;

#[derive(Deserialize)]
struct Args {
    question: String,
    /// Up to MAX_CLARIFY_CHOICES predefined answer options.
    /// Omit for open-ended questions.
    #[serde(default)]
    choices: Option<Vec<String>>,
}

#[async_trait]
impl ToolHandler for ClarifyTool {
    fn name(&self) -> &'static str {
        "clarify"
    }

    fn toolset(&self) -> &'static str {
        "meta"
    }

    fn emoji(&self) -> &'static str {
        "❓"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "clarify".into(),
            description: (
                "Ask the user a question when you need clarification, feedback, or a decision \
                 before proceeding. Two modes:\n\n\
                 1. **Multiple choice** — provide up to 4 choices. The user picks one \
                 or types their own answer via a 5th 'Other' option.\n\
                 2. **Open-ended** — omit choices entirely. The user types a free-form response.\n\n\
                 Do NOT use for simple dangerous-command confirmation (terminal tool handles that). \
                 Prefer making a reasonable default when the decision is low-stakes."
            ).into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question to present to the user."
                    },
                    "choices": {
                        "type": "array",
                        "items": { "type": "string" },
                        "maxItems": MAX_CLARIFY_CHOICES,
                        "description": "Up to 4 predefined answer choices. Omit for open-ended questions. \
                                        The UI automatically appends an 'Other (type your answer)' option."
                    }
                },
                "required": ["question"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        true
    }

    fn check_fn(&self, ctx: &ToolContext) -> bool {
        // No interactive channel exists in cron sessions — clarify would
        // either block forever or return a stray [CLARIFY] marker that
        // no one reads. Matches hermes disabled_toolsets=["clarify"] for cron.
        ctx.platform != Platform::Cron
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let mut args: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "clarify".into(),
            message: e.to_string(),
        })?;

        let question = args.question.trim().to_string();
        if question.is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "clarify".into(),
                message: "Question text is required".into(),
            });
        }

        // Validate and cap choices: strip empty strings, limit to MAX_CLARIFY_CHOICES.
        if let Some(ref mut choices) = args.choices {
            choices.retain(|c| !c.trim().is_empty());
            choices.truncate(MAX_CLARIFY_CHOICES);
            if choices.is_empty() {
                args.choices = None; // empty list → treat as open-ended
            }
        }

        // If an interactive channel is available, send the question and
        // wait until the user answers or the session is cancelled.
        if let Some(ref tx) = ctx.clarify_tx {
            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<String>();
            let req = ClarifyRequest {
                question: question.clone(),
                choices: args.choices.clone(),
                response_tx: resp_tx,
            };
            if tx.send(req).is_ok() {
                tokio::select! {
                    _ = ctx.cancel.cancelled() => {
                        return Err(ToolError::Other("Interrupted by user".into()));
                    }
                    result = resp_rx => {
                        if let Ok(answer) = result {
                            return Ok(answer);
                        }
                    }
                }
            }
        }

        // Fallback for batch / gateway modes:
        // Include choices in the marker so gateway can surface them.
        match &args.choices {
            Some(choices) if !choices.is_empty() => {
                let opts = choices
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{}. {}", i + 1, c))
                    .collect::<Vec<_>>()
                    .join(", ");
                Ok(format!("[CLARIFY] {} (choices: {})", question, opts))
            }
            _ => Ok(format!("[CLARIFY] {}", question)),
        }
    }
}

inventory::submit!(&ClarifyTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn clarify_returns_marker_without_channel() {
        let ctx = ToolContext::test_context(); // no clarify_tx
        let result = ClarifyTool
            .execute(json!({"question": "Which file do you mean?"}), &ctx)
            .await
            .expect("ok");
        assert!(result.contains("Which file do you mean?"));
        assert!(result.starts_with("[CLARIFY]"));
    }

    #[tokio::test]
    async fn clarify_with_choices_renders_options() {
        let ctx = ToolContext::test_context();
        let result = ClarifyTool
            .execute(
                json!({
                    "question": "Which approach?",
                    "choices": ["Option A", "Option B", "Option C"]
                }),
                &ctx,
            )
            .await
            .expect("ok");
        assert!(result.contains("Which approach?"));
        assert!(result.contains("choices:"));
        assert!(result.contains("Option A"));
    }

    #[tokio::test]
    async fn clarify_choices_capped_at_max() {
        let ctx = ToolContext::test_context();
        let result = ClarifyTool
            .execute(
                json!({
                    "question": "Pick one",
                    "choices": ["A", "B", "C", "D", "E", "F"] // 6 > MAX_CLARIFY_CHOICES
                }),
                &ctx,
            )
            .await
            .expect("ok");
        // Only first MAX_CLARIFY_CHOICES options should appear
        // D, E, F should NOT appear (cap at 4)
        assert!(!result.contains("5. E"));
        assert!(!result.contains("6. F"));
    }

    #[tokio::test]
    async fn clarify_empty_question_rejected() {
        let ctx = ToolContext::test_context();
        assert!(
            ClarifyTool
                .execute(json!({"question": "   "}), &ctx)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn clarify_uses_channel_when_provided() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ClarifyRequest>();
        let mut ctx = ToolContext::test_context();
        ctx.clarify_tx = Some(tx);

        // Spawn a task to answer the question
        let handle = tokio::spawn(async move {
            if let Some(req) = rx.recv().await {
                // choices should carry through from the tool call
                assert_eq!(req.choices, Some(vec!["Yes".to_string(), "No".to_string()]));
                let _ = req.response_tx.send("Yes".into());
            }
        });

        let result = ClarifyTool
            .execute(
                json!({"question": "Which file do you mean?", "choices": ["Yes", "No"]}),
                &ctx,
            )
            .await
            .expect("ok");
        handle.await.expect("spawned task panicked");
        assert_eq!(result, "Yes");
    }

    #[tokio::test]
    async fn clarify_is_available() {
        assert!(ClarifyTool.is_available());
    }
}
