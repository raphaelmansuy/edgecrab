---
title: DingTalk Setup
description: Connect EdgeCrab to DingTalk (Alibaba Cloud enterprise messaging) via the DingTalk Open Platform API. Grounded in crates/edgecrab-gateway/src/dingtalk.rs.
sidebar:
  order: 8
---

The DingTalk adapter connects EdgeCrab to DingTalk via the DingTalk Open Platform v2 API. It uses a stream connection for receiving messages and REST for replies.

---

## Prerequisites

1. A DingTalk developer account at [open.dingtalk.com](https://open.dingtalk.com)
2. A registered DingTalk app with `Robot` permissions
3. `AppKey` and `AppSecret` from your app credentials

---

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `DINGTALK_APP_KEY` | **Yes** | DingTalk app AppKey |
| `DINGTALK_APP_SECRET` | **Yes** | DingTalk app AppSecret |
| `DINGTALK_ROBOT_CODE` | No | Robot code if using multiple robots |
| `DINGTALK_WEBHOOK_PORT` | No | Webhook port (default auto-assigned) |

---

## Setup

### 1. Create a DingTalk App

1. Go to [open.dingtalk.com](https://open.dingtalk.com) → **Application Development**
2. Create a new Internal Enterprise Application
3. Navigate to **Credentials and basic info** — copy `AppKey` and `AppSecret`
4. Under **Capabilities**, enable **Robot**
5. Configure the robot name and icon

### 2. Set Environment Variables

```bash
# ~/.edgecrab/.env
DINGTALK_APP_KEY=dingxxxxxxxxxxxxxxxx
DINGTALK_APP_SECRET=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

### 3. Start the Gateway

```bash
edgecrab gateway start
```

The adapter uses the DingTalk Stream SDK to receive messages without requiring a public webhook endpoint.

---

## Usage

1. In DingTalk, search for your robot by name and add it to a group or send a direct message
2. In a group: `@EdgeCrab your question here`
3. In a direct message: just type your message

**Gateway slash commands** (send as DingTalk messages):

| Command | Effect |
|---------|--------|
| `/help` | List all available gateway commands |
| `/new` | Start a fresh conversation (clears history) |
| `/reset` | Alias for `/new` |
| `/stop` | Cancel the currently running agent response |
| `/retry` | Re-send your last message |
| `/status` | Show whether the agent is running or idle |
| `/usage` | Show session stats |
| `/hooks` | List loaded event hooks |

---

## Notes

- DingTalk supports both group and direct message conversations
- Group messages require `@mentioning` the bot; DM messages do not
- The adapter auto-reconnects on disconnection with exponential backoff
- Max message length: 6,000 characters (auto-chunked for longer responses)

---

## Pro Tips

- **Stream SDK vs. public webhook:** The DingTalk Stream SDK used here doesn't need a public URL — it maintains an outbound stream connection. This makes it safe to run behind NAT/firewalls.
- **`AppKey` freshness:** DingTalk app credentials expire after inactivity in some configurations. If the bot stops responding after weeks, regenerate credentials in the DingTalk Open Platform console.
- **Group vs. DM sessions:** Group chats and DM chats are separate sessions. DM EdgeCrab for private experiments, use a group channel for team-shared tasks.
- **Enterprise deployment:** Use *Internal Enterprise Applications* (not ISV apps) for self-hosted use. Internal apps don't require public marketplace review.

---

## Troubleshooting

**Bot not visible in DingTalk search:**  
Publish the robot from the Open Platform console. Internal robots must be *published* even for internal use before they appear in search.

**Messages arrive but bot doesn't respond:**  
Verify `DINGTALK_APP_KEY` and `DINGTALK_APP_SECRET` match the *Credentials and basic info* page exactly. No extra spaces or newlines.

**Stream connection keeps dropping:**  
```bash
RUST_LOG=debug edgecrab gateway start
# Look for [dingtalk] connection/reconnect log lines
```
Exponential backoff means reconnects start at 1s and increase. Normal behavior on flaky networks.

---

## FAQ

**Q: Does EdgeCrab support DingTalk Mini Programs or Intelligent Workbench?**  
Not currently. The adapter uses the Robot/Stream API for conversational messages only.

**Q: Can I use this with DingTalk's international version?**  
Yes — DingTalk international (`dingtalk.com`) and the China version use the same Open Platform API. The Stream SDK connects to the same endpoint regardless of region.

**Q: How do I restrict which DingTalk users can chat with EdgeCrab?**  
Use `DINGTALK_ALLOWED_USERS` (comma-separated DingTalk user IDs). Find user IDs in the DingTalk admin console under *Address Book → Member Management*.

**Q: Does EdgeCrab support DingTalk message cards (JSON templates)?**  
Not currently — EdgeCrab sends plain text responses. Rich card support is planned.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) — Multi-platform routing
- [Security Model](/user-guide/security/) — Access control and approval workflow
- [Self-Hosting Guide](/guides/self-hosting/) — Running EdgeCrab 24/7
