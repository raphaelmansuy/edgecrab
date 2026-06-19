# 002 — First Principles

What an agent harness **must** do, stripped of EdgeCrab/Hermes/Claude specifics.

**Cross-ref:** [agent_harness/000_overview.md](../agent_harness/000_overview.md) · [improve_plan/31-harness-deep-comparison.md](../improve_plan/31-harness-deep-comparison.md)

---

## 1. Four questions every harness must answer

```text
  ┌─────────────────────────────────────────────────────────────┐
  │ Q1  What is happening now?        → Progress / liveness      │
  │ Q2  Is forward progress real?     → Heartbeats · stall detect│
  │ Q3  What work remains?            → Plan state · subgoals    │
  │ Q4  Why did the run stop?         → ExitReason (explicit)    │
  │ Q5  Was the task actually done?   → Verification evidence  │
  └─────────────────────────────────────────────────────────────┘
         EdgeCrab today: strong Q1, partial Q2–Q4, weak Q5
```

These are **orthogonal**. Collapsing them produces the games003 experience: busy UI, no verified outcome.

---

## 2. The harness loop (physics)

```text
                    ┌──────────────┐
                    │   INTENT     │  user message + goals + platform hints
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │   ASSEMBLE   │  system blocks + messages + wire tools
                    └──────┬───────┘
                           │
              ┌────────────▼────────────┐
              │      LLM COMPLETION      │  streaming OR non-streaming
              └────────────┬────────────┘
                           │
              ┌────────────▼────────────┐
         no   │     TOOL CALLS?         │  yes
         ┌────┤                         ├────┐
         │    └─────────────────────────┘    │
         │                                     │
         │    ┌─────────────────────────┐      │
         │    │ VALIDATE + BUDGET GATE  │      │  mutation_turn_policy
         │    └───────────┬─────────────┘      │
         │                │                    │
         │    ┌───────────▼─────────────┐      │
         │    │ DISPATCH (parallel/seq) │      │  registry + path jail
         │    └───────────┬─────────────┘      │
         │                │                    │
         │    ┌───────────▼─────────────┐      │
         │    │ SHAPE RESULT            │      │  spill · prune · summarize
         │    └───────────┬─────────────┘      │
         │                │                    │
         │    ┌───────────▼─────────────┐      │
         │    │ VERIFY (optional)       │      │  ◄── GAP: perception tools
         │    └───────────┬─────────────┘      │
         │                │                    │
         └────────────────┼────────────────────┘
                          │ (next iteration)
                          ▼
                    ┌──────────────┐
                    │ ASSESS STOP  │  CompletionPolicy + harness gates
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  RunOutcome  │
                    └──────────────┘
```

**Invariant:** Each box must have **one module owner** (SOLID SRP). Today `conversation.rs` owns too many boxes.

---

## 3. Seven jobs of the harness (J1–J7)

From [improve_plan/31-harness-deep-comparison.md](../improve_plan/31-harness-deep-comparison.md):

| Job | Owner (target) | EdgeCrab today |
|-----|----------------|----------------|
| **J1** Schema contract | `edgecrab-tools` schemas + `schema_mode` | Strong strict JSON; indexed deferred |
| **J2** Dispatch | `ToolRegistry` | Strong |
| **J3** Validation | `validateInput` phase per tool + pipeline | Partial (budget gate post-schema) |
| **J4** Effect mediation | `edgecrab-security` + read tracker | Strong path jail |
| **J5** Result shaping | spill + compression + summaries | Strong but **hides truth** on spill |
| **J6** Failure recovery | recovery_catalog + escalation | Good escalation messages |
| **J7** Cost discipline | context budget + iteration cap | Good; OTEL noisy when misconfigured |

**Design rule (FP2):** Make the **correct** tool sequence easier than the incorrect one.

| Wrong path (games003) | Correct path (harness should bias) |
|-----------------------|-------------------------------------|
| `write_file` entire HTML | `read_file` offset/limit → `patch` CSS/HUD |
| `search_files` vague pattern | `terminal find` or manifest index |
| Blind rewrite after spill stub | Read artifact path OR auto-range read |
| `browser_navigate` localhost | Dev-profile preview OR `vision` on screenshot file |

---

## 4. Three primitives (measurable)

From [001-gap-analysis-v14/000-methodology.md](../001-gap-analysis-v14/000-methodology.md):

```text
  ┌────────────────────┬────────────────────────────────────────┐
  │ Reliability          │ tool-error rate · goal drift · done %  │
  │ Trust in side-effects│ rollback rate · bad-write detection    │
  │ Cost per useful turn │ USD/turn · p50 latency · recovery mins │
  └────────────────────┴────────────────────────────────────────┘

  A harness improvement must move ≥2 primitives measurably.
```

---

## 5. DRY zones (single source of truth)

| Concept | Must live in ONE place | Drift risk today |
|---------|------------------------|------------------|
| Tool result spill | `artifact_spill` + `tool_result_spill` | TUI summary vs model stub vs history prune |
| Mutation byte budget | `mutation_turn_policy` | conversation pre-dispatch + provider max_tokens |
| Tool wire partition | `tool_schema_index` | TUI "107 tools" vs harness `tool_count: 55` |
| Completion meaning | `CompletionPolicy` → `RunOutcome` | gateway `completed` vs `StreamEvent::Done` |
| Progress liveness | `tool_progress_tail` + `StreamEvent` | shelf vs status bar vs activity feed |
| Injection scan | `injection_scan` (target) | multiple pattern lists (gap 031) |

---

## 6. SOLID module boundaries (target)

```text
  edgecrab-core
  ├── loop/              execute_loop orchestration ONLY
  ├── progress/          ProgressSink trait · StreamEvent mapping
  ├── completion/        CompletionPolicy · RunOutcome (exists: completion_assessor)
  ├── perception/        TaskClass · verification requirements (NEW)
  ├── spill/             re-export tools spill; conversation calls trait
  └── provider_policy/   local_provider_policy (exists)

  edgecrab-tools           ToolHandler · validation · spill write
  edgecrab-cli             TUI consumes ProgressSink only
  edgecrab-gateway         same RunOutcome as CLI
```

**Dependency rule:** `edgecrab-core` must not import `edgecrab-cli` or `edgecrab-gateway` (already in methodology).

---

## 7. Task classes (verification requirements)

Not every task needs browser preview. The harness should classify intent and attach **minimum verification**:

| Task class | Example | Minimum verification |
|------------|---------|----------------------|
| `code_edit` | fix bug in `game.js` | LSP diagnostics OR test command exit 0 |
| `visual_ux` | beautify HTML game | preview URL OR screenshot + vision summary |
| `research` | summarize doc | cite artifact path or URL in final response |
| `ops` | run migration | command exit code + tail in result |
| `conversation` | explain code | no tool verification required |

**games003** = `visual_ux` but ran with `code_edit` verification (`node -c`) only.

See [007-architecture-target.md](./007-architecture-target.md) § TaskClassifier.
