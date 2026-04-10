# ADR-002: Plugin Transport Protocol — Newline-Delimited JSON-RPC 2.0

**Status:** ACCEPTED  
**Date:** 2026-04-09  
**Deciders:** Engineering Team  
**Cross-refs:** [001_adr_architecture], [004_plugin_types], [007_registry], [008_host_api]

---

## Context

Having chosen subprocess-based ToolServer plugins ([001_adr_architecture]), we must
define the precise wire format and framing used between the agent host and plugin processes.

The existing `mcp_client.rs` uses JSON-RPC 2.0 over stdio with newline framing.
We must decide whether to be protocol-compatible with MCP or diverge.

---

## Options Considered

### Option A — Raw protobuf over stdin/stdout

**Pros:** Compact, typed, fast.  
**Cons:** Plugin authors must generate proto stubs; terrible developer experience.
No tooling for debugging (binary wire format). Completely incompatible with MCP ecosystem.

**Verdict: REJECTED.**

### Option B — HTTP/REST over localhost TCP socket

```
edgecrab → POST http://127.0.0.1:{plugin-port}/tools/call → plugin HTTP server
```

**Pros:** Easy to debug with curl; standard HTTP tooling.  
**Cons:**
- Port allocation is a footgun (conflicts, TOCTOU races).
- Plugin must be a network server — adds complexity for simple plugins.
- Requires plugin to bind to a socket BEFORE the agent can call it — startup race.
- HTTP overhead is higher than stdio for short-lived invocations.

**Verdict: REJECTED** — reserved as optional second transport in Phase 2 for plugins
that cannot use stdin/stdout (e.g., long-running daemons).

### Option C — MCP-Compatible JSON-RPC 2.0 over stdio (CHOSEN)

```
Host → Plugin:
{"jsonrpc": "2.0", "id": 1, "method": "tools/call",
 "params": {"name": "my_tool", "arguments": {"param": "value"}}}

Plugin → Host:
{"jsonrpc": "2.0", "id": 1,
 "result": {"content": [{"type": "text", "text": "result"}]}}
```

Each JSON object is terminated by `\n` (newline-delimited JSON, NDJSON).

**Pros:**
- **100% MCP-compatible**: Any MCP server is ALSO a valid ToolServer plugin.
- Reuses `mcp_client.rs` code (DRY).
- Human-readable: debugging with `cat`, `jq`, or VS Code terminal is trivial.
- Rich ecosystem of MCP SDKs (Python, TypeScript, Rust, Go).
- Init handshake (`initialize`, `tools/list`) matches existing patterns.

**Cons:**
- JSON parsing on every call (~0.01ms, negligible).
- Newline framing requires careful buffering across partial reads.

**Verdict: ACCEPTED.**

---

## Protocol Specification

### 3.1 Initialization Handshake

On subprocess start, host sends `initialize` FIRST:

```
Host → Plugin:
{
  "jsonrpc": "2.0",
  "id": 0,
  "method": "initialize",
  "params": {
    "protocolVersion": "2024-11-05",
    "capabilities": {},
    "clientInfo": {
      "name": "edgecrab",
      "version": "0.1.4"
    }
  }
}

Plugin → Host:
{
  "jsonrpc": "2.0",
  "id": 0,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": {
      "tools": {}
    },
    "serverInfo": {
      "name": "my-plugin",
      "version": "1.0.0"
    }
  }
}
```

Then host sends `notifications/initialized` (no response expected):
```json
{"jsonrpc":"2.0","method":"notifications/initialized"}
```

### 3.2 Tool Discovery

```
Host → Plugin:
{"jsonrpc":"2.0","id":1,"method":"tools/list"}

Plugin → Host:
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {
        "name": "my_tool",
        "description": "Does something useful",
        "inputSchema": {
          "type": "object",
          "properties": {
            "param": { "type": "string" }
          },
          "required": ["param"]
        }
      }
    ]
  }
}
```

### 3.3 Tool Invocation

```
Host → Plugin:
{
  "jsonrpc": "2.0",
  "id": 42,
  "method": "tools/call",
  "params": {
    "name": "my_tool",
    "arguments": { "param": "hello" }
  }
}

Plugin → Host (success):
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": {
    "content": [{ "type": "text", "text": "Hello from my_tool!" }],
    "isError": false
  }
}

Plugin → Host (error):
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": {
    "content": [{ "type": "text", "text": "Something went wrong: ..." }],
    "isError": true
  }
}
```

### 3.4 Host-to-Plugin Notifications (Host API calls)

Some plugins need to call back into the host (e.g., to read agent memory).
This is accomplished via a **reverse request pattern** — plugin sends a request
TO the host on stdout, and reads the response on stdin.

```
Plugin → Host (on stdout):
{
  "jsonrpc": "2.0",
  "id": "host-1",
  "method": "host/memory_read",
  "params": { "key": "MEMORY.md" }
}

Host → Plugin (on stdin):
{
  "jsonrpc": "2.0",
  "id": "host-1",
  "result": { "content": "...memory content..." }
}
```

See [008_host_api.md] for the full host function catalog.

### 3.5 Framing Rules

```
┌─────────────────────────────────────────────────┐
│  Framing: each JSON object terminated by \n     │
│                                                 │
│  sender writes:  JSON_BYTES + b'\n'             │
│  receiver reads: line by line (BufReader)       │
│                                                 │
│  Max message size: 4 MiB                        │
│  (plugin sending >4MiB returns ToolError)       │
│                                                 │
│  request id type: integer OR string             │
│  (host uses u64, plugin uses string for         │
│   host-API reverse calls to avoid collision)    │
└─────────────────────────────────────────────────┘
```

### 3.6 Timeout Policy

| Event | Timeout | Action on expiry |
|---|---|---|
| Plugin startup (initialize response) | 10 seconds | Kill plugin, PluginError::StartupTimeout |
| Tool call (tools/call response) | 60 seconds (default) | Kill plugin, PluginError::CallTimeout |
| Plugin shutdown (graceful) | 5 seconds | SIGKILL |

Timeouts are configurable per-plugin in `plugin.toml` (see [003_manifest]).

---

## Consequences

### Positive
- MCP ecosystem compatibility: any MCP server = free plugin with zero extra work.
- Existing `mcp_client.rs` internals can be refactored/shared with plugin transport.
- Human-readable = easier debugging and security auditing.

### Negative
- Bidirectional protocol (plugin calling host back) requires careful ID-space management.
- Partial-read buffering must be tested carefully on slow subprocess output.

### Risk Mitigations
- ID namespace collision: host uses `u64`, plugin uses string-prefixed IDs → disjoint.
- Partial reads: `tokio::io::BufReader::read_line()` handles this correctly.
- Plugin sending invalid JSON: isolate per-message, return PluginError::MalformedResponse.
