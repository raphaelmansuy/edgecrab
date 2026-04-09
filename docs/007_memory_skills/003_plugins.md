# Plugin System

EdgeCrab now includes a shared plugin runtime in `crates/edgecrab-plugins/`.

## Supported Plugin Kinds

- `skill`: Hermes-compatible `SKILL.md` bundles injected into the system prompt
- `tool-server`: subprocess plugins exposing tools over stdio JSON-RPC
- `script`: Rhai-based local plugins for lightweight tool handlers

## Config

Plugin policy lives in `~/.edgecrab/config.yaml`:

```yaml
plugins:
  enabled: true
  auto_enable: true
  disabled: []
  platform_disabled: {}
  install_dir: ~/.edgecrab/plugins
  quarantine_dir: ~/.edgecrab/plugins/.quarantine
```

## CLI

```bash
edgecrab plugins list
edgecrab plugins info <name>
edgecrab plugins status
edgecrab plugins enable <name>
edgecrab plugins disable <name>
edgecrab plugins toggle <name>
edgecrab plugins install github:owner/repo/path
edgecrab plugins install ./local-plugin
edgecrab plugins audit --lines 20
edgecrab plugins hub-search github
edgecrab plugins hub-refresh
edgecrab plugins remove <name>
```

Plugin installs now flow through quarantine, a static security scan, and an audit log at `~/.edgecrab/plugins/.hub/audit.log`. Disabling a plugin hides it from prompt injection or tool exposure without deleting its files.
