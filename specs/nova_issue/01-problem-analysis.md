# Bedrock Nova Premature Stop — Problem Analysis

Cross-ref: [00-index.md](00-index.md), [02-first-principles.md](02-first-principles.md), [03-adr-completion-gate.md](03-adr-completion-gate.md)

## Symptom

With `bedrock/amazon.nova-lite-v1:0`, EdgeCrab may:

1. Execute one tool successfully.
2. Receive a non-empty assistant message such as "Now I'll create the full file...".
3. End the turn as if the request were complete.

The user then has to type `Continue` manually to get the next tool call.

## Runtime Evidence

### AWS contract

From the AWS Bedrock Converse docs:

- `stopReason` explains **why the model stopped generating content**.
- Example final response uses `"stopReason": "end_turn"`.
- The docs do **not** say that `end_turn` means the overall user task is done.

From the AWS Bedrock tool-use docs:

- Client-side tool calling requires the application to continue the
  conversation after a tool result.
- The official sequence is:
  1. Send user message plus tools.
  2. Receive assistant message with `toolUse` and `stopReason == tool_use`.
  3. Execute the tool.
  4. Append a user `toolResult` message.
  5. Call `converse` again.

Conclusion: Bedrock gives a **turn-level** stop signal, not a **task-level**
 completion signal.

### Edgequake / Bedrock provider evidence

`edgequake-llm/src/providers/bedrock.rs` already does the Bedrock-specific
message mapping correctly:

- Assistant tool requests are preserved as `ContentBlock::ToolUse`.
- Tool results are replayed as a user-role `ContentBlock::ToolResult`.
- `StopReason::EndTurn` is mapped to `finish_reason = "stop"`.

This means the provider is representing the Bedrock contract faithfully.

### EdgeCrab loop evidence

`edgecrab-core/src/conversation.rs` already preserves the required assistant
tool-call message before appending the tool result message.

That rules out the most obvious Bedrock protocol bug.

### Actual harness gap

`edgecrab-core/src/completion_assessor.rs` currently marks a run `Completed`
when all of the following are true:

- the run is not interrupted,
- no clarification or approval is pending,
- there are no active TODO markers,
- the final response is non-empty,
- verification debt is not detected.

There is currently no heuristic for this case:

> The assistant returned **future-tense / deferred-work text** immediately after
> tool activity, which means the model narrated its next action instead of
> actually taking it.

That is exactly the failure mode visible with Nova-lite.

## Root Cause

The root cause is **not** Bedrock message replay.

The root cause is that EdgeCrab currently conflates:

- **non-empty assistant text**, and
- **task completion**.

For stronger models this often works accidentally. For Nova-lite it is unsafe,
because the model can legally end a Bedrock turn with interim narration such as
"Now I'll do X".

## Controlling Hypothesis

If EdgeCrab marks responses containing clear deferred-work language as
`Incomplete` when they appear after recent tool activity, then the existing
auto-continue path in `conversation.rs` will keep the loop running without any
manual `Continue` from the user.