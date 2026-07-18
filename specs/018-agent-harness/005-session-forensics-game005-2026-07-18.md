# 005 — Session Forensics: game005 VisualUx (2026-07-18)

**Date:** 2026-07-18  
**Thesis:** Binding Constraint — controller bugs (recovery + evidence quality + port truth)
dominate model quality on VisualUx create tasks. Code is law.

**Sources:**
- Sessions `a63fd17c-fdf5-41cf-8655-c3f768013990` (persisted) and
  `0f6fc6d6-6fc8-4154-946c-d28406a82e03` (live; DB flush lag)
- DB: `~/.edgecrab/profiles/homelab/state.db`
- Logs: `~/.edgecrab/profiles/homelab/logs/{harness,agent}.jsonl`
- Peer field: [001-peer-field-2026-07.md](./001-peer-field-2026-07.md)

## Baseline honesty (Code is Law)

Sessions `a63fd17c` and `0f6fc6d6` are **pre-fix baselines**. They ran on a
binary **before** P3–P5 (empty-response recovery, port-bind truth, navigate≠evidence)
and the Wave B–D structural controllers. Do **not** treat their tool_error shapes
or false `Completed` as current tree behavior — use them as regression seeds and
acceptance fixtures only.

Runtime closure requires: rebuild CLI + proof tests in
[proof/p6-wave-b-structural.md](./proof/p6-wave-b-structural.md).

## Verdict

Integrity stack (One Done, structured terminal evidence, indexed hot set, armed
guardrails) is real. These baseline sessions proved: harness advises “serve then
navigate,” failed to recover from `ERR_EMPTY_RESPONSE` + `EADDRINUSE`, then
accepted `Completed` on thin navigate success while perception showed a broken page.

## Session A — `a63fd17c`

| Fact           | Evidence                                                                    |
| ----------------| -----------------------------------------------------------------------------|
| Visual create  | User: 3D Chess in `./demo/game005` → `demo/` ⇒ `TaskClass::VisualUx`        |
| Write budget   | `write_file` refused ~29KB vs 27852B; prose recovery, typed `recovery` null |
| Spawn lie      | bg `{"ok":true,"process_id":"proc-1"}` + “Dev server expected at :8000”     |
| Navigate ×5    | `net::ERR_EMPTY_RESPONSE` — tool_error with **zero** recovery fields        |
| Vision         | `browser_vision` described “127.0.0.1 didn’t send any data”                 |
| Port conflict  | `OSError: [Errno 48] Address already in use` — no structured recovery       |
| Assess         | `decision=completed`, `exit_reason=model_returned_final_text`               |
| Exit narrative | Model claimed WebGL unsupported; harness did not reopen                     |

## Session B — `0f6fc6d6`

Same physics (navigate fail → patch fail → model invents `lsof` → navigate OK).
DB row had `message_count=0` while harness.jsonl showed 50+ tool events.

## Laws vs code

| Law | Violation |
|-----|-----------|
| Evidence > vibes | VisualUx evidence accepts `browser_navigate` alone; not content quality |
| Observe → recover | `errorText` path recovers only `err_connection*`; empty-response bare fails |
| Port truth | `record_session_http_server` at spawn, not bind-ready |
| One Done | Assess path OK; quality gate weak (one late navigate defeats exhausted gate) |

## Peer delta (steal)

| Peer | Steal |
|------|-------|
| Hermes | Content-aware visual evidence (no chrome-error / loader-only) |
| Codex | Process ready / exit as first-class signals |
| Claude Code | Perception before Done (Stop/TaskCompleted semantics) |
| Pi | Keep hot wire small |

## Acceptance (P0–P1)

See proofs:

- [proof/p3-empty-response-recovery.md](./proof/p3-empty-response-recovery.md)
- [proof/p4-port-bind-truth.md](./proof/p4-port-bind-truth.md)
- [proof/p5-visual-evidence-quality.md](./proof/p5-visual-evidence-quality.md)

## Related

- [000-overview.md](./000-overview.md)
- [004-implementation-plan.md](./004-implementation-plan.md)
- [`../004-sota-harness/005-first-principles-assessment-2026-07.md`](../004-sota-harness/005-first-principles-assessment-2026-07.md)
