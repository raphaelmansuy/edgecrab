# Memory and Skills

Verified against:
- `crates/edgecrab-core/src/prompt_builder.rs`
- `crates/edgecrab-tools/src/tools/memory.rs`
- `crates/edgecrab-tools/src/tools/skills.rs`
- `crates/edgecrab-tools/src/tools/honcho.rs`

EdgeCrab has three related context systems:

- durable memory files
- reusable skills
- optional Honcho-backed profile/context tools

## Memory

The prompt builder loads memory sections from the EdgeCrab home directory:

- `MEMORY.md`
- `USER.md`

These are intended for durable facts, not transient task progress. The builder also injects memory-specific guidance when the relevant tools are enabled.

## Skills

Skills live as directories containing `SKILL.md`. The skills system supports:

- listing and viewing skills
- create, edit, patch, and delete through `skill_manage`
- category and description extraction from frontmatter
- extra linked files through `read_files`
- optional external skill directories from config

## Prompt-time behavior

```text
session starts
  -> memory sections loaded
  -> skills summary loaded
  -> tool-specific guidance injected
  -> selected preloaded skills can be added to the session prompt
```

## Safety behavior

- memory writes are scanned for injection patterns before persistence
- skill mutations can invalidate the in-process skills prompt cache
- missing external skill directories are skipped quietly

## Honcho integration

The Honcho tools are separate from file-backed memory. They provide:

- `honcho_profile`
- `honcho_context`
- `honcho_search`
- `honcho_list`
- `honcho_remove`
- `honcho_conclude`

Use the file-backed memory system for prompt-injected durable notes and the Honcho tools for the external memory service integration.
