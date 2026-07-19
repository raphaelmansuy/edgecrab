# 023 — First Principles: Browser ↔ Localhost Reliability

**Status:** Implemented (Chrome launch policy + honest nav failures)  
**Date:** 2026-07-19  
**Companion:** [022 harness deadlock](022-session-roadblock-4f94111e-harness-deadlock.md)

## Laws

| # | Principle | Implementation |
|---|-----------|----------------|
| L1 | Transport ≠ document ≠ scene | ContentClass + heal latch + WebGL path |
| L2 | External browser ≠ CDP client | Headless software GL; no `--disable-gpu` default |
| L3 | Loopback is never proxied | Chrome `--proxy-bypass-list=<-loopback>` matches reqwest |
| L4 | Failure messages name the true layer | `http_error_page` not “SSRF, CDP, or…” |
| L5 | edgecrab-proxy is LLM-only | Docs/config; not browser path |

## Config

```yaml
browser:
  headless_gpu: software   # or disable (legacy blank canvas)
  proxy_bypass_loopback: true
```

Env: `EDGECRAB_BROWSER_HEADLESS_GPU=software|disable`

## Code

- `edgecrab-tools` `browser.rs`: `BrowserLaunchPolicy`, `chrome_headless_launch_args`
- `AppConfig::apply_security_runtime` installs policy
- `harness_advisory`: structured fail class in thrash blocks
- **`browser_diagnostics.rs`**: single report for `/browser status` + `edgecrab doctor` / `doctor harness`
  - CDP mode/reachability, Chrome version
  - GPU mode + flags
  - Proxy + loopback bypass
  - security.preview
  - session HTTP ports
  - **URL content probes** (no proxy): HTTP status + `ContentClass` + title
