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
- load pip entry-point plugins from the `hermes_agent.plugins` group
- support `ctx.register_cli_command(...)`
- support memory-provider CLI trees from `cli.py register_cli(subparser)`
- support the full Hermes hook set used by `VALID_HOOKS`

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

Current EdgeCrab parity:

- `requires_env` setup-needed gating
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
- official EdgeCrab repo examples indexed from the repository `plugins/` tree
- explicit source selection for Hermes-oriented searches
- install-ready `hub:<source>/<plugin>` references in search results

Implementation proof now includes:

- official repo examples `plugins/productivity/calculator` and `plugins/developer/json-toolbox`
- local end-to-end install and runtime execution for those examples
- official search visibility for those examples through `edgecrab plugins search --source edgecrab ...`

## Non-goals

- automatic execution of Claude Code prompt-shell blocks embedded in standalone skills
- automatic Claude-style forked skill-agent invocation from skill frontmatter
- replacing EdgeCrab-native `skill`, `tool-server`, or `script` plugins

This spec supersedes the earlier study doc as the compatibility baseline. `00_study.md` should be treated as example ideas, not the precise contract.
