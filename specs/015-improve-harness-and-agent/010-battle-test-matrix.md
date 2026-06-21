# 010 — Battle Test Matrix

Stress scenarios beyond games003. Each row: **scenario → expected harness behavior → failure if**.

Cross-ref: [002-terminal-ux-ui/006-stuck-scenarios](../002-terminal-ux-ui/006-stuck-scenarios-playbook.md) · [005-evidence-games003.md](./005-evidence-games003.md).

---

## Matrix

```text
  SCENARIO          HARNESS MUST                         FAIL IF
  ═══════════════════════════════════════════════════════════════════════════
  S15 games003 UX   spill+recipe; preview or vision    blind rewrite; read ?
  S16 large write   reject + patch hint                silent retry loop
  S17 Copilot tool  label non-streaming wait           appears deadlocked
  S18 indexed tool  tool_search path clear             deferred call errors
  S19 interrupt     RunOutcome Interrupted shown       silent stop
  S20 OTEL down     no log spam                        ERROR/min > 1
  S21 parallel read same file                         race / duplicate spill
  S22 spill artifact read same turn                   model still blind
  S23 goal + UX     goal inject + verify advisory      goal in system prompt
  S24 kanban worker same RunOutcome as CLI             divergent completion
  S25 memory cap       structured recovery + no blind retry  silent fail
  S26 todo + UX        todo items include verify step        plan without perceive
  S27 terminal storm   WARN after N shell w/o perception     churn loops
  S28 wrong port       navigate hint uses detected port      8888 vs 8000
```

---

## S25 — Memory write cap (`2e720f47`)

**Setup:** MEMORY.md near 2200 chars; model calls `memory_write`.

| Expected | |
|----------|--|
| Tool error | `used_chars`, `max_chars`, prune/session_search hint |
| No | immediate identical retry |

**Code:** `tools/memory.rs` + `recovery_catalog.rs` · Gate HA-17.

---

## S26 — Todo plan includes verification

**Setup:** `manage_todo_list` on visual UX task.

| Expected | |
|----------|--|
| TaskClassifier advisory | preview or `browser_vision` before "done" todo |
| Optional | todo template suggests verify item for `VisualUx` |

**Gate:** HA-20d, HA-19.

---

## S27 — Terminal iteration storm

**Setup:** Mock 6× `terminal` without `browser_*` / `vision_*`.

| Expected | |
|----------|--|
| Harness WARN | `iteration_storm` or equivalent in JSONL |
| Shelf | optional activity notice |

**Gate:** HA-20e.

---

## S28 — Dev server port mismatch

**Setup:** `python -m http.server 8000`; model navigates `localhost:8888`.

| Expected | |
|----------|--|
| Error or steer | cites port 8000 from process table |
| With preview | navigate `http://127.0.0.1:8000/...` succeeds |

**Gate:** HA-20c, HA-05.

---

## S15 — Visual UX task (games003 replay)

**Input:** `Improve demo/games003/index.html HUD styling. No new markdown files.`

| Step | Expected |
|------|----------|
| Discovery | `search_files` or find succeeds for `games003` |
| Read | Spill stub with artifact + `read_file(offset,limit)` |
| Edit | `patch` on CSS blocks, not full `write_file` |
| Verify | Preview or vision evidence before final text |
| Stop | `RunOutcome` with clear message |

**Battle test:** Mock LLM script from harness.jsonl; assert message history contains patch calls, not 30k write args.

---

## S16 — Mutation budget wall

**Input:** Model attempts `write_file` with 30k content.

| Expected | |
|----------|--|
| Pre-dispatch reject | no file touched |
| Tool result | `tool_argument_budget_exceeded` + patch recommendation |
| Next iteration | model uses `patch` (in strict eval) |

**Code:** `conversation.rs` + `mutation_turn_policy.rs`.

---

## S17 — Copilot non-streaming tool turn

**Setup:** Provider `vscode-copilot`, tool_choice required.

| Expected | |
|----------|--|
| Shelf | `composing tool call — non-streaming — waiting on Copilot API` |
| Status | elapsed seconds increase |
| No | duplicate identical activity lines >3 |

**Code:** `tool_progress_tail.rs` `nonstreaming_wait_liveness`.

---

## S18 — Deferred tool without materialization

**Setup:** indexed mode, model calls `browser_vision` without prior `tool_search`.

| Expected | |
|----------|--|
| Dispatch blocked | `deferred_tool_error_response` |
| Message | names `tool_search` explicitly |

**Code:** `tool_schema_index.rs` + dispatch gate.

---

## S19 — Ctrl+C interrupt mid-tool

| Expected | |
|----------|--|
| Tools cancelled | cancel token propagated |
| TUI | `Stopped — run interrupted` (RunOutcome) |
| DB | `end_reason = interrupted` |

---

## S20 — OTEL collector absent

**Setup:** `otel_export: true`, no docker collector.

| Expected | |
|----------|--|
| Agent runs | no functional impact |
| Logs | ≤1 WARN startup; no repeating ERROR |

---

## S21 — Parallel read same path

**Setup:** Model emits 3× `read_file` same path parallel.

| Expected | |
|----------|--|
| All complete | no panic |
| Spill | deterministic artifact names or single artifact |

**Code:** parallel JoinSet + spill_seq.

---

## S22 — Spill without follow-up (anti-pattern detector)

**Setup:** Telemetry only — dogfood sessions.

| Metric | Alert if |
|--------|----------|
| `spill_without_artifact_read` | stub in messages, no read on artifact path within 3 turns |

**Implementation:** harness.jsonl analyzer (future `edgecrab doctor harness`).

---

## S23 — Goals + visual task

**Setup:** `/goal polish games003` + UX message.

| Expected | |
|----------|--|
| Goal block | in **messages**, not cached system |
| Completion | considers goal + verification advisory |

---

## S24 — Surface parity

| Surface | Must receive |
|---------|--------------|
| CLI | full RunOutcome |
| Gateway | exit_reason in hook |
| ACP | terminal status enum |
| Cron output | exit_reason in markdown frontmatter |

---

## Red team (ideas that failed battle test)

| Idea | Why rejected |
|------|--------------|
| Auto-read spill into context | Blows turn budget — recipe only |
| Disable spill for HTML | Breaks compression — fix stub instead |
| Force browser for all tasks | SSRF + latency |
| Haiku-only guardrails | Provider-agnostic harness required |
| Merge 107 tools on wire | Defeats spec 007 indexed mode |

---

## Dogfood checklist (operator)

Before closing P0:

- [ ] Run games003 prompt on Copilot Haiku — observe wire count + spill stub
- [ ] Open `game-beautiful.html` manually — compare agent claim vs reality
- [ ] `/doctor harness` shows spill path + last exit_reason
- [ ] Interrupt — read final banner
- [ ] Logs clean with collector stopped
