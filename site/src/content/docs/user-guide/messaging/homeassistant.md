---
title: Home Assistant Setup
description: Connect EdgeCrab to Home Assistant via WebSocket and REST API for smart home automation. Grounded in crates/edgecrab-gateway/src/homeassistant.rs.
sidebar:
  order: 11
---

The Home Assistant adapter connects EdgeCrab to your Home Assistant instance via WebSocket for conversation events and REST API for responses. This enables natural language control of your smart home.

**Max message length**: 10,000 characters.

---

## Prerequisites

1. A running [Home Assistant](https://www.home-assistant.io/) instance (Core, OS, or Container)
2. A long-lived access token from Home Assistant

---

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `HA_URL` | **Yes** | Home Assistant URL (e.g. `http://homeassistant.local:8123`) |
| `HA_TOKEN` | **Yes** | Long-lived access token |
| `HA_ALLOWED_USERS` | No | Comma-separated HA user IDs allowed to chat |

---

## Setup

### 1. Create a Long-Lived Access Token

1. In Home Assistant, go to **Profile** (your user icon in the sidebar)
2. Scroll to **Long-Lived Access Tokens**
3. Click **Create Token**, name it `edgecrab`, and copy the token

### 2. Set Environment Variables

```bash
# ~/.edgecrab/.env
HA_URL=http://homeassistant.local:8123
HA_TOKEN=eyJhbGciOiJIUzI1NiIs...
```

### 3. Start the Gateway

```bash
edgecrab gateway start
```

---

## Home Assistant Tools

When `HA_URL` and `HA_TOKEN` are set, EdgeCrab automatically enables Home Assistant tools in the agent:

| Tool | Description |
|------|-------------|
| `ha_list_entities` | List all entities (lights, switches, sensors, etc.) |
| `ha_get_state` | Get current state and attributes of an entity |
| `ha_list_services` | List available services (domains and service names) |
| `ha_call_service` | Call a Home Assistant service (e.g. turn on a light) |

These tools are available in any EdgeCrab session (not just the gateway), as long as `HA_URL` and `HA_TOKEN` are set.

### Example Interactions

```
Turn on the kitchen lights
Check if the front door is locked
What's the temperature in the bedroom?
Set the living room thermostat to 72 degrees
```

---

## Gateway Integration

The HA adapter listens for `conversation_chat` events over the HA WebSocket API and responds via the REST API.

To use it as a conversation agent in Home Assistant:

1. In HA, go to **Settings → Voice Assistants → Add Assistant**
2. Select **EdgeCrab** as the conversation agent (requires the companion HACS integration)

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `401 Unauthorized` | Token expired or wrong | Create a new Long-Lived Access Token |
| WebSocket errors | HA URL incorrect | Verify `HA_URL` (include port 8123) |
| Tools not available | Missing env vars | Set both `HA_URL` and `HA_TOKEN` |

---

## Pro Tips

- **HA tools work outside the gateway too.** Because `HA_URL` and `HA_TOKEN` gate the tools at the agent level, you can control your smart home from the CLI TUI without starting the gateway at all. Just set both env vars and run `edgecrab`.
- **Use specific entity IDs in prompts.** Instead of "turn on the lights", say "turn on `light.kitchen_overhead`" for precise control. Ask the agent to list entities first: "list lights in the kitchen".
- **Security — scope your HA token:** The Long-Lived Access Token grants full HA admin access. Create a separate HA user account with restricted permissions for the EdgeCrab token if you're concerned about blast radius.
- **Voice assistant integration:** The gateway HA adapter + `edgecrab tui` in voice mode creates a powerful local voice assistant pipeline entirely within your home network.
- **Test tools without the gateway:**
  ```bash
  HA_URL=http://homeassistant.local:8123 \
  HA_TOKEN=eyJ... \
  edgecrab --quiet "list all lights and their current states"
  ```

---

## FAQ

**Q: Does EdgeCrab replace the built-in HA conversation agent?**  
As an optional alternative. When configured as a Voice Assistant conversation agent, EdgeCrab handles requests instead of the built-in LLM. Both can coexist — create separate voice assistants in HA settings.

**Q: Can EdgeCrab control entities on a remote HA instance (not local)?**  
Yes — any HA instance reachable via `HA_URL` works. Set `HA_URL` to the external URL (e.g. `https://your-ha.duckdns.org`) and a valid token. The same tools apply.

**Q: The `ha_call_service` tool returns success but nothing happens.**  
Verify the entity ID and service name. Common mistakes: `light.turn_on` should be domain=`light`, service=`turn_on`, entity_id=`light.kitchen`. Use `ha_list_entities` to confirm the exact entity ID.

**Q: Does EdgeCrab see HA state changes in real-time?**  
Not proactively. EdgeCrab calls `ha_get_state` on demand. For real-time monitoring, use cron jobs to periodically check states and send alerts.

**Q: Is the HA HACS integration required?**  
Only for the "conversation agent" integration in HA's voice assistant settings. The `ha_*` tools work without HACS — you just need `HA_URL` and `HA_TOKEN`.

---

## See Also

- [Messaging Gateway Overview](/user-guide/messaging/) — Multi-platform routing
- [Features Overview](/features/overview/) — Full Home Assistant tools list
- [Cron Jobs](/features/cron/) — Scheduled HA state checks and alerts
- [Security Model](/user-guide/security/) — Token scoping and access control
