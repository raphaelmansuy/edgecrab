# ADR-001: Unified MCP Transport Control Plane

Status: accepted

## Context

EdgeCrab supports both stdio and HTTP MCP transports in `mcp_client.rs`.

The temptation is to expose them as different product features. That would be a mistake.

## Decision

EdgeCrab will treat MCP as one control plane with transport-specific adapters behind it.

## Why

1. The config model already unifies transports.
2. Discovery, install, reload, test, and diagnose are conceptually transport-agnostic.
3. Tool discovery and dynamic registration do not care about transport after connection.
4. Splitting the UX would duplicate code and confuse operators.

## Consequences

- One `/mcp` command family.
- One `edgecrab mcp` command family.
- One doctor workflow.
- Transport-specific facts rendered as details, not as separate user journeys.

## References

- `crates/edgecrab-tools/src/tools/mcp_client.rs`
- [architecture.md](./architecture.md)

