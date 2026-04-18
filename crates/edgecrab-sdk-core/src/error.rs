//! SDK error hierarchy.
//!
//! [`SdkError`] is the single error type returned by all SDK methods.
//! It wraps the internal [`AgentError`] and [`ToolError`] types and adds
//! SDK-specific variants for configuration and conversion failures.

use edgecrab_types::{AgentError, ToolError};

/// Top-level error type for all SDK operations.
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    /// An error originating from the agent runtime.
    #[error(transparent)]
    Agent(#[from] AgentError),

    /// An error originating from a tool execution.
    #[error(transparent)]
    Tool(#[from] ToolError),

    /// Configuration error (missing keys, bad values, file not found).
    #[error("SDK configuration error: {0}")]
    Config(String),

    /// Provider could not be created for the given model string.
    #[error("Provider error for model '{model}': {message}")]
    Provider { model: String, message: String },

    /// Serialization / deserialization failure at the SDK boundary.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// The agent has not been properly initialized.
    #[error("Agent not initialized: {0}")]
    NotInitialized(String),
}

impl SdkError {
    /// Returns `true` if the error is retryable (rate limit, transient network).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Agent(AgentError::RateLimited { .. }) => true,
            Self::Agent(AgentError::Llm(msg)) => {
                msg.contains("timeout") || msg.contains("connection")
            }
            _ => false,
        }
    }

    /// Returns the category string for observability/logging.
    pub fn category(&self) -> &'static str {
        match self {
            Self::Agent(_) => "agent",
            Self::Tool(_) => "tool",
            Self::Config(_) => "config",
            Self::Provider { .. } => "provider",
            Self::Serialization(_) => "serialization",
            Self::NotInitialized(_) => "initialization",
        }
    }
}
