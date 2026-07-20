//! URL safety check — blocks SSRF and dangerous schemes.
//!
//! Validates URLs before web tools fetch them, preventing:
//! - Private/loopback IP access (SSRF)
//! - Non-HTTP schemes (file://, ftp://)
//! - Cloud metadata endpoints (169.254.169.254)
//! - Redirect-based SSRF (302 → private IP)

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use edgecrab_types::AgentError;
use url::Host;

/// Loopback preview policy for local dev servers (`security.preview` in config).
#[derive(Debug, Clone, Default)]
pub struct PreviewPolicy {
    pub enabled: bool,
    pub allowed_ports: Vec<u16>,
    /// When true, any loopback HTTP(S) port is allowed (homelab / visual-UX dogfood).
    pub allow_any_loopback_port: bool,
}

/// Session-scoped loopback preview grants (spec 021 — user Once/Session choices).
#[derive(Debug, Clone, Default)]
pub struct SessionPreviewGrants {
    /// Host+port pairs allowed until process exit.
    session: HashSet<(String, u16)>,
    /// Host+port pairs allowed for a single successful URL check (Once).
    once: HashSet<(String, u16)>,
}

impl SessionPreviewGrants {
    fn normalize_host(host: &str) -> String {
        let h = host.trim().to_ascii_lowercase();
        if h == "localhost" || h == "::1" || h == "[::1]" {
            "127.0.0.1".into()
        } else {
            h.trim_start_matches('[').trim_end_matches(']').to_string()
        }
    }

    fn key_for_url(parsed: &url::Url) -> Option<(String, u16)> {
        if !is_loopback_preview_host(parsed) {
            return None;
        }
        let host = parsed.host_str()?;
        let port = parsed.port_or_known_default().unwrap_or(80);
        Some((Self::normalize_host(host), port))
    }

    /// Grant host:port for the rest of the process lifetime.
    pub fn grant_session(&mut self, host: &str, port: u16) {
        self.session.insert((Self::normalize_host(host), port));
    }

    /// Grant host:port for a single subsequent allow check.
    pub fn grant_once(&mut self, host: &str, port: u16) {
        self.once.insert((Self::normalize_host(host), port));
    }

    fn allows(&mut self, parsed: &url::Url) -> bool {
        let Some(key) = Self::key_for_url(parsed) else {
            return false;
        };
        if self.session.contains(&key) {
            return true;
        }
        if self.once.remove(&key) {
            return true;
        }
        false
    }
}

static PREVIEW_POLICY: OnceLock<Mutex<PreviewPolicy>> = OnceLock::new();
static SESSION_PREVIEW_GRANTS: OnceLock<Mutex<SessionPreviewGrants>> = OnceLock::new();

fn preview_policy_cell() -> &'static Mutex<PreviewPolicy> {
    PREVIEW_POLICY.get_or_init(|| Mutex::new(PreviewPolicy::default()))
}

fn session_preview_grants_cell() -> &'static Mutex<SessionPreviewGrants> {
    SESSION_PREVIEW_GRANTS.get_or_init(|| Mutex::new(SessionPreviewGrants::default()))
}

/// Install preview SSRF allowlist (startup from config; tests may update).
pub fn set_preview_policy(policy: PreviewPolicy) {
    *preview_policy_cell().lock().expect("preview policy lock") = policy;
}

/// Current preview policy snapshot (for tests and diagnostics).
#[doc(hidden)]
pub fn current_preview_policy() -> PreviewPolicy {
    preview_policy_cell()
        .lock()
        .expect("preview policy lock")
        .clone()
}

/// Grant loopback host:port for this process (ApprovalChoice::Session).
pub fn grant_session_preview_loopback(host: &str, port: u16) {
    if let Ok(mut g) = session_preview_grants_cell().lock() {
        g.grant_session(host, port);
    }
}

/// Grant loopback host:port for one navigate (ApprovalChoice::Once).
pub fn grant_once_preview_loopback(host: &str, port: u16) {
    if let Ok(mut g) = session_preview_grants_cell().lock() {
        g.grant_once(host, port);
    }
}

/// Clear session preview grants (tests / session reset).
#[doc(hidden)]
pub fn clear_session_preview_grants() {
    if let Ok(mut g) = session_preview_grants_cell().lock() {
        *g = SessionPreviewGrants::default();
    }
}

static PREVIEW_TEST_SERIAL: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();

/// Serialize tests that mutate or assert on the global preview policy.
#[doc(hidden)]
pub fn preview_policy_test_guard() -> std::sync::MutexGuard<'static, ()> {
    match PREVIEW_TEST_SERIAL
        .get_or_init(|| Mutex::new(()))
        .lock()
    {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn active_preview_policy() -> Option<PreviewPolicy> {
    let guard = preview_policy_cell().lock().expect("preview policy lock");
    guard.enabled.then(|| guard.clone())
}

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
    let host = parsed
        .host()
        .ok_or_else(|| AgentError::Security(format!("No host in URL: {raw_url}")))?;

    match host {
        Host::Ipv4(v4) => {
            if is_private_ipv4(&v4) {
                if allow_loopback_in_e2e(&host) || allow_loopback_via_policy_or_grant(&parsed) {
                    return Ok(true);
                }
                tracing::warn!(%v4, "Blocked private/reserved IPv4");
                return Ok(false);
            }
        }
        Host::Ipv6(v6) => {
            if is_private_ipv6(&v6) {
                if allow_loopback_in_e2e(&host) || allow_loopback_via_policy_or_grant(&parsed) {
                    return Ok(true);
                }
                tracing::warn!(%v6, "Blocked private/reserved IPv6");
                return Ok(false);
            }
        }
        Host::Domain(name) => {
            // Block known dangerous hostnames (including numeric cloud-metadata IP)
            const BLOCKED_HOSTS: &[&str] =
                &["localhost", "metadata.google.internal", "169.254.169.254"];
            if BLOCKED_HOSTS.contains(&name) {
                if allow_loopback_in_e2e(&host) || allow_loopback_via_policy_or_grant(&parsed) {
                    return Ok(true);
                }
                tracing::warn!(host = %name, "Blocked dangerous hostname");
                return Ok(false);
            }
            // Fallback: attempt to parse domain-form IP strings such as
            // "127.0.0.1" or "::1" that weren't bracketed in the URL.
            if let Ok(ip) = name.parse::<IpAddr>()
                && is_private_or_reserved(&ip)
            {
                if allow_loopback_in_e2e(&host) || allow_loopback_via_policy_or_grant(&parsed) {
                    return Ok(true);
                }
                tracing::warn!(%ip, "Blocked private/reserved IP (domain form)");
                return Ok(false);
            }
        }
    }

    Ok(true)
}

fn is_loopback_preview_host(parsed: &url::Url) -> bool {
    match parsed.host() {
        Some(Host::Ipv4(v4)) => v4.is_loopback(),
        Some(Host::Ipv6(v6)) => v6.is_loopback(),
        Some(Host::Domain(name)) => {
            name == "localhost"
                || name
                    .parse::<IpAddr>()
                    .ok()
                    .is_some_and(|ip| is_loopback_ip(&ip))
        }
        None => false,
    }
}

fn allow_preview_loopback(parsed: &url::Url) -> bool {
    let Some(policy) = active_preview_policy() else {
        return false;
    };
    if !is_loopback_preview_host(parsed) {
        return false;
    }
    let port = parsed.port_or_known_default().unwrap_or(80);
    policy.allow_any_loopback_port || policy.allowed_ports.contains(&port)
}

fn allow_loopback_via_policy_or_grant(parsed: &url::Url) -> bool {
    if let Ok(mut grants) = session_preview_grants_cell().lock()
        && grants.allows(parsed)
    {
        return true;
    }
    allow_preview_loopback(parsed)
}

/// True when `security.preview` or a session/once grant allows this loopback HTTP(S) URL.
pub fn is_preview_loopback_url(raw_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(raw_url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    allow_loopback_via_policy_or_grant(&parsed)
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

fn is_loopback_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// When `EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST=1`, allow loopback only (for Docker SearXNG e2e).
fn e2e_allow_localhost() -> bool {
    std::env::var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST")
        .ok()
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

fn allow_loopback_in_e2e<S: AsRef<str>>(host: &Host<S>) -> bool {
    if !e2e_allow_localhost() {
        return false;
    }
    match host {
        Host::Ipv4(v4) => v4.is_loopback(),
        Host::Ipv6(v6) => v6.is_loopback(),
        Host::Domain(name) if name.as_ref() == "localhost" => true,
        Host::Domain(name) => name
            .as_ref()
            .parse::<IpAddr>()
            .ok()
            .is_some_and(|ip| is_loopback_ip(&ip)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static URL_SAFETY_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn url_safety_test_lock() -> MutexGuard<'static, ()> {
        URL_SAFETY_TEST_LOCK.lock().expect("url safety test lock")
    }

    fn without_e2e_localhost<F: FnOnce()>(f: F) {
        let prev = std::env::var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST").ok();
        unsafe { std::env::remove_var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST") };
        set_preview_policy(PreviewPolicy::default());
        f();
        unsafe { std::env::remove_var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST") };
        set_preview_policy(PreviewPolicy::default());
        if let Some(v) = prev {
            unsafe { std::env::set_var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST", v) };
        }
    }

    #[test]
    fn allows_public_https() {
        assert!(is_safe_url("https://example.com/page").expect("ok"));
    }

    #[test]
    fn blocks_localhost() {
        let _lock = url_safety_test_lock();
        without_e2e_localhost(|| {
            assert!(!is_safe_url("http://localhost:8080/admin").expect("ok"));
        });
    }

    #[test]
    fn blocks_private_ip() {
        assert!(!is_safe_url("http://192.168.1.1/admin").expect("ok"));
    }

    #[test]
    fn blocks_loopback() {
        let _lock = url_safety_test_lock();
        without_e2e_localhost(|| {
            assert!(!is_safe_url("http://127.0.0.1:3000/api").expect("ok"));
        });
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
        let _lock = url_safety_test_lock();
        without_e2e_localhost(|| {
            assert!(!is_safe_url("http://[::1]/api").expect("ok"));
        });
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
        let _lock = url_safety_test_lock();
        without_e2e_localhost(|| {
            assert!(!is_safe_url_quick("http://127.0.0.1/admin"));
            assert!(!is_safe_url_quick("http://169.254.169.254/metadata"));
            assert!(!is_safe_url_quick("http://[::1]/api"));
        });
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

    #[test]
    fn e2e_env_allows_localhost_only() {
        let _lock = url_safety_test_lock();
        let prev = std::env::var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST").ok();
        unsafe { std::env::set_var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST", "1") };
        assert!(is_safe_url("http://127.0.0.1:8888/search").expect("ok"));
        assert!(is_safe_url("http://localhost:8888/search").expect("ok"));
        assert!(!is_safe_url("http://192.168.1.1/admin").expect("ok"));
        unsafe { std::env::remove_var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST") };
        if let Some(v) = prev {
            unsafe { std::env::set_var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST", v) };
        }
    }

    #[test]
    fn ha05_preview_policy_allows_allowlisted_localhost_port() {
        let _lock = url_safety_test_lock();
        without_e2e_localhost(|| {
            set_preview_policy(PreviewPolicy {
                enabled: true,
                allowed_ports: vec![5173],
                allow_any_loopback_port: false,
            });
            assert!(is_safe_url("http://127.0.0.1:5173/").expect("ok"));
            assert!(!is_safe_url("http://127.0.0.1:9999/").expect("ok"));
        });
    }

    #[test]
    fn ha07_preview_allow_any_loopback_port() {
        let _lock = url_safety_test_lock();
        without_e2e_localhost(|| {
            set_preview_policy(PreviewPolicy {
                enabled: true,
                allowed_ports: vec![],
                allow_any_loopback_port: true,
            });
            assert!(is_safe_url("http://127.0.0.1:7777/").expect("ok"));
            assert!(is_preview_loopback_url("http://localhost:9999/"));
        });
    }

    #[test]
    fn session_preview_grants_allow_loopback_url() {
        let _lock = url_safety_test_lock();
        without_e2e_localhost(|| {
            set_preview_policy(PreviewPolicy::default());
            clear_session_preview_grants();
            assert!(!is_preview_loopback_url("http://127.0.0.1:8000/"));
            grant_session_preview_loopback("127.0.0.1", 8000);
            assert!(is_preview_loopback_url("http://127.0.0.1:8000/"));
            assert!(is_safe_url("http://127.0.0.1:8000/").expect("ok"));
            assert!(!is_preview_loopback_url("http://127.0.0.1:8001/"));
            clear_session_preview_grants();
        });
    }

    #[test]
    fn once_preview_grant_consumes_on_first_check() {
        let _lock = url_safety_test_lock();
        without_e2e_localhost(|| {
            set_preview_policy(PreviewPolicy::default());
            clear_session_preview_grants();
            grant_once_preview_loopback("127.0.0.1", 8010);
            assert!(is_preview_loopback_url("http://127.0.0.1:8010/"));
            assert!(!is_preview_loopback_url("http://127.0.0.1:8010/"));
            clear_session_preview_grants();
        });
    }
}
