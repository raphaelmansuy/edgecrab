# The Shadow Judge: When AI Agents Need a Second Opinion on Themselves

*A general exploration of a practical pattern for keeping autonomous AI loops honest.*

---

## WHY: The Agent That Thought It Was Done

Picture an AI coding assistant running inside your IDE. You ask it to build a small browser game — HTML, CSS, JavaScript, three files. The agent calls its tools, reads the filesystem, writes a partial HTML stub, then emits this:

> "I'll now create the CSS file with the game styles and wire up the event handlers."

Then it stops. No CSS. No JavaScript. Session over.

If you ask the agent whether it finished, it sincerely believes it did. The underlying model produced `end_turn` — the protocol signal for "I am done." The completion logic in the agent framework saw `end_turn`, found no pending tool calls, and broke the loop. From the framework's perspective, every rule was followed.

The problem is that the agent narrated a future action as if announcing it were the same as executing it. The task is objectively incomplete. But no component in the standard ReAct loop caught it, because no component in that loop had enough context to know.

This is not a bug in a single model. It is a **structural gap** in how reactive agent frameworks detect completion.

---

## The Standard Completion Problem

Most ReAct-style agent frameworks use one of three strategies to decide when to stop:

```
Standard Completion Decision Strategies
========================================

  1. Trust the model's stop signal
     ─────────────────────────────
     Model emits end_turn or stop reason → loop breaks.
     Problem: end_turn is often wrong.

  2. Detect explicit task marker
     ────────────────────────────
     Wait for agent to call report_task_status("done") tool.
     Problem: changes the protocol; not all models comply.

  3. Phrase heuristic
     ─────────────────
     Scan final message for future-tense phrases ("I will", "Let me").
     If found → inject nudge; continue loop.
     Problem: brittle, incomplete vocabulary,
              no semantic understanding.
```

All three approaches share the same fundamental limitation: they operate on **surface signals** — stop codes, tool names, or vocabulary lists — not on the **semantic content** of the actual conversation.

---

## Why Heuristics Are Not Enough

A phrase-matching heuristic seems appealing at first. If the model says "I will write the file", keep going. Simple.

But natural language has infinitely many surface forms for the same underlying meaning:

```
Deferred-intent phrases (partial list):
  "I will..."           "Let me..."
  "I'll now..."         "I should now..."
  "I plan to..."        "The next step is..."
  "I intend to..."      "Subsequently I will..."
  "I'll go ahead and..."  "I need to..."
  ... and dozens more across languages and model fine-tunes
```

No finite list covers all of them. Every model update — fine-tuning, RLHF, instruction tuning — can change the phrasing without changing the underlying intent. A heuristic curated for one model version is silently stale for the next.

Worse, heuristics cause **false positives**: the model genuinely finished the task but used a preamble like "Let me write that poem for you: [poem text follows]." The phrase fires, the loop continues unnecessarily, and the agent re-does work it already completed.

The fundamental issue: a heuristic has no concept of what the original user task was. It cannot distinguish "I will write the file" (deferred) from "Let me write you that poem: [inline delivery]" (complete). Only semantic understanding of the full conversation trajectory can make that distinction.

---

## First Principles: What Does "Done" Actually Mean?

A task is complete if and only if:

> Every sub-goal of the original user request has been addressed, **with evidence** that the state of the world matches the intended outcome.

This definition has three important properties:

```
Properties of a Sound "Done" Test
===================================

  1. SEMANTIC
     Depends on understanding intent, not surface text.
     "I wrote the file" is only evidence if the tool call succeeded.

  2. TRAJECTORY-GLOBAL
     Requires reading the full conversation, not just the last message.
     Sub-goal 1 may have been addressed 20 messages ago.

  3. EVIDENCE-REQUIRING
     A promise is not evidence. An announcement is not evidence.
     Only tool output or verifiable artifact is evidence.
```

When you put these three requirements together, one thing becomes clear: the right entity to evaluate completion is something that understands **language**, **context**, and **evidence chains** — in short, an LLM.

The entity best suited to verify that an LLM finished a task is another LLM.

---

## Introducing the Shadow Judge

A **Shadow Judge** is a single, stateless LLM classification call that fires *after* the primary agent signals completion, with one job: verify the claim.

```
Shadow Judge — Control Flow
============================

  Main Agent Loop (ReAct)
  ┌─────────────────────────────────────────────────────┐
  │                                                     │
  │  User message                                       │
  │       │                                             │
  │       ▼                                             │
  │  ┌──────────┐    tool call    ┌──────────────────┐  │
  │  │  LLM     │ ─────────────► │  Tool Executor   │  │
  │  │  (main)  │ ◄───────────── │  (file/terminal) │  │
  │  └──────────┘    tool result └──────────────────┘  │
  │       │                                             │
  │       │ end_turn / stop                             │
  │       ▼                                             │
  │  ┌──────────────────────────┐                       │
  │  │ Synchronous Assessor     │  fast, no API call    │
  │  │ (heuristic pass 1)       │                       │
  │  └──────────┬───────────────┘                       │
  │             │ "Completed"                           │
  │             ▼                                       │
  │  ┌──────────────────────────┐                       │
  │  │   SHADOW JUDGE           │  one LLM call         │
  │  │   (semantic pass 2)      │                       │
  │  └──────────┬───────────────┘                       │
  │             │                                       │
  │     ┌───────┴────────┐                              │
  │     │                │                              │
  │  complete        incomplete                         │
  │     │                │                              │
  │     ▼                ▼                              │
  │  break loop    inject steering hint                 │
  │                ───────────────────                  │
  │                push nudge message                   │
  │                into session.messages                │
  │                → loop continues                     │
  └─────────────────────────────────────────────────────┘
```

The Shadow Judge is:

- **A single chat API call** — no tools, no streaming, no session persistence.
- **Read-only** — it never writes to the main conversation's message history.
- **Veto-only** — it can downgrade "completed" to "incomplete" but can never upgrade "incomplete" to "completed."
- **Fast and cheap** — with a lightweight judge model and prompt caching, each invocation costs roughly $0.004.
- **Opt-in** — it does not activate unless explicitly configured.

---

## Session Isolation: A Critical Design Constraint

One of the most important properties of the Shadow Judge is that it is **invisible to the main conversation**.

```
Session Isolation Diagram
==========================

  session.messages (main loop, never mutated by judge)
  ┌──────────────────────────────────────────────────┐
  │  [system]  cached stable system prompt           │
  │  [user]    "build a browser game"                │
  │  [asst]    "I'll start with the HTML..."         │
  │  [tool]    write_file("index.html", ...)         │
  │  [asst]    "I'll now create style.css..."        │ ← end_turn
  └──────────────────────────────────────────────────┘
          │
          │ clone (read-only snapshot)
          ▼
  shadow_messages (ephemeral, discarded after judge call)
  ┌──────────────────────────────────────────────────┐
  │  [system]  SHADOW_JUDGE_SYSTEM_PROMPT            │
  │  [user]    ... (cloned from session) ...         │
  │  [asst]    ... (cloned from session) ...         │
  │  [user]    SHADOW_JUDGE_QUERY (appended only     │
  │            in this ephemeral list)               │
  └──────────────────────────────────────────────────┘
          │
          │ provider.chat(shadow_messages)
          ▼
  {"verdict":"incomplete","confidence":0.92,
   "reason":"CSS and JS files not created.",
   "steering_hint":"Write style.css and game.js."}
          │
          │ verdict.is_complete == false
          ▼
  session.messages.push(
    Message::user("[system: do not stop. Create style.css and game.js.]")
  )
  → main loop continues
```

The Anthropic prompt cache — keyed on the prefix of the main message list — is never invalidated. The judge's isolated HTTP request uses a separate message array. The main session's cached tokens are preserved, keeping cost low for both the judge call and all subsequent main loop calls.

---

## The Verdict Schema

The judge is prompted to output a single JSON object:

```json
{
  "verdict": "complete" | "incomplete",
  "confidence": 0.0 to 1.0,
  "reason": "one sentence explaining the verdict",
  "steering_hint": "specific next action if incomplete, or null"
}
```

No prose. No markdown fences. Structured and machine-readable.

The `steering_hint` is the high-value field: instead of a generic "keep working" nudge, the judge tells the main agent *exactly* what is missing. "Create style.css with the game styles" is more actionable than "the task is not done."

---

## The Invariants

Ten invariants govern a correctly implemented Shadow Judge:

```
Shadow Judge Invariants
========================

  SJ-1  Never writes to session.messages (read-only)
  SJ-2  History is a clone; never the live reference
  SJ-3  Veto-only: Completed → Incomplete (never reverse)
  SJ-4  Isolated provider call; own message list
  SJ-5  Reuses cached stable system block (cache HIT)
  SJ-6  Opt-in and configurable per model family
  SJ-7  Per-session invocation cap (prevents spirals)
  SJ-8  Fires only when synchronous assessor passes
  SJ-9  Output is structured JSON (auditable)
  SJ-10 Token costs attributed to session total
```

---

## Cost and Latency Analysis

The cost argument for the Shadow Judge is compelling precisely because it is not symmetric. The question it answers — "did the agent actually finish?" — is much narrower than the question the main agent answers. A small, cheap model can answer it reliably.

```
Per-Call Cost (10k-token session, claude-haiku-4 as judge)
============================================================

  Component               Tokens    Rate               Cost
  ───────────────────────────────────────────────────────────
  Stable system (cached)  2,000    $0.30/MTok read    $0.0006
  Conversation (cached)   10,000   $0.30/MTok read    $0.003
  Judge prompt (new)      120      $3.75/MTok write   $0.00045
  Verdict output          60       $1.25/MTok output  $0.000075
  ───────────────────────────────────────────────────────────
  Total per invocation             ≈                  $0.004

  Typical Opus-4 session cost: $0.50–$2.00
  Shadow judge overhead: < 0.8% of session cost
```

The latency cost is 300–800ms on a fast model. The alternative — a missed completion that requires the user to manually re-prompt, observe incomplete output, and restart — routinely costs 30+ seconds and complete loss of context.

---

## Why Not a Child Agent?

A reasonable question: why not spin up a child agent to verify completion? This would reuse the existing delegation infrastructure.

The answer is cost and precision. A child agent — even a minimal one — comes with:

- Its own `execute_loop` with tool dispatch
- A session database connection
- A todo store
- An iteration budget
- A separate streaming channel

This is appropriate for a *task*. It is 100× over-engineered for a binary classification question. The Shadow Judge needs exactly one API call, one response, one JSON parse. Nothing else.

---

## Where This Pattern Applies

The Shadow Judge is not specific to any particular agent framework or model family. The pattern applies wherever:

1. An agent loop breaks on a model-emitted stop signal.
2. The agent's task involves multiple sub-goals (not single-turn Q&A).
3. Completion matters — the loop should not exit early, nor run indefinitely.
4. Token cost is a constraint.

The pattern is model-agnostic. It was motivated by the specific case of AWS Bedrock Nova-lite, which frequently emits `end_turn` with future-tense narration. But because it operates semantically, it generalizes: it catches the same failure mode regardless of which model exhibits it, and regardless of how that model phrases the deferred intent.

The layered architecture is key:

```
Layered Completion Gate
========================

  Layer 1: Synchronous heuristic
  ────────────────────────────────
  Cost: 0 API calls
  Latency: <1ms
  Coverage: known patterns on known models
  Correct when: the failure mode is predictable and narrow
  ↓ passes "Completed"

  Layer 2: Shadow Judge (async LLM)
  ────────────────────────────────────
  Cost: ~$0.004 per invocation
  Latency: 300–800ms
  Coverage: any model, any phrasing, any sub-goal structure
  Correct when: the task has verifiable sub-goals
  ↓ vetoes if evidence gaps found
```

Layer 1 handles the common, cheap case. Layer 2 catches what Layer 1 cannot.

---

## Summary

The Shadow Judge is a lightweight LLM verification step inserted at the moment an agent claims to be done. It does not change how the main agent works. It does not change the protocol the model follows. It adds one cheap read-only oracle call at the single point where the system is most likely to be wrong: the completion boundary.

The central insight is that **the best entity to verify that an LLM finished a task is another LLM** — specifically one given a narrow, well-scoped classification task rather than an open-ended generative one. The result is semantic verification at heuristic cost.

When the judge says "incomplete," the loop continues with a targeted hint. When the judge says "complete" — or errors out — the loop breaks normally. The system degrades gracefully in every failure mode.

The pattern is simple enough to implement in a few hundred lines. The impact — measured in tasks that actually finish rather than stopping one step short — is substantial.
