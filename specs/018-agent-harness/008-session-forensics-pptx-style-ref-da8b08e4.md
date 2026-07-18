# 008 — Session Forensics: pptx style-ref false Done (2026-07-18)

**Date:** 2026-07-18  
**Session:** `da8b08e4-c767-4652-9be3-fac17a254098`  
**Thesis:** Document Done latch must require create/mutation evidence.
Inspect-only `ls` of a style-reference `.pptx` must not end the turn as Completed.

**Sources:**
- DB: `~/.edgecrab/profiles/homelab/state.db`
- Logs: `~/.edgecrab/profiles/homelab/logs/{harness,agent}.jsonl`
- Workspace: `demos/raphael/` (empty after session)

## Verdict

User asked for a new deck under `./demos/raphael` matching a Downloads style
reference. Harness correctly classed Document from the reference `.pptx` path
in the prompt. Agent ran `tool_search` then `ls` of the existing Downloads file.
Mid-loop Document Done latch fired on stdout path tokens (~1 ms after tool
complete, no further API call). Outcome `completed` / `model_returned_final_text`
with **no product** under `demos/raphael/`.

## Facts

| Fact | Evidence |
|------|----------|
| Model | `copilot/kimi-k2.7-code` |
| Messages / tools | 11 msgs; 4 tool calls |
| Outcome | `decision=completed`, `exit_reason=model_returned_final_text` |
| Product | None — `demos/raphael/` empty |
| Latch trigger | Successful `ls` stdout containing `.pptx` path token (`AI.pptx`) |
| Class | Document advisory from user-message style path (correct for advisory) |

## Laws (fix)

1. **Observe ≠ create** — inspect-only argv (`ls`, `stat`, `find`, …) never
   contributes Document artifact evidence from stdout.
2. **Mutation honesty** — `write_file` / `patch` evidence requires structured
   `ok: true` + document `path`.
3. **Intent ≠ landing** — assistant `path` args on `terminal` / `execute_code`
   are not evidence.
4. **Preserve P8** — non-inspect shell generators that print a deliverable
   document path still Done.

## Proof

See [proof/p9-document-create-evidence.md](./proof/p9-document-create-evidence.md).
