# EdgeCrab MCP Specification

> Goal: make EdgeCrab's MCP support operationally stronger than Claude Code's and more integrated than nous-hermes-agent's by treating MCP as a first-class transport, discovery, and operator UX plane across CLI, TUI, ACP, and tool discovery.

## Code Is Law

This specification is grounded in the current implementation, not an imagined greenfield design.

Current code anchors:

- `crates/edgecrab-tools/src/tools/mcp_client.rs`
- `crates/edgecrab-cli/src/app.rs`
- `crates/edgecrab-cli/src/main.rs`
- `crates/edgecrab-cli/src/mcp_catalog.rs`
- `crates/edgecrab-cli/src/doctor.rs`
- `crates/edgecrab-cli/src/runtime.rs`
- `crates/edgecrab-core/src/config.rs`

The spec therefore distinguishes:

1. What EdgeCrab already does.
2. Where the current code leaves operator-visible gaps.
3. What must be implemented next without violating DRY, SOLID, or the current crate boundaries.

## Why EdgeCrab Can Exceed Claude Code and Hermes

### Claude Code baseline

Claude Code is strong at coding ergonomics, but its MCP story is not the product's center of gravity. The EdgeCrab codebase already has structural advantages:

- Runtime MCP tool discovery and dynamic registration via `discover_and_register_mcp_tools()`.
- Both stdio and HTTP JSON-RPC transports in one client.
- Config-backed connection pooling and hot reload.
- TUI-native MCP browsing, install, test, and remove flows.
- CLI, TUI, and ACP-adjacent integration in one binary.

### Hermes baseline

Hermes-agent is the spiritual predecessor, but EdgeCrab is already beyond it in MCP operationalization:

- Rust-native concurrency and process control.
- Centralized config model in `AppConfig.mcp_servers`.
- Preset catalog install flow in `mcp_catalog.rs`.
- TUI overlay support instead of text-only command management.
- Direct integration with the broader tool registry and slash-command shell.

### EdgeCrab target advantage

EdgeCrab should win on three axes:

| Axis | Claude Code | Hermes | EdgeCrab target |
|------|-------------|--------|-----------------|
| Transport coverage | partial / product-dependent | historically stdio-centric | one transport plane for stdio + HTTP with shared diagnostics |
| Operator UX | editor-centric | terminal-text-centric | exceptional TUI + CLI + doctor flow |
| Control plane | tool invocation | tool invocation | discovery, install, probe, diagnose, remove, reload, token lifecycle |

## First Principles

1. MCP is not just a protocol adapter. It is an operational boundary with failure modes.
2. The user should not need to edit YAML blind to understand why an MCP server fails.
3. The TUI must be good enough that MCP setup does not require dropping back to shell commands for routine work.
4. Path handling must be cross-platform by construction. A Windows path with spaces is not an edge case.
5. The config model is the source of truth. UI flows must serialize into it, not around it.

## Current State Summary

EdgeCrab already has:

- Stdio MCP subprocess support.
- HTTP MCP server support with bearer token storage.
- Dynamic MCP tool registration.
- Curated official catalog search and install.
- Multi-source official discovery across the steering-group reference catalog, official integrations, archived upstream entries, and the official MCP Registry.
- TUI selector overlay for configured servers and official entries.
- Native remote `/mcp search` browser with per-source labels and install/view actions.
- Slash commands: `/mcp`, `/reload-mcp`, `/mcp-token`.
- CLI commands: `edgecrab mcp list|refresh|search|view|install|test|doctor|auth|add|remove`.

The current gaps are operational, not foundational:

- TUI `/mcp` parsing is whitespace-fragile for quoted values and Windows-style paths.
- There is no dedicated MCP diagnostic workflow that combines static config analysis with live probing.
- TUI overlay actions are good, but not yet operator-complete.
- Documentation is spread across README, site docs, and feature docs without one code-backed MCP control-plane spec.
- Registry discovery is broader than install support. EdgeCrab currently auto-installs only streamable HTTP, npm stdio, and PyPI stdio registry entries; other registry transports remain view-only.

## Document Map

| Document | Purpose |
|----------|---------|
| [architecture.md](./architecture.md) | Current-state architecture and target design constraints |
| [tui-and-operations.md](./tui-and-operations.md) | CLI/TUI workflows, operator stories, and UX rules |
| [roadblocks.md](./roadblocks.md) | Failure modes, edge cases, and cross-platform risks |
| [adr-001-unified-transport-control-plane.md](./adr-001-unified-transport-control-plane.md) | Why MCP stays unified across stdio and HTTP |
| [adr-002-tui-first-mcp-operator-ux.md](./adr-002-tui-first-mcp-operator-ux.md) | Why MCP is a first-class TUI workflow |
| [adr-003-cross-platform-command-and-path-parsing.md](./adr-003-cross-platform-command-and-path-parsing.md) | Why command parsing must be quote-aware and Windows-safe |

## Implementation Priorities

Priority 1:

- Cross-platform `/mcp` TUI command parsing with quoting support.
- Dedicated MCP doctor flow in CLI and TUI.
- Dedicated MCP auth flow in CLI and TUI with explicit refresh-token next steps.

Priority 2:

- Richer configured-server rendering in the selector and detail views.
- Probe output that distinguishes static misconfiguration from live transport failure.

Priority 3:

- Optional cached health state per configured server.
- Safer reconnect and stale-connection recovery heuristics.
