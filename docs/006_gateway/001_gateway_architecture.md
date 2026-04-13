# Gateway Architecture 🦀

> **Verified against:** `crates/edgecrab-gateway/src/lib.rs` ·
> `crates/edgecrab-gateway/src/platform.rs` ·
> `crates/edgecrab-gateway/src/run.rs` ·
> `crates/edgecrab-gateway/src/session.rs` ·
> `crates/edgecrab-gateway/src/hooks.rs`

---

## Why the gateway exists

Most AI agents require one integration per messaging platform. Adding Telegram
means writing Telegram-specific code in the core agent; adding Discord means
more core changes. The surface area grows with every channel.

EdgeCrab's gateway separates the problem: one shared `Agent` runtime, N platform
adapters. Each adapter normalises its platform's events into `IncomingMessage` and
translates `String` responses back to platform-native formats. The agent sees only
standard `IncomingMessage` regardless of origin.

🦀 *`hermes-agent` (EdgeCrab's Python predecessor) supported multiple gateway platforms.
OpenClaw focuses on single-user desktop use. EdgeCrab currently ships 15 gateway
adapters — the crab fights everywhere at once.*

---

## Supported platforms

```
  ┌─────────────────────────────────────────────────────────────────┐
  │  Platform adapters in edgecrab-gateway                          │
  │                                                                 │
  │  Messaging          Social/Dev        IoT/Internal              │
  │  ─────────────────  ───────────────── ──────────────────────    │
  │  telegram           discord           homeassistant             │
  │  whatsapp           slack             webhook                   │
  │  signal             matrix            api_server (REST)         │
  │  email              mattermost                                  │
  │  sms (Twilio)       dingtalk                                    │
  │                     feishu                                      │
  │                     wecom                                       │
  └─────────────────────────────────────────────────────────────────┘
```

---

## Main request flow

```
  Platform event (Telegram message, Discord mention, Webhook POST)
        │
        ▼
  ┌─────────────────────────────────────────┐
  │  PlatformAdapter::start(tx)             │
  │  → normalises event to IncomingMessage  │
  │  → sends to mpsc::Sender<IncomingMessage>│
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  ┌─────────────────────────────────────────┐
  │  GatewayEventProcessor                  │
  │  → resolves SessionKey                  │
  │    (platform, user_id, channel_id)      │
  │  → SessionManager::resolve()            │
  │    gets or creates GatewaySession       │
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  ┌─────────────────────────────────────────┐
  │  Hook: gateway:agent:start              │
  │  → HookRegistry::emit()                 │
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  ┌─────────────────────────────────────────┐
  │  Agent::chat_streaming(message, tx)     │
  │  → full conversation loop               │
  │  → StreamEvent::Token events            │
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  ┌─────────────────────────────────────────┐
  │  DeliveryRouter                         │
  │  → reassembles token stream             │
  │  → extracts [MEDIA:/path] tags          │
  │  → sends text via adapter.send()        │
  │  → uploads media via adapter.send_photo()│
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  Platform receives reply
```

---

## `PlatformAdapter` trait

All 15 gateway adapters implement this trait:

```rust
#[async_trait]
pub trait PlatformAdapter: Send + Sync + 'static {
    fn platform(&self) -> Platform;

    // Start listening and push events into tx
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()>;

    // Send a text message
    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()>;

    // Format response for this platform (markdown → plain text for SMS, etc.)
    fn format_response(&self, text: &str, metadata: &MessageMetadata) -> String;

    // Platform capability flags
    fn max_message_length(&self)  -> usize;
    fn supports_markdown(&self)   -> bool;
    fn supports_images(&self)     -> bool;
    fn supports_files(&self)      -> bool;
    fn supports_editing(&self)    -> bool { false }  // live message editing

    // Optional — default implementations provided
    async fn edit_message(&self, id, metadata, text) -> anyhow::Result<String>;
    async fn send_status(&self, text, metadata) -> anyhow::Result<()>;
    async fn send_typing(&self, metadata) -> anyhow::Result<()>;
    async fn send_and_get_id(&self, msg) -> anyhow::Result<Option<String>>;
    async fn send_photo(&self, path, caption, metadata) -> anyhow::Result<()>;
    async fn send_document(&self, path, caption, metadata) -> anyhow::Result<()>;
}
```

---

## Message model

### Inbound

```rust
pub struct IncomingMessage {
    pub platform: Platform,
    pub user_id:  String,
    pub channel_id: Option<String>,
    pub text:     String,
    pub thread_id: Option<String>,
    pub metadata: MessageMetadata,
}
impl IncomingMessage {
    pub fn is_command(&self) -> bool  // starts with /
    pub fn get_command(&self) -> Option<&str>  // "/help" → "help"
    pub fn get_command_args(&self) -> &str
}
```

### Outbound

```rust
pub struct OutgoingMessage {
    pub text:     String,
    pub metadata: MessageMetadata,
}

pub struct MessageMetadata {
    pub message_id:        Option<String>,
    pub channel_id:        Option<String>,
    pub thread_id:         Option<String>,
    pub user_display_name: Option<String>,
    pub attachments:       Vec<MessageAttachment>,
}
```

---

## Media tag protocol

Agents can produce media files by including special tags in their responses.
The `DeliveryRouter` intercepts these before sending:

```
  Agent produces:  "Here is the generated chart: [IMAGE:/tmp/chart.png]"
        │
        ▼
  extract_media_from_response(text)
        │
  ┌─────┴──────────────────────────────────────────────┐
  │  text:   "Here is the generated chart: "          │
  │  media: [MediaRef { path: "/tmp/chart.png",        │
  │                      is_image: true }]             │
  └─────┬──────────────────────────────────────────────┘
        │
        ▼
  adapter.send(OutgoingMessage { text, .. })
  adapter.send_photo("/tmp/chart.png", caption, metadata)
```

Tags: `[IMAGE:/path]`, `[MEDIA:/path]`, `[FILE:/path]`

Image detection heuristic: extension in `[png, jpg, jpeg, gif, webp, svg, bmp]`

---

## Session management

```rust
pub struct SessionKey {
    pub platform: Platform,
    pub user_id:  String,
    pub channel_id: Option<String>,
}

pub struct GatewaySession {
    pub session_id:      String,
    pub history:         Vec<Message>,
    pub last_activity:   Instant,
    pub model_override:  Option<String>,
}

pub struct SessionManager {
    sessions:      DashMap<SessionKey, Arc<RwLock<GatewaySession>>>,
    idle_timeout:  Duration,
}
```

Sessions are cleaned up after `idle_timeout` (configurable in `GatewayConfig`).
The cleanup task runs on the background GC loop.

---

## Hooks

The hook system allows custom logic to run at every significant event.

### Event catalogue

| Event | When | Cancellable? |
|---|---|---|
| `gateway:startup` | Process starts | No |
| `session:start` | New user session | No |
| `session:end` | Session ended/timed out | No |
| `session:reset` | User types `/new` | No |
| `agent:start` | Agent begins processing | No |
| `agent:step` | Each tool-call iteration | No |
| `agent:end` | Agent returns response | No |
| `command:*` | Any slash command | Yes |
| `tool:pre` | Before tool executes | Yes |
| `tool:post` | After tool returns | No |
| `llm:pre` | Before API call | Yes |
| `llm:post` | After API response | No |
| `cli:start` / `cli:end` | CLI session lifecycle | No |

### Native Rust hooks

```rust
// Implement in a gateway hook module
pub struct MyHook;

#[async_trait]
impl GatewayHook for MyHook {
    fn name(&self)   -> &'static str { "my_hook" }
    fn events(&self) -> &'static [&'static str] { &["agent:end"] }

    async fn handle(&self, ctx: &HookContext) -> HookResult {
        println!("Agent responded: {:?}", ctx.extra.get("response"));
        HookResult::Continue
    }
}
```

### File-based script hooks

Place a hook in `~/.edgecrab/hooks/<hook-name>/`:
- `HOOK.yaml` — metadata (name, events)
- `handler.py` / `handler.js` / `handler.ts` — script

EdgeCrab passes `HookContext` as JSON on stdin. For cancellable events,
the script can cancel by writing `{"cancel": true}` to stdout.

```yaml
# ~/.edgecrab/hooks/log-responses/HOOK.yaml
name: log-responses
events: [agent:end]
```

```python
# ~/.edgecrab/hooks/log-responses/handler.py
import json, sys
ctx = json.load(sys.stdin)
with open("/tmp/responses.log", "a") as f:
    f.write(ctx.get("response", "") + "\n")
```

---

## Streaming delivery

For platforms that support message editing (e.g. Telegram), the gateway can
update the message in place as tokens arrive — like Claude.ai's streaming effect:

```
  User sends message
  → typing indicator shown
  → after first N tokens: initial message created
  → subsequent tokens: message edited in place (rate-limited)
  → final token: message finalised
```

Controlled by `gateway.config.streaming_edits` (per-platform flag).

---

## Pairing flow

New Telegram/WhatsApp/Signal users are required to pair before the agent responds:

```
  Unknown user sends message
        │
        ▼
  pairing.rs: generate 6-digit code
        │
        ▼
  "To use EdgeCrab, visit https://... and enter code: 123456"
        │
        ▼
  Admin approves in CLI: edgecrab gateway configure
        │
        ▼
  user is now authorised; sessions resume normally
```

---

## Authorization (auth.rs)

Every inbound message passes through `check_authorization()` before reaching the agent.
The authorization chain evaluates rules in strict priority order — the first match wins:

| Step | Rule | Result |
|------|------|--------|
| 1a | System platform bypass (Webhook, HomeAssistant, Cron, Api) | `Allowed(PlatformBypass)` |
| 1b | WhatsApp self-chat: `WHATSAPP_MODE=self-chat` | `Allowed(PlatformBypass)` |
| 1c | Generic self-chat: `{PREFIX}_SELF_CHAT=true` (any platform) | `Allowed(PlatformBypass)` |
| 2 | Group policy: `GroupPolicy::Disabled` for group/channel messages | `Denied(GroupPolicyDeny)` |
| 3 | Global allow-all: `GATEWAY_ALLOW_ALL_USERS=true` | `Allowed(GlobalAllowAll)` |
| 4 | Per-platform allow-all: `{PREFIX}_ALLOW_ALL_USERS=true` | `Allowed(PlatformAllowAll)` |
| 5 | Pairing store match | `Allowed(PairingApproved)` |
| 6 | Allowlist match: `GATEWAY_ALLOWED_USERS` or `{PREFIX}_ALLOWED_USERS` | `Allowed(Allowlist)` |
| 7 | No match — secure by default | `Denied(NoAllowlistDeny)` |

---

## WhatsApp Self-Chat Mode

Self-chat mode lets the user talk to the EdgeCrab agent through their own WhatsApp
number — messaging themselves. The agent receives only the user's own messages
and never replies to groups or other contacts.

### Three-Layer Defence

```
  ┌─────────────────────────────────────────────────────┐
  │  Layer 1: bridge.js (JavaScript)                    │
  │  ─────────────────────────────────────              │
  │  • fromMe=true  → skip groups, skip bot echoes,     │
  │                    allow only isSelfChat messages    │
  │  • fromMe=false → drop in self-chat mode            │
  │  • Echo guard:  recentlySentIds + REPLY_PREFIX      │
  └──────────────────────┬──────────────────────────────┘
                         │ HTTP POST /events
                         ▼
  ┌─────────────────────────────────────────────────────┐
  │  Layer 2: WhatsApp Rust Adapter (whatsapp.rs)       │
  │  ─────────────────────────────────────              │
  │  • mode != "bot" && !event.from_me → drop           │
  │  • Defence-in-depth: catches stale/old bridge       │
  └──────────────────────┬──────────────────────────────┘
                         │ mpsc::send(IncomingMessage)
                         ▼
  ┌─────────────────────────────────────────────────────┐
  │  Layer 3: Gateway Auth (auth.rs)                    │
  │  ─────────────────────────────────────              │
  │  • WHATSAPP_MODE=self-chat → PlatformBypass         │
  │  • No allowlist required in self-chat mode          │
  └─────────────────────────────────────────────────────┘
```

### Configuration

In `~/.edgecrab/config.yaml`:

```yaml
gateway:
  enabled_platforms:
    - whatsapp
  whatsapp:
    enabled: true
    mode: self-chat       # "self-chat" or "bot"
    bridge_port: 3000
    allowed_users: []     # not needed in self-chat mode
    reply_prefix: "⚕ *EdgeCrab Agent*"
```

### Echo Prevention

When EdgeCrab sends a reply in self-chat mode, the reply appears as a message
from the same WhatsApp account. Without echo prevention, this would create an
infinite loop. The bridge prevents this with two mechanisms:

1. **`recentlySentIds`** — a Set of message IDs recently sent by the agent.
   When a new `fromMe` message matches, it is silently dropped.
2. **`REPLY_PREFIX`** — messages starting with the configured prefix (e.g.
   `⚕ *EdgeCrab Agent*`) are identified as agent echoes and dropped.

### `from_me` Field

The `WhatsAppInboundEvent` struct includes a `from_me: bool` field
(`#[serde(rename = "fromMe", default)]`). It defaults to `false` (conservative)
so that an unknown-origin message is treated as a contact message and dropped
in self-chat mode.

---

## Tips

> **Tip: Check `ADAPTER_RETRY_DELAY = 5s` and `ADAPTER_MAX_RETRY_DELAY = 60s`.**
> Adapters that fail to connect (network issue, wrong token) retry with exponential
> backoff capped at 60 seconds. Watch logs for repeated retry messages to diagnose
> misconfigured platform credentials.

> **Tip: Mirror mode duplicates sessions across platforms.**
> `mirror.rs` implements cross-platform session mirroring — a Telegram session can
> be mirrored to Slack so the same conversation appears in both. Configure via
> `gateway.mirrors` in config.

> **Tip: The REST API adapter (`api_server`) is the fastest integration path.**
> If you're building a custom frontend, POST to the gateway's HTTP API rather
> than implementing a full adapter.

---

## FAQ

**Q: How many concurrent users can the gateway handle?**
One `Agent` per `SessionKey`. Agents run as Tokio tasks — concurrency is
limited by memory (each agent holds its conversation history) and provider rate
limits. `DashMap` in `SessionManager` ensures session lookups don't serialise
on a global lock.

**Q: Can a single user have sessions on multiple platforms?**
Yes, but each `(platform, user_id)` is a separate session by default. Session
mirroring (`mirror.rs`) can link them — the same conversation appears on both.

**Q: Does the gateway use the same SQLite database as the CLI?**
Yes, by default. Both use `~/.edgecrab/state.db` in WAL mode. The jitter-retry
policy in `SessionDb` handles concurrent writes from both processes without
corruption.

---

## Cross-references

- `GatewaySender` trait (tool layer) → [Tool Registry](../004_tools_system/001_tool_registry.md)
- Session DB shared with CLI → [Session Storage](../009_config_state/002_session_storage.md)
- Hook configuration → [Hooks](../hooks.md)
- Concurrency model for session fan-out → [Concurrency Model](../002_architecture/003_concurrency_model.md)
- Path security and `jail_read_path` → `edgecrab-security/path_policy.rs`
- WhatsApp bridge source → `scripts/whatsapp-bridge/bridge.js`
