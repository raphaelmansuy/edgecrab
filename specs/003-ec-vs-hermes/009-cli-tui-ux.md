# 009 — CLI, TUI & Operator UX

Interactive surfaces: terminal UX, slash commands, overlays, theming.

---

## Entry surfaces

| Surface | Hermes | EdgeCrab |
|---------|--------|----------|
| Classic REPL | `cli.py` (prompt_toolkit) | — (unified TUI) |
| Modern TUI | `hermes --tui` (React/Ink) | Default ratatui |
| Desktop app | Electron + embedded terminal | None |
| Web dashboard | Kanban dashboard plugin | None |
| Gateway-only chat | Platform DMs | Same |

**Verdict:** **Hermes leads surface count**; **EdgeCrab leads single-binary CLI**.

---

## Slash command coverage

| | Hermes | EdgeCrab |
|---|--------|----------|
| Catalog size | 75 (`COMMAND_REGISTRY`) | 84 (`BUILTIN_SLASH_COMMANDS`) |
| Categories | Session, Config, Tools, Info, Gateway | Similar + proxy/auth extras |
| Dynamic skill commands | `/<skill>` | Partial |
| Gateway-only commands | `/start`, `/topic`, `/approve`… | `/approve`, `/sethome`, … |

### Hermes-only commands (notable)

| Command | Purpose |
|---------|---------|
| `/redraw` | Terminal drift recovery |
| `/snapshot` | Config/state snapshots |
| `/codex-runtime` | Toggle Codex app-server |
| `/fast` | Priority processing tier |
| `/footer` | Gateway metadata footer |
| `/busy` | queue/steer/interrupt while working |
| `/bundles` | Skill bundles |
| `/curator` | Skill maintenance |
| `/kanban` | Multi-agent board |
| `/blueprint` | Automation templates |
| `/suggestions` | Suggested automations |
| `/gquota` | Gemini quota |
| `/whoami` | Slash access level |

### EdgeCrab-only commands (notable)

| Command | Purpose |
|---------|---------|
| `/done` | Mark subgoal complete |
| `/transfer-model` | Model switch with context brief |
| `/cheap_model`, `/vision_model`, `/image_model` | Auxiliary routing |
| `/stream` | Streaming toggle |
| `/worktree` | Git worktree session |
| `/handoff` | Platform handoff (also Hermes) |
| `/replay` | Spawn tree replay |
| `/proxy` | Proxy hub |
| `/lsp` | LSP tooling |
| `/computer` | Computer use toggle |
| `/mouse` | Mouse capture |
| `/permissions` | macOS permissions |
| `/uninstall` | Safe artifact removal |

**Verdict:** **≠** — EC has more catalog entries; Hermes has **deeper product features** behind commands (kanban, curator, blueprints).

---

## TUI architecture (see also spec 002)

| Dimension | Hermes | EdgeCrab |
|-----------|--------|----------|
| Framework | Custom Ink fork | ratatui |
| Process | UI + gateway server | In-process |
| Activity shelf | `thinking.tsx` | `activity_shelf.rs` |
| `/details` disclosure | RPC `details_mode` | `shelf_details.rs` YAML |
| `/agents` overlay | `agentsOverlay.tsx` | `agents_overlay.rs` + Gantt |
| Steering UX | `/steer` text | Ctrl+S overlay (HINT/REDIRECT/STOP) |
| Queued messages panel | Yes | `queued_messages.rs` |
| Spawn tree replay | Partial | `/replay list\|load` disk persist |
| Model picker | Multi-step wizard + disconnect | Hot-swap + expensive confirm |
| Skin engine | `skin_engine.py` | `skin_engine.rs` |
| FPS / perf pane | Yes | No |
| Test count (UI) | 71 TS test files | Rust integration tests |

**Verdict:** **Hermes leads UI architecture maturity** (component model, recovery). **EdgeCrab leads delegation control plane** (pause, per-agent kill, replay) per [002-tui spec](../002-tui-hemes-vs-edgecrab/007-first-principles-lead-plan.md).

---

## Voice mode

Both: `/voice on|off` → TTS readback after agent response via `text_to_speech` tool.

**Verdict:** **Parity**.

---

## Git worktrees

| | Hermes | EdgeCrab |
|---|--------|----------|
| Flag | `hermes --worktree` | `edgecrab --worktree` / `/worktree` |
| Isolated cwd session | Yes | Yes |

**Verdict:** **Parity** (both support).

---

## Operator ergonomics

| Feature | Hermes | EdgeCrab |
|---------|--------|----------|
| Slash autocomplete | prompt_toolkit | TUI built-in |
| Multiline input | Yes | Yes |
| Paste overlay | `/paste` | `/paste` |
| Cost/usage | `/usage`, `/credits` | `/cost`, `/usage` |
| Doctor | `hermes doctor` | `edgecrab doctor` |
| Update | `hermes update` | `edgecrab update` |
| Backup/import | Yes | Yes |
| i18n | zh-Hans docs | Gap 025 |

---

## Brutal TUI debt callout

**EdgeCrab `app.rs` ~34k lines** remains the highest-risk maintainability item in either codebase for UI work. Hermes splits UI across `ui-tui/` + `tui_gateway/` — more files, but bounded blast radius.

**Hermes Ink OOM** (issue #34095) is the counterweight — verbose tool output can kill the UI process.

---

## Grades

| Dimension | Hermes | EdgeCrab |
|-----------|--------|----------|
| Slash breadth | B+ (fewer, deeper) | A− (more, some thin) |
| TUI liveness | A− | A (post parity pass) |
| Multi-surface | A | C |
| Delegation UX | A− | A |
| Theming | A | A |
| Maintainability | B+ | C+ (app.rs) |

Cross-ref: [specs/002-tui-hemes-vs-edgecrab/](../002-tui-hemes-vs-edgecrab/)
