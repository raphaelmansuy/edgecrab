# Bedrock Nova Premature Stop — Specification Index

> **Status:** Analysis + Implementation + Architectural Evolution
> **Scope:** edgecrab-core, edgequake-llm

## Documents in this Series

| # | File | Purpose | Status |
|---|------|---------|--------|
| 01 | [01-problem-analysis.md](01-problem-analysis.md) | Evidence chain from user symptom, runtime loop, and AWS docs | Complete |
| 02 | [02-first-principles.md](02-first-principles.md) | First-principle decomposition of why Nova stops early | Complete |
| 03 | [03-adr-completion-gate.md](03-adr-completion-gate.md) | ADR: heuristic fix (deferred-work phrase detection) | Accepted + Implemented |
| 04 | [04-implementation-plan.md](04-implementation-plan.md) | Concrete heuristic implementation and regression coverage | Complete |
| 05 | [05-verification-plan.md](05-verification-plan.md) | Heuristic validation steps and pass criteria | Complete |
| 06 | [06-shadow-judge-critique-and-concept.md](06-shadow-judge-critique-and-concept.md) | Why the heuristic is structurally wrong + first-principles shadow judge design | Complete |
| 07 | [07-adr-shadow-judge.md](07-adr-shadow-judge.md) | ADR: shadow judge — semantic LLM completion oracle | Proposed |
| 08 | [08-shadow-judge-implementation.md](08-shadow-judge-implementation.md) | Shadow judge Rust implementation plan (all files, API, tests) | Ready to implement |

## Evolution of This Spec Set

### Phase 1 — Nova Heuristic (docs 01–05, implemented)

Amazon Bedrock Nova is allowed to return `stopReason = end_turn` after a tool
result even when the higher-level user task is still unfinished. EdgeCrab must
therefore distinguish **"model stopped generating"** from **"task is actually
complete"**.

The initial fix (ADR 003, doc 03) added a phrase-matching heuristic in
`completion_assessor.rs` that detects deferred-work language after tool activity.
This was implemented and all 12 existing tests pass.

### Phase 2 — Shadow Judge Architecture (docs 06–08, proposed)

The heuristic is a syntactic fix for a semantic problem. Doc 06 demonstrates that
phrase-matching over LLM output cannot reliably classify task completion because:

- Natural language has infinite surface forms (the vocabulary is always incomplete).
- The 240-character window is arbitrary and model-update-fragile.
- The heuristic has no concept of the original user intent.
- False positives block completed sessions unnecessarily on strong models.

The **shadow judge** (ADR 007, doc 07) is the principled successor: a single
lightweight LLM classification call that reads the full conversation history and
returns a structured JSON verdict (`complete` / `incomplete` + confidence + reason
+ steering hint).

Key properties:
- **Opt-in** (default: disabled). Zero impact on strong-model sessions.
- **Session-isolated**: never mutates `session.messages`; prompt cache preserved.
- **Near-zero cost**: ~$0.004/invocation due to Anthropic prompt cache reuse.
- **General**: applicable to all weak models, not just Nova.
- **Steering**: judge provides specific next-action hints, not generic "continue" nudges.

## One-Line Summary

Amazon Bedrock Nova uses `end_turn` as a turn signal, not a task signal; the
short-term fix is phrase-detection at the completion gate; the principled fix is
a lightweight LLM oracle that classifies task completion semantically over the
full conversation trajectory.