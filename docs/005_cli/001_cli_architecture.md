# 005.001 — CLI Architecture

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.001 System Architecture](../002_architecture/001_system_architecture.md) | [→ 006.001 Gateway](../006_gateway/001_gateway_architecture.md) | [→ 009.001 Config & State](../009_config_state/001_config_state.md)
> **Verified against source**: `crates/edgecrab-cli/src/main.rs`, `app.rs`, `cli_args.rs`, `commands.rs`, `profile.rs`, `theme.rs`, `skin_engine.rs`
> **Intent**: terminal-first personal agent UX inspired by OpenClaw’s tool-centric flow and Nous Hermes’s adaptive agent patterns.

---

## 1. What the CLI Owns

The `edgecrab` binary is the **local control plane** for the project. It does four jobs:

1. **Starts the interactive TUI** when no subcommand is given.
2. **Runs one-shot prompts** in quiet mode.
3. **Exposes operational subcommands** such as `setup`, `doctor`, `gateway`, `profile`, and `sessions`.
4. **Boots ACP mode** so editors can talk to the same agent core over stdio.

```text
edgecrab [OPTIONS] [PROMPT]
        |
        +--> `CliArgs::parse()`
        +--> `load_runtime()`
        +--> `create_provider()`
        +--> `build_tool_registry_with_mcp_discovery()`
        +--> `build_agent()`
               |
               +--> quiet mode  -> `agent.chat()` -> print and exit
               +--> ACP mode    -> JSON-RPC stdio server
               `--> TUI mode    -> `App::new()` -> event loop
```

---

## 2. Entry Modes

| Mode | Entry | Real implementation |
|---|---|---|
| Interactive TUI | `edgecrab` | Builds runtime + provider + `Agent`, then runs `App::new()` from `app.rs` |
| One-shot/headless | `edgecrab -q "..."` | Calls `agent.chat()` and prints the final response |
| Setup wizard | `edgecrab setup` | Interactive configuration flow in `setup.rs` |
| Diagnostics | `edgecrab doctor` | Environment, provider, and config checks in `doctor.rs` |
| Gateway control | `edgecrab gateway ...` | Starts/stops the messaging gateway from the CLI |
| ACP integration | `edgecrab acp` | Starts the ACP stdio server from `edgecrab-acp` |
| Migration | `edgecrab migrate [--dry-run]` | Imports config and user data from `~/.hermes/` into `~/.edgecrab/` |

---

## 3. Global Flags That Matter Most

These are the high-signal flags verified in `cli_args.rs`:

| Flag | Short | Purpose |
|---|---|---|
| `--model` | `-m` | Override the active model, e.g. `copilot/gpt-4.1-mini` |
| `--toolset` | — | Enable a specific toolset or alias set |
| `--session` | — | Resume an exact session ID |
| `--continue` | `-C` | Resume the most recent CLI session, optionally by title |
| `--resume` | `-r` | Resolve a session by ID prefix or title |
| `--quiet` | `-q` | No TUI; print the answer and exit |
| `--config` | `-c` | Use an alternate config file |
| `--debug` | — | Turn on verbose tracing |
| `--no-banner` | — | Skip the startup banner |
| `--worktree` | `-w` | Create and enter an isolated git worktree |
| `--skill` | `-S` | Preload one or more skills before the first turn |
| `--profile` | `-p` | Run under a named profile without changing the sticky default |

---

## 4. TUI Runtime (`app.rs`)

The interactive UI is built on `ratatui` + `crossterm` + `tui-textarea`.
That matches the actual code path and aligns with the current upstream docs for immediate-mode rendering and multiline text input.

### 4.1 Layout

```text
┌──────────────────────────────────────────────────────────────┐
│ Output transcript                                            │
│ - assistant text                                             │
│ - tool progress lines                                        │
│ - reasoning blocks (optional)                                │
│ - system / error messages                                    │
├──────────────────────────────────────────────────────────────┤
│ Status bar                                                   │
│ spinner | model | token count | session cost | tool status   │
├──────────────────────────────────────────────────────────────┤
│ Input area                                                   │
│ `TextArea` + ghost text + slash completion / overlays        │
└──────────────────────────────────────────────────────────────┘
```

### 4.2 Core UI State

`App` in `app.rs` keeps the following live state:

- `textarea: TextArea<'static>` — multiline, Unicode-safe input
- `output: Vec<OutputLine>` — rendered transcript buffer
- `model_name`, `total_tokens`, `session_cost` — live status metrics
- `completion`, `model_selector`, `skill_selector` — overlays for power-user flows
- `display_state` — idle / thinking / streaming / tool execution states
- `response_rx` / `response_tx` — async bridge between the agent and the TUI loop

### 4.3 What the UI Already Does Well

- **Streaming output** with incremental updates
- **Slash-command completion** with alias-aware help text
- **Model picker** backed by `ModelCatalog`
- **Skill browser** that recursively scans installed skills
- **Tool progress lines** with emoji, preview text, and duration
- **Unicode-width-aware rendering** so wide glyphs do not wreck alignment

> `theme.rs` also includes a defensive fallback for terminals that cannot render extended kaomoji cleanly, switching to safer ASCII-style faces when needed.

---

## 5. Slash Command Architecture

`commands.rs` centralizes the command registry. The UI does **not** execute everything inline; instead, commands return a `CommandResult`, and the `App` loop decides what to do next.

```text
user types `/model openai/gpt-4o`
        |
        v
`CommandRegistry::dispatch()`
        |
        v
`CommandResult::ModelSwitch("openai/gpt-4o")`
        |
        v
`App` performs the actual provider / agent swap
```

### High-signal command groups

| Group | Examples |
|---|---|
| Navigation | `/help`, `/clear`, `/quit`, `/version`, `/status` |
| Model control | `/model`, `/models`, `/provider`, `/reasoning`, `/stream` |
| Session control | `/new`, `/session`, `/retry`, `/undo`, `/stop`, `/history`, `/resume` |
| Tooling | `/tools`, `/toolsets`, `/reload-mcp`, `/mcp-token`, `/plugins` |
| Scheduling & media | `/cron`, `/voice`, `/browser` |
| Appearance | `/theme`, `/skin`, `/mouse`, `/paste` |

The registry currently covers **42+ commands with 50+ aliases**, which is consistent with the live code comments and tests.

---

## 6. Profiles, Sessions, and Worktrees

### 6.1 Profiles

Profiles isolate a user’s agent state under `~/.edgecrab/profiles/<name>/`.
The active profile name is stored in `~/.edgecrab/.active_profile`.

```text
~/.edgecrab/
├── config.yaml
├── .active_profile
├── skin.yaml
├── profiles/
│   ├── work/
│   │   ├── config.yaml
│   │   ├── .env
│   │   ├── memories/
│   │   ├── skills/
│   │   └── sessions/
│   └── personal/
└── skills/
```

`ProfileManager` in `profile.rs` owns:

- `list`, `use_profile`, `create`, `delete`, `show`
- alias wrapper generation under `~/.local/bin/`
- `rename`, `export`, `import`

### 6.2 Session restore

At startup, `main.rs` resolves sessions in this order:

1. `--session` (exact ID)
2. `--resume` (ID prefix or title)
3. `--continue` (most recent CLI session or titled session)

That logic is implemented in `resolve_session_flag()` and backed by `SessionDb`.

### 6.3 Git worktree isolation

`-w/--worktree` creates a new disposable worktree under `.worktrees/` in the current repository and switches into it before the session starts.
This is the safest way to run parallel coding sessions without trampling the main checkout.

---

## 7. Skin and Theme System

There are **two layers** in the current implementation:

1. `skin_engine.rs` — named skins and YAML-compatible presets
2. `theme.rs` — converts the chosen palette and symbols into `ratatui::Style` values

### Built-in named skins

The verified built-ins are:

```text
default   ares   mono   slate   poseidon   sisyphus   charizard
```

Additional developer-comfort presets also ship today:

```text
dracula   monokai   catppuccin
```

### Resolution model

```text
named skin request
    |
    +--> `~/.edgecrab/skins/<name>.yaml`  (user override)
    `--> built-in preset                  (compiled into binary)
             |
             `--> merged with `default`
```

If the user just wants terminal colors and symbols without named-skin switching, `theme.rs` also loads `~/.edgecrab/skin.yaml` directly.

---

## 8. ACP and Editor Integration

`edgecrab acp` starts the stdio-based ACP server from `edgecrab-acp`.
This lets editors talk to the same agent core, tool registry, and session model without inventing a second orchestration layer.

```text
Editor
  -> ACP JSON-RPC over stdio
  -> `edgecrab acp`
  -> `AcpServer`
  -> same `Agent` + same tool registry
```

---

## 9. Practical Reading Guide

If you want to understand the CLI quickly, read the files in this order:

1. `crates/edgecrab-cli/src/main.rs`
2. `crates/edgecrab-cli/src/app.rs`
3. `crates/edgecrab-cli/src/commands.rs`
4. `crates/edgecrab-cli/src/profile.rs`
5. `crates/edgecrab-cli/src/theme.rs` and `skin_engine.rs`

That path gives the cleanest mental model of how EdgeCrab turns the agent core into a fast, opinionated personal terminal assistant.
