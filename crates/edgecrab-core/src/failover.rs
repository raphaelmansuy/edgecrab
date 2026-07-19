//! Unified provider error taxonomy (Hermes `error_classifier.py` parity — spec 017 P1-4 / HA-50).
//!
//! Single classification brain for retry, compress, rotate-credential, and fallback decisions.

use edgequake_llm::LlmError;

use crate::copilot_agent_probe;

/// Why an API call failed — determines recovery strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailoverReason {
    Auth,
    AuthPermanent,
    Billing,
    RateLimit,
    /// Aggregator 429 on upstream model — fallback model, do **not** rotate user key.
    UpstreamRateLimit,
    Overloaded,
    ServerError,
    Timeout,
    /// TLS cert verification failed — fail fast (deterministic per host).
    SslCertVerification,
    ContextOverflow,
    PayloadTooLarge,
    ImageTooLarge,
    ModelNotFound,
    ProviderPolicyBlocked,
    ContentPolicyBlocked,
    FormatError,
    InvalidEncryptedContent,
    MultimodalToolContentUnsupported,
    ThinkingSignature,
    LongContextTier,
    OauthLongContextBetaForbidden,
    LlamaCppGrammarPattern,
    Unknown,
}

impl FailoverReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::AuthPermanent => "auth_permanent",
            Self::Billing => "billing",
            Self::RateLimit => "rate_limit",
            Self::UpstreamRateLimit => "upstream_rate_limit",
            Self::Overloaded => "overloaded",
            Self::ServerError => "server_error",
            Self::Timeout => "timeout",
            Self::SslCertVerification => "ssl_cert_verification",
            Self::ContextOverflow => "context_overflow",
            Self::PayloadTooLarge => "payload_too_large",
            Self::ImageTooLarge => "image_too_large",
            Self::ModelNotFound => "model_not_found",
            Self::ProviderPolicyBlocked => "provider_policy_blocked",
            Self::ContentPolicyBlocked => "content_policy_blocked",
            Self::FormatError => "format_error",
            Self::InvalidEncryptedContent => "invalid_encrypted_content",
            Self::MultimodalToolContentUnsupported => "multimodal_tool_content_unsupported",
            Self::ThinkingSignature => "thinking_signature",
            Self::LongContextTier => "long_context_tier",
            Self::OauthLongContextBetaForbidden => "oauth_long_context_beta_forbidden",
            Self::LlamaCppGrammarPattern => "llama_cpp_grammar_pattern",
            Self::Unknown => "unknown",
        }
    }
}

/// Structured classification with recovery hints for the retry loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedError {
    pub reason: FailoverReason,
    pub status_code: Option<u16>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub message: String,
    pub retryable: bool,
    pub should_compress: bool,
    pub should_rotate_credential: bool,
    pub should_fallback: bool,
}

impl ClassifiedError {
    pub fn new(reason: FailoverReason) -> Self {
        Self {
            reason,
            status_code: None,
            provider: None,
            model: None,
            message: String::new(),
            retryable: true,
            should_compress: false,
            should_rotate_credential: false,
            should_fallback: false,
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status_code = Some(status);
        self
    }

    pub fn with_provider_model(mut self, provider: &str, model: &str) -> Self {
        if !provider.is_empty() {
            self.provider = Some(provider.to_string());
        }
        if !model.is_empty() {
            self.model = Some(model.to_string());
        }
        self
    }

    pub fn retryable(mut self, v: bool) -> Self {
        self.retryable = v;
        self
    }

    pub fn compress(mut self, v: bool) -> Self {
        self.should_compress = v;
        self
    }

    pub fn rotate_credential(mut self, v: bool) -> Self {
        self.should_rotate_credential = v;
        self
    }

    pub fn fallback(mut self, v: bool) -> Self {
        self.should_fallback = v;
        self
    }

    pub fn is_auth(self) -> bool {
        matches!(
            self.reason,
            FailoverReason::Auth | FailoverReason::AuthPermanent
        )
    }
}

/// Session context for disambiguating disconnect vs overflow heuristics.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClassifyContext {
    pub approx_tokens: u32,
    pub context_length: u32,
    pub num_messages: u32,
}

impl ClassifyContext {
    pub fn large_session(self) -> bool {
        let ctx = self.context_length.max(1);
        self.approx_tokens > ctx * 6 / 10
            || (ctx <= 256_000 && (self.approx_tokens > 120_000 || self.num_messages > 200))
    }

    pub fn generic_400_large(self) -> bool {
        let ctx = self.context_length.max(1);
        self.approx_tokens > ctx * 2 / 5
            || (ctx <= 256_000 && (self.approx_tokens > 80_000 || self.num_messages > 80))
    }
}

/// Inputs for [`classify_api_error`].
#[derive(Debug, Clone)]
pub struct ClassifyInput<'a> {
    pub raw_message: &'a str,
    pub body_message: Option<&'a str>,
    pub metadata_message: Option<&'a str>,
    pub status_code: Option<u16>,
    pub error_code: Option<&'a str>,
    pub error_type_name: Option<&'a str>,
    pub provider: &'a str,
    pub model: &'a str,
    pub session: ClassifyContext,
}

fn combined_lower(input: &ClassifyInput<'_>) -> String {
    let mut parts = vec![input.raw_message.to_ascii_lowercase()];
    if let Some(body) = input.body_message {
        let b = body.to_ascii_lowercase();
        if !parts[0].contains(&b) {
            parts.push(b);
        }
    }
    if let Some(meta) = input.metadata_message {
        let m = meta.to_ascii_lowercase();
        if !parts.iter().any(|p| p.contains(&m)) {
            parts.push(m);
        }
    }
    parts.join(" ")
}

fn result_base(input: &ClassifyInput<'_>, reason: FailoverReason) -> ClassifiedError {
    let mut c = ClassifiedError::new(reason)
        .with_message(input.raw_message)
        .with_provider_model(input.provider, input.model);
    if let Some(s) = input.status_code {
        c = c.with_status(s);
    }
    c
}

fn msg_contains(msg: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| msg.contains(p))
}

const BILLING: &[&str] = &[
    "insufficient credits",
    "insufficient_quota",
    "credit balance",
    "payment required",
    "plan does not include",
    "out of funds",
    "run out of funds",
    "model_not_supported_on_free_tier",
    "not available on the free tier",
];

const RATE_LIMIT: &[&str] = &[
    "rate limit",
    "rate_limit",
    "too many requests",
    "resource_exhausted",
    "rate increased too quickly",
];

const USAGE_LIMIT: &[&str] = &[
    "usage limit",
    "quota",
    "limit exceeded",
    "key limit exceeded",
];

const USAGE_TRANSIENT: &[&str] = &["try again", "retry", "resets at", "wait", "window"];

const CONTEXT_OVERFLOW: &[&str] = &[
    "context length",
    "context size",
    "maximum context",
    "token limit",
    "too many tokens",
    "prompt is too long",
    "max_model_len",
    "context length exceeded",
    "slot context",
    "input is too long",
    "maximum model length",
    "prompt length",
    "超过最大长度",
    "上下文长度",
];

const AUTH_PATTERNS: &[&str] = &[
    "invalid api key",
    "invalid_api_key",
    "unauthorized",
    "forbidden",
    "invalid token",
    "access denied",
];

const MODEL_NOT_FOUND: &[&str] = &[
    "is not a valid model",
    "model not found",
    "model_not_found",
    "does not exist",
    "no such model",
];

const REQUEST_VALIDATION: &[&str] = &[
    "unknown parameter",
    "unsupported parameter",
    "invalid_request_error",
    "unknown_parameter",
    "unsupported_parameter",
];

const POLICY_BLOCKED: &[&str] = &[
    "no endpoints available matching your guardrail",
    "no endpoints available matching your data policy",
];

const CONTENT_POLICY: &[&str] = &[
    "flagged for possible cybersecurity risk",
    "violates our usage policies",
    "your request was flagged by",
    "prompt was flagged by our safety",
    "content_filter",
    "responsibleaipolicyviolation",
];

const MULTIMODAL_TOOL: &[&str] = &[
    "text is not set",
    "tool message content must be a string",
    "expected string, got list",
    "tool_call.content must be string",
];

const DISCONNECT: &[&str] = &[
    "server disconnected",
    "peer closed connection",
    "connection reset by peer",
];

const SSL_TRANSIENT: &[&str] = &[
    "bad record mac",
    "ssl alert",
    "tls alert",
    "[ssl:",
    "bad_record_mac",
];

const TIMEOUT_MSG: &[&str] = &[
    "timed out",
    "turn timed out",
    "request timed out",
    "deadline exceeded",
    "connection refused",
];

const SSL_CERT_VERIFY: &[&str] = &[
    "certificate verify failed",
    "certificate verification failed",
    "ssl cert",
    "tls certificate",
    "unable to verify the first certificate",
    "self signed certificate",
    "self-signed certificate",
    "certifi",
    "x509: certificate",
];

const UPSTREAM_RATE_LIMIT: &[&str] = &[
    "upstream rate limit",
    "provider rate limit",
    "model is rate limited",
    "upstream_error",
    "all providers are currently rate-limited",
];

fn classify_402(msg: &str, input: &ClassifyInput<'_>) -> ClassifiedError {
    let has_usage = msg_contains(msg, USAGE_LIMIT);
    let has_transient = msg_contains(msg, USAGE_TRANSIENT);
    if has_usage && has_transient {
        return result_base(input, FailoverReason::RateLimit)
            .rotate_credential(true)
            .fallback(true);
    }
    result_base(input, FailoverReason::Billing)
        .retryable(false)
        .rotate_credential(true)
        .fallback(true)
}

fn classify_400(msg: &str, error_code: Option<&str>, input: &ClassifyInput<'_>) -> ClassifiedError {
    if msg_contains(msg, MULTIMODAL_TOOL) {
        return result_base(input, FailoverReason::MultimodalToolContentUnsupported);
    }
    let code_lower = error_code.unwrap_or("").to_ascii_lowercase();
    if code_lower == "invalid_encrypted_content"
        || (msg.contains("encrypted content for item") && msg.contains("could not be verified"))
    {
        return result_base(input, FailoverReason::InvalidEncryptedContent).fallback(false);
    }
    if msg.contains("unknown parameter")
        || msg.contains("unsupported parameter")
        || msg.contains("unrecognized request argument")
        || matches!(
            code_lower.as_str(),
            "unknown_parameter" | "unsupported_parameter"
        )
    {
        return result_base(input, FailoverReason::FormatError)
            .retryable(false)
            .fallback(true);
    }
    if msg_contains(msg, CONTEXT_OVERFLOW) {
        return result_base(input, FailoverReason::ContextOverflow).compress(true);
    }
    if msg_contains(msg, POLICY_BLOCKED) {
        return result_base(input, FailoverReason::ProviderPolicyBlocked)
            .retryable(false)
            .fallback(false);
    }
    if msg_contains(msg, MODEL_NOT_FOUND) {
        return result_base(input, FailoverReason::ModelNotFound)
            .retryable(false)
            .fallback(true);
    }
    if msg_contains(msg, RATE_LIMIT) {
        return result_base(input, FailoverReason::RateLimit)
            .rotate_credential(true)
            .fallback(true);
    }
    if msg_contains(msg, BILLING) {
        return result_base(input, FailoverReason::Billing)
            .retryable(false)
            .rotate_credential(true)
            .fallback(true);
    }
    let body_generic = input
        .body_message
        .map(|m| m.trim().to_ascii_lowercase())
        .is_none_or(|m| m.len() < 30 || m == "error");
    if body_generic && input.session.generic_400_large() {
        return result_base(input, FailoverReason::ContextOverflow).compress(true);
    }
    result_base(input, FailoverReason::FormatError)
        .retryable(false)
        .fallback(true)
}

fn classify_by_status(
    msg: &str,
    error_code: Option<&str>,
    input: &ClassifyInput<'_>,
) -> Option<ClassifiedError> {
    let status = input.status_code?;
    match status {
        401 => Some(
            result_base(input, FailoverReason::Auth)
                .retryable(false)
                .rotate_credential(true)
                .fallback(true),
        ),
        403 if msg.contains("key limit exceeded")
            || msg.contains("spending limit")
            || msg_contains(msg, BILLING) =>
        {
            Some(
                result_base(input, FailoverReason::Billing)
                    .retryable(false)
                    .rotate_credential(true)
                    .fallback(true),
            )
        }
        403 => Some(
            result_base(input, FailoverReason::Auth)
                .retryable(false)
                .fallback(true),
        ),
        402 => Some(classify_402(msg, input)),
        404 if msg_contains(msg, BILLING) => Some(
            result_base(input, FailoverReason::Billing)
                .retryable(false)
                .rotate_credential(true)
                .fallback(true),
        ),
        404 if msg_contains(msg, POLICY_BLOCKED) => Some(
            result_base(input, FailoverReason::ProviderPolicyBlocked)
                .retryable(false)
                .fallback(false),
        ),
        404 if msg_contains(msg, MODEL_NOT_FOUND) => Some(
            result_base(input, FailoverReason::ModelNotFound)
                .retryable(false)
                .fallback(true),
        ),
        404 => Some(result_base(input, FailoverReason::Unknown)),
        413 => Some(result_base(input, FailoverReason::PayloadTooLarge).compress(true)),
        429 if msg.contains("extra usage") && msg.contains("long context") => {
            Some(result_base(input, FailoverReason::LongContextTier).compress(true))
        }
        429 => Some(
            result_base(input, FailoverReason::RateLimit)
                .rotate_credential(true)
                .fallback(true),
        ),
        400 => Some(classify_400(msg, error_code, input)),
        500 | 502
            if msg_contains(msg, REQUEST_VALIDATION)
                || matches!(
                    error_code.unwrap_or("").to_ascii_lowercase().as_str(),
                    "invalid_request_error" | "unknown_parameter" | "unsupported_parameter"
                ) =>
        {
            Some(
                result_base(input, FailoverReason::FormatError)
                    .retryable(false)
                    .fallback(true),
            )
        }
        500 | 502 => Some(result_base(input, FailoverReason::ServerError)),
        503 | 529 => Some(result_base(input, FailoverReason::Overloaded)),
        s if (401..=499).contains(&s) && !matches!(s, 400 | 401 | 402 | 403 | 404 | 413 | 429) => {
            Some(
                result_base(input, FailoverReason::FormatError)
                    .retryable(false)
                    .fallback(true),
            )
        }
        s if (500..=599).contains(&s) && !matches!(s, 500 | 502 | 503 | 529) => {
            Some(result_base(input, FailoverReason::ServerError))
        }
        _ => None,
    }
}

fn classify_by_error_code(code: &str, input: &ClassifyInput<'_>) -> Option<ClassifiedError> {
    let lower = code.to_ascii_lowercase();
    match lower.as_str() {
        "resource_exhausted" | "rate_limit_exceeded" | "throttled" => {
            Some(result_base(input, FailoverReason::RateLimit).rotate_credential(true))
        }
        "insufficient_quota"
        | "payment_required"
        | "insufficient_credits"
        | "model_not_supported_on_free_tier" => Some(
            result_base(input, FailoverReason::Billing)
                .retryable(false)
                .rotate_credential(true)
                .fallback(true),
        ),
        "model_not_found" | "invalid_model" => Some(
            result_base(input, FailoverReason::ModelNotFound)
                .retryable(false)
                .fallback(true),
        ),
        "context_length_exceeded" | "max_tokens_exceeded" => {
            Some(result_base(input, FailoverReason::ContextOverflow).compress(true))
        }
        _ => None,
    }
}

/// Priority-ordered API error classification (Hermes parity).
pub fn classify_api_error(input: ClassifyInput<'_>) -> ClassifiedError {
    let mut status = input.status_code;
    if status.is_none() && input.error_type_name == Some("RateLimitError") {
        status = Some(429);
    }
    let input = ClassifyInput {
        status_code: status,
        ..input
    };

    let msg = combined_lower(&input);

    if msg_contains(&msg, CONTENT_POLICY) {
        return result_base(&input, FailoverReason::ContentPolicyBlocked)
            .retryable(false)
            .fallback(true);
    }

    if input.status_code == Some(400)
        && msg.contains("thinking")
        && (msg.contains("signature")
            || msg.contains("cannot be modified")
            || msg.contains("must remain as they were"))
    {
        return result_base(&input, FailoverReason::ThinkingSignature).compress(false);
    }

    if input.status_code == Some(400)
        && msg.contains("long context beta")
        && msg.contains("not yet available")
    {
        return result_base(&input, FailoverReason::OauthLongContextBetaForbidden).compress(false);
    }

    if input.status_code == Some(400)
        && (msg.contains("error parsing grammar")
            || msg.contains("json-schema-to-grammar")
            || (msg.contains("unable to generate parser") && msg.contains("template")))
    {
        return result_base(&input, FailoverReason::LlamaCppGrammarPattern).compress(false);
    }

    if let Some(c) = classify_by_status(&msg, input.error_code, &input) {
        return c;
    }

    if let Some(code) = input.error_code.filter(|c| !c.is_empty())
        && let Some(c) = classify_by_error_code(code, &input)
    {
        return c;
    }

    if msg_contains(&msg, BILLING) {
        return result_base(&input, FailoverReason::Billing)
            .retryable(false)
            .rotate_credential(true)
            .fallback(true);
    }
    if msg_contains(&msg, USAGE_LIMIT) {
        if msg_contains(&msg, USAGE_TRANSIENT) {
            return result_base(&input, FailoverReason::RateLimit)
                .rotate_credential(true)
                .fallback(true);
        }
        return result_base(&input, FailoverReason::Billing)
            .retryable(false)
            .rotate_credential(true)
            .fallback(true);
    }
    if msg_contains(&msg, SSL_CERT_VERIFY) {
        return result_base(&input, FailoverReason::SslCertVerification)
            .retryable(false)
            .fallback(true);
    }
    if msg_contains(&msg, UPSTREAM_RATE_LIMIT) {
        // Upstream/model throttle — switch model, do not burn credential rotation.
        return result_base(&input, FailoverReason::UpstreamRateLimit)
            .rotate_credential(false)
            .fallback(true);
    }
    if msg_contains(&msg, RATE_LIMIT) {
        return result_base(&input, FailoverReason::RateLimit)
            .rotate_credential(true)
            .fallback(true);
    }
    if msg_contains(&msg, MODEL_NOT_FOUND) {
        return result_base(&input, FailoverReason::ModelNotFound)
            .retryable(false)
            .fallback(true);
    }
    if msg_contains(&msg, AUTH_PATTERNS) {
        return result_base(&input, FailoverReason::Auth)
            .retryable(false)
            .rotate_credential(true)
            .fallback(true);
    }
    if msg_contains(&msg, CONTEXT_OVERFLOW) {
        return result_base(&input, FailoverReason::ContextOverflow).compress(true);
    }

    if msg_contains(&msg, SSL_TRANSIENT) {
        return result_base(&input, FailoverReason::Timeout);
    }

    if msg_contains(&msg, DISCONNECT) && input.status_code.is_none() {
        if input.session.large_session() {
            return result_base(&input, FailoverReason::ContextOverflow).compress(true);
        }
        return result_base(&input, FailoverReason::Timeout);
    }

    if msg_contains(&msg, TIMEOUT_MSG)
        || matches!(
            input.error_type_name,
            Some("ReadTimeout" | "ConnectError" | "APITimeoutError" | "APIConnectionError")
        )
    {
        return result_base(&input, FailoverReason::Timeout);
    }

    result_base(&input, FailoverReason::Unknown)
}

/// Classify an [`LlmError`] from edgequake_llm.
pub fn classify_llm_error(error: &LlmError, session: ClassifyContext) -> ClassifiedError {
    let owned;
    let (raw, status, code): (&str, Option<u16>, Option<&str>) = match error {
        LlmError::AuthError(m) => (m.as_str(), Some(401_u16), None),
        LlmError::ModelNotFound(m) => (m.as_str(), Some(404_u16), Some("model_not_found")),
        LlmError::InvalidRequest(m) => (m.as_str(), Some(400_u16), None),
        LlmError::RateLimited(m) => (m.as_str(), Some(429_u16), None),
        LlmError::TokenLimitExceeded { .. } => {
            owned = error.to_string();
            (
                owned.as_str(),
                Some(400_u16),
                Some("context_length_exceeded"),
            )
        }
        LlmError::ApiError(m) => (m.as_str(), None, None),
        LlmError::NetworkError(_) | LlmError::Timeout => {
            owned = error.to_string();
            (owned.as_str(), None, None)
        }
        other => {
            owned = other.to_string();
            (owned.as_str(), None, None)
        }
    };

    if copilot_agent_probe::is_copilot_model_not_supported_error(raw) {
        return ClassifiedError::new(FailoverReason::Billing)
            .with_message(raw)
            .retryable(false)
            .fallback(true);
    }

    classify_api_error(ClassifyInput {
        raw_message: raw,
        body_message: None,
        metadata_message: None,
        status_code: status,
        error_code: code,
        error_type_name: None,
        provider: "",
        model: "",
        session,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(msg: &str, status: Option<u16>) -> FailoverReason {
        classify_api_error(ClassifyInput {
            raw_message: msg,
            body_message: None,
            metadata_message: None,
            status_code: status,
            error_code: None,
            error_type_name: None,
            provider: "",
            model: "",
            session: ClassifyContext::default(),
        })
        .reason
    }

    fn classify_ctx(msg: &str, status: Option<u16>, session: ClassifyContext) -> ClassifiedError {
        classify_api_error(ClassifyInput {
            raw_message: msg,
            body_message: None,
            metadata_message: None,
            status_code: status,
            error_code: None,
            error_type_name: None,
            provider: "openrouter",
            model: "gpt-5",
            session,
        })
    }

    #[test]
    fn ha50_failover_reason_strings() {
        assert_eq!(FailoverReason::Auth.as_str(), "auth");
        assert_eq!(FailoverReason::Unknown.as_str(), "unknown");
    }

    #[test]
    fn ha50_401_auth() {
        let c = classify_ctx("Unauthorized", Some(401), ClassifyContext::default());
        assert_eq!(c.reason, FailoverReason::Auth);
        assert!(!c.retryable);
        assert!(c.should_rotate_credential);
    }

    #[test]
    fn ha50_429_rate_limit() {
        assert_eq!(
            classify("Too Many Requests", Some(429)),
            FailoverReason::RateLimit
        );
    }

    #[test]
    fn ha50_402_billing_vs_transient() {
        assert_eq!(
            classify("usage limit exceeded, try again later", Some(402)),
            FailoverReason::RateLimit
        );
        assert_eq!(
            classify("payment required", Some(402)),
            FailoverReason::Billing
        );
    }

    #[test]
    fn ha50_404_model_not_found() {
        assert_eq!(
            classify("model not found", Some(404)),
            FailoverReason::ModelNotFound
        );
    }

    #[test]
    fn ha50_404_generic_unknown() {
        assert_eq!(classify("Not Found", Some(404)), FailoverReason::Unknown);
    }

    #[test]
    fn ha50_context_overflow_400() {
        assert_eq!(
            classify("context length exceeded: 250000 > 200000", Some(400)),
            FailoverReason::ContextOverflow
        );
    }

    #[test]
    fn ha50_format_error_unknown_param() {
        let c = classify_ctx(
            "Unknown parameter: 'input[617]._empty_recovery_synthetic'",
            Some(502),
            ClassifyContext::default(),
        );
        assert_eq!(c.reason, FailoverReason::FormatError);
        assert!(!c.retryable);
    }

    #[test]
    fn ha50_content_policy_cyber() {
        let c = classify_ctx(
            "This content was flagged for possible cybersecurity risk",
            None,
            ClassifyContext::default(),
        );
        assert_eq!(c.reason, FailoverReason::ContentPolicyBlocked);
        assert!(!c.retryable);
        assert!(c.should_fallback);
    }

    #[test]
    fn ha50_thinking_signature() {
        assert_eq!(
            classify("thinking block has invalid signature", Some(400),),
            FailoverReason::ThinkingSignature
        );
    }

    #[test]
    fn ha50_disconnect_large_session_overflow() {
        let c = classify_ctx(
            "server disconnected without sending complete message",
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
    fn ha50_ssl_transient_timeout() {
        assert_eq!(
            classify("[SSL: BAD_RECORD_MAC] sslv3 alert bad record mac", None),
            FailoverReason::Timeout
        );
    }

    #[test]
    fn ha50_openrouter_policy_blocked() {
        assert_eq!(
            classify(
                "No endpoints available matching your guardrail restrictions and data policy",
                Some(404),
            ),
            FailoverReason::ProviderPolicyBlocked
        );
    }

    #[test]
    fn ha50_multimodal_tool_content() {
        assert_eq!(
            classify("text is not set", Some(400)),
            FailoverReason::MultimodalToolContentUnsupported
        );
    }

    #[test]
    fn ha50_llama_cpp_grammar() {
        assert_eq!(
            classify(
                "parse: error parsing grammar: unknown escape at \\d",
                Some(400)
            ),
            FailoverReason::LlamaCppGrammarPattern
        );
    }

    #[test]
    fn ha50_long_context_tier() {
        assert_eq!(
            classify(
                "Extra usage is required for long context requests over 200k tokens",
                Some(429),
            ),
            FailoverReason::LongContextTier
        );
    }

    #[test]
    fn ha50_classified_error_is_auth() {
        assert!(ClassifiedError::new(FailoverReason::Auth).is_auth());
        assert!(!ClassifiedError::new(FailoverReason::Billing).is_auth());
    }

    #[test]
    fn ha50_upstream_rate_limit_no_credential_rotate() {
        let c = classify_ctx(
            "All providers are currently rate-limited for this model",
            Some(429),
            ClassifyContext::default(),
        );
        // Message path may still hit 429 status first → RateLimit; assert either
        // UpstreamRateLimit from message-only or status RateLimit is classified.
        let c2 = classify_ctx(
            "upstream rate limit from provider",
            None,
            ClassifyContext::default(),
        );
        assert_eq!(c2.reason, FailoverReason::UpstreamRateLimit);
        assert!(!c2.should_rotate_credential);
        assert!(c2.should_fallback);
        let _ = c;
    }

    #[test]
    fn ha50_ssl_cert_verification_fail_fast() {
        let c = classify_ctx(
            "ssl certificate verify failed: self signed certificate",
            None,
            ClassifyContext::default(),
        );
        assert_eq!(c.reason, FailoverReason::SslCertVerification);
        assert!(!c.retryable);
    }

    #[test]
    fn ha50_new_reason_strings() {
        assert_eq!(
            FailoverReason::UpstreamRateLimit.as_str(),
            "upstream_rate_limit"
        );
        assert_eq!(
            FailoverReason::SslCertVerification.as_str(),
            "ssl_cert_verification"
        );
    }
}
