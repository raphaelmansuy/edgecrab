# Example Hermes Plugins

This repository ships small Hermes-style plugin examples under `plugins/`.

- `plugins/productivity/calculator`
  - Safe arithmetic, unit conversion, bundled `SKILL.md`, and a `post_tool_call` hook.
- `plugins/developer/json-toolbox`
  - JSON validation, JSON Pointer lookup, bundled `SKILL.md`, and a top-level plugin CLI command.

These examples are intentionally dependency-free and are indexed by the curated
`edgecrab-official` plugin search source.

Try them locally:

```bash
edgecrab plugins install ./plugins/productivity/calculator
edgecrab plugins install ./plugins/developer/json-toolbox

edgecrab plugins search --source edgecrab calculator
edgecrab plugins search --source edgecrab json
```
