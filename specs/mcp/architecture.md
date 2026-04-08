# EdgeCrab MCP Architecture

## 1. System View

Today, EdgeCrab's MCP support is already split into the right layers:

```text
┌────────────────────────────────────────────────────────────────────────────┐
│ edgecrab-core / edgecrab-cli                                              │
│                                                                            │
│  TUI `/mcp` and CLI `edgecrab mcp ...`                                     │
│     │                                                                      │
│     ├── config mutation (`AppConfig.mcp_servers`)                          │
│     ├── catalog workflows (`mcp_catalog.rs`)                               │
│     ├── registry bootstrap (`runtime.rs`)                                  │
│     └── diagnostics / probe UX                                             │
└───────────────────────────────┬────────────────────────────────────────────┘
                                │
                                ▼
┌────────────────────────────────────────────────────────────────────────────┐
│ edgecrab-tools / tools/mcp_client.rs                                       │
│                                                                            │
│  load_mcp_config()                                                         │
│  configured_servers()                                                      │
│  get_or_connect()                                                          │
│  probe_configured_server()                                                 │
│  discover_and_register_mcp_tools()                                         │
│                                                                            │
│  Transports:                                                               │
│  - stdio subprocess JSON-RPC                                               │
│  - HTTP JSON-RPC                                                           │
└───────────────────────────────┬────────────────────────────────────────────┘
                                │
                    JSON-RPC 2.0 over stdio / HTTP
                                │
                                ▼
┌────────────────────────────────────────────────────────────────────────────┐
│ External MCP servers                                                       │
└────────────────────────────────────────────────────────────────────────────┘
```

This is the correct high-level direction. The problem is not missing architecture. The problem is incomplete operational closure.

## 2. Current Code Truth

### Transport plane

`crates/edgecrab-tools/src/tools/mcp_client.rs` already provides:

- `HttpMcpConnection`
- `McpConnection` for stdio
- `McpConnectionKind` as the unifying transport enum
- `MCP_CONNECTIONS: OnceLock<DashMap<String, Mutex<McpConnectionKind>>>`

This means the code already supports the core architectural decision from [ADR-001](./adr-001-unified-transport-control-plane.md): one MCP control plane, multiple transports.

### Config plane

`crates/edgecrab-core/src/config.rs` defines:

- `AppConfig.mcp_servers: HashMap<String, McpServerConfig>`
- `McpServerConfig` with `command`, `args`, `cwd`, `url`, `headers`, `bearer_token`, `timeout`, `connect_timeout`, and `tools`

This is already the right persistence boundary. UI flows must write here and diagnostics must explain this state.

### UX plane

`crates/edgecrab-cli/src/app.rs` already contains:

- `open_mcp_selector()`
- `handle_mcp_command()`
- `render_mcp_selector()`
- key bindings for install, test, view, and remove

`crates/edgecrab-cli/src/main.rs` already contains the non-TUI CLI equivalents.

### Discovery plane

`crates/edgecrab-cli/src/runtime.rs` calls `discover_and_register_mcp_tools()` during tool registry construction.

This is the key EdgeCrab differentiator: configured MCP servers are not merely manually callable, they can become first-class tools in the agent's active tool surface.

## 3. Design Rule: Keep MCP Unified

Do not split EdgeCrab into separate HTTP-MCP and stdio-MCP user experiences.

Reasons:

1. The config model is already unified.
2. The tool registry does not care about transport; it cares about server capability.
3. The operator problem is the same across transports: discover, connect, diagnose, recover.
4. DRY is violated if each transport gets separate command, doctor, and rendering logic.

## 4. Design Rule: Keep Diagnostics Above the Transport

MCP failures happen at multiple layers:

1. Static config invalid.
2. Command not resolvable.
3. `cwd` missing or not a directory.
4. HTTP auth not configured.
5. MCP handshake fails.
6. Server connects but exposes zero tools.
7. Configured filters hide all tools.

Transport-specific code should report facts. Operator-facing doctor flows should interpret them.

That means:

- `mcp_client.rs` remains the transport and discovery substrate.
- CLI/TUI should own user-facing diagnosis orchestration.
- Shared CLI/TUI MCP helpers should render and normalize results once.

## 5. Target Architecture Delta

```text
Current:
  TUI/CLI handlers
      ├── parse ad hoc
      ├── mutate config
      └── call probe directly

Target:
  Shared MCP support module
      ├── quoted token parser
      ├── option extraction
      ├── static configured-server diagnosis
      ├── async live probe rendering
      └── shared output formatting for CLI + TUI
```

Benefits:

- DRY: one parser, one doctor formatter.
- SOLID: app/main stay thin, support module owns MCP operator concerns.
- Testability: parser and diagnostics become unit-testable without full TUI event loops.

## 6. What "Exceptional TUI MCP Support" Means

It does not mean adding visual noise.

It means the TUI is complete enough that an operator can:

1. Browse configured and official entries.
2. Install a preset.
3. View exact config and transport details.
4. Test a server.
5. Diagnose why a server is unhealthy.
6. Remove or reload it.
7. Do all of the above with quoted paths and names that survive real-world shells and Windows paths.

That is stronger than a simple searchable list. It is an MCP operations console.

