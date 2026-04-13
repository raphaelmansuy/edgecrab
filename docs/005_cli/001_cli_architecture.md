# CLI Architecture 🦀

> **Verified against:** `crates/edgecrab-cli/src/main.rs` ·
> `crates/edgecrab-cli/src/cli_args.rs` ·
> `crates/edgecrab-cli/src/commands.rs` ·
> `crates/edgecrab-cli/src/app.rs` ·
> `crates/edgecrab-command-catalog/src/lib.rs`

---

## Why the CLI is the composition root

`edgecrab-cli` is the heaviest crate — it imports everything. That is intentional.
The CLI is the *composition root*: the only place in the codebase where all parts
are wired together into a runnable whole.

When startup fails or the agent is wired incorrectly, the bug is almost always in
`main.rs` or `runtime.rs`, not in the TUI widgets or the tool implementations.

🦀 *The CLI is the crab's brain stem — it connects all the neural pathways
but does not implement the thoughts.*

**Reference:** [clap docs](https://docs.rs/clap) ·
[ratatui docs](https://docs.rs/ratatui)

---

## Entry flow

```
  edgecrab [args]
        │
        ▼
  main.rs
    │
    ├── CliArgs::parse()     (clap)
    │
    ├── no subcommand + no prompt?  →  interactive TUI (ratatui event loop)
    │
    ├── no subcommand + prompt?     →  single-turn non-interactive mode
    │
    └── subcommand?                 →  dispatch to command handler
```

---

## Clap subcommands

```
  edgecrab
   ├── [none]         Interactive TUI / single-turn (default)
   ├── chat           Hermes-compatible chat entrypoint
   ├── model          Open the interactive model selector
   ├── slash          Generic bridge into the slash-command registry
   ├── insights       Historical usage analytics
   ├── setup          Onboarding wizard
   ├── doctor         System diagnostics
   ├── version        Print version
   ├── auth           Copilot + MCP auth control plane
   │    ├── list
   │    ├── status
   │    ├── add
   │    ├── login
   │    ├── remove
   │    └── reset
   ├── login          Hermes-style auth shortcut
   ├── logout         Clear cached local auth state
   ├── dump           Support snapshot
   ├── logs           Local log inspection
   ├── pairing        Gateway pairing approval management
   ├── memory         MEMORY.md / USER.md inspection
   ├── honcho         Honcho-compatible user-model control plane
   ├── webhook        Dynamic gateway webhook subscriptions
   │    ├── subscribe
   │    ├── list
   │    ├── remove
   │    ├── test
   │    └── path
   ├── profile        Profile management
   │    ├── list
   │    ├── use
   │    ├── create
   │    ├── delete
   │    ├── show
   │    ├── alias
   │    ├── rename
   │    ├── export
   │    └── import
   ├── sessions       Session management
   │    ├── list
   │    ├── browse     (interactive fuzzy selector)
   │    ├── export
   │    ├── delete
   │    ├── rename
   │    ├── prune
   │    └── stats
   ├── config
   │    ├── show
   │    ├── edit
   │    ├── set
   │    ├── path
   │    └── env-path
   ├── tools
   │    ├── list
   │    ├── enable
   │    └── disable
   ├── mcp
   │    ├── list
   │    ├── refresh
   │    ├── search
   │    ├── view
   │    ├── install
   │    ├── test
   │    ├── doctor
   │    ├── auth
   │    ├── login
   │    ├── add
   │    └── remove
   ├── plugins
   │    ├── list
   │    ├── install
   │    ├── update
   │    └── remove
   ├── skills
   │    ├── list
   │    ├── view
   │    ├── search
   │    ├── install
   │    ├── update
   │    └── remove
   ├── cron
   │    ├── list
   │    ├── status
   │    ├── tick
   │    ├── create
   │    ├── edit
   │    ├── pause
   │    ├── resume
   │    ├── run
   │    └── remove
   ├── gateway
   │    ├── start
   │    ├── stop
   │    ├── restart
   │    ├── status
   │    └── configure
   ├── claw
   │    └── migrate
   ├── acp            JSON-RPC 2.0 stdio server (editor integration)
   ├── migrate        Hermes → EdgeCrab migration
   ├── whatsapp       WhatsApp bridge pairing
   ├── status         Agent and gateway status
   ├── uninstall      Remove EdgeCrab-managed local artifacts
   └── completion     Shell completion generation (bash/zsh)
```

---

## CLI flags (inline mode)

```sh
edgecrab [OPTIONS] [PROMPT]...

Options:
  -m, --model <MODEL>             Override model for this session
      --toolset <T1,T2,...>       Comma-separated toolsets or aliases
  -s, --session <ID>              Attach to session ID
  -C, --continue [ID]             Continue last session (or given ID)
  -r, --resume <ID>               Resume session from history
  -q, --quiet                     Suppress banner and status
  -c, --config <PATH>             Custom config file path
      --debug                     Enable debug logging
      --no-banner                 Skip the startup banner
  -w, --worktree                  Create a git worktree for this session
  -S, --skill <SKILL,...>         Pre-load session skills by name
  -p, --profile <PROFILE>         Use a named profile
      --yolo                      Start with approval prompts bypassed
```

`edgecrab claw migrate` now mirrors Hermes' OpenClaw entrypoint rather than
aliasing the Hermes importer. It imports EdgeCrab-native OpenClaw data and
archives unsupported OpenClaw-only config for manual review.

`--config <PATH>` is not just a file override. The parent directory of that
config file becomes the effective runtime home for sibling `.env`, `state.db`,
plugins, skills, and other binary-command state.

---

## The slash commands

Slash commands are handled in `commands.rs`, but the user-facing metadata now
lives in the shared `edgecrab-command-catalog` crate so the TUI help and the
gateway help no longer maintain separate hardcoded tables.

That is the right dependency direction:

- `edgecrab-command-catalog` owns static command metadata
- `edgecrab-cli` owns TUI dispatch and rendering
- `edgecrab-gateway` owns messaging dispatch and runtime semantics

This keeps the command definitions DRY without creating an illegal dependency
from the gateway back into the CLI crate.

For argv parity without an explosion of one-off clap subcommands, EdgeCrab now
uses `edgecrab slash <command...>` as the generic bridge into the same
`CommandRegistry` used by the TUI. That preserves one command grammar and one
handler graph.

| Category | Commands |
|---|---|
| Navigation | `/help`, `/quit`, `/clear`, `/version`, `/status`, `/new` |
| Session | `/session`, `/retry`, `/undo`, `/stop`, `/history`, `/save`, `/export`, `/title`, `/resume` |
| Model | `/model`, `/cheap_model`, `/vision_model`, `/image_model`, `/moa`, `/models`, `/provider`, `/reasoning`, `/stream` |
| Config | `/config`, `/prompt`, `/verbose`, `/personality`, `/statusbar`, `/log`, `/worktree`, `/mouse` |
| Tools | `/tools`, `/toolsets`, `/mcp`, `/reload-mcp`, `/mcp-token`, `/plugins`, `/skills`, `/browser`, `/memory` |
| Analysis | `/cost`, `/usage`, `/compress`, `/insights` |
| Appearance | `/skin`, `/paste` |
| Workflow | `/queue`, `/background`, `/rollback`, `/cron`, `/voice` |
| Gateway | `/platforms`, `/approve`, `/deny`, `/sethome`, `/update` |
| Diagnostics | `/doctor` and `/permissions` on macOS |

Recent UX notes:

- `/config` opens a searchable config center instead of only dumping paths.
- `/clear` now follows Hermes behavior: it clears the transcript and starts a fresh session.
- `/cheap_model` and `/moa aggregator` reuse the same fast selector pattern as `/model`.
- `/moa experts` uses a searchable multi-select overlay for full roster editing, while `/moa add` and `/moa remove` open focused add/remove expert pickers.
- `/moa on|off` gives Mixture-of-Agents the same explicit enable/disable ergonomics as cheap-model routing, and `/config` exposes a live MoA toggle.
- `/skin` is the Hermes-compatible primary command; `/theme` remains an alias.
- `/skin` opens the skin browser by default; `/skin reload` explicitly refreshes `~/.edgecrab/skin.yaml`.
- `--skill` and in-TUI skill activation now both feed the same session-scoped preloaded-skill set on the agent instead of prepending ad hoc text to the next user message.
- `/prompt` now manages the persisted `agent.system_prompt` override: `/prompt`, `/prompt clear`, `/prompt <text>`.
- `/insights [days]` now matches Hermes' optional day-window argument instead of hardcoding 30 days.
- `/statusbar` is a real persisted toggle.
- `/log` opens a real split-pane log browser plus entry inspector, both overlays live-follow by default with `F` as the toggle, and `/log level <level>` persists `logging.level` while reloading the live runtime filter when possible.
- `/worktree` opens a real report overlay instead of writing status into scrollback, and `/worktree on|off|toggle` persists the default for future launches only.
- `/verbose` cycles immediately on bare `/verbose`; `/verbose open` keeps the richer EdgeCrab picker available.
- `/approve`, `/deny`, `/sethome`, and `/update` now operate on live TUI or config state.
- `/webhook subscribe` now mirrors Hermes route semantics for `skills`, `deliver`, and templated `deliver_extra`, while reusing the shared gateway delivery router instead of duplicating platform send code.

---

## Important source modules

| File | What it is |
|---|---|
| `main.rs` | Entry point; arg parsing, startup, dispatch |
| `app.rs` | ratatui event loop, input handling, render |
| `runtime.rs` | `Agent` + `ToolRegistry` construction |
| `commands.rs` | Slash command registration and dispatch |
| `setup.rs` | Interactive onboarding wizard |
| `doctor.rs` | System diagnostics (deps, config, keys) |
| `profile.rs` | Profile isolation logic |
| `skin_engine.rs` | Theme loading from YAML/TOML skin files |
| `markdown_render.rs` | Markdown → ANSI rendering (terminal output) |
| `tool_display.rs` | Tool call/result rendering in the TUI |
| `fuzzy_selector.rs` | Interactive fuzzy session/model selector |
| `gateway_cmd.rs` | Gateway start/stop/status commands |
| `auth_cmd.rs` | Copilot + provider-env + MCP auth control plane |
| `webhook_cmd.rs` | Dynamic webhook subscription management |
| `uninstall_cmd.rs` | Safe uninstall planning and execution |
| `cron_cmd.rs` | Cron subcommand implementations |
| `model_discovery.rs` | Model list fetching from provider APIs |
| `status_cmd.rs` | `edgecrab status` output |
| `acp_setup.rs` | ACP server configuration wizard |
| `permissions.rs` | Approval policy UI |

---

## Profiles

Profiles are named configurations that isolate everything:
config, memory, skills, plugins, hooks, sessions, and the state database.
The CLI seeds bundled starter profiles (`work`, `research`, `homelab`)
from compiled templates on startup and before profile-management commands.

```
  ~/.edgecrab/                     (default profile)
    config.yaml
    state.db
    memories/
    skills/

  ~/.edgecrab/profiles/work/       (profile: work)
    config.yaml
    state.db
    memories/
    skills/

  ~/.edgecrab/profiles/research/   (profile: research)
    config.yaml
    ...
```

```sh
edgecrab profile create work
edgecrab --profile work "..."
edgecrab profile use work  # set as default
```

In the TUI, `/profile` shows the active profile summary and `/profiles`
opens a searchable browser. `/profile use <name>` performs a live runtime
switch by rebuilding the agent, tool registry, skills view, MCP
connections, and state DB path immediately.

---

## TUI layout

The ratatui TUI renders four main areas:

```
  ┌──────────────────────────────────────────────────────────────────┐
  │  status bar: model | tokens | cost | session                     │
  ├──────────────────────────────────────────────────────────────────┤
  │                                                                   │
  │  conversation pane                                                │
  │  (markdown-rendered messages, tool calls, tool results)          │
  │                                                                   │
  │                                                                   │
  ├──────────────────────────────────────────────────────────────────┤
  │  tool activity bar (spinner / tool name / duration)              │
  ├──────────────────────────────────────────────────────────────────┤
  │  input box  >_                                                    │
  └──────────────────────────────────────────────────────────────────┘
```

The conversation pane scrolls independently. Tool activity shows a live
spinner for each active tool call with elapsed time.

---

## Tips

> **Tip: `edgecrab doctor` is the first thing to run when something is wrong.**
> It checks for required binaries (`docker`, `chromium-browser`, `ffmpeg`),
> validates config file syntax, verifies API keys are set (but never prints them),
> and reports which tools are available vs unavailable.

> **Tip: Non-interactive mode is useful for pipelines.**
> ```sh
> # Pipe output to another command
> edgecrab -q "extract all TODO comments from src/" | tee todos.txt
>
> # Chain with git
> git diff HEAD~1 | edgecrab -q "summarise this diff for a commit message"
> ```

> **Tip: `--worktree` creates an isolated git worktree for the session.**
> EdgeCrab now also supports a config-level default (`worktree: true`) and a TUI `/worktree` overlay. Disposable worktrees are cleaned automatically, but worktrees with unpushed commits are preserved.

---

## FAQ

**Q: How does the TUI know which tools are running?**
`Agent::chat_streaming()` emits `StreamEvent::ToolExec { name, args_json }` when
a tool starts and `StreamEvent::ToolDone { name, duration_ms }` when it finishes.
The TUI subscribes to these events via the `UnboundedReceiver<StreamEvent>`.

**Q: Why is the slash command count 53 and not in a simple enum?**
Slash commands are registered via a map of name → handler closure. Adding a new
command is a matter of inserting into that map. This is more flexible than an enum
when commands need runtime state (current model, config, session).

**Q: Is the TUI accessible?**
It uses ANSI escape codes for colour and cursor control — accessible to screen
readers that understand terminal output (e.g. brltty). No graphical dependencies.

---

## Cross-references

- Agent construction in `runtime.rs` → [Agent Struct](../003_agent_core/001_agent_struct.md)
- Config loading → [Config and State](../009_config_state/001_config_state.md)
- Profile paths → [Config and State](../009_config_state/001_config_state.md)
- Security approval flow → [Security](../011_security/001_security.md)
