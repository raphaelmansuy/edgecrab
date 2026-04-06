---
title: Matrix Setup
description: Connect EdgeCrab to a Matrix homeserver via the Client-Server API. Grounded in crates/edgecrab-gateway/src/matrix.rs.
sidebar:
  order: 6
---

The Matrix adapter connects to any Matrix homeserver using the Client-Server REST API (`/_matrix/client/v3`). It uses long-poll sync (30-second timeout) for receiving messages and REST for sending replies.

**Max message length**: 4000 characters — longer responses are auto-chunked.

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

Register a dedicated bot account, then retrieve its access token:

```bash
curl -X POST https://matrix.org/_matrix/client/v3/login \
  -H 'Content-Type: application/json' \
  -d '{"type":"m.login.password","user":"edgecrab-bot","password":"<password>"}'
# Returns: {"access_token": "syt_xxx..."}
```

Or in Element Web: **Settings → Help & About → Access Token** (scroll to the bottom).

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

## Supported Media Types

The Matrix adapter supports these file types for upload and receive:
`png`, `jpg/jpeg`, `gif`, `webp`, `svg`, `bmp`, `pdf`, `txt`, `md`, `csv`, `json`, `zip`, `doc`, `docx`, `xls`, `xlsx`, `ppt`, `pptx`. Any other type falls back to `application/octet-stream`.

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Bot doesn't join room | Not invited | `/invite @botname:server` in the room |
| Auth errors | Token expired | Re-authenticate and update `MATRIX_ACCESS_TOKEN` |
| Bot sees old messages | Sync from past | Bot only processes messages after startup |
| Responses truncated | Message > 4000 chars | EdgeCrab auto-chunks — check all reply segments |
| File upload fails | Media server issue | Verify `MATRIX_HOMESERVER` allows media uploads |

---

## Pro Tips

- **Use a dedicated bot account** — gives it a distinct display name so it's obvious in rooms.
- **Long-lived access tokens** — use tokens from `/_matrix/client/v3/login` rather than embedding passwords. Tokens can be invalidated individually without exposing your password.
- **Room-per-project pattern** — EdgeCrab maintains one session per room, so each project gets independent memory and history. Create `#app-dev:matrix.org`, `#ops:matrix.org`, etc.
- **E2E encryption rooms** — The current adapter does not support end-to-end encrypted rooms. Create rooms without encryption enabled.

---

## FAQ

**Q: Can I use matrix.org or do I need my own homeserver?**
Any Matrix homeserver works — matrix.org, element.io, or your own Synapse/Dendrite. Only `MATRIX_HOMESERVER` and an access token are needed.

**Q: The bot joins a room but doesn't respond.**
Check `MATRIX_ALLOWED_USERS`. If set, only listed Matrix user IDs can trigger responses. Also verify the bot has actually joined (accepted the invite), not just been invited.

**Q: Can the bot be in multiple rooms at once?**
Yes — invite the bot to as many rooms as you want. It maintains separate sessions per room.

**Q: Does EdgeCrab support Matrix reactions or threads?**
Not currently. EdgeCrab sends plain text replies (auto-chunked at 4000 chars) and does not interpret reaction or thread events.

**Q: How long before the bot reconnects after a network drop?**
The adapter uses a 30-second sync timeout. If the homeserver doesn't respond, it retries with exponential backoff: 5s → 10s → 20s → … → 120s cap.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) — Multi-platform routing
- [Security Model](/user-guide/security/) — Access control and allowed users
- [Sessions](/user-guide/sessions/) — One session per room
