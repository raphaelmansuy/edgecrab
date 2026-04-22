# The Shadow Judge: Teaching AI Agents to Doubt Themselves

Your AI agent just said "I'll now create the CSS file." Then stopped. Task incomplete. No CSS. Session over.

This is one of the most common silent failures in autonomous AI agents: the model emits a stop signal while narrating a future action as if announcing it were the same as executing it. The framework sees "stop," breaks the loop, and returns a half-finished result to the user.

The naive fix is a phrase-matching heuristic — scan for "I will," "Let me," "I'm going to." Block the exit if those phrases appear. It works for a week, until the model updates and starts saying "I should now" or "The next step is." No finite vocabulary list can cover all surface forms of deferred intent. Every model update silently breaks coverage.

**The real fix requires semantic understanding.** A task is complete only when every sub-goal has been addressed *with evidence* — tool output, created files, verified results. A phrase list has no concept of the original user goal. It can't distinguish "Let me write the file" (deferred) from "Let me write that poem for you: [poem follows]" (complete delivery).

The right entity to verify that an LLM finished a task is **another LLM** — one given a narrow classification task rather than an open-ended generative one.

```
Main Agent says "done"
        ↓
Synchronous heuristic (fast, free)
        ↓ passes
Shadow Judge — one LLM call
  "Has every sub-goal been completed with evidence?"
        ↓              ↓
    complete       incomplete
        ↓              ↓
   break loop    inject targeted hint → loop continues
```

The Shadow Judge is a single, stateless chat API call. It takes a read-only snapshot of the conversation history, prepends a minimal system prompt ("you are a completion oracle, output JSON only"), and returns a structured verdict: `complete/incomplete`, a confidence score, and — critically — a specific `steering_hint` telling the main agent exactly what is missing.

It **never writes to the main session**. It never invalidates the prompt cache. It reuses the cached conversation tokens, so on Anthropic the entire judge call costs ~$0.004 — less than 0.8% of a typical session. Latency: 300–800ms on a fast model.

Three design constraints make this safe to deploy:

**Veto-only semantics.** The judge can downgrade "complete" to "incomplete" but can never upgrade "incomplete" to "complete." This asymmetry prevents false terminations — the conservative direction is always to keep working.

**Session isolation.** The judge's message list is an ephemeral clone. The main conversation is untouched. The Anthropic prompt cache is preserved for all subsequent main-loop calls.

**Bounded by a per-session cap.** Maximum 5 invocations per session prevents infinite correction spirals if something else is wrong.

The result: tasks that stopped one step short now finish. At heuristic cost. With semantic accuracy.

---

*The Shadow Judge pattern was developed while debugging premature stop behavior on AWS Bedrock Nova-lite. It generalizes to any ReAct agent loop where completion matters and model stop signals cannot be fully trusted.*
