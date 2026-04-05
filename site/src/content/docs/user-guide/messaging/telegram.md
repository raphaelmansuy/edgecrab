---
title: Telegram
description: Connect EdgeCrab to Telegram. Step-by-step bot creation, configuration, security, and troubleshooting. Grounded in crates/edgecrab-gateway/src/telegram.rs.
sidebar:
  order: 1
---

Chat with EdgeCrab from anywhere on your phone via Telegram. One bot, one token, and EdgeCrab answers all your messages with the full power of the agent loop including tools, memory, and skills.

---

## Step 1 — Create a Telegram Bot

1. Open Telegram and search for `@BotFather`
2. Send `/newbot`
3. Enter a name (display name), then a username (must end in `bot`, e.g. `my_edgecrab_bot`)
4. Copy the token: `1234567890:AAF...` — you'll need this

**For group chats:** In BotFather, send `/mybots` → select your bot → *Bot Settings* → *Group Privacy* → **Disable** (so the bot receives all messages, not just commands).

---

## Step 2 — Get Your Chat ID

For personal use, you need your own chat ID so you can restrict access:

1. Message `@userinfobot` to get your user ID
2. Or use `@getmyid_bot`

For a group chat: Add the bot to the group, then run:
```bash
curl https://api.telegram.org/bot<TOKEN>/getUpdates
```
Look for `"chat":{"id":` in the response.

---

## Step 3 — Configure EdgeCrab

### Environment Variable (Quickest)

```bash
export TELEGRAM_BOT_TOKEN=1234567890:AAF...
edgecrab gateway start
```

Setting `TELEGRAM_BOT_TOKEN` auto-enables Telegram on startup.

### Persistent Setup (Recommended)

Add to `~/.edgecrab/.env`:
```bash
TELEGRAM_BOT_TOKEN=1234567890:AAF...
TELEGRAM_ALLOWED_USERS=your_username   # restrict access
```

Or in `config.yaml`:
```yaml
gateway:
  telegram:
    enabled: true
    token_env: "TELEGRAM_BOT_TOKEN"
    allowed_users:
      - your_username          # Telegram usernames (no @)
    home_channel: ~            # chat ID for proactive messages (optional)
```

### All Env Vars

| Variable | Effect |
|----------|--------|
| `TELEGRAM_BOT_TOKEN` | Bot token (required) — auto-enables Telegram when set |
| `TELEGRAM_ALLOWED_USERS` | Comma-separated allowed usernames (no @) |
| `TELEGRAM_HOME_CHANNEL` | Default home channel chat ID for proactive messages |

---

## Step 4 — Start the Gateway

```bash
edgecrab gateway start
```

Check status:
```bash
edgecrab gateway status
# Expected: telegram: connected
```

Send your first message to the bot on Telegram!

---

## Usage

**Starting a session:** Send any message to the bot. A new agent session is created per Telegram chat — group chats and private chats have separate session contexts.

**Platform slash commands** (sent as Telegram messages):

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

**Multi-line messages:** Telegram sends messages line by line. If you need to send a multi-line prompt, use a code block in Telegram or send the text as a file attachment.

**File attachments:** Send a file and EdgeCrab sees the file content (for text files) or the file path (for binaries). Useful for code reviews.

**Inline approval buttons:** When `security.approval_required` matches a command, EdgeCrab sends an inline keyboard with ✅ Approve / ❌ Deny buttons directly in the chat.

---

## Proactive Messages and Cron

Set a `home_channel` to receive cron job results and background task completions:

```yaml
gateway:
  telegram:
    home_channel: "123456789"   # your chat ID
```

Then cron results, background task completions, and explicit `/sethome` messages all go to this channel. You can use EdgeCrab as a personal monitoring bot that sends you daily summaries, alerts, and reports.

---

## Security Notes

- **Always set `allowed_users`.** Without it, anyone who discovers your bot username can interact with your agent and all its tools.
- **Never put your bot token in `config.yaml`** — that file might be committed to git. Use `.env` or an environment variable.
- **Private chats are more secure than groups.** In group chats, any member can send commands.
- Set `privacy mode` to **off** in BotFather only if you need the bot to see all messages in a group (not just commands). Otherwise, use the `/command@botname` syntax in groups.

---

## Troubleshooting

**Bot doesn't respond:**
- Check `edgecrab gateway status` — is Telegram showing as connected?
- Verify the token: `curl https://api.telegram.org/bot<TOKEN>/getMe`
- Ensure the bot is not blocked by `allowed_users` (try sending from the allowed username)

**"Unauthorized" in gateway logs:**
- Token is invalid or revoked. Create a new token with BotFather: `/mybots` → select bot → API Token → Revoke current token + create new.

**Bot is in a group but doesn't see messages:**
- Disable privacy mode in BotFather: `/mybots` → Bot Settings → Group Privacy → Disable
- Or prefix commands with `/command@your_bot_username`

**Message chunking:** Telegram has a 4096-character message limit. EdgeCrab auto-chunks long responses. If a response seems cut off, it should continue automatically.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) — Multi-platform setup
- [Cron Jobs](/features/cron/) — Scheduling tasks that post to Telegram
- [Security Model](/user-guide/security/) — Approval workflow and allowed users
- [Self-Hosting Guide](/guides/self-hosting/) — Run the gateway 24/7
