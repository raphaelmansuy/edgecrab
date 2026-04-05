---
title: Context Files
description: How EdgeCrab auto-loads SOUL.md, AGENTS.md, and other context files into the system prompt. Grounded in crates/edgecrab-core/src/context.rs.
sidebar:
  order: 7
---

Context files tell EdgeCrab who it is and how to behave. They are injected into the system prompt at session start — before any user message.

---

## How Context Loading Works

At each session start, EdgeCrab scans the following paths in order and injects any files it finds:

1. `~/.edgecrab/SOUL.md` — Global agent identity
2. `~/.edgecrab/AGENTS.md` — Global project-agnostic instructions  
3. `AGENTS.md` in the current working directory — Project-specific instructions
4. `AGENTS.md` traversed up from CWD (like git, stops at filesystem root)
5. All files in `~/.edgecrab/memories/` — Persistent memory

### Injection Order (in system prompt)

```
[1] SOUL.md (identity/persona)
[2] ~/.edgecrab/AGENTS.md (global instructions)
[3] ./AGENTS.md (project instructions, if present)
[4] Memory files (persistent facts)
```

---

## SOUL.md — Agent Identity

`~/.edgecrab/SOUL.md` defines the agent's core personality and directives. It is the first thing in every system prompt.

Example:

```markdown
# EdgeCrab Agent Identity

You are an expert Rust and TypeScript software engineer.
You write clean, idiomatic, well-tested code.
You always run tests before declaring a task complete.
You prefer explicit error handling over panics.
You explain your reasoning concisely — no filler text.
```

Edit it:

```bash
edgecrab config edit-soul      # opens SOUL.md in $EDITOR
# or directly:
$EDITOR ~/.edgecrab/SOUL.md
```

---

## AGENTS.md — Project Instructions

Place `AGENTS.md` in your project root to give EdgeCrab project-specific context:

```markdown
# Project: my-rust-api

## Build
cargo build --workspace

## Test
cargo test --workspace -- --nocapture

## Code Style
- All public APIs must have doc comments
- Use `thiserror` for error types
- Prefer `Arc<Mutex<T>>` over raw pointers

## Architecture
Services are in `crates/*/`, shared types in `crates/types/`.
The HTTP layer uses Axum 0.8.
```

EdgeCrab reads this automatically when it starts in or navigates into `my-rust-api/`.

---

## Skipping Context Files

Disable for one session:

```bash
edgecrab --skip-context-files "ignore SOUL.md and AGENTS.md"
```

Or via environment variable:

```bash
EDGECRAB_SKIP_CONTEXT_FILES=1 edgecrab "task"
```

---

## Profile Context Files

Each [profile](/user-guide/profiles/) has its own `SOUL.md`:

```
~/.edgecrab/profiles/work/SOUL.md
~/.edgecrab/profiles/personal/SOUL.md
```

Profile files override the global `~/.edgecrab/SOUL.md` when the profile is active.

---

## Pro Tips

**Commit AGENTS.md to your repo.** It gets auto-loaded by anyone using EdgeCrab (or Hermes Agent) in that project. Put build commands, test commands, code style rules, and architecture notes there.

**SOUL.md is your agent's personality.** Keep it short (< 500 tokens) and focused on behaviors, not facts. Facts belong in `memories/`. Example persona:
```markdown
You are a senior Rust engineer. You write clean, idiomatic Rust.
You run cargo test before calling any task complete.
You explain your reasoning but skip pleasantries.
```

**Use AGENTS.md hierarchy for monorepos.** Place a top-level AGENTS.md with repo-wide rules, then crate-level AGENTS.md files with local rules. EdgeCrab traverses up from CWD, loading all it finds.

---

## Frequently Asked Questions

**Q: How do I verify what context files are being loaded?**

Check at startup — EdgeCrab logs which files it found. Or ask the agent: "Which AGENTS.md and memory files are currently loaded in your context?"

**Q: Why is my `AGENTS.md` not being loaded?**

Common causes:
1. The file is named `agents.md` (lowercase) — it must be `AGENTS.md` (exact case)
2. You're running `edgecrab` in a directory that doesn't have an AGENTS.md and no parent does either
3. `--skip-context-files` flag is active

**Q: Can project AGENTS.md override security settings?**

No. AGENTS.md is injected into the LLM context (as instructions to the model), but it cannot override compiled-in security rules (path safety, SSRF). It can affect agent *behavior* but not *security constraints*.

**Q: What's the token cost of context files?**

SOUL.md + AGENTS.md + memory files are all injected at the start of every session. Keep them concise. As a rough guide: 100 lines of Markdown ≈ 500-800 tokens. If your context files grow large, split them — the agent reads all files in `memories/`, so you can spread content across multiple focused files.

**Q: Can I use environment variables in context files?**

No — context files are injected as-is into the system prompt. For dynamic content, use memory tools during the session instead.

---

## See Also

- [Memory](/features/memory/) — Persistent facts vs. context file instructions
- [Skills](/features/skills/) — Procedural instructions loaded at session start
- [Profiles](/user-guide/profiles/) — Profile-scoped SOUL.md and memories
- [Configuration](/user-guide/configuration/) — `skip_context_files` and related options
