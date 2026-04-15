---
title: Security Hardening (v0.6)
description: v0.6 security enhancements — hardened SSRF client, gateway webhook validation, skills guard, and proxy support. Grounded in crates/edgecrab-security/ and crates/edgecrab-gateway/.
sidebar:
  order: 14
---

EdgeCrab v0.6 introduces several security hardening improvements across the stack.

---

## Hardened SSRF HTTP Client

All web tools now share a single hardened HTTP client constructed via `build_ssrf_client()` in `edgecrab-security`:

- **DNS pre-resolution**: IP addresses checked against private range blocklist before connection
- **Redirect following disabled**: Prevents open-redirect SSRF chains
- **Connection timeouts**: Enforced at the client level
- **Shared configuration**: All web endpoints use the same security posture

Previously, each web tool constructed its own HTTP client. The centralized client ensures consistent SSRF protection.

---

## Gateway Webhook Validation

### Twilio SMS Signature Verification

Incoming SMS webhooks from Twilio are now validated using `X-Twilio-Signature` HMAC-SHA1 verification:

```
Incoming POST → Extract X-Twilio-Signature header
             → Compute HMAC-SHA1(auth_token, url + sorted_params)
             → Compare against header value
             → Reject if mismatch
```

This prevents spoofed webhook requests from unauthorized senders.

### Weixin XML Encryption

WeChat messages use AES-256-CBC XML encryption (`weixin_crypto.rs`):
- Messages are encrypted in transit using the app's encoding AES key
- Signatures are verified using SHA-1 of token + timestamp + nonce
- Decrypted XML payloads are parsed for message content

---

## Skills Security Scanner

External skills installed via `/skills install` pass through a 23-pattern security scanner (`skills_guard.rs`):

| Category | Patterns | Example |
|----------|----------|---------|
| Exfiltration | 5 | `curl/wget` piping secrets, base64 encoding credentials |
| Injection | 4 | Prompt override attempts, hidden Unicode instructions |
| Destructive | 4 | `rm -rf`, `format`, database drop commands |
| Persistence | 5 | Crontab modification, startup script injection |
| Obfuscation | 5 | Base64-encoded commands, Unicode homoglyphs, steganography |

Skills exceeding the severity threshold are quarantined and not installed.

---

## HTTP Proxy Support

All outbound HTTP requests now respect `HTTPS_PROXY` / `HTTP_PROXY` environment variables. See [Proxy documentation](/features/proxy/) for details.

---

## Sandbox Hardening

The `execute_code` sandbox now blocks additional terminal parameters in sandboxed tool calls:
- `background` — no background processes from sandbox
- `check_interval` — no polling from sandbox
- `pty` — no pseudo-terminal allocation from sandbox
- `watch_patterns` — no output pattern monitoring from sandbox (new in v0.6)
