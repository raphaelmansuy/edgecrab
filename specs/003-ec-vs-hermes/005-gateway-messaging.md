# 005 — Gateway & Messaging

Multi-platform delivery: adapters, session model, operator features.

---

## Built-in platform adapters

### EdgeCrab (`edgecrab-gateway/src/`)

17 `PlatformAdapter` implementations:

| Platform | Module |
|----------|--------|
| Telegram | `telegram.rs` |
| Discord | `discord.rs` |
| Slack | `slack.rs` |
| WhatsApp | `whatsapp.rs` |
| Signal | `signal.rs` |
| SMS (Twilio) | `sms.rs` |
| Email | `email.rs` |
| Matrix | `matrix.rs` |
| Mattermost | `mattermost.rs` |
| DingTalk | `dingtalk.rs` |
| Feishu/Lark | `feishu.rs` |
| WeCom | `wecom.rs` |
| WeChat (Weixin) | `weixin.rs` |
| BlueBubbles (iMessage) | `bluebubbles.rs` |
| Home Assistant | `homeassistant.rs` |
| Webhook | `webhook.rs` |
| API Server | `api_server.rs` |

### Hermes (`gateway/platforms/` + plugins)

**Built-in (~20):** telegram, discord, slack, whatsapp (+ cloud variant), signal, sms, email, matrix, mattermost, dingtalk, feishu, wecom, weixin, bluebubbles, webhook, api_server, msgraph_webhook, yuanbao, qqbot (partial)

**Plugin platforms (`plugins/platforms/`):**

| Plugin | Platform |
|--------|----------|
| google_chat | Google Chat |
| teams | Microsoft Teams |
| line | LINE |
| ntfy | ntfy push |
| simplex | SimpleX |
| photon | Photon/Matrix-related |
| irc | IRC |
| homeassistant | HA (also built-in on EC) |
| mattermost | duplicate path |
| discord | voice extensions |

---

## Platform coverage verdict

| Region / channel | Hermes | EdgeCrab |
|------------------|--------|----------|
| Western chat (TG/Discord/Slack) | A | A |
| WhatsApp / Signal / SMS | A | A |
| Email | A | A |
| Matrix / Mattermost | A | A |
| China (Feishu/WeCom/Weixin/DingTalk) | A | A |
| iMessage (BlueBubbles) | A | A |
| **Teams / Google Chat / LINE** | A (plugins) | **D** |
| **ntfy / SimpleX / IRC / Photon** | A (plugins) | **D** |
| **Yuanbao / QQ** | A | **D** |
| Home Assistant | A | A |
| OpenAI-compat API server | A | A |

**Verdict:** **Hermes leads long tail** (~10 plugin platforms). **Parity on major messaging APIs**.

---

## Gateway subsystems

| Feature | Hermes | EdgeCrab |
|---------|--------|----------|
| Session manager | `gateway/session.py` | `session.rs` |
| Stream editing (typing…) | Yes | `stream_consumer.rs` (300ms throttle) |
| Message splitting | Yes | `delivery.rs` |
| MEDIA:// native upload | Yes | Yes |
| DM pairing codes | `pairing.py` | `pairing.rs` |
| Cross-session mirror | `mirror.py` | `mirror.rs` |
| Channel directory | Partial | `channel_directory.rs` |
| Lifecycle hooks | `hooks.py` + scripts | `hooks.rs` + `~/.edgecrab/hooks/` |
| Voice delivery (TTS) | Yes | `voice_delivery.rs` |
| Webhook subscriptions | Partial | `webhook_subscriptions.rs` |
| **Circuit breaker / pause platform** | **`/platform pause`** | Partial |
| **Discord history backfill** | Yes | Gap 016 |
| **Native clarify buttons** | Partial | Gap 015 |
| Second message mode | `/busy` | `second_message_mode` config |
| Session handoff CLI→chat | `/handoff` | `platform_handoff.rs` (spec 005) |

**Verdict:** **Hermes leads resilience ops** (circuit breaker, backfill). **EdgeCrab leads handoff + stream consumer polish**.

---

## Authorization & DM policy

Both support:

- Pairing codes for unknown DMs
- Admin vs user slash command tiers
- Per-platform allowlists
- Group mention gating (platform-specific)

**Verdict:** **Parity (A)** — platform-specific edge cases differ; both production-viable.

---

## Gateway config split

| | Hermes | EdgeCrab |
|---|--------|----------|
| Separate gateway yaml | `gateway-config.yaml` | Unified `config.yaml` `gateway:` |
| Profile-scoped gateway | Yes | Profiles supported |

**Verdict:** **≠** — Hermes separates operator config; EdgeCrab simplifies.

---

## Grades

| Dimension | Hermes | EdgeCrab |
|-----------|--------|----------|
| Core platforms | A | A |
| Long-tail platforms | A | C |
| Streaming UX | A | A |
| Session persistence | A | A |
| Operator tooling | A | B+ |
| Handoff / mirror | A | A− |

Cross-ref: [001-gap-analysis 005/015/016/026](../001-gap-analysis-v14/999-roadmap.md)
