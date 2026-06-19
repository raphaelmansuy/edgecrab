# 006 — Comparator: Hermes · Claude Code · PI-Style Agents

Cross-reference patterns from sibling projects. **Code is law** in each checkout.

| Project | Path |
|---------|------|
| Hermes Agent | `/Users/raphaelmansuy/Github/03-working/hermes-agent` |
| Claude Code (analysis) | `/Users/raphaelmansuy/Github/03-working/claude-code-analysis` |
| PI-style (thesis) | Embodied / verification-first loops — see §4 |

Also see [improve_plan/02-hermes-patterns.md](../improve_plan/02-hermes-patterns.md) · [improve_plan/31-harness-deep-comparison.md](../improve_plan/31-harness-deep-comparison.md).

---

## 1. Hermes Agent — callbacks + flat loop

### Shape

```text
  run_agent.py (monolith)
  ├── OpenAI client
  ├── tool dispatch inline
  ├── callback hooks (on_tool_start, on_token, …)
  └── CLI spinner / print side effects
```

**Strength:** Fast iteration; rich operator callbacks; `hermes_bootstrap` for Windows UTF-8.

**Weakness:** Progress + completion + dispatch **coupled** — harder to guarantee same semantics across gateway/TUI/CLI.

### Patterns to adopt

| Pattern | Hermes | EdgeCrab action |
|---------|--------|-----------------|
| Flat core tool list (~36) | `_HERMES_CORE_TOOLS` | Already: `core` default 56 + indexed |
| Session cwd on CLI | `_launch_cwd_for_session` | Partial — document in session resume |
| Recovery error shapes | tool error JSON | Already: `recovery_catalog` — extend budget hints |
| Gateway stream consumer | `gateway/stream_consumer.py` | Parity: `stream_consumer.rs` |

### Patterns NOT to adopt

| Pattern | Why |
|---------|-----|
| Advisory schemas (no strict) | EdgeCrab wins on Code Is Law |
| Monolithic loop | Rust async + typed events already better |

### Hermes spill / context

Hermes prunes tool output in compression paths; EdgeCrab's explicit **artifact spill** is more operator-transparent — **keep EdgeCrab spill**, fix stub actionability.

---

## 2. Claude Code — validation kernel + readFileState

### Shape

```text
  query.ts (generator loop)
  ├── Task lifecycle + terminal reasons
  ├── readFileState (path → mtime, partial flag)
  ├── validateInput() BEFORE call()
  └── stop hooks / TaskCompleted hooks
```

### readFileState contract

```text
  READ ──► readFileState[path] = { timestamp, isPartialView }
              │
  WRITE validateInput ──► reject if never read OR stale mtime
              │
  WRITE call() ──► atomic re-check (TOCTOU)
```

EdgeCrab equivalent: `read_tracker` + write rejection with file content in error — **already strong**.

### Terminal reasons (adopt fully)

Claude returns explicit stop enums; EdgeCrab has `ExitReason` — **must surface in TUI status bar and final transcript line**.

| Claude Code | EdgeCrab `ExitReason` |
|-------------|----------------------|
| completed | `Completed` |
| aborted | `Interrupted` |
| prompt too long | `ContextExceeded` |
| blocking limit | `BudgetExhausted` |
| stop hook | `HarnessBlocked` |

### Snip / re-read seeding

Claude MCP can **seed** `readFileState` when context snips a read — EdgeCrab spill is analogous but lacks **automatic re-read recipe** in stub.

**Adopt:** spill stub = `read_file(path=artifact, offset=1, limit=N)` template.

---

## 3. PI-style agents (Physical Intelligence / embodied thesis)

No single repo in workspace; pattern from robotics + visuomotor agent literature (2024–2026):

```text
  SENSE ──► PLAN ──► ACT ──► SENSE ──► …
     │                              │
     └──────── closed loop ─────────┘

  Failure mode: open-loop ACT without SENSE → confident wrong behavior
```

**Mapping to games003:**

| PI principle | games003 failure |
|--------------|------------------|
| Observation mandatory | spill stubs without re-read |
| Sim / preview before deploy | no localhost preview |
| Verify goal state | `node -c` ≠ visual UX |
| Episode boundary clear | interrupted without RunOutcome message |

**EdgeCrab harness addition:** `TaskClass::VisualUx` requires **PerceptionEvidence** before `CompletionPolicy` returns `Completed`.

Not a new model — a **gate** in `completion_assessor.rs`.

---

## 4. Comparative scorecard (harness only)

```text
  Dimension              Hermes   Claude   EdgeCrab   Target
  ─────────────────────  ──────   ──────   ────────   ──────
  Progress transport     C        A        A          A
  Schema strictness      C        A        A          A
  read-before-write      B        A        A          A
  Result shaping         B        B        B+         A
  Perception loop        C        B        D          B+
  Completion contract    C        A        B          A
  Local provider policy  B        N/A      A          A
  Operator honesty       B        B        C          A
```

---

## 5. Synthesis — what EdgeCrab should steal

```text
  FROM HERMES     → session cwd · recovery prose · stream consumer discipline
  FROM CLAUDE     → terminal reasons in UI · validateInput parity · read state
  FROM PI-STYLE   → mandatory verify step per task class · no open-loop edits

  EDGECRAB KEEPS  → typed StreamEvent · strict schemas · indexed tools · Rust safety
```

Implementation mapping: [008-improvement-plan.md](./008-improvement-plan.md).
