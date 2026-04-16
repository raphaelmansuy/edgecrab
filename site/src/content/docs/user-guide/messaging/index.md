---
title: Messaging Gateway
description: Run EdgeCrab as a persistent AI agent accessible from Telegram, Discord, Slack, Signal, WhatsApp, Matrix, Mattermost, DingTalk, SMS, Email, Home Assistant, Feishu, WeCom, iMessage, and WeChat. Grounded in crates/edgecrab-gateway/src/.
sidebar:
  order: 11
---

The EdgeCrab Gateway runs as a background process that bridges messaging platforms to the EdgeCrab agent. Each message from a platform creates or resumes an agent session; replies are sent back through the same platform.

---

## Quick Start

```bash
# Start gateway with Telegram
TELEGRAM_BOT_TOKEN=xxxx edgecrab gateway start

# Or with the gateway config section
edgecrab gateway start --foreground
```

The gateway listens on `http://127.0.0.1:8080` by default. Platform adapters connect as clients.

---

## How It Works

```
Platform message --> Gateway HTTP server --> Platform adapter --> Session manager --> Agent loop --> Reply
                     (127.0.0.1:8080)      (Telegram/Discord/...)  (SQLite)        (full tools)
```

Each platform maintains independent sessions. A Telegram conversation and a Discord conversation with the same user have separate agent contexts unless manually merged.

---

## Supported Platforms (17 total)

| Platform | Required Env Vars | Guide |
|----------|-------------------|-------|
| Telegram | `TELEGRAM_BOT_TOKEN` | [Telegram Setup](/user-guide/messaging/telegram/) |
| Discord | `DISCORD_BOT_TOKEN` | [Discord Setup](/user-guide/messaging/discord/) |
| Slack | `SLACK_BOT_TOKEN` + `SLACK_APP_TOKEN` | [Slack Setup](/user-guide/messaging/slack/) |
| Signal | `SIGNAL_HTTP_URL` + `SIGNAL_ACCOUNT` | [Signal Setup](/user-guide/messaging/signal/) |
| WhatsApp | `WHATSAPP_ENABLED=1` | [WhatsApp Setup](/user-guide/messaging/whatsapp/) |
| Matrix | `MATRIX_HOMESERVER` + `MATRIX_ACCESS_TOKEN` | [Matrix Setup](/user-guide/messaging/matrix/) |
| Mattermost | `MATTERMOST_URL` + `MATTERMOST_TOKEN` | [Mattermost Setup](/user-guide/messaging/mattermost/) |
| DingTalk | `DINGTALK_APP_KEY` + `DINGTALK_APP_SECRET` | [DingTalk Setup](/user-guide/messaging/dingtalk/) |
| SMS (Twilio) | `TWILIO_ACCOUNT_SID` + `TWILIO_AUTH_TOKEN` + `TWILIO_PHONE_NUMBER` | [SMS Setup](/user-guide/messaging/sms/) |
| Email | `EMAIL_PROVIDER` + `EMAIL_FROM` + provider-specific credentials | [Email Setup](/user-guide/messaging/email/) |
| Home Assistant | `HA_URL` + `HA_TOKEN` | [Home Assistant](/user-guide/messaging/homeassistant/) |
| Feishu/Lark | `FEISHU_APP_ID` + `FEISHU_APP_SECRET` | — |
| WeCom | `WECOM_BOT_ID` + `WECOM_SECRET` | — |
| Webhook | *(any HTTP caller)* | — |
| API Server | `API_SERVER_PORT` *(optional)* | — |
| iMessage | `BLUEBUBBLES_SERVER_URL` + `BLUEBUBBLES_PASSWORD` | [iMessage Setup](/user-guide/messaging/imessage/) |
| WeChat | `WEIXIN_TOKEN` + `WEIXIN_ACCOUNT_ID` | [WeChat Setup](/user-guide/messaging/wechat/) |

---

## Gateway Configuration

```yaml
# ~/.edgecrab/config.yaml
gateway:
  host: "127.0.0.1"       # bind address (use 0.0.0.0 to accept external connections)
  port: 8080
  webhook_enabled: true
  session_timeout_minutes: 30
```

Override with environment variables:

```bash
EDGECRAB_GATEWAY_HOST=0.0.0.0
EDGECRAB_GATEWAY_PORT=9090
```

---

## Security

All platforms support an `allowed_users` list. When set, messages from other users are silently ignored:

```yaml
gateway:
  telegram:
    enabled: true
    allowed_users: ["myusername", "teammate"]

  discord:
    enabled: true
    allowed_users: ["123456789012345678"]  # Discord user IDs
```

For maximum security, run the gateway on `127.0.0.1` behind a reverse proxy with TLS.

---

## Home Channel (Proactive Messaging)

When `home_channel` is set, EdgeCrab can send proactive messages — e.g., from cron jobs or completed background tasks:

```yaml
gateway:
  telegram:
    home_channel: "-100123456789"  # chat ID
```

Then from the TUI or the agent:

```
/sethome           # set current channel as home_channel
```

---

## Managing Platforms

```
/platforms         # show status of all configured platforms
```

From the CLI:

```bash
edgecrab gateway status          # gateway status + connected platforms
edgecrab gateway start           # start the gateway daemon
edgecrab gateway stop            # stop the gateway daemon
edgecrab gateway logs            # follow gateway logs
```

---

## Approval Workflow

When `security.approval_required` is set, commands matching those patterns require explicit approval before execution:

```yaml
security:
  approval_required:
    - "rm "
    - "git push"
    - "kubectl delete"
```

The agent sends a confirmation message to the platform; you reply `/approve` or `/deny` (or click the inline button on Telegram/Discord).

---

## Gateway Architecture

```
[Telegram Bot]  -+
[Discord Bot]   -+
[Slack App]     -+-> [EdgeCrab Gateway] --> [Agent Loop] --> [LLM + Tools]
[WhatsApp]      -+             |
[Signal]        -+        [SQLite DB]   <- sessions stored per platform/chat
[Matrix]        -+
```

Each platform adapter is stateless — session context is always in SQLite.

---

## Pro Tips

**Start small: one platform at a time.** Don't configure all 11 platforms at once. Start with Telegram or Discord, verify it works, then add more.

**Set `allowed_users` immediately.** Without it, anyone who finds your bot can interact with your agent. Whether that's acceptable depends on your use case but for personal use, always restrict.

**Use the home channel for cron results.** When EdgeCrab runs a scheduled task, the result appears in your Telegram/Discord channel automatically. This turns EdgeCrab into a personal monitoring bot.

**Run the gateway in Docker for 24/7 availability.** A local terminal session will disconnect. Use the Docker setup from [Self-Hosting](/guides/self-hosting/) for a persistent gateway.

---

## Frequently Asked Questions

**Q: The gateway starts but no messages arrive.**

Check the platform-specific setup (the bot token is set, the bot is added to the chat, intents are enabled). Run `edgecrab gateway status` to see which platforms are connected. For Telegram, ensure you're messaging the correct bot username.

**Q: I want to run multiple gateways with different models.**

Use profiles. Create a `work` profile with `model: anthropic/claude-opus-4` and run:
```bash
edgecrab -p work gateway start &  # uses claude-opus on Slack
edgecrab -p fast gateway start &  # uses copilot/gpt-4.1-mini on Telegram
```
Note: multiple gateway processes on the same port will conflict — use different ports via `gateway.port`.

**Q: How do I handle long agent responses in Telegram/Discord?**

All platforms have message length limits. EdgeCrab auto-chunks responses at each platform's limit:

| Platform | Char Limit | Behavior |
|----------|-----------|----------|
| Telegram | 4,096 | Chunked into sequential messages |
| Discord | 2,000 | Chunked into sequential messages |
| Slack | 39,000 | Chunked into sequential messages |
| Matrix | 4,000 | Chunked |
| Mattermost | 4,000 | Chunked |
| Signal | 8,000 | Chunked |
| WhatsApp | 65,536 | Chunked |
| DingTalk | 6,000 | Chunked |
| SMS | 1,600 | Chunked (~10 segments) |
| Email | 50,000 | Single message |
| Home Assistant | 10,000 | Single message |

If a response is truncated, ask the agent to "continue" or shorten the response.

**Q: Can the bot handle multiple users simultaneously?**

Yes. Each platform/user/channel combination gets its own agent session. Multiple users can interact with the bot concurrently.

**Q: Is the gateway traffic encrypted?**

The gateway server itself runs over HTTP on localhost. For externally accessible deployments, put it behind a reverse proxy with TLS (nginx or Caddy). Platform connections (Telegram, Discord, etc.) use HTTPS/WSS to the platform APIs.

**Q: Can I test the gateway without a messaging app?**

Use the OpenAI-compatible HTTP API directly:
```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "edgecrab", "messages": [{"role": "user", "content": "hello"}]}'
```

---

## See Also

- [Telegram](/user-guide/messaging/telegram/)
- [Discord](/user-guide/messaging/discord/)
- [Slack](/user-guide/messaging/slack/)
- [Self-Hosting Guide](/guides/self-hosting/) — Docker gateway deployment
- [Cron Jobs](/features/cron/) — Using cron with home channel messages
- [Security Model](/user-guide/security/) — Approval workflow, allowed users
