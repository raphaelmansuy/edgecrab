# Gateway Architecture

Verified against:
- `crates/edgecrab-gateway/src/lib.rs`
- `crates/edgecrab-gateway/src/platform.rs`
- `crates/edgecrab-gateway/src/run.rs`
- `crates/edgecrab-gateway/src/session.rs`

The gateway is the chat-facing runtime. It normalizes inbound platform events into `IncomingMessage`, runs the shared agent runtime, and routes replies back through the correct adapter.

## Main loop

```text
+-----------------------------+
| platform adapter            |
+-----------------------------+
               |
               v
+-----------------------------+
| IncomingMessage             |
+-----------------------------+
               |
               v
+-----------------------------+
| SessionManager resolves     |
| the session slot            |
+-----------------------------+
               |
               v
+-----------------------------+
| GatewayEventProcessor       |
| runs the agent              |
+-----------------------------+
               |
               v
+-----------------------------+
| DeliveryRouter sends        |
| streaming or final output   |
+-----------------------------+
```

## Current adapter modules

- `telegram`
- `discord`
- `slack`
- `whatsapp`
- `signal`
- `email`
- `sms`
- `matrix`
- `mattermost`
- `dingtalk`
- `feishu`
- `wecom`
- `homeassistant`
- `webhook`
- `api_server`

## Shared gateway services

- `session.rs`: per-user or per-chat session mapping
- `delivery.rs`: outbound routing and platform-aware delivery
- `stream_consumer.rs`: progressive token delivery where supported
- `channel_directory.rs`: reachable-channel lookup
- `pairing.rs`: code-based approval flow for new users
- `mirror.rs`: cross-platform session mirroring
- `hooks.rs`: native and scriptable hook execution
- `attachment_cache.rs`: media caching support

## Message model

`PlatformAdapter` standardizes:

- `start(tx)` for inbound events
- `send(msg)` for outbound delivery
- response formatting
- capability flags such as markdown support, images, files, and message editing

## Behavior worth knowing

- The gateway can pre-analyze a small number of attached images before the model turn.
- Streaming edits are controlled by gateway config, not hardcoded per adapter.
- Session cleanup is time-based through `SessionManager`.
- Editing support is optional per platform; non-editable platforms fall back to final-message sends.
