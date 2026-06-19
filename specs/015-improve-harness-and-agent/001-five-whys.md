# 001 — Five Whys (Grounded in games003 Evidence)

**Anchor sessions:** `927f4d85-a417-4171-bf67-a041756ab191` (2026-06-18) · `e22c0a28-f25f-4152-9936-5476c5ce3f55` (2026-06-19)  
**Prompt:** *Improve game demo/games003 make it more beautiful, amazing UX/UI*  
**Evidence:** [005-evidence-games003.md](./005-evidence-games003.md) · `~/.edgecrab/profiles/homelab/logs/harness.jsonl`

---

## The symptom (What the operator saw)

```text
  TUI transcript                         Shelf / status
  ──────────────                         ──────────────
  find ... 2.1s                          ~25k/128k ctx
  read → [spilled — use read_file...]    composing next tool call
  read → [read_file] read ?              non-streaming — waiting
  write 3.0s                             27s … 32s …
  (no user-visible "done")               (¬_¬ ) priming
```

---

## Five Whys

### Why 1 — Why did the run feel stuck after tools finished quickly?

**Because** the operator waited on **LLM composition** (Copilot non-streaming tool turn), not on tool dispatch. Tools ran in 0–3s; API iterations took 28–40s.

```text
  wall clock
  0s   tools done ────────────────────────────────┐
  6s   still composing tool call                │
 12s   still composing tool call                │  MODEL + NETWORK
 27s   still composing tool call                │  (opaque to TUI)
 32s   still composing tool call                │
       ▼                                        ▼
  USER: "stuck"                         ACTUAL: blocked on HTTP completion
```

**Code law:** [`tool_progress_tail.rs`](../../crates/edgecrab-tools/src/tool_progress_tail.rs) `nonstreaming_wait_liveness` · [`conversation.rs`](../../crates/edgecrab-core/src/conversation.rs) non-streaming downgrade path.

**Cross-ref:** [014 § Executive summary](../014-improve-local-harness/README.md) · [002-terminal-ux-ui/006 S14](../002-terminal-ux-ui/006-stuck-scenarios-playbook.md)

---

### Why 2 — Why did the model struggle to improve UX despite low context use (~20%)?

**Because** the **perception loop was broken**: large reads were **spilled** to artifacts; the model received stubs, not full `index.html` / `game.js`. The TUI showed `read ?` when summaries lost path args.

```text
  read_file(index.html)  ──► 29k bytes
         │
         ▼
  maybe_spill() ──► stub: "[tool_result_spill] artifact: .edgecrab-artifacts/…/read_file_002.md"
         │
         ▼
  MODEL CONTEXT: ~200 chars + "use read_file on artifact"
         │
         ▼
  write_file(whole new game)  ◄── blind rewrite, not UX polish
```

**Code law:** [`tool_result_spill.rs`](../../crates/edgecrab-core/src/tool_result_spill.rs) · [`artifact_spill.rs`](../../crates/edgecrab-tools/src/artifact_spill.rs) · [`tool_result_summary.rs`](../../crates/edgecrab-core/src/tool_result_summary.rs) line 196.

**Battle-test:** Artifacts exist at `edgecrab/.edgecrab-artifacts/{session}/read_file_*.md`.

---

### Why 3 — Why did write attempts fail or split into awkward chunks?

**Because** `mutation_turn_policy` enforces **one-completion argument budget** (~27,852 B at 8192 output tokens). Full-file HTML writes exceeded the cap; harness **rejected** calls at 28,077 B and 30,546 B before dispatch.

```text
  write_file(full index.html)  ~30k arg bytes
         │
         ▼
  check_tool_argument_budget() ──► REJECT (conversation.rs ~5095)
         │
         ▼
  model retries smaller files → game-beautiful.html + game-beautiful.js
```

**Code law:** [`mutation_turn_policy.rs`](../../crates/edgecrab-tools/src/mutation_turn_policy.rs) · [`conversation.rs`](../../crates/edgecrab-core/src/conversation.rs) `check_tool_argument_budget`.

**First principle:** Budget is **correct physics** for local LLMs; the harness must steer models to **`patch`** / ranged reads **before** they attempt whole-file writes — that steering is missing.

---

### Why 4 — Why could the agent not verify visual UX?

**Because** verification tools were **blocked by policy**: `browser_navigate(file://…)` (scheme) and `http://localhost:8000/…` (SSRF). No alternate **perception path** exists for “see the game.”

```text
  intent: "more beautiful UI"
       │
       ├─ browser_navigate(file://)  → permission_denied
       ├─ python http.server        → ok
       ├─ browser_navigate(localhost)→ SSRF blocked
       └─ node -c / grep HTML       → syntax theater (not UX)
```

**Code law:** `edgecrab-security` SSRF · browser tool permission model.

**PI-style insight:** Embodied agents treat **observation** as mandatory between actions. EdgeCrab allowed **act without sense** — see [006-comparator-hermes-claude-pi.md](./006-comparator-hermes-claude-pi.md).

---

### Why 5 — Why does the harness allow this class of task to fail silently?

**Because** completion semantics still conflate **stream finished**, **model spoke**, and **task verified**. Vague prompts have no acceptance criteria; `DefaultCompletionPolicy` may mark budget/interrupted runs inconsistently with operator expectations; todo/report_task_status are **not** wired as hard gates for creative tasks.

```text
  CONCEPTS TODAY (too conflated)
  ┌────────────────┬──────────────────────────────────────────┐
  │ StreamEvent::Done│ transport ended                         │
  │ final_response   │ model produced text                     │
  │ agent:done hook    │ gateway delivered                       │
  │ completed flag     │ often ≈ not interrupted + non-empty text│
  │ RunOutcome         │ exists but not all surfaces use it    │
  └────────────────┴──────────────────────────────────────────┘

  REQUIRED SEPARATION (ADR-001)
  progress ≠ completion ≠ delivery ≠ verification
```

**Code law:** [`completion_assessor.rs`](../../crates/edgecrab-core/src/completion_assessor.rs) · [`agent_harness/001_adr_unified_agent_harness.md](../agent_harness/001_adr_unified_agent_harness.md).

---

## Root cause statement

> EdgeCrab optimizes **token economics** (spill, indexed tools, mutation budgets) without a matching **perception and completion contract** for tasks that require seeing large artifacts or proving UX outcomes. Operators pay latency for non-streaming cloud providers while the model operates on incomplete observations.

---

## What changes (summary pointer)

See [008-improvement-plan.md](./008-improvement-plan.md) — P0 items directly address Why 2–5; P1 addresses Why 1 provider liveness.
