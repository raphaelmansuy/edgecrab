//! HA-50 failover classifier matrix — Hermes scenario port (spec 017 P1-4).

use edgecrab_core::failover::{
    ClassifiedError, ClassifyContext, ClassifyInput, FailoverReason, classify_api_error,
};

fn classify(msg: &str, status: Option<u16>) -> ClassifiedError {
    classify_api_error(ClassifyInput {
        raw_message: msg,
        body_message: None,
        metadata_message: None,
        status_code: status,
        error_code: None,
        error_type_name: None,
        provider: "openrouter",
        model: "gpt-5",
        session: ClassifyContext::default(),
    })
}

fn classify_session(msg: &str, status: Option<u16>, session: ClassifyContext) -> ClassifiedError {
    classify_api_error(ClassifyInput {
        raw_message: msg,
        body_message: None,
        metadata_message: None,
        status_code: status,
        error_code: None,
        error_type_name: None,
        provider: "anthropic",
        model: "claude-sonnet-4",
        session,
    })
}

macro_rules! matrix_case {
    ($name:ident, $msg:expr, $status:expr, $reason:expr) => {
        #[test]
        fn $name() {
            assert_eq!(classify($msg, $status).reason, $reason);
        }
    };
}

matrix_case!(m401_auth, "Unauthorized", Some(401), FailoverReason::Auth);
matrix_case!(m403_auth, "Forbidden", Some(403), FailoverReason::Auth);
matrix_case!(
    m403_billing_key_limit,
    "Key limit exceeded for this key",
    Some(403),
    FailoverReason::Billing
);
matrix_case!(
    m402_billing,
    "Payment Required",
    Some(402),
    FailoverReason::Billing
);
matrix_case!(
    m402_rate_transient,
    "usage limit exceeded, try again later",
    Some(402),
    FailoverReason::RateLimit
);
matrix_case!(
    m429_rate,
    "Too Many Requests",
    Some(429),
    FailoverReason::RateLimit
);
matrix_case!(
    m500_server,
    "Internal Server Error",
    Some(500),
    FailoverReason::ServerError
);
matrix_case!(
    m502_server,
    "Bad Gateway",
    Some(502),
    FailoverReason::ServerError
);
matrix_case!(
    m503_overloaded,
    "Service Unavailable",
    Some(503),
    FailoverReason::Overloaded
);
matrix_case!(
    m529_overloaded,
    "Overloaded",
    Some(529),
    FailoverReason::Overloaded
);
matrix_case!(
    m413_payload,
    "Request Entity Too Large",
    Some(413),
    FailoverReason::PayloadTooLarge
);
matrix_case!(
    m404_model,
    "model not found",
    Some(404),
    FailoverReason::ModelNotFound
);
matrix_case!(
    m404_unknown,
    "Not Found",
    Some(404),
    FailoverReason::Unknown
);
matrix_case!(
    m400_context,
    "context length exceeded: 50000 > 32768",
    Some(400),
    FailoverReason::ContextOverflow
);
matrix_case!(
    m400_tokens,
    "This model's maximum context is 128000 tokens, too many tokens",
    Some(400),
    FailoverReason::ContextOverflow
);
matrix_case!(
    m400_prompt_long,
    "prompt is too long: 300000 tokens > 200000 maximum",
    Some(400),
    FailoverReason::ContextOverflow
);
matrix_case!(
    m400_vllm,
    "The engine prompt length 1327246 exceeds the max_model_len 131072",
    Some(400),
    FailoverReason::ContextOverflow
);
matrix_case!(
    m400_ollama,
    "context length exceeded",
    Some(400),
    FailoverReason::ContextOverflow
);
matrix_case!(
    m400_chinese_overflow,
    "超过最大长度限制",
    Some(400),
    FailoverReason::ContextOverflow
);
matrix_case!(
    m400_alibaba_rate,
    "Request rate increased too quickly",
    Some(400),
    FailoverReason::RateLimit
);
matrix_case!(
    m400_format_temp,
    "Invalid value for parameter 'temperature': must be between 0 and 2",
    Some(400),
    FailoverReason::FormatError
);
matrix_case!(
    m422_format,
    "Unprocessable Entity",
    Some(422),
    FailoverReason::FormatError
);
matrix_case!(
    m502_unknown_param,
    "Unknown parameter: 'foo'",
    Some(502),
    FailoverReason::FormatError
);
matrix_case!(
    m502_unsupported_param,
    "Unsupported parameter: logprobs",
    Some(502),
    FailoverReason::FormatError
);
matrix_case!(
    m404_policy,
    "No endpoints available matching your guardrail restrictions and data policy",
    Some(404),
    FailoverReason::ProviderPolicyBlocked
);
matrix_case!(
    m400_policy,
    "No endpoints available matching your data policy",
    Some(400),
    FailoverReason::ProviderPolicyBlocked
);
matrix_case!(
    m_cyber_policy,
    "This content was flagged for possible cybersecurity risk",
    None,
    FailoverReason::ContentPolicyBlocked
);
matrix_case!(
    m_openai_policy,
    "Your request was flagged by the moderation system",
    Some(400),
    FailoverReason::ContentPolicyBlocked
);
matrix_case!(
    m_anthropic_safety,
    "Your prompt was flagged by our safety system",
    None,
    FailoverReason::ContentPolicyBlocked
);
matrix_case!(
    m_msg_billing,
    "insufficient credits to complete this request",
    None,
    FailoverReason::Billing
);
matrix_case!(
    m_msg_rate,
    "rate limit reached for this model",
    None,
    FailoverReason::RateLimit
);
matrix_case!(
    m_msg_auth,
    "invalid api key provided",
    None,
    FailoverReason::Auth
);
matrix_case!(
    m_msg_model,
    "gpt-99 is not a valid model",
    None,
    FailoverReason::ModelNotFound
);
matrix_case!(
    m_msg_overflow,
    "maximum context length exceeded",
    None,
    FailoverReason::ContextOverflow
);
matrix_case!(
    m_msg_usage_rate,
    "usage limit exceeded, try again in 5 minutes",
    None,
    FailoverReason::RateLimit
);
matrix_case!(
    m_msg_usage_billing,
    "usage limit reached",
    None,
    FailoverReason::Billing
);
matrix_case!(
    m_timeout_read,
    "Read timed out",
    None,
    FailoverReason::Timeout
);
matrix_case!(
    m_timeout_connect,
    "Connection refused",
    None,
    FailoverReason::Timeout
);
matrix_case!(
    m_runtime_timeout,
    "claude CLI turn timed out",
    None,
    FailoverReason::Timeout
);
matrix_case!(
    m_thinking_sig,
    "thinking block has invalid signature",
    Some(400),
    FailoverReason::ThinkingSignature
);
matrix_case!(
    m_thinking_frozen,
    "`thinking` blocks in the latest assistant message cannot be modified",
    Some(400),
    FailoverReason::ThinkingSignature
);
matrix_case!(
    m_long_context_tier,
    "Extra usage is required for long context requests over 200k tokens",
    Some(429),
    FailoverReason::LongContextTier
);
matrix_case!(
    m_oauth_beta,
    "The long context beta is not yet available for this subscription",
    Some(400),
    FailoverReason::OauthLongContextBetaForbidden
);
matrix_case!(
    m_llama_grammar,
    "error parsing grammar: unknown escape",
    Some(400),
    FailoverReason::LlamaCppGrammarPattern
);
matrix_case!(
    m_multimodal_mimo,
    "text is not set",
    Some(400),
    FailoverReason::MultimodalToolContentUnsupported
);
matrix_case!(
    m_multimodal_string,
    "tool message content must be a string",
    Some(400),
    FailoverReason::MultimodalToolContentUnsupported
);
matrix_case!(
    m_ssl_bad_mac,
    "[SSL: BAD_RECORD_MAC] sslv3 alert bad record mac",
    None,
    FailoverReason::Timeout
);
matrix_case!(
    m_generic_unknown,
    "something weird happened",
    None,
    FailoverReason::Unknown
);

#[test]
fn m403_spending_billing() {
    assert_eq!(
        classify("spending limit reached", Some(403)).reason,
        FailoverReason::Billing
    );
}

#[test]
fn m402_out_of_funds_billing() {
    assert_eq!(
        classify("Your API key has run out of funds", Some(402)).reason,
        FailoverReason::Billing
    );
}

#[test]
fn m402_quota_retry_rate() {
    assert_eq!(
        classify(
            "quota exceeded, please retry after the window resets",
            Some(402)
        )
        .reason,
        FailoverReason::RateLimit
    );
}

#[test]
fn m500_invalid_request_format() {
    assert_eq!(
        classify_api_error(ClassifyInput {
            raw_message: "bad request",
            body_message: Some("bad request"),
            metadata_message: None,
            status_code: Some(500),
            error_code: Some("invalid_request_error"),
            error_type_name: None,
            provider: "",
            model: "",
            session: ClassifyContext::default(),
        })
        .reason,
        FailoverReason::FormatError
    );
}

#[test]
fn m_disconnect_small_timeout() {
    let c = classify_session(
        "server disconnected without sending complete message",
        None,
        ClassifyContext {
            approx_tokens: 5_000,
            context_length: 200_000,
            num_messages: 0,
        },
    );
    assert_eq!(c.reason, FailoverReason::Timeout);
}

#[test]
fn m_disconnect_large_overflow() {
    let c = classify_session(
        "peer closed connection without sending complete message",
        None,
        ClassifyContext {
            approx_tokens: 150_000,
            context_length: 200_000,
            num_messages: 0,
        },
    );
    assert_eq!(c.reason, FailoverReason::ContextOverflow);
    assert!(c.should_compress);
}

#[test]
fn m_rate_limit_error_type_forces_429() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "You have exceeded your rate limit.",
        body_message: None,
        metadata_message: None,
        status_code: None,
        error_code: None,
        error_type_name: Some("RateLimitError"),
        provider: "copilot",
        model: "gpt-4o",
        session: ClassifyContext::default(),
    });
    assert_eq!(c.reason, FailoverReason::RateLimit);
}

#[test]
fn m_invalid_encrypted_content() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "Error code: 400 - invalid_encrypted_content",
        body_message: None,
        metadata_message: None,
        status_code: Some(400),
        error_code: Some("invalid_encrypted_content"),
        error_type_name: None,
        provider: "custom",
        model: "gpt-5.4",
        session: ClassifyContext::default(),
    });
    assert_eq!(c.reason, FailoverReason::InvalidEncryptedContent);
    assert!(c.retryable);
}

#[test]
fn m_error_code_resource_exhausted() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "Resource exhausted",
        body_message: None,
        metadata_message: None,
        status_code: None,
        error_code: Some("resource_exhausted"),
        error_type_name: None,
        provider: "",
        model: "",
        session: ClassifyContext::default(),
    });
    assert_eq!(c.reason, FailoverReason::RateLimit);
}

#[test]
fn m_error_code_context_length() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "Context too large",
        body_message: None,
        metadata_message: None,
        status_code: None,
        error_code: Some("context_length_exceeded"),
        error_type_name: None,
        provider: "",
        model: "",
        session: ClassifyContext::default(),
    });
    assert_eq!(c.reason, FailoverReason::ContextOverflow);
    assert!(c.should_compress);
}

#[test]
fn m_502_plain_still_retryable_server() {
    let c = classify("Bad Gateway", Some(502));
    assert_eq!(c.reason, FailoverReason::ServerError);
    assert!(c.retryable);
}

#[test]
fn m_403_plan_billing() {
    assert_eq!(
        classify("This plan does not include the requested model", Some(403)).reason,
        FailoverReason::Billing
    );
}

#[test]
fn m_free_tier_billing_code() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "Model unavailable",
        body_message: None,
        metadata_message: None,
        status_code: None,
        error_code: Some("model_not_supported_on_free_tier"),
        error_type_name: None,
        provider: "nous",
        model: "gpt-5",
        session: ClassifyContext::default(),
    });
    assert_eq!(c.reason, FailoverReason::Billing);
    assert!(!c.retryable);
}

#[test]
fn m_msg_quota_reset_rate() {
    assert_eq!(
        classify("quota exceeded, resets at midnight UTC", None).reason,
        FailoverReason::RateLimit
    );
}

#[test]
fn m_msg_limit_wait_rate() {
    assert_eq!(
        classify("key limit exceeded, please wait before retrying", None).reason,
        FailoverReason::RateLimit
    );
}

#[test]
fn m_400_max_tokens_unsupported_format() {
    let msg = "Unsupported parameter: 'max_tokens' is not supported with this model";
    let c = classify_session(
        msg,
        Some(400),
        ClassifyContext {
            approx_tokens: 6_962,
            context_length: 1_050_000,
            num_messages: 0,
        },
    );
    assert_eq!(c.reason, FailoverReason::FormatError);
    assert!(!c.should_compress);
}

#[test]
fn m_400_real_overflow_still_compresses() {
    let msg = "This model's maximum context length is 128000 tokens, however you requested 150000 tokens.";
    let c = classify_session(
        msg,
        Some(400),
        ClassifyContext {
            approx_tokens: 150_000,
            context_length: 128_000,
            num_messages: 0,
        },
    );
    assert_eq!(c.reason, FailoverReason::ContextOverflow);
    assert!(c.should_compress);
}

#[test]
fn m_404_free_tier_billing_body() {
    assert_eq!(
        classify(
            "Model 'gpt-5' is not available on the Free Tier. Upgrade at portal.",
            Some(404),
        )
        .reason,
        FailoverReason::Billing
    );
}

#[test]
fn m_400_rate_in_body() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "rate limit policy",
        body_message: Some("rate limit exceeded on this model"),
        metadata_message: None,
        status_code: Some(400),
        error_code: None,
        error_type_name: None,
        provider: "openrouter",
        model: "",
        session: ClassifyContext::default(),
    });
    assert_eq!(c.reason, FailoverReason::RateLimit);
}

#[test]
fn m_400_billing_in_body() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "billing",
        body_message: Some("insufficient credits for this request"),
        metadata_message: None,
        status_code: Some(400),
        error_code: None,
        error_type_name: None,
        provider: "",
        model: "",
        session: ClassifyContext::default(),
    });
    assert_eq!(c.reason, FailoverReason::Billing);
}

#[test]
fn m_400_generic_large_overflow() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "Error",
        body_message: Some("Error"),
        metadata_message: None,
        status_code: Some(400),
        error_code: None,
        error_type_name: None,
        provider: "",
        model: "",
        session: ClassifyContext {
            approx_tokens: 100_000,
            context_length: 200_000,
            num_messages: 0,
        },
    });
    assert_eq!(c.reason, FailoverReason::ContextOverflow);
}

#[test]
fn m_400_generic_small_format() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "Error",
        body_message: Some("Error"),
        metadata_message: None,
        status_code: Some(400),
        error_code: None,
        error_type_name: None,
        provider: "",
        model: "",
        session: ClassifyContext {
            approx_tokens: 1_000,
            context_length: 200_000,
            num_messages: 0,
        },
    });
    assert_eq!(c.reason, FailoverReason::FormatError);
}

#[test]
fn m_llama_slot_context() {
    assert_eq!(
        classify(
            "slot context: 4096 tokens, prompt 8192 tokens — not enough space",
            Some(400)
        )
        .reason,
        FailoverReason::ContextOverflow
    );
}

#[test]
fn m_azure_content_filter() {
    assert_eq!(
        classify(
            "The response was filtered: ResponsibleAIPolicyViolation (finish_reason=content_filter).",
            Some(400),
        )
        .reason,
        FailoverReason::ContentPolicyBlocked
    );
}

#[test]
fn m_403_free_tier_billing_message() {
    assert_eq!(
        classify("Model 'gpt-5' is not available on the Free Tier.", None).reason,
        FailoverReason::Billing
    );
}

#[test]
fn m_deadline_exceeded_timeout() {
    assert_eq!(
        classify("deadline exceeded", None).reason,
        FailoverReason::Timeout
    );
}

#[test]
fn m_request_timed_out_timeout() {
    assert_eq!(
        classify("request timed out after 120s", None).reason,
        FailoverReason::Timeout
    );
}

#[test]
fn m_500_none_body_server() {
    assert_eq!(
        classify("fail", Some(500)).reason,
        FailoverReason::ServerError
    );
}

#[test]
fn m_empty_exception_unknown() {
    let c = classify("", None);
    assert_eq!(c.reason, FailoverReason::Unknown);
    assert!(c.retryable);
}

#[test]
fn m_401_non_retryable_with_fallback() {
    let c = classify("Unauthorized", Some(401));
    assert!(!c.retryable);
    assert!(c.should_fallback);
}

#[test]
fn m_429_rotate_and_fallback() {
    let c = classify("Too Many Requests", Some(429));
    assert!(c.should_rotate_credential);
    assert!(c.should_fallback);
}

#[test]
fn m_model_not_found_fallback() {
    let c = classify("model not found", Some(404));
    assert!(c.should_fallback);
    assert!(!c.retryable);
}

#[test]
fn m_policy_no_fallback() {
    let c = classify(
        "No endpoints available matching your guardrail restrictions and data policy",
        Some(404),
    );
    assert!(!c.should_fallback);
    assert!(!c.retryable);
}

#[test]
fn m_content_policy_no_retry() {
    let c = classify(
        "This content was flagged for possible cybersecurity risk",
        None,
    );
    assert!(!c.retryable);
    assert!(c.should_fallback);
}

#[test]
fn m_thinking_retryable() {
    let c = classify("thinking block has invalid signature", Some(400));
    assert!(c.retryable);
    assert!(!c.should_compress);
}

#[test]
fn m_oauth_beta_no_compress() {
    let c = classify(
        "The long context beta is not yet available for this subscription.",
        Some(400),
    );
    assert!(c.retryable);
    assert!(!c.should_compress);
}

#[test]
fn m_long_context_tier_compress() {
    let c = classify(
        "Extra usage is required for long context requests over 200k tokens",
        Some(429),
    );
    assert!(c.should_compress);
}

#[test]
fn m_413_compress() {
    let c = classify("Request Entity Too Large", Some(413));
    assert!(c.should_compress);
}

#[test]
fn m_invalid_encrypted_no_fallback() {
    let c = classify_api_error(ClassifyInput {
        raw_message: "invalid_encrypted_content",
        body_message: None,
        metadata_message: None,
        status_code: Some(400),
        error_code: Some("invalid_encrypted_content"),
        error_type_name: None,
        provider: "",
        model: "",
        session: ClassifyContext::default(),
    });
    assert!(!c.should_fallback);
}

#[test]
fn m_multimodal_retryable() {
    let c = classify("text is not set", Some(400));
    assert!(c.retryable);
}

#[test]
fn m_404_model_still_not_policy() {
    assert_eq!(
        classify(
            "openrouter/nonexistent-model is not a valid model ID",
            Some(404)
        )
        .reason,
        FailoverReason::ModelNotFound
    );
}

#[test]
fn m_400_input_too_long() {
    assert_eq!(
        classify("input is too long for model", Some(400)).reason,
        FailoverReason::ContextOverflow
    );
}

#[test]
fn m_400_prompt_length_exceeds() {
    assert_eq!(
        classify(
            "prompt length 200000 exceeds maximum model length 131072",
            Some(400)
        )
        .reason,
        FailoverReason::ContextOverflow
    );
}

#[test]
fn m_ssl_no_compress_on_large_session() {
    let c = classify_session(
        "[SSL: BAD_RECORD_MAC] sslv3 alert bad record mac",
        None,
        ClassifyContext {
            approx_tokens: 180_000,
            context_length: 200_000,
            num_messages: 300,
        },
    );
    assert_eq!(c.reason, FailoverReason::Timeout);
    assert!(!c.should_compress);
}

#[test]
fn m_disconnect_non_ssl_large_compress() {
    let c = classify_session(
        "Server disconnected without sending a response",
        None,
        ClassifyContext {
            approx_tokens: 180_000,
            context_length: 200_000,
            num_messages: 300,
        },
    );
    assert_eq!(c.reason, FailoverReason::ContextOverflow);
    assert!(c.should_compress);
}
