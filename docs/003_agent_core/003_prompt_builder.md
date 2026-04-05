# Prompt Builder

Verified against `crates/edgecrab-core/src/prompt_builder.rs`.

`PromptBuilder` assembles the system prompt once per session and the agent caches the result in `SessionState.cached_system_prompt`.

## Source order

```text
identity
  -> platform hint
  -> timestamp
  -> context files
  -> memory guidance
  -> memory content
  -> session-search guidance
  -> skills guidance
  -> skill summary
  -> tool-specific guidance blocks
```

## Context files currently scanned

- global `SOUL.md`
- project `.hermes.md`, `HERMES.md`, `.edgecrab.md`, `EDGECRAB.md`
- `AGENTS.md` in the working tree and subdirectories
- `CLAUDE.md`
- `.cursorrules`
- `.cursor/rules/*.mdc`

`SOUL.md` is treated specially as the identity slot, not as a generic context file.

## Conditional guidance blocks

The builder injects extra instructions only when the relevant tools are present, including:

- memory guidance
- session search guidance
- skills maintenance guidance
- cron scheduling guidance
- message delivery guidance
- image-analysis guidance

## Safety behavior

- prompt-injection checks run before context files and memories are injected
- suspicious files can be blocked and replaced with a placeholder
- skills summary uses a short-lived in-process cache so startup does not rescan the skill tree on every session

## Important rule

Do not rebuild the prompt mid-conversation unless the code path explicitly intends to reset prompt state. The runtime is built around prompt reuse, not prompt churn.
