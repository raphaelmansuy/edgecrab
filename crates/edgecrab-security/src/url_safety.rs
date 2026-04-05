//! URL safety check — blocks SSRF and dangerous schemes.
//!
//! Validates URLs before web tools fetch them, preventing:
//! - Private/loopback IP access (SSRF)
//! - Non-HTTP schemes (file://, ftp://)
//! - Cloud metadata endpoints (169.254.169.254)

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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
            if let Ok(ip) = name.parse::<IpAddr>() {
                if is_private_or_reserved(&ip) {
                    tracing::warn!(%ip, "Blocked private/reserved IP (domain form)");
                    return Ok(false);
                }
            }
        }
    }

    Ok(true)
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
}
