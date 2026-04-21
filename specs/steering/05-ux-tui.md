# Mission Steering — UX & TUI Design

## 1. UX Principles

1. **Discoverable:** The steer shortcut appears in the help bar when the agent is running.
2. **Non-intrusive:** The overlay is compact (3 lines max), does not obscure agent output.
3. **Acknowledged immediately:** Pending count appears within 1 render frame.
4. **Self-explanatory:** The tag `[⛵ STEER]` in history is self-documenting.
5. **Ergonomic:** Ctrl+S is the steer shortcut (muscle memory: S = Steer).
   Esc closes without sending. Enter sends.

---

## 2. Keybindings

| Key         | Context                  | Action                               |
|-------------|--------------------------|--------------------------------------|
| `Ctrl+S`    | Agent running (any state)| Open steer overlay                   |
| `Enter`     | Steer overlay active     | Send steer, close overlay            |
| `Esc`       | Steer overlay active     | Discard steer, close overlay         |
| `Ctrl+S`    | Steer overlay already open | Close overlay (toggle)             |
| `Tab`       | Steer overlay active     | Cycle: Hint / Redirect / Stop kinds  |

---

## 3. TUI Layout During Steering

### Normal state (agent running):

```
+----------------------------------------------------------------+
| ⚕ claude-opus-4.5 | 12k/128k | 9% | 00:23                     |
+----------------------------------------------------------------+
| User: Implement a REST API for user authentication             |
|                                                                |
| [Thinking...]                                                  |
| > read_file(src/auth.rs) ✓                                     |
| > write_file(src/routes/auth.rs) ...                           |
|                                                                |
| Streaming: "Now I'll add the JWT middleware..."                |
+----------------------------------------------------------------+
| [▶ Type a message... ] (Ctrl+C cancel) (Ctrl+S steer)          |
+----------------------------------------------------------------+
```

### Steer overlay open:

```
+----------------------------------------------------------------+
| ⚕ claude-opus-4.5 | 12k/128k | 9% | 00:23  ⛵ STEER MODE      |
+----------------------------------------------------------------+
| User: Implement a REST API for user authentication             |
|                                                                |
| > write_file(src/routes/auth.rs) ...                           |
|                                                                |
+--------------------------------------------------+             |
| ⛵ Steer the agent (⇥ kind: [HINT] redirect stop)|             |
| Focus on the JWT refresh token logic first       |             |
| [Enter send] [Esc cancel]                        |             |
+--------------------------------------------------+             |
| [▶ main input preserved...                      ]              |
+----------------------------------------------------------------+
```

### After steer sent (pending):

```
+----------------------------------------------------------------+
| ⚕ claude-opus-4.5 | 12k/128k | 9% | 00:23  ⛵ 1 steer pending |
+----------------------------------------------------------------+
| > write_file(src/routes/auth.rs) ✓                             |
|                                                                |
| [⛵ STEER applied] Focus on the JWT refresh token logic first  |
| Streaming: "You're right, let me focus on refresh tokens..."  |
+----------------------------------------------------------------+
```

---

## 4. Status Bar Fragments

The status bar gains a new steering indicator:

```rust
// Steering state fragment (only when relevant):
if pending_steer_count > 0 {
    frags.push(("class:steer-pending", format!(" ⛵ {} pending", pending_steer_count)));
} else if steer_just_applied {
    frags.push(("class:steer-applied", " ⛵ applied "));  // fades after 3s
}
```

**Styles:**
- `steer-pending`: Yellow/amber text — attention-grabbing but not alarming
- `steer-applied`: Green text — success confirmation, auto-fades

---

## 5. Help Bar Extension

When the agent is running, the bottom help bar shows:

```
[Ctrl+C] cancel    [Ctrl+S] steer    [Ctrl+L] clear
```

When the steer overlay is active:

```
[Enter] send steer    [Tab] change kind    [Esc] cancel
```

---

## 6. Steer Kind Selector (Tab cycling)

The overlay shows the current kind and lets the user cycle:

```
kind: [HINT] ──Tab──> [REDIRECT] ──Tab──> [STOP] ──Tab──> [HINT]
```

**Visual indicator:**
```
┌─────────────────────────────────────────────────────────┐
│ ⛵ Steer  kind: [ HINT • redirect  stop ]               │
│ > Focus on JWT refresh token logic first                │
│  Enter: send  Tab: kind  Esc: cancel                    │
└─────────────────────────────────────────────────────────┘
```

---

## 7. Applied Steer in Output History

When a steer is applied, the TUI shows it in the output buffer:

```
 ⛵  [Steer] Focus on JWT refresh token logic first
```

This line uses a distinct style (muted cyan or amber) so users can distinguish
steers from agent output in scroll history.

---

## 8. Steer while idle (agent not running)

If the user presses Ctrl+S when the agent is idle:

```
┌─────────────────────────────────────────────────────────┐
│ ℹ  Agent is idle — steer will be sent as a new message  │
│ > Your steering text here...                            │
│  Enter: send  Esc: cancel                               │
└─────────────────────────────────────────────────────────┘
```

This promotes the steer to a full conversation turn (same as typing in the
main input), with the `[⛵ STEER]` prefix preserved for context.

---

## 9. Colour Palette

| Element          | Light theme    | Dark theme     |
|------------------|---------------|----------------|
| Steer pending    | `#d97706`     | `#fbbf24`      |
| Steer applied    | `#16a34a`     | `#4ade80`      |
| Steer kind badge | `#0369a1`     | `#38bdf8`      |
| Overlay border   | `#6366f1`     | `#818cf8`      |
| Overlay text     | `#1e293b`     | `#e2e8f0`      |

These integrate with the existing skin engine via new semantic color keys:
- `steer_pending_color`
- `steer_applied_color`
- `steer_overlay_border_color`

---

## 10. Accessibility

- The steer overlay is keyboard-only (no mouse required).
- Screen reader: focus moves to overlay on open; title announces "Steer input".
- The pending count in the status bar is text-only (no icon-only indicators).
- Ctrl+S is chosen because it does not conflict with existing system shortcuts
  in terminal contexts (where Ctrl+S may be XOFF; we handle this by checking
  if the terminal has xon/xoff flow control enabled — if so, fall back to
  `Ctrl+Shift+S` or `Alt+S`).
