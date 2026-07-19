# 021 — SuperGrok 3D Chess Session: Verification Forensics + Reliability Plan

**Status:** Root-cause note + implementation plan (code is law)  
**Date:** 2026-07-19  
**Session:** `809b1e39-35ca-44a4-96d9-016cd0a2fd28`  
**Profile / DB:** `homelab` · `~/.edgecrab/profiles/homelab/state.db`  
**Model:** `super-grok/grok-4.5` (xAI OAuth path — working)  
**Product:** `demos/chess` (Three.js 3D chess — **created successfully**)  
**End reason:** `interrupted` (operator stop) after ~10 min verify thrash  
**Authority:** AE1–AE10 · FP1–FP10 (019) · July 2026 agent-engineering practices  
**Related:** [017 forensics](017-session-forensics-2026-07-19.md) · [018 practices](018-non-flaky-harness-best-practices-2026-07.md) · [019 plan](019-non-flaky-harness-improvement-plan.md)

---

## 0. Executive verdict

| Layer | Result |
|-------|--------|
| **OAuth / SuperGrok** | Working — chat, tools, multi-file create |
| **Create path** | **Success** — full game under `demos/chess/` |
| **Serve / DOM verify** | Eventually OK (title, 6 a11y nodes, buttons) |
| **Vision verify** | Structured `content_class: ok` but analysis: **blank WebGL canvas** |
| **Harness verify loop** | **Failed reliability** — thrash after product shipped; no clean stop |
| **Exit** | `interrupted` — not `Completed` |

**One-line law:** EdgeCrab can *build* visual demos with SuperGrok; it still cannot *reliably finish* them because **evidence latches are recorded but not load-bearing** in completion, reopen, and post-success thrash policy.

Waves A–D in [019](019-non-flaky-harness-improvement-plan.md) shipped unit/e2e plumbing. This session proves residual **production wiring gaps** and a **preview-reuse correctness bug**.

---

## 1. Session coordinates (SQLite is law)

| Field | Value |
|-------|--------|
| ID | `809b1e39-35ca-44a4-96d9-016cd0a2fd28` |
| Title (DB) | `hello` (first turn was greeting; task in turn 2) |
| Wall clock | 20:02:24 → 20:12:31 local (~10.1 min) |
| End | **`interrupted`** |
| Messages / tools | 88 / 53 |
| Tokens | in **596,789** · out 21,664 · cache_read **0** |
| Est. cost | ~$1.32 |
| API iterations | **32** (harness.jsonl) |
| Human turns | 2 real (`hello`, chess task) + 3 synthetic system/harness |

### Role mix

| Role | N | Notes |
|------|---|--------|
| user | 5 | 2 human + 2 stream-interrupt recovery + 1 iteration-storm advisory |
| assistant | 30 | mix of prose + pure tool turns |
| tool | 53 | create then verify thrash |

### Tool mix

| Tool | N | Phase |
|------|---|--------|
| `terminal` | 12 | mkdir, serve, kill-port, blocked re-serve |
| `write_file` | 8 | product |
| `patch` | 4 | product fixes |
| `browser_navigate` | 5 | thrash + reload + **wrong path** `…/js/main.js` |
| `browser_snapshot` | 3 | empty → empty → **ok** |
| `browser_click` | 4 | Flip/New/Undo after DOM ok |
| `browser_vision` | 1 | UI ok, canvas blank |
| `browser_console` | 2 | 0 errors |
| `tool_search` | 5 | activate browser tools (churn) |
| `report_task_status` | 1 | **stuck `in_progress`** |
| `manage_todo_list` | 1 | **0 completed** |

### Product on disk (success)

```text
demos/chess/
  index.html, styles.css, README.md
  js/{main,chess,board,pieces}.js
  scripts/check.sh
```

Server at end of investigation: `python -c … SimpleHTTPRequestHandler` on **:8765** serving chess (title `♔ 3D Chess — Three.js`).

---

## 2. Timeline (causal story)

```text
T0  Human: "hello" → assistant greets
T1  Human: Write full 3D chess in ./demos/chess
T2  CREATE  skill_view(three*) → todos → write/patch ×12 files
    · Stream inter-chunk timeout 60s → "incremental edit" recovery inject
    · report_task_status: in_progress + remaining_steps (never updated)
    · manage_todo_list: 5 todos, none completed
T3  VERIFY  tool_search browser → terminal "serve" on 8765
    · CRITICAL: tool returns reused:true, bind_ready:true on EXISTING :8765
    · Existing process was NOT chess (empty title / API "Not Found" pre body)
    · evidence.latch_preview() fires → clean PreviewLatch
T4  browser_navigate ok (empty title, content_class still "ok")
    browser_snapshot empty_document (node_count=0)
T5  Model tries other ports / restart
    · pre_dispatch: preview_latch_block (correct vs port shopping)
    · Model fights the latch instead of healing content on latched URL
T6  After kill/rebind: navigate title "3D Chess", snapshot node_count=6 OK
    wait_for "White", click New/Flip/Undo, console clean
T7  browser_vision content_class=ok
    body: polished HUD **but blank WebGL canvas** (real product signal)
T8  Model does NOT stop or fix render deterministically
    · navigates to /js/main.js (raw module source as "page")
    · read_file thrash, more console, second stream timeout recovery
T9  Operator interrupt → exit_reason=interrupted
```

### Harness injects observed

1. **Iteration storm advisory** (warn only):  
   `harness: iteration storm without perception evidence act_tools=6 window_secs=60 task_class=VisualUx`  
   → user message: mutation without verification recipe.
2. **Stream partial tool-call recovery** ×2 (LM Studio copy used for Grok 4.5 too): forces incremental-edit behavior mid-create and mid-verify.
3. **Preview latch block** ×3: correct against re-serve; wrong if latch was on alien process.
4. **No** “do not stop yet” completion reopen visible — run never reached clean model-final-text after latched DOM/vision; operator cut it.

---

## 3. Root causes (First Principles, ordered)

### RC-0 — Epistemic law (what “done” must mean)

For `TaskClass::VisualUx`:

```text
DONE ⇔ ArtifactLatch ∧ PreviewLatch(content-correct) ∧ PerceiveLatch(Ok)
       ∧ (optional product oracle) ∧ ¬sticky InProgress ledger
```

Anything weaker (prose, navigate transport, reused port without content probe) is flaky.

---

### RC-1 — **Evidence latches are not load-bearing** (primary harness bug)

| Symbol | Status |
|--------|--------|
| `EvidenceState::visual_evidence_complete()` | **Implemented** (`evidence_latch.rs`) |
| Used by `completion_assessor` | **No** |
| Used by `should_reopen_loop_with_messages` | **No** (only `document_done_latch_ready`) |
| Used by pre_dispatch post-success thrash halt | **No** |
| `verify_budget_exhausted()` hard-stop in loop | **Not wired to terminal exit** |

**Why it hurts:** After snapshot `ok` + vision `ok`, the model can keep calling navigate/click/console forever. Assessor still walks message bags; reopen policy has Document twin, not Visual twin.

**Code anchors:**

- `crates/edgecrab-core/src/evidence_latch.rs` — `visual_evidence_complete`, `verify_budget_exhausted`
- `crates/edgecrab-core/src/completion_assessor.rs` — `CompletionContext` has **no** `EvidenceState`
- `crates/edgecrab-core/src/turn_epilogue.rs` — `should_reopen_loop_with_messages` Document-only
- `crates/edgecrab-core/src/conversation.rs` — document done latch early-exit; no visual twin

---

### RC-2 — **Preview latch on reused alien process** (correctness bug)

First “serve” result (msg `74467`):

```json
{
  "ok": true,
  "reused": true,
  "bind_ready": true,
  "port": 8765,
  "process_id": null,
  "note": "Preview server already listening on port 8765 — reused existing bind"
}
```

Then:

1. `turn_dispatch::record_tool_outcome` calls `latch_preview(..., http_status: Some(200))` **without HTTP content probe**.
2. Snapshots: `content_class: empty_document`, body includes `{"detail":"Not Found"}` (API shape, not chess HTML).
3. Pre-dispatch blocks further `http.server` with `preview_latch_block`.
4. Model spends many tools fighting the latch instead of “heal once then re-latch.”

**Law:** Bind-ready TCP ≠ serve-correct directory. Reuse must be **content-qualified** (GET `/` title or marker) before latch.

---

### RC-3 — **Navigate `content_class: ok` with empty title** (weak content class)

Empty-title navigate still classifies as `Ok` (no node_count on navigate).  
`note_perceive` latches **any** `ContentClass::Ok` from navigate/snapshot/vision.

Session:

| Call | title | content_class | Should latch Perceive? |
|------|-------|---------------|------------------------|
| navigate empty | `""` | `ok` | **No** (weak) |
| snapshot empty | — | `empty_document` | No (clears) |
| navigate + snapshot chess | good | `ok` | **Yes** |
| vision | — | `ok` | Yes (DOM/UI) |

`visual_perception_evidence_ok` already requires snapshot `node_count > 0` for snapshot tool, but **navigate alone** returns true → can pollute evidence lists and latch state.

---

### RC-4 — **Sticky progress ledger blocks honest completion**

```json
// report_task_status (never updated)
{"status":"in_progress","remaining_steps":[...4 items...]}

// manage_todo_list (never updated)
summary: completed=0, in_progress=1, not_started=4
```

Assessor (`completion_assessor.rs`):

```text
reported_in_progress || has_remaining_steps || active_todos > 0
  → CompletionDecision::Incomplete
```

Even with perfect browser evidence, if the model tried to stop with final text, the loop would **reopen** with “do not stop yet.” Model never marked todos done — **no harness auto-clear when visual_evidence_complete**.

---

### RC-5 — **Post-success thrash is only soft-bounded**

After DOM ok + vision:

- `browser_navigate` → `/js/main.js` (source as page) still `content_class: ok`
- clicks, console, read_file, tool_search continue
- thrash fingerprint clears on success (`note_perceive` clears counts on Ok)
- `verify_tool_budget` (default 12) counts but **does not hard-stop conversation**

Storm advisory is **rate-limited whisper**, not `GuardrailHalt`.

---

### RC-6 — **Stream inter-chunk timeout (60s) mid-flight**

Logs:

```text
api_call_streaming: inter-chunk timeout (60s) elapsed — stream stale
tool-call draft interrupted before delivery — injecting incremental-edit recovery
```

Happened twice (create + verify). Recovery text is LM-Studio-shaped and pushes more file mutations when the real issue is **preview content / WebGL**, adding noise and token burn.

---

### RC-7 — **Vision `ok` vs product truth (canvas blank)**

Vision envelope:

- `content_class: ok`, `ok: true` (document ready)
- Prose: HUD perfect; **3D scene absent / blank canvas**

Harness treats vision Ok as **perceive complete**. That is correct for “page loads” gate, but **not** for “Three.js scene rendered.” For WebGL demos need either:

1. **DOM-level done** (ship when HUD + canvas element + zero console errors), or  
2. **Pixel oracle** (non-uniform canvas pixels / WebGL draw count via `browser_evaluate`), not free-form vision prose.

Session model chose (2) via prose and then thrash-debugged without a structured pixel check.

---

### RC-8 — Secondary noise (not primary)

| Signal | Impact |
|--------|--------|
| AGENTS.md blocked (prompt injection: edgecrab_env/hermes_env) | Less project context; not cause of thrash |
| `cache_read_tokens=0` | SuperGrok path may not report cache; higher $ |
| Title stuck as `hello` | Session UX; not loop physics |
| LSP unavailable for README.md | Noise only |

---

## 4. Five Whys (compressed)

1. **Why interrupted?** Operator stopped a 10-min run that would not finish.  
2. **Why no finish?** Agent kept verifying/debugging after DOM ok; no hard done latch.  
3. **Why keep verifying?** Vision said blank canvas + sticky in_progress + no post-success thrash halt.  
4. **Why so much early thrash?** Preview latched on **reused wrong process**; empty snapshots; latch forbade healthy re-serve.  
5. **Why latch wrong process?** Reuse path sets bind_ready without **content qualification**; assessor/loop never require `visual_evidence_complete`.

---

## 5. What 019 already fixed vs what chess still broke

| 019 claim | Chess reality |
|-----------|---------------|
| ContentClass | Worked for empty_document; **failed** weak navigate Ok |
| Preview latch | Fired — but on **alien** reuse |
| Thrash fingerprint | Not the main post-success problem (success clears it) |
| verify budget | Counted in state — **no exit** |
| visual_evidence_complete | **Dead code path** in production loop |
| Document done latch twin | **Missing** for VisualUx |
| MediaRender path | N/A (chess is VisualUx) |

---

## 6. Improvement plan (make verification reliable)

### Design principles (July 2026, non-negotiable)

| ID | Principle | Rule for this plan |
|----|-----------|--------------------|
| **AE1** | Bounded autonomy | Hard stop after latched evidence or verify budget |
| **AE3** | Completion = evidence | `visual_evidence_complete` is necessary for VisualUx Completed |
| **FP1** | Model proposes; harness judges | Never “model said blank canvas” alone as infinite loop |
| **FP2** | Transport ≠ content ≠ task | Reuse require content probe |
| **FP4** | Success is latched | After Perceive Ok, block verify thrash tools |
| **FP5** | Exact fingerprints | Keep; add post-success **allowlist** not soft scores |
| **FP7** | Phased budgets | Create vs verify; escalate once |
| **FP8** | Cache-stable system prompt | Inject gates only into messages |
| **FP10** | No flaky heuristics | No “probably done” scores; boolean latches + exit codes |

**Generator ≠ evaluator:** optional Phase 3 cheap judge only for WebGL pixel class; default is DOM+console.

---

### Architecture target

```text
                    ┌──────────────────────────────┐
                    │  EvidenceState (session)       │
                    │  Artifact → Preview → Perceive │
                    └──────────────┬───────────────┘
                                   │
     record_tool_outcome ──────────┤
     (only content-qualified)      │
                                   ▼
┌──────────────┐   visual_done?   ┌─────────────────────┐
│ pre_dispatch │◄──yes────────────│ blocks post-success │
│ thrash halt  │                  │ browser_* / re-serve│
└──────┬───────┘                  └──────────▲──────────┘
       │                                     │
       ▼                                     │
┌──────────────┐  assess_completion  ┌───────┴──────────┐
│ conversation │────────────────────►│ CompletionContext │
│ loop reopen  │  visual_done?       │ + EvidenceState   │
└──────────────┘  → Completed /      └───────────────────┘
                  stop reopen
```

---

### Phase V1 — Wire latches into the loop (P0, 1–2 days)

**Goal:** Chess-shaped sessions stop cleanly when DOM evidence is latched; thrash cannot continue.

| ID | Change | Owner crate | Acceptance |
|----|--------|-------------|------------|
| **V1.1** | Add `evidence: &EvidenceState` (or snapshot) to `CompletionContext` | core | unit |
| **V1.2** | VisualUx Completed requires `visual_evidence_complete()` when `evidence_latches` | assessor | unit + e2e |
| **V1.3** | `should_reopen_loop_with_messages`: if VisualUx && visual_evidence_complete → **false** (twin of document_done_latch) | epilogue | unit |
| **V1.4** | Mid-loop early exit when model returns text **and** visual_evidence_complete (mirror document path in `conversation.rs`) | conversation | e2e |
| **V1.5** | pre_dispatch: if `perceive.is_some()` && clean → block `browser_navigate|snapshot|vision|click|console` **except** one optional `browser_evaluate` diagnostic budget (0 default) | turn_dispatch_policy | unit |
| **V1.6** | `verify_budget_exhausted` → `ExitReason::GuardrailHalt` or `BudgetExhausted` with single structured summary (no more tools) | conversation | e2e |
| **V1.7** | When visual_evidence_complete, auto-clear sticky `report_task_status` incompleteness **for assess only** (or inject one force-complete advisory max once) | assessor | unit |

**Non-goal V1:** Fix WebGL blank canvas product quality.

---

### Phase V2 — Content-qualified preview latch (P0, same or next day)

| ID | Change | Acceptance |
|----|--------|------------|
| **V2.1** | On `reused:true` or any `latch_preview`, require GET `http://127.0.0.1:{port}/` with deterministic markers: HTTP 200 + body contains demo path fingerprint OR title non-empty and not HttpErrorPage allowlist | unit + chess replay fixture |
| **V2.2** | Fail content probe → **do not latch**; return structured error `preview_content_mismatch` with heal recipe once | unit |
| **V2.3** | Snapshot body containing exact `{"detail":"Not Found"}` / FastAPI-style API JSON → `HttpErrorPage` or new `WrongService` class (exact prefix, not free text) | unit |
| **V2.4** | Empty-title navigate → not Perceive latch; only snapshot(node_count>0) or vision(document_ready) can set Perceive | unit (amend note_perceive policy) |
| **V2.5** | One **heal** slot: clear PreviewLatch on content fail fingerprint thrash limit, allow **one** re-serve of same dir/port | e2e pinguin/chess |

---

### Phase V3 — WebGL / canvas product gate (P1, optional for demos)

| ID | Change | Acceptance |
|----|--------|------------|
| **V3.1** | Task subclass or hint: `visual_ux.webgl` when Three.js/importmap detected | unit |
| **V3.2** | Structured evaluate: `canvas` present + `getContext('webgl')` + non-zero draw proxy **or** pixel sample variance | tool test |
| **V3.3** | Vision prose never infinite-loops; if canvas oracle fails once → single fix budget (N mutations) then escalate to operator with screenshot path | e2e |
| **V3.4** | Default Done for VisualUx remains DOM+console unless contract/goal says “3D scene visible” | unit |

---

### Phase V4 — Stream & ledger hygiene (P1)

| ID | Change | Acceptance |
|----|--------|------------|
| **V4.1** | SuperGrok: raise inter-chunk timeout or treat partial tool JSON as retry-without-LM-Studio prose when provider is xai | unit config |
| **V4.2** | Auto-seed VisualUx goals/subgoals from `visual_ux_auto_subgoals()` (already in evidence_latch) on first artifact | e2e |
| **V4.3** | Auto-complete todos when visual_evidence_complete (or mark cancelled with reason) so active_todos cannot reopen | unit |
| **V4.4** | Session title from first **task** user message if title is greeting | CLI polish |

---

### Phase V5 — Observability & golden replay (P1 continuous)

| ID | Change |
|----|--------|
| **V5.1** | Persist EvidenceState snapshot into harness.jsonl each tool turn |
| **V5.2** | Golden fixture from session `809b1e39` tool sequence (create→reuse fail→empty→ok→vision→post thrash) in `tests/harness_nonflaky_e2e.rs` as **NF-E6 chess** |
| **V5.3** | Operator metrics: % VisualUx sessions ending Completed vs interrupt/budget; mean verify_tools after first Perceive Ok (target **0**) |

---

## 7. Implementation order (DAG)

```text
V2.1–V2.4 (content-qualified latch + perceive policy)
    │
    ▼
V1.1–V1.7 (wire complete / reopen / thrash / budget)
    │
    ├──────────────► V5.2 golden chess e2e
    │
    ▼
V4 ledger/stream hygiene
    │
    ▼
V3 WebGL oracle (only if product gate required)
```

**Ship gate for “reliable”:** NF-E6 + existing NF-E1…E5 green; live chess replay ends `model_returned_final_text` or clean `GuardrailHalt` with evidence summary — **never multi-minute thrash after first good snapshot**.

---

## 8. Acceptance criteria (operator-visible)

### AC-Chess-Happy (DOM gate)

Given SuperGrok + request “3D chess in demos/chess”:

1. Artifacts land under `demos/chess/`.  
2. At most **one** content-qualified preview latch.  
3. `browser_snapshot` with `node_count > 0` and non-error title → Perceive latch.  
4. **Zero** further browser_* tools after latch (unless V3 enabled).  
5. Exit `Completed` / `ModelReturnedFinalText` with evidence lines listing snapshot/vision.  
6. Input tokens after first good snapshot **&lt; 50k** (no thrash burn).

### AC-Chess-Reuse-Fail

Given alien process already on target port:

1. Reuse does **not** latch without content match.  
2. Structured heal once; no port shopping.  
3. No `preview_latch_block` trap on wrong content.

### AC-Chess-Budget

If evidence never arrives: after `verify_tool_budget` (default 12), hard stop with summary — not silent interrupt dependency.

---

## 9. Explicit denylist (do not ship)

| Forbidden | Why |
|-----------|-----|
| More `[harness]` spam without blocks | Chess already ignored storm advisory |
| Soft “vision says good enough” score | Flaky; use DOM/pixel booleans |
| Random alternate ports in recovery | Port shopping |
| Treating empty-title navigate as Perceive | Session RC-3 |
| Leaving `visual_evidence_complete` uncalled | Dead code recurrence |
| Auto-complete on vision alone when canvas contract required without oracle | False done on blank WebGL |

---

## 10. Test plan (minimal high leverage)

```bash
# unit
cargo test -p edgecrab-tools --lib structured_browser
cargo test -p edgecrab-core --lib evidence_latch
cargo test -p edgecrab-core --lib completion_assessor
cargo test -p edgecrab-core --lib turn_dispatch_policy
cargo test -p edgecrab-core --lib turn_epilogue

# e2e
cargo test -p edgecrab-core --test harness_nonflaky_e2e
# after V5.2:
cargo test -p edgecrab-core --test harness_nonflaky_e2e nf_e6_chess
```

Fixtures:

1. Reuse port with API JSON body → no latch.  
2. Empty snapshot thrash ×3 → thrash_blocked.  
3. Good snapshot → visual_evidence_complete → pre_dispatch blocks click.  
4. Sticky in_progress + complete evidence → assess Completed.  
5. verify_tools ≥ budget → GuardrailHalt.

---

## 11. Config knobs (defaults)

| Key | Default | Role |
|-----|---------|------|
| `harness.evidence_latches` | `true` | Master |
| `harness.verify_tool_budget` | `12` | Hard stop after V1.6 |
| `harness.thrash_fingerprint_limit` | `3` | Fail thrash |
| `harness.post_perceive_browser_budget` | **`0`** (new) | Tools allowed after Perceive Ok |
| `harness.preview_content_probe` | **`true`** (new) | V2 |
| `harness.webgl_pixel_oracle` | `false` | V3 |

Rollback: `evidence_latches: false` + `preview_content_probe: false` restores pre-plan behavior.

---

## 12. Risk register

| Risk | Mitigation |
|------|------------|
| False Completed when canvas blank | V1 = DOM gate only; V3 opt-in for 3D contract |
| Heal slot abused for infinite re-serve | Single heal counter on EvidenceState |
| SuperGrok stream timeouts | V4.1 provider-specific timeout |
| AGENTS.md blocked | Separate injection allowlist for known project docs (out of scope) |

---

## 13. Success definition (product)

EdgeCrab + SuperGrok is **reliable for visual demos** when:

1. Create succeeds (already true).  
2. Verify is a **finite state machine** with hard exits.  
3. Operator sees Completed with screenshot path, not a 10-minute thrash ending in interrupt.  
4. Mean verify tools after first good snapshot → **0**.

---

## 14. Recommended next action

Implement **V2 + V1** as a single PR series:

1. Content-qualified preview + perceive policy  
2. Wire `visual_evidence_complete` through assessor / reopen / pre_dispatch / budget  
3. NF-E6 chess golden test from this session  

Defer V3 WebGL oracle until DOM reliability is green.

---

## Appendix A — Key log evidence

```text
WARN harness: iteration storm without perception evidence act_tools=6 … task_class=VisualUx
WARN api_call_streaming: inter-chunk timeout (60s) … tool-call draft interrupted
WARN Prompt injection detected … file=AGENTS.md threats=["edgecrab_env","hermes_env"]
```

Preview reuse JSON: see msg id `74467`.  
Empty snapshot: msg `74472` / `74478` (`empty_document`).  
Good snapshot: msg `74494` (`node_count: 6`).  
Vision blank canvas: msg `74506` + `browser_vision_1784463029.png`.

## Appendix B — Code map (change surface)

| File | Touch |
|------|-------|
| `edgecrab-core/src/evidence_latch.rs` | heal counter, post_perceive budget, content-qualified latch API |
| `edgecrab-core/src/turn_dispatch.rs` | probe before latch_preview; note_perceive policy |
| `edgecrab-core/src/turn_dispatch_policy.rs` | post-success block |
| `edgecrab-core/src/completion_assessor.rs` | EvidenceState in context |
| `edgecrab-core/src/turn_epilogue.rs` | visual done twin of document latch |
| `edgecrab-core/src/conversation.rs` | early exit + verify budget halt |
| `edgecrab-tools/src/structured_browser.rs` | WrongService / empty-title policy; API body markers |
| `edgecrab-tools` dev_server / terminal preview | content probe on reuse |
| `edgecrab-core/tests/harness_nonflaky_e2e.rs` | NF-E6 |
| `edgecrab-core/src/config.rs` | new knobs |

---

*End of note. Code is law; this plan is falsifiable against session `809b1e39` replay.*
