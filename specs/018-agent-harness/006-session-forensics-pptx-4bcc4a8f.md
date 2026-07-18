# 006 — Session Forensics: pptx_raphael Document thrash (2026-07-18)

**Date:** 2026-07-18  
**Session:** `4bcc4a8f-88ec-4e80-93d1-aa2251d8994c`  
**Thesis:** `demo/` path alone must not force VisualUx; document/deck jobs need
`TaskClass::Document` + artifact evidence — not localhost browser theater.

**Sources:**
- DB: `~/.edgecrab/profiles/homelab/state.db`
- Logs: `~/.edgecrab/profiles/homelab/logs/{harness,agent}.jsonl`
- Workspace: `demo/pptx_raphael/`

## Verdict

Agent delivered PPTX + PDF + slide JPGs + HTML gallery. Harness classified
`visual_ux` from `demo/`, armed storm/port/nav gates, raced ahead of TCP
bind-ready across 13 bg spawns, and ended `budget_exhausted` / `harness_blocked`
without verifying the binary deck.

## Facts

| Fact | Evidence |
|------|----------|
| Messages | 199 (92 assistant / 97 tool); distinct timestamps |
| Header lie | `tool_call_count=1` vs 97 tool messages |
| Outcome | `budget_exhausted`, `harness_blocked=true` (~api iter 89) |
| Product | `.pptx`, `.pdf`, `create_presentation.js`, `index.html`, `slide-*.jpg` |
| Class inject | `[harness] Task class: visual_ux` |
| Thrash | `proc-1`…`proc-13`; repeated EADDRINUSE :8000 |
| Binary | `read_file` pptx → invalid UTF-8 |
| Skills | profile skill path `permission_denied` |
| Disclosure | `vision_analyze` “not on wire” with `tool_is_error:false` |

## Laws (fix)

1. **Document class** — landed `.pptx`/`.pdf`/`.docx` ⇒ Document; `demo/` alone ≠ VisualUx.
2. **VisualUx** — only landed web assets (`.html`/`.css`) or browser contract kind.
3. **Bind latch** — spawn exposes `bind_ready`; navigate waits; one preview server per port.
4. **Media router** — binary reads return structured stub, never UTF-8 panic.
5. **Skill roots** — profile `edgecrab_home`/`skills` readable for file tools.
6. **Disclosure honesty** — deferred-tool soft fail is typed `tool_error`.
7. **Telemetry** — `tool_call_count` derived from tool messages + restored.

## Proof

See [proof/p7-document-class-bind-latch.md](./proof/p7-document-class-bind-latch.md).
