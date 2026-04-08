# edgecrab-gateway

> **Why this crate?** Your agent shouldn't live only in a terminal tab. `edgecrab-gateway`  
> connects EdgeCrab to 15 messaging platforms — Telegram, Discord, Slack, WhatsApp, Signal,  
> Matrix, SMS, email, Home Assistant, and more — so you can reach it (and it can reach you)  
> from anywhere, on any device, without opening a laptop.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## Supported platforms

| Platform | Crate feature | Required env vars |
|----------|--------------|-------------------|
| Telegram | `telegram` | `TELEGRAM_BOT_TOKEN` |
| Discord | `discord` | `DISCORD_BOT_TOKEN` |
| Slack | `slack` | `SLACK_BOT_TOKEN`, `SLACK_APP_TOKEN` |
| WhatsApp | `whatsapp` | `WHATSAPP_PHONE_NUMBER_ID`, `WHATSAPP_ACCESS_TOKEN` |
| Signal | `signal` | `SIGNAL_CLI_PATH` |
| Matrix | `matrix` | `MATRIX_HOMESERVER`, `MATRIX_ACCESS_TOKEN` |
| Mattermost | `mattermost` | `MATTERMOST_URL`, `MATTERMOST_TOKEN` |
| DingTalk | `dingtalk` | `DINGTALK_APP_KEY`, `DINGTALK_APP_SECRET` |
| SMS (Twilio) | `sms` | `TWILIO_ACCOUNT_SID`, `TWILIO_AUTH_TOKEN`, `TWILIO_PHONE_NUMBER` |
| Email | `email` | `EMAIL_PROVIDER`, `EMAIL_FROM`, provider SMTP / API creds |
| Home Assistant | `homeassistant` | `HASS_URL`, `HASS_TOKEN` |
| Webhook | `webhook` | *(any HTTP caller)* |
| API server | `api_server` | `API_SERVER_PORT` *(optional, default 8080)* |

## Quick start

```bash
# Start the gateway (Telegram example)
export TELEGRAM_BOT_TOKEN=...
edgecrab gateway --platforms telegram
```

```toml
# Or in ~/.edgecrab/config.yaml
gateway:
  platforms: [telegram, discord]
  allowed_users: ["@alice", "123456789"]
```

## How it works

```
Platform adapter → DeliveryRouter → Agent.chat() → tool loop → reply
                                                              ↓
                                              StreamConsumer (live token edit)
```

Each platform implements the `PlatformAdapter` trait. `SessionManager` keeps one conversation  
per `(platform, user_id)` pair with configurable idle timeout. `DeliveryRouter` handles the  
`MEDIA://path` protocol for native photo / audio / document delivery.

## Per-user approval (DM pairing)

New users generate a pairing code your bot sends you in a DM. You approve with  
`/approve <code>` in the gateway session — no server restart needed.

## Add to your binary

```toml
[dependencies]
edgecrab-gateway = { path = "../edgecrab-gateway" }
```

```rust
use edgecrab_gateway::GatewayRunner;

GatewayRunner::new(agent, config)
    .add_platform("telegram")?
    .run()
    .await?;
```

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
