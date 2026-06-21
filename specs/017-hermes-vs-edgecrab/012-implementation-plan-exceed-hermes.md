# 012 — Implementation Plan: Exceed Hermes (First Principles)

**Date:** 2026-06-21 (rev 2)  
**Cross-ref:** [001 rubric](./001-first-principles-rubric.md) · [011 borrow matrix](./011-borrow-reject-matrix.md) · [015/009 HA gates](../015-improve-harness-and-agent/009-acceptance-criteria.md) · [016/007 backlog](../016-harness-assessment/007-priority-backlog.md)

**Goal:** EdgeCrab harness **wins Q1–Q5** and **leads or ties J1–J7** — without duplicating Hermes file layout or weakening security.

**Rule:** One owner module per behavior. One assess path. One classifier brain. Tests are law alongside code.

---

## 1. North star (first principles)

### 1.1 Win condition

| Axis | Exceed Hermes when… |
|------|---------------------|
| **Q5 VERIFY** | `Completed` ⟹ evidence in `HarnessSnapshot` OR explicit `NeedsVerification` (TUI + gateway) |
| **Q2 Progress** | Task-class blocks are **loop physics**, not optional warnings |
| **Q4 Exit** | `RunOutcome` + explainer + budget summary — never ambiguous string |
| **J6 Recovery** | `failover.rs` + `recovery_catalog.rs` — one taxonomy, two layers |
| **J7 Maintainability** | `conversation.rs` orchestrates only; no duplicate gate logic |

```text
  INVARIANT (Q5 — non-negotiable)
  ┌─────────────────────────────────────────────────────────────────────┐
  │  assess_completion() uses build_harness_snapshot() — never Default  │
  │  HarnessTurnAdvisory flags feed HarnessSnapshot — not parallel paths  │
  │  Mid-loop Done and end-loop Done call the same assess_epilogue()    │
  └─────────────────────────────────────────────────────────────────────┘
```

Hermes cannot satisfy Q5 (no typed gates). EdgeCrab has types — **enforcement + tests** close the gap.

### 1.2 Scoreboard → 90-day target

| Dimension | Hermes | EdgeCrab today | Target |
|-----------|--------|----------------|--------|
| Q5 VERIFY | ██░░░ | ██░░░ | ████░ |
| Q2 guardrails | ██░░░ | ████░ | █████ |
| Q4 completion | ███░░ | ████░ | █████ |
| J6 recovery | ████░ | ███░░ | █████ |
| J7 modularity | ████░ | ██░░░ | ████░ |
| Q1 observability | ███░░ | ████░ | █████ |

---

## 2. SOLID + DRY module contract

**DRY:** Each behavior has exactly **one owner**. `conversation.rs` is an orchestrator — it calls owners, never re-implements them.

**SOLID mapping:**

| Principle | Harness application |
|-----------|---------------------|
| **S** Single responsibility | See ownership table — no gate logic in `conversation.rs` after Phase 1 |
| **O** Open/closed | Extend via `CompletionPolicy`, `FailoverReason` handlers, plugins — not loop forks |
| **L** Liskov | `DefaultCompletionPolicy` replaceable in tests with mock policy |
| **I** Interface segregation | `CompletionPolicy`, `ProgressSink`, `ClassifiedError` — small public surfaces |
| **D** Dependency inversion | Loop depends on `assess_completion()`, `classify_provider_error()`, `build_harness_snapshot()` — not inline heuristics |

### 2.1 Single ownership map (DRY)

```text
  ORCHESTRATION (thin)                DOMAIN OWNERS (fat, tested)
  ─────────────────────               ─────────────────────────────
  conversation.rs::execute_loop  ──►  turn_prologue.rs      (setup once)
                                 ──►  turn_dispatch.rs      (tool batch)
                                 ──►  turn_epilogue.rs      (assess once) ← NEW
                                 ──►  provider_call.rs      (API + retry entry)
                                 ──►  compression.rs        (context reshape)
                                 ──►  failover.rs           (error taxonomy) ← NEW

  completion_assessor.rs         ◄──  harness_gates.rs       (facts only)
                                 ◄──  harness_advisory.rs    (window + flags)
                                 ◄──  task_class.rs          (VisualUx detect)

  edgecrab-tools                   ◄──  artifact_spill.rs
                                 ◄──  tool_loop_guardrails.rs
                                 ◄──  recovery_catalog.rs
                                 ◄──  mutation_turn_policy.rs

  OBSERVABILITY (read-only)      ◄──  harness_analyzer.rs
                                 ◄──  doctor.rs (CLI)
```

### 2.2 Forbidden duplications (review blockers)

| Anti-pattern | Correct owner |
|--------------|---------------|
| Gate check in `conversation.rs` + `completion_assessor.rs` | `harness_gates.rs` only; assessor reads snapshot |
| Advisory warn in loop + block in dispatch | `harness_advisory.rs` records flag → snapshot |
| Retry logic in `provider_call.rs` + loop | `failover.rs` classifies; provider_call executes |
| Provisional vs final different assess paths | `turn_epilogue.rs::assess_and_finalize()` — one function |
| Hermes port pasted into `conversation.rs` | New module or extend named owner |

---

## 3. Work item registry (single source of truth)

Each row: **one ID, one owner, one HA gate, one test anchor**. Details in linked specs — not duplicated here.

| ID | Phase | Q/J | Owner module | Change (one line) | HA | Test anchor |
|----|-------|-----|--------------|-------------------|-----|-------------|
| **P0-1** | 0 | Q5 | `harness_gates.rs`, `turn_epilogue.rs` | `build_harness_snapshot` before every assess; ban `Default` | **HA-45** | `completion_assessor` + replay E16 |
| **P0-2** | 0 | Q2/Q5 | `harness_advisory.rs`, `harness_gates.rs` | Advisory flags → `HarnessSnapshot.blocks_completion()` | **HA-48** | `harness_advisory` + assessor |
| **P0-3** | 0 | Q2/Q4 | `turn_dispatch.rs`, `turn_epilogue.rs` | `Halt` → `ExitReason::GuardrailHalt` + single exit path | **HA-46** | `tool_loop_guardrails` + integration |
| **P0-4** | 0 | Q5 | `artifact_spill.rs`, `turn_dispatch.rs`, `completion_assessor.rs` | Theater write cap + spill-unread block | **HA-43**, **HA-49**, **HA-23** | replay + spill tests |
| **P0-5** | 0 | Q1 | `harness_analyzer.rs`, `doctor.rs` | Parse JSONL `fields.message` / `fields.tool_name` | **HA-44** | `harness_analyzer` fixture |
| **P0-6** | 0 | Q5/J4 | `config.rs`, `runtime.rs`, `profile.rs`, `doctor.rs` | Merge global preview; migrate on load; doctor assert | **HA-41**, **HA-05** | `games003_profile_inherits_*` |
| **P1-1** | 1 | J7 | `turn_prologue.rs` (new) | Extract setup; MCP refresh; defer preflight compress | **HA-52** | `turn_prologue` unit tests |
| **P1-2** | 1 | J7/Q4 | `turn_epilogue.rs` (new) | Single assess+persist+explainer; shrink `conversation.rs` | **HA-51**, **HA-52** | epilogue unit + LOC gate |
| **P1-3** | 1 | Q4 | `turn_epilogue.rs` | `handle_budget_exhausted` toolless summary API call | **HA-11** | epilogue + mock provider |
| **P1-4** | 1 | J6 | `failover.rs` (new) | `classify_provider_error` → `ClassifiedError` | **HA-50** | ≥40 scenario port tests |
| **P1-5** | 1 | Q4 | `completion_assessor.rs` | `count_unanswered_tool_calls > 0` → `Incomplete` | **HA-51** | assessor unit test |
| **P2-1** | 2 | J7 | `edgecrab-state` | Compression SQLite lock | **HA-53** | state concurrency test |
| **P2-2** | 2 | Q4 | `edgecrab-state` | `parent_session_id` on compress rotate | **HA-53** | session DB test |
| **P2-3** | 2 | J7 | `compression.rs` | Defer preflight when real usage fits | **HA-54** | compressor unit test |
| **P2-4** | 2 | J1 | `turn_prologue.rs` | MCP between-turn refresh | **HA-55** | prologue mock MCP test |
| **P2-5** | 2 | Q5 | `dev_server.rs` | Port discovery in SSRF recovery | **HA-20c** | dev_server unit test |
| **P2-6** | 2 | Q5 | `tests/harness_*` | Expand replay fixtures (E16, `0aeef965`) | **HA-27**, **HA-49** | CI replay pack |
| **P3-1** | 3 | Q3 | `background_review.rs` (new) | Post-turn forked review task | **HA-56** | optional integration |
| **P3-2** | 3 | J6 | `provider_call.rs` | Image shrink on provider error | **HA-57** | provider error fixture |
| **P3-3** | 3 | J7 | `agent.rs` `IterationBudget` | Refund after execute_code-only batch | **HA-58** | budget unit test |
| **P3-4** | 3 | Q5 | `completion_assessor.rs` | Shadow judge only when strict + no evidence | **HA-30** | assessor mock judge |

**Hermes borrow detail:** [011 borrow matrix](./011-borrow-reject-matrix.md) — do not duplicate in PRs; link HA ID instead.

---

## 4. Phases (execution only)

### Phase 0 — VERIFY loop physics (weeks 1–3)

**Unlocks:** Q5 differentiation. **Gate:** `cargo test -p edgecrab-core --test harness_games003_replay` + new HA-44..49 green.

Items: **P0-5 → P0-6 → P0-1 → P0-2 → P0-3 → P0-4** (doctor + preview before snapshot; see §6 dependency graph).

### Phase 1 — Structure + error brain (weeks 4–6)

**Unlocks:** J7 velocity, J6 parity+. **Gate:** `conversation.rs` ≤ 6,000 LOC; `failover.rs` ≥ 40 tests.

Items: **P1-1 + P1-2** (parallel), then **P1-4**, then **P1-3 + P1-5**.

### Phase 2 — Reliability (weeks 7–10)

**Unlocks:** Hermes battle scars without security regression. **Gate:** replay CI expanded; compression lock test.

Items: **P2-6** early; **P2-1..P2-5** as capacity allows.

### Phase 3 — Optional (weeks 11–14)

**Only if Phase 0 gate green.** Items P3-*.

---

## 5. Test strategy (code is law)

### 5.1 Pyramid

```text
                    ┌─────────────────────┐
                    │  Replay CI (HA-27)  │  games003 + E16 fixtures, no live LLM
                    └──────────┬──────────┘
                               │
              ┌────────────────┴────────────────┐
              │  Integration (mock provider)     │  loop + epilogue + halt path
              └────────────────┬────────────────┘
                               │
     ┌─────────────────────────┴─────────────────────────┐
     │  Unit — one module, one owner (bulk of harness law) │
     └─────────────────────────────────────────────────────┘
```

**Rule:** No Phase 0 item merges without **unit test in owner crate** + **HA gate row** updated in [015/009](../015-improve-harness-and-agent/009-acceptance-criteria.md).

### 5.2 Existing test assets (reuse — DRY)

| Asset | Path | Covers |
|-------|------|--------|
| games003 replay pack | `crates/edgecrab-core/tests/harness_games003_replay.rs` | HA-01,04,05,19,27,41,42 |
| Completion assessor | `crates/edgecrab-core/src/completion_assessor.rs` `mod tests` | HA-30,43 (partial) |
| Harness advisory | `crates/edgecrab-core/src/harness_advisory.rs` `mod tests` | HA-20e (partial) |
| Harness gates | `crates/edgecrab-tools/src/harness_gates.rs` `mod tests` | mutation debt, oracles |
| Harness analyzer | `crates/edgecrab-core/src/harness_analyzer.rs` `mod tests` | HA-25 (partial) |
| Guardrail policy | `crates/edgecrab-core/src/harness_loop_policy.rs` `mod tests` | hard_stop default |
| Tool guardrails | `crates/edgecrab-tools/src/tool_loop_guardrails.rs` `mod tests` | block/halt thresholds |
| Artifact spill | `crates/edgecrab-tools/src/artifact_spill.rs` `mod tests` | HA-23,24 |
| Recovery catalog | `crates/edgecrab-tools/src/recovery_catalog.rs` `mod tests` | HA-04,16,42 |

### 5.3 New HA gates (add to 015/009 when implementing)

| ID | Criterion | Test file / name |
|----|-----------|------------------|
| **HA-44** | Doctor `tool_starts` equals JSONL harness events | `harness_analyzer::parses_jsonl_fields_message` |
| **HA-45** | `LoopAction::Done` never calls assess with empty snapshot | `turn_epilogue::assess_uses_built_snapshot` |
| **HA-46** | Guardrail halt → `ExitReason::GuardrailHalt`, loop stops | `turn_dispatch::halt_sets_typed_outcome` |
| **HA-47** | `guardrails_hard_stop` default true | `harness_loop_policy::guardrails_hard_stop_default_on` ✓ exists |
| **HA-48** | VisualUx + act storm → `blocks_completion()` | `harness_gates::visual_storm_blocks_completion` |
| **HA-49** | E16 fixture: 0 perception → not `Completed` | `harness_games003_replay::e16_no_false_completed` |
| **HA-50** | Failover classifier ≥40 Hermes scenarios | `failover::classifier_matrix` |
| **HA-51** | Unanswered tool_calls → `Incomplete` | `completion_assessor::unanswered_tools_incomplete` |
| **HA-52** | `conversation.rs` LOC ≤ 6000 after extract | CI script / `wc -l` gate |
| **HA-53** | Compression lock + `parent_session_id` | `edgecrab-state` tests |
| **HA-54** | Defer preflight when usage fits | `compression::defer_preflight_when_fits` |
| **HA-55** | MCP refresh in prologue when generation changes | `turn_prologue::refreshes_mcp_tools` |

### 5.4 Per-item test checklist

| ID | Required tests before merge |
|----|----------------------------|
| **P0-1** | Unit: snapshot non-empty when mutations present. Integration: mock Done → assess called with snapshot. **HA-45** |
| **P0-2** | Unit: advisory records storm → snapshot flag. Assessor: VisualUx storm → `Incomplete`. **HA-48** |
| **P0-3** | Unit: `take_halt_decision` → epilogue sets `GuardrailHalt`. **HA-46** |
| **P0-4** | Unit: 2nd `*VERIFY*` write blocked. Spill without read blocks patch. Replay: theater cap. **HA-43,49,23** |
| **P0-5** | Fixture: `specs/016-harness-assessment/fixtures/harness.jsonl` (or sample) → non-zero `tool_starts`. **HA-44** |
| **P0-6** | Extend `games003_profile_inherits_global_preview`. Doctor integration. **HA-41,05** |
| **P1-1** | `turn_prologue` tests: budget reset, tracker init, MCP no-op when empty. **HA-55** stub |
| **P1-2** | Epilogue: mid-loop Done and max-iter exit share `assess_and_finalize`. **HA-51,52** |
| **P1-3** | Mock provider: budget exhausted → summary text + `Incomplete`. **HA-11** |
| **P1-4** | Port matrix from `hermes-agent/tests/agent/test_error_classifier.py`. **HA-50** |
| **P1-5** | Messages with orphan tool_calls → `Incomplete`. **HA-51** |
| **P2-6** | Add `e16_no_false_completed`, `0aeef965_terminal_storm` replay cases. **HA-27,49** |

### 5.5 CI commands (must pass on every harness PR)

```bash
cargo test -p edgecrab-core --lib completion_assessor harness_advisory harness_analyzer harness_loop_policy
cargo test -p edgecrab-tools --lib harness_gates artifact_spill tool_loop_guardrails recovery_catalog
cargo test -p edgecrab-core --test harness_games003_replay
cargo test -p edgecrab-core --test harness_games003_replay --test harness_failover_matrix   # after P1-4
cargo clippy --workspace -- -D warnings
```

**Recommended CI addition** (`.github/workflows/ci.yml`):

```yaml
- name: Harness regression pack
  run: cargo test -p edgecrab-core --test harness_games003_replay
- name: Conversation LOC gate (post Phase 1)
  run: test $(wc -l < crates/edgecrab-core/src/conversation.rs) -le 6000
```

### 5.6 Replay fixtures to add (P2-6)

| Fixture | Source session | Assert |
|---------|----------------|--------|
| `e16_no_false_completed` | E16 homelab visual task | `NeedsVerification` or `Incomplete`; never `Completed` without browser |
| `0aeef965_terminal_storm` | Assessment session | ≥5 terminal, 0 perception → storm flag set |
| `games003_verify_theater` | games003 | ≤1 verify markdown path; assessor blocks |
| `spill_blindness` | Synthetic | spill → write without read → block |

Store message JSON under `crates/edgecrab-core/tests/fixtures/harness/` — replay drives mock provider, not live API.

---

## 6. Dependency graph

```text
  P0-5 Doctor ──┐
  P0-6 Preview ─┴──► P0-1 Snapshot ──► P0-2 Advisory gates
                           │                    │
                           ▼                    ▼
                      P0-4 Theater/spill    P0-3 Guardrail halt
                           │
                           ▼
                  ═══ PHASE 0 GATE ═══
                  HA-44,45,46,48,49 green
                  false Completed = 0
                           │
         ┌─────────────────┼─────────────────┐
         ▼                 ▼                 ▼
    P1-1 Prologue    P1-4 Failover     P2-6 Replay expand
    P1-2 Epilogue         │                 │
         └────────┬────────┘                 │
                  ▼                          │
           PHASE 1 GATE                      │
           HA-50,51,52                       │
                  └──────────► Phase 2 ──────┘
```

---

## 7. Phase exit gates (metrics + tests)

| Gate | When | Metrics | Test command |
|------|------|---------|--------------|
| **Phase 0** | Week 3 | False `Completed` visual = 0; doctor accuracy 100%; terminal share <30% | `harness_games003_replay` + HA-44..49 |
| **Phase 1** | Week 6 | `conversation.rs` ≤6000 LOC; failover ≥40 tests | `harness_failover_matrix` + epilogue tests |
| **Phase 2** | Week 10 | Replay fixtures ≥4; compression lock passes | `cargo test -p edgecrab-state` + replay |
| **Phase 3** | Week 14 | Optional features behind config flags | feature-gated integration |

---

## 8. Sprint order

```text
  Sprint 1   P0-5 + P0-6          (observability + preview — unblocks evidence)
  Sprint 2   P0-1 + P0-3          (snapshot + halt — core physics)
  Sprint 3   P0-2 + P0-4 + E16    (gates + theater — Phase 0 gate review)
  Sprint 4   P1-1 turn_prologue
  Sprint 5   P1-2 turn_epilogue + P1-3
  Sprint 6   P1-4 failover + P1-5  (Phase 1 gate review)
  Sprint 7+  P2-* / P3-*          (capacity)
```

---

## 9. Competition scorecard (post Phase 0+1)

| Criterion | Hermes | EdgeCrab target | Proof |
|-----------|--------|-----------------|-------|
| Typed completion | ✗ | ✓ | `RunOutcome` + HA-45 |
| Re-open premature done | ✗ | ✓ | `turn_epilogue` + HA-45 |
| VisualUx without browser | possible | blocked | HA-49 replay |
| Task-class storm block | ✗ | ✓ | HA-48 |
| Harness doctor | ✗ | ✓ | HA-44 |
| Unified error taxonomy | ✓ | ✓ | HA-50 |
| Loop testability | ✓ | ✓ | HA-52 |
| Structured tool recovery | partial | ✓ | HA-04,42 (existing) |

**Done when:** ≥8/8 rows proven by CI — not manual dogfood alone.

---

## 10. Non-goals

| Reject | Why |
|--------|-----|
| Global `allow_private_urls` | SSRF — [011](./011-borrow-reject-matrix.md) |
| `hard_stop_enabled: false` default | Product positioning — HA-47 locks true |
| Auto-inject full spill | HA-24 turn budget |
| Profile-isolated security | games003 E15 — HA-41 merge instead |
| Re-expand `conversation.rs` | HA-52 |
| New `done` tool | `CompletionPolicy` sufficient |
| Harness PR without tests | This plan §5 |

---

## 11. PR checklist

```text
  FIRST PRINCIPLES
  □ Which Q1–Q5 or J1–J7 does this PR improve? (cite in description)
  □ Does it strengthen loop physics (Q2/Q5) or only add advisory/UI?

  SOLID / DRY
  □ Single owner module — no duplicate logic in conversation.rs
  □ Epilogue/prologue paths not forked (one assess_and_finalize)
  □ Classifier logic only in failover.rs (not provider_call + loop)

  TESTS (mandatory)
  □ Unit test in owner crate mod tests
  □ HA-XX ID listed in PR + added to 015/009 if new
  □ cargo test harness_games003_replay green (if Phase 0 touch)
  □ No test writes to ~/.edgecrab (TempDir + EDGECRAB_HOME)

  SECURITY
  □ Rejected Hermes patterns that regress SSRF/path jail
  □ Operator can explain stop reason (Q4) via RunOutcome
```

---

## 12. Related documents

| Doc | Use for |
|-----|---------|
| [001 rubric](./001-first-principles-rubric.md) | Q1–Q5, J1–J7 definitions |
| [011 borrow matrix](./011-borrow-reject-matrix.md) | Hermes mechanism detail |
| [015/009 HA gates](../015-improve-harness-and-agent/009-acceptance-criteria.md) | CI gate registry (extend with HA-44+) |
| [016/007 backlog](../016-harness-assessment/007-priority-backlog.md) | Forensics evidence |
| [010 completion truth](./010-completion-truth-verify.md) | Q5 deep dive |
