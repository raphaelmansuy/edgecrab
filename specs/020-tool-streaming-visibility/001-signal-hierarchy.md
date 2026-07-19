# 001 — Signal Hierarchy

## Five user questions (tool phase)

| # | Question | Signal | Surface |
|---|----------|--------|---------|
| 1 | What tool is running? | name + verb | L0 status, L1 header |
| 2 | On what? | command / path / query | L1 header preview |
| 3 | Is it making progress? | newest stdout lines | L1 body (≤3 lines) |
| 4 | How long / stuck? | elapsed + heat | L0 + L1 header |
| 5 | What can I do? | stop / steer / expand | L0 hints (`^C`, `^S`, `t`) |

## Priority when tools are in-flight

1. **Active tools** (evidence) — header + multi-line body for primary
2. **Subagents** — if any
3. **Thinking** — collapsed / skipped during `ToolExec` unless user expanded `/details thinking`
4. **Activity charms / LLM-wait** — demoted; never above tool body; charms omitted when stdout evidence exists
5. **Token footer** — yields space first under shelf budget

## Progressive disclosure ladder

| Level | Surface | Content |
|-------|---------|---------|
| L0 | Status bar | verb + preview + elapsed + `t=expand` (≥3s) |
| L1 | Activity shelf | Focus Tool Pane (header + ≤3 stdout lines) |
| L2 | Transcript | Done lines per `/tool-progress` policy |
| L3 | Ctrl+Shift+T | Expand finished tool result body |
| L4 | `t` / overlay | Rolling 4KB foreground progress buffer |
| L4b | `/tail <id>` | Background process buffer |

## Compact terminals (&lt;60 cols)

One caption line: `tool · last-stdout-line · elapsed` — never provider diagnostics (`vscode-copilot: iter…`).
