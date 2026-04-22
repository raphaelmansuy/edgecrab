# Shadow Judge — Critique of the Heuristic and First-Principles Design

Cross-ref: [00-index.md](00-index.md), [02-first-principles.md](02-first-principles.md),
[03-adr-completion-gate.md](03-adr-completion-gate.md), [07-adr-shadow-judge.md](07-adr-shadow-judge.md),
[08-shadow-judge-implementation.md](08-shadow-judge-implementation.md)

---

## 1. Why the Heuristic Is Wrong — A Brutal Analysis

The heuristic fix in `completion_assessor.rs` (ADR 003) scans the first 240 characters of
the final assistant message for phrases such as `"Let me"`, `"I will"`, `"I'm going to"`,
gated by a check that a tool result appeared in the last six messages.

This is **syntactic classification over semantic content**. It solves a surface symptom
while leaving the root structural gap open.

### 1.1 The Open-World Language Problem

Natural language has infinitely many surface forms for the same underlying meaning.
The action-verb vocabulary in the heuristic is a finite, hand-curated list.
Any list curated today is already stale tomorrow because:

- Models change (fine-tuning, RLHF, instruction tuning alters output phrasing).
- The current list contains 13 phrases. The English language has hundreds of ways
  to signal deferred intent: `"I plan to"`, `"I intend to"`, `"I'll go ahead and"`,
  `"I should now"`, `"the next step is"`, `"subsequently I will"`, etc.
- The list will drift further as Nova and other weak models are updated by AWS/OpenAI/Google.

**Consequence:** Every model update can silently break coverage. The heuristic requires
permanent maintenance with no principled stopping condition.

### 1.2 The 240-Character Window Fallacy

The heuristic only looks at the first 240 characters of the final message.
This was chosen because Nova's deferred-work phrases typically appear early in the text.
But:

- Nova's phrasing is not contractually stable. A different Nova instruction tune can
  reorder the text.
- Other models (GPT-4o-mini, Mistral-small, Gemini-flash) can produce deferred-work
  signals mid-response or late-response.
- 240 characters is not derived from any model contract or tokenizer analysis.
  It is an empirical number from one observed Nova failure.

**Consequence:** The heuristic misses any model that happens to produce deferred-work
language beyond the 240-character mark.

### 1.3 The Context-Blindness Problem

The heuristic has **no concept of what the original user task was**.

Consider: the user asks `"Write me a poem about the ocean."` The model replies:
`"Let me write that poem for you:\n\nDeep calls to deep..."`. The heuristic fires
`has_deferred_work_signal` because "Let me" + "write" matches, and there was
a previous tool call (e.g., reading context from disk), so `has_recent_tool_activity`
is true.

**Result: false positive.** The task is complete. The poem is in the response. The model
is correctly using "Let me write" as a rhetorical preamble to inline delivery, not as an
announcement of a future step. The heuristic incorrectly injects a continuation nudge.

The heuristic cannot distinguish:
- "Let me write the file" → **deferred** (the file hasn't been written yet)
- "Let me write that poem for you: [poem]" → **not deferred** (the poem is the next token)

Only a semantic reasoner that understands the context can make this distinction.

### 1.4 The Precision-Recall Trade-off Has No Floor

There is no principled way to tune the heuristic to be both precise and complete:

- **If we narrow the vocabulary**: more false negatives (real deferral not caught).
- **If we widen the vocabulary**: more false positives (completion falsely blocked).

The only way to achieve both is to understand the semantics of the conversation, which
the heuristic cannot do.

### 1.5 The False-Positive Cost Is Non-Trivial

A false positive causes the loop to continue when the task is done. This:

- Wastes tokens on the follow-up nudge and the model's response to it.
- Can cause the model to re-do work it already completed.
- Creates a confusing user experience where the agent "keeps going" after finishing.
- Consumes iteration budget unnecessarily.

For strong models (Claude Opus 4, GPT-4o), which are already self-consistent about
task completion, the heuristic is more likely to create false positives than to fix
anything real.

### 1.6 The Adversarial Update Problem

If a weak model is fine-tuned to work well with EdgeCrab, it will learn to avoid the
trigger phrases even when deferring work. The heuristic is not robust to model updates.
It is permanently one step behind model behavior with no principled termination condition.

### 1.7 Summary: The Heuristic Violates First Principle 3 (Structural Context)

From [02-first-principles.md](02-first-principles.md), First Principle 5:

> Future-tense phrases alone are too broad.  
> To avoid false positives, the heuristic should require both:
> 1. recent tool activity, and
> 2. explicit deferred-work phrasing in the final assistant text.

The existing specs acknowledged this limitation. The heuristic is therefore the **minimum
viable patch**, not the permanent solution. This document establishes why, and what the
principled replacement is.

---

## 2. First-Principles Derivation of Completion Detection

### 2.1 The Fundamental Question

A task is complete if and only if:

> Every sub-goal of the original user request has been addressed, and the state of the
> world (files, processes, services, conversation) matches the intended final state.

This definition has three key properties:

1. It is **semantic** — it depends on understanding intent, not surface text.
2. It is **trajectory-global** — it requires reading the full conversation history,
   not just the last message.
3. It is **evidence-requiring** — it requires that the state change is real, not just
   announced.

### 2.2 What Information Is Required to Detect Completion

To evaluate whether a task is complete, a decision-maker needs:

| Required Information | Available to Heuristic | Available to LLM Judge |
|----------------------|------------------------|------------------------|
| Original user intent | ❌ (no memory) | ✅ (read first message) |
| Sub-goals enumerated | ❌ | ✅ (read the whole trajectory) |
| Tool execution evidence | ✅ (checks last 6 msgs) | ✅ |
| Semantic meaning of final text | ❌ | ✅ |
| Relationship between sub-goals | ❌ | ✅ |
| Model-specific phrasing patterns | ❌ (requires tuning) | ✅ (naturally handles) |

The heuristic satisfies 1/6. An LLM judge satisfies 6/6.

### 2.3 The Self-Referential Justification

An LLM is the right entity to judge LLM task completion because:

1. **Same capability**: The judge can understand sub-goals that require LLM reasoning.
2. **Same language**: The judge reads the same natural language output that humans read.
3. **Different task**: The judge performs a narrow classification task (complete / not complete)
   which is reliably elicitable via a constrained, targeted prompt, unlike the open-ended
   generative task of the main agent.
4. **Verifiable**: The judge's output is structured JSON, not prose — machine-readable and
   auditable.

This is not circular: a "is this done?" query is provably narrower in scope than an
"execute this task" query. The classification error rate for well-designed judges over
structured trajectories is much lower than the false-positive/false-negative rate of
any static heuristic.

### 2.4 First Principle: The Shadow Judge Invariants

| # | Invariant | Rationale |
|---|-----------|-----------|
| SJ-1 | Shadow judge **never writes** to `session.messages` | Preserves session purity and Anthropic prompt cache |
| SJ-2 | Shadow judge is **read-only** on session history | Prevents state pollution of the main conversation |
| SJ-3 | Shadow judge can only **downgrade** (Completed → Incomplete), never upgrade | Veto-only semantics; avoids falsely ending blocked runs |
| SJ-4 | Shadow judge uses an **isolated provider call** with its own message list | Cache on the main session is untouched |
| SJ-5 | Shadow judge **reuses the cached stable system block** | Makes the shadow call nearly free on Anthropic |
| SJ-6 | Shadow judge is **opt-in** and configurable per model family | Avoids overhead on strong models that don't need it |
| SJ-7 | Shadow judge has a **per-session invocation limit** | Prevents infinite correction loops |
| SJ-8 | Shadow judge fires only when the synchronous assessor returns **Completed** | Short-circuits cleanly; avoids redundant calls |
| SJ-9 | Shadow judge output is **structured JSON** (`verdict`, `confidence`, `reason`, `steering_hint`) | Machine-readable, testable, auditable |
| SJ-10 | Shadow judge tokens are accounted in the session's **total token usage** | Cost transparency |

---

## 3. The Shadow Judge Concept

### 3.1 What It Is

A *shadow judge* is a single, stateless LLM classification call that:

1. Takes a snapshot of the current conversation history (read-only clone of
   `session.messages`).
2. Prepends a compact system prompt: "You are a task-completion oracle. Output JSON."
3. Appends the original user goal and a structured classification question.
4. Makes one chat API call — **no tools, no streaming, no session persistence**.
5. Parses the JSON verdict.
6. If incomplete: constructs a targeted continuation message (steering hint) and injects
   it into `session.messages` as a `Message::user(...)`.
7. If complete: does nothing. The main loop breaks normally.

The judge's own API call and response are never added to `session.messages`. The session
is unmodified except for the optional steering hint if the task is incomplete.

### 3.2 What It Is Not

- **Not a child Agent**: `CoreSubAgentRunner` spawns a full `Agent` instance with its own
  `execute_loop`, tool dispatch, session DB, todo store, etc. That is 10× more overhead.
  The shadow judge is a single `provider.chat_with_tools()` call with no tool dispatch.
- **Not compression**: `compress_with_llm` reshapes the session history. The shadow judge
  reads it but does not reshape it.
- **Not learning reflection**: `run_learning_reflection_bg` fires after session completion
  to persist skills/memories. The shadow judge fires *before* completion to veto premature
  exits.

### 3.3 Judge Prompt Design

The judge prompt is designed to:

1. Be minimal (~120 tokens) so it doesn't pollute the context or cost much.
2. Be precise about the output format.
3. Err toward "incomplete" (strict judge) to avoid missing real gaps.

```
System (shadow judge only):
  You are a task-completion oracle. Your only output MUST be a JSON object.
  No prose. No explanation outside the JSON.
  Schema: {"verdict":"complete"|"incomplete","confidence":0.0-1.0,"reason":"<1 sentence>","steering_hint":"<next action if incomplete, else null>"}
  Rules:
    - "complete" means ALL parts of the user's original request are DONE with evidence.
    - If the agent described a future action but has not taken it yet, output "incomplete".
    - If any sub-task is missing evidence, output "incomplete".
    - Be strict. When uncertain, prefer "incomplete".
```

Final message appended to the judge's message list (not the main session):

```
[shadow-judge query]
Original user request: <first user message in the session>
Has this request been fully completed? Check all sub-goals against the conversation history.
Output JSON verdict.
```

### 3.4 Token Economics (Anthropic)

Anthropic pricing (claude-haiku-4 family as reference judge model):
- Cache read: $0.30 / MTok
- Cache write: $3.75 / MTok  
- Output: $1.25 / MTok (haiku) vs $15 / MTok (Opus)

**For a 10k-token conversation (typical), with prompt caching active:**

| Component | Tokens | Rate | Cost |
|-----------|--------|------|------|
| Stable system block (cached) | 2,000 | $0.30/MTok cache read | $0.0006 |
| Conversation history (cached) | 10,000 | $0.30/MTok cache read | $0.003 |
| Judge prompt (new) | 120 | $3.75/MTok cache write | $0.00045 |
| Verdict output | 60 | $1.25/MTok output | $0.000075 |
| **Total shadow call** | **12,180** | — | **~$0.004** |

A typical Opus-4 session runs $0.50–$2.00. The shadow call adds **<0.8%** to session cost.
With a cheap judge model (haiku-4, flash-3, mini), this is negligible.

**If the judge is set to the same model as the main agent (e.g., claude-opus-4):**

| Component | Tokens | Rate | Cost |
|-----------|--------|------|------|
| Conversation history (cached) | 10,000 | $0.30/MTok cache read | $0.003 |
| Verdict output | 60 | $15/MTok output | $0.0009 |
| **Total shadow call** | — | — | **~$0.004** |

Even with Opus as judge, the cost is trivially small because output is tiny and input is cached.

**Latency:** One extra API round-trip to a fast model (haiku, flash) adds ~300–800ms.
With Opus, ~1–3s. Acceptable given the Nova failure mode is 30+ seconds of manual retries.

### 3.5 Cache Isolation Guarantee

The Anthropic prompt cache is keyed on the prefix of the message list, including
`cache_control: ephemeral` markers.

**Main session's cache is safe because:**

1. The shadow judge calls `provider.chat_with_tools()` on a **cloned** message list, built
   independently, never mutating `session.messages`.
2. The judge's call is a separate HTTP request with its own message array.
3. Anthropic's cache is server-side: the cache key depends on the content of the messages
   array you send. The judge's call is a distinct request and creates or hits its own
   cache entries.
4. The judge's call does NOT write `cache_control` markers into `session.messages`.

**There is no shared mutable state between the main session's prompt cache and the
shadow call. They are two separate HTTP requests.**

The one nuance: if the main session's conversation history is cached (as it will be for
messages up to the last `cache_control` breakpoint), the shadow call that reads the same
history will HIT that cache. This is the desired behavior — cheap cache reads, not
expensive re-processing.

---

## 4. Edge Cases

### 4.1 Shadow Judge Returns "Incomplete" Spuriously

**Scenario:** Judge confidently says "incomplete" but the task was actually done.

**Mitigation:**
- `max_per_session` limit (default: 5) caps correction loops.
- `confidence_threshold` (default: 0.70) — if confidence < threshold, treat as "complete"
  to avoid spurious loops.
- The synchronous assessor already handles structural signals (todo list, clarify markers).
  The judge is only called when those pass. If the judge fires 5 times and the task isn't
  progressing, something else is wrong.

### 4.2 Weak Model Used as Judge

**Scenario:** The shadow judge is routed to the same weak model that's causing the
problem (e.g., Nova-lite as both main agent and judge).

**Mitigation:**
- `shadow_judge.model` should default to `auxiliary.model`, which should be a stronger/
  different model. If the user is running Nova-lite + Nova-lite (judge = main), the judge
  may also be unreliable.
- Document this: shadow judge works best when `model` ≠ main agent model.
- Future work: model-family-aware default fallback (e.g., auto-select claude-haiku-4 when
  main model is `amazon.nova-*`).

### 4.3 Shadow Judge Itself Fails (Network, Auth, Timeout)

**Scenario:** The shadow judge API call throws an error.

**Mitigation:**
- Judge failure is non-fatal: `run_shadow_judge()` returns `Option<ShadowVerdict>`.
- `None` is treated the same as `complete` verdict (conservatively fall back to
  synchronous assessor's judgment).
- Error is logged at `tracing::warn!` level for observability.

### 4.4 Infinite Correction Loop

**Scenario:** Judge returns "incomplete" → agent responds → judge again returns "incomplete"
→ repeat forever.

**Mitigation:**
- `max_per_session: 5` (hard cap). After 5 invocations, the shadow judge is skipped for
  the rest of the session.
- The main loop already has `max_iterations` as a hard cap (default: 90).
- If after 5 judge invocations the task still isn't completing, the agent has a deeper
  problem that the shadow judge cannot fix. Log and let the session complete normally.

### 4.5 Prompt Cache Miss on Judge Call (First Turn)

**Scenario:** First invocation of the session — nothing is cached yet.

**Behavior:** Normal cache write cost (same as the main conversation turn). The cache
investment pays for itself on future turns. This is identical to any other first-turn
Anthropic call.

### 4.6 Very Short Sessions (< 3 Messages)

**Scenario:** User asks a one-shot question, agent responds with no tools, shadow judge
fires.

**Mitigation:**
- SJ-8: Shadow judge only fires when the synchronous assessor returns `Completed`.
- For tool-free sessions, the synchronous assessor correctly returns `Completed`.
- The shadow judge should additionally skip when `session.messages.len() < MINIMUM_MESSAGES`
  (suggested: 4). A one-shot Q&A doesn't need a judge.

### 4.7 Token Budget and `max_per_session` Interaction

**Scenario:** Session has used 88 out of 90 max iterations. Shadow judge fires and returns
"incomplete", injecting a continuation nudge. The loop continues but hits the hard cap at
iteration 90.

**Behavior:** Correct. The loop exits due to `budget_exhausted = true`, which the
synchronous assessor already marks as `Incomplete` / `Failed`. The shadow judge doesn't
change this — it only fires when the synchronous assessor has already said "Completed."
If the session is nearly out of budget, the synchronous assessor returns `Failed` due
to budget exhaustion, and the shadow judge never fires (SJ-8).

### 4.8 Streaming vs Non-Streaming

**Scenario:** Shadow judge is called from a streaming main session.

**Behavior:** The shadow judge call is always non-streaming (a single `chat_with_tools`
call awaited for its structured JSON response). The streaming session of the main loop
is unaffected because the shadow judge runs in the `LoopAction::Done` branch, which is
reached only after streaming has completed for that turn.

### 4.9 Session `skip_memory` and `skip_context_files` Flags

**Scenario:** Session has `skip_memory = true`.

**Behavior:** The shadow judge is passed only the conversation history snapshot (no memory
or context files). The judge's system prompt is self-contained. The skip flags on the main
agent config do NOT apply to the shadow judge's internal prompt because the judge has its
own dedicated system prompt.

### 4.10 Anthropic Prompt Caching and the Dual System Block

The main session uses a stable/dynamic system block split (see `build_chat_messages_blocks`
in `conversation.rs`). The stable block has `cache_control: ephemeral`.

The shadow judge builds its own message list:
1. A single judge system prompt (no stable/dynamic split needed).
2. The conversation history clone from `session.messages`.
3. The judge query message.

The shadow judge does NOT set `cache_control` on any messages. This is intentional:
- The conversation history is already cached by the main session's breakpoints.
- A shadow call that reads the same cached content will hit those existing cache entries.
- Setting cache_control on the judge's messages would create new cache entries that
  overlap with the main session's entries, wasting cache write budget.

**Correct behavior:** Shadow call's message list hits the existing cache entries created
by the main session's `apply_cache_control` calls. No `cache_control` needed on the judge
call itself.

---

## 5. Scope Beyond Nova

The shadow judge is not a Nova-specific patch. It is a general solution to the structural
problem described in [02-first-principles.md](02-first-principles.md):

> The safest place to fix this is the completion gate.

The shadow judge upgrades the completion gate from a syntactic filter to a semantic
reasoner. Beneficiaries beyond Nova-lite:

| Scenario | How Shadow Judge Helps |
|----------|------------------------|
| GPT-4o-mini declares completion after first step | Judge identifies missing sub-goals |
| Claude Haiku halts after scaffold with "the rest is left as an exercise" | Judge detects incomplete delivery |
| Gemini Flash-lite narrates future steps in Markdown format | Judge parses intent vs. evidence |
| Any weak model that fails `report_task_status` | Judge provides semantic coverage |
| Complex multi-file tasks where agent misses one file | Judge notices sub-goal gap |
| Long sessions where agent loses track of original goal | Judge re-anchors to first user message |

The shadow judge also enables **proactive steering**: the `steering_hint` field in the
verdict allows the judge to specify exactly what action the agent should take next,
rather than the generic "do not stop yet" message from `build_completion_follow_up_message`.

---

## 6. Relationship to Existing Heuristic (ADR 003)

The shadow judge is a **successor** to ADR 003's heuristic, not a replacement at the same
level of implementation complexity. The layered architecture is:

```
Layer 1 (synchronous, free):
  DefaultCompletionPolicy::assess()
  ├── Active todos → Incomplete
  ├── Clarify/approval pending → Incomplete
  ├── has_remaining_steps → Incomplete
  ├── [ADR 003] deferred_work heuristic → Incomplete   ← current state
  └── else → Completed

Layer 2 (async, cheap LLM call):
  run_shadow_judge()                                    ← proposed
  ├── verdict="incomplete", confidence ≥ threshold → Incomplete + steering_hint
  └── verdict="complete" or error → Completed (pass through)
```

Layer 1 remains in place. It handles structural signals cheaply and is correct for the
easy cases. Layer 2 only fires when Layer 1 passes, providing semantic coverage for the
cases Layer 1 cannot handle.

**Should ADR 003's heuristic be removed once the shadow judge is implemented?**

Not immediately. The heuristic provides zero-cost coverage for the specific Nova pattern.
It should be retained as a free fast-path guard. Long term, once the shadow judge is
battle-tested, the deferred-work heuristic can be narrowed or removed. This is tracked
in the implementation plan.

---

## 7. Activation and Configuration Philosophy

### 7.1 Default Off

Shadow judge is off by default. Most sessions on strong models do not need it. Adding
latency and cost without user consent is poor defaults discipline.

### 7.2 Auto-Suggest for Known Weak Models

The `model_catalog.yaml` or `AgentBuilder` can carry a `suggest_shadow_judge: true` flag
for known weak models. The CLI / setup wizard surfaces a suggestion: "For best results
with amazon.nova-lite-v1:0, consider enabling shadow_judge in your config."

This is a suggestion, not auto-enable. The user controls costs.

### 7.3 Per-Session Override

Config key `shadow_judge.enabled` is read from the layered config (default → disk →
env → CLI). A per-session override is supported via a flag or CLI argument, consistent
with the existing config override pattern.

### 7.4 Model Routing

`shadow_judge.model` defaults to `auxiliary.model`. If `auxiliary.model` is not set,
falls back to the main agent model. The recommended configuration is:

```yaml
auxiliary:
  model: "anthropic/claude-haiku-4-20250514"
  provider: "anthropic"

shadow_judge:
  enabled: true
  # model: null → inherits auxiliary.model = claude-haiku-4
```

This routes all side-task LLM calls (compression, shadow judge) to a cheap fast model.

---

_See [07-adr-shadow-judge.md](07-adr-shadow-judge.md) for the decision record and
[08-shadow-judge-implementation.md](08-shadow-judge-implementation.md) for the
concrete implementation plan._
