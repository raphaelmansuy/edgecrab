---
title: Build A Hermes Plugin
description: Create Hermes-style plugins that run in EdgeCrab without EdgeCrab-specific files.
sidebar:
  order: 4
---

EdgeCrab treats the Hermes plugin guide as the contract. If you build a standard Hermes plugin bundle, EdgeCrab should load it unchanged.

Use this guide when you need runtime registration. If you only need a reusable
instruction bundle with helper files or Python scripts, build a standalone
skill under `~/.edgecrab/skills/` instead of a plugin.

## Layout

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

## `register(ctx)`

```python
from schemas import CALCULATE, UNIT_CONVERT
from tools import calculate, unit_convert


def register(ctx):
    ctx.register_tool("calculate", CALCULATE, calculate)
    ctx.register_tool("unit_convert", UNIT_CONVERT, unit_convert)
```

EdgeCrab accepts the same Hermes-style registration APIs real plugins use:

- `ctx.register_tool(...)`
- `ctx.register_hook(...)`
- `ctx.register_memory_provider(...)`
- `ctx.inject_message(...)`
- `ctx.register_cli_command(...)`

That is the practical boundary:

- standalone skill bundle: prompt guidance plus helper files/scripts
- Hermes plugin: prompt guidance plus runtime registration

## Bundled Skill

If you add `SKILL.md`, EdgeCrab loads it as bundled plugin skill metadata. Use it for prompt guidance, `compatibility`, and `metadata.hermes.related_skills`.

## Install

```bash
edgecrab plugins install ./calculator
edgecrab plugins info calculator
```

## Search Real Hermes Sources

```bash
edgecrab plugins search --source edgecrab calculator
edgecrab plugins search --source edgecrab json
edgecrab plugins search --source hermes holographic
edgecrab plugins search --source hermes-evey telemetry

edgecrab plugins install ./plugins/productivity/calculator
edgecrab plugins install ./plugins/developer/json-toolbox
edgecrab plugins install hub:hermes-plugins/plugins/memory/holographic
edgecrab plugins install hub:hermes-evey/evey-telemetry
```

## Official Repo Examples

This repository ships two Hermes-style examples that are already indexed by the
official plugin search source:

- `plugins/productivity/calculator`
  - tools: `calculate`, `unit_convert`
  - hook: `post_tool_call`
  - bundled skill metadata in `SKILL.md`
- `plugins/developer/json-toolbox`
  - tools: `json_validate`, `json_pointer_get`
  - CLI: `edgecrab json-toolbox pretty|validate`
  - bundled skill metadata in `SKILL.md`

`42-evey/hermes-plugins` uses repo-root Hermes plugin directories. EdgeCrab indexes that layout directly and, for curated GitHub installs, fetches declared shared support files such as `evey_utils.py` when required by the source contract.

## Proven Examples

This repository has real tests covering:

- official repo examples `calculator` and `json-toolbox`
- guide-style `calculator`
- upstream `holographic`
- upstream `honcho` CLI bridging from `cli.py register_cli(subparser)`
- local-bundle compatibility for upstream Hermes optional skills such as `1password`
- `42-evey` plugins `evey-telemetry` and `evey-status`
- pip entry-point plugins discovered from `hermes_agent.plugins`
- gateway session lifecycle parity through reset and shutdown paths
