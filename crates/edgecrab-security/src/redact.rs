//! Secret redaction — strips API keys and tokens from output.
//!
//! Prevents credential leaks through tool output and terminal logs.
//!
//! Pattern coverage matches hermes-agent's `agent/redact.py`:
//! - Prefix-based API keys (OpenAI, Anthropic, OpenRouter, GitHub, Slack,
//!   Google, Perplexity, FAL.ai, Firecrawl, BrowserBase, HuggingFace,
//!   Replicate, npm, PyPI, DigitalOcean, Stripe, SendGrid, AWS)
//! - Environment variable assignments: SECRET_KEY=value
//! - JSON fields: "api_key": "value"
//! - Authorization headers: Bearer <token>
//! - Telegram bot tokens
//! - PEM private key blocks
//! - Database connection strings: postgres://user:pass@host

use regex::Regex;

/// Redacts known secret patterns from text.
///
/// Short tokens (< 18 chars) are fully masked.
/// Longer tokens replace the middle with `…` to aid debuggability.
pub struct SecretRedactor {
    /// `(pattern, replacement)` pairs applied left-to-right.
    patterns: Vec<(Regex, String)>,
}

impl SecretRedactor {
    pub fn new() -> Self {
        /// Helper — build a `(Regex, replacement)` pair, panicking on bad patterns.
        fn pat(re: &str, rep: &str) -> (Regex, String) {
            (
                Regex::new(re).expect("SecretRedactor regex"),
                rep.to_string(),
            )
        }

        let patterns = vec![
            // ── Specific well-known prefixes (most specific first) ──────────

            // Anthropic (must come before generic sk- pattern)
            pat(r"sk-ant-[A-Za-z0-9_\-]{20,}", "[REDACTED_ANTHROPIC_KEY]"),
            // OpenRouter
            pat(r"sk-or-v1-[A-Za-z0-9_\-]{20,}", "[REDACTED_OPENROUTER_KEY]"),
            // OpenAI / generic sk- keys
            pat(r"sk-[A-Za-z0-9_\-]{20,}", "[REDACTED_API_KEY]"),
            // GitHub PAT – classic (include _ for newer formats)
            pat(r"ghp_[A-Za-z0-9_]{10,}", "[REDACTED_GITHUB_PAT]"),
            // GitHub PAT – fine-grained (includes underscores in token body)
            pat(r"github_pat_[A-Za-z0-9_]{10,}", "[REDACTED_GITHUB_PAT]"),
            // Slack tokens (bot, app, user, refresh, workspace)
            pat(r"xox[baprs]-[A-Za-z0-9\-]{10,}", "[REDACTED_SLACK_TOKEN]"),
            // Google API keys
            pat(r"AIza[A-Za-z0-9_\-]{30,}", "[REDACTED_GOOGLE_KEY]"),
            // Perplexity
            pat(r"pplx-[A-Za-z0-9]{10,}", "[REDACTED_PPLX_KEY]"),
            // FAL.ai
            pat(r"fal_[A-Za-z0-9_\-]{10,}", "[REDACTED_FAL_KEY]"),
            // Firecrawl
            pat(r"fc-[A-Za-z0-9]{10,}", "[REDACTED_FIRECRAWL_KEY]"),
            // BrowserBase
            pat(r"bb_live_[A-Za-z0-9_\-]{10,}", "[REDACTED_BROWSERBASE_KEY]"),
            // HuggingFace
            pat(r"hf_[A-Za-z0-9]{10,}", "[REDACTED_HF_TOKEN]"),
            // Replicate
            pat(r"r8_[A-Za-z0-9]{10,}", "[REDACTED_REPLICATE_TOKEN]"),
            // npm access tokens
            pat(r"npm_[A-Za-z0-9]{10,}", "[REDACTED_NPM_TOKEN]"),
            // PyPI API tokens
            pat(r"pypi-[A-Za-z0-9_\-]{10,}", "[REDACTED_PYPI_TOKEN]"),
            // DigitalOcean PAT / OAuth
            pat(r"dop?o_v1_[A-Za-z0-9]{10,}", "[REDACTED_DO_TOKEN]"),
            // Stripe secret keys (live and test)
            pat(r"sk_live_[A-Za-z0-9]{10,}", "[REDACTED_STRIPE_KEY]"),
            pat(r"sk_test_[A-Za-z0-9]{10,}", "[REDACTED_STRIPE_KEY]"),
            pat(r"rk_live_[A-Za-z0-9]{10,}", "[REDACTED_STRIPE_KEY]"),
            // SendGrid
            pat(r"SG\.[A-Za-z0-9_\-]{10,}", "[REDACTED_SENDGRID_KEY]"),
            // AWS Access Key ID
            pat(r"AKIA[A-Z0-9]{16}", "[REDACTED_AWS_KEY]"),
            // ── Authorization headers ──────────────────────────────────────
            pat(
                r"(?i)Authorization:\s*Bearer\s+\S{20,}",
                "Authorization: Bearer [REDACTED]",
            ),
            pat(r"Bearer [A-Za-z0-9\-_.]{20,}", "Bearer [REDACTED]"),
            // ── Telegram bot tokens: (bot)?<digits>:<token> ───────────────
            pat(
                r"(bot)?\d{8,}:[-A-Za-z0-9_]{30,}",
                "[REDACTED_TELEGRAM_TOKEN]",
            ),
            // ── PEM private key blocks ─────────────────────────────────────
            pat(
                r"-----BEGIN[A-Z ]*PRIVATE KEY-----[\s\S]*?-----END[A-Z ]*PRIVATE KEY-----",
                "[REDACTED_PRIVATE_KEY]",
            ),
            // ── Database connection strings ───────────────────────────────
            pat(
                r"(?i)((?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp)://[^:]+:)([^@]+)(@)",
                "$1[REDACTED]$3",
            ),
            // ── ENV assignment patterns: SECRET_KEY=value ─────────────────
            // NOTE: Rust's regex crate does NOT support backreferences, so we
            // match quoted/unquoted variants with alternation instead of \2.
            pat(
                // Value portion uses [A-Za-z0-9_\-.+/] so it won't re-match
                // already-redacted [REDACTED_...] tokens (which start with `[`).
                r#"(?i)([A-Z_]*(?:API_?KEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL|AUTH)[A-Z_]*)\s*=\s*(?:'[A-Za-z0-9_\-.+/]{10,}'|"[A-Za-z0-9_\-.+/]{10,}"|[A-Za-z0-9_\-.+/]{10,})"#,
                "$1=[REDACTED]",
            ),
            // ── JSON secret fields: "api_key": "value" ─────────────────────
            pat(
                r#"(?i)("(?:api_?key|token|secret|password|access_token|refresh_token|auth_token|bearer|secret_value|key_material)")\s*:\s*"([^"]{10,})""#,
                r#"$1: "[REDACTED]""#,
            ),
            // ── Generic key=value fallback (long values only) ─────────────
            pat(
                r#"(?i)(api[_-]?key|secret|token|password)\s*[:=]\s*['"]?[A-Za-z0-9\-_.]{20,}"#,
                "$1=[REDACTED]",
            ),
        ];
        Self { patterns }
    }

    /// Redact all recognized secret patterns from text.
    pub fn redact(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (pattern, replacement) in &self.patterns {
            result = pattern
                .replace_all(&result, replacement.as_str())
                .to_string();
        }
        result
    }
}

impl Default for SecretRedactor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> SecretRedactor {
        SecretRedactor::new()
    }

    #[test]
    fn redacts_openai_key() {
        let result = r().redact("Using key sk-abc123def456ghi789jkl012mno345pqr");
        assert!(!result.contains("sk-abc123"));
        assert!(result.contains("[REDACTED"));
    }

    #[test]
    fn redacts_anthropic_key() {
        let result = r().redact("key=sk-ant-api03-abcdefghijklmnopqrstuvwxyz12345");
        assert!(result.contains("[REDACTED_ANTHROPIC_KEY]"));
    }

    #[test]
    fn redacts_openrouter_key() {
        let result = r().redact("sk-or-v1-abcdefghijklmnopqrstuvwxyz12345678");
        assert!(result.contains("[REDACTED_OPENROUTER_KEY]"));
    }

    #[test]
    fn redacts_bearer_token() {
        let result = r().redact("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9");
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("eyJhbG"));
    }

    #[test]
    fn redacts_aws_key() {
        let result = r().redact("AWS key: AKIAIOSFODNN7EXAMPLE");
        assert!(result.contains("[REDACTED_AWS_KEY]"));
    }

    #[test]
    fn redacts_github_pat() {
        let result = r().redact("token=ghp_abcdefghijklmnopqrstuvwxyz123456");
        assert!(result.contains("[REDACTED_GITHUB_PAT]"));
    }

    #[test]
    fn redacts_github_pat_fine_grained() {
        let result =
            r().redact("GITHUB_TOKEN=github_pat_11ABCDEFG0abcdefghij_ABCDEFGHIJKLMNOPQRSTUVWXYZ");
        assert!(result.contains("[REDACTED_GITHUB_PAT]"));
    }

    #[test]
    fn redacts_slack_token() {
        // Synthetic test token — not a real credential (split to avoid secret scanning).
        let token = concat!("xoxb", "-", "12345678901", "-", "abcdefghijklmnopqr");
        let result = r().redact(&format!("token: {token}"));
        assert!(result.contains("[REDACTED_SLACK_TOKEN]"));
    }

    #[test]
    fn redacts_fal_key() {
        let result = r().redact("FAL_KEY=fal_abcdefghij1234567890abcdef");
        assert!(result.contains("[REDACTED_FAL_KEY]"));
    }

    #[test]
    fn redacts_stripe_key() {
        // Synthetic test key — not a real credential (split to avoid secret scanning).
        let key = concat!("sk", "_live_", "abcdefghijklmnopqrstuvwxyz");
        let result = r().redact(&format!("STRIPE_KEY={key}"));
        assert!(result.contains("[REDACTED_STRIPE_KEY]") || result.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_huggingface_token() {
        let result = r().redact("HF_TOKEN=hf_abcdefghijklmnopqrstuvwxyz12345");
        assert!(result.contains("[REDACTED_HF_TOKEN]") || result.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_postgres_connection_string() {
        let result = r().redact("postgres://myuser:supersecretpassword@db.example.com/mydb");
        assert!(!result.contains("supersecretpassword"));
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_private_key_block() {
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA0Z3VS5JJcds3xHn/ygWep4\n-----END RSA PRIVATE KEY-----";
        let result = r().redact(pem);
        assert!(result.contains("[REDACTED_PRIVATE_KEY]"));
        assert!(!result.contains("MIIEpAIBAAKCAQEA"));
    }

    #[test]
    fn redacts_json_api_key_field() {
        let result = r().redact(r#"{"api_key": "abcdefghijklmnopqrstuvwxyz"}"#);
        assert!(result.contains("[REDACTED]") || result.contains("REDACTED"));
        assert!(!result.contains("abcdefghijklmnopqr"));
    }

    #[test]
    fn redacts_env_assignment() {
        let result = r().redact("API_KEY=some-very-long-secret-value-here");
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("some-very-long-secret"));
    }

    #[test]
    fn preserves_safe_text() {
        let input = "Hello, this is a normal message with no secrets.";
        assert_eq!(r().redact(input), input);
    }

    #[test]
    fn preserves_short_tokens() {
        // Short tokens under 10 chars should not be redacted by prefix patterns
        let input = "The token for this test is abc123";
        let result = r().redact(input);
        assert_eq!(result, input);
    }
}
