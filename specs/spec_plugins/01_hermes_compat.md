# Hermes Plugin Compatibility Contract

Source of truth: upstream Hermes Agent plugin runtime and docs, especially:

- `hermes_cli/plugins.py`
- `website/docs/user-guide/features/plugins.md`
- `website/docs/user-guide/features/hooks.md`
- `website/docs/guides/build-a-hermes-plugin.md`

## Directory Shape

Hermes-compatible plugins are Python directory plugins:

```text
<plugin>/
├── plugin.yaml | plugin.yml
└── __init__.py
```

`__init__.py` must expose `register(ctx)`.

## Manifest Surface

Required and commonly used fields from `plugin.yaml`:

- `name`
- `version`
- `description`
- `author`
- `provides_tools`
- `provides_hooks`
- `requires_env`

## Runtime Contract

`register(ctx)` may:

- `register_tool(...)`
- `register_hook(...)`
- `register_cli_command(...)`
- `inject_message(...)`

For EdgeCrab compatibility, the minimum supported Hermes behaviors are:

- discover user and project plugin directories
- load `plugin.yaml` + `__init__.py register(ctx)`
- respect `requires_env` readiness gating from `plugin.yaml`
- expose Hermes-registered tools to the agent runtime
- invoke `on_session_start`
- invoke `pre_llm_call`
- accept `pre_llm_call` return values as ephemeral user-message context injection
- support CLI-safe `inject_message`

## Hook Semantics

Hermes defines these plugin hooks:

- `pre_tool_call`
- `post_tool_call`
- `pre_llm_call`
- `post_llm_call`
- `pre_api_request`
- `post_api_request`
- `on_session_start`
- `on_session_end`
- `on_session_finalize`
- `on_session_reset`

Current EdgeCrab parity target in this phase:

- `requires_env` setup-needed gating
- `on_session_start`
- `pre_llm_call`

`pre_llm_call` is the only hook whose return value affects prompt flow. If a callback returns a string, or a dict containing `context`, that text is appended to the current turn's user message rather than mutating the system prompt.

## Discovery Rules

Hermes source behavior:

- user plugins: `~/.hermes/plugins/`
- project plugins: `./.hermes/plugins/` gated by `HERMES_ENABLE_PROJECT_PLUGINS`

EdgeCrab compatibility behavior:

- keep native discovery roots under `~/.edgecrab/plugins/` and `./.edgecrab/plugins/`
- additionally discover Hermes roots so existing Hermes plugins can run without repackaging

## Operator UX

To keep Hermes-compatible plugins practical to adopt, the CLI must support:

- remote plugin search from the normal `plugins` command surface
- explicit source selection for Hermes-oriented searches
- install-ready `hub:<source>/<plugin>` references in search results

## Non-goals For This Phase

- full pip entry-point plugin loading parity
- Hermes CLI subcommand registration parity
- all Hermes hook types
- replacing EdgeCrab-native `skill`, `tool-server`, or `script` plugins

This spec supersedes the earlier study doc as the compatibility baseline. `00_study.md` should be treated as example ideas, not the precise contract.
