## Feature v 0.6

Update specification and ADR to implement features from
https://github.com/NousResearch/hermes-agent/blob/main/RELEASE_v0.9.0.md

---

## Feature List

| # | Feature                                  | ADR          | Spec Document                                  | Status   |
|---|------------------------------------------|--------------|-------------------------------------------------|----------|
| 1 | iMessage via BlueBubbles                 | ADR-0601     | [06-feature-06-01-bluebubbles-imessage.md]      | Proposed |
| 2 | WeChat (Weixin) & WeCom Callback Mode    | ADR-0602     | [06-feature-06-02-wechat-wecom.md]              | Proposed |
| 3 | Termux / Android Support                 | ADR-0603     | [06-feature-06-03-termux-android.md]            | Proposed |
| 4 | Background Process Monitoring            | ADR-0604     | [06-feature-06-04-watch-patterns.md]            | Proposed |
| 5 | Pluggable Context Engine                 | ADR-0605     | [06-feature-06-05-pluggable-context-engine.md]  | Proposed |
| 6 | Unified Proxy Support                    | ADR-0606     | [06-feature-06-06-unified-proxy.md]             | Proposed |
| 7 | Comprehensive Security Hardening         | ADR-0607     | [06-feature-06-07-security-hardening.md]        | Proposed |
| 8 | edgecrab backup & edgecrab import        | ADR-0608     | [06-feature-06-08-backup-import.md]             | Proposed |
| 9 | /debug & edgecrab dump                   | ADR-0609     | [06-feature-06-09-debug-dump.md]                | Proposed |

### 16 Supported Platforms

Feature #10 ("16 Supported Platforms") is a meta-feature — the platform count
reaches 16+ once the new adapters from ADR-0601 (BlueBubbles/iMessage) and
ADR-0602 (WeChat + WeCom) are added to the existing 13 platform adapters:

| #  | Platform         | Status      | Adapter Crate        |
|----|------------------|-------------|----------------------|
| 1  | CLI              | Existing    | edgecrab-cli         |
| 2  | Telegram         | Existing    | edgecrab-gateway     |
| 3  | Discord          | Existing    | edgecrab-gateway     |
| 4  | Slack            | Existing    | edgecrab-gateway     |
| 5  | WhatsApp         | Existing    | edgecrab-gateway     |
| 6  | Signal           | Existing    | edgecrab-gateway     |
| 7  | Email            | Existing    | edgecrab-gateway     |
| 8  | Matrix           | Existing    | edgecrab-gateway     |
| 9  | Mattermost       | Existing    | edgecrab-gateway     |
| 10 | DingTalk         | Existing    | edgecrab-gateway     |
| 11 | SMS (Twilio)     | Existing    | edgecrab-gateway     |
| 12 | Webhook          | Existing    | edgecrab-gateway     |
| 13 | Home Assistant   | Existing    | edgecrab-gateway     |
| 14 | API Server       | Existing    | edgecrab-gateway     |
| 15 | Feishu           | Existing    | edgecrab-gateway     |
| 16 | WeCom            | ADR-0602    | edgecrab-gateway     |
| 17 | WeChat (Weixin)  | ADR-0602    | edgecrab-gateway     |
| 18 | iMessage         | ADR-0601    | edgecrab-gateway     |
| 19 | ACP (VS Code)    | Existing    | edgecrab-acp         |
| 20 | Cron             | Existing    | edgecrab-cron        |

---

## Cross-Cutting Concerns

| Concern                  | Covered By          | Notes                                       |
|--------------------------|---------------------|----------------------------------------------|
| SSRF redirect guard      | ADR-0607 §4.1       | `url_safety.rs` redirect-following guard     |
| Webhook signature auth   | ADR-0607 §4.2       | Platform-specific HMAC verification          |
| API server auth          | ADR-0607 §4.3       | Bearer token enforcement                    |
| CRLF header injection    | ADR-0607 §4.4       | Header value sanitization                   |
| Proxy for all HTTP       | ADR-0606             | `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY`   |
| Path traversal in backup | ADR-0608 §5          | Tar member validation, symlink blocking      |
| Credential exclusion     | ADR-0608 §4.1       | `.env`, `mcp-tokens/` never in backup        |
| Key redaction in dump    | ADR-0609 §4.3       | First 4 + last 4 chars only                  |

---

## Implementation Order (Recommended)

```
Phase 1 — Security & Infrastructure
  ADR-0607  Security Hardening         (unblocks all network features)
  ADR-0606  Unified Proxy              (unblocks all HTTP-based adapters)
  ADR-0604  watch_patterns             (independent, low risk)

Phase 2 — New Platforms
  ADR-0602  WeChat & WeCom             (WeCom stub exists, extend)
  ADR-0601  BlueBubbles / iMessage     (new adapter, HTTP REST)

Phase 3 — User Tools
  ADR-0605  Pluggable Context Engine   (cross-cutting, medium risk)
  ADR-0608  Backup & Import            (new CLI subcommands)
  ADR-0609  /debug & dump              (new CLI subcommand + slash command)

Phase 4 — Platform Support
  ADR-0603  Termux / Android           (conditional compilation, testing)
```

[06-feature-06-01-bluebubbles-imessage.md]: 06-feature-06-01-bluebubbles-imessage.md
[06-feature-06-02-wechat-wecom.md]: 06-feature-06-02-wechat-wecom.md
[06-feature-06-03-termux-android.md]: 06-feature-06-03-termux-android.md
[06-feature-06-04-watch-patterns.md]: 06-feature-06-04-watch-patterns.md
[06-feature-06-05-pluggable-context-engine.md]: 06-feature-06-05-pluggable-context-engine.md
[06-feature-06-06-unified-proxy.md]: 06-feature-06-06-unified-proxy.md
[06-feature-06-07-security-hardening.md]: 06-feature-06-07-security-hardening.md
[06-feature-06-08-backup-import.md]: 06-feature-06-08-backup-import.md
[06-feature-06-09-debug-dump.md]: 06-feature-06-09-debug-dump.md

