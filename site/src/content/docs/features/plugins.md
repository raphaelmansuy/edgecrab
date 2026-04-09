---
title: Plugin System
description: EdgeCrab plugin runtime for prompt skills, JSON-RPC tool servers, and Rhai script plugins.
sidebar:
  order: 5
---

EdgeCrab can now discover plugins from `~/.edgecrab/plugins/` and fold them into the same runtime used by built-in tools.

## Kinds

- `skill`: injects `SKILL.md` content into the system prompt
- `tool-server`: proxies subprocess tools over newline-delimited JSON-RPC
- `script`: loads Rhai code for lightweight local extension logic
- `hermes`: loads Hermes-style Python plugins from `plugin.yaml` + `__init__.py`

## Discovery Roots

```text
~/.edgecrab/plugins/
.edgecrab/plugins/
~/.hermes/plugins/
/usr/share/edgecrab/plugins/
```

When `HERMES_ENABLE_PROJECT_PLUGINS=true`, EdgeCrab also discovers `./.hermes/plugins/` for project-local Hermes compatibility.

## Config

```yaml
plugins:
  enabled: true
  auto_enable: true
  disabled: []
  platform_disabled: {}
  install_dir: ~/.edgecrab/plugins
```

Plugin disable state is persistent. Disabled plugins stay installed on disk but are excluded from prompt injection and tool dispatch.

## Security and Hub

- Installs are staged in `~/.edgecrab/plugins/.quarantine/`
- Every install is statically scanned before activation
- Installed manifests are stamped with trust metadata and a directory checksum
- Install and remove events are appended to `~/.edgecrab/plugins/.hub/audit.log`
- `edgecrab plugins search <query>` searches curated and configured plugin indices
- `edgecrab plugins search --source hermes <query>` targets Hermes-oriented registries directly
- `hub:<source>/<plugin>` resolves through the configured hub index before install
- Direct `https://...zip` plugin archives are supported in addition to GitHub and local paths

## Host API

Tool-server plugins now use MCP-style newline-delimited JSON-RPC over stdio in both directions.

- Host requests: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`
- Plugin reverse-calls: `host:platform_info`, `host:log`, `host:memory_read`, `host:memory_write`, `host:session_search`, `host:secret_get`, `host:inject_message`, `host:tool_call`

## Hermes Compatibility

EdgeCrab now recognizes Hermes directory plugins with:

```text
plugin.yaml
__init__.py
```

and a `register(ctx)` function. Hermes-compatible hook support currently covers:

- `requires_env` readiness gating to `setup-needed`
- `on_session_start`
- `pre_llm_call`

`pre_llm_call` results are appended to the current user message, preserving the system-prompt cache behavior used by Hermes. Plugins that are disabled or `setup-needed` are not exposed as runtime tools.

## CLI

```bash
edgecrab plugins list
edgecrab plugins info github-tools
edgecrab plugins status
edgecrab plugins enable github-tools
edgecrab plugins disable github-tools
edgecrab plugins toggle [github-tools]
edgecrab plugins install github:edgecrab/plugins/github-tools
edgecrab plugins install hub:community/github-tools
edgecrab plugins install https://example.com/github-tools.zip
edgecrab plugins install ./plugins/github-tools
edgecrab plugins audit --lines 20
edgecrab plugins search github
edgecrab plugins search --source hermes weather
edgecrab plugins browse
edgecrab plugins refresh
edgecrab plugins remove github-tools
```
