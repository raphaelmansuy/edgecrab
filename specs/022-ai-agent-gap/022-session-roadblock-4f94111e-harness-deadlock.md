# 022 — Session Roadblock: Harness Deadlock (4f94111e)

**Status:** Deep RCA + **implemented** (2026-07-19 WP-A/B/C + NF-E6/E7)  
**Date:** 2026-07-19  
**Session:** `4f94111e-4e71-473d-a96b-ba04a774844a`  
**Model:** `super-grok/grok-4.5`  
**Task:** Write full 3D chess in `./demos/chess`  
**End:** `interrupted` · ~10 min · **2.55M** input tokens · ~$5.21  
**Authority:** First principles · AE1–AE10 · no flaky heuristics  
**Related:** [021 chess forensics](021-chess-verification-forensics-and-plan-2026-07-19.md) · [019 latches](019-non-flaky-harness-improvement-plan.md)

---

## 0. Verdict (one screen)

| Layer | Result |
|-------|--------|
| Create / polish product | **Succeeded** (existing chess + AI + boot overlay + tests) |
| Engine unit tests | **38/38** (model-reported; node path worked after package.json) |
| Browser evidence (late) | Snapshot **ok**, `node_count=9`, but heading **“3D failed to load”** |
| Harness | **Deadlock** — mutually exclusive blocks, no single heal path |
| Completion | Model final text **reopened** (“do not stop yet”) while heal was impossible |
| Operator | Forced interrupt |

**Roadblock name:** **Preview / Verify / Heal Deadlock**  
Independent guardrails each locally “correct” compose into a **closed action set**: the agent cannot re-serve, cannot shell-debug, cannot navigate, cannot stop.

This is **not** a SuperGrok OAuth failure and **not** primarily a model intelligence failure.

---

## 1. Session coordinates

| Field | Value |
|-------|--------|
| ID | `4f94111e-4e71-473d-a96b-ba04a774844a` |
| Start → end | 20:24:42 → 20:34:37 local |
| Msgs / tools | 168 / 99 |
| Tokens | in **2,546,122** · out 19,206 · cache_read **0** |
| API iterations | **61+** |
| Human | 1 task + 1 “Disable all stock cron” mid-run |
| Synthetic user | **7** harness/system injections |

### Tool mix

| Tool | N | Role |
|------|---|------|
| `terminal` | 33 | serve, tests, curl, headless chrome, **mostly blocked** |
| `patch` | 15 | product polish (AI, WebGL boot) |
| `read_file` | 13 | inspect existing chess |
| `browser_navigate` | 10 | mostly fail/block; 1 late ok |
| `tool_search` | 9 | activate browser; later blocked |
| `browser_snapshot` | 5 | error page → late ok |
| `write_file` | 4 | tests, package.json, capture script |
| `browser_get_images` | 3 | useless thrash (`[]`) |
| `vision_analyze` | 2 | headless PNG: blank canvas |
| `browser_vision` | 1 | thrash-blocked |

### Block counts (tool results)

| Block / outcome | N |
|-----------------|---|
| `preview_latch_block` | **11** |
| `shell_debug_block` | **9** |
| `evidence_thrash_block` | **8** |
| `nav_repeat_block` | **3** |
| `tool_search` as workaround block | **2** |
| `http_error_page` navigates | **2** |
| `empty_document` snapshots | **3** |
| `browser_snapshot` ok | **2** (late) |
| `browser_navigate` ok | **1** (late, `?v=2`) |

---

## 2. Causal timeline

```text
PHASE A — CREATE / AUDIT (~2 min)
  demos/chess already exists from prior session
  skill_view, read_file, engine tests, package.json type:module
  report_task_status: in_progress (sticky)
  product already largely complete

PHASE B — FALSE PREVIEW LATCH (the trap)
  terminal: python http.server → {"reused":true,"bind_ready":true,"port":8000}
  evidence.latch_preview(:8000) WITHOUT content qualification
  browser_navigate → title "Error response" · content_class=http_error_page
  browser_snapshot → empty_document / Error response heading

PHASE C — DEADLOCK (majority of tokens)
  Want re-serve chess dir  → preview_latch_block (11×)
  Want shell diagnose/kill → shell_debug_block (9×)  “use browser evidence”
  Want browser_navigate    → nav_repeat / thrash_block (11× combined)
  Want tool_search         → verification workaround block
  Recovery TEXT says: “heal preview once / start http.server 8000”
  Recovery ACTIONS: all forbidden by other guards
  Model polishes code anyway (15 patches) while verify impossible

PHASE D — FALSE STOP
  Model: “game is in place; preview blocked by latched broken server”
  Assessor: NeedsVerification → “do not stop yet” (no evidence)
  Loop reopens into same deadlock

PHASE E — PARTIAL ESCAPE (too late)
  Headless chrome screenshot + vision_analyze: HUD ok, blank 3D
  Cache-bust navigate http://127.0.0.1:8000/?v=2 → title chess, snapshot ok
  Snapshot includes heading “3D failed to load” (boot overlay)
  content_class still Ok → more get_images thrash
  Operator: interrupt
```

---

## 3. First-principles diagnosis

### 3.1 What “done” must mean (VisualUx)

```text
DONE ⇔ Artifact ∧ Preview(content-correct) ∧ Perceive(Ok)
       ∧ (optional product oracle) ∧ ¬impossible-action-set
```

**Impossible-action-set** is a new law this session forces:

> If the harness has closed every legal recovery action while evidence is missing,  
> the loop must **Escalate** (structured halt + summary), not whisper “heal” and reopen.

### 3.2 The deadlock as a composition failure

Four policies, each justified in isolation:

| Policy | Local intent | Session effect |
|--------|--------------|----------------|
| **PreviewLatch** | Stop port shopping / re-serve thrash | Latched **Error response** process; forbade heal re-serve |
| **Shell debug block** | Force browser evidence on VisualUx | Forbade kill/lsof/heal that latch message required |
| **Nav thrash / evidence thrash** | Stop identical fail retries | Forbade browser after Error response ×N |
| **Completion reopen** | No silent incomplete delivery | Forced re-entry into deadlock |

```text
        ┌─────────────────────────────────────────┐
        │  Need evidence (assessor / storm inject) │
        └──────────────────┬──────────────────────┘
                           ▼
              ┌────────────────────────┐
              │  Navigate latched URL  │──fail──► thrash lock
              └────────────┬───────────┘
                           │ success path blocked early
                           ▼
              ┌────────────────────────┐
              │  Re-serve / heal port  │──block──► preview_latch
              └────────────┬───────────┘
                           ▼
              ┌────────────────────────┐
              │  Shell kill / diagnose │──block──► shell_debug
              └────────────┬───────────┘
                           ▼
              ┌────────────────────────┐
              │  Stop with product     │──block──► do not stop yet
              └────────────────────────┘
                           │
                           └──► infinite token burn until human interrupt
```

**Law (anti-flaky):** Guardrails must form a **single state machine** with an always-nonempty **allowed action set** for each phase. Composing independent blocklists without a global invariant is flaky by construction.

### 3.3 Why messages are worse than silence

Thrash block text:

> “Escalate or change strategy after **healing the preview server once**.”

Shell block text:

> “call terminal with `python3 -m http.server 8000 --directory <demo-dir>`”

Preview latch text:

> “do **not** start another http.server”

These three strings were all true in different modules and **false together**. That is prompt-level flakiness: the model is ordered to perform an action the harness will refuse.

### 3.4 Content qualification failure (same family as 021)

```json
{"ok":true,"reused":true,"bind_ready":true,"port":8000}
```

then navigate:

```json
{"ok":false,"title":"Error response","content_class":"http_error_page"}
```

Latch should never stick on transport-only reuse. `http_error_page` must **dirty/clear** PreviewLatch or never set it.

### 3.5 Late “success” still not Done

After `?v=2`:

- Snapshot `content_class=ok`, `node_count=9`
- A11y tree includes **heading “3D failed to load”** + Reload button
- Vision: blank canvas
- Harness has no class for **app-level failure overlay** (deterministic title/heading markers)
- `browser_get_images` thrash (`[]` thrice) — not evidence, not blocked early enough after snapshot ok

### 3.6 Sticky ledger + reopen

- `report_task_status` stayed `in_progress`
- Assessor Incomplete / NeedsVerification path fired once with full delivery prose present
- Document tasks have `document_done_latch_ready` to stop reopen; VisualUx has **no** twin when evidence impossible **or** when DOM ok

---

## 4. What is *not* the roadblock

| Hypothesis | Verdict |
|------------|---------|
| SuperGrok auth broken | No — 61 API turns succeeded |
| Model cannot code chess | No — product + 38 tests + features landed |
| Need smarter prompts only | No — actions were **structurally** forbidden |
| SSRF / preview disabled | Partial — navigates reached localhost; content was wrong |
| Budget exhausted | No — `interrupted` by human |

---

## 5. Improvement plan (no flaky behavior)

### 5.1 Non-negotiable principles

| ID | Principle | Design rule |
|----|-----------|-------------|
| **FP1** | Model proposes; harness judges | Done/Halt from state, not prose |
| **FP2** | Transport ≠ content | Reuse/latch only after content probe |
| **FP3** | Allowed-action invariant | Every phase has ≥1 legal tool class |
| **FP4** | One recovery recipe | Heal once, exclusive; supersede contradictory blocks |
| **FP5** | Exact fingerprints | Keep thrash; clear on content-class change |
| **FP6** | Escalate, don’t loop | Closed action set → `GuardrailHalt` + structured summary |
| **FP7** | No contradictory injects | Block messages must list **currently allowed** tools only |
| **FP8** | Cache-stable system prompt | State in messages / trackers only |
| **FP9** | Test the deadlock | Golden fixture from this session |
| **FP10** | No soft scores | Boolean latches + exit codes + exact heading allowlist |

**Denylist:** more `[harness]` spam; alternate random ports; “try again” without state change; vision prose as sole gate; soft 0.7 done scores.

---

### 5.2 Target state machine (single owner)

```text
EvidencePhase (session-scoped, sole owner of pre_dispatch allows):

  Create
    allow: mutations, tests, skill, todo
    → on first html/js under target dir: Artifact=true

  PreviewBind
    allow: exactly one content-qualified serve OR reuse-if-probe-ok
    probe: GET / → not HttpErrorPage, body not empty error page
    fail probe → do NOT latch; stay PreviewBind (budget N)
    success → PreviewLatch + phase=Perceive

  Perceive
    allow: navigate(latched URL once) + snapshot/vision budget M
    HttpErrorPage / EmptyDocument → phase=Heal (clear thrash counts for heal only)

  Heal  (at most once per dirty cycle)
    allow EXCLUSIVELY:
      - terminal kill latched port PID (if known) OR
      - terminal exact: python3 -m http.server {port} --directory {dir}
    forbid: shell debug thrash, port shopping, navigate until re-probe
    success probe → Preview re-latch, phase=Perceive
    fail → phase=Escalated

  LatchedDone  (Perceive Ok: snapshot node_count>0, not fail overlay)
    allow: final text only (or report_task_status completed)
    forbid: browser_*, re-serve, shell debug
    assessor: Completed; reopen=false

  Escalated
    hard stop GuardrailHalt
    user_summary: structured {artifact, preview_fail_class, blocks_hit, heal_used}
    reopen=false
```

**Invariant (assert in tests):**  
`allowed_tools(phase)` is never empty until Escalated or LatchedDone.

---

### 5.3 Work packages (ordered)

#### WP-A — Break the deadlock (P0)

| ID | Change | Acceptance |
|----|--------|------------|
| **A1** | Content probe before `latch_preview`; refuse latch on `http_error_page` / empty title+body | unit |
| **A2** | On structured browser result `HttpErrorPage` or `EmptyDocument` against latched URL: **clear PreviewLatch** (or mark dirty) and enter **Heal** with heal_budget=1 | unit + e2e |
| **A3** | In Heal: **disable** `shell_debug_block` and `preview_latch_block` for the **one** allowed re-serve command (exact argv shape) | unit |
| **A4** | Unify block messages via `allowed_action_message(phase)` — never mention a blocked command | unit |
| **A5** | When thrash_blocked **and** heal_budget exhausted → `GuardrailHalt` immediately (no more LLM tool turns) | e2e |
| **A6** | When thrash_blocked **and** heal_budget>0 → inject **one** superseding message listing only the heal command (not “use browser”) | unit |

#### WP-B — Completion truth (P0)

| ID | Change | Acceptance |
|----|--------|------------|
| **B1** | `visual_evidence_complete` → Completed + reopen false | unit |
| **B2** | Escalated / closed action set → reopen false; do **not** inject “do not stop yet” | unit |
| **B3** | Sticky `in_progress` ignored when Escalated or visual_evidence_complete | unit |
| **B4** | Model final text while in deadlock → Halt with evidence debt, not reopen into same blocks | e2e from 4f94111e |

#### WP-C — App-level failure markers (P1, deterministic)

| ID | Change | Acceptance |
|----|--------|------------|
| **C1** | Exact a11y/title markers: `"3D failed to load"`, `"Error response"` already → not Perceive Ok | unit |
| **C2** | After snapshot Ok, block `browser_get_images` unless images listed in contract (default block) | unit |
| **C3** | Optional WebGL pixel oracle later; **not** required for Halt/Done DOM gate | — |

#### WP-D — Golden tests (P0 with A/B)

| ID | Fixture |
|----|---------|
| **D1** | Reuse :8000 Error response → no latch → heal once → chess title → Done |
| **D2** | Latch Error → without A2 behavior would deadlock; with A2 Heal then Escalated if heal fails |
| **D3** | Closed action set never coexists shell_block ∧ latch_block ∧ thrash_block without Escalated |

---

### 5.4 Code ownership (SOLID)

| Concern | Module |
|---------|--------|
| Phase + allowed tools | `evidence_latch.rs` (extend; single owner) |
| pre_dispatch reads phase only | `turn_dispatch_policy.rs` |
| Latch record + probe | `turn_dispatch.rs` + tools `dev_server` content probe |
| Content class markers | `structured_browser.rs` |
| Done / reopen | `completion_assessor.rs`, `turn_epilogue.rs` |
| Hard stop exit | `conversation.rs` |

No second “deadlock detector” service — **phase enum is the detector**.

---

### 5.5 Acceptance criteria (operator)

| ID | Criterion |
|----|-----------|
| **AC1** | No session spends > **verify_tool_budget** tools after first `http_error_page` without either Heal success or GuardrailHalt |
| **AC2** | Zero instances of three concurrent block classes without Escalated (assert in e2e) |
| **AC3** | After snapshot Ok with non-fail markers → ≤0 browser tools; exit Completed |
| **AC4** | Replay of 4f94111e early sequence ends Halt or Completed in **&lt; 15** verify tools after first Error response |
| **AC5** | Block tool results never instruct a command that pre_dispatch will reject |

---

### 5.6 Explicit non-goals

- Multi-agent vision GAN evaluator  
- Soft “looks like chess” scores  
- Allowing unlimited port shopping  
- Disabling all guardrails  
- Fixing Three.js blank canvas as a harness problem (product bug; DOM gate is enough for stop/escalate)

---

## 6. Comparison to prior chess session (809b1e39)

| Dimension | 809b1e39 | **4f94111e (this)** |
|-----------|----------|---------------------|
| Tokens | 0.6M | **2.5M** |
| Port | 8765 (eventually correct) | **8000 wrong latch prolonged** |
| DOM success | Yes mid-run | Yes only at end + fail overlay |
| Deadlock severity | Moderate (post-success thrash) | **Severe (pre-success closed set)** |
| “do not stop yet” | Not observed | **Yes** while heal impossible |
| Primary bug | Latches not load-bearing | **Latch + block composition** |

021 plan (wire latches, content probe) remains necessary. **This session adds: Heal phase + allowed-action invariant + no reopen on Escalated.**

---

## 7. Recommended ship order

```text
A1–A2 (content latch + dirty on error)   ─┐
A3–A4 (Heal exclusive allow + messages) ─┼─► D1–D3 e2e
A5–A6 + B1–B4 (halt / done / no reopen) ─┘
         │
         ▼
C1–C2 (fail overlay + get_images thrash)
```

---

## 8. Success definition

EdgeCrab verification is **non-flaky** for VisualUx when:

1. Wrong reuse cannot trap the agent.  
2. Failure always leaves a **legal next action** or a **hard stop**.  
3. “Heal” in a message implies heal is **executable**.  
4. Completed requires evidence; Incomplete never reopens into an empty action set.  
5. Mean tokens after first `http_error_page` is bounded (budget), not multi-million.

---

*Code is law. Session `4f94111e` is the golden deadlock fixture.*
