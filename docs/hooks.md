# 🦀 Hooks

> **WHY**: Operators and platform adapters often need custom logic at specific lifecycle points — audit logging, policy enforcement, auto-resume gates, completion reviews, or message/session controls — without modifying the core runtime. Hooks are the extension point across both the CLI/TUI and the gateway.

**Source**: `crates/edgecrab-gateway/src/hooks.rs`

---

## Two Hook Types

```text
┌───────────────────────────────────────────────┐
│                  HookRegistry                  │
│                                                │
│  ┌──────────────────┐  ┌─────────────────────┐ │
│  │  Native hooks    │  │  Script hooks        │ │
│  │  (Rust structs)  │  │  (.py / .js / .ts)  │ │
│  │  impl GatewayHook│  │  discovered from     │ │
│  │                  │  │  ~/.edgecrab/hooks/  │ │
│  └──────────────────┘  └─────────────────────┘ │
└───────────────────────────────────────────────┘
          │                         │
          ▼                         ▼
    HookResult::Continue    HookResult::Cancel
    HookResult::Cancel { reason }
```

**Native hooks** are Rust structs compiled into the binary — lowest latency, type-safe, access to all internal types.

**Script hooks** are loaded from disk at startup — zero recompile required, writable in Python, JavaScript, or TypeScript.

---

## Core Types

```rust
/// Passed to every hook invocation
pub struct HookContext {
    pub event: String,           // e.g. "session:start", "tool:pre"
    pub session_id: Option<String>,
    pub user_id: Option<String>,
    pub platform: Option<String>,
    pub fields: serde_json::Map<String, Value>, // event-specific payload
}

/// Hook return value — controls whether processing continues
pub enum HookResult {
    Continue,                    // let the event proceed
    Cancel { reason: String },   // abort the event with a reason
}

/// Trait all native hooks implement
pub trait GatewayHook: Send + Sync {
    fn events(&self) -> &[&str];             // which events this hook handles
    async fn handle(&self, ctx: HookContext) -> HookResult;
}

/// Parsed HOOK.yaml manifest
pub struct HookManifest {
    pub name: String,
    pub events: Vec<String>,     // event patterns this hook subscribes to
    pub language: String,        // "python" | "javascript" | "typescript"
    pub handler: String,         // filename: "handler.py", "handler.js"…
}
```

---

## Script Hook Layout

```text
~/.edgecrab/hooks/
└── my-audit-hook/
    ├── HOOK.yaml         ← manifest
    └── handler.py        ← or handler.js / handler.ts
```

### `HOOK.yaml` Format

```yaml
name: my-audit-hook
events:
  - session:start
  - session:end
  - tool:pre
language: python
handler: handler.py
```

### `handler.py` Contract

```python
import json, sys

def handle(ctx: dict) -> dict:
    """
    ctx keys: event, session_id, user_id, platform, fields
    Return: {"action": "continue"} or {"action": "cancel", "reason": "..."}
    """
    event = ctx["event"]

    if event == "session:start":
        # audit log
        with open("/var/log/edgecrab-audit.log", "a") as f:
            f.write(json.dumps(ctx) + "\n")

    return {"action": "continue"}


if __name__ == "__main__":
    ctx = json.loads(sys.stdin.read())
    result = handle(ctx)
    print(json.dumps(result))
```

Python hooks run via `python3`. JavaScript/TypeScript hooks run via `bun`.

Use `/hooks` in the TUI to inspect loaded hooks, see which events they subscribe to, and reload the registry after edits.

### `handler.ts` Contract (TypeScript via Bun)

```typescript
import { readFileSync } from "fs";

interface HookContext {
  event: string;
  session_id?: string;
  user_id?: string;
  platform?: string;
  fields: Record<string, unknown>;
}

function handle(ctx: HookContext): { action: "continue" | "cancel"; reason?: string } {
  if (ctx.event === "tool:pre" && ctx.fields.tool_name === "bash") {
    const cmd = ctx.fields.command as string;
    if (cmd.includes("rm -rf /")) {
      return { action: "cancel", reason: "Destructive command blocked by hook" };
    }
  }
  return { action: "continue" };
}

const ctx = JSON.parse(readFileSync("/dev/stdin", "utf8")) as HookContext;
console.log(JSON.stringify(handle(ctx)));
```

---

## Event Catalogue

### Gateway Lifecycle

| Event | Fires when | Key `fields` |
| --- | --- | --- |
| `gateway:startup` | Gateway process starts | `platform`, `adapter_version` |
| `session:start` | New session created | `source`, `user_id` |
| `session:end` | Session ends normally | `turn_count`, `total_tokens` |
| `session:reset` | `/reset` slash command | `session_id` |

### Agent Lifecycle

| Event | Fires when | Key `fields` |
| --- | --- | --- |
| `agent:start` | Agent begins processing a turn | `model`, `toolset`, `message` |
| `agent:step` | Each ReAct loop iteration | `iteration`, `tool_name` (if tool call) |
| `agent:end` | Agent finishes a turn | `iterations`, `tokens` |
| `agent:run_finished` | A run reaches a terminal harness outcome | `completion_state`, `exit_reason`, `summary` |
| `agent:done` | Final lifecycle notification after a run ends | `completion_state`, `exit_reason`, `summary` |
| `agent:stop` | Final stop-review gate before a run is accepted | `completion_state`, `exit_reason`, `summary`, `active_tasks`, `blocked_tasks` |
| `agent:task_completed` | Run completed successfully | `summary` |
| `agent:task_blocked` | Run ended blocked or awaiting user input | `completion_state`, `summary` |
| `agent:needs_input` | Run needs clarification from the user | `completion_state`, `summary` |
| `agent:needs_verification` | Run lacks fresh evidence for completion | `summary`, `evidence_count` |
| `agent:task_incomplete` | Run stopped with work still pending | `summary`, `active_tasks`, `blocked_tasks` |

### Tool Events

| Event | Fires when | Key `fields` |
| --- | --- | --- |
| `tool:pre` | Before tool execution | `tool_name`, `arguments` |
| `tool:post` | After tool execution | `tool_name`, `success`, `output_bytes` |

### LLM Events

| Event | Fires when | Key `fields` |
| --- | --- | --- |
| `llm:pre` | Before sending request to provider | `model`, `message_count`, `prompt_tokens_est` |
| `llm:post` | After receiving response | `model`, `finish_reason`, `tokens` |

### CLI Events

| Event | Fires when | Key `fields` |
| --- | --- | --- |
| `cli:start` | CLI process starts | `args` |
| `cli:end` | CLI process exits | `exit_code` |

### Command Events

| Pattern | Fires when |
| --- | --- |
| `command:*` | Any slash command (`/reset`, `/memory`, `/skills`…) |
| `command:reset` | Specifically `/reset` |

---

## Event Matching

The `HookRegistry` supports three matching modes:

```text
Exact match:    "session:start"  → only that event
Prefix wildcard: "command:*"     → any event starting with "command:"
Global wildcard: "*"             → every event (use sparingly)
```

A hook can subscribe to multiple patterns:

```yaml
events:
  - session:start
  - session:end
  - command:*
```

---

## Hook Execution Order

When an event fires:

```text
event fires
     │
     ▼
collect all matching hooks (native + script)
     │
     ▼
execute in registration order
     │
     ├── HookResult::Continue → next hook
     │
     └── HookResult::Cancel { reason }
              │
              ▼
         event aborted
         reason returned to caller
```

A single `Cancel` short-circuits the rest of the hook chain.

In live behavior, `command:*`, `agent:start`, and `agent:stop` are especially useful because they can block or defer execution with a human-readable reason.

---

## Native Hook Example

```rust
use edgecrab_gateway::hooks::{GatewayHook, HookContext, HookResult};

pub struct RateLimitHook {
    max_sessions_per_minute: u32,
}

impl GatewayHook for RateLimitHook {
    fn events(&self) -> &[&str] {
        &["session:start"]
    }

    async fn handle(&self, ctx: HookContext) -> HookResult {
        let user_id = ctx.user_id.as_deref().unwrap_or("anonymous");
        if self.over_limit(user_id) {
            return HookResult::Cancel {
                reason: format!("Rate limit exceeded for user {user_id}"),
            };
        }
        HookResult::Continue
    }
}
```

Register it when building the gateway:

```rust
gateway_builder.register_hook(Box::new(RateLimitHook { max_sessions_per_minute: 10 }));
```

---

## Important Caveat

The hook system is **gateway-owned**. It fires for sessions arriving through platform adapters (Telegram, Discord, CLI-as-gateway, etc.). It does **not** provide a general core-runtime extension point. For compile-time tool registration, see [`inventory::submit!`](004_tools_system/001_tool_registry.md).

---

## Tips

- **Use `Cancel` sparingly** — a hook that cancels `agent:step` on every iteration will silently prevent all tool use. Test hook cancellation thoroughly.
- **Script hooks run as a subprocess** — there is I/O serialisation overhead. For hot paths (`agent:step`, `llm:pre`), prefer native Rust hooks.
- **`fields` is schemaless** — the exact keys depend on the event. Log `ctx` in development to discover the full payload for a given event.
- **Python hooks need `python3` on `PATH`** — if the gateway runs in a minimal container, ensure `python3` is available or use `bun`-based TypeScript hooks instead.

---

## FAQ

**Q: Can a hook modify the message before it reaches the agent?**
A: Not directly via `HookResult` — the current API is Continue/Cancel only. For message transformation, use a native Rust `PlatformAdapter` middleware or a gateway-level interceptor.

**Q: Do hooks run in parallel?**
A: No. Hooks for a given event run sequentially in registration order. This keeps the Cancel semantics deterministic.

**Q: Can I ship a hook as part of a skill?**
A: Not currently. Hooks live in `~/.edgecrab/hooks/` and are gateway-level. Skills live in `~/.edgecrab/skills/` and are agent-level.

---

## Cross-References

- Gateway architecture (where hooks fire) → [`006_gateway/001_gateway_architecture.md`](006_gateway/001_gateway_architecture.md)
- Platform adapters (source of gateway events) → [`006_gateway/001_gateway_architecture.md`](006_gateway/001_gateway_architecture.md)
- Skills (agent-level extension, not gateway-level) → [`007_memory_skills/002_creating_skills.md`](007_memory_skills/002_creating_skills.md)
- Hooks discovery path config → [`009_config_state/001_config_state.md`](009_config_state/001_config_state.md)
