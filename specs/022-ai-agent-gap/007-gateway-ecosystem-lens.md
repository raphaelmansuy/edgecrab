# 007 — Gateway & Messaging Lens (Re-assessed)

**Authority:** [000 §11](000-code-is-law.md) · J9  
**Date:** 2026-07-19

---

## 1. Job

Same agent brain → many human habitats: event → authz → session → turn → stream → media → mirror.

---

## 2. Platform coverage (law)

### EdgeCrab — 17 production `impl PlatformAdapter`

Telegram, Discord, Slack, WhatsApp, Signal, Matrix, Mattermost, DingTalk, Feishu, WeCom, Weixin, BlueBubbles, Email, SMS, Webhook, Home Assistant, API Server.

### Hermes

- **20** `plugins/platforms/*` including Teams, Google Chat, LINE, ntfy, IRC, SimpleX, Photon, Raft, …  
- In-tree specialties: Yuanbao, WhatsApp cloud, MS Graph webhook, Signal rate limit helpers, …

**Net:** EC batteries-included core; H long-tail via plugins.

---

## 3. Shared features

Stream consumer, channel directory, DM pairing, mirror, MEDIA://, hooks, slash on gateway, handoff, second_message_mode (queue/steer/interrupt), Discord backfill — **parity class**.

### Differentiation

| Feature | Leader | Evidence |
|---------|--------|----------|
| Clarify multi-platform buttons | **EC** | TG/Discord/WA wiring |
| Circuit breaker / drain / scale-to-zero | **H** | gateway modules |
| Profile routing | **H** | profile_routing |
| Relay contracts | **H** | docs + relay/ |
| Restart loop guard | **H** | restart_loop_guard |

---

## 4. Recommendation

```text
Do not port 20 platforms into Rust without demand.
1) Perfect core 17 + circuit breaker + profile routing
2) Webhook + MCP for long-tail
3) Native adapter only with proven P2 demand
```

---

## 5. Scorecard

| Dimension | Score |
|-----------|-------|
| Core platforms | = |
| Long-tail | **H** |
| Stream/pairing/MEDIA | = |
| Ops resilience | **H** |
| Batteries-included | **EC** |
| Clarify multi-platform | **EC** |
