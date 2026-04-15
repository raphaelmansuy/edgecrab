---
title: iMessage (BlueBubbles) Setup
description: Connect EdgeCrab to iMessage via the BlueBubbles server. Grounded in crates/edgecrab-gateway/src/bluebubbles.rs.
sidebar:
  order: 13
---

The BlueBubbles adapter connects EdgeCrab to Apple iMessage through a [BlueBubbles](https://bluebubbles.app/) server running on a Mac. BlueBubbles acts as a REST + webhook bridge to Apple's private messaging APIs.

---

## Prerequisites

1. A Mac running BlueBubbles server (requires macOS and a logged-in Apple ID)
2. BlueBubbles server accessible on your network (or via Ngrok/Cloudflare tunnel)
3. A server password configured in BlueBubbles settings

---

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `BLUEBUBBLES_SERVER_URL` | **Yes** | BlueBubbles server URL (e.g. `http://192.168.1.50:1234`) |
| `BLUEBUBBLES_PASSWORD` | **Yes** | Server password from BlueBubbles settings |
| `BLUEBUBBLES_WEBHOOK_HOST` | No | Public host for webhook callbacks |
| `BLUEBUBBLES_WEBHOOK_PORT` | No | Port for webhook listener (default: auto) |

---

## Gateway Configuration

```yaml
# ~/.edgecrab/config.yaml
gateway:
  platforms:
    bluebubbles:
      enabled: true
```

```bash
# Start with BlueBubbles
BLUEBUBBLES_SERVER_URL=http://192.168.1.50:1234 \
BLUEBUBBLES_PASSWORD=your-password \
edgecrab gateway start
```

---

## How It Works

```
iMessage → Mac (Messages.app) → BlueBubbles Server → Webhook → EdgeCrab Gateway → Agent
                                                                        ↓
iMessage ← Mac (Messages.app) ← BlueBubbles REST API ←────── EdgeCrab Reply
```

1. BlueBubbles monitors iMessage on a Mac and exposes a REST API
2. EdgeCrab registers a webhook with BlueBubbles for incoming messages
3. When a message arrives, BlueBubbles posts it to EdgeCrab's webhook endpoint
4. EdgeCrab processes the message through the agent loop
5. The reply is sent back via BlueBubbles REST API

---

## Features

- Text messaging (send and receive)
- Group chat support
- Media attachments (images, files) — inbound attachments auto-downloaded via BB API
- Typing indicators (with Private API)
- Read receipts (with Private API)
- Private API auto-detection at startup
- Crash-recovery webhook deduplication
- Improved markdown stripping (code fences, links, italic, bold, strikethrough)

---

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Connection refused | Verify `BLUEBUBBLES_SERVER_URL` is reachable from EdgeCrab host |
| Authentication failed | Check `BLUEBUBBLES_PASSWORD` matches BlueBubbles server settings |
| No messages received | Ensure webhook is registered — check BlueBubbles server logs |
| Duplicate messages | Webhook dedup handles this automatically on restart |
