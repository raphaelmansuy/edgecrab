---
title: Discord
description: Connect EdgeCrab to Discord. Step-by-step bot setup, permissions, security, troubleshooting, and pro tips. Grounded in crates/edgecrab-gateway/src/discord.rs.
sidebar:
  order: 2
---

## Prerequisites

1. Create a Discord application at https://discord.com/developers/applications
2. Add a Bot under the "Bot" tab — enable **Message Content Intent**
3. Copy the bot token
4. Invite the bot: `https://discord.com/api/oauth2/authorize?client_id=<id>&permissions=2048&scope=bot`

---

## Configuration

### Environment Variable (Quick Start)

```bash
export DISCORD_BOT_TOKEN=OTY3...
edgecrab gateway start
```

### config.yaml

```yaml
gateway:
  discord:
    enabled: true
    token_env: "DISCORD_BOT_TOKEN"
    allowed_users: []                 # empty = all users; use numeric Discord IDs
    home_channel: ~                   # channel ID for proactive messages
```

Additional optional env vars:

| Variable | Effect |
|----------|--------|
| `DISCORD_ALLOWED_USERS` | Comma-separated Discord user IDs |
| `DISCORD_HOME_CHANNEL` | Default home channel ID |

---

## Usage

Mention the bot or send a DM to start a session. Each Discord channel or DM thread gets its own agent session.

**Approval flow:** When a command requires approval, EdgeCrab adds ✅/❌ reaction buttons to its message.

**Platform slash commands** (use as normal Discord slash commands if registered):

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

## Required Bot Permissions

| Permission | Reason |
|-----------|--------|
| `Send Messages` | Reply to users |
| `Read Message History` | Context for thread-based conversations |
| `Read Messages/View Channels` | Receive messages |
| `Message Content Intent` | (Privileged) receive message content |

---

## Pro Tips

- **Restrict to a single channel:** Set `DISCORD_HOME_CHANNEL` and in your Discord server, create a dedicated `#edgecrab` channel. Share only that channel invite link.
- **Use a separate bot account for each environment** (dev/prod). Same token = same bot = same session store.
- **Enable `allowed_users`** (use numeric Discord IDs, not usernames — IDs are stable when users rename themselves). Right-click a user in Discord → "Copy User ID" (enable Developer Mode in Settings first).
- **Long responses:** Discord has a 2000-character message limit. EdgeCrab auto-chunks longer responses. If responses look incomplete, check gateway logs — chunked messages arrive as sequential posts.
- **Threads:** Replies in a thread automatically inherit the parent channel's session context.

---

## Troubleshooting

**Bot online but not responding:**
- Check that `Message Content Intent` is enabled in the Developer Portal under `Bot` → `Privileged Gateway Intents`
- Confirm `DISCORD_ALLOWED_USERS` includes your numeric user ID (or is empty for no restriction)
- Verify the bot has `View Channel` + `Read Message History` permissions in the specific channel

**`PermissionError: Missing Access` in logs:**
- Bot is missing `Send Messages` permission in that channel. Edit channel permissions and add the bot role.

**Bot not receiving DMs:**
- The user and bot must share at least one server. Bot DMs only work when they've interacted in a mutual server first.

**Token error on startup:**
```bash
# Verify token still valid:
curl -H "Authorization: Bot $DISCORD_BOT_TOKEN" https://discord.com/api/v10/users/@me
# If 401, regenerate token in Developer Portal: Bot tab -> Reset Token
```

---

## FAQ

**Q: How do I find my numeric Discord user ID?**  
Enable Developer Mode: *User Settings → App Settings → Advanced → Developer Mode*. Then right-click your username anywhere and select *Copy User ID*.

**Q: Can I run multiple Discord bots with one EdgeCrab?**  
Not in a single instance. Each EdgeCrab process supports one Discord bot token. Run multiple EdgeCrab gateway instances with separate configurations for multiple bots.

**Q: The bot ignores me in a server but works in DMs (or vice versa).**  
DMs and server channels are separate context sources. Check `DISCORD_ALLOWED_USERS` — an empty list allows everyone; a populated list restricts globally (DMs and servers).

**Q: Does EdgeCrab work in Discord forums or threads?**  
Forums/threads are treated as separate channels, so each thread gets its own independent session.

**Q: How do I approve/deny a security confirmation?**  
When EdgeCrab's security policy requires approval, it replies with two reaction buttons (✅ / ❌). Click the reaction in Discord to approve or deny.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) — Multi-platform setup
- [Cron Jobs](/features/cron/) — Scheduling tasks that post to Discord
- [Security Model](/user-guide/security/) — Approval workflow and allowed users
- [Profiles](/user-guide/profiles/) — Per-profile gateway configuration
