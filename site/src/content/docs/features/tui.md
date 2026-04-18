---
title: TUI Interface
description: Full guide to the EdgeCrab ratatui terminal interface — layout, status bar, all keyboard shortcuts, slash commands, themes, and background sessions. Grounded in crates/edgecrab-cli/src/commands.rs and app.rs.
sidebar:
  order: 3
---

EdgeCrab's TUI is built with [ratatui](https://ratatui.rs) — a Rust-native
TUI framework with double-buffered rendering. It features streaming token
output, slash-command autocomplete, and a live status bar.

---

## Interface Layout

```
 EdgeCrab  -  openai/gpt-4o  -  ~/my-project
--------------------------------------------------------------
                                                              
  You: explain the ReAct loop                                 
                                                              
  EdgeCrab: The ReAct loop alternates between Reasoning       
  and Acting -- the agent first thinks about which tool to    
  call, calls it, then observes the result...                 
                                                              
  | read_file  src/agent/loop.rs  (0.1s)                      
  | terminal  cargo test  (2.4s)                              
                                                              
--------------------------------------------------------------
  openai/gpt-4o | 8.4K/200K | [####......] 4% | $0.02 | 5m 
--------------------------------------------------------------
  > _                                                         
```

Three fixed areas:

1. **Header** — agent brand, active model, working directory
2. **Conversation stream** — scrollable output with live tool feed
3. **Status bar + input prompt** — fixed at the bottom

---

## Status Bar

```
openai/gpt-4o | 8.4K/200K | [####......] 4% | $0.02 | 5m
```

| Column | Description |
|--------|-------------|
| Model name | Active provider/model |
| Token count | Context tokens used / model max window |
| Context bar | Visual fill indicator |
| Cost | Estimated session USD cost (needs `display.show_cost: true`) |
| Duration | Elapsed session wall time |

**Context bar colors:**

| Color | Range | Action |
|-------|-------|--------|
| Green | < 50% | All good |
| Yellow | 50-80% | Getting full |
| Orange | 80-95% | Consider `/compress` |
| Red | >= 95% | Near overflow — compress now |

Toggle with `/statusbar on|off`.

---

## Tool Execution Feed

Live feedback as the agent works:

```
pondering...  (1.2s)
got it! (2.8s)

| read_file  src/main.rs  (0.1s)
| terminal  cargo test  (2.4s)
| web_search  "ratatui events"  (1.1s)
```

Cycle verbosity with `/verbose`:

| Mode | Output |
|------|--------|
| `off` | Final response only |
| `new` | One indicator per new tool type |
| `all` | Every tool call with preview (default) |
| `verbose` | Full arguments, results, debug logs |

---

## Keyboard Shortcuts

All shortcuts sourced from the `/help` output in
`crates/edgecrab-cli/src/commands.rs`.

### Scrolling Output

| Key | Action |
|-----|--------|
| `PgUp` / `PgDn` | Scroll output up / down one page |
| `Shift+Up` / `Shift+Down` | Scroll output 3 rows |
| `Alt+Up` / `Alt+Down` | Scroll output 5 rows |
| `Ctrl+Home` | Jump to top of output |
| `Ctrl+End` or `Ctrl+G` | Jump to bottom (live view) |

### Input Editing

| Key | Action |
|-----|--------|
| `Enter` | Submit message |
| `Shift+Enter` | Insert newline (multi-line input) |
| `Up` / `Down` | Navigate prompt history |
| `Right` | Accept ghost-text autocomplete hint |
| `Tab` | Tab completion |
| `Ctrl+U` | Clear the input field |
| `Ctrl+L` | Clear the screen |

### Session Control

| Key | Action |
|-----|--------|
| `Ctrl+C` (x1) | Clear input / interrupt current agent request |
| `Ctrl+C` (x2 within 2s) | Force exit |
| `Ctrl+D` (on empty input) | Exit |
| `Ctrl+M` | Toggle terminal mouse-capture / text-selection mode |

### Multi-line Input

Two ways to insert newlines:

1. `Shift+Enter` — insert newline inline
2. Backslash continuation — end a line with `\` and press Enter:

```
> Fix all failing tests in this repo\
  Start with the file tool to list test files\
  then run cargo test to see failures
```

### Interrupting the Agent

- Type a new message while the agent is working — interrupts and sends the new message
- `Ctrl+C` once — interrupts the current tool call
- `Ctrl+C` twice within 2 seconds — force exits
- In-progress terminal processes receive `SIGTERM`, then `SIGKILL` after 1 s

---

## Slash Commands Quick Reference

Type `/` to open the autocomplete dropdown. All installed skills also
appear as slash commands.

```
Navigation   /help /quit /clear /new /status /version
Model        /model /provider /reasoning /stream
Session      /session /retry /undo /stop /history /save /export /title /resume
Config       /config /prompt /verbose /personality /statusbar
Tools        /tools /toolsets /reload-mcp /plugins
Memory       /memory /skills
Analysis     /cost /usage /compress /insights
Advanced     /queue /background /rollback
Gateway      /platforms /approve /deny /sethome /update
Scheduling   /cron
Media        /voice /browser
Appearance   /skin /theme /paste /mouse
Diagnostics  /doctor
Auth         /copilot-auth
MCP          /mcp-token
```

Full documentation: [Slash Commands Reference](/reference/slash-commands/)

---

## Background Sessions

Run a task concurrently while continuing to use the TUI:

```
/background Analyze all Python files in this repo for security issues
```

EdgeCrab confirms immediately:

```
Background task #1 started: "Analyze all Python files..."
  Task ID: bg_143022_a1b2c3
```

When finished, the result appears inline:

```
EdgeCrab (background #1)
Found 3 potential issues:
1. SQL injection risk in db.py line 42
2. Hardcoded secret in config.py line 8
3. Unvalidated file path in upload.py line 91
```

Background sessions inherit your model, toolsets, and reasoning settings
but have no access to the foreground session's history.

---

## Theming

The skin file lives at `~/.edgecrab/skin.yaml`. Reload without
restarting with `/skin` or `/skin reload`.

### SkinConfig Fields (from `theme.rs`)

| Field | Default | Description |
|-------|---------|-------------|
| `prompt_color` | `"cyan"` | Input prompt color |
| `assistant_color` | `"green"` | Assistant response color |
| `tool_color` | `"yellow"` | Tool output line color |
| `error_color` | `"red"` | Error message color |
| `system_color` | `"dim"` | System message color |
| `prompt_symbol` | `"> "` | Input prompt symbol |
| `tool_prefix` | `"| "` | Prefix for tool execution lines |
| `agent_name` | `"EdgeCrab"` | Name displayed in the UI |
| `welcome_msg` | (auto) | Custom welcome message |
| `goodbye_msg` | (auto) | Custom goodbye message |
| `thinking_verbs` | (list) | Words used in thinking animation |

Example `skin.yaml`:

```yaml
agent_name: "DevBot"
prompt_symbol: "> "
tool_prefix: "  - "
assistant_color: "bright_blue"
welcome_msg: "Hello! Ready to work."
goodbye_msg: "See you next time!"
thinking_verbs:
  - "thinking"
  - "processing"
  - "reasoning"
```

Reload after editing:

```
/skin
```

Switch to a named preset:

```
/skin catppuccin
```

---

## Personality Presets

```
/personality concise      # ultra-terse responses
/personality technical    # deep detail, no hand-holding
/personality kawaii       # enthusiastic and cute
/personality pirate       # arr, matey
```

Full preset list: `helpful`, `concise`, `technical`, `kawaii`, `pirate`,
`philosopher`, `hype`, `shakespeare`, `noir`, `catgirl`, `creative`,
`teacher`, `surfer`, `uwu`.

---

## Accessibility and Headless Mode

- **Quiet mode** (`-q`): suppresses TUI; prints only the final response to stdout — pipe-friendly
- **No banner** (`--no-banner`): skips the startup ASCII art
- **Compact mode**: `display.compact: true` reduces whitespace between messages
- **Font compatibility**: kaomoji and Unicode symbols require a font with broad Unicode coverage; fall back to ASCII with `--no-banner` on minimal terminals

---

## Pro Tips

**Learn the 5 essential shortcuts.** With just `Enter`, `Ctrl+C`, `Ctrl+L`, `Alt+Up/Down`, and `/help` you can handle 90% of interactions. Everything else can be discovered via `/help`.

**Use `/model` to switch mid-session.** Don't restart EdgeCrab to try a different model. Jump to GPT-4o for a complex synthesis task, then back to a cheaper model for routine edits:
```
/model openai/gpt-4o
```

**Set a custom skin for each profile.** Working on multiple projects? Each profile's `skin.yaml` can have a different `agent_name` and `assistant_color` — instant visual context when you have multiple terminals open.

**Record sessions for onboarding.** The TUI's streaming output is human-readable. For team onboarding, create a script that runs EdgeCrab in headless mode (`--quiet`) and captures the output for documentation.

---

## Frequently Asked Questions

**Q: The TUI looks broken / garbled in my terminal.**

EdgeCrab uses ratatui with crossterm. Ensure:
1. `TERM=xterm-256color` or better
2. Your terminal supports 256-color or true color
3. Unicode is supported (use UTF-8 locale: `export LANG=en_US.UTF-8`)

For minimal terminals (no Unicode), add `--no-banner` and set `display.compact: true`.

**Q: Kaomoji / emoji characters show as `???`.**

This is a font issue: your terminal's font doesn't include those Unicode characters. Install a Nerd Font or any font with broad Unicode coverage (e.g., JetBrains Mono, Fira Code). The agent's behavior is identical regardless — it's purely cosmetic.

**Q: How do I increase the font size in the TUI?**

EdgeCrab doesn't control font size — that's your terminal emulator's setting. Use `Cmd+` on macOS Terminal/iTerm2, or your terminal's preferences.

**Q: Can I use EdgeCrab in VS Code's integrated terminal?**

Yes. The VS Code integrated terminal fully supports EdgeCrab's TUI. For the best experience, use VS Code's built-in terminal with a Nerd Font configured.

**Q: The output is too wide and wraps badly.**

Resize your terminal window or use `display.compact: true`. EdgeCrab follows your terminal's column width automatically.

**Q: How do I copy text from the TUI?**

Use your terminal emulator's normal copy (mouse select, then `Cmd+C` on macOS or `Ctrl+Shift+C` on Linux). The TUI doesn't intercept copy operations.

---

## See Also

- [Slash Commands](/reference/slash-commands/) — Complete slash command reference
- [Configuration](/user-guide/configuration/) — `display.*` config section
- [Sessions](/user-guide/sessions/) — Session management from the CLI
- [Profiles](/user-guide/profiles/) — Per-profile TUI personalization
