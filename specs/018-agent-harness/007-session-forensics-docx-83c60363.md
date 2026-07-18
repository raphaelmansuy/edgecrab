# 007 — Session Forensics: docx_raphael never-stop (2026-07-18)

**Date:** 2026-07-18  
**Session:** `83c60363-4efd-4a7d-9bed-b581b0f52cad`  
**Thesis:** Shell-landed `.docx` must flip `TaskClass::Document` and mid-loop
Done latch; reopen + screencapture theater must not burn the iteration budget.

**Sources:**
- DB: `~/.edgecrab/profiles/homelab/state.db`
- Logs: `~/.edgecrab/profiles/homelab/logs/{harness,agent}.jsonl`
- Workspace: `demo/docx_raphael/`

## Verdict

Agent delivered `Raphael_Mansuy_Profile.docx` (~68KB). Harness stayed
`CodeChange` (`.py` path), injected browser/dev-server storm nudges, reopened
with “do not stop yet” despite 6× `report_task_status` completed, and burned
iters ~70–90 on `open` / `screencapture` / TCC until `budget_exhausted`.

## Facts

| Fact | Evidence |
|------|----------|
| Model | `copilot/kimi-k2.7-code` |
| Messages / tools | 188 msgs; 83 tool results; api iter 90/90 |
| Outcome | `budget_exhausted`, `harness_blocked=false` |
| Class | Storm logs `task_class: CodeChange` — Document never applied |
| Product | `.docx` on disk ~2.5 min before capture thrash |
| Reopen | 7× `[system: do not stop yet… No structured verification evidence]` |
| Thrash | open×4, sleep×6, screencapture×3–4, vision of Cursor IDE |

## Laws (fix)

1. **Shell landing** — successful terminal/`execute_code` with document path ⇒ Document.
2. **Done latch** — Document + artifact evidence ⇒ break ReAct; no wait for model silence.
3. **No reopen** — Document evidence ⇒ never inject “do not stop yet” for NeedsVerification.
4. **Storm copy** — CodeChange storm cites compile/test only, not browser/dev-server.
5. **TCC halt** — `screencapture` → typed capability error; repeated identical fails Halt.

## Proof

See [proof/p8-document-done-latch.md](./proof/p8-document-done-latch.md).
