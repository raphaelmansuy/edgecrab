# Plugin System

EdgeCrab now includes a shared plugin runtime in `crates/edgecrab-plugins/`.

## Supported Plugin Kinds

- `skill`: Hermes-compatible `SKILL.md` bundles injected into the system prompt
- `tool-server`: subprocess plugins exposing tools over stdio JSON-RPC
- `script`: Rhai-based local plugins for lightweight tool handlers
- `hermes`: Python directory plugins with `plugin.yaml` + `__init__.py register(ctx)` compatibility

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
edgecrab plugins toggle [name]
edgecrab plugins install github:owner/repo/path
edgecrab plugins install hub:community/github-tools
edgecrab plugins install https://example.com/plugin.zip
edgecrab plugins install ./local-plugin
edgecrab plugins audit --lines 20
edgecrab plugins search github
edgecrab plugins search --source hermes weather
edgecrab plugins browse
edgecrab plugins refresh
edgecrab plugins remove <name>
```

Plugin installs now flow through quarantine, a static security scan, trust assignment, checksum stamping in `plugin.toml`, and an audit log at `~/.edgecrab/plugins/.hub/audit.log`.

Remote search now lives on the main plugin command surface. `edgecrab plugins search` supports `--source hermes` and prints install-ready `hub:<source>/<plugin>` targets so Hermes-compatible registries are discoverable without remembering hub internals.

Hermes-compatible plugins are also discovered from legacy roots:

- `~/.hermes/plugins/`
- `./.hermes/plugins/` when `HERMES_ENABLE_PROJECT_PLUGINS=true`

Hermes `requires_env` declarations are honored during discovery. Missing variables move the plugin to `setup-needed`, and non-available plugins are not exposed as runtime tools.

Tool-server plugins now speak MCP-compatible newline-delimited JSON-RPC:

- host -> plugin: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`
- plugin -> host: `host:platform_info`, `host:log`, `host:memory_read`, `host:memory_write`, `host:session_search`, `host:secret_get`, `host:inject_message`, `host:tool_call`

Hermes-compatible hook parity currently includes:

- `on_session_start`
- `pre_llm_call` with ephemeral user-message context injection

Disabling a plugin hides it from prompt injection or tool exposure without deleting its files.
