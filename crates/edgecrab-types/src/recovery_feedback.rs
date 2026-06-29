//! Self-reflective tool error recovery payloads (Schema v0.1-inspired).
//!
//! On validation failure, tools return a concise diagnosis in `error` and attach
//! machine-readable `recovery_feedback.suggestions[]` so the agent can retry
//! without guessing fixes from training-data priors.
//!
//! See: Canedo & Chethan, "Self-Reflective APIs: Structure Beats Verbosity
//! for AI Agent Recovery" (arXiv:2606.05037).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Typed recovery verbs — EdgeCrab domain vocabulary (discrete, mergeable params).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryAction {
    /// Change a tool argument to a specific literal value.
    SetParameter,
    /// Pick a different filesystem path.
    UseDifferentPath,
    /// Invoke another tool before retrying (e.g. `read_file` before `write_file`).
    CallToolFirst,
    /// Prefer a different tool for the same intent (e.g. `patch` over `write_file`).
    SwitchTool,
    /// Split a large artifact into scaffold + incremental edits.
    SplitPayload,
    /// Retry the same tool call — prior guard state already satisfied.
    RetrySameCall,
    /// No automated recovery path exists for this rejection.
    NoRecoveryAvailable,
}

/// One machine-readable repair step the agent can merge into its next tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoverySuggestion {
    pub action: RecoveryAction,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub parameters: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl RecoverySuggestion {
    pub fn new(action: RecoveryAction, parameters: Value) -> Self {
        Self {
            action,
            parameters,
            message: None,
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// Top-level recovery block attached to [`crate::ToolErrorResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryFeedback {
    #[serde(rename = "type")]
    pub feedback_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub suggestions: Vec<RecoverySuggestion>,
}

/// Fluent builder — single place to assemble suggestions (DRY across tools).
#[derive(Debug, Clone, Default)]
pub struct RecoveryFeedbackBuilder {
    feedback_type: String,
    message: Option<String>,
    suggestions: Vec<RecoverySuggestion>,
}

impl RecoveryFeedbackBuilder {
    pub fn new(feedback_type: impl Into<String>) -> Self {
        Self {
            feedback_type: feedback_type.into(),
            message: None,
            suggestions: Vec::new(),
        }
    }

    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn suggestion(mut self, action: RecoveryAction, parameters: Value) -> Self {
        self.suggestions.push(RecoverySuggestion::new(action, parameters));
        self
    }

    pub fn suggestion_with_message(
        mut self,
        action: RecoveryAction,
        parameters: Value,
        message: impl Into<String>,
    ) -> Self {
        self.suggestions
            .push(RecoverySuggestion::new(action, parameters).with_message(message));
        self
    }

    pub fn build(self) -> RecoveryFeedback {
        RecoveryFeedback {
            feedback_type: self.feedback_type,
            message: self.message,
            suggestions: self.suggestions,
        }
    }
}

/// Convenience for `SET_PARAMETER` suggestions.
pub fn set_parameter(tool: &str, params: Value) -> RecoverySuggestion {
    let mut parameters = params.as_object().cloned().unwrap_or_default();
    parameters.insert("tool".into(), json!(tool));
    RecoverySuggestion::new(RecoveryAction::SetParameter, Value::Object(parameters))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_feedback_serializes_schema_shape() {
        let feedback = RecoveryFeedbackBuilder::new("recovery_guidance")
            .message("Path already exists")
            .suggestion(
                RecoveryAction::SetParameter,
                json!({ "tool": "write_file", "if_exists": "overwrite", "path": "src/main.rs" }),
            )
            .build();
        let json = serde_json::to_value(&feedback).expect("serialize");
        assert_eq!(json["type"], "recovery_guidance");
        assert_eq!(json["suggestions"][0]["action"], "SET_PARAMETER");
        assert!(json["suggestions"][0]["parameters"]["if_exists"].is_string());
    }
}
