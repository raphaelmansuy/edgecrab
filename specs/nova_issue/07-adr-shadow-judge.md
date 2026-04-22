# ADR 007: Shadow Judge — Semantic Completion Oracle

Cross-ref: [06-shadow-judge-critique-and-concept.md](06-shadow-judge-critique-and-concept.md),
[08-shadow-judge-implementation.md](08-shadow-judge-implementation.md),
[02-first-principles.md](02-first-principles.md), [03-adr-completion-gate.md](03-adr-completion-gate.md)

---

## Status

**Proposed**

---

## Context

[ADR 003](03-adr-completion-gate.md) introduced a phrase-matching heuristic in
`completion_assessor.rs` to prevent premature session exits on Bedrock Nova-lite.

That fix is correct for the narrow Nova pattern but violates the first principles
established in [02-first-principles.md](02-first-principles.md):

> First Principle 3: Narrated intent is a negative completion signal.

The heuristic detects *surface syntax* (future-tense phrases) as a proxy for narrated
intent. As documented in [06-shadow-judge-critique-and-concept.md](06-shadow-judge-critique-and-concept.md),
this proxy is:

1. **Not complete** — finite vocabulary cannot cover all surface forms.
2. **Not context-aware** — cannot distinguish delivery from announcement.
3. **Not stable** — breaks silently with model updates.
4. **Not generalizable** — narrow enough to avoid false positives on Nova, but unable to
   detect task incompleteness in any model that phrases deferral differently.

A principled completion gate requires **semantic understanding of the full conversation
trajectory**, not phrase matching over the final message.

The existing architecture already provides the necessary primitives:

| Primitive | Location | Shadow Judge Use |
|-----------|----------|-----------------|
| `AuxiliaryConfig` | `config.rs` | Route shadow calls to a cheap model |
| `Arc<dyn LLMProvider>` | `agent.rs` | Make the shadow API call |
| `SteeringReceiver` / `steer_tx` | `steering.rs` | Deliver the steering hint to the loop |
| `session.messages` snapshot (clone) | `conversation.rs` | Provide read-only history to judge |
| `LoopAction::Done(text)` branch | `conversation.rs` | Integration point: veto before break |

---

## Decision

Add a **Shadow Judge** — a single-call LLM completion oracle that:

1. Fires only after `DefaultCompletionPolicy::assess()` returns `Completed`.
2. Sends a read-only snapshot of the conversation history to a lightweight LLM.
3. Returns a structured JSON verdict (`complete` / `incomplete` + confidence + reason +
   optional steering hint).
4. On `incomplete`: injects the steering hint as a `Message::user(...)` into
   `session.messages` and continues the loop.
5. On `complete` or error: does nothing. The loop breaks normally.

The shadow judge is:
- **Opt-in** (default: disabled).
- **Configurable** via a new `shadow_judge` section in `config.yaml`.
- **Auto-suggested** for known weak models in the model catalog.
- **Cost-safe** due to Anthropic prompt cache reuse (~$0.004 per invocation).

---

## Why This Decision

### Why an LLM oracle and not a better heuristic

The completion question is semantically complex. No finite static rule set can cover all
surface forms of "deferred intent" across all current and future models. An LLM oracle
operates at the same semantic level as the content it is evaluating — it reads intent, not
just syntax. The error rate of a well-prompted LLM judge for this narrow classification
task is substantially lower than the false-positive/false-negative rate of any heuristic
that is also required to avoid breaking strong models.

See §1 and §2 of [06-shadow-judge-critique-and-concept.md](06-shadow-judge-critique-and-concept.md)
for the full technical argument.

### Why not a child Agent (`CoreSubAgentRunner`)

`CoreSubAgentRunner` creates a full `Agent` instance with its own `execute_loop`, tool
dispatch, session DB, todo store, and iteration budget. This is appropriate for delegated
sub-tasks. For a single classification query it is 100× more overhead than necessary. The
shadow judge requires only one `provider.chat_with_tools()` call with no tool dispatch.

### Why not integrate into `CompletionPolicy` trait

`CompletionPolicy::assess()` is synchronous and takes no `Arc<dyn LLMProvider>`. Making
it async would require changing every call site in `conversation.rs` and would couple the
cheap synchronous assessment path to a potentially slow network call. The shadow judge
belongs at the call site in `conversation.rs` — after the synchronous assessor passes, as
an optional async veto.

### Why not require `report_task_status` for all completions

`report_task_status` requires the main agent to explicitly signal completion. This was
rejected in ADR 003 as a behavioral change across all models. The shadow judge is additive
and external — it does not change the protocol the main agent follows.

### Why veto-only (downgrade, not upgrade)

The shadow judge can observe that the task is *not* done by checking evidence gaps. It
cannot reliably certify that a task *is* done — completion is a stronger claim than
incompletion, and false positives (judge says "done" when not) would end the session
early. The conservative design is: the judge vetoes "complete" verdicts but never promotes
"incomplete" verdicts to "complete".

### Why keep ADR 003's heuristic

ADR 003's heuristic is a zero-cost first pass. It correctly handles the Nova specific
pattern with no latency and no API call. The shadow judge fires only when the heuristic
passes. Removing the heuristic would force a shadow API call on every completion decision,
including trivial cases that the heuristic handles cheaply. The layered architecture
(heuristic fast-path → LLM slow-path) minimizes cost and latency.

---

## Consequences

### Positive

- Semantic completion gate applicable to all LLM models without per-model tuning.
- Steering hints are specific and actionable (judge knows what sub-goal is missing),
  replacing the generic "do not stop yet" message.
- Near-zero token cost due to prompt cache reuse.
- Opt-in; zero impact on strong-model sessions that don't need it.
- Generalizable beyond Nova: benefits all weak models and complex multi-step tasks.
- `max_per_session` guard prevents infinite correction loops.

### Negative

- Adds ~300–1500ms latency per invocation on the completion branch.
- Requires a second LLM API credential / routing config if the judge model is different
  from the main model (mitigated by sharing `AuxiliaryConfig`).
- Judge model quality affects verdict quality. An unreliable judge model (e.g., using
  Nova-lite itself as judge) may be no better than the heuristic.
- Not a silver bullet for all forms of model misbehavior. The judge catches task
  incompleteness; it cannot fix context exhaustion, tool failures, or model hallucination.

---

## Rejected Alternatives

### A. Improved heuristic with wider vocabulary

Rejected. Adding more phrases to the heuristic is a patch, not a fix. The open-world
language problem ensures any finite vocabulary is always incomplete. See §1.1 of doc 06.

### B. Model-specific `tool_choice = required` forcing

Rejected. Too blunt. Forces tool calls even when a final text answer is the correct
response to the user.

### C. Full AsyncCompletionPolicy trait

Rejected. Would require all synchronous `CompletionPolicy::assess()` callers to become
async. Too much churn for a selectively-needed feature.

### D. Background shadow call (fire-and-forget like learning reflection)

Rejected. The shadow judge needs to veto the loop *before* `final_response = text; break`
is reached. A fire-and-forget call cannot block the loop. Background mode is for
post-session side effects (skills, memory). The judge is a pre-break gate.

### E. Prompt-based instruction to the main model to call `report_task_status`

Rejected. Strong models already self-report reliably. Weak models ignore or misuse the
instruction — this is the fundamental problem the shadow judge is designed to solve.

---

## Design Constraints (Non-Negotiable)

| # | Constraint | Origin |
|---|-----------|--------|
| C1 | Shadow call MUST NOT mutate `session.messages` | Prompt cache preservation |
| C2 | Shadow call MUST NOT rebuild the main session's system prompt | Cache invalidation risk |
| C3 | Shadow result MUST be structured JSON parseable without LLM retry | Reliability |
| C4 | Shadow judge MUST be skippable without code changes (config `enabled: false`) | User control |
| C5 | Shadow judge invocations per session MUST be bounded (`max_per_session`) | Loop safety |
| C6 | Shadow judge failure MUST be non-fatal (fall back to synchronous assessor) | Reliability |
| C7 | Shadow judge tokens MUST be accounted in session usage totals | Cost transparency |

---

## Configuration Schema

New top-level `config.yaml` section:

```yaml
shadow_judge:
  enabled: false                      # opt-in
  model: null                         # null → use auxiliary.model → use main model
  provider: null                      # null → use auxiliary.provider → use main provider
  max_per_session: 5                  # hard cap on invocations per session
  confidence_threshold: 0.70          # below this confidence → treat as "complete"
  context_messages: 20                # last N messages to send (0 = all)
  min_messages_before_enable: 4       # skip judge for very short sessions
```

---

## Migration Path

1. **Phase 1 (this ADR):** Implement `ShadowJudgeConfig`, `run_shadow_judge()`, and
   integration point in `conversation.rs`. Deploy with `enabled: false` default.
2. **Phase 2:** Auto-suggest `enabled: true` in setup wizard for known weak models.
   Update model catalog YAML with `suggest_shadow_judge: true` annotations.
3. **Phase 3 (long term):** Once shadow judge is battle-tested, narrow or remove the
   ADR 003 deferred-work heuristic. The shadow judge subsumes its coverage.

---

_Implementation details: [08-shadow-judge-implementation.md](08-shadow-judge-implementation.md)_
