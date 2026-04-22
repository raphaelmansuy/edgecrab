# ADR 003: Completion Gate For Deferred-Work Responses

Cross-ref: [01-problem-analysis.md](01-problem-analysis.md), [02-first-principles.md](02-first-principles.md), [04-implementation-plan.md](04-implementation-plan.md)

## Status

Accepted

## Context

Bedrock Nova can return a syntactically valid assistant message and stop the
turn even when the requested workflow is not complete.

EdgeCrab already has:

- correct Bedrock tool-use history replay,
- a generic auto-continue loop for `Incomplete` runs,
- a completion assessor that is currently too optimistic.

## Decision

Add a completion-assessment heuristic that marks a run `Incomplete` when:

1. the final assistant text contains explicit deferred-work language, and
2. the recent message history shows tool activity.

Examples of deferred-work language:

- "Now I'll ..."
- "I will ... next"
- "Let me ..."
- "I’m going to ..."

## Why this decision

- It fixes the root cause at the decision boundary where the premature stop is
  misclassified.
- It reuses the existing continuation machinery instead of adding another loop
  or another provider retry path.
- It remains stable even if Bedrock adds more Nova variants, because the issue
  is not the model ID itself but the harness interpretation of the returned text.

## Rejected alternatives

### 1. Bedrock-specific provider retry logic

Rejected because the provider is already faithfully representing AWS semantics.

### 2. Force `tool_choice = required` after every tool result

Rejected because it is too blunt and can force unnecessary tools when the next
turn should legitimately answer in text.

### 3. Require `report_task_status` for all completions

Rejected because it would create a larger behavioral change across all models
and all tasks.

## Consequences

- Nova-lite no longer needs a manual `Continue` for this class of premature stop.
- Other weaker tool-using models benefit from the same safeguard.
- The heuristic must stay narrow to avoid false positives on genuine final text.