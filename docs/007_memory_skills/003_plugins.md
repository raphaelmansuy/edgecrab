# Plugin System

EdgeCrab now includes a shared plugin runtime in `crates/edgecrab-plugins/`.

## Plugins vs Skills

First-principles distinction:

- A `skill` is reusable prompt guidance.
- A `plugin` is a runtime extension bundle that EdgeCrab discovers and manages.

Implications:

- Skills are the right primitive for instructions, checklists, examples, and
  repeatable workflows.
- Plugins are the right primitive for code, tools, hooks, subprocesses,
  readiness checks, trust metadata, and install/update lifecycle.
- A plugin may bundle a `SKILL.md`, but that skill content is still part of a
  plugin-managed bundle.
- Standalone skills can still bundle helper files and scripts. That does not
  make them plugins; it means the skill can point the agent at those files
  through the normal tool surfaces.
- Claude Code-style standalone skills that bundle Python or shell helper
  scripts are still skills, not plugins, unless they also need runtime
  registration, hooks, or install/audit lifecycle.

Examples:

- `~/.edgecrab/skills/release/SKILL.md` is a standalone skill.
- `~/.edgecrab/plugins/release-helper/plugin.toml` is a plugin.
- `~/.edgecrab/plugins/calculator/plugin.yaml` with `__init__.py` is a Hermes plugin.

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

Plugin config controls plugin lifecycle. It does not replace the separate
`skills:` config block used for standalone skills in `~/.edgecrab/skills/`.

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

Curated Hermes-oriented sources now include:

- `edgecrab-official` for the official EdgeCrab repo plugin examples under `plugins/`
- `hermes-plugins` for `NousResearch/hermes-agent`
- `hermes-evey` for `42-evey/hermes-plugins`

Hermes-compatible plugins are also discovered from legacy roots:

- `~/.hermes/plugins/`
- `./.hermes/plugins/` when `HERMES_ENABLE_PROJECT_PLUGINS=true`

Hermes `requires_env` declarations are honored during discovery. Missing variables move the plugin to `setup-needed`, and non-available plugins are not exposed as runtime tools.

Tool exposure is live in the console:

- enabled plugin tools appear in `/tools` under the `plugins` toolset
- disabling a plugin removes those tools from the active registry immediately
- re-enabling a plugin restores them without restarting the session

Minimal verification flow:

```text
/plugins
/tools
/plugins disable calculator
/tools
/plugins enable calculator
/tools
```

Tool-server plugins now speak MCP-compatible newline-delimited JSON-RPC:

- host -> plugin: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`
- plugin -> host: `host:platform_info`, `host:log`, `host:memory_read`, `host:memory_write`, `host:session_search`, `host:secret_get`, `host:inject_message`, `host:tool_call`

Hermes-compatible hook parity currently includes:

- `pre_tool_call`
- `post_tool_call`
- `on_session_start`
- `pre_llm_call` with ephemeral user-message context injection
- `post_llm_call`
- `pre_api_request`
- `post_api_request`
- `on_session_end`
- `on_session_finalize`
- `on_session_reset`

EdgeCrab's Hermes Python bridge now ships the minimal runtime shims real upstream
Hermes plugins expect:

- `agent.memory_provider.MemoryProvider`
- `tools.registry.tool_error`
- `hermes_constants.get_hermes_home()` / `display_hermes_home()`
- namespace-package wiring for `plugins.*` imports from the Hermes repo tree

Claude-style standalone skill bundles are also supported separately from the
plugin runtime:

- `Base directory for this skill: ...` rendering in `skill_view` and preloaded skills
- `${CLAUDE_SKILL_DIR}` and `${CLAUDE_SESSION_ID}` substitution
- helper-file discovery from `references/`, `templates/`, `scripts/`, and `assets/`
- metadata parsing for `when_to_use`, `arguments`, `argument-hint`, `allowed-tools`,
  `user-invocable`, `disable-model-invocation`, `context`, and `shell`

Current non-parity boundary:

- EdgeCrab does not auto-execute Claude prompt-shell blocks from skill text.
- EdgeCrab does not automatically fork a dedicated Claude-style skill sub-agent.

Hermes skill compatibility now preserves additional metadata fields during load:

- top-level `compatibility`
- `metadata.hermes.related_skills`
- `metadata.hermes.category`

Hermes local installs now accept raw upstream bundle layouts without requiring authors
to add `plugin.toml`:

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

Examples:

```bash
edgecrab plugins install ./calculator
edgecrab plugins info calculator

edgecrab plugins search --source edgecrab calculator
edgecrab plugins search --source edgecrab json
edgecrab plugins install ./plugins/productivity/calculator
edgecrab plugins install ./plugins/developer/json-toolbox
edgecrab plugins info json-toolbox

edgecrab plugins install ~/src/hermes-agent/plugins/memory/holographic
edgecrab plugins info holographic

EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab plugins list
EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab entry-demo status

edgecrab plugins search --source hermes-evey telemetry
edgecrab plugins install hub:hermes-evey/evey-telemetry
edgecrab plugins install hub:hermes-evey/evey-status
```

Remote plugin search is now plugin-only. Hermes standalone skills such as
`1password` belong in the remote skills browser:

```bash
edgecrab skills search 1password
edgecrab skills install hermes-agent:security/1password
```

Bundled `SKILL.md` files inside Hermes plugin roots are now loaded as plugin skills, so
their `compatibility`, `related_skills`, and readiness state are surfaced through normal
plugin discovery and `/plugins info`.

For a step-by-step authoring tutorial, see
`docs/007_memory_skills/005_building_hermes_style_plugins.md`.

Disabling a plugin hides it from prompt injection or tool exposure without deleting its files.

Verified compatibility coverage now includes:

- official repo Hermes examples `calculator` and `json-toolbox`, including official-search visibility plus local install/runtime proof
- guide-style Hermes plugin install + tool execution + `post_tool_call` hook via CLI E2E
- actual Hermes plugins from `NousResearch/hermes-agent` (`honcho`, `holographic`)
- actual Hermes optional-skill bundle compatibility from `NousResearch/hermes-agent` (`github-issues`, `1password`)
- actual Hermes plugins from `42-evey/hermes-plugins` (`evey-telemetry`, `evey-status`)
- pip entry-point plugin discovery + CLI command dispatch via E2E
- Hermes memory-provider `cli.py register_cli(subparser)` bridging, including real `honcho` CLI invocation
- Hermes hub indexing for upstream `plugins/...` directories and repo-root Hermes plugin directories in the plugin browser
- gateway per-chat session isolation plus `on_session_start`, `on_session_end`, `on_session_finalize`, and `on_session_reset` proof in gateway tests

Verification:

```bash
cargo test -p edgecrab-plugins hermes_plugin_loads_bundled_skill_metadata -- --nocapture
cargo test -p edgecrab-plugins cached_hermes_repo_index_includes_python_plugin_directories -- --nocapture
cargo test -p edgecrab-core api_call_with_retry_invokes_hermes_api_hooks -- --nocapture
cargo test -p edgecrab-core session_boundary_hooks_fire_on_new_and_finalize -- --nocapture
cargo test -p edgecrab-cli --test plugins_e2e -- --nocapture
```
