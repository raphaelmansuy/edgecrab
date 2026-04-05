# edgecrab Hooks System

> File-based event hooks with Python and JavaScript/TypeScript (Bun) support.

**Related docs:**
- [Architecture overview](architecture/README.md) — how edgecrab fits together
- [Features](features.md) — full feature list
- [TUI design guidelines](tui-design-guidelines.md) — slash commands and UI
- [MCP server](mcp-server.md) — alternative programmatic integration

---

## Overview

edgecrab's hook system lets you run custom scripts at key points in the agent
lifecycle — before and after tool calls, around LLM API calls, on session start
and end, and more.  Each hook is a standalone script with a `HOOK.yaml`
manifest.  Multiple hooks can subscribe to the same event; they fire in
priority order.

### Advantages over hermes-agent hooks

| Feature | hermes-agent | edgecrab |
|---|---|---|
| Languages | Python only | Python **and** JS/TS via Bun |
| Cancellation | ✗ | ✓ pre-hooks can abort operations |
| Priority ordering | ✗ | ✓ `priority:` field (lower = first) |
| Per-hook timeout | ✗ | ✓ `timeout_secs:` field |
| Per-hook env vars | ✗ | ✓ `env:` map |
| Global wildcard | ✗ | ✓ `events: ["*"]` |
| Tool-level events | via plugin only | ✓ `tool:pre`, `tool:post` |
| LLM-level events | via plugin only | ✓ `llm:pre`, `llm:post` |
| Context data | `HashMap<String,String>` | `serde_json::Value` (arbitrary JSON) |
| Native Rust hooks | ✗ | ✓ `GatewayHook` trait |
| Test harness | ✗ | ✓ `discover_and_load_from(path)` + tempdir |

---

## Architecture

```
~/.edgecrab/hooks/
  my-hook/
    HOOK.yaml       ← parsed into HookManifest
    handler.py      ← Python 3 subprocess
  notifier/
    HOOK.yaml
    handler.ts      ← Bun subprocess

Gateway starts
  └── Gateway::new()
        └── HookRegistry::discover_and_load()
              ├── scan ~/.edgecrab/hooks/*
              ├── parse HOOK.yaml (serde_yml)
              ├── detect handler.py / handler.ts / handler.js
              ├── verify runtime on PATH (python3 / bun)
              └── sort entries by priority

Incoming message
  └── Gateway::run()
        ├── emit("command:new", ctx)   ← HookRegistry::emit()
        ├── emit("agent:start", ctx)
        └── dispatch_streaming_arc()
              └── GatewayEventProcessor::run()
                    └── StreamEvent::HookEvent { event, context_json }
                          └── HookRegistry::emit(event, ctx)

conversation.rs (core)
  ├── api_call_with_retry()
  │     ├── send HookEvent { "llm:pre", ... }
  │     └── send HookEvent { "llm:post", ... }
  └── dispatch_single_tool()
        ├── send HookEvent { "tool:pre", ... }
        └── send HookEvent { "tool:post", ... }
```

### Key design decisions

**Why subprocess-based scripting?**
Spawning a subprocess per event isolates crash domains.  A broken Python hook
cannot panic the Rust process.  Timeouts are enforced with `tokio::time::timeout`.

**Why `tool:pre` / `llm:pre` are fire-and-forget from core**
The agent core (`edgecrab-core`) knows nothing about hooks.  It sends
`StreamEvent::HookEvent` into the mpsc channel, and the gateway's
`GatewayEventProcessor` receives these and dispatches them via `HookRegistry`.
This keeps the core free of gateway dependencies.

**Why cancel only propagates from gateway-level events**
Cancellation requires synchronous blocking: the gateway waits for all hooks
before proceeding.  For `tool:pre` / `llm:pre` the core cannot wait for the
gateway (it would deadlock the channel).  Pre-hook cancellation for
gateway-level events (`agent:start`, `command:*`) works because the gateway
owns the decision loop.

---

## Quick start

1. Create `~/.edgecrab/hooks/<name>/` directory.
2. Add a `HOOK.yaml` manifest.
3. Add a `handler.py`, `handler.ts`, or `handler.js` script.

```
~/.edgecrab/
└── hooks/
    ├── my-logger/
    │   ├── HOOK.yaml
    │   └── handler.py
    └── notifier/
        ├── HOOK.yaml
        └── handler.ts
```

edgecrab discovers and loads hooks at startup.  No restart needed if you add
hooks — restart edgecrab to pick up new hooks.

---

## HOOK.yaml schema

```yaml
name: my-logger           # Human-readable hook name (required)
description: Log events   # Optional description (shown in /hooks slash command)
events:                   # List of event patterns to subscribe to (required)
  - "tool:*"
  - "llm:pre"
timeout_secs: 10          # Max seconds to wait for handler (default: 10)
priority: 50              # Lower = fires first; default 50; range 0-999
enabled: true             # Set false to disable without deleting (default: true)
env:                      # Extra environment variables passed to handler
  SLACK_WEBHOOK: "https://..."
  LOG_LEVEL: "debug"
```

All fields except `name` and `events` are optional and have defaults.

### Event pattern matching

| Pattern | Matches |
|---|---|
| `"tool:pre"` | Exact match only |
| `"tool:*"` | Prefix wildcard — `tool:pre`, `tool:post` |
| `"session:*"` | `session:start`, `session:end`, `session:reset` |
| `"command:*"` | `command:new`, `command:model`, `command:stop`, … |
| `"*"` | Global wildcard — every event |

Pattern matching is case-sensitive.  Event names are always lowercase.

---

## Event catalogue

### Events fired by the Gateway

| Event | Fired when | Cancel? | Context fields |
|---|---|---|---|
| `gateway:startup` | Gateway process boots | — | `platforms` (list of adapter names) |
| `session:start` | A session slot is first used | — | `platform`, `user_id`, `session_id` |
| `session:end` | Session expires or is removed | — | `platform`, `user_id`, `session_key` |
| `session:reset` | User runs `/new` or `/reset` | — | `platform`, `user_id`, `session_key` |
| `agent:start` | Agent begins processing a message | ✓ | `platform`, `user_id`, `session_id`, `message` |
| `agent:step` | Each iteration of the REACT loop | — | `platform`, `user_id`, `session_id`, `iteration`, `tool_names` |
| `agent:end` | Agent finishes responding | — | `platform`, `user_id`, `session_id`, `response` |
| `command:<name>` | Any slash command | ✓ | `platform`, `user_id`, `command`, `args` |

### Events fired from core (via StreamEvent bridge)

| Event | Fired when | Cancel? | Context fields |
|---|---|---|---|
| `tool:pre` | Before a tool call executes | fire-and-forget | `session_id`, `tool_name`, `args_json` |
| `tool:post` | After a tool call completes | fire-and-forget | `session_id`, `tool_name`, `result`, `is_error`, `duration_ms` |
| `llm:pre` | Before an LLM API request | fire-and-forget | `session_id`, `model`, `platform` |
| `llm:post` | After an LLM API response | fire-and-forget | `session_id`, `model`, `platform`, `prompt_tokens`, `completion_tokens` |

### Events fired by the CLI

| Event | Fired when | Context fields |
|---|---|---|
| `cli:start` | CLI session opens | `session_id`, `model`, `platform` |
| `cli:end` | CLI session exits | `session_id`, `model`, `platform` |

> **Cancel note:** `tool:pre` and `llm:pre` are "fire-and-forget" — hooks run
> after the fact because the event crosses an async channel boundary from core
> to gateway.  Cancellation of these events is a roadmap item.

---

## Per-event context field reference

### `gateway:startup`

```json
{
  "event": "gateway:startup",
  "platforms": ["telegram", "discord"]
}
```

### `session:start`

```json
{
  "event": "session:start",
  "session_id": "sess-abc123",
  "user_id": "u-42",
  "platform": "telegram"
}
```

### `session:end` / `session:reset`

```json
{
  "event": "session:end",
  "user_id": "u-42",
  "platform": "telegram",
  "session_key": "telegram:u-42"
}
```

### `agent:start`

```json
{
  "event": "agent:start",
  "session_id": "sess-abc123",
  "user_id": "u-42",
  "platform": "telegram",
  "message": "Search the web for Rust 2024 edition changes"
}
```

### `command:<name>`

```json
{
  "event": "command:new",
  "user_id": "u-42",
  "platform": "telegram",
  "command": "new",
  "args": ""
}
```

### `tool:pre`

```json
{
  "event": "tool:pre",
  "session_id": "sess-abc123",
  "tool_name": "bash",
  "args_json": "{\"command\": \"ls -la\"}"
}
```

### `tool:post`

```json
{
  "event": "tool:post",
  "session_id": "sess-abc123",
  "tool_name": "bash",
  "result": "total 48\ndrwxr-xr-x ...",
  "is_error": false,
  "duration_ms": 42
}
```

### `llm:pre`

```json
{
  "event": "llm:pre",
  "session_id": "sess-abc123",
  "model": "claude-opus-4-5",
  "platform": "telegram"
}
```

### `llm:post`

```json
{
  "event": "llm:post",
  "session_id": "sess-abc123",
  "model": "claude-opus-4-5",
  "platform": "telegram",
  "prompt_tokens": 1842,
  "completion_tokens": 317
}
```

---

## Handler protocol

### Input (stdin)

Your handler receives a JSON object on stdin:

```json
{
  "event": "tool:pre",
  "session_id": "abc-123",
  "tool_name": "bash",
  "args_json": "{\"command\": \"ls -la\"}",
  "is_error": false
}
```

The exact fields depend on the event — see the per-event reference above.

### Output (stdout) — optional

Handlers can return a JSON object to influence the caller:

```json
{ "cancel": true, "reason": "command blocked by policy" }
```

| Field | Type | Meaning |
|---|---|---|
| `cancel` | `bool` | `true` to abort the operation (pre-hooks only) |
| `reason` | `string` | Human-readable reason shown in logs |

If you don't need to cancel, return nothing, `{}`, or omit stdout entirely.
Invalid JSON on stdout is silently ignored (the hook returns `Continue`).

### Runtime selection

| File | Runtime |
|---|---|
| `handler.py` | Python 3 (`python3` on PATH) |
| `handler.ts` | Bun (`bun` on PATH) |
| `handler.js` | Bun (`bun` on PATH) |

Priority: `handler.py` > `handler.ts` > `handler.js` when multiple exist.

If the required runtime is not on PATH, the hook is skipped at load time with
a warning in the edgecrab logs.

### Error handling

- Non-zero exit code → logged as warning, hook returns `Continue`.
- Timeout reached → logged as warning, hook returns `Continue`.
- Invalid JSON stdout → silently falls back to `Continue`.
- Runtime not found → hook not loaded (warning at startup).

Hooks **never crash the agent process**.

---

## Examples

### Python — Telegram alert on tool errors

**HOOK.yaml**
```yaml
name: error-alert
description: Send a Telegram message when a tool call fails
events: ["tool:post"]
timeout_secs: 5
env:
  BOT_TOKEN: "123456:ABC..."
  CHAT_ID: "-100987654321"
```

**handler.py**
```python
import json, sys, os, urllib.request

data = json.load(sys.stdin)

if data.get("is_error"):
    tool = data.get("tool_name", "unknown")
    session = data.get("session_id", "?")
    msg = f"⚠️ Tool error: {tool!r} in session {session}"

    token = os.environ["BOT_TOKEN"]
    chat_id = os.environ["CHAT_ID"]
    url = f"https://api.telegram.org/bot{token}/sendMessage"
    payload = json.dumps({"chat_id": chat_id, "text": msg}).encode()
    urllib.request.urlopen(url, data=payload)
```

---

### TypeScript (Bun) — Session webhook

**HOOK.yaml**
```yaml
name: session-webhook
description: POST session lifecycle events to an external API
events: ["session:*"]
timeout_secs: 8
env:
  WEBHOOK_URL: "https://api.example.com/events"
```

**handler.ts**
```typescript
const data = await Bun.stdin.json();

const url = process.env.WEBHOOK_URL!;
await fetch(url, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    source: "edgecrab",
    event: data.event,
    session_id: data.session_id ?? null,
    timestamp: new Date().toISOString(),
  }),
});
```

---

### Python — LLM usage logger

**HOOK.yaml**
```yaml
name: llm-logger
description: Append LLM usage stats to a local JSONL file
events: ["llm:post"]
```

**handler.py**
```python
import json, sys, pathlib, datetime

data = json.load(sys.stdin)

log_path = pathlib.Path.home() / ".edgecrab" / "llm_usage.jsonl"
entry = {
    "ts": datetime.datetime.utcnow().isoformat(),
    "model": data.get("model"),
    "prompt_tokens": data.get("prompt_tokens", 0),
    "completion_tokens": data.get("completion_tokens", 0),
}
with log_path.open("a") as f:
    f.write(json.dumps(entry) + "\n")
```

---

### Python — Startup guard (cancellable)

Use a `priority: 1` hook (fires first) to abort if required env vars are missing.

**HOOK.yaml**
```yaml
name: startup-guard
description: Abort gateway startup if required env vars are missing
events: ["gateway:startup"]
priority: 1
```

**handler.py**
```python
import json, sys, os

data = json.load(sys.stdin)

required = ["ANTHROPIC_API_KEY"]
missing = [k for k in required if not os.environ.get(k)]

if missing:
    print(json.dumps({
        "cancel": True,
        "reason": f"Missing required env vars: {', '.join(missing)}"
    }))
```

---

### TypeScript (Bun) — Rate-limit incoming messages

**HOOK.yaml**
```yaml
name: rate-limiter
description: Block more than 5 messages per minute per user
events: ["agent:start"]
priority: 5
timeout_secs: 2
env:
  MAX_RPM: "5"
```

**handler.ts**
```typescript
import { readFileSync } from "fs";
import { join } from "path";

const data = await Bun.stdin.json();
const userId: string = data.user_id ?? "anonymous";
const maxRpm = parseInt(process.env.MAX_RPM ?? "5", 10);

const stateFile = join(process.env.HOME!, ".edgecrab", "rate_state.json");
let state: Record<string, { count: number; window: number }> = {};
try {
  state = JSON.parse(readFileSync(stateFile, "utf8"));
} catch {}

const now = Math.floor(Date.now() / 60_000); // minute bucket
const rec = state[userId] ?? { count: 0, window: now };
if (rec.window !== now) {
  rec.count = 0;
  rec.window = now;
}
rec.count++;
state[userId] = rec;

require("fs").writeFileSync(stateFile, JSON.stringify(state));

if (rec.count > maxRpm) {
  process.stdout.write(JSON.stringify({
    cancel: true,
    reason: `Rate limit: ${maxRpm} messages/minute exceeded for user ${userId}`,
  }));
}
```

---

## Slash command: `/hooks`

Send `/hooks` in any platform or the edgecrab TUI to list all hooks currently
loaded by the gateway.

Example output:
```
🪝 Loaded hooks (3 total):

• error-alert [python] p=50
  Events: `tool:post`
  _Send a Telegram message when a tool call fails_

• llm-logger [python] p=50
  Events: `llm:post`

• startup-guard [python] p=1
  Events: `gateway:startup`
  _Abort gateway startup if required env vars are missing_
```

The gateway `HELP_TEXT` also lists `/hooks` alongside all other slash commands.

---

## Native Rust hooks (advanced)

In addition to script hooks, you can register native Rust hooks that are
compiled into your binary.  Implement the `GatewayHook` trait:

```rust
use async_trait::async_trait;
use edgecrab_gateway::hooks::{GatewayHook, HookContext, HookResult};

pub struct MyAuditHook;

#[async_trait]
impl GatewayHook for MyAuditHook {
    fn name(&self) -> &str { "my-audit-hook" }

    fn events(&self) -> &[&str] {
        &["agent:start", "tool:post", "llm:post"]
    }

    async fn handle(&self, event: &str, ctx: &HookContext) -> anyhow::Result<HookResult> {
        tracing::info!(
            event,
            session_id = ?ctx.session_id,
            "audit log"
        );
        Ok(HookResult::Continue)
    }
}
```

Register at gateway construction time:

```rust
let mut registry = HookRegistry::new();
registry.register(Box::new(MyAuditHook));
// ... also call discover_and_load() for script hooks
gateway.set_hooks(registry);
```

### `HookContext` builder API

```rust
let ctx = HookContext::new("tool:post")
    .with_session("sess-abc")          // sets session_id
    .with_user("u-42")                 // sets user_id
    .with_platform("telegram")         // sets platform
    .with_str("tool_name", "bash")     // adds string to extra
    .with_value("result", json!(42));  // adds any JSON Value to extra

// Serialize to JSON string (used for subprocess stdin)
let json: String = ctx.to_json()?;
```

### `HookRegistry` API

```rust
// Construct
let mut reg = HookRegistry::new();

// Load script hooks
reg.discover_and_load();                // from ~/.edgecrab/hooks/
reg.discover_and_load_from(&path);     // from explicit path (for tests)

// Register native hooks
reg.register(Box::new(MyHook));

// Emit — fire-and-forget, errors logged
reg.emit("llm:post", &ctx).await;

// Emit cancellable — first Cancel wins, returns it
let result: HookResult = reg.emit_cancellable("agent:start", &ctx).await;

// Introspection
reg.hook_count();          // total hooks (native + script)
reg.loaded_hooks();        // &[LoadedHookInfo] — metadata for script hooks
```

### `HookResult`

```rust
pub enum HookResult {
    Continue,
    Cancel { reason: String },
}

result.is_cancel()  // true if Cancel variant
```

---

## Testing hooks

### Unit testing with native hooks

```rust
use edgecrab_gateway::hooks::{GatewayHook, HookContext, HookRegistry, HookResult};

struct AlwaysCancelHook;

#[async_trait::async_trait]
impl GatewayHook for AlwaysCancelHook {
    fn name(&self) -> &str { "test-cancel" }
    fn events(&self) -> &[&str] { &["tool:pre"] }
    async fn handle(&self, _event: &str, _ctx: &HookContext) -> anyhow::Result<HookResult> {
        Ok(HookResult::Cancel { reason: "blocked in test".into() })
    }
}

#[tokio::test]
async fn my_hook_cancels_tool_pre() {
    let mut reg = HookRegistry::new();
    reg.register(Box::new(AlwaysCancelHook));
    let result = reg.emit_cancellable("tool:pre", &HookContext::new("tool:pre")).await;
    assert!(result.is_cancel());
}
```

### Integration testing script hooks

Use `HookRegistry::discover_and_load_from(&tempdir)` to run script hooks
found in a temporary directory:

```rust
use tempfile::tempdir;
use edgecrab_gateway::hooks::{HookContext, HookRegistry, HookResult};

#[tokio::test]
async fn python_hook_cancels_on_bad_input() {
    let dir = tempdir().unwrap();
    let hook_dir = dir.path().join("my-hook");
    std::fs::create_dir_all(&hook_dir).unwrap();

    std::fs::write(hook_dir.join("HOOK.yaml"), b"
name: my-hook
events:
  - tool:pre
").unwrap();
    std::fs::write(hook_dir.join("handler.py"), b"
import json, sys
data = json.load(sys.stdin)
# Cancel if tool is 'bash'
if data.get('tool_name') == 'bash':
    sys.stdout.write(json.dumps({'cancel': True, 'reason': 'bash blocked'}))
").unwrap();

    let mut reg = HookRegistry::new();
    reg.discover_and_load_from(dir.path());

    let ctx = HookContext::new("tool:pre").with_str("tool_name", "bash");
    let result = reg.emit_cancellable("tool:pre", &ctx).await;
    assert!(result.is_cancel());
}
```

See `crates/edgecrab-gateway/tests/hooks_integration.rs` for the full
integration test suite with 16 test cases covering discovery, priority,
cancellation, env vars, timeouts, and JS/TS handlers.

---

## Troubleshooting

**Hook not loading?**
- Check `~/.edgecrab/hooks/<name>/HOOK.yaml` exists and is valid YAML.
- Check that `handler.py` / `handler.ts` / `handler.js` exists alongside it.
- Make sure `python3` (for `.py`) or `bun` (for `.ts` / `.js`) is on `PATH`.
- Set `enabled: false` to sanity-check that discovery reaches the hook.
- Run edgecrab with `RUST_LOG=debug` to see hook loading messages.

**Hook timing out?**
- Increase `timeout_secs:` in `HOOK.yaml`.
- Check for network issues or blocking I/O.
- Add `import sys; sys.stderr.write("alive\n")` diagnostics — stderr goes to
  the edgecrab debug log.

**Hook fires but cancel is ignored?**
- Cancellation is only honoured for `gateway:startup`, `session:*`,
  `agent:start`, and `command:*` events.
- `tool:pre` and `llm:pre` are fire-and-forget across the core→gateway
  channel boundary.

**Debugging handler output?**
- Write diagnostics to `stderr` — captured by edgecrab and forwarded to the
  structured log.  `stdout` is reserved for the JSON response.

**Hook order wrong?**
- Lower `priority:` values fire first.  Use `priority: 1` for guards that
  must run before anything else.  Default priority is `50`.

**My TypeScript hook doesn't run?**
- Make sure `bun` is on `PATH`: `which bun`.  Install via `curl -fsSL https://bun.sh/install | bash`.
- Use `handler.ts` not `index.ts` — only `handler.{py,ts,js}` filenames are
  recognized.

---

## Security considerations

Hooks run with full access to the process environment and filesystem.  Treat
hook scripts with the same trust as your edgecrab configuration:

- Store secrets in `env:` fields in `HOOK.yaml`, not hardcoded in scripts.
- `HOOK.yaml` and scripts should be owned by your user and not world-writable.
- Hooks that make network calls should use `timeout_secs:` to prevent hangs.
- Secrets can be injected via `env:` in `HOOK.yaml` — they are not logged.

---

## Roadmap

- [ ] Cancellation for `tool:pre` / `llm:pre` (requires sync channel between
      core and gateway)
- [ ] Hot-reload hooks without restarting edgecrab
- [ ] Retry on transient error (configurable in `HOOK.yaml`)
- [ ] `edgecrab hooks validate` CLI command


1. Create `~/.edgecrab/hooks/<name>/` directory.
2. Add a `HOOK.yaml` manifest.
3. Add a `handler.py`, `handler.ts`, or `handler.js` script.

```
~/.edgecrab/
└── hooks/
    ├── my-logger/
    │   ├── HOOK.yaml
    │   └── handler.py
    └── notifier/
        ├── HOOK.yaml
        └── handler.ts
```

---

## HOOK.yaml schema

```yaml
name: my-logger           # Human-readable hook name (required)
description: Log events   # Optional description (shown in /hooks slash command)
events:                   # List of event patterns to subscribe to (required)
  - "tool:*"
  - "llm:pre"
timeout_secs: 10          # Max seconds to wait for handler (default: 10)
priority: 50              # Lower = fires first; default 50; range 0-999
enabled: true             # Set false to disable without deleting (default: true)
env:                      # Extra environment variables passed to handler
  SLACK_WEBHOOK: "https://..."
  LOG_LEVEL: "debug"
```

### Event pattern matching

| Pattern | Matches |
|---|---|
| `"tool:pre"` | Exact match |
| `"tool:*"` | Prefix wildcard — matches `tool:pre`, `tool:post` |
| `"session:*"` | Matches `session:start`, `session:end`, `session:reset` |
| `"*"` | Global wildcard — matches every event |

---

## Event catalogue

| Event | When fired | CLI | Gateway | Cancel? |
|---|---|---|---|---|
| `gateway:startup` | Gateway boots | — | ✓ | — |
| `session:start` | New session starts | — | ✓ | — |
| `session:end` | Session ends cleanly | — | ✓ | — |
| `session:reset` | `/new` or `/reset` command | — | ✓ | — |
| `agent:start` | Agent receives a message | — | ✓ | ✓ |
| `agent:step` | Each REACT loop iteration | — | ✓ | — |
| `agent:end` | Agent finishes responding | — | ✓ | — |
| `command:*` | Slash command received | — | ✓ | ✓ |
| `tool:pre` | Before each tool call | ✓ | ✓ | — |
| `tool:post` | After each tool call | ✓ | ✓ | — |
| `llm:pre` | Before each LLM API call | ✓ | ✓ | — |
| `llm:post` | After each LLM API call | ✓ | ✓ | — |

> **Cancel?** — Pre-hooks that return `{"cancel": true}` can abort the
> operation.  Supported on `agent:start` and `command:*` gateway events.
> `tool:pre` and `llm:pre` emit fire-and-forget because they cross the
> core→gateway boundary via the streaming channel.

---

## Handler protocol

### Input (stdin)

Your handler receives a JSON object on stdin:

```json
{
  "event": "tool:pre",
  "session_id": "abc-123",
  "tool_name": "bash",
  "args_json": "{\"command\": \"ls -la\"}",
  "is_error": false
}
```

The exact fields depend on the event — see the event catalogue above.

### Output (stdout) — optional

Handlers can return a JSON object to influence the caller:

```json
{ "cancel": true, "reason": "command blocked by policy" }
```

| Field | Type | Meaning |
|---|---|---|
| `cancel` | `bool` | `true` to abort the operation (pre-hooks only) |
| `reason` | `string` | Human-readable reason shown in logs |

If you don't need to cancel, return nothing or an empty JSON object `{}`.

### Runtime selection

| File | Runtime |
|---|---|
| `handler.py` | Python 3 (`python3` on PATH) |
| `handler.ts` | Bun (`bun` on PATH) |
| `handler.js` | Bun (`bun` on PATH) |

If both `handler.py` and `handler.ts` exist, Python takes priority.

If the required runtime is not on PATH, the hook is skipped at load time with
a warning.

---

## Examples

### Python — Telegram alert on tool errors

**HOOK.yaml**
```yaml
name: error-alert
description: Send a Telegram message when a tool call fails
events: ["tool:post"]
timeout_secs: 5
env:
  BOT_TOKEN: "123456:ABC..."
  CHAT_ID: "-100987654321"
```

**handler.py**
```python
import json, sys, os, urllib.request

data = json.load(sys.stdin)

if data.get("is_error"):
    tool = data.get("tool_name", "unknown")
    session = data.get("session_id", "?")
    msg = f"⚠️ Tool error: {tool!r} in session {session}"

    token = os.environ["BOT_TOKEN"]
    chat_id = os.environ["CHAT_ID"]
    url = f"https://api.telegram.org/bot{token}/sendMessage"
    payload = json.dumps({"chat_id": chat_id, "text": msg}).encode()
    urllib.request.urlopen(url, data=payload)
```

---

### TypeScript (Bun) — Session webhook

**HOOK.yaml**
```yaml
name: session-webhook
description: POST session lifecycle events to an external API
events: ["session:*"]
timeout_secs: 8
env:
  WEBHOOK_URL: "https://api.example.com/events"
```

**handler.ts**
```typescript
const data = await Bun.stdin.json();

const url = process.env.WEBHOOK_URL!;
await fetch(url, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    source: "edgecrab",
    event: data.event,
    session_id: data.session_id ?? null,
    timestamp: new Date().toISOString(),
  }),
});
```

---

### Python — LLM usage logger

**HOOK.yaml**
```yaml
name: llm-logger
description: Append LLM usage stats to a local JSONL file
events: ["llm:post"]
```

**handler.py**
```python
import json, sys, pathlib, datetime

data = json.load(sys.stdin)

log_path = pathlib.Path.home() / ".edgecrab" / "llm_usage.jsonl"
entry = {
    "ts": datetime.datetime.utcnow().isoformat(),
    "model": data.get("model"),
    "prompt_tokens": data.get("prompt_tokens", 0),
    "completion_tokens": data.get("completion_tokens", 0),
}
with log_path.open("a") as f:
    f.write(json.dumps(entry) + "\n")
```

---

### Python — Startup check (cancellable)

**HOOK.yaml**
```yaml
name: startup-check
description: Abort gateway startup if required env vars are missing
events: ["gateway:startup"]
priority: 1
```

**handler.py**
```python
import json, sys, os

data = json.load(sys.stdin)

required = ["ANTHROPIC_API_KEY"]
missing = [k for k in required if not os.environ.get(k)]

if missing:
    print(json.dumps({
        "cancel": True,
        "reason": f"Missing required env vars: {', '.join(missing)}"
    }))
```

---

## Slash command: /hooks

Run `/hooks` in the edgecrab TUI to list all loaded hooks, their subscribed
events, priority, and runtime.

---

## Troubleshooting

**Hook not loading?**
- Check `~/.edgecrab/hooks/<name>/HOOK.yaml` exists and is valid YAML.
- Check that `handler.py` / `handler.ts` / `handler.js` exists.
- Make sure `python3` (for Python hooks) or `bun` (for JS/TS hooks) is on
  your `PATH`.
- Set `enabled: false` in HOOK.yaml to temporarily disable without deleting.

**Hook timing out?**
- Increase `timeout_secs:` in HOOK.yaml.
- Check for network issues or blocking I/O in your handler.

**Debugging handler output?**
- Write to stderr — edgecrab captures stdout for the JSON response, but
  stderr is forwarded to the edgecrab debug log.
