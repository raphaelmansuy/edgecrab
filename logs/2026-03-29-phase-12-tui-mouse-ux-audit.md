# Phase 12 TUI Audit — Mouse, Keyboard & UX Excellence
_2026-03-29 | edgecrab TUI — app.rs_

---

## Audit Scope

Full usability audit of `crates/edgecrab-cli/src/app.rs` against best-practice
TUI applications in the Rust ecosystem (lazygit, gitui, bottom, helix, zellij).

---

## Findings Summary

### Critical (Fixed)

| # | Issue | Fix |
|---|-------|-----|
| C1 | **Mouse capture disabled** — scroll wheel produced no output |Enable `EnableMouseCapture` in `run_tui`; add `DisableMouseCapture` in both cleanup path and panic hook |
| C2 | **No mouse event handler** — `Event::Mouse` fell through to `_ => {}` | Add `Event::Mouse(mouse) => { app.handle_mouse_event(mouse); }` in `event_loop` |
| C3 | **No `handle_mouse_event` method** | Add method: `ScrollUp → scroll_output(5)`, `ScrollDown → scroll_output(-5)`, `Left click → collapse overlays` |

### Major (Fixed)

| # | Issue | Fix |
|---|-------|-----|
| M1 | Missing F-key shortcuts | F1=help, F2=model selector, F5=retry, F10=verbose |
| M2 | Missing readline `Ctrl+K` (kill to EOL) | Added `Ctrl+K → delete_line_by_end()` |
| M3 | Missing readline `Ctrl+A`/`Ctrl+E` | Added `Ctrl+A → CursorMove::Head`, `Ctrl+E → CursorMove::End` |
| M4 | Turn separator fixed at 72 chars | Now dynamic: `area.width − 4` (fills terminal width correctly) |
| M5 | Completion overlay had no border/count | Added border + title `Commands N/M` + footer hint bar |
| M6 | Input not styled when agent busy | Border dims + shows `⊗ waiting…` title during processing |

### Minor (Fixed)

| # | Issue | Fix |
|---|-------|-----|
| MI1 | Status bar hints omitted scroll/F-keys | Updated to `F1=help  F2=model  ↕scroll  Tab=complete  ^C=cancel` |
| MI2 | Scroll-up hint unclear | Updated to `↑N  ^G=end  ↕scroll  PgUp/Dn` |
| MI3 | Shift+Up/Down step was 3 (inconsistent with mouse 5) | Unified to 5 rows |
| MI4 | During processing, hints showed scroll but not cancel prominence | Processing hint: `^C=cancel  ↕scroll` |

---

## Keyboard Shortcut Reference (Post-Audit)

| Key | Action |
|-----|--------|
| `Enter` | Submit input |
| `Shift+Enter` | Insert newline in multi-line mode |
| `Tab` | Open/cycle completion overlay |
| `Shift+Tab` | Cycle completion backward |
| `↑` / `↓` | History navigation (single-line) / completion navigation (overlay) |
| `→` at EOL | Accept ghost hint (Fish-style) |
| `PageUp` / `PageDown` | Scroll output by viewport height |
| `Shift+↑/↓` | Scroll output by 5 rows |
| `Alt+↑/↓` | Scroll output by 5 rows |
| `Ctrl+G` | Jump to bottom of output |
| `Ctrl+Home` | Jump to top of output |
| `Ctrl+End` | Jump to bottom of output |
| `Ctrl+C` | Clear input → cancel agent → exit |
| `Ctrl+D` | Exit (on empty input) |
| `Ctrl+L` | Clear screen |
| `Ctrl+U` | Clear entire input |
| `Ctrl+K` | Kill from cursor to end of line *(new)* |
| `Ctrl+A` | Move to beginning of line *(new)* |
| `Ctrl+E` | Move to end of line *(new)* |
| `F1` | Open help *(new)* |
| `F2` | Open model selector *(new)* |
| `F5` | Retry last message *(new)* |
| `F10` | Toggle verbose mode *(new)* |
| `Esc` | Close overlays |

## Mouse Support (New)

| Event | Action |
|-------|--------|
| Scroll wheel up | Scroll output up 5 rows |
| Scroll wheel down | Scroll output down 5 rows |
| Left click | Collapse completion/model selector |

> **Text selection note:** Most modern terminals (iTerm2, WezTerm, Kitty) support
> `Shift+drag` for text selection even when mouse capture is enabled. Standard
> terminal copy (Cmd+C / Ctrl+Shift+C) works with Shift-select.

---

## Test Results

```
test result: ok. 136 passed; 0 failed
app tests: 15/15 passed
```

## Commit

`9bde4a6 feat(tui): Phase 12 mouse scroll, F-keys, readline shortcuts, overlay polish`
