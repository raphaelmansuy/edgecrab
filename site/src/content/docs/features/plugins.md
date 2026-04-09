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

## CLI

```bash
edgecrab plugins list
edgecrab plugins info github-tools
edgecrab plugins status
edgecrab plugins enable github-tools
edgecrab plugins disable github-tools
edgecrab plugins toggle github-tools
edgecrab plugins install owner/repo
edgecrab plugins remove github-tools
```
