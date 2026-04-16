---
title: WeChat (Weixin) Setup
description: Connect EdgeCrab to WeChat via the iLink Bot API. Grounded in crates/edgecrab-gateway/src/weixin.rs.
sidebar:
  order: 14
---

The Weixin adapter connects EdgeCrab to WeChat (微信) through the iLink Bot API, which provides a POST-based long-polling interface with persistent sync buffer for reliable message ordering across restarts.

---

## Prerequisites

1. An iLink Bot account with API access
2. A configured bot token from the iLink Bot dashboard
3. Your WeChat account ID

---

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `WEIXIN_TOKEN` | **Yes** | iLink Bot API token |
| `WEIXIN_ACCOUNT_ID` | **Yes** | Your WeChat account identifier |
| `WEIXIN_BASE_URL` | No | Custom API base URL (default: iLink Bot API) |
| `WEIXIN_CDN_BASE_URL` | No | CDN endpoint for media upload/download |
| `WEIXIN_ALLOWED_USERS` | No | Comma-separated list of allowed user IDs |
| `WEIXIN_DM_POLICY` | No | DM access policy: `open` or `allowlist` |
| `WEIXIN_GROUP_POLICY` | No | Group access policy: `open` or `allowlist` |

---

## Gateway Configuration

```yaml
# ~/.edgecrab/config.yaml
gateway:
  platforms:
    weixin:
      enabled: true
```

```bash
# Start with WeChat
WEIXIN_TOKEN=your-bot-token \
WEIXIN_ACCOUNT_ID=your-account-id \
edgecrab gateway start
```

---

## How It Works

```
WeChat User → WeChat Server → iLink Bot API → POST long-poll → EdgeCrab Gateway → Agent
                                                                        ↓
WeChat User ← WeChat Server ← iLink Bot API ←──────────── EdgeCrab Reply
```

1. EdgeCrab POST-polls the iLink Bot API with a sync buffer for reliable message ordering
2. When messages arrive, media items (images, voice, video, files) are downloaded from CDN and decrypted via AES-128-ECB
3. Messages are dispatched to the agent loop with text and attachment metadata
4. The agent processes the message and generates a reply
5. Outbound media is AES-128-ECB encrypted and uploaded to CDN before sending
6. If the session expires (errcode -14), the adapter auto-recovers without losing state

---

## Security

- **AES-128-ECB media encryption**: Media files are encrypted/decrypted for CDN transport
- **AES-256-CBC XML encryption**: WeCom XML payloads use AES-256-CBC encryption
- **User allowlist**: Set `WEIXIN_ALLOWED_USERS` to restrict which WeChat users can interact with the bot
- **Token authentication**: All API calls are authenticated with the bot token
- **Session-expired recovery**: Detects errcode -14 and re-authenticates automatically

---

## Features

- Text messaging (send and receive)
- Image, voice, video, and file attachments (send and receive)
- AES-128-ECB encrypted CDN media pipeline (upload + download)
- POST-based polling with persistent sync buffer
- User allowlist filtering (DM and group policies)
- Automatic message deduplication
- Session-expired auto-recovery
- Context token echo for conversation threading
- Markdown reformatting for WeChat's text-only display
- Typing indicator support via ticket-based API

---

## Troubleshooting

| Issue | Fix |
|-------|-----|
| No messages received | Verify `WEIXIN_TOKEN` and `WEIXIN_ACCOUNT_ID` are correct |
| Unauthorized users | Add user IDs to `WEIXIN_ALLOWED_USERS` |
| Connection timeouts | Check network connectivity to iLink Bot API |
| Media download fails | Verify `WEIXIN_CDN_BASE_URL` is reachable |
| Session expired errors | Normal — adapter auto-recovers via re-authentication |
