# Building Hermes-Style Plugins For EdgeCrab

This guide follows the Hermes plugin contract from `website/docs/guides/build-a-hermes-plugin.md` and shows the EdgeCrab-compatible path end to end.

## Minimal Layout

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

EdgeCrab treats this as a Hermes plugin when `plugin.yaml` and `__init__.py` are both present.

If you only need reusable prompt guidance plus bundled helper scripts such as
`scripts/check.py`, do not build a plugin. Use a standalone skill directory in
`~/.edgecrab/skills/` instead. Build a plugin when you need Hermes runtime
registration: tools, hooks, memory providers, or CLI commands.

## `plugin.yaml`

```yaml
name: calculator
version: 1.0.0
description: Math calculator plugin for exact arithmetic and unit conversion
provides_tools:
  - calculate
  - unit_convert
provides_hooks:
  - post_tool_call
```

Keep this declarative. The runtime still trusts `register(ctx)` as the source of truth for actual tool and hook registration.

## `__init__.py`

```python
from schemas import CALCULATE, UNIT_CONVERT
from tools import calculate, unit_convert


def log_post_tool_call(payload, **kwargs):
    with open("calculator-hook.jsonl", "a") as f:
        f.write(str(payload) + "\n")


def register(ctx):
    ctx.register_tool("calculate", CALCULATE, calculate)
    ctx.register_tool("unit_convert", UNIT_CONVERT, unit_convert)
    ctx.register_hook("post_tool_call", log_post_tool_call)
```

EdgeCrab supports the Hermes registration surface that real plugins use:

- `ctx.register_tool(...)`
- `ctx.register_hook(...)`
- `ctx.register_memory_provider(...)`
- `ctx.inject_message(...)`
- `ctx.register_cli_command(...)`

That is the core runtime distinction from a standalone Claude/Hermes-style
skill bundle: standalone skills can point at helper scripts, but only plugins
can register runtime behavior.

## Optional Bundled `SKILL.md`

```yaml
---
name: calculator-skill
description: Guidance for exact arithmetic and unit conversion.
compatibility: Requires calculator plugin
metadata:
  hermes:
    related_skills: [arithmetic-playbook]
---

# Calculator Skill

Use `calculate` for exact arithmetic.
Use `unit_convert` for unit conversions.
```

EdgeCrab loads bundled `SKILL.md` content as plugin skill metadata. That means `compatibility`, readiness, and `metadata.hermes.related_skills` show up in `edgecrab plugins info`.

## Install And Verify

```bash
edgecrab plugins install ./calculator
edgecrab plugins info calculator
edgecrab plugins status
```

Real verification in this repository covers:

- official repo examples `plugins/productivity/calculator` and `plugins/developer/json-toolbox`
- guide-style `calculator`
- upstream Hermes `holographic`
- local-bundle compatibility for upstream Hermes optional skills such as `1password`
- `42-evey` Hermes plugins `evey-telemetry` and `evey-status`
- pip entry-point discovery from `hermes_agent.plugins`

## Search And Install Real Hermes Plugins

```bash
edgecrab plugins search --source edgecrab calculator
edgecrab plugins search --source edgecrab json
edgecrab plugins search --source hermes holographic
edgecrab plugins search --source hermes-evey telemetry

edgecrab plugins install ./plugins/productivity/calculator
edgecrab plugins install ./plugins/developer/json-toolbox
edgecrab plugins install hub:hermes-plugins/plugins/memory/holographic
edgecrab plugins install hub:hermes-evey/evey-telemetry
edgecrab plugins install hub:hermes-evey/evey-status
```

Official repo examples to study:

- `plugins/productivity/calculator`
  - safe arithmetic tool
  - unit conversion tool
  - bundled `SKILL.md`
  - `post_tool_call` hook
- `plugins/developer/json-toolbox`
  - JSON validation tool
  - JSON Pointer lookup tool
  - bundled `SKILL.md`
  - top-level `edgecrab json-toolbox ...` CLI command

`42-evey/hermes-plugins` uses repo-root plugin directories. EdgeCrab indexes that layout directly. For curated GitHub installs from that source, EdgeCrab also materializes declared shared support files such as `evey_utils.py` when the source contract requires them.

## Design Rules

- Keep plugin bundles self-describing: `plugin.yaml` and `SKILL.md` carry metadata, `register(ctx)` carries runtime truth.
- Treat bundled data as regular files under the plugin root. `Path(__file__)` and relative reads are preserved.
- Prefer deterministic repository contracts over implicit guessing. EdgeCrab only auto-resolves repo-root shared support files when the repository identity is explicit through a curated GitHub source.

## Verified Runtime Notes

- Hermes memory-provider bundles can expose top-level EdgeCrab CLI trees through `cli.py` `register_cli(subparser)`.
- Gateway sessions are isolated per chat and have dedicated proof coverage for `on_session_start`, `on_session_end`, `on_session_finalize`, and `on_session_reset`.
