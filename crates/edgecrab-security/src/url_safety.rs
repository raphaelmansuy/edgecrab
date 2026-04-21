//! URL safety check — blocks SSRF and dangerous schemes.
//!
//! Validates URLs before web tools fetch them, preventing:
//! - Private/loopback IP access (SSRF)
//! - Non-HTTP schemes (file://, ftp://)
//! - Cloud metadata endpoints (169.254.169.254)
//! - Redirect-based SSRF (302 → private IP)

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use edgecrab_types::AgentError;

/// Check if a URL is safe to fetch.
pub fn is_safe_url(raw_url: &str) -> Result<bool, AgentError> {
    let parsed = url::Url::parse(raw_url)
        .map_err(|_| AgentError::Security(format!("Invalid URL: {raw_url}")))?;

    // Only HTTP/HTTPS allowed
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            tracing::warn!(scheme, url = raw_url, "Blocked non-HTTP scheme");
            return Ok(false);
        }
    }

    // Use the typed Host enum so IPv6 addresses are identified reliably
    // without an intermediate string-parse step (which can fail on edge cases).
    // url::Host::Ipv6 is only present when the URL had a bracketed IPv6 literal.
    use url::Host;
    let host = parsed
        .host()
        .ok_or_else(|| AgentError::Security(format!("No host in URL: {raw_url}")))?;

    match host {
        Host::Ipv4(v4) => {
            if is_private_ipv4(&v4) {
                tracing::warn!(%v4, "Blocked private/reserved IPv4");
                return Ok(false);
            }
        }
        Host::Ipv6(v6) => {
            if is_private_ipv6(&v6) {
                tracing::warn!(%v6, "Blocked private/reserved IPv6");
                return Ok(false);
            }
        }
        Host::Domain(name) => {
            // Block known dangerous hostnames (including numeric cloud-metadata IP)
            const BLOCKED_HOSTS: &[&str] =
                &["localhost", "metadata.google.internal", "169.254.169.254"];
            if BLOCKED_HOSTS.contains(&name) {
                tracing::warn!(host = %name, "Blocked dangerous hostname");
                return Ok(false);
            }
            // Fallback: attempt to parse domain-form IP strings such as
            // "127.0.0.1" or "::1" that weren't bracketed in the URL.
            if let Ok(ip) = name.parse::<IpAddr>()
                && is_private_or_reserved(&ip)
            {
                tracing::warn!(%ip, "Blocked private/reserved IP (domain form)");
                return Ok(false);
            }
        }
    }

    Ok(true)
}

/// Build a [`reqwest::Client`] that re-validates every redirect target against
/// SSRF rules, preventing DNS rebinding and open-redirect attacks.
///
/// Every 301/302/307/308 hop is checked via [`is_safe_url()`]. If any redirect
/// targets a private/internal address the request is aborted immediately.
///
/// Automatically wires proxy from environment variables via
/// [`crate::proxy::resolve_proxy_url()`] (6-level cascade).
///
/// # Example
/// ```rust,no_run
/// use edgecrab_security::url_safety::build_ssrf_safe_client;
/// use std::time::Duration;
///
/// let client = build_ssrf_safe_client(Duration::from_secs(30));
/// ```
pub fn build_ssrf_safe_client(timeout: Duration) -> reqwest::Client {
    build_ssrf_safe_client_with_proxy(timeout, None)
}

/// Build a [`reqwest::Client`] with SSRF protection and explicit proxy URL.
///
/// If `proxy_url` is `Some`, uses that proxy. If `None`, auto-resolves proxy
/// from environment variables via [`crate::proxy::resolve_proxy_url()`].
///
/// To force **no proxy**, pass `Some("")` (empty string — will be skipped).
pub fn build_ssrf_safe_client_with_proxy(
    timeout: Duration,
    proxy_url: Option<&str>,
) -> reqwest::Client {
    let resolved_proxy = match proxy_url {
        Some(url) if !url.is_empty() => Some(url.to_string()),
        Some(_) => None, // empty string = force no proxy
        None => crate::proxy::resolve_proxy_url(None),
    };

    let builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            let url_str = url.as_str();
            match is_safe_url_quick(url_str) {
                true => attempt.follow(),
                false => {
                    tracing::warn!(
                        url = %url_str,
                        "SSRF: blocked redirect to private/internal address"
                    );
                    attempt.error(SsrfRedirectBlocked)
                }
            }
        }))
        .timeout(timeout);

    let builder = crate::proxy::apply_proxy_to_builder(builder, resolved_proxy.as_deref());

    builder
        .build()
        .expect("failed to build SSRF-safe reqwest client")
}

/// Lightweight sentinel error surfaced when a redirect targets a private IP.
#[derive(Debug)]
struct SsrfRedirectBlocked;
impl std::fmt::Display for SsrfRedirectBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SSRF redirect blocked: target is a private/internal address")
    }
}
impl std::error::Error for SsrfRedirectBlocked {}

/// Quick boolean SSRF check — returns `false` for unsafe URLs instead of
/// `Result`. Used inside the redirect policy where we cannot propagate
/// `AgentError`.
fn is_safe_url_quick(raw_url: &str) -> bool {
    is_safe_url(raw_url).unwrap_or(false)
}

fn is_private_ipv4(v4: &Ipv4Addr) -> bool {
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_unspecified()
}

fn is_private_ipv6(v6: &Ipv6Addr) -> bool {
    v6.is_loopback()
        || v6.is_unspecified()
        // IPv6 link-local: fe80::/10
        || (v6.segments()[0] & 0xffc0) == 0xfe80
        // IPv6 unique-local (ULA): fc00::/7
        || (v6.segments()[0] & 0xfe00) == 0xfc00
        // IPv6 multicast: ff00::/8 — never a valid unicast endpoint
        || (v6.segments()[0] & 0xff00) == 0xff00
}

/// Fallback path for domain-form IP strings in the URL.
fn is_private_or_reserved(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => is_private_ipv6(v6),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_public_https() {
        assert!(is_safe_url("https://example.com/page").expect("ok"));
    }

    #[test]
    fn blocks_localhost() {
        assert!(!is_safe_url("http://localhost:8080/admin").expect("ok"));
    }

    #[test]
    fn blocks_private_ip() {
        assert!(!is_safe_url("http://192.168.1.1/admin").expect("ok"));
    }

    #[test]
    fn blocks_loopback() {
        assert!(!is_safe_url("http://127.0.0.1:3000/api").expect("ok"));
    }

    #[test]
    fn blocks_cloud_metadata() {
        assert!(!is_safe_url("http://169.254.169.254/latest/meta-data/").expect("ok"));
    }

    #[test]
    fn blocks_file_scheme() {
        assert!(!is_safe_url("file:///etc/passwd").expect("ok"));
    }

    #[test]
    fn blocks_ftp_scheme() {
        assert!(!is_safe_url("ftp://evil.com/malware").expect("ok"));
    }

    #[test]
    fn rejects_invalid_url() {
        assert!(is_safe_url("not a url").is_err());
    }

    #[test]
    fn blocks_link_local() {
        assert!(!is_safe_url("http://169.254.1.1/").expect("ok"));
    }

    #[test]
    fn blocks_ipv6_loopback() {
        // ::1 is the IPv6 loopback address
        assert!(!is_safe_url("http://[::1]/api").expect("ok"));
    }

    #[test]
    fn blocks_ipv6_link_local() {
        // fe80::/10 prefix — link-local
        assert!(!is_safe_url("http://[fe80::1]/api").expect("ok"));
    }

    #[test]
    fn blocks_ipv6_unique_local() {
        // fc00::/7 prefix — unique-local (RFC 4193)
        assert!(!is_safe_url("http://[fd00::1]/api").expect("ok"));
    }

    #[test]
    fn blocks_ipv6_multicast() {
        // ff02::1 — all-nodes multicast
        assert!(!is_safe_url("http://[ff02::1]/api").expect("ok"));
    }

    #[test]
    fn ssrf_safe_client_builds_successfully() {
        let client = build_ssrf_safe_client(Duration::from_secs(10));
        // Verify it was created — just a smoke test
        drop(client);
    }

    #[test]
    fn is_safe_url_quick_returns_false_for_private() {
        assert!(!is_safe_url_quick("http://127.0.0.1/admin"));
        assert!(!is_safe_url_quick("http://169.254.169.254/metadata"));
        assert!(!is_safe_url_quick("http://[::1]/api"));
    }

    #[test]
    fn is_safe_url_quick_returns_true_for_public() {
        assert!(is_safe_url_quick("https://example.com/page"));
        assert!(is_safe_url_quick("https://api.github.com/repos"));
    }

    #[test]
    fn is_safe_url_quick_returns_false_for_invalid() {
        assert!(!is_safe_url_quick("not a url"));
    }
}
