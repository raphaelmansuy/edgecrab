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

## Discovery Roots

```text
~/.edgecrab/plugins/
.edgecrab/plugins/
/usr/share/edgecrab/plugins/
```

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
- `edgecrab plugins hub-search <query>` searches curated and configured plugin indices
- `hub:<source>/<plugin>` resolves through the configured hub index before install
- Direct `https://...zip` plugin archives are supported in addition to GitHub and local paths

## Host API

Tool-server plugins now use MCP-style newline-delimited JSON-RPC over stdio in both directions.

- Host requests: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`
- Plugin reverse-calls: `host:platform_info`, `host:log`, `host:memory_read`, `host:memory_write`, `host:session_search`, `host:secret_get`, `host:tool_call`

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
edgecrab plugins hub-search github
edgecrab plugins hub-browse
edgecrab plugins hub-refresh
edgecrab plugins remove github-tools
```
