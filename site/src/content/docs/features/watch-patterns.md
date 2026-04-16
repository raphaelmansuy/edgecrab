---
title: Watch Patterns
description: Monitor terminal output for specific patterns during background process execution. Grounded in crates/edgecrab-tools/src/tools/terminal.rs.
sidebar:
  order: 12
---

Watch patterns allow the agent to monitor background process output for specific text patterns and receive notifications when they appear. This is useful for detecting build completions, error messages, or service readiness indicators.

---

## How It Works

When using the `terminal` tool with `background: true`, the agent can specify `watch_patterns` — a list of regex patterns to watch for in the process output.

```json
{
  "command": "cargo build --release 2>&1",
  "background": true,
  "watch_patterns": ["error\\[", "warning\\[", "Finished"]
}
```

When any pattern matches, the agent receives a notification with the matched line and pattern.

---

## Security

Watch patterns are **blocked in the `execute_code` sandbox**. The `TERMINAL_BLOCKED_PARAMS` list in `execute_code.rs` includes `watch_patterns` alongside `background`, `check_interval`, and `pty` — preventing sandboxed code from spawning monitored background processes.

---

## Use Cases

- **Build monitoring**: Watch for `error` or `Finished` patterns during compilation
- **Server readiness**: Watch for `listening on port` when starting a dev server
- **Test completion**: Watch for `passed` or `failed` in test runner output
- **Log monitoring**: Watch for specific error codes or exception types
