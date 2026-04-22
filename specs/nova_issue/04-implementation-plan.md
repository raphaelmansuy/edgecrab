# Bedrock Nova Premature Stop — Implementation Plan

Cross-ref: [03-adr-completion-gate.md](03-adr-completion-gate.md), [05-verification-plan.md](05-verification-plan.md)

## Files

- `crates/edgecrab-core/src/completion_assessor.rs`
- optionally `crates/edgecrab-core/src/conversation.rs` if follow-up wording needs tightening

## Planned changes

1. Add a helper that detects recent tool activity in the message history.
2. Add a helper that detects deferred-work language in the final assistant text.
3. In `assess_completion`, classify that combination as `Incomplete`.
4. Add unit tests for:
   - a Nova-style premature "Now I'll ..." response after a tool result,
   - a valid final answer after a tool result,
   - future-tense text without recent tool activity.

## Non-goals

- No Bedrock protocol rewrite.
- No provider-specific retry loop.
- No change to the AWS Bedrock `StopReason` mapping.