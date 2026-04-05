# Tool Registry

Verified against:
- `crates/edgecrab-tools/src/registry.rs`
- `crates/edgecrab-tools/src/tools/mod.rs`

EdgeCrab uses compile-time tool registration through `inventory`, then resolves tool availability and toolset filtering at runtime.

## Registry shape

```text
+-----------------------------+
| tool implementation         |
+-----------------------------+
               |
               v
+-----------------------------+
| implements ToolHandler      |
+-----------------------------+
               |
               v
+-----------------------------+
| inventory::submit!          |
+-----------------------------+
               |
               v
+-----------------------------+
| ToolRegistry loads handlers |
+-----------------------------+
               |
               v
+-----------------------------+
| definitions are filtered    |
| by toolset and availability |
+-----------------------------+
               |
               v
+-----------------------------+
| dispatch executes tool      |
+-----------------------------+
```

## `ToolHandler` contract

Every tool provides:

- `name()`
- `toolset()`
- `schema()`
- `execute(args, ctx)`

Optional hooks:

- `is_available()` for startup-time capability checks
- `check_fn()` for per-request gating
- `parallel_safe()` for concurrency hints
- `emoji()` for UI display

## `ToolContext`

The shared execution context can carry:

- working directory
- session id
- cancellation token
- app config snapshot
- optional `SessionDb`
- platform
- optional `ProcessTable`
- optional provider and tool registry for delegation
- clarify and approval channels
- optional gateway sender and origin chat
- optional per-session todo store

## Current scale

- 65 tools are registered through `inventory::submit!`
- the core tool surface currently exposes the same 65 names through `CORE_TOOLS`
- the ACP subset exposes 54 tools

## Runtime behavior

- fuzzy matching can suggest a likely tool when the model asks for a bad name
- availability is not only static; some tools are visible but runtime-gated
- tool errors are returned as structured payloads so the model can recover
