//! Conversions between SDK types and internal types.
//!
//! This module provides [`parse_model_string`] for splitting `"provider/model"`
//! strings and `From`/`Into` impls for SDK ↔ internal type conversions.
//!
//! ## No Heuristic Provider Guessing
//!
//! Model strings **must** use the `"provider/model"` format. Bare model names
//! are rejected with a clear error. The canonical source of truth for
//! provider ↔ model mappings is [`ModelCatalog`](edgecrab_core::model_catalog::ModelCatalog),
//! not prefix-matching heuristics.

use crate::error::SdkError;

// ── Error conversions ────────────────────────────────────────────────

impl From<String> for SdkError {
    fn from(msg: String) -> Self {
        SdkError::Config(msg)
    }
}

/// Parse a `"provider/model"` string into `(provider, model)` components.
///
/// Returns `Err` if the string does not contain exactly one `/` separator.
/// This enforces an explicit, deterministic contract — no guessing.
///
/// # Examples
///
/// ```
/// use edgecrab_sdk_core::convert::parse_model_string;
///
/// let (p, m) = parse_model_string("anthropic/claude-sonnet-4").unwrap();
/// assert_eq!(p, "anthropic");
/// assert_eq!(m, "claude-sonnet-4");
///
/// assert!(parse_model_string("claude-sonnet-4").is_err());
/// ```
pub fn parse_model_string(model: &str) -> Result<(String, String), SdkError> {
    match model.split_once('/') {
        Some((provider, model_name)) if !provider.is_empty() && !model_name.is_empty() => {
            Ok((provider.to_string(), model_name.to_string()))
        }
        _ => Err(SdkError::Config(format!(
            "Model string must be \"provider/model\" (e.g. \"anthropic/claude-sonnet-4\"), got: \"{model}\""
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::AgentError;

    #[test]
    fn parse_provider_model() {
        let (p, m) = parse_model_string("anthropic/claude-sonnet-4").unwrap();
        assert_eq!(p, "anthropic");
        assert_eq!(m, "claude-sonnet-4");
    }

    #[test]
    fn parse_rejects_bare_model_name() {
        let result = parse_model_string("claude-sonnet-4");
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_empty_provider() {
        let result = parse_model_string("/claude-sonnet-4");
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_empty_model() {
        let result = parse_model_string("anthropic/");
        assert!(result.is_err());
    }

    #[test]
    fn sdk_error_from_agent_error() {
        let agent_err = AgentError::Config("test".into());
        let sdk_err: SdkError = agent_err.into();
        assert_eq!(sdk_err.category(), "agent");
    }

    #[test]
    fn sdk_error_retryable() {
        let rate_err = SdkError::Agent(AgentError::RateLimited {
            provider: "anthropic".into(),
            retry_after_ms: 1000,
        });
        assert!(rate_err.is_retryable());

        let config_err = SdkError::Config("bad".into());
        assert!(!config_err.is_retryable());
    }
}
