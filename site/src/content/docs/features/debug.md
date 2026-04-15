---
title: Debug & Dump
description: Diagnostic dump for EdgeCrab sessions, configuration, and API key status. Grounded in crates/edgecrab-cli/src/dump_cmd.rs.
sidebar:
  order: 15
---

The `/dump` (aliased as `/debug`) slash command produces a comprehensive diagnostic snapshot of the current EdgeCrab session state.

---

## Usage

In the TUI:

```
/dump          # full diagnostic dump
/debug         # alias for /dump
```

---

## What's Included

The dump output includes:

| Section | Contents |
|---------|----------|
| **Session** | Session ID, title, message count, start time |
| **Model** | Current model, provider, context window, token usage |
| **Config** | Loaded config values, overrides from env vars |
| **API Keys** | Status of all known API keys (present/missing) |
| **Tools** | Enabled toolsets, active tool count, MCP servers |
| **Memory** | Loaded memory files, total size |
| **Platform** | Current platform, gateway status |

---

## API Key Redaction

API key values are redacted by default. With `show_keys` enabled, keys display in `first4****last4` format:

```
ANTHROPIC_API_KEY: sk-a****xyz3 ✓
OPENAI_API_KEY:    (not set)
GITHUB_TOKEN:      ghp_****ab12 ✓
```

---

## Plain-Text Output

Dump output is intentionally plain text (no ANSI escape codes) so it can be copy-pasted into bug reports or support requests without formatting artifacts.

---

## Programmatic Access

The dump is also available via the CLI subcommand:

```bash
edgecrab dump              # print to stdout
edgecrab dump --show-keys  # include redacted key values
```
