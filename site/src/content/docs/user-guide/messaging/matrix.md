---
title: Matrix Setup
description: Connect EdgeCrab to a Matrix homeserver via the Client-Server API. Grounded in crates/edgecrab-gateway/src/matrix.rs.
sidebar:
  order: 6
---

The Matrix adapter connects to any Matrix homeserver using the Client-Server REST API. It uses long-poll sync for receiving messages and REST for sending replies.

**Max message length**: 4000 characters (auto-chunked for longer messages).

---

## Prerequisites

1. A Matrix homeserver (matrix.org, self-hosted Synapse/Dendrite, etc.)
2. A bot account with a long-lived access token
3. The bot invited to at least one room

---

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `MATRIX_HOMESERVER` | **Yes** | Homeserver URL (e.g. `https://matrix.org`) |
| `MATRIX_ACCESS_TOKEN` | **Yes** | Access token (from `/_matrix/client/v3/login`) |
| `MATRIX_USER_ID` | No | Full user ID (e.g. `@edgecrab:matrix.org`) |
| `MATRIX_ALLOWED_USERS` | No | Comma-separated Matrix user IDs allowed to chat |

---

## Setup

### 1. Create a Bot Account

Using the Matrix client API or any Matrix client, register a dedicated bot account. Retrieve its access token:

```bash
curl -X POST https://matrix.org/_matrix/client/v3/login \
  -H 'Content-Type: application/json' \
  -d '{"type":"m.login.password","user":"edgecrab-bot","password":"<password>"}'
# Returns: {"access_token": "syt_xxx..."}
```

### 2. Set Environment Variables

```bash
# ~/.edgecrab/.env
MATRIX_HOMESERVER=https://matrix.org
MATRIX_ACCESS_TOKEN=syt_xxx...
MATRIX_USER_ID=@edgecrab-bot:matrix.org
MATRIX_ALLOWED_USERS=@you:matrix.org,@teammate:matrix.org
```

### 3. Start the Gateway

```bash
edgecrab gateway start
```

The adapter auto-detects `MATRIX_HOMESERVER` and `MATRIX_ACCESS_TOKEN` and connects automatically.

---

## Configuration in config.yaml

Matrix adapter settings are read from environment variables only — there is no `gateway.matrix` config section.

---

## Usage

1. Invite the bot to a room: `/invite @edgecrab-bot:matrix.org`
2. Send a message in the room — the bot responds in the same room
3. The bot maintains one EdgeCrab session per room (not per user)

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Bot doesn't join room | Not invited | `/invite @botname:server` in the room |
| Auth errors | Token expired | Re-authenticate and update `MATRIX_ACCESS_TOKEN` |
| Bot sees old messages | Sync from past | Bot only processes messages after startup |

See [Messaging Gateway](/user-guide/messaging/) for gateway-level config.\n\n---\n\n## Pro Tips\n\n- **Use a dedicated bot account** rather than your personal Matrix account. A separate account has a distinct display name, making it obvious in rooms which messages are from the agent.\n- **Long-lived access tokens vs. passwords:** Prefer using long-lived tokens from `/_matrix/client/v3/login` rather than embedding passwords. Tokens don't reveal your password if leaked and can be invalidated individually.\n- **Room-per-project pattern:** Create a separate Matrix room for each project or context (e.g. `#my-app-dev:matrix.org`). EdgeCrab maintains one session per room, so each project has independent memory and history.\n- **E2E encryption rooms:** The current Matrix adapter does not support encrypted rooms (e2e). Create rooms without encryption (`m.room.join_rules: public` or invite-only without encryption) for the bot.\n- **Verify the bot's device:** For rooms with other members, you may be prompted to verify the bot's Matrix device. Either verify it or disable E2E encryption for the room.\n\n---\n\n## FAQ\n\n**Q: Can I use a matrix.org account or do I need my own homeserver?**  \nYou can use any Matrix homeserver including matrix.org, element.io, or your own Synapse/Dendrite server. The adapter only needs the `MATRIX_HOMESERVER` URL and an access token.\n\n**Q: How do I get a long-lived access token without using curl?**  \nIn Element Web: *Settings \u2192 Help & About \u2192 Access Token* (scroll to bottom of the page). Copy the token shown there.\n\n**Q: The bot joins a room but doesn't respond to messages.**  \nCheck `MATRIX_ALLOWED_USERS`. If set, only those Matrix user IDs can trigger responses. Also verify the bot account has joined the room (not just been invited).\n\n**Q: Can the bot be in multiple rooms at once?**  \nYes \u2014 invite the bot to as many rooms as you want. It maintains separate sessions per room.\n\n**Q: Does EdgeCrab support Matrix reactions or threads?**  \nNot currently. EdgeCrab sends plain text replies (auto-chunked if >4000 chars) and does not interpret reaction events.\n\n---\n\n## See Also\n\n- [Messaging Gateway Overview](/user-guide/messaging/) \u2014 Multi-platform routing\n- [Security Model](/user-guide/security/) \u2014 Access control and allowed users\n- [Sessions](/user-guide/sessions/) \u2014 One session per room
