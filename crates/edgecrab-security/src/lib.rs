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
pub mod normalize;
pub mod path_jail;
pub mod path_policy;
pub mod redact;
pub mod url_safety;

/// Re-export injection checks at crate root for convenience.
pub use injection::{check_injection, check_memory_content};
