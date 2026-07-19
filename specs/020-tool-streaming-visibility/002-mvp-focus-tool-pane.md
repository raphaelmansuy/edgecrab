# 002 — MVP: Focus Tool Pane

## States

### Idle / awaiting

Shelf empty or phase caption only. No focus pane.

### Drafting (`ToolGenerating`)

```text
  ├─ ✎ drafting terminal · {"command":"npm i…
```

No multi-line body until `ToolExec`.

### Single long tool (≥0s; body when detail present)

```text
▾ tools  ~232 tokens
  └─ 💻 terminal  $ npm install                    · 2m 35s
      npm warn EBADENGINE …
      added 142 packages in 2m
      run `npm fund` for details
```

- Header: icon, name, preview, elapsed (heat color)
- Body: up to 3 non-empty lines from `detail` (match `OUTPUT_TAIL_LINE_COUNT`)
- Charms: omitted when body has evidence

### Parallel tools

- Each active tool gets a header row (cap unchanged: 3 collapsed / 12 expanded)
- Only **primary** (longest elapsed unfinished tool) gets the multi-line body
- Overflow: `+N more tool(s)`

### Compact

```text
⠹ terminal · run `npm fund` for details · 2m 35s
```

Last non-empty detail line only.

### Expand (`t`)

When textarea empty + foreground tool active: open overlay with rolling `progress_log` (4KB, same chrome as `/tail`). Esc closes. `^C` still cancels the agent.

## Shelf budget

- Soft cap remains `MAX_SHELF_LINES = 8`
- When focus body active, tools section may use 5–6 lines; activity / thinking / tokens yield
- Assembly order: tools → subagents → thinking → activity → tokens

## Charm policy

Emit long-run charms only when `detail` is empty or a wait heartbeat (`still running…`). Never replace stdout with charm text.
