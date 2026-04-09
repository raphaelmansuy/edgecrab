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

Standalone skills remain separate from plugins. A skill in `~/.edgecrab/skills/`
can bundle helper files and scripts, but a plugin is still the installable
runtime unit with its own lifecycle, trust metadata, and enable/disable policy.

That distinction matters for Claude-style skill bundles too:

- A standalone skill may bundle Python helpers under `scripts/`.
- EdgeCrab renders `${CLAUDE_SKILL_DIR}` and `${CLAUDE_SESSION_ID}` for those skills.
- Those helper scripts still run through normal tools such as `run_terminal`.
- Automatic prompt-shell execution and Claude-only fork semantics are not part
  of the plugin runtime and are not auto-executed.

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

That runtime policy is reflected live in the TUI:

- enabled plugin tools appear in `/tools` under the `plugins` toolset
- disabled plugins disappear from the active tool inventory without a restart
- re-enabling a plugin re-registers its tools in the same session

Example:

```text
/plugins
/tools
/plugins disable calculator
/tools
/plugins enable calculator
/tools
```

Plugin search also uses layered caches under `~/.edgecrab/plugins/.hub/cache/`:

- source indexes are cached with a TTL
- repo-backed plugin tree scans are cached separately from descriptions
- per-plugin descriptions are cached so repeated searches do not refetch remote metadata
- stale cache is reused when a refresh fails, which keeps the remote browser usable during transient network or GitHub failures

## Security and Hub

- Installs are staged in `~/.edgecrab/plugins/.quarantine/`
- Every install is statically scanned before activation
- Installed manifests are stamped with trust metadata and a directory checksum
- Install and remove events are appended to `~/.edgecrab/plugins/.hub/audit.log`
- `edgecrab plugins search <query>` searches curated and configured plugin indices
- `edgecrab plugins search --source edgecrab <query>` targets the official EdgeCrab repo examples directly
- `edgecrab plugins search --source hermes <query>` targets Hermes-oriented registries directly
- `edgecrab plugins search --source hermes-evey <query>` targets the curated `42-evey/hermes-plugins` catalog
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
- `pre_tool_call`
- `post_tool_call`
- `on_session_start`
- `pre_llm_call`
- `post_llm_call`
- `pre_api_request`
- `post_api_request`
- `on_session_end`
- `on_session_finalize`
- `on_session_reset`

`pre_llm_call` results are appended to the current user message, preserving the system-prompt cache behavior used by Hermes. Plugins that are disabled or `setup-needed` are not exposed as runtime tools.

The Hermes Python bridge now provides the minimal runtime shims real upstream
Hermes plugins expect:

- `agent.memory_provider`
- `tools.registry.tool_error`
- `hermes_constants`
- `plugins.*` namespace-package imports for repo-backed plugin trees

Compatibility is verified against real upstream `NousResearch/hermes-agent`
assets, including the `holographic` and `honcho` plugins plus local-bundle
compatibility checks for Hermes optional skills such as `github-issues` and
`1password`.

The official EdgeCrab repo also ships Hermes-format examples under `plugins/`:

- `plugins/productivity/calculator`
- `plugins/developer/json-toolbox`

Those examples are indexed by `edgecrab-official`, so they show up in normal
plugin search and can be installed directly from a local checkout.

Compatibility is also verified against real plugins from
`42-evey/hermes-plugins`, including `evey-telemetry` and `evey-status`.

Hermes plugin roots can now be installed directly from the upstream guide layout:

```text
calculator/
├── plugin.yaml
├── __init__.py
├── schemas.py
├── tools.py
├── SKILL.md
└── data/
    └── units.json
```

```bash
edgecrab plugins search --source edgecrab calculator
edgecrab plugins search --source edgecrab json
edgecrab plugins install ./plugins/productivity/calculator
edgecrab plugins install ./plugins/developer/json-toolbox

edgecrab plugins install ./calculator
edgecrab plugins info calculator

edgecrab plugins install ~/src/hermes-agent/plugins/memory/holographic
edgecrab plugins info holographic

edgecrab plugins search --source hermes-evey telemetry
edgecrab plugins install hub:hermes-evey/evey-telemetry
edgecrab plugins install hub:hermes-evey/evey-status

EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab plugins list
EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab entry-demo status
```

Remote plugin search only shows plugin-capable artifacts. Hermes standalone
skills such as `1password` belong in the remote skills browser:

```bash
edgecrab skills search 1password
edgecrab skills install hermes-agent:security/1password
```

Bundled `SKILL.md` files inside Hermes plugin directories are loaded as plugin skills, so
their readiness, `compatibility`, and `related_skills` metadata appear in discovery and
`edgecrab plugins info`.

For a full authoring walkthrough, see the [Build A Hermes Plugin](/docs/guides/build-hermes-plugin/) guide.

Verified runtime coverage includes the Hermes memory-provider `cli.py register_cli(subparser)`
convention and gateway session lifecycle parity across reset and shutdown flows.

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

The same plugin state is visible in the TUI Tool Manager:

- `/tools` shows only tools that are currently registered
- plugin tools are marked as dynamic and grouped under the `plugins` toolset
- toggling a plugin through `/plugins`, the plugin toggle overlay, install, update, or remove refreshes that tool inventory immediately
