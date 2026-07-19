//! Structured provider error classification — thin facade over [`crate::failover`] (HA-50 / DRY).

use edgequake_llm::LlmError;

use crate::failover::{self, ClassifiedError, ClassifyContext, FailoverReason};

/// High-level provider failure class for retry policy and operator messaging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorClass {
    ModelNotSupported,
    Auth,
    InvalidRequest,
    ModelNotFound,
    ContextOverflow,
    RateLimit,
    StreamAssembly,
    Retryable,
}

impl ProviderErrorClass {
    pub fn is_non_retryable(self) -> bool {
        matches!(
            self,
            Self::ModelNotSupported
                | Self::Auth
                | Self::InvalidRequest
                | Self::ModelNotFound
                | Self::ContextOverflow
        )
    }

    pub fn operator_hint(self) -> Option<&'static str> {
        match self {
            Self::ModelNotSupported => Some(
                "Try `/model copilot/auto` or `/model copilot/gpt-4.1-mini`. \
                 Run `edgecrab doctor` — the agent ping tests chat+tools.",
            ),
            Self::Auth => {
                Some("Run `edgecrab auth login copilot` (or your provider login) and retry.")
            }
            Self::RateLimit => Some("Wait for the rate-limit window, then retry or switch models."),
            _ => None,
        }
    }
}

/// Map a [`ClassifiedError`] from the unified failover brain to legacy enum.
pub fn to_provider_error_class(c: &ClassifiedError) -> ProviderErrorClass {
    if is_stream_assembly_message(&c.message) {
        return ProviderErrorClass::StreamAssembly;
    }
    match c.reason {
        FailoverReason::Auth | FailoverReason::AuthPermanent => ProviderErrorClass::Auth,
        FailoverReason::Billing => ProviderErrorClass::ModelNotSupported,
        FailoverReason::ModelNotFound => ProviderErrorClass::ModelNotFound,
        FailoverReason::ContextOverflow
        | FailoverReason::PayloadTooLarge
        | FailoverReason::ImageTooLarge
        | FailoverReason::LongContextTier => ProviderErrorClass::ContextOverflow,
        FailoverReason::RateLimit
        | FailoverReason::UpstreamRateLimit
        | FailoverReason::Overloaded => ProviderErrorClass::RateLimit,
        FailoverReason::FormatError
        | FailoverReason::ProviderPolicyBlocked
        | FailoverReason::ContentPolicyBlocked
        | FailoverReason::InvalidEncryptedContent
        | FailoverReason::SslCertVerification => ProviderErrorClass::InvalidRequest,
        FailoverReason::ThinkingSignature
        | FailoverReason::MultimodalToolContentUnsupported
        | FailoverReason::LlamaCppGrammarPattern
        | FailoverReason::OauthLongContextBetaForbidden
        | FailoverReason::ServerError
        | FailoverReason::Timeout
        | FailoverReason::Unknown => ProviderErrorClass::Retryable,
    }
}

fn is_stream_assembly_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("finished without arguments") || lower.contains("invalid json arguments")
}

pub fn classify_llm_error(error: &LlmError) -> ProviderErrorClass {
    classify_llm_error_with_session(error, ClassifyContext::default())
}

pub fn classify_llm_error_with_session(
    error: &LlmError,
    session: ClassifyContext,
) -> ProviderErrorClass {
    to_provider_error_class(&failover::classify_llm_error(error, session))
}

pub fn classify_error_text(message: &str) -> ProviderErrorClass {
    classify_error_with_session(message, ClassifyContext::default())
}

pub fn classify_error_with_session(message: &str, session: ClassifyContext) -> ProviderErrorClass {
    to_provider_error_class(&failover::classify_api_error(failover::ClassifyInput {
        raw_message: message,
        body_message: None,
        metadata_message: None,
        status_code: None,
        error_code: None,
        error_type_name: None,
        provider: "",
        model: "",
        session,
    }))
}

pub fn format_classified_failure(class: ProviderErrorClass, attempt: u32, detail: &str) -> String {
    let attempts_note = if attempt == 0 {
        "failed immediately".to_string()
    } else {
        format!("failed after {} retries", attempt)
    };
    let mut msg = match class {
        ProviderErrorClass::ModelNotSupported => {
            format!("Copilot model not supported for agent chat+tools ({attempts_note}): {detail}")
        }
        _ => format!("Provider call {attempts_note}: {detail}"),
    };
    if let Some(hint) = class.operator_hint() {
        msg.push(' ');
        msg.push_str(hint);
    }
    msg
}

/// Format a failover [`ClassifiedError`] for operator display.
pub fn format_classified(c: &ClassifiedError, attempt: u32, detail: &str) -> String {
    format_classified_failure(to_provider_error_class(c), attempt, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_model_not_supported() {
        assert_eq!(
            classify_error_text(r#"{"code":"model_not_supported"}"#),
            ProviderErrorClass::ModelNotSupported
        );
        assert!(ProviderErrorClass::ModelNotSupported.is_non_retryable());
    }

    #[test]
    fn classifies_stream_assembly() {
        assert_eq!(
            classify_error_text("streamed tool call finished without arguments"),
            ProviderErrorClass::StreamAssembly
        );
        assert!(!ProviderErrorClass::StreamAssembly.is_non_retryable());
    }

    #[test]
    fn delegates_billing_to_failover() {
        assert_eq!(
            classify_error_text("insufficient credits to complete this request"),
            ProviderErrorClass::ModelNotSupported
        );
    }

    #[test]
    fn copilot_probe_still_maps_to_model_not_supported() {
        assert_eq!(
            classify_llm_error(&LlmError::ApiError(
                r#"{"error":{"code":"model_not_supported","message":"nope"}}"#.into()
            )),
            ProviderErrorClass::ModelNotSupported
        );
    }
}
