# Execution Backends

Verified against:
- `crates/edgecrab-tools/src/tools/backends/mod.rs`
- `crates/edgecrab-tools/src/tools/terminal.rs`
- `crates/edgecrab-tools/src/tools/process.rs`

In the current Rust workspace, "environments" means execution backends for terminal-style work, not a separate RL environment subsystem.

## Supported backend kinds

- `local`
- `docker`
- `ssh`
- `modal`
- `daytona`
- `singularity`

## Dispatch model

```text
terminal-style tool call
  -> read configured BackendKind
  -> get or create backend instance
  -> execute command
  -> redact output
  -> return formatted stdout/stderr/exit status
```

## Backend responsibilities

- `local`: persistent local shell with environment passthrough controls
- `docker`: isolated command execution through the Docker API
- `ssh`: remote command execution over SSH
- `modal`: remote execution through Modal
- `daytona`: remote workspace execution through Daytona
- `singularity`: command execution through Apptainer or Singularity

## Shared behavior

- commands receive a cancellation token
- output is truncated and redacted before it reaches the model
- dangerous commands can require approval
- backend instances can be cached so sequential tool calls reuse the same shell or remote session

## What is not documented here

This page intentionally does not claim benchmark or training environments that are not represented in the current crate tree.
