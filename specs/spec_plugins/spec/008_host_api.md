# Host API — Plugin-to-Host Reverse Calls

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [002_adr_transport], [004_plugin_types], [006_security], [007_registry]

---

## 1. Purpose

Plugins are sandboxed subprocesses or scripts.  
They need controlled access to EdgeCrab capabilities (memory, session history, secrets, etc.).

The **Host API** is the controlled set of JSON-RPC 2.0 methods a plugin may call
back on the host over the SAME stdio channel used for tool dispatch.

```
 Host API channel (same stdio as tool dispatch):
 ─────────────────────────────────────────────

  host process                         plugin subprocess
  ────────────                         ─────────────────
  HostApiRouter  ←──── host/request ──  plugin code calls host.call("host:memory_read", ...)
                 ────► host/response ──►
```

This is the **reverse direction** in the JSON-RPC 2.0 channel defined in [002_adr_transport].

---

## 2. Protocol Mechanics

### 2.1 Direction Convention

| Direction | Who sends | JSON-RPC method prefix |
|---|---|---|
| Host → Plugin | host | `tools/list`, `tools/call` |
| Plugin → Host | plugin | `host:*` |

The plugin uses a **client role** when calling host methods.
The host **router** distinguishes host calls from tool results by checking
whether `method` starts with `host:`.

```
reader task in ToolServerPlugin:

    let msg = parse_json(&line)?;
    if msg.has("method") {
        let method = msg["method"].as_str()?;
        if method.starts_with("host:") {
            // Plugin is calling host
            host_api.handle(pid, msg).await?;
        } else {
            // It's a notification from the plugin — ignore or log
        }
    } else if msg.has("result") || msg.has("error") {
        // It's a response to a tool call we issued
        let id = msg["id"].as_u64()?;
        if let Some(tx) = pending.remove(&id) { tx.send(msg); }
    }
```

### 2.2 Plugin Client Pattern (pseudo-code)

```python
# Python plugin example — thin JSON-RPC client
import sys, json

_pending_id = 0

def host_call(method, params):
    global _pending_id
    _pending_id += 1
    req = {"jsonrpc":"2.0","id":f"p{_pending_id}","method":method,"params":params}
    sys.stdout.write(json.dumps(req) + "\n")
    sys.stdout.flush()
    # Read responses until we get a matching id
    while True:
        line = sys.stdin.readline()
        msg = json.loads(line)
        if msg.get("id") == f"p{_pending_id}":
            if "error" in msg:
                raise RuntimeError(msg["error"]["message"])
            return msg["result"]

# Usage
facts = host_call("host:memory_read", {"keys": ["user_name", "user_lang"]})
```

---

## 3. Host API Method Catalog

All methods require the capability declared in `plugin.toml [capabilities]`.
If the plugin issues a request for a method without the required capability,
the host returns `{"code": -32001, "message": "Capability not granted: X"}`.

### 3.1 Memory

#### `host:memory_read`

Read persistent key-value facts from `MEMORY.md` / `USER.md`.

```
Capability: memory:read
Params:     { "keys": ["<key>", ...] }   // empty = read all (max 200 keys)
Result:     { "facts": { "<key>": "<value>" | null, ... } }
```

```json
// Request
{"jsonrpc":"2.0","id":"p1","method":"host:memory_read","params":{"keys":["user_lang"]}}
// Response
{"jsonrpc":"2.0","id":"p1","result":{"facts":{"user_lang":"English"}}}
```

#### `host:memory_write`

Persist a new fact or update an existing one.

```
Capability: memory:write
Params:     { "key": "<key>", "value": "<string>" }
            Limits: key max 128 chars, value max 4096 chars
Result:     { "ok": true }
```

Constraint: value must pass `PluginSecurityScanner::scan_value()` — plain text injection
attempts return `{"code":-32002,"message":"Content blocked: injection pattern detected"}`.

---

### 3.2 Session Search

#### `host:session_search`

Full-text search over past conversation sessions (SQLite FTS5).

```
Capability: session:search
Params:     { "query": "<text>", "limit": 10 }  // limit max 50
Result:     { "hits": [ { "session_id": "...", "excerpt": "...", "score": 0.94 }, ... ] }
```

Note: result excerpts are pre-sanitized — no raw message content with secrets.

---

### 3.3 Secret Store

#### `host:secret_get`

Read a secret from the EdgeCrab secret store (env vars / keychain).

```
Capability: secrets:read
Params:     { "name": "<SECRET_NAME>" }
Result:     { "value": "<redacted_or_actual>" }
```

Rules:
- Only secrets explicitly whitelisted in `plugin.toml [capabilities].secrets` are readable.
- The secret name must match one of the whitelisted names exactly.
- Audit log entry is written for every secret read.

```toml
# plugin.toml
[capabilities]
secrets = ["GITHUB_TOKEN", "JIRA_API_KEY"]
```

```json
// Request
{"jsonrpc":"2.0","id":"p2","method":"host:secret_get","params":{"name":"GITHUB_TOKEN"}}

// Denied (not in whitelist)
{"jsonrpc":"2.0","id":"p2","error":{"code":-32003,"message":"Secret not whitelisted: GITHUB_TOKEN"}}
```

---

### 3.4 Message Injection

#### `host:inject_message`

Inject a synthetic user or assistant message into the conversation.

```
Capability: conversation:inject
Params:     { "role": "user" | "assistant",
              "content": "<text>" }
            Max content length: 32 000 characters
Result:     { "ok": true }
```

**Use-case examples:**
- A plugin that monitors a webhooks and wants to push an incoming event to the agent
- A scheduler plugin injecting "It's time for the daily standup reminder"

**Safety enforcement:**  
Content is scanned for injection patterns before being added to the conversation.
If blocked: `{"code":-32004,"message":"Content blocked: prompt injection detected"}`.

---

### 3.5 Agent Tool Call (Plugin-to-Plugin)

#### `host:tool_call`

A plugin may call a compile-time tool or another plugin's tool via the host.

```
Capability: tool:delegate
Params:     { "tool": "<tool_name>", "args": { ... } }
Result:     { "result": "<tool output string>" }
```

**Reentrancy limit (INV-10):** At most 3 levels of nested `host:tool_call` are allowed.
The host tracks depth per call-stack. At depth 3, the request is rejected:
`{"code":-32005,"message":"Reentrancy limit exceeded (max depth: 3)"}`.

**Loop detection:** A plugin cannot call itself via `host:tool_call`.

---

### 3.6 Platform Context

#### `host:platform_info`

Returns information about the current platform context.

```
Capability: none required (always available)
Params:     {}
Result:     {
              "platform": "cli" | "telegram" | "discord" | ...,
              "session_id": "<uuid>",
              "model": "anthropic/claude-opus-4.6",
              "timestamp_utc": "2026-04-09T12:00:00Z"
            }
```

---

### 3.7 Logging

#### `host:log`

Emit a structured log entry into the host's tracing system.

```
Capability: none required (always available)
Params:     { "level": "trace"|"debug"|"info"|"warn"|"error",
              "message": "<text>",
              "fields": { ... }   // optional extra fields
            }
Result:     { "ok": true }
```

This allows plugin developers to use `tracing::info!()` semantics from any language.
Log lines appear prefixed with `[plugin:<name>]` in EdgeCrab's log output.

---

## 4. HostApiRouter Implementation

```rust
/// Routes host:* method calls from plugin subprocesses.
pub struct HostApiRouter {
    db:          Arc<SessionDb>,
    memory_path: PathBuf,     // ~/.edgecrab/memories/
    secret_store: Arc<SecretStore>,
    plugin_reg:  Weak<dyn PluginRegistry>,   // Weak to avoid cycle
    audit_log:  Arc<AuditLog>,
}

impl HostApiRouter {
    pub async fn handle(
        &self,
        plugin_pid: u32,
        plugin_name: &str,
        manifest:   &PluginManifest,
        request:    serde_json::Value,
    ) -> serde_json::Value {
        let method = request["method"].as_str().unwrap_or("");
        let id     = request["id"].clone();
        let params = request["params"].clone();

        let result = match method {
            "host:memory_read"    => self.mem_read(manifest, params).await,
            "host:memory_write"   => self.mem_write(manifest, params).await,
            "host:session_search" => self.session_search(manifest, params).await,
            "host:secret_get"     => self.secret_get(plugin_name, manifest, params).await,
            "host:inject_message" => self.inject_message(manifest, params).await,
            "host:tool_call"      => self.tool_call(plugin_name, manifest, params, 0).await,
            "host:platform_info"  => self.platform_info().await,
            "host:log"            => self.log(plugin_name, params).await,
            unknown               => Err(HostApiError::UnknownMethod(unknown.into())),
        };

        // Audit every host call
        self.audit_log.record_host_call(plugin_name, method, result.is_ok()).await;

        match result {
            Ok(v)  => rpc_response(id, v),
            Err(e) => rpc_error(id, e.code(), e.message()),
        }
    }
}
```

---

## 5. Capability Declaration (plugin.toml)

```toml
[capabilities]
# Host API capabilities (each enables one or more host:* methods)
memory_read     = true              # host:memory_read
memory_write    = true              # host:memory_write
session_search  = false             # host:session_search
secrets         = ["GITHUB_TOKEN"]  # host:secret_get (only these names)
conversation_inject = false         # host:inject_message
tool_delegate   = false             # host:tool_call
```

If a field is omitted, the default is `false` / empty.
The host validates capabilities at install time (from plugin.toml) and at runtime (per request).

---

## 6. Error Codes

| Code | Name | Meaning |
|---|---|---|
| -32001 | CapabilityNotGranted | Method requires a capability not declared in plugin.toml |
| -32002 | ContentBlocked | Memory write / injection blocked by scanner |
| -32003 | SecretNotWhitelisted | Requested secret not in [capabilities].secrets |
| -32004 | InjectionDetected | Message inject content blocked by injection scanner |
| -32005 | ReentrancyLimit | host:tool_call nesting depth > 3 |
| -32006 | RateLimitExceeded | Plugin issued too many host: calls (> 100/minute) |
| -32007 | Forbidden | Generic authorization failure |

---

## 7. Rate Limiting

A plugin may not flood the host. The HostApiRouter applies per-plugin rate limits:

| Method | Limit | Window |
|---|---|---|
| `host:memory_write` | 60 calls | 1 minute |
| `host:secret_get` | 20 calls | 1 minute |
| `host:inject_message` | 5 calls | 1 minute |
| `host:tool_call` | 30 calls | 1 minute |
| all others | 200 calls | 1 minute |

Exceeding a limit returns error code `-32006`.

Rate limits are token-bucket counters per plugin, reset every minute.
They live in memory (not persisted); a restart clears them.

---

## 8. SDK Stubs

The `edgecrab-plugins` crate exports a Rust SDK for building plugins.
For other languages, thin client stubs are provided:

```
edgecrab-plugins/
  sdk/
    rust/          Rust proc-macro + client (for Rhai is not needed)
    python/        python/edgecrab_plugin/host.py  (<100 lines, zero deps)
    typescript/    ts/src/host.ts  (uses Node.js stdio)
    go/            go/host.go
```

### Python SDK (key surface)

```python
# edgecrab_plugin/host.py
class HostClient:
    def memory_read(self, *keys: str) -> dict: ...
    def memory_write(self, key: str, value: str) -> None: ...
    def secret_get(self, name: str) -> str: ...
    def session_search(self, query: str, limit: int = 10) -> list: ...
    def inject_message(self, role: str, content: str) -> None: ...
    def tool_call(self, tool: str, **kwargs) -> str: ...
    def log(self, level: str, message: str, **fields) -> None: ...
    def platform_info(self) -> dict: ...
```

### TypeScript SDK (key surface)

```typescript
// host.ts
export class HostClient {
    async memoryRead(keys: string[]): Promise<Record<string, string | null>> { ... }
    async memoryWrite(key: string, value: string): Promise<void> { ... }
    async secretGet(name: string): Promise<string> { ... }
    async sessionSearch(query: string, limit?: number): Promise<SearchHit[]> { ... }
    async injectMessage(role: "user" | "assistant", content: string): Promise<void> { ... }
    async toolCall<T>(tool: string, args: Record<string, unknown>): Promise<T> { ... }
    async log(level: LogLevel, message: string, fields?: Record<string, unknown>): Promise<void> { ... }
    async platformInfo(): Promise<PlatformInfo> { ... }
}
```
