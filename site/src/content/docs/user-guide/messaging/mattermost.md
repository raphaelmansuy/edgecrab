---
title: Mattermost Setup
description: Connect EdgeCrab to a self-hosted or cloud Mattermost instance via REST API and WebSocket. Grounded in crates/edgecrab-gateway/src/mattermost.rs.
sidebar:
  order: 7
---

The Mattermost adapter connects to any Mattermost instance using the REST v4 API and WebSocket for real-time message events.

**Max message length**: 4000 characters.

---

## Prerequisites

1. A Mattermost instance (self-hosted or cloud)
2. A bot account or personal access token with `create_post` permissions
3. The bot added to at least one channel

---

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `MATTERMOST_URL` | **Yes** | Server URL (e.g. `https://chat.example.com`) |
| `MATTERMOST_TOKEN` | **Yes** | Bot token or personal access token |
| `MATTERMOST_ALLOWED_USERS` | No | Comma-separated Mattermost user IDs |

---

## Setup

### 1. Create a Bot Account

In Mattermost **System Console → Integrations → Bot Accounts**, create a new bot. Copy the access token.

Alternatively, use a personal access token: **Account Settings → Security → Personal Access Tokens**.

### 2. Set Environment Variables

```bash
# ~/.edgecrab/.env
MATTERMOST_URL=https://chat.example.com
MATTERMOST_TOKEN=your-bot-token
MATTERMOST_ALLOWED_USERS=user1_id,user2_id
```

### 3. Add Bot to Channels

In Mattermost, add the bot to any channels it should respond in.

### 4. Start the Gateway

```bash
edgecrab gateway start
```

---

## Usage

- Mention the bot in a channel: `@edgecrab-bot explain this PR`
- DM the bot directly for a private conversation
- Each channel maintains its own persistent EdgeCrab session

**Gateway slash commands** (send as Mattermost messages):

| Command | Effect |
|---------|--------|
| `/help` | List all available gateway commands |
| `/new` | Start a fresh conversation (clears history) |
| `/reset` | Alias for `/new` |
| `/stop` | Cancel the currently running agent response |
| `/retry` | Re-send your last message |
| `/status` | Show whether the agent is running or idle |
| `/usage` | Show session stats (running, queued, retryable) |
| `/hooks` | List loaded event hooks |

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `401 Unauthorized` | Wrong token | Check `MATTERMOST_TOKEN` |
| Bot doesn't see messages | Not in channel | Add bot to the channel |
| WebSocket disconnect | Network issue | Gateway auto-reconnects with exponential backoff |

---

## Pro Tips

- **Bot account vs. personal access token:** Use a dedicated bot account for production. Bot accounts can be disabled centrally, don't count against your license seat on Enterprise, and show a distinct bot icon in channels.
- **System Console permissions:** Bot accounts need `create_post` permission explicitly enabled in *System Console → Integration Management → Enable Bot Account Creation*.
- **Channel naming convention:** Name the bot `@edgecrab` for auto-complete discoverability. Users can mention it without remembering exotic names.
- **Self-hosted Mattermost:** Set `MATTERMOST_URL` without a trailing slash. The adapter appends API paths internally.
- **Message threads:** Mattermost thread replies maintain context from the parent post. Replies in a thread go to the same EdgeCrab session as the channel.

---

## FAQ

**Q: Bot joins the channel but never responds to `@mentions`.**  
Check that Event Subscriptions are enabled in your Mattermost app configuration. The adapter uses REST polling if WebSocket fails, so check `MATTERMOST_URL` is reachable and the token is valid.

**Q: Can I use Mattermost slash commands (not just messages)?**  
Not currently — EdgeCrab responds to DMs and `@mentions`, not registered Mattermost slash commands. The `/help`, `/new`, etc. commands above are sent as messages, not registered slash command endpoints.

**Q: Does EdgeCrab work with Mattermost Cloud?**  
Yes — set `MATTERMOST_URL` to your cloud URL (e.g. `https://yourteam.mattermost.com`) and use a personal access token from your account settings.

**Q: Can multiple team members share one EdgeCrab session in a channel?**  
Yes — all messages in a channel share one session. This is useful for shared project channels but means context can mix between users. Use DMs for private, per-user sessions.

**Q: The WebSocket shows `403` in logs.**  
Ensure your Mattermost instance allows WebSocket connections. Some reverse proxies (nginx, Apache) need `Upgrade: websocket` headers forwarded. Add `proxy_set_header Upgrade $http_upgrade;` to your proxy config.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) — Multi-platform setup
- [Security Model](/user-guide/security/) — Approval workflow and access control
- [Self-Hosting Guide](/guides/self-hosting/) — Running EdgeCrab in production
- [Sessions](/user-guide/sessions/) — One session per channel
