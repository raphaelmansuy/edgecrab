# ADR-0606: Unified Proxy Support

| Field       | Value                                                   |
|-------------|---------------------------------------------------------|
| Status      | Proposed                                                |
| Date        | 2026-04-14                                              |
| Implements  | hermes-agent PR #6814                                   |
| Crate       | `edgecrab-gateway`, `edgecrab-tools`                    |
| File        | `crates/edgecrab-gateway/src/proxy.rs` (NEW)            |

---

## 1. Context

EdgeCrab's gateway platform adapters use `reqwest` with no proxy configuration.
Users behind corporate firewalls, in China (GFW), or using SOCKS tunnels
cannot reach external APIs or platform endpoints. hermes-agent v0.9.0 ships
unified proxy support: SOCKS, per-platform env vars, and macOS system proxy
auto-detection.

---

## 2. First Principles

| Principle       | Application                                              |
|-----------------|----------------------------------------------------------|
| **SRP**         | Proxy resolution in one module; adapters consume it      |
| **OCP**         | Adapters get proxy-aware clients without code changes    |
| **DRY**         | Single `resolve_proxy_url()` used by all adapters        |
| **Code is Law** | hermes-agent `gateway/platforms/base.py:L59-170` as ref  |

---

## 3. Architecture

```
+-------------------------------------------------------------------+
|                     Proxy Resolution Layer                         |
|                                                                    |
|  resolve_proxy_url(platform_env_var)                               |
|    |                                                               |
|    +-- 1. Platform-specific var (e.g. DISCORD_PROXY)               |
|    +-- 2. HTTPS_PROXY / HTTP_PROXY / ALL_PROXY (+ lowercase)      |
|    +-- 3. macOS: scutil --proxy auto-detect                        |
|    +-- 4. None (no proxy)                                          |
|                                                                    |
|  build_proxy_client(proxy_url) -> reqwest::Client                  |
|    |                                                               |
|    +-- HTTP/HTTPS proxy  -> reqwest::Proxy::all(url)               |
|    +-- SOCKS5 proxy      -> reqwest::Proxy::all(url) (reqwest      |
|    |                         native socks5 support)                |
|    +-- None              -> reqwest::Client::new()                 |
+-------------------------------------------------------------------+
         |                |                |
         v                v                v
  +-----------+    +------------+   +-------------+
  | Telegram  |    | Discord    |   | Web Tools   |
  | Adapter   |    | Adapter    |   | (reqwest)   |
  +-----------+    +------------+   +-------------+
```

---

## 4. Data Model

### 4.1 Proxy Resolution

```rust
// crates/edgecrab-gateway/src/proxy.rs

/// Resolve proxy URL with cascade priority.
///
/// Priority order (first non-empty wins):
/// 1. `platform_env_var` (e.g. "DISCORD_PROXY")
/// 2. HTTPS_PROXY / https_proxy
/// 3. HTTP_PROXY / http_proxy
/// 4. ALL_PROXY / all_proxy
/// 5. macOS system proxy (scutil --proxy)
/// 6. None
pub fn resolve_proxy_url(platform_env_var: Option<&str>) -> Option<String> { ... }

/// Build a reqwest::Client with proxy configuration.
///
/// Handles HTTP, HTTPS, and SOCKS5 proxy URLs.
/// SOCKS5 uses remote DNS resolution (rdns) for GFW bypass.
pub fn build_proxy_client(proxy_url: Option<&str>, timeout: Duration) -> reqwest::Client { ... }

/// Detect macOS system proxy via `scutil --proxy`.
///
/// - Prefers HTTPS proxy key, falls back to HTTP
/// - 3s subprocess timeout
/// - Returns None on non-macOS or on failure
#[cfg(target_os = "macos")]
fn detect_macos_system_proxy() -> Option<String> { ... }
```

### 4.2 Env Vars (cascade)

| Priority | Variable              | Scope                          |
|----------|-----------------------|--------------------------------|
| 0        | `<PLATFORM>_PROXY`    | Platform-specific override     |
| 1        | `HTTPS_PROXY`         | Standard (uppercase)           |
| 1        | `https_proxy`         | Standard (lowercase)           |
| 2        | `HTTP_PROXY`          | Standard (uppercase)           |
| 2        | `http_proxy`          | Standard (lowercase)           |
| 3        | `ALL_PROXY`           | Standard (uppercase)           |
| 3        | `all_proxy`           | Standard (lowercase)           |
| 4        | *(macOS auto-detect)* | `scutil --proxy` subprocess    |

### 4.3 Platform-Specific Env Vars

| Platform    | Variable              |
|-------------|-----------------------|
| Discord     | `DISCORD_PROXY`       |
| Telegram    | `TELEGRAM_PROXY`      |
| Slack       | `SLACK_PROXY`         |
| Generic     | `GATEWAY_PROXY`       |

---

## 5. SOCKS5 Considerations

```
SOCKS5 proxy URL format: socks5://host:port
                      or socks5h://host:port  (remote DNS)

reqwest handles SOCKS natively via the "socks" feature flag.
Cargo.toml: reqwest = { features = ["socks"] }

Remote DNS (rdns=true in hermes-agent):
  socks5h:// => DNS resolved through the SOCKS proxy
  Required for GFW bypass (Shadowrocket, Clash, etc.)
  where local DNS is poisoned.

EdgeCrab mapping:
  socks5://  -> reqwest::Proxy::all("socks5://...")
  socks5h:// -> reqwest::Proxy::all("socks5h://...")  (reqwest native)
```

---

## 6. macOS System Proxy Detection

```rust
#[cfg(target_os = "macos")]
fn detect_macos_system_proxy() -> Option<String> {
    // Run: scutil --proxy
    // Parse output for:
    //   HTTPSEnable : 1
    //   HTTPSProxy : host
    //   HTTPSPort : port
    // Fallback to HTTP keys if HTTPS not set
    // Return http://host:port or None
    // 3 second timeout on subprocess
}
```

---

## 7. Integration Points

### 7.1 Gateway Adapters

Each adapter's `from_env()` or constructor should call:

```rust
let proxy_url = resolve_proxy_url(Some("DISCORD_PROXY"));
let client = build_proxy_client(proxy_url.as_deref(), Duration::from_secs(30));
```

### 7.2 Web Tools (edgecrab-tools)

`web.rs` web_search and web_extract tools should respect proxy:

```rust
let proxy_url = resolve_proxy_url(None);  // no platform-specific var
let client = build_proxy_client(proxy_url.as_deref(), Duration::from_secs(30));
```

### 7.3 MCP Client

HTTP MCP connections should respect proxy for corporate firewall scenarios:

```rust
let proxy_url = resolve_proxy_url(None);
// Apply to MCP HTTP client
```

---

## 8. Edge Cases & Roadblocks

| #  | Edge Case                              | Remediation                                      | Source                          |
|----|----------------------------------------|--------------------------------------------------|---------------------------------|
| 1  | SOCKS proxy requires auth              | `socks5://user:pass@host:port` (reqwest native)  | Standard SOCKS5 auth            |
| 2  | macOS proxy disabled                   | `HTTPSEnable: 0` → return None                   | `_detect_macos_system_proxy()`  |
| 3  | Proxy URL malformed                    | Log warning, proceed without proxy               | Graceful degradation            |
| 4  | Proxy down → timeouts                  | Adapter-level timeout (30s) catches it            | Existing timeout handling       |
| 5  | Local DNS poisoned (GFW)               | `socks5h://` forces remote DNS                   | `rdns=True` in hermes-agent     |
| 6  | `NO_PROXY` env var                     | Respect NO_PROXY/no_proxy (reqwest handles this)  | Standard behavior               |
| 7  | WebSocket through SOCKS                | tokio-tungstenite + socks: use `connect_async`    | WeCom adapter needs this         |
| 8  | subprocess hang on `scutil`            | 3s timeout on macOS detection                    | `_detect_macos_system_proxy()`  |
| 9  | Non-macOS system proxy                 | Not supported — explicitly Linux env-var only     | hermes-agent decision           |
| 10 | Proxy for local BlueBubbles server     | Skip proxy for localhost/127.0.0.1 (NO_PROXY)    | Standard NO_PROXY behavior      |

---

## 9. Implementation Plan

### 9.1 Files to Create

| File                                      | Purpose                            |
|-------------------------------------------|-------------------------------------|
| `crates/edgecrab-gateway/src/proxy.rs`    | Proxy resolution + client builder  |

### 9.2 Files to Modify

| File                                        | Change                                       |
|---------------------------------------------|----------------------------------------------|
| `crates/edgecrab-gateway/src/lib.rs`        | Add `pub mod proxy;`                         |
| `crates/edgecrab-gateway/src/telegram.rs`   | Use `build_proxy_client()` for reqwest       |
| `crates/edgecrab-gateway/src/discord.rs`    | Use `build_proxy_client()` for reqwest       |
| `crates/edgecrab-gateway/src/slack.rs`      | Use `build_proxy_client()` for reqwest       |
| `crates/edgecrab-gateway/src/wecom.rs`      | SOCKS-aware WebSocket connection             |
| `crates/edgecrab-tools/src/tools/web.rs`    | Use `build_proxy_client()` for web tools     |
| `Cargo.toml` (edgecrab-gateway)            | Add `reqwest/socks` feature                  |

### 9.3 Dependencies

```toml
[dependencies]
reqwest = { features = ["socks"] }   # enables native SOCKS5 support
# No additional crates needed
```

### 9.4 Test Matrix

| Test                              | Validates                                    |
|-----------------------------------|----------------------------------------------|
| `test_resolve_platform_var`       | Platform-specific var takes priority         |
| `test_resolve_https_proxy`        | HTTPS_PROXY used when no platform var        |
| `test_resolve_all_proxy`          | ALL_PROXY as last resort                     |
| `test_resolve_case_insensitive`   | Lowercase env vars work                      |
| `test_resolve_none`               | Returns None when no proxy configured        |
| `test_build_http_proxy`           | reqwest client with HTTP proxy               |
| `test_build_socks5_proxy`         | reqwest client with SOCKS5 proxy             |
| `test_build_socks5h_rdns`         | Remote DNS variant preserved                 |
| `test_macos_detect_enabled`       | Parses scutil output correctly               |
| `test_macos_detect_disabled`      | Returns None when proxy disabled             |

---

## 10. Acceptance Criteria

- [ ] `resolve_proxy_url()` with 6-level cascade priority
- [ ] `build_proxy_client()` handles HTTP, HTTPS, SOCKS5, SOCKS5h
- [ ] macOS system proxy auto-detection via `scutil --proxy`
- [ ] All gateway adapters use proxy-aware reqwest clients
- [ ] Web tools respect proxy configuration
- [ ] SOCKS5 with remote DNS (`socks5h://`) supported for GFW bypass
- [ ] `NO_PROXY` / `no_proxy` respected (reqwest native)
- [ ] WebSocket connections (WeCom) proxy-aware
- [ ] All tests pass: `cargo test -p edgecrab-gateway -- proxy`
