//! Error types for the EdgeCrab agent.
//!
//! Strategy: `thiserror` for all library crates (structured, matchable),
//! `anyhow` only in binary crate entry points.

use serde_json;

/// Top-level agent error — covers every failure mode documented in the spec.
///
/// Each variant maps to a specific recovery strategy in the conversation loop:
/// - Retryable errors trigger exponential backoff
/// - Budget/interrupt errors break the loop cleanly
/// - Tool errors are fed back to the LLM as JSON
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("LLM API error: {0}")]
    Llm(String),

    #[error("Tool execution failed: {tool} — {message}")]
    ToolExecution { tool: String, message: String },

    #[error("Context limit exceeded: {used}/{limit} tokens")]
    ContextLimit { used: usize, limit: usize },

    #[error("Budget exhausted: {used}/{max} iterations")]
    BudgetExhausted { used: u32, max: u32 },

    #[error("Interrupted by user")]
    Interrupted,

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Provider rate limited: retry after {retry_after_ms}ms")]
    RateLimited {
        provider: String,
        retry_after_ms: u64,
    },

    #[error("Context compression failed: {0}")]
    CompressionFailed(String),

    #[error("API refusal: {0}")]
    ApiRefusal(String),

    #[error("Malformed tool call from LLM: {0}")]
    MalformedToolCall(String),

    #[error("Plugin error in {plugin}: {message}")]
    Plugin { plugin: String, message: String },

    #[error("Gateway delivery failed to {platform}: {message}")]
    GatewayDelivery { platform: String, message: String },

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Security violation: {0}")]
    Security(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

/// Per-tool-call error record accumulated in `ConversationResult.tool_errors`.
///
/// Mirrors hermes-agent's `ToolError` dataclass (used in `AgentResult.tool_errors`).
/// Provides first-class error observability without requiring callers to parse raw
/// message history — enables RL training signal extraction and structured logging.
///
/// Fields:
/// - `turn`        — API call index within the conversation (1-based).
/// - `tool_name`   — Name of the tool that was called.
/// - `arguments`   — Raw JSON arguments string passed to the tool.
/// - `error`       — Human-readable error description.
/// - `tool_result` — Full tool result string returned to the LLM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolErrorRecord {
    pub turn: u32,
    pub tool_name: String,
    pub arguments: String,
    pub error: String,
    pub tool_result: String,
}

/// Tool-specific errors with retry strategy metadata.
///
/// These are converted to JSON and sent back to the LLM so it can
/// self-correct (e.g. fix a bad path, retry with different args).
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Unknown tool: {0}")]
    NotFound(String),

    #[error("Invalid arguments for {tool}: {message}")]
    InvalidArgs { tool: String, message: String },

    #[error("Tool {tool} unavailable: {reason}")]
    Unavailable { tool: String, reason: String },

    #[error("Execution timeout after {seconds}s: {tool}")]
    Timeout { tool: String, seconds: u64 },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Execution failed in {tool}: {message}")]
    ExecutionFailed { tool: String, message: String },

    #[error("{message}")]
    CapabilityDenied {
        tool: String,
        code: String,
        message: String,
        suppression_key: Option<String>,
        suggested_tool: Option<String>,
        suggested_action: Option<String>,
    },

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ToolErrorResponse {
    #[serde(rename = "type")]
    pub response_type: String,
    pub category: String,
    pub code: String,
    pub error: String,
    pub retryable: bool,
    pub suppress_retry: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppression_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
    /// Required parameter names — populated from tool schema on InvalidArgs.
    /// Gives the LLM a precise checklist of what to fix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_fields: Option<Vec<String>>,
    /// One-line corrective hint — e.g. "content must be a non-null string".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_hint: Option<String>,
}

impl ToolError {
    pub fn capability_denied(
        tool: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::CapabilityDenied {
            tool: tool.into(),
            code: code.into(),
            message: message.into(),
            suppression_key: None,
            suggested_tool: None,
            suggested_action: None,
        }
    }

    pub fn with_suppression_key(self, suppression_key: impl Into<String>) -> Self {
        match self {
            Self::CapabilityDenied {
                tool,
                code,
                message,
                suggested_tool,
                suggested_action,
                ..
            } => Self::CapabilityDenied {
                tool,
                code,
                message,
                suppression_key: Some(suppression_key.into()),
                suggested_tool,
                suggested_action,
            },
            other => other,
        }
    }

    pub fn with_suggested_tool(self, suggested_tool: impl Into<String>) -> Self {
        match self {
            Self::CapabilityDenied {
                tool,
                code,
                message,
                suppression_key,
                suggested_action,
                ..
            } => Self::CapabilityDenied {
                tool,
                code,
                message,
                suppression_key,
                suggested_tool: Some(suggested_tool.into()),
                suggested_action,
            },
            other => other,
        }
    }

    pub fn with_suggested_action(self, suggested_action: impl Into<String>) -> Self {
        match self {
            Self::CapabilityDenied {
                tool,
                code,
                message,
                suppression_key,
                suggested_tool,
                ..
            } => Self::CapabilityDenied {
                tool,
                code,
                message,
                suppression_key,
                suggested_tool,
                suggested_action: Some(suggested_action.into()),
            },
            other => other,
        }
    }

    pub fn to_llm_payload(&self) -> ToolErrorResponse {
        ToolErrorResponse {
            response_type: "tool_error".into(),
            category: self.category().into(),
            code: self.code().into(),
            error: self.to_string(),
            retryable: self.is_retryable(),
            suppress_retry: self.should_suppress_retry(),
            suppression_key: self.suppression_key(),
            tool: self.tool_name().map(str::to_string),
            suggested_tool: self.suggested_tool().map(str::to_string),
            suggested_action: self.suggested_action().map(str::to_string),
            required_fields: None,
            usage_hint: None,
        }
    }

    /// Build an enriched LLM payload with schema-derived corrective hints.
    ///
    /// WHY: When the LLM sends invalid arguments, a bare "missing field X"
    /// message forces it to guess the full schema from memory. By echoing
    /// the required fields and a usage hint, we give it a precise checklist.
    /// Hermes-agent's `coerce_tool_args` + schema echo pattern reduced
    /// retry loops by ~40% in their production telemetry.
    pub fn to_llm_payload_enriched(
        &self,
        required_fields: Option<Vec<String>>,
        usage_hint: Option<String>,
    ) -> ToolErrorResponse {
        let mut payload = self.to_llm_payload();
        payload.required_fields = required_fields;
        payload.usage_hint = usage_hint;
        payload
    }

    /// Convert to a JSON string suitable for the LLM to parse.
    pub fn to_llm_response(&self) -> String {
        serde_json::to_string(&self.to_llm_payload()).expect("tool error payload serializes")
    }

    /// Whether the LLM should retry with different parameters.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ToolError::Timeout { .. } | ToolError::Unavailable { .. }
        )
    }

    pub fn should_suppress_retry(&self) -> bool {
        matches!(
            self,
            ToolError::InvalidArgs { .. }
                | ToolError::Unavailable { .. }
                | ToolError::PermissionDenied(_)
                | ToolError::CapabilityDenied { .. }
        )
    }

    pub fn category(&self) -> &'static str {
        match self {
            ToolError::NotFound(_) => "resolution",
            ToolError::InvalidArgs { .. } => "arguments",
            ToolError::Unavailable { .. } => "availability",
            ToolError::Timeout { .. } => "timeout",
            ToolError::PermissionDenied(_) => "permission",
            ToolError::ExecutionFailed { .. } => "execution",
            ToolError::CapabilityDenied { .. } => "capability",
            ToolError::Other(_) => "other",
        }
    }

    pub fn code(&self) -> &str {
        match self {
            ToolError::NotFound(_) => "tool_not_found",
            ToolError::InvalidArgs { .. } => "invalid_arguments",
            ToolError::Unavailable { .. } => "tool_unavailable",
            ToolError::Timeout { .. } => "tool_timeout",
            ToolError::PermissionDenied(_) => "permission_denied",
            ToolError::ExecutionFailed { .. } => "execution_failed",
            ToolError::CapabilityDenied { code, .. } => code,
            ToolError::Other(_) => "tool_error",
        }
    }

    pub fn tool_name(&self) -> Option<&str> {
        match self {
            ToolError::InvalidArgs { tool, .. }
            | ToolError::Unavailable { tool, .. }
            | ToolError::Timeout { tool, .. }
            | ToolError::ExecutionFailed { tool, .. }
            | ToolError::CapabilityDenied { tool, .. } => Some(tool),
            ToolError::NotFound(_) | ToolError::PermissionDenied(_) | ToolError::Other(_) => None,
        }
    }

    pub fn suggested_tool(&self) -> Option<&str> {
        match self {
            ToolError::CapabilityDenied { suggested_tool, .. } => suggested_tool.as_deref(),
            _ => None,
        }
    }

    pub fn suppression_key(&self) -> Option<String> {
        match self {
            ToolError::Unavailable { tool, .. } => Some(format!("{tool}:{}", self.code())),
            ToolError::PermissionDenied(_) => Some(self.code().to_string()),
            ToolError::CapabilityDenied {
                tool,
                code,
                suppression_key,
                ..
            } => Some(
                suppression_key
                    .clone()
                    .unwrap_or_else(|| format!("{tool}:{code}")),
            ),
            _ => None,
        }
    }

    pub fn suggested_action(&self) -> Option<&str> {
        match self {
            ToolError::CapabilityDenied {
                suggested_action, ..
            } => suggested_action.as_deref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_error_display() {
        let err = AgentError::BudgetExhausted { used: 90, max: 90 };
        assert_eq!(err.to_string(), "Budget exhausted: 90/90 iterations");
    }

    #[test]
    fn tool_error_to_llm_response_retryable() {
        let err = ToolError::Timeout {
            tool: "terminal".into(),
            seconds: 30,
        };
        let json: serde_json::Value =
            serde_json::from_str(&err.to_llm_response()).expect("valid json");
        assert_eq!(json["retryable"], true);
        assert_eq!(json["category"], "timeout");
        assert_eq!(json["code"], "tool_timeout");
        assert_eq!(json["tool"], "terminal");
    }

    #[test]
    fn tool_error_to_llm_response_not_retryable() {
        let err = ToolError::NotFound("nonexistent".into());
        let json: serde_json::Value =
            serde_json::from_str(&err.to_llm_response()).expect("valid json");
        assert_eq!(json["retryable"], false);
        assert_eq!(json["suppress_retry"], false);
    }

    #[test]
    fn capability_error_serializes_with_suggestions() {
        let err = ToolError::capability_denied(
            "terminal",
            "macos_automation_unknown",
            "Automation consent could not be determined.",
        )
        .with_suggested_tool("clarify")
        .with_suppression_key("terminal:macos_automation_unknown:notes")
        .with_suggested_action("Open Notes.app, run /permissions bootstrap, then retry.");

        let json: serde_json::Value =
            serde_json::from_str(&err.to_llm_response()).expect("valid json");
        assert_eq!(json["type"], "tool_error");
        assert_eq!(json["category"], "capability");
        assert_eq!(json["code"], "macos_automation_unknown");
        assert_eq!(json["retryable"], false);
        assert_eq!(json["suppress_retry"], true);
        assert_eq!(
            json["suppression_key"],
            "terminal:macos_automation_unknown:notes"
        );
        assert_eq!(json["tool"], "terminal");
        assert_eq!(json["suggested_tool"], "clarify");
        assert_eq!(
            json["suggested_action"],
            "Open Notes.app, run /permissions bootstrap, then retry."
        );
    }

    #[test]
    fn tool_error_invalid_args() {
        let err = ToolError::InvalidArgs {
            tool: "read_file".into(),
            message: "path is required".into(),
        };
        assert_eq!(
            err.to_string(),
            "Invalid arguments for read_file: path is required"
        );
        assert!(!err.is_retryable());
        assert!(err.should_suppress_retry());
    }

    #[test]
    fn agent_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let agent_err: AgentError = io_err.into();
        assert!(agent_err.to_string().contains("file not found"));
    }

    #[test]
    fn agent_error_from_serde() {
        let serde_err =
            serde_json::from_str::<serde_json::Value>("bad json").expect_err("should fail");
        let agent_err: AgentError = serde_err.into();
        assert!(agent_err.to_string().contains("Serialization"));
    }
}
