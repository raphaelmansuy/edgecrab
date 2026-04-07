# ADR-004 — Language Server Process Lifecycle

**Status**: Accepted  
**Date**: 2025

---

## Context

Language servers are child processes. They can crash, hang, fail to start, or not be installed.
The lifecycle strategy directly affects agent reliability.

---

## Requirements

1. A server must be started lazily (on first use for a language) — not on EdgeCrab startup
2. Crashes must not crash EdgeCrab
3. The agent must receive a clear error when a server is unavailable (not a hang)
4. Servers must be restarted automatically up to a limit
5. Restart attempts must use exponential backoff — prevent tight crash loops
6. On `edgecrab` process exit, all child servers must be gracefully shut down

---

## Design

### State machine (formalized)

```
  ┌─────────┐  first request  ┌─────────────┐  initialize ok  ┌───────┐
  │  Absent │ ──────────────► │Initializing │ ──────────────► │ Ready │
  └─────────┘                  └─────────────┘                 └───┬───┘
      ▲                             │                               │
      │ manual restart              │ init timeout (30s)            │ crash / unexpected exit
      │ or config reload            ▼                               ▼
      │                         ┌──────────┐              ┌──────────────┐
      │                         │  Failed  │◄─────────────│  Restarting  │
      └─────────────────────────│ (give up)│  max attempts └──────────────┘
                                └──────────┘  exceeded
```

### Restart policy

```rust
pub struct RestartPolicy {
    pub max_restarts: u32,        // default: 5
    pub initial_delay: Duration,  // default: 1s
    pub backoff_factor: f32,      // default: 2.0
    pub max_delay: Duration,      // default: 60s
}

impl RestartPolicy {
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let secs = self.initial_delay.as_secs_f32()
            * self.backoff_factor.powi(attempt as i32);
        Duration::from_secs_f32(secs.min(self.max_delay.as_secs_f32()))
    }
}
// attempt 0 → 1s, 1 → 2s, 2 → 4s, 3 → 8s, 4 → 16s, 5 → Failed
```

### Initialization timeout

`textDocument/initialize` must respond within 30 seconds. Use `tokio::time::timeout`.
If it exceeds the budget, the child process is killed and the state transitions to `Failed`.

```rust
let init_result = tokio::time::timeout(
    Duration::from_secs(30),
    socket.request::<Initialize>(init_params),
).await
.map_err(|_| LspError::InitTimeout)?
.map_err(LspError::Protocol)?;
```

### Process monitoring

The `async-lsp` `MainLoop` task completes when the child exits. We watch its `JoinHandle`:

```rust
// Spawn a watcher task per server
tokio::spawn(async move {
    let _ = join_handle.await;
    // notify the restart channel
    let _ = restart_tx.send(lang_id).await;
});
```

The restart channel is consumed by a background tokio task in `LspServerManager::run()`.

### Graceful shutdown

On `Drop` of `LspServerManager` (or on explicit `shutdown()`):

```rust
pub async fn shutdown(&self) {
    for entry in self.servers.iter() {
        let state = entry.value().lock().await;
        if let Some(socket) = &state.server_socket {
            // LSP shutdown sequence: shutdown request → exit notification
            let _ = socket.request::<Shutdown>(()).await;
            let _ = socket.notify::<Exit>(());
        }
    }
}
```

---

## Server Discovery

Language servers must be configured, not auto-discovered. This prevents arbitrary binary
execution from model-controlled file arguments.

```yaml
# ~/.edgecrab/config.yaml
lsp:
  servers:
    rust:
      command: rust-analyzer
      args: []
    typescript:
      command: typescript-language-server
      args: ["--stdio"]
    python:
      command: pylsp
      args: []
```

If a server is not configured for a language, tools return:
```json
{ "supported": false, "reason": "No LSP server configured for language 'go'. Add one in ~/.edgecrab/config.yaml under lsp.servers.go" }
```

---

## Consequences

- `LspServerManager::shutdown()` must be called at EdgeCrab exit (add to cleanup handler in `edgecrab-cli/main.rs`)
- Zombie processes are prevented by `tokio::process::Command` ownership — child is killed when `Child` is dropped
- Server stderr is redirected to `null` in production; can be redirected to a file via `lsp.debug_stderr: true` in config for debugging
