//! # Unified proxy support — cascade resolution + SOCKS5
//!
//! Delegates to [`edgecrab_security::proxy`] for the core proxy resolution
//! logic (6-level cascade: platform env → HTTPS_PROXY → HTTP_PROXY →
//! ALL_PROXY → macOS scutil → None).
//!
//! Re-exported here for backward compatibility and the gateway-specific
//! `build_proxy_client()` convenience function.

use std::time::Duration;

use tracing::warn;

/// Resolve proxy URL with cascade priority.
///
/// See [`edgecrab_security::proxy::resolve_proxy_url()`] for details.
pub fn resolve_proxy_url(platform_env_var: Option<&str>) -> Option<String> {
    edgecrab_security::proxy::resolve_proxy_url(platform_env_var)
}

/// Build a [`reqwest::Client`] with optional proxy configuration.
///
/// Handles HTTP, HTTPS, SOCKS5, and SOCKS5h proxy URLs.
/// SOCKS5h uses remote DNS resolution (required for GFW bypass).
///
/// If `proxy_url` is `None`, returns a plain client with no proxy.
/// If `proxy_url` is malformed, logs a warning and proceeds without proxy.
///
/// NOTE: This does NOT apply SSRF protection. For SSRF-safe clients, use
/// [`edgecrab_security::url_safety::build_ssrf_safe_client()`] instead.
pub fn build_proxy_client(proxy_url: Option<&str>, timeout: Duration) -> reqwest::Client {
    let builder = reqwest::Client::builder().timeout(timeout);
    let builder = edgecrab_security::proxy::apply_proxy_to_builder(builder, proxy_url);

    builder.build().unwrap_or_else(|e| {
        warn!(error = %e, "Failed to build proxy client, using default");
        reqwest::Client::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RAII guard that removes env vars on drop to avoid test pollution.
    struct EnvGuard {
        vars: Vec<String>,
    }

    impl EnvGuard {
        fn new(vars: &[&str]) -> Self {
            let vars: Vec<String> = vars.iter().map(|s| s.to_string()).collect();
            for v in &vars {
                unsafe { std::env::remove_var(v) };
            }
            Self { vars }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for v in &self.vars {
                unsafe { std::env::remove_var(v) };
            }
        }
    }

    #[test]
    fn resolve_returns_none_without_env_vars() {
        let _guard = EnvGuard::new(&[
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ]);
        assert!(resolve_proxy_url(None).is_none());
    }

    #[test]
    fn resolve_platform_var_takes_priority() {
        let _guard = EnvGuard::new(&[
            "TEST_PLATFORM_PROXY",
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ]);
        unsafe { std::env::set_var("TEST_PLATFORM_PROXY", "socks5://platform:1080") };
        unsafe { std::env::set_var("HTTPS_PROXY", "http://generic:8080") };
        assert_eq!(
            resolve_proxy_url(Some("TEST_PLATFORM_PROXY")),
            Some("socks5://platform:1080".to_string())
        );
    }

    #[test]
    fn resolve_https_proxy_when_no_platform_var() {
        let _guard = EnvGuard::new(&[
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ]);
        unsafe { std::env::set_var("HTTPS_PROXY", "http://proxy:3128") };
        assert_eq!(
            resolve_proxy_url(None),
            Some("http://proxy:3128".to_string())
        );
    }

    #[test]
    fn resolve_all_proxy_as_fallback() {
        let _guard = EnvGuard::new(&[
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ]);
        unsafe { std::env::set_var("ALL_PROXY", "socks5h://tunnel:1080") };
        assert_eq!(
            resolve_proxy_url(None),
            Some("socks5h://tunnel:1080".to_string())
        );
    }

    #[test]
    fn build_http_proxy_client() {
        let client = build_proxy_client(Some("http://proxy:8080"), Duration::from_secs(10));
        drop(client);
    }

    #[test]
    fn build_socks5_proxy_client() {
        let client = build_proxy_client(Some("socks5://socks:1080"), Duration::from_secs(10));
        drop(client);
    }

    #[test]
    fn build_socks5h_rdns_proxy_client() {
        let client = build_proxy_client(Some("socks5h://tunnel:1080"), Duration::from_secs(10));
        drop(client);
    }

    #[test]
    fn build_no_proxy_client() {
        let client = build_proxy_client(None, Duration::from_secs(10));
        drop(client);
    }

    #[test]
    fn build_malformed_proxy_graceful() {
        let client = build_proxy_client(Some("not-a-url"), Duration::from_secs(10));
        drop(client);
    }
}
