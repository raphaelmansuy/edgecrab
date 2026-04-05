---
title: WhatsApp
description: Connect EdgeCrab to WhatsApp via a local WA bridge. Grounded in crates/edgecrab-gateway/src/whatsapp.rs.
sidebar:
  order: 5
---

## Prerequisites

WhatsApp integration uses a local bridge (whatsapp-web.js or baileys) that EdgeCrab can install automatically.

---

## Configuration

### Environment Variable (Quick Start)

```bash
export WHATSAPP_ENABLED=1
edgecrab gateway start
```

EdgeCrab will auto-install bridge dependencies on first run if `whatsapp.install_dependencies: true` (default).

### config.yaml

```yaml
gateway:
  whatsapp:
    enabled: false               # set true or use WHATSAPP_ENABLED=1
    bridge_port: 3000            # local bridge HTTP port
    bridge_url: ~                # custom bridge URL (overrides port)
    mode: "self-chat"            # "self-chat" | "any-sender"
    allowed_users: []            # phone numbers (include country code)
    reply_prefix: "⚕ *EdgeCrab Agent*\n------------\n"
    install_dependencies: true   # auto-install bridge on start
```

Additional optional env vars:

| Variable | Effect |
|----------|--------|
| `WHATSAPP_MODE` | Bridge mode (`self-chat` or `any-sender`) |
| `WHATSAPP_ALLOWED_USERS` | Comma-separated phone numbers |
| `WHATSAPP_BRIDGE_PORT` | Override bridge port (default: 3000) |
| `WHATSAPP_BRIDGE_URL` | Override full bridge URL |
| `WHATSAPP_SESSION_PATH` | Path to bridge session storage |
| `WHATSAPP_REPLY_PREFIX` | Text prepended to all replies |

---

## Modes

| Mode | Description |
|------|-------------|
| `self-chat` | Only responds to messages from the linked phone number (safe for personal use) |
| `any-sender` | Responds to messages from any sender in `allowed_users` |

---

## First Run

Pairing uses the dedicated `whatsapp` subcommand, not the gateway start command:

```bash
# Step 1: Run the pairing wizard (shows QR code, installs bridge)
edgecrab whatsapp
# → Follow prompts, scan the QR code with WhatsApp on your phone

# Step 2: Start the gateway (WhatsApp auto-enables via WHATSAPP_ENABLED=1)
edgecrab gateway start
```

Scan the QR code in the wizard with the WhatsApp app: *Linked Devices → Link a device*. The session is persisted to `WHATSAPP_SESSION_PATH` and survives restarts.

---

## Pro Tips

- **Run in headless mode on a server:** Run the pairing wizard `edgecrab whatsapp` from an interactive SSH session to scan the QR code. Once paired, start the gateway `edgecrab gateway start` in a tmux/screen session and detach. The session persists.
- **`self-chat` mode is safest for personal use.** Only messages from your own linked number are processed \u2014 nobody else can interact with your agent.
- **Persist the session:** Set `WHATSAPP_SESSION_PATH` to a persistent directory (e.g. `~/.edgecrab/whatsapp-session`). Without this, you'll need to re-scan the QR code on every restart.
- **Avoid using WhatsApp Web simultaneously.** WhatsApp only allows *one* active Linked Device web session at a time. If you open WhatsApp Web in a browser while EdgeCrab is running, it may disconnect the bridge.
- **`WHATSAPP_REPLY_PREFIX`** helps visually distinguish agent replies from human messages in a chat \u2014 useful if others share the conversation.

---

## Troubleshooting

**QR code not appearing:**  
Run with `RUST_LOG=debug` to see bridge output:
```bash
RUST_LOG=debug edgecrab whatsapp
```
Look for `[whatsapp]` prefixed log lines.

**QR code expired before scanning:**  
Press Ctrl+C and restart \u2014 a fresh QR is generated each time. You have ~60 seconds to scan.

**"Session disconnected" after a day:**  
WhatsApp limits Linked Device sessions. If you don't use the device for several days, it may be unlinked. Re-scan the QR to re-link. Store session data in `WHATSAPP_SESSION_PATH` to make re-links faster.

**Bridge dependencies fail to install:**  
```bash
# Manual install
cd ~/.edgecrab/whatsapp-bridge
npm install
```
Requires Node.js 16+. Check with `node --version`.

**Messages from other senders arriving in `self-chat` mode:**  
This is expected \u2014 `self-chat` means only messages originating *from your own linked number* are dispatched to EdgeCrab. Messages from others are silently ignored.

---

## FAQ

**Q: Will my WhatsApp contacts know I'm using a bot?**  
No \u2014 the bridge acts as a Linked Device, indistinguishable from WhatsApp Web. Your contacts see messages from your number as normal.

**Q: Is WhatsApp integration officially supported by Meta?**  
No \u2014 this uses an unofficial bridge library. Use at your own risk. Meta's Terms of Service prohibit automated messaging at scale. This is designed for personal-use agent access, not bulk messaging.

**Q: Can I use the WhatsApp Business API instead?**  
Not currently. EdgeCrab's WhatsApp adapter targets the consumer WhatsApp protocol via Linked Devices. Business API support is planned.

**Q: What happens if I lose my phone?**  
The bridge session is tied to your WhatsApp account, not your phone. As long as your WhatsApp account is active, you can re-link on a new phone and re-scan the QR code.

**Q: Can multiple people use EdgeCrab via WhatsApp?**  
Yes \u2014 set `mode: any-sender` and `allowed_users` to permit multiple phone numbers. Each sender gets their own session.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) \u2014 Multi-platform setup
- [Security Model](/user-guide/security/) \u2014 Access control and approval workflow
- [Self-Hosting Guide](/guides/self-hosting/) \u2014 Running EdgeCrab 24/7 with session persistence
- [Sessions](/user-guide/sessions/) \u2014 Managing persistent conversation sessions
