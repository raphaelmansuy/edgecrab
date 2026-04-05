# Tools Runtime

Verified against:
- `crates/edgecrab-tools/src/registry.rs`
- `crates/edgecrab-tools/src/process_table.rs`
- `crates/edgecrab-tools/src/tools/terminal.rs`
- `crates/edgecrab-tools/src/tools/backends/mod.rs`

Tool execution is stateful. The runtime passes more than JSON arguments into a tool call.

## Execution flow

```text
model emits tool call
  -> registry resolves handler
  -> ToolContext is built
  -> tool runs
  -> result or ToolError is serialized
  -> tool message is appended to the conversation
```

## Shared runtime objects

- `ToolContext`: per-call view of session state and runtime services
- `ProcessTable`: per-agent table for background jobs and persistent shell sessions
- backend cache in `terminal.rs`: reuses execution backends per task

## Execution backends used by terminal-style tools

- `local`
- `docker`
- `ssh`
- `modal`
- `daytona`
- `singularity`

## Operational details

- `ProcessTable` uses `DashMap` plus inner mutexes so long-running process records can be updated concurrently.
- A GC task cleans up expired finished processes.
- Output redaction is part of backend/runtime handling, not only presentation.
- Dangerous command approval is interactive when approval channels exist and otherwise fails closed.

## Rule of thumb

If you change terminal behavior, check both the tool implementation and the backend abstraction. A lot of the real behavior lives in the backend layer, not in `terminal.rs` alone.
