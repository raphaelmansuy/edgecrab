# 006.001 — Gateway Architecture

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.001 System Architecture](../002_architecture/001_system_architecture.md) | [→ 009.001 Config & State](../009_config_state/001_config_state.md) | [→ 011.001 Security](../011_security/001_security.md)
> **Verified against source**: `crates/edgecrab-gateway/src/run.rs`, `config.rs`, `platform.rs`, `session.rs`, `delivery.rs`, `api_server.rs`
> **Intent**: turn the same EdgeCrab agent core into a multi-platform personal assistant without forking the runtime model.

---

## 1. Boot Sequence

`Gateway::run()` is the real orchestration entry point.
It does not invent a second agent stack; it wraps the same `Agent` with platform adapters, HTTP surfaces, and delivery routing.

```text
Gateway::run()
   |
   +--> create `mpsc::channel<IncomingMessage>(256)`
   +--> emit `gateway:startup` hook
   +--> build axum router (`/health`, `/webhook/incoming`)
   +--> bind the gateway HTTP listener
   +--> spawn each enabled platform adapter
   +--> start session cleanup ticker
   `--> enter dispatch loop
           |
           +--> auth guard
           +--> slash-command interception
           +--> queue / cancel logic per session
           +--> call `Agent`
           `--> route the reply back via `DeliveryRouter`
```

---

## 2. Message Flow

```text
Telegram / Discord / Slack / WhatsApp / Signal / ...
                     |
                     v
        `PlatformAdapter::start()` produces `IncomingMessage`
                     |
                     v
                `Gateway::run()`
                     |
         +-----------+-----------+
         |                       |
         | slash command?        | normal message
         |                       v
         |                 session guard
         |                 (`running_sessions`, `pending_messages`)
         |                       |
         |                       v
         |               `Agent::chat_with_origin()`
         |               or `chat_streaming_with_origin()`
         |                       |
         +----------> `DeliveryRouter::deliver()`
                                 |
                                 v
                        formatted platform response
```

The queue semantics are important and verified in `run.rs`:

- only **one active task** is allowed per session key
- if a new message arrives while the session is busy, the gateway **queues the latest message**
- after the current turn completes, the queued message is re-dispatched automatically

---

## 3. Actual Session Model

### 3.1 Session key

Gateway sessions are keyed by:

```text
(platform, user_id, optional channel_id)
```

That logic lives in `SessionKey` and prevents a single user’s DM, group, and channel conversations from collapsing into one transcript.

### 3.2 Session store

`SessionManager` uses:

- `DashMap<SessionKey, Arc<RwLock<GatewaySession>>>`
- `idle_timeout` for expiration
- `cleanup_expired()` for background eviction

### 3.3 Defaults from `GatewayConfig`

| Setting | Default |
|---|---|
| host | `127.0.0.1` |
| port | `8080` |
| session idle timeout | `3600s` |
| cleanup interval | `300s` |
| default model | `anthropic/claude-sonnet-4-20250514` |
| webhook enabled | `true` |

---

## 4. Built-in Command Interception

The gateway handles a small operational command set **before** the message reaches the model.
This is the part that keeps support and session control predictable in chat environments.

| Command | Effect in `run.rs` |
|---|---|
| `/help` | returns the built-in gateway help text |
| `/new`, `/reset` | clears the session and cancels any running task |
| `/stop` | cancels the current in-flight session task |
| `/retry` | re-injects the last user message |
| `/status` | reports whether the session is idle, running, or queued |
| `/usage` | prints lightweight session stats |

---

## 5. Platform Surface in This Repository

These adapters exist as real Rust modules today:

```text
telegram.rs      discord.rs      slack.rs        whatsapp.rs
signal.rs        matrix.rs       mattermost.rs   dingtalk.rs
sms.rs           email.rs        homeassistant.rs
webhook.rs       api_server.rs
```

### What the shared `PlatformAdapter` trait standardizes

- inbound boot via `start()`
- outbound delivery via `send()`
- formatting via `format_response()`
- capability flags:
  - `supports_markdown()`
  - `supports_images()`
  - `supports_files()`
  - `supports_editing()`
- optional higher-level helpers:
  - `edit_message()`
  - `send_status()`
  - `send_typing()`
  - `send_and_get_id()`
  - `send_photo()` / `send_document()`

This lets the gateway stay platform-agnostic while still taking advantage of richer adapters when available.

---

## 6. Streaming and Delivery

### 6.1 Gateway-side progressive streaming

When streaming is enabled and the adapter supports it, the gateway uses:

- `GatewayEventProcessor`
- `StreamConsumer`
- adapter-side `edit_message()` support

The defaults are explicit in `GatewayStreamingConfig`:

| Setting | Default |
|---|---|
| `enabled` | `true` |
| `edit_interval_ms` | `300` |
| `buffer_threshold` | `40` |
| `cursor` | ` ▉` |
| `tool_progress` | `true` |
| `show_reasoning` | `false` |

### 6.2 DeliveryRouter behaviour

`DeliveryRouter::deliver()`:

1. formats the text with the platform adapter
2. checks `max_message_length()`
3. splits oversized replies at **paragraph → newline → space → hard-cut** boundaries
4. sends chunks with a small pause to reduce rate-limit pressure

That logic lives in `delivery.rs` and is deliberately simple and robust.

---

## 7. Incoming Images and Outgoing Media

### 7.1 Incoming image handling

`platform.rs` defines a normalized `MessageAttachment` model.
When inbound attachments include local images, the gateway injects a standardized prompt block so the agent knows it must call `vision_analyze`.

```text
incoming attachment
   -> `MessageAttachment { kind: Image, local_path: ... }`
   -> `format_image_attachment_block()`
   -> user message is enriched before agent dispatch
```

### 7.2 Outgoing media tags

The gateway can also extract media directives from model text using:

- `[IMAGE:/path/to/file.png]`
- `[FILE:/path/to/report.pdf]`
- `[MEDIA:/path/to/anything]`

`extract_media_from_response()` strips those markers from the visible reply and then lets the adapter deliver the actual file or image if the platform supports it.

---

## 8. HTTP Surfaces: There Are Two

One of the easiest operational mistakes is forgetting that the gateway exposes **two different HTTP entry points**.

### 8.1 Gateway server (`run.rs`)

Bound from `GatewayConfig::bind_addr()` — default:

```text
127.0.0.1:8080
```

Endpoints:

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/health` | basic gateway health |
| `POST` | `/webhook/incoming` | queue inbound webhook payloads |

### 8.2 Optional OpenAI-compatible API server (`api_server.rs`)

Enabled only when:

```text
API_SERVER_ENABLED=true
```

Defaults:

| Variable | Default |
|---|---|
| `API_SERVER_HOST` | `127.0.0.1` |
| `API_SERVER_PORT` | `8642` |
| `API_SERVER_KEY` | unset |
| `API_SERVER_CORS_ORIGINS` | empty |

Endpoints implemented today:

| Method | Path |
|---|---|
| `POST` | `/v1/chat/completions` |
| `GET` | `/v1/models` |
| `POST` | `/v1/responses` |
| `GET` | `/v1/responses/{id}` |
| `DELETE` | `/v1/responses/{id}` |
| `GET` | `/v1/health` |
| `GET` | `/health` |

> **Important fact-check:** the API-server `stream=true` path currently emits an SSE stream built from the completed response text in `make_sse_stream()`. It is useful and compatible, but it is not the same thing as raw provider-token passthrough in every case.

---

## 9. Security Boundaries

### 9.1 Gateway user authorization

`run.rs` checks a small allowlist policy before dispatch:

- `GATEWAY_ALLOW_ALL_USERS`
- `GATEWAY_ALLOWED_USERS`
- `TELEGRAM_ALLOW_ALL_USERS`, `TELEGRAM_ALLOWED_USERS`
- `DISCORD_ALLOW_ALL_USERS`, `DISCORD_ALLOWED_USERS`

**Current behaviour:** if no allowlist env vars are configured at all, the gateway is effectively open. That is convenient for local single-user setups, but operators should lock it down for internet-facing deployments.

### 9.2 API server headers and CORS

`api_server.rs` adds:

```text
X-Content-Type-Options: nosniff
Referrer-Policy: no-referrer
```

CORS is **disabled by default**.
If `API_SERVER_CORS_ORIGINS` is provided, only those explicit origins are allowed; there is no wildcard fallback.

---

## 10. Hook Lifecycle

### 10.1 Two hook layers

| Layer | Registration | Overhead |
|-------|-------------|----------|
| Native Rust (`GatewayHook` trait) | `HookRegistry::register()` at boot | Zero — direct async call |
| File-based scripts (Python / JS / TS) | Auto-discovered from `~/.edgecrab/hooks/` | Subprocess spawn per event |

### 10.2 Event catalogue

| Event | Fires | Cancellable |
|-------|-------|-------------|
| `gateway:startup` | Gateway process starts | No |
| `session:start` | New session created | No |
| `session:end` | Session ended (before reset) | No |
| `session:reset` | User ran `/new` or `/reset` | No |
| `agent:start` | Agent begins processing a message | No |
| `agent:step` | Each iteration of the tool-call loop | No |
| `agent:end` | Agent finishes processing | No |
| `command:*` | Any slash command executed (wildcard) | No |
| `tool:pre` | Before any tool executes | **Yes** |
| `tool:post` | After any tool returns | No |
| `llm:pre` | Before LLM API request | **Yes** |
| `llm:post` | After LLM API response | No |
| `cli:start` | CLI session begins | No |
| `cli:end` | CLI session ends | No |
| `*` | Global wildcard — fires for every event | No |

Pre-hooks (`tool:pre`, `llm:pre`) support cancellation: return `HookResult::Cancel { reason }` to abort the pending operation.

### 10.3 Delivery path

```text
Agent / Gateway
    │
    ├─ emits StreamEvent::HookEvent { event, context_json }
    │
    └─▶ GatewayEventProcessor::handle()
            │
            └─▶ HookRegistry::emit() / emit_cancellable()
                    │
                    ├─▶ native Rust hook.handle(event, &ctx)    ← direct async call
                    │
                    └─▶ ScriptHook::execute()
                            │
                            ├─ spawn python3 / bun with context JSON on stdin
                            ├─ wait up to timeout_secs (default: 10s)
                            └─ parse stdout JSON → ScriptResponse { cancel, reason, extra }
```

The agent emits `StreamEvent::HookEvent` inside the tool-call loop. The gateway event processor receives these on its stream consumer and dispatches them to the registry synchronously (awaiting all hooks before continuing, unless the hook is marked fire-and-forget).

### 10.4 HOOK.yaml format

```yaml
# ~/.edgecrab/hooks/my_hook/HOOK.yaml
name: my_hook
description: Log every tool call to a remote audit endpoint
events:
  - tool:pre
  - tool:post
  - agent:end
timeout_secs: 5        # default: 10
priority: 10           # lower fires first; default: 50
enabled: true
env:
  AUDIT_URL: https://my-audit-server/events
  LOG_LEVEL: info
```

Handler must be one of: `handler.py`, `handler.js`, or `handler.ts`.

### 10.5 Example Python hook

```python
# ~/.edgecrab/hooks/audit_logger/handler.py
import json, sys, datetime, requests, os

ctx = json.load(sys.stdin)
event = ctx.get("event")
session_id = ctx.get("session_id")

payload = {
    "ts": datetime.datetime.utcnow().isoformat(),
    "event": event,
    "session": session_id,
    "tool": ctx.get("tool_name"),
}
requests.post(os.environ["AUDIT_URL"], json=payload, timeout=4)

# No output → HookResult::Continue (operation proceeds)
```

To cancel from a `tool:pre` hook:

```python
import json, sys
ctx = json.load(sys.stdin)
if ctx.get("tool_name") == "terminal" and "rm -rf" in json.dumps(ctx.get("args", {})):
    print(json.dumps({"cancel": True, "reason": "destructive terminal command blocked"}))
```

### 10.6 Native Rust hook

```rust
use edgecrab_gateway::hooks::{GatewayHook, HookContext, HookResult};
use async_trait::async_trait;

pub struct MetricsHook;

#[async_trait]
impl GatewayHook for MetricsHook {
    fn name(&self) -> &str { "metrics" }

    fn events(&self) -> &[&str] { &["agent:end", "llm:post"] }

    async fn handle(&self, event: &str, ctx: &HookContext) -> anyhow::Result<HookResult> {
        if event == "llm:post" {
            if let Some(tokens) = ctx.extra.get("tokens") {
                tracing::info!(tokens = %tokens, "LLM call completed");
            }
        }
        Ok(HookResult::Continue)
    }
}

// Registration (in gateway boot):
registry.register(Arc::new(MetricsHook));
```

### 10.7 Hook discovery

`HookRegistry::discover_and_load(hooks_dir)`:

1. Lista all subdirectories of `hooks_dir`
2. Looks for `HOOK.yaml` in each
3. Looks for `handler.py` → Python | `handler.ts` / `handler.js` → Bun
4. Skips silently if HOOK.yaml is missing, invalid, or `enabled: false`
5. Logs a warning if HOOK.yaml exists but no handler file is found

```bash
# Runtime: list loaded hooks
edgecrab hooks list
```

---

## 11. Source Map for Fast Navigation

| File | Why read it |
|---|---|
| `run.rs` | boot sequence, dispatch loop, command interception, auth guard |
| `config.rs` | runtime defaults and streaming knobs |
| `platform.rs` | the normalized adapter contract |
| `session.rs` | session keying and idle cleanup |
| `delivery.rs` | chunking and outbound delivery |
| `api_server.rs` | HTTP API and security headers |
| `stream_consumer.rs` | progressive message editing |
| `hooks.rs` | event hook registry |
| `pairing.rs` | pairing and approval logic surface |

If you want the shortest accurate path through the gateway code, read them in exactly that order.
