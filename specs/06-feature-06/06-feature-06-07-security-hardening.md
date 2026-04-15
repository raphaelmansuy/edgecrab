# ADR-0607: Comprehensive Security Hardening

| Field       | Value                                                         |
|-------------|---------------------------------------------------------------|
| Status      | Proposed                                                      |
| Date        | 2026-04-14                                                    |
| Implements  | hermes-agent PR #7933, #7944, #7940, #7151, #7156, #7455     |
| Crates      | `edgecrab-security`, `edgecrab-gateway`, `edgecrab-tools`     |

---

## 1. Context

hermes-agent v0.9.0 delivered a deep security hardening pass: Twilio webhook
signatures, shell injection in sandbox writes, SSRF redirect guards, git arg
injection in checkpoints, API server auth enforcement, and approval button
authorization. EdgeCrab already has strong security foundations
(`edgecrab-security`) but has **three critical gaps** identified by
cross-referencing both codebases.

---

## 2. Gap Analysis: edgecrab-security vs hermes-agent

```
+-----------------------------+-------------------+-------------------+
| Security Feature            | edgecrab-security | hermes-agent      |
+-----------------------------+-------------------+-------------------+
| Path traversal jail         | path_jail.rs  [Y] | approval.py   [Y] |
| SSRF - initial URL check    | url_safety.rs [Y] | url_safety.py [Y] |
| SSRF - redirect following   | ............. [N] | slack.py      [Y] | <-- GAP 1
| Command scan (dangerous)    | command_scan  [Y] | approval.py   [Y] |
| Unicode normalization       | normalize.rs  [Y] | approval.py   [Y] |
| Prompt injection scan       | injection.rs  [Y] | prompt_builder[Y] |
| Secret redaction            | redact.rs     [Y] | redact.py     [Y] |
| Approval policy engine      | approval.rs   [Y] | approval.py   [Y] |
| Skills guard (23 patterns)  | skills_guard  [Y] | skills_guard  [Y] |
| Webhook signature validation| ............. [N] | sms.py        [P] | <-- GAP 2*
| API server auth (timing-safe)| ............. [N] | api_server.py [Y] | <-- GAP 3
| CRLF header injection guard | ............. [N] | api_server.py [Y] | <-- GAP 4
| Gateway user allowlists     | platform.rs   [P] | discord.py    [Y] | Partial
| Git arg injection (list API)| checkpoint.rs [Y] | checkpoint.py [Y] | No gap
| Shell heredoc neutralization| N/A (Rust uses Command) | N/A         | No gap
+-----------------------------+-------------------+-------------------+

[Y] = Implemented    [N] = Not implemented    [P] = Partial
* hermes-agent's Twilio signature validation was also missing (the fix IS PR #7933)
```

---

## 3. First Principles

| Principle       | Application                                              |
|-----------------|----------------------------------------------------------|
| **Defense in Depth** | Multiple layers; one failure doesn't cascade         |
| **Fail Closed** | Reject on validation failure, never silently pass        |
| **SRP**         | Each security check is a standalone, testable function   |
| **DRY**         | Security primitives in `edgecrab-security`, consumed     |
| **Code is Law** | Each fix maps to a specific hermes-agent PR              |

---

## 4. Fix Specifications

### 4.1 SSRF Redirect Guard (GAP 1)

**Problem**: `url_safety::is_safe_url()` validates the initial URL but
`reqwest` follows 302 redirects silently. A public URL can redirect to
`169.254.169.254` (cloud metadata), `127.0.0.1`, or internal services.

**hermes-agent pattern** (`slack.py:L653-671`):

```python
async def _ssrf_redirect_guard(response):
    if response.is_redirect and response.next_request:
        redirect_url = str(response.next_request.url)
        if not is_safe_url(redirect_url):
            raise ValueError("Blocked redirect to private/internal address")

async with httpx.AsyncClient(
    follow_redirects=True,
    event_hooks={"response": [_ssrf_redirect_guard]},
) as client:
```

**EdgeCrab implementation**:

```rust
// crates/edgecrab-security/src/url_safety.rs (MODIFY)

/// Build a reqwest::Client that validates every redirect target against
/// SSRF rules. This prevents DNS rebinding and open-redirect attacks.
pub fn build_ssrf_safe_client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            match is_safe_url(url.as_str()) {
                true => attempt.follow(),
                false => {
                    tracing::warn!(
                        url = %url,
                        "SSRF: blocked redirect to private/internal address"
                    );
                    attempt.error(anyhow::anyhow!("SSRF redirect blocked"))
                }
            }
        }))
        .timeout(timeout)
        .build()
        .expect("failed to build SSRF-safe client")
}
```

**Files to modify**:
- `crates/edgecrab-security/src/url_safety.rs` — add `build_ssrf_safe_client()`
- `crates/edgecrab-tools/src/tools/web.rs` — use `build_ssrf_safe_client()`
- `crates/edgecrab-gateway/src/slack.rs` — use for image downloads

### 4.2 Webhook Signature Validation (GAP 2 — SMS/Twilio)

**Problem**: When EdgeCrab's SMS adapter ships, it must validate Twilio
`X-Twilio-Signature` headers to prevent inbound message spoofing.

**Algorithm** (Twilio spec):

```
signature = HMAC-SHA1(auth_token, url + sorted(POST params as key=value))
expected  = base64(signature)
compare   = X-Twilio-Signature header value
```

```rust
// crates/edgecrab-gateway/src/sms.rs (when SMS adapter is created)

fn validate_twilio_signature(
    auth_token: &str,
    url: &str,
    params: &BTreeMap<String, String>,
    signature_header: &str,
) -> bool {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    let mut data = url.to_string();
    for (key, value) in params {
        data.push_str(key);
        data.push_str(value);
    }

    let mut mac = Hmac::<Sha1>::new_from_slice(auth_token.as_bytes())
        .expect("HMAC key length");
    mac.update(data.as_bytes());
    let expected = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    // Constant-time comparison
    subtle::ConstantTimeEq::ct_eq(expected.as_bytes(), signature_header.as_bytes()).into()
}
```

### 4.3 API Server Auth Enforcement (GAP 3)

**Problem**: When EdgeCrab's gateway API server binds to non-localhost,
requests must be authenticated. Without auth, anyone on the network can
inject messages.

**hermes-agent pattern** (`api_server.py:L406-427`):

```
Defense layers:
  1. hmac.compare_digest() for timing-safe token comparison
  2. Refuse non-localhost bind without API_SERVER_KEY
  3. Session-ID header required for continuation
  4. Auth check on every endpoint
```

```rust
// crates/edgecrab-gateway/src/api_server.rs (MODIFY when API server ships)

fn check_auth(
    api_key: &str,
    auth_header: Option<&str>,
) -> Result<(), axum::http::StatusCode> {
    let token = auth_header
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|t| t.trim())
        .ok_or(axum::http::StatusCode::UNAUTHORIZED)?;

    // Timing-safe comparison
    if subtle::ConstantTimeEq::ct_eq(token.as_bytes(), api_key.as_bytes()).into() {
        Ok(())
    } else {
        Err(axum::http::StatusCode::UNAUTHORIZED)
    }
}

fn validate_bind_address(host: &str, api_key: &Option<String>) -> anyhow::Result<()> {
    let is_localhost = host == "127.0.0.1" || host == "::1" || host == "localhost";
    if !is_localhost && api_key.is_none() {
        anyhow::bail!(
            "API_SERVER_KEY required when binding to non-localhost address '{}'",
            host
        );
    }
    Ok(())
}
```

### 4.4 CRLF Header Injection Guard (GAP 4)

**Problem**: Session IDs and user-provided values used in HTTP response
headers can contain `\r\n` to inject additional headers.

```rust
// crates/edgecrab-security/src/lib.rs (ADD)

/// Check for CRLF injection in header values.
/// Returns Err if the value contains carriage return, newline, or null bytes.
pub fn validate_header_value(value: &str) -> Result<(), &'static str> {
    if value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
        Err("header value contains CRLF or null byte")
    } else {
        Ok(())
    }
}
```

### 4.5 Gateway User Authorization Enhancement

**Current state**: Platform adapters have `allowed_users` hash sets.
**Enhancement**: Unify authorization into a shared middleware layer.

```rust
// crates/edgecrab-gateway/src/auth.rs (NEW)

pub struct GatewayAuthPolicy {
    allowed_users: HashMap<Platform, HashSet<String>>,
    allow_all: HashMap<Platform, bool>,
}

impl GatewayAuthPolicy {
    /// Check if a user is authorized for a specific platform action.
    /// Checks button clicks, voice input, and message sends.
    pub fn is_authorized(&self, platform: Platform, user_id: &str) -> bool {
        if *self.allow_all.get(&platform).unwrap_or(&false) {
            return true;
        }
        self.allowed_users
            .get(&platform)
            .map(|set| set.contains(user_id))
            .unwrap_or(true) // default: allow if no allowlist configured
    }
}
```

---

## 5. Edge Cases & Roadblocks

| #  | Edge Case                              | Remediation                                         | Source                   |
|----|----------------------------------------|------------------------------------------------------|--------------------------|
| 1  | DNS rebinding attack on SSRF           | Redirect guard re-validates on every 302             | `slack.py:L653`          |
| 2  | IPv6 mapped IPv4 in redirects          | `is_safe_url()` already handles `::ffff:127.0.0.1`  | `url_safety.rs`         |
| 3  | Twilio signature clock skew            | No timestamp validation (Twilio doesn't require it)  | Twilio spec              |
| 4  | API key timing attack                  | `subtle::ConstantTimeEq` for all token comparisons   | `api_server.py:L420`    |
| 5  | Session ID with null bytes             | CRLF guard rejects null bytes too                    | `api_server.py:L600`    |
| 6  | Multiple redirects (301 → 302 → 200)  | Guard fires on EVERY redirect, not just first        | `slack.py:L660`         |
| 7  | API server bound to 0.0.0.0 no key    | Hard fail at startup — refuse to bind               | `api_server.py:L1755`   |
| 8  | Allowlist username vs user ID          | Resolve usernames to IDs at startup (Discord)        | `discord.py:L1305`      |
| 9  | Button click from unauthorized user    | Check user ID before processing interaction          | `discord.py:L435`       |
| 10 | SSRF via WebSocket redirect            | WS upgrades don't follow HTTP redirects — no risk    | N/A                     |

---

## 6. Implementation Plan

### 6.1 Files to Create

| File                                           | Purpose                              |
|------------------------------------------------|--------------------------------------|
| `crates/edgecrab-gateway/src/auth.rs`          | Unified gateway authorization        |

### 6.2 Files to Modify

| File                                           | Change                                        |
|------------------------------------------------|-----------------------------------------------|
| `crates/edgecrab-security/src/url_safety.rs`   | Add `build_ssrf_safe_client()` with redirect guard |
| `crates/edgecrab-security/src/lib.rs`          | Add `validate_header_value()` for CRLF        |
| `crates/edgecrab-tools/src/tools/web.rs`       | Use `build_ssrf_safe_client()`                |
| `crates/edgecrab-gateway/src/slack.rs`         | Use `build_ssrf_safe_client()` for image DL   |
| `crates/edgecrab-gateway/src/api_server.rs`    | Add auth enforcement + bind guard             |
| `crates/edgecrab-gateway/src/lib.rs`           | Add `pub mod auth;`                           |
| `Cargo.toml` (edgecrab-security)              | Add `subtle` crate for constant-time ops      |

### 6.3 Dependencies

```toml
[dependencies]
subtle = "2"       # constant-time comparison
hmac = "0.12"      # HMAC-SHA1 for Twilio (when SMS ships)
sha1 = "0.10"      # SHA1 for Twilio signature validation
```

### 6.4 Test Matrix

| Test                                  | Validates                                        |
|---------------------------------------|--------------------------------------------------|
| `test_ssrf_redirect_private_ip`       | 302 to 127.0.0.1 blocked                        |
| `test_ssrf_redirect_metadata`         | 302 to 169.254.169.254 blocked                  |
| `test_ssrf_redirect_public_ok`        | 302 to public IP allowed                         |
| `test_ssrf_multi_hop_redirect`        | 301 → 302 → private IP blocked at step 2        |
| `test_crlf_injection_blocked`         | `\r\n` in header value rejected                  |
| `test_crlf_null_byte_blocked`         | Null byte in header value rejected               |
| `test_crlf_clean_value_ok`            | Clean string passes validation                   |
| `test_api_auth_timing_safe`           | Correct token accepted                           |
| `test_api_auth_wrong_token`           | Wrong token rejected with 401                    |
| `test_api_auth_missing_header`        | Missing Authorization header rejected            |
| `test_bind_guard_localhost_no_key`    | Localhost bind allowed without API key            |
| `test_bind_guard_public_no_key`       | Public bind refused without API key              |
| `test_twilio_signature_valid`         | Correct HMAC-SHA1 signature accepted             |
| `test_twilio_signature_tampered`      | Tampered signature rejected                       |
| `test_gateway_auth_allowed_user`      | Authorized user passes                            |
| `test_gateway_auth_denied_user`       | Unauthorized user blocked                         |

---

## 7. Security Audit Checklist

After implementation, verify:

- [ ] `reqwest` never used with default redirect policy for external URLs
- [ ] All gateway HTTP endpoints check auth when `API_SERVER_KEY` is set
- [ ] `subtle::ConstantTimeEq` used for ALL secret comparisons (no `==`)
- [ ] No user-controlled values in HTTP response headers without CRLF check
- [ ] Twilio webhook handler validates signature before processing
- [ ] API server refuses non-localhost bind without auth key
- [ ] Each platform adapter checks user authorization before processing

---

## 8. Acceptance Criteria

- [ ] SSRF redirect guard validates every 302 target via `is_safe_url()`
- [ ] `build_ssrf_safe_client()` available as public API in `edgecrab-security`
- [ ] `validate_header_value()` rejects CRLF and null bytes
- [ ] API server auth: timing-safe Bearer token, bind guard
- [ ] Gateway user authorization unified in `auth.rs`
- [ ] Twilio signature validation (for future SMS adapter)
- [ ] All security tests pass: `cargo test -p edgecrab-security`
- [ ] `cargo clippy --workspace -- -D warnings` clean
