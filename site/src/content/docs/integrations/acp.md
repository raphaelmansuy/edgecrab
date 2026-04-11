---
title: ACP / VS Code Copilot
description: Use EdgeCrab as a VS Code GitHub Copilot agent via the Agent Communication Protocol (ACP). JSON-RPC 2.0 stdio adapter setup, manifest configuration, and testing.
sidebar:
  order: 1
---

EdgeCrab implements the [Agent Communication Protocol](https://github.com/i-am-bee/acp) (ACP) — a JSON-RPC 2.0 stdio interface that lets it register as a VS Code GitHub Copilot agent, or integrate with any ACP-compatible agent runner.

---

## How ACP Works

ACP is a simple protocol: the host (VS Code Copilot) launches EdgeCrab as a subprocess and communicates via JSON-RPC 2.0 messages over stdin/stdout:

```
VS Code Copilot
      │   (launches subprocess)
      ▼
  edgecrab acp
      │
      ▼  JSON-RPC 2.0 over stdin/stdout
      │
  edgecrab-acp crate
      │
      ▼
  edgecrab-core (ReAct loop)
```

---

## Setting Up in VS Code

### Prerequisites

- VS Code with GitHub Copilot extension installed
- EdgeCrab installed and configured (`edgecrab doctor` should pass)

### 1. Generate Workspace-Local ACP Files

Run this from the workspace you want VS Code to use:

```bash
edgecrab acp init
```

This creates:

- `.edgecrab/acp_registry/agent.json`
- `.vscode/settings.json`

The generated settings point VS Code at a workspace-local ACP registry, so you
do not need to hard-code the path to your EdgeCrab checkout.

### 2. Inspect the Generated Manifest

The generated agent manifest looks like this:

```json
{
  "name": "edgecrab",
  "description": "EdgeCrab — an ACP-compatible agent with file, terminal, and skill tools. Launched as a child process and communicates via JSON-RPC 2.0 over stdio.",
  "version": "<current-version>",
  "launch": {
    "type": "command",
    "command": "edgecrab acp"
  },
  "protocol": {
    "transport": "stdio",
    "format": "jsonrpc2"
  },
  "capabilities": {
    "session": {
      "fork": true,
      "list": true
    },
    "approval": true,
    "streaming": true
  }
}
```

### 3. Configure VS Code

`edgecrab acp init` writes the VS Code entry automatically. The generated
settings look like this:

```json
{
  "acpClient.agents": [
    {
      "name": "edgecrab",
      "registryDir": "/absolute/path/to/workspace/.edgecrab/acp_registry"
    }
  ]
}
```

If your workspace already has `.vscode/settings.json`, EdgeCrab preserves the
other settings and only upserts the `edgecrab` ACP entry.

### 4. Use in Copilot Chat

In the Copilot Chat panel, type `@edgecrab` to route your request to EdgeCrab:

```
@edgecrab fix all failing tests in src/
@edgecrab refactor the authentication module to use JWT
@edgecrab write a benchmark for the parser
```

---

## Starting the ACP Server Manually

For testing or integration with other ACP hosts:

```bash
edgecrab acp
# Listening on stdin/stdout for ACP messages...
```

EdgeCrab reads JSON-RPC 2.0 requests from stdin and writes responses to stdout. Stderr is used for logging.

### Testing with a Raw Message

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"agent/chat","params":{"messages":[{"role":"user","content":"hello"}]}}' | edgecrab acp
```

---

## ACP Capabilities

EdgeCrab's ACP adapter exposes all core capabilities:

| Capability | Supported |
|------------|-----------|
| Single-turn chat | ✓ |
| Multi-turn conversation | ✓ |
| Streaming tokens | ✓ |
| Tool calls (file, terminal, web, etc.) | ✓ |
| Session persistence | ✓ |
| Memory injection | ✓ |
| Skill loading | ✓ |

---

## Security in ACP Mode

When running in ACP mode, EdgeCrab applies all the same security controls as in
interactive mode. The editor workspace becomes the session root, file tools can
only read and write inside the workspace plus configured `tools.file.allowed_roots`,
and deny-list restrictions still win over both. The ACP host does not bypass any
security layer.

Tool calls are logged to stderr (for debugging), not to the ACP response stream.

---

## Troubleshooting

### `edgecrab acp` exits immediately

Check that `edgecrab doctor` passes and your API keys are set. The ACP server requires a working LLM provider.

### VS Code doesn't show EdgeCrab as an agent

Run `edgecrab acp init` from the workspace again, then verify that `edgecrab`
is in `$PATH` (`which edgecrab`). Restart or reload VS Code after updating
settings.

### Tool calls fail with permission errors

Run `edgecrab doctor` to check allowed roots and tool configuration. The VS Code workspace directory is always allowed automatically; `tools.file.allowed_roots` only adds extra locations outside that workspace.

---

## Pro Tips

- **Use `@edgecrab` without a profile** first — the default `helpful` personality works well for most VS Code tasks.
- **Prefix with the directory**: `@edgecrab Fix all failing tests in src/auth/` is faster than `@edgecrab fix tests` because the agent can skip the discovery phase.
- **Combine with skills**: Drop a `skills/` directory into your workspace root. EdgeCrab picks it up automatically in ACP mode, giving the agent custom workflows tuned to your project.
- **Send SIGINT (Ctrl-C in terminal)** to cancel an in-flight ACP request without restarting VS Code.
- **Test the raw protocol** with the `echo | edgecrab acp` pattern before debugging VS Code config issues — it confirms the binary itself is working.

---

## FAQ

**Does ACP mode save conversation history?**
Yes. Each VS Code workspace gets its own session in `~/.edgecrab/state.db`. Conversation history persists across VS Code restarts.

**Can I use a custom model in ACP mode?**
Set `EDGECRAB_MODEL=openai/gpt-4o` or configure `model` in `~/.edgecrab/config.yaml`. ACP mode inherits all config settings.

**Does the agent have access to the file system in ACP mode?**
Yes — the same security policy applies. The workspace root is always allowed, and `tools.file.allowed_roots` can extend access to extra locations. Files outside that effective policy are blocked.

**Is ACP mode compatible with remote development (SSH, Dev Containers)?**
Yes, as long as `edgecrab` is installed on the remote host and is in `$PATH` as seen by VS Code.

---

## See Also

- [Security Model](/user-guide/security/) — same rules apply in ACP mode
- [Skills System](/features/skills/) — project-specific skill files loaded automatically
- [Configuration Reference](/reference/configuration/) — tune model, iterations, and toolsets
