# Bedrock Nova Premature Stop — First Principles

Cross-ref: [01-problem-analysis.md](01-problem-analysis.md), [03-adr-completion-gate.md](03-adr-completion-gate.md)

## First Principle 1: Provider stop != task complete

`stopReason = end_turn` means only that the model stopped emitting tokens for
this API call.

It does **not** prove that the user's requested outcome exists yet.

## First Principle 2: Tool success != workflow success

A successful `write`, `read_file`, or `terminal` call is only evidence that one
step ran. It is not evidence that the whole requested workflow is finished.

Creating `./game2/` is not equivalent to creating the whole game.

## First Principle 3: Narrated intent is a negative completion signal

When the assistant says:

- "Now I'll create..."
- "Let me write..."
- "Next I will run..."

the assistant is explicitly stating that a required step still lies in the
future.

That language is evidence of incompleteness, not completion.

## First Principle 4: The safest place to fix this is the completion gate

The Bedrock provider should continue to map AWS semantics faithfully.

The conversation loop already has a generic recovery path for incomplete final
text:

- `assess_completion(...)` returns `Incomplete`.
- `conversation.rs` injects a follow-up system nudge.
- the ReAct loop continues.

Therefore the least invasive fix is to improve completion assessment, not to
invent Bedrock-specific tool replay logic or special-case the provider.

## First Principle 5: Precision requires structural context

Future-tense phrases alone are too broad.

To avoid false positives, the heuristic should require both:

1. recent tool activity, and
2. explicit deferred-work phrasing in the final assistant text.

That keeps normal explanatory answers safe while catching Nova's premature
handoff behavior.

## Design Invariants

| # | Invariant |
|---|-----------|
| I1 | Preserve Bedrock provider semantics exactly |
| I2 | Preserve existing assistant `toolUse` + user `toolResult` replay |
| I3 | Classify narrated-next-step responses after tool activity as `Incomplete` |
| I4 | Do not require model-specific branches when a generic completion heuristic works |
| I5 | Add regression tests for both positive and negative cases |