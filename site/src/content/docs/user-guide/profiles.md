---
title: Profiles
description: Named EdgeCrab profiles with isolated config, SOUL.md, memories, skills, sessions, and live TUI switching. Grounded in crates/edgecrab-cli/src/profile.rs and app.rs.
sidebar:
  order: 9
---

Profiles give EdgeCrab isolated runtime homes under `~/.edgecrab/profiles/<name>/`. Each profile has its own config, personality, memory, skills, session database, plugins, hooks, and state. The default profile is still `~/.edgecrab/`.

EdgeCrab now ships starter profiles and seeds them automatically on normal startup and profile commands:

- `work` for production engineering and code review
- `research` for evidence-heavy synthesis and comparison work
- `homelab` for infrastructure, automation, and Home Assistant workflows

## What A Profile Contains

```text
~/.edgecrab/profiles/work/
├── config.yaml
├── .env
├── SOUL.md
├── state.db
├── memories/
│   ├── USER.md
│   └── MEMORY.md
├── skills/
├── plugins/
├── hooks/
├── mcp-tokens/
└── ...
```

The active sticky profile is stored in `~/.edgecrab/.active_profile`.

## Bundled Starter Profiles

Bundled profiles are seeded from templates compiled into the CLI crate. They are created once, skipped if you already have a profile with the same name, and never overwrite user edits.

### `work`

Focus: production coding, review, and high-discipline execution.

```yaml
model:
  default: "openai/gpt-5"
  max_iterations: 90

display:
  personality: "technical"
  show_cost: true
  show_status_bar: true
  tool_progress: "verbose"

honcho:
  enabled: true
  cloud_sync: false

reasoning_effort: "high"
```

### `research`

Focus: source-backed analysis, synthesis, and comparisons.

```yaml
model:
  default: "openai/gpt-5"
  max_iterations: 120

display:
  personality: "teacher"
  show_cost: true
  show_status_bar: true
  tool_progress: "verbose"

honcho:
  enabled: true
  cloud_sync: false

reasoning_effort: "high"
```

### `homelab`

Focus: local infra, automations, containers, and Home Assistant.

```yaml
model:
  default: "copilot/gpt-4.1"
  max_iterations: 90

display:
  personality: "technical"
  show_cost: true
  show_status_bar: true
  tool_progress: "verbose"

honcho:
  enabled: true
  cloud_sync: false

reasoning_effort: "medium"
```

## YAML Format

A profile is not a special schema. It is just a normal EdgeCrab home rooted at `~/.edgecrab/profiles/<name>/`, and its `config.yaml` uses the same `AppConfig` structure as the default profile.

Minimal example:

```yaml
model:
  default: "copilot/gpt-4.1"
  max_iterations: 60

display:
  personality: "concise"

reasoning_effort: "medium"
```

Typical profile-local files:

- `config.yaml` for models, toolsets, display, gateway, MCP, plugins, and policy
- `.env` for profile-specific secrets
- `SOUL.md` for profile identity and operating rules
- `memories/USER.md` and `memories/MEMORY.md` for durable memory

## CLI Commands

Use the binary subcommands for lifecycle operations:

```bash
edgecrab profile list
edgecrab profile show
edgecrab profile show work
edgecrab profile use work
edgecrab profile create client-acme
edgecrab profile create lab-copy --clone
edgecrab profile create audit-sandbox --clone-all --clone-from work
edgecrab profile alias work --name w
edgecrab profile rename client-acme client-acme-2026
edgecrab profile export work -o ./work-backup.tar.gz
edgecrab profile import ./work-backup.tar.gz --name work-restored
edgecrab profile delete work-restored --yes
```

Use `-p` or `--profile` to run under a profile without changing the sticky default:

```bash
edgecrab -p research "compare these two APIs"
edgecrab -p homelab "check the Home Assistant automations"
```

## TUI Commands

The TUI now has first-class profile UX while still keeping Hermes-style status output.

- `/profile` shows the active profile name and effective home directory
- `/profile list` opens the browser in summary mode
- `/profile show <name>` opens the browser focused on that profile in summary mode
- `/profile config <name>` opens the browser in `config.yaml` mode
- `/profile soul <name>` opens the browser in `SOUL.md` mode
- `/profile memory <name>` opens the browser in memory mode
- `/profile tools <name>` opens the browser in tool policy mode
- `/profile use <name>` focuses that profile for live switching inside the running TUI
- `/profiles` opens the interactive profile browser directly
- `/profiles use <name>` works as a shorthand for switching directly

Important: `/profile use <name>` is a real runtime switch. The TUI rebuilds the runtime, agent, tool registry, MCP connections, skills, and session DB path immediately. This is stronger than a "next launch only" toggle.

Inside the profile browser:

- `Enter` switches to the selected profile
- `C` shows `config.yaml`
- `S` shows `SOUL.md`
- `M` shows profile memory files
- `T` shows tool policy and toolset configuration
- `A` writes or refreshes the default alias
- `E` opens inline export
- `D` opens inline delete confirmation
- `N` opens inline profile creation
- `I` opens inline profile import
- `O` opens inline profile rename
- `Tab`, `Shift-Tab`, `Left`, and `Right` cycle detail views without leaving the overlay
- `H` or `?` opens the profile-browser help tab
- `Home` and `End` jump to the first or last visible result

## Isolation Model

These are isolated per profile:

- `config.yaml`
- `.env`
- `SOUL.md`
- `memories/`
- `skills/`
- `plugins/`
- `hooks/`
- `state.db`
- gateway PID/state files
- MCP token storage

These remain outside profile isolation:

- the `edgecrab` binary itself
- global sticky-profile marker `~/.edgecrab/.active_profile`
- shared shell alias directory `~/.local/bin/`
- repo-local context files such as `AGENTS.md`

## SOUL.md Example

Each profile can have a different operating stance. Example `SOUL.md`:

```md
# Client Review Profile

You are operating in a client-specific profile.

- Prefer evidence from the repository over assumptions.
- Treat compatibility and migration risk as first-class concerns.
- Be concise, but never omit materially relevant risk.
```

## Practical Patterns

- Keep `work` as the sticky default and use `-p research` for one-off comparison tasks.
- Use `--clone` for safe forks that copy identity and secrets but not the whole runtime state.
- Use `--clone-all` only when you explicitly want sessions, skills, and local state copied too.
- Export profiles for backup or handoff; imports refuse `default` so the base home cannot be silently replaced.

## See Also

- [CLI Commands](/reference/cli-commands/) for `edgecrab profile`
- [Slash Commands](/reference/slash-commands/) for `/profile` and `/profiles`
- [Configuration](/user-guide/configuration/) for the full YAML schema
- [Memory](/features/memory/) for profile-local memory behavior
