//! # Unified proxy resolution
//!
//! Resolves proxy configuration with a 6-level cascade:
//!
//! 1. Platform-specific env var (e.g. `DISCORD_PROXY`)
//! 2. `HTTPS_PROXY` / `https_proxy`
//! 3. `HTTP_PROXY` / `http_proxy`
//! 4. `ALL_PROXY` / `all_proxy`
//! 5. macOS system proxy via `scutil --proxy`
//! 6. None (direct connection)
//!
//! Supports HTTP, HTTPS, SOCKS5, and SOCKS5h (remote DNS) proxies.
//! SOCKS5h is required for GFW bypass where local DNS is poisoned.

use tracing::{debug, warn};

/// Resolve proxy URL with cascade priority.
///
/// Priority order (first non-empty wins):
/// 1. `platform_env_var` (e.g. `"DISCORD_PROXY"`)
/// 2. `HTTPS_PROXY` / `https_proxy`
/// 3. `HTTP_PROXY` / `http_proxy`
/// 4. `ALL_PROXY` / `all_proxy`
/// 5. macOS system proxy (`scutil --proxy`)
/// 6. `None`
pub fn resolve_proxy_url(platform_env_var: Option<&str>) -> Option<String> {
    // 1. Platform-specific var
    if let Some(var_name) = platform_env_var {
        if let Ok(url) = std::env::var(var_name) {
            if !url.is_empty() {
                debug!(var = var_name, url = %url, "Using platform-specific proxy");
                return Some(url);
            }
        }
    }

    // 2. HTTPS_PROXY (uppercase then lowercase)
    for var in &["HTTPS_PROXY", "https_proxy"] {
        if let Ok(url) = std::env::var(var) {
            if !url.is_empty() {
                debug!(var, url = %url, "Using HTTPS proxy");
                return Some(url);
            }
        }
    }

    // 3. HTTP_PROXY (uppercase then lowercase)
    for var in &["HTTP_PROXY", "http_proxy"] {
        if let Ok(url) = std::env::var(var) {
            if !url.is_empty() {
                debug!(var, url = %url, "Using HTTP proxy");
                return Some(url);
            }
        }
    }

    // 4. ALL_PROXY (uppercase then lowercase)
    for var in &["ALL_PROXY", "all_proxy"] {
        if let Ok(url) = std::env::var(var) {
            if !url.is_empty() {
                debug!(var, url = %url, "Using ALL_PROXY");
                return Some(url);
            }
        }
    }

    // 5. macOS system proxy detection
    #[cfg(target_os = "macos")]
    if let Some(url) = detect_macos_system_proxy() {
        debug!(url = %url, "Using macOS system proxy");
        return Some(url);
    }

    None
}

/// Configure a [`reqwest::ClientBuilder`] with proxy support.
///
/// If `proxy_url` is `Some`, adds the proxy. If `None`, does nothing.
/// Logs a warning on invalid proxy URLs and skips proxy configuration.
pub fn apply_proxy_to_builder(
    mut builder: reqwest::ClientBuilder,
    proxy_url: Option<&str>,
) -> reqwest::ClientBuilder {
    if let Some(url) = proxy_url {
        match reqwest::Proxy::all(url) {
            Ok(proxy) => {
                debug!(url, "Configuring proxy for reqwest client");
                builder = builder.proxy(proxy);
            }
            Err(e) => {
                warn!(url, error = %e, "Invalid proxy URL, proceeding without proxy");
            }
        }
    }
    builder
}

/// Detect macOS system proxy via `scutil --proxy`.
///
/// Parses the plist-like output for HTTPS proxy first, falls back to HTTP.
/// Returns `None` on non-macOS, if proxy is disabled, or on parse failure.
#[cfg(target_os = "macos")]
fn detect_macos_system_proxy() -> Option<String> {
    use std::process::Command;

    let output = Command::new("scutil").arg("--proxy").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);

    // Try HTTPS first, then HTTP
    for (enable_key, host_key, port_key) in &[
        ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
        ("HTTPEnable", "HTTPProxy", "HTTPPort"),
    ] {
        let enabled = parse_scutil_value(&text, enable_key);
        if enabled.as_deref() != Some("1") {
            continue;
        }

        let host = parse_scutil_value(&text, host_key);
        let port = parse_scutil_value(&text, port_key);

        if let Some(h) = host {
            let p = port.unwrap_or_else(|| "8080".to_string());
            return Some(format!("http://{h}:{p}"));
        }
    }

    None
}

/// Parse a single key from `scutil --proxy` output.
#[cfg(target_os = "macos")]
fn parse_scutil_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(key) {
            let rest = rest.trim();
            if let Some(value) = rest.strip_prefix(':') {
                let v = value.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex, MutexGuard};

    static ENV_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    /// RAII guard that serializes and restores proxy env vars for the duration of a test.
    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        vars: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn new(var_names: &[&str]) -> Self {
            let lock = ENV_MUTEX.lock().expect("env mutex poisoned");
            let vars = var_names
                .iter()
                .map(|&name| {
                    let prev = std::env::var(name).ok();
                    // SAFETY: test-only mutation protected by ENV_MUTEX.
                    unsafe { std::env::remove_var(name) };
                    (name.to_string(), prev)
                })
                .collect();
            Self { _lock: lock, vars }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, prev) in &self.vars {
                match prev {
                    Some(val) => unsafe { std::env::set_var(name, val) },
                    None => unsafe { std::env::remove_var(name) },
                }
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
        unsafe { std::env::set_var("HTTPS_PROXY", "http://https-proxy:8080") };
        assert_eq!(
            resolve_proxy_url(Some("TEST_PLATFORM_PROXY")),
            Some("socks5://platform:1080".to_string())
        );
    }

    #[test]
    fn resolve_https_proxy_fallback() {
        let _guard = EnvGuard::new(&[
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ]);
        unsafe { std::env::set_var("HTTPS_PROXY", "http://https-only:3128") };
        assert_eq!(
            resolve_proxy_url(None),
            Some("http://https-only:3128".to_string())
        );
    }

    #[test]
    fn resolve_http_proxy_fallback() {
        let _guard = EnvGuard::new(&[
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ]);
        unsafe { std::env::set_var("HTTP_PROXY", "http://http-only:3128") };
        assert_eq!(
            resolve_proxy_url(None),
            Some("http://http-only:3128".to_string())
        );
    }

    #[test]
    fn resolve_all_proxy_fallback() {
        let _guard = EnvGuard::new(&[
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ]);
        unsafe { std::env::set_var("ALL_PROXY", "socks5h://all:1080") };
        assert_eq!(
            resolve_proxy_url(None),
            Some("socks5h://all:1080".to_string())
        );
    }

    #[test]
    fn apply_proxy_noop_on_none() {
        let builder = reqwest::Client::builder();
        let builder = apply_proxy_to_builder(builder, None);
        // Should build successfully without proxy
        let _client = builder.build().expect("should build without proxy");
    }

    #[test]
    fn apply_proxy_invalid_url_logs_warning() {
        let builder = reqwest::Client::builder();
        let builder = apply_proxy_to_builder(builder, Some("not-a-url"));
        // Should still build — invalid proxy is skipped
        let _client = builder
            .build()
            .expect("should build with invalid proxy URL");
    }
}
