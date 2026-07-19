# 019 — Non-Flaky Harness Improvement Plan

**Status:** Waves A–D implemented (2026-07-19) · **022 Heal + completion wiring shipped**  
**Date:** 2026-07-19  
**Authority:** [000-code-is-law](000-code-is-law.md) · [001-first-principles](001-first-principles.md) · [017 forensics](017-session-forensics-2026-07-19.md) · [018 practices](018-non-flaky-harness-best-practices-2026-07.md)  
**Principles:** **First principles** · **no flaky heuristics** · **DRY** · **SOLID** · **e2e-first**

---

## Assessment (shipped)

| Wave | Status | Key modules |
|------|--------|-------------|
| **A** Content + oracles | ✅ | `structured_browser::ContentClass`, `harness_gates::oracle_command_for_path` / `is_browser_esm_artifact` |
| **B** Latch + thrash | ✅ | `evidence_latch::EvidenceState`, pre_dispatch thrash/preview blocks, recovery no alternate port |
| **C** Budgets + class | ✅ | verify budget counters, `TaskClass::MediaRender`, media evidence in assessor |
| **D** E2E | ✅ | `tests/harness_nonflaky_e2e.rs` NF-E1…E5 |

### Tests (2026-07-19)

```text
structured_browser unit     6 passed
harness_gates unit          9 passed (incl. NF-U3/U4)
evidence_latch unit         7 passed (NF-U5…U10)
harness_nonflaky_e2e        5 passed
task_class regression      19 passed
turn_dispatch regression   10 passed
```

### Config

`harness.evidence_latches` (default true), `harness.verify_tool_budget` (12), `harness.thrash_fingerprint_limit` (3).

---

## 0. Intent

Close the **visual_ux / media verify spinlock** observed in production sessions without adding more advisory text, fuzzy thresholds, or “try again” loops.

**Success signal (operator):** After `write_file` of a browser demo, the agent either:

1. **Latches** preview + perceive evidence and stops, **or**  
2. **Escalates** with a single structured failure — never burns multi-million tokens thrashing.

**Non-goal:** Multi-agent GAN evaluator (018 BP-08 heavy path) in v1. Optional later.

---

## 1. First principles (constraints — non-negotiable)

| ID | Principle | Plan rule |
|----|-----------|-----------|
| **FP1** | Model proposes; harness judges **done** | Completion reads **latches + oracles**, not prose |
| **FP2** | Transport ≠ content ≠ task | Separate classifiers; never collapse |
| **FP3** | Oracles match **artifact class** | No global `.js → node --check` |
| **FP4** | Success is **latched** | After latch, re-entry is forbidden until dirty |
| **FP5** | Failure fingerprints are **exact** | Same `(tool, args_shape, error_code, content_class)` only |
| **FP6** | One recovery recipe per class | Supersede advisories; never stack |
| **FP7** | Budgets are **phased** | Create budget ≠ verify budget |
| **FP8** | Cache-stable system prompt | Latches/goals/gates → messages only |
| **FP9** | Test what production broke | Golden fixtures from session IDs in 017 |
| **FP10** | **No flaky heuristics** | See §1.1 denylist |

### 1.1 Explicit denylist (no flaky heuristics)

| Forbidden | Why flaky | Allowed instead |
|-----------|-----------|-----------------|
| “Looks like a demo” path regex alone for oracles | False pos/neg | Explicit class + file peers (importmap HTML) |
| Soft score thresholds (0.7 “probably done”) | Non-reproducible | Boolean latches + exit codes + HTTP status |
| Time-based “maybe thrash” without fingerprint | Clock-sensitive CI | Count of identical fingerprints |
| Random port suggestions in recovery | Port shopping | Single latched port or explicit dirty clear |
| LLM self-judge of “page looks good” as sole gate | Positive bias | DOM markers / chrome-error / title allowlist fail |
| More `[harness]` user spam | Context noise | Rate-limited superseding inject |
| Re-parse free-form stderr for “success vibes” | Brittle | Structured tool JSON fields only |
| Sleep-and-retry without state change | Masks races | `bind_ready` / `wait_for_process` state |

If a change requires a denylisted technique, **reject the design** — do not “ship and tune.”

---

## 2. SOLID / DRY ownership map

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ edgecrab-core                                                            │
│                                                                          │
│  evidence_latch.rs     ← NEW: session latch graph (pure state machine)   │
│  completion_assessor   ← reads latches + content class; no I/O           │
│  task_class            ← class + media_render; no paint                  │
│  harness_advisory      ← rate-limit inject; records fingerprints only    │
│  conversation /        ← thin: update latch on tool result; phase budget │
│    turn_dispatch                                                         │
│  goals (existing)      ← auto-seed criteria; no second progress store    │
└───────────────────────────────┬──────────────────────────────────────────┘
                                │ depends on types + tools APIs
┌───────────────────────────────▼──────────────────────────────────────────┐
│ edgecrab-tools                                                           │
│                                                                          │
│  structured_browser    ← content_class enum (ChromeError, HttpErrorPage…)│
│  harness_gates         ← class-aware oracles (deterministic subprocess)  │
│  dev_server            ← serve detection; feeds PreviewLatch inputs      │
│  recovery_catalog      ← ONE recipe per error code (no alternate ports)  │
│  tool_loop_guardrails  ← fingerprint thrash (exact match)                │
└──────────────────────────────────────────────────────────────────────────┘
         ▲
         │ never imports TUI
┌────────┴─────────┐
│ edgecrab-cli     │  optional /verify surface later
└──────────────────┘
```

| SOLID | Application |
|-------|-------------|
| **S** | Latch state ≠ oracle runner ≠ assessor ≠ recovery text |
| **O** | New task classes register `OraclePolicy` / `LatchRequirements` without rewriting assessor |
| **L** | Any `ContentClass` classifier works with same thrash fingerprint |
| **I** | Assessor needs only `EvidenceSnapshot`; conversation needs only `record_tool_result` |
| **D** | Assessor depends on latch + snapshot traits, not CDP |

| DRY | Application |
|-----|-------------|
| One content classifier | `structured_browser` only |
| One thrash counter | `tool_loop_guardrails` fingerprints |
| One oracle entry | `harness_gates::oracle_for(path, class, peers)` |
| One inject channel | `harness_advisory` supersede map `class → last_msg_id` |
| No second “done” store | Latches + existing goals; no parallel progress.md required in v1 |

---

## 3. Data model (minimal, typed)

```rust
// Conceptual — final names in evidence_latch.rs / structured_browser

pub enum ContentClass {
    Ok,
    ChromeError,      // chrome-error://
    HttpErrorPage,    // title/body Error response, 404 page markers
    EmptyDocument,    // node_count == 0 && !chrome
    TransportFail,    // tool_unavailable / connection
}

pub struct PreviewLatch {
    pub dir: PathBuf,
    pub port: u16,
    pub url: String,
    pub process_id: Option<String>,
    pub bind_ready: bool,
    pub http_status: Option<u16>,
    /// Hash of artifact paths under dir when latched (dirty if writes change).
    pub artifact_fingerprint: u64,
}

pub struct PerceiveLatch {
    pub url: String,
    pub content_class: ContentClass, // must be Ok
    pub evidence_tool: &'static str, // browser_snapshot | browser_vision
}

pub struct EvidenceState {
    pub artifact: bool,
    pub preview: Option<PreviewLatch>,
    pub perceive: Option<PerceiveLatch>,
    pub oracle_ok: bool,
    pub thrash_blocked: bool,
    pub phase: Phase, // Create | Verify | Escalated
}

pub enum Phase {
    Create,
    Verify,
    Escalated,
}
```

**Dirty rule (deterministic):**  
`write_file`/`patch`/`apply_patch` under `preview.dir` → clear `perceive` + optionally clear `preview` if path set changed; recompute `artifact_fingerprint`.

---

## 4. Phased delivery

### Wave A — Truth & oracles (P0, ~3–5 days)

| Step | Work | Owner | FP |
|------|------|-------|-----|
| A1 | `ContentClass` on navigate/snapshot/vision parse | tools | FP2 |
| A2 | Assessor: only `ContentClass::Ok` counts as visual evidence | core | FP1–2 |
| A3 | `oracle_for_path(path, task_class, workspace_peers)` | tools | FP3 |
| A4 | Skip `node --check` for browser ESM (importmap peer HTML or demos/** + type=module script) | tools | FP3, FP10 |
| A5 | Unit tests: ESM fixture must not oracle-fail | tools | FP9 |

**Done when:** Pinguin-style `game.js` does **not** produce harness-gates fail solely from `node --check`.

### Wave B — Latch + thrash (P0, ~5–7 days)

| Step | Work | Owner | FP |
|------|------|-------|-----|
| B1 | `EvidenceState` session store (in `SessionState` or Turn trackers) | core | FP4 |
| B2 | On successful serve + bind_ready + optional curl 200 → `PreviewLatch` | tools+core | FP4 |
| B3 | Forbid second preview serve / other ports while latch clean | dispatch | FP4, FP10 |
| B4 | Fingerprint thrash: N=3 identical content_fail or blocked_navigate → `thrash_blocked` | tools+core | FP5 |
| B5 | Single recovery inject; supersede prior advisory same class | advisory | FP6 |
| B6 | Remove alternate-port free suggestions from recovery_catalog for latched sessions | tools | FP10 |
| B7 | Dirty invalidation on mutation under latch dir | core | FP4 |

**Done when:** Simulated session with 3 Error-response navigates → 4th blocked; no port shopping.

### Wave C — Budgets, class, goals (P1, ~3–5 days)

| Step | Work | Owner | FP |
|------|------|-------|-----|
| C1 | `Phase::Create` vs `Phase::Verify` tool counters + config caps | core | FP7 |
| C2 | Exhaust verify budget → `RunOutcome` NeedsEvidence / Escalated (not generic budget_exhausted alone) | assessor | FP1, FP7 |
| C3 | `TaskClass::MediaRender` (Hyperframe/video/render intent) | task_class | FP3 |
| C4 | Media evidence: output file exists + size > 0 (optional ffprobe later) | assessor | FP2 |
| C5 | Auto-seed session goals for visual_ux / media_render | core goals | FP8 |
| C6 | Progress speech nudge every K tools only if empty assistant streak (optional, off by default) | core | FP6 |

**Done when:** Hyperframe-class fixture can complete on file artifact without 20× browser_vision.

### Wave D — E2E harness suite + observability (P0–P1, parallel)

| Step | Work | Owner |
|------|------|-------|
| D1 | Fixture pack from 017 sessions (sanitized message JSONL) | tests |
| D2 | `harness_nonflaky_e2e` integration tests | core |
| D3 | Metrics: create_tools, verify_tools, latches, thrash_blocks, oracle_class | analyzer |
| D4 | Clippy + workspace unit for touched crates | CI |

---

## 5. E2E / unit test plan (no flaky tests)

### Rules for tests

| Rule | Detail |
|------|--------|
| No network | TempDir + mocked tool results / fake snapshot JSON |
| No sleep | Deterministic state transitions only |
| No LLM | Pure assessor + latch + oracle |
| Fixture-stable | Golden JSON committed; no wall-clock |

### Test matrix

| ID | Layer | Asserts |
|----|-------|---------|
| **NF-U1** | unit | `ContentClass` from title “Error response” → HttpErrorPage |
| **NF-U2** | unit | chrome-error URL → ChromeError; not visual evidence |
| **NF-U3** | unit | browser ESM + importmap peer → no `node --check` fail |
| **NF-U4** | unit | Node CJS `.cjs` still runs `node --check` |
| **NF-U5** | unit | PreviewLatch forbids second port serve while clean |
| **NF-U6** | unit | Mutation under dir dirties perceive latch |
| **NF-U7** | unit | 3× same fingerprint → thrash_blocked; 4th blocked |
| **NF-U8** | unit | Advisory supersede: second inject replaces, count=1 active |
| **NF-U9** | unit | MediaRender completes with mp4 size>0 without browser |
| **NF-U10** | unit | Verify budget exhaust → Escalated/NeedsEvidence |
| **NF-E1** | e2e | Replay pinguin-shaped tool sequence → no ESM oracle fail; thrash after N content_fails |
| **NF-E2** | e2e | Happy path: write → serve → snapshot Ok → assessor Complete eligible |
| **NF-E3** | e2e | Hyperframe-shaped: writes + render file → media latch Complete eligible |
| **NF-E4** | e2e | Document latch regression still green (no visual_ux regression) |
| **NF-E5** | e2e | Guardrails hard-stop still ON; visual_storm still blocks non-serve terminal |

```bash
# Implementation target commands
cargo test -p edgecrab-tools --lib structured_browser
cargo test -p edgecrab-tools --lib harness_gates
cargo test -p edgecrab-core --lib evidence_latch
cargo test -p edgecrab-core --lib completion_assessor
cargo test -p edgecrab-core --test harness_nonflaky_e2e
cargo clippy -p edgecrab-core -p edgecrab-tools -- -D warnings
```

### Fixture sources (017)

| Fixture | Session | Use |
|---------|---------|-----|
| `fixtures/pinguin_verify_thrash.jsonl` | `e256f862…` | NF-E1 |
| `fixtures/hyperframe_budget.jsonl` (trimmed) | `cb635449…` | NF-E3 class path |
| Synthetic happy path | hand-written | NF-E2 |

Sanitize: strip secrets; keep tool names, structured result JSON, paths under TempDir remap.

---

## 6. Acceptance criteria (ship checklist)

### Wave A

- [x] Navigate/snapshot with Error response **never** counts as visual evidence  
- [x] Browser ESM demo does **not** fail harness-gates on `node --check` alone  
- [x] Node library `.cjs` still oracle-checked  
- [x] NF-U1…U4 green  

### Wave B

- [x] PreviewLatch recorded after bind-ready serve of demo dir  
- [x] Second http.server / other port blocked while latch clean  
- [x] N identical content_fail fingerprints → thrash_blocked  
- [x] Advisory class tracked on EvidenceState (`may_inject_advisory`)  
- [x] NF-U5…U8, NF-E1 green  

### Wave C

- [x] Create vs verify counters on EvidenceState  
- [x] MediaRender can complete without browser loop  
- [ ] Auto-goals seeded for visual_ux (helpers exported; SQLite seed optional follow-up)  
- [x] NF-U9…U10, NF-E3 green  

### Wave D

- [x] `harness_nonflaky_e2e`  
- [x] Document latch classification regression  
- [ ] Clippy clean on full workspace (pre-existing conversation warnings)  

---

## 7. Config surface (minimal)

| Key | Default | Notes |
|-----|---------|-------|
| `harness.evidence_latches` | `true` | Master switch |
| `harness.verify_tool_budget` | `12` | After ArtifactLatch |
| `harness.thrash_fingerprint_limit` | `3` | Exact match only |
| `harness.post_mutation_oracles` | existing | Class-aware behavior inside |
| `EDGECRAB_HARNESS_ORACLES` | existing | Unchanged env override |

No new soft “confidence” knobs.

---

## 8. Risk & rollback

| Risk | Mitigation |
|------|------------|
| Over-strict content class blocks real 404 sites | Only apply HttpErrorPage markers for **loopback preview** URLs |
| ESM skip too broad | Require importmap peer **or** explicit `type=module` script src + not package main |
| Latch sticks after user kills server | Perceive fail + transport fail clears perceive; heal recipe once then escalate |
| Verify budget too low | Config `verify_tool_budget`; tests pin default |

**Rollback:** `harness.evidence_latches: false` restores pre-wave behavior (oracles still class-aware if A shipped).

---

## 9. Out of scope (v1)

| Item | Why |
|------|-----|
| Full generator/evaluator multi-agent | Cost; BP-08 later |
| Progress.md / git initializer agent | Overlap goals; optional v2 |
| Operator `/verify` slash command | Thin CLI later |
| ffprobe deep media QA | size>0 first |
| Rewriting entire conversation loop | Thin hooks only |

---

## 10. Implementation order (sprint)

```text
Day 1–2   Wave A (content class + oracles) + NF-U1…U4
Day 3–5   Wave B (latch + thrash) + NF-U5…U8 + NF-E1
Day 6–7   Wave C (budgets + media_render + goals) + NF-E2…E3
Day 8     Wave D polish, NF-E4…E5, clippy, docs assessment update
```

---

## 11. Definition of done (program)

1. Replaying pinguin thrash **cannot** spend unbounded verify tools (breaker fires).  
2. Browser ESM **cannot** false-fail on `node --check`.  
3. Happy-path demo **can** reach Complete with latched evidence in e2e without LLM.  
4. No new heuristic denylist items in merged code.  
5. SOLID ownership map respected (no assessor→CDP, no duplicate thrash counters).  
6. 017 AC-V1…V4 and 018 BP-01…06 implemented; V5–V8 / BP-07+ as Wave C.  

---

## 12. Cross-refs

| Doc | Role |
|------|------|
| [017](017-session-forensics-2026-07-19.md) | Production evidence + AC-V* |
| [018](018-non-flaky-harness-best-practices-2026-07.md) | SOTA BP catalog |
| [014](014-improvement-plan.md) | Prior harness/MCP waves (done) |
| [001](001-first-principles.md) | AE1–AE10 |
| `harness_gates.rs` | Oracles |
| `completion_assessor.rs` | Done policy |
| `structured_browser.rs` | Content parse |
| `recovery_catalog.rs` | Recipes |
| `harness_loop_policy.rs` | Visual storm / hard-stop |

---

## 13. One-line summary

**Implement a typed evidence latch graph with class-aware deterministic oracles and exact-fingerprint thrash bounds — DRY/SOLID owners, golden e2e from real sessions, zero flaky heuristics.**
