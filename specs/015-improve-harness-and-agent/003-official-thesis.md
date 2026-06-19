# 003 — Official Thesis & External Grounding

EdgeCrab improvements must align with **provider contracts**, **security baselines**, and published agent-system design — not vibe-driven harness tweaks.

---

## 1. Provider & API thesis

### OpenAI / compatible tool calling

- Tool arguments are a **single JSON object** per call; large payloads compete with reasoning tokens in the same completion budget.
- **Implication:** Whole-file writes are physically wrong for 8k output caps — EdgeCrab's `mutation_turn_policy` is aligned with API physics ([`mutation_turn_policy.rs`](../../crates/edgecrab-tools/src/mutation_turn_policy.rs)).
- **Source:** OpenAI function calling docs — parameters are part of the completion; max_tokens bounds total generation.

### Anthropic tool use & prompt caching

- System prompt stability enables prefix caching; **dynamic content must not pollute stable blocks**.
- EdgeCrab already splits stable/dynamic in `prompt_builder.rs` + `prompt_cache_policy.rs`.
- **Implication:** Harness injections (goals, steers, budget warnings) must stay in **messages**, never cached system — already documented in AGENTS.md.

### Streaming vs non-streaming

- Streaming enables **early cancellation** and **partial tool-arg assembly**; non-streaming is correct fallback when stream tool assembly fails.
- EdgeCrab downgrades per-session on stall (`conversation.rs` non-streaming recovery).
- **Implication:** Operator UX must label non-streaming waits as **provider behavior**, not harness deadlock — [`tool_progress_tail.rs`](../../crates/edgecrab-tools/src/tool_progress_tail.rs).

---

## 2. Agent safety & trust thesis

### OWASP LLM Top 10 (agent-relevant)

| Risk | Harness response |
|------|------------------|
| Prompt injection via tool results | `injection_scan` on context files + memory; extend to spill artifacts |
| Excessive agency | path jail, SSRF, command scan — **keep** |
| Supply chain (skills) | `skills_guard` — **keep** |
| Improper output handling | redaction pipeline in core |

**Do not weaken SSRF** to fix games003; add **dev-profile preview** instead.

### Claude Code pattern (validation before effect)

`claude-code-analysis` separates `validateInput` from `call()` and enforces **read-before-write** via `readFileState` with mtime staleness.

EdgeCrab parity: `read_tracker` + write rejection with embedded content — **stronger than Hermes** ([improve_plan/31](../improve_plan/31-harness-deep-comparison.md)).

**Thesis:** Validation is **kernel**; description is **hint**. Harness improvements must add validators/gates, not more prompt prose.

---

## 3. Observability thesis (OpenTelemetry)

- Traces/metrics should be **opt-in** and **fail silent** when collector unavailable.
- Homelab evidence: `BatchSpanProcessor.Flush.ExportError` spam every 5s when collector down — **violates** operator trust in logs.
- **Target:** `observability.rs` should degrade: one WARN at startup, suppress repeated export errors, or `otel_export: false` default when endpoint unreachable.

**Code:** [`observability.rs`](../../crates/edgecrab-core/src/observability.rs) · [`otel_export.rs`](../../crates/edgecrab-core/src/otel_export.rs)

---

## 4. Minimum context / tool surface thesis

Spec 007 established:

- Default `core` toolset + **indexed** wire schemas + `tool_search` materialization.
- CI gates on wire byte budget.

**Thesis:** Cognitive load on the model is **schema bytes + guidance + messages**. Harness must show operators **wire vs deferred** explicitly — not `107 tools` headline without `55 on wire` footnote.

---

## 5. Long-horizon execution thesis (Ralph / goals)

Persistent goals inject **user-role** blocks each turn without mutating cached system prompt.

**Thesis:** Standing intent belongs outside message compression path; completion assessor should consult `session_goals` when deciding `ExitReason::GoalIncomplete`.

---

## 6. Internal ADR alignment

| ADR / spec | Decision we extend |
|------------|---------------------|
| [agent_harness/001](../agent_harness/001_adr_unified_agent_harness.md) | Unified RunOutcome · CompletionPolicy |
| [014-improve-local-harness](../014-improve-local-harness/README.md) | Output budget geometry is law |
| [007-minimum-context](../007-minimum-context/007-implementation-assessment.md) | Indexed tools default |
| [002-terminal-ux-ui/004](../002-terminal-ux-ui/004-stream-event-contract.md) | StreamEvent producer/consumer matrix |

---

## 7. Design thesis summary

```text
  OFFICIAL STACK (what we obey)
  ═══════════════════════════
  API physics     → mutation budgets · spill · indexed schemas
  Security        → SSRF · path jail · injection scan (no bypass)
  Cache economics → stable/dynamic prompt split
  Observability   → fail-soft · single harness target
  Code is law     → schema + validator > prompt wishful thinking

  PRODUCT STACK (what we add)
  ═══════════════════════════
  Perception loop → task-class verification
  Honest UI       → spill paths · wire counts · exit reasons
  Completion      → RunOutcome everywhere
```
