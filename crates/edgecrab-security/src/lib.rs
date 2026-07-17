//! # edgecrab-security
//!
//! Security primitives for the EdgeCrab agent:
//! - Path traversal prevention (jail check)
//! - URL safety (SSRF prevention)
//! - Command injection scanning (~30 destructive patterns)
//! - Prompt injection detection for user-supplied content
//! - Command normalization (ANSI strip + NFKC + null byte removal)
//! - Secret redaction (API keys, tokens in output)
//! - Approval policy engine (manual / smart / off)

#![deny(clippy::unwrap_used)]

pub mod approval;
pub mod command_scan;
pub mod injection;
pub mod threat_patterns;
pub mod normalize;
pub mod os_sandbox;
pub mod path_jail;
pub mod path_policy;
pub mod proxy;
pub mod redact;
pub mod secrets;
pub mod url_safety;
pub mod url_validation;
pub mod website_policy;

pub use secrets::{
    EnvSecretBackend, FileSecretBackend, SecretBackend, SecretError, SecretResolver,
    default_resolver,
};

/// Re-export injection checks at crate root for convenience.
pub use injection::{check_injection, check_memory_content};
pub use os_sandbox::{OsSandboxMode, WrappedCommand, wrap_command, wrapped_to_shell_line};
pub use threat_patterns::{
    ScanContext, ScanResult as ThreatScanResult, Verdict as ThreatVerdict,
    prepare_tool_result_body, scan, tool_output_delimiters_enabled, wrap_tool_result,
};

/// Check for CRLF injection in header values.
///
/// Returns `Err` if the value contains carriage return (`\r`), newline (`\n`),
/// or null bytes — all of which can be abused for HTTP header injection.
///
/// # Examples
/// ```
/// # use edgecrab_security::validate_header_value;
/// assert!(validate_header_value("clean-value-123").is_ok());
/// assert!(validate_header_value("evil\r\nInjected: header").is_err());
/// assert!(validate_header_value("null\0byte").is_err());
/// ```
pub fn validate_header_value(value: &str) -> Result<(), &'static str> {
    if value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
        Err("header value contains CRLF or null byte")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_clean_value_ok() {
        assert!(validate_header_value("session-abc-123").is_ok());
        assert!(validate_header_value("Bearer sk-abc").is_ok());
        assert!(validate_header_value("").is_ok());
    }

    #[test]
    fn crlf_injection_blocked() {
        assert!(validate_header_value("evil\r\nX-Injected: true").is_err());
        assert!(validate_header_value("evil\nX-Injected: true").is_err());
        assert!(validate_header_value("evil\rX-Injected: true").is_err());
    }

    #[test]
    fn crlf_null_byte_blocked() {
        assert!(validate_header_value("null\0byte").is_err());
    }

    #[test]
    fn crlf_unicode_ok() {
        // Unicode is fine — only raw control bytes are blocked
        assert!(validate_header_value("日本語-value").is_ok());
        assert!(validate_header_value("emoji-🦀").is_ok());
    }
}
