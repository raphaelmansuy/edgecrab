---
title: HTTP Proxy
description: Configure outbound HTTP proxy for EdgeCrab. Grounded in crates/edgecrab-security/src/proxy.rs.
sidebar:
  order: 11
---

EdgeCrab supports routing outbound HTTP requests through an HTTP/HTTPS proxy. This is useful for corporate networks, VPNs, or when you need to inspect/audit outbound traffic.

---

## Configuration

Set standard environment variables:

```bash
export HTTPS_PROXY=http://proxy.corp.example.com:8080
export HTTP_PROXY=http://proxy.corp.example.com:8080
```

All outbound requests from web tools, gateway adapters, and LLM API calls will route through the configured proxy.

---

## How It Works

The proxy configuration is applied at the `edgecrab-security` crate level via `build_ssrf_client()`. This ensures:

- All HTTP clients share the same proxy settings
- SSRF guards still apply (requests to private IPs are blocked even through a proxy)
- TLS verification is maintained

---

## Supported Proxy Types

| Type | Variable | Example |
|------|----------|---------|
| HTTPS proxy | `HTTPS_PROXY` | `http://proxy:8080` |
| HTTP proxy | `HTTP_PROXY` | `http://proxy:8080` |
| No proxy | `NO_PROXY` | `localhost,127.0.0.1,.internal` |

---

## Notes

- The proxy applies to all outbound HTTP calls: LLM API, web tools, gateway webhook registrations
- SOCKS5 proxies are not currently supported
- Proxy authentication (basic auth in URL) is supported: `http://user:pass@proxy:8080`
