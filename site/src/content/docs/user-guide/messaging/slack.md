---
title: Slack
description: Connect EdgeCrab to Slack using Socket Mode. Grounded in crates/edgecrab-gateway/src/slack.rs.
sidebar:
  order: 3
---

## Prerequisites

Slack requires **two tokens** — a bot token and an app-level token for Socket Mode:

1. Create a Slack app at https://api.slack.com/apps
2. Under **OAuth & Permissions**, add bot scopes: `chat:write`, `app_mentions:read`, `im:history`, `im:read`
3. Install to workspace → copy **Bot User OAuth Token** (`xoxb-...`)
4. Enable **Socket Mode** → generate an app-level token with `connections:write` scope → copy token (`xapp-...`)
5. Enable **Event Subscriptions** → subscribe to `app_mention`, `message.im`

---

## Configuration

### Environment Variables (Quick Start)

```bash
export SLACK_BOT_TOKEN=xoxb-...
export SLACK_APP_TOKEN=xapp-...
edgecrab gateway start
```

Both `SLACK_BOT_TOKEN` and `SLACK_APP_TOKEN` must be set to auto-enable Slack.

### config.yaml

```yaml
gateway:
  slack:
    enabled: true
    bot_token_env: "SLACK_BOT_TOKEN"   # xoxb-...
    app_token_env: "SLACK_APP_TOKEN"   # xapp-...
    allowed_users: []                   # Slack user IDs (U...)
    home_channel: ~                     # channel ID for proactive messages
```

Additional optional env var:

| Variable | Effect |
|----------|--------|
| `SLACK_ALLOWED_USERS` | Comma-separated Slack user IDs |

---

## Usage

Mention `@EdgeCrab` in any channel, or send a direct message. Each Slack channel/DM gets its own session.

**Approval flow:** EdgeCrab adds Block Kit action buttons (Approve / Deny) to the approval message.

---

## Pro Tips

- **Socket Mode is preferred over HTTP events** — you don't need a public-facing webhook URL. Keep `SLACK_APP_TOKEN` (xapp-) set and EdgeCrab uses Socket Mode automatically.
- **Pin the bot to a channel:** Create a dedicated `#edgecrab` channel and remove the bot from all others. Reduces noise and makes sessions predictable.
- **Set `allowed_users`** using Slack User IDs (starts with `U`). Find your ID: in Slack, click your profile \u2192 *More* \u2192 *Profile* \u2192 three dots \u2192 *Copy member ID*.
- **Slash commands:** You can register EdgeCrab slash commands in the Slack App config (under *Slash Commands*) that map to EdgeCrab gateway actions like `/ecnew` (new session) or `/ecstop` (stop task).
- **Long responses:** EdgeCrab auto-chunks responses at 39,000 characters (Slack's practical limit). Chunks arrive as sequential messages in the same channel. There is no automatic thread-posting — long conversations stay in-line.

---

## Troubleshooting

**"An API error occurred: not_in_channel":**  
Invite the bot to the channel: `/invite @EdgeCrab`

**Bot receives no events:**  
Check Event Subscriptions in the Slack App config are enabled and subscribed to `app_mention` and `message.im`. Socket Mode must be enabled and `SLACK_APP_TOKEN` must start with `xapp-`

**Messages appear but bot doesn't reply:**  
Verify the bot has the `chat:write` scope in *OAuth & Permissions*. Re-install the app to workspace after adding scopes.

**"Missing scope: app_mentions:read":**  
Add `app_mentions:read` scope in *OAuth & Permissions* \u2192 *Bot Token Scopes*, then reinstall (click *Reinstall to Workspace*).

---

## FAQ

**Q: Why does Slack need two tokens (bot token + app token)?**  
The `xoxb-` bot token authenticates API calls. The `xapp-` app-level token is used specifically for Socket Mode \u2014 the persistent WebSocket connection that delivers events to EdgeCrab without a public webhook.

**Q: Can I use Incoming Webhooks instead of Socket Mode?**  
Incoming Webhooks only send messages *to* Slack \u2014 they can't receive messages. You need Socket Mode or HTTP Events API to receive.

**Q: How do I restrict EdgeCrab to specific Slack channels?**  
Use `allowed_users` to restrict *who* can chat. To restrict *where*, don't invite the bot to other channels (EdgeCrab only responds where it's invited).

**Q: Is there a difference between `app_mention` and DM sessions?**  
Channel mentions and DMs are separate sessions \u2014 each channel/DM gets its own persistent EdgeCrab session and memory context.

**Q: The bot works in direct messages but not in channels.**  
In channels, you must `@mention` the bot unless you've subscribed to `message.channels` scope (which receives all messages, not just mentions). Usually `@mention` is the right approach.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) \u2014 Multi-platform setup
- [Cron Jobs](/features/cron/) \u2014 Posting scheduled summaries to Slack
- [Security Model](/user-guide/security/) \u2014 Approval workflow
- [Self-Hosting Guide](/guides/self-hosting/) \u2014 Running the gateway in production
