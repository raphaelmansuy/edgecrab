---
title: Profiles
description: Named profiles with isolated home directories — separate config, env, SOUL.md, memory, skills, and state per profile. Grounded in crates/edgecrab-cli/src/profile.rs.
sidebar:
  order: 9
---

Profiles let you run EdgeCrab with completely isolated configurations. Each profile has its own `config.yaml`, `.env`, `SOUL.md`, memories, skills, and state database — making it easy to maintain separate agent identities for work, personal use, different clients, or different projects.

---

## Profile Directory Structure

Each profile lives under `~/.edgecrab/profiles/<name>/`:

```
~/.edgecrab/profiles/
├── work/
│   ├── config.yaml       # Work-specific model, toolsets, etc.
│   ├── .env              # Work API keys
│   ├── SOUL.md           # Work agent identity
│   ├── memories/         # Work-specific memory
│   ├── skills/           # Work-specific skills
│   └── state.db          # Work sessions database
├── personal/
│   ├── config.yaml
│   ├── .env
│   ├── SOUL.md
│   └── ...
└── client-acme/
    └── ...
```

The active profile is tracked in `~/.edgecrab/.active_profile`. Shell aliases are created at `~/.local/bin/<profile_name>` as thin wrappers for `edgecrab -p <name>`.

---

## Managing Profiles

### List Profiles

```bash
edgecrab profile list
```

Output:
```
  default   (built-in)
* work      ~/.edgecrab/profiles/work/
  personal  ~/.edgecrab/profiles/personal/
```

`*` marks the active profile.

### Create a Profile

```bash
edgecrab profile create work
edgecrab profile create client-acme --clone work   # clone from an existing profile
```

This creates the profile directory with default config files. Edit them to customize:

```bash
edgecrab -p work config edit    # edit work profile config
```

### Switch Active Profile

```bash
edgecrab profile use work
```

All subsequent `edgecrab` invocations use the work profile until you switch again.

### Delete a Profile

```bash
edgecrab profile delete client-acme
```

This removes the entire `~/.edgecrab/profiles/client-acme/` directory permanently.

### Show Profile Info

```bash
edgecrab profile show            # show active profile
edgecrab profile show work       # show a specific profile
edgecrab profile path            # print active profile home path
edgecrab profile path work       # print a specific profile's path
```

---

## Running Under a Profile

Use `-p` / `--profile` to run EdgeCrab under a specific profile without switching the active profile:

```bash
edgecrab -p work "open a PR for the auth refactor"
edgecrab -p personal "help me plan my vacation"
edgecrab -p client-acme -S deploy-aws "deploy the staging environment"
```

---

## Shell Aliases

When you create a profile, EdgeCrab registers a shell alias at `~/.local/bin/<name>` (if that directory is in `$PATH`). This lets you invoke profiles directly:

```bash
# After: edgecrab profile create work
work "open a PR for the auth refactor"

# After: edgecrab profile create personal
personal "what should I make for dinner?"
```

The alias is a thin wrapper:

```bash
#!/bin/bash
exec edgecrab -p work "$@"
```

---

## Profile-Specific SOUL.md

Each profile can have a different agent identity. Edit `~/.edgecrab/profiles/<name>/SOUL.md` to define the persona:

```markdown
# Work Profile Agent

You are a professional software engineering assistant. You work on production Rust
systems. Be concise, precise, and always reference actual code. Never mock
implementations — only write or suggest code that actually compiles and works.
```

```markdown
# Personal Profile Agent

You are a helpful personal assistant. Help with task planning, research,
writing, cooking, and life organization. Be warm and conversational.
```

---

## Profile Isolation

Profiles are fully isolated:

| Resource | Isolated per profile? |
|----------|-----------------------|
| `config.yaml` | ✅ |
| `.env` (API keys) | ✅ |
| `SOUL.md` (identity) | ✅ |
| `memories/` | ✅ |
| `skills/` | ✅ |
| `state.db` (sessions) | ✅ |
| Binary / version | ❌ (shared) |
| Cron jobs | ❌ (shared `~/.edgecrab/cron/`) |

---

## Example: Work vs Personal

```bash
# Create work profile with high reasoning model
edgecrab profile create work
cat > ~/.edgecrab/profiles/work/config.yaml << 'EOF'
model:
  default: "anthropic/claude-opus-4"
  max_iterations: 90
tools:
  enabled_toolsets: ["coding"]
reasoning_effort: "high"
EOF

# Create personal profile with fast cheap model
edgecrab profile create personal
cat > ~/.edgecrab/profiles/personal/config.yaml << 'EOF'
model:
  default: "copilot/gpt-4.1-mini"
  max_iterations: 30
display:
  personality: "helpful"
EOF
```

Now just run:

```bash
work "refactor the auth module"     # uses claude-opus-4, reasoning=high
personal "plan my weekend"          # uses gpt-4.1-mini, friendly tone
```

---

## Pro Tips

**Create profiles for clients.** Each client gets a dedicated profile with their codebase context in `memories/` and project guidelines in `AGENTS.md`. Switch instantly: `edgecrab -p acme-corp`.

**Use a `--clone` profile for experiments.** Before exploring a risky refactoring, clone your active profile and experiment there — the sessions are isolated and won't pollute your main history:
```bash
edgecrab profile create refactor-experiment --clone
edgecrab -p refactor-experiment "aggressively refactor the auth module"
# not happy? just delete the profile
edgecrab profile delete refactor-experiment
```

**Don't use profiles for toolset switching.** Profile isolation is heavyweight (separate directories). For "I just want fewer tools today", use `--toolset` instead.

---

## Frequently Asked Questions

**Q: How much disk space does each profile use?**

Creating a profile creates an empty directory structure: essentially zero. Space grows with sessions (state.db), memories, and skills — just like the default profile.

**Q: Can I share skills between profiles?**

Use `skills.external_dirs` in each profile's `config.yaml` to point at a shared directory:
```yaml
skills:
  external_dirs:
    - ~/.edgecrab/skills   # the default profile's skills
```

**Q: I deleted my default profile by accident. How do I recover?**

The default profile is `~/.edgecrab/` itself. If you deleted `~/.edgecrab/profiles/default`, you haven't deleted the default. If you deleted `~/.edgecrab/`, restore from backup. `edgecrab setup` can recreate the directory structure but cannot recover sessions or memories.

**Q: Can I use the same API key in multiple profiles?**

Yes. The simplest approach: put the key in `~/.edgecrab/.env` (the default profile's env). The profile-specific `.env` overrides or extends it. If `OPENAI_API_KEY` is not in the profile's `.env`, EdgeCrab falls back to the default profile's `.env` (this fallback behavior may be project-specific — check `edgecrab doctor -p <name>` to confirm key visibility).

**Q: Can profiles have different gateway configurations?**

Yes — each profile's `config.yaml` has its own `gateway:` section. A `work` profile could have Slack enabled, while a `personal` profile has Telegram enabled.

---

## See Also

- [Configuration](/user-guide/configuration/) — Full `config.yaml` structure
- [Context Files](/features/context-files/) — Profile-scoped SOUL.md
- [Memory](/features/memory/) — Profile-isolated memory directories
- [CLI Commands](/reference/cli-commands/) — `edgecrab profile` subcommand reference
