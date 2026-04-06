# Delegation Parity Audit

This audit compares `edgecrab` against Hermes on four first-principles concerns:

1. Delegation must stay bounded.
2. Context-pressure display must reflect real prompt pressure, not a misleading subset.
3. Compression warnings must remain informative without entering noisy or flaky loops.
4. Users must be able to see what delegated agents are doing, across CLI and gateway paths.

## Implemented fixes

- Status-bar context tokens now use prompt-side pressure (`input + cache_read + cache_write`) instead of `input + output`.
- Context-window percentage in the TUI is now clamped to `100%`.
- Same-turn `delegate_task` bursts are capped before dispatch so one model response cannot fan out beyond the supported concurrency limit.
- Delegated child reasoning now flows through the event pipeline. The TUI uses it to update live delegated-task status; the gateway surfaces it only when reasoning display is enabled.
- `delegate_task` results now expose prompt/cache/reasoning token buckets in addition to plain input/output counts.

## Environment-specific test matrix

### Core loop

- Unit: cap excess `delegate_task` calls in one model turn while preserving non-delegate calls and order.
- Unit: compression warning fires once, resets after successful compression, and never reports percentages above `100`.
- Unit: streamed usage preserves authoritative provider usage when present and falls back to deterministic estimation when omitted.
- Integration: subagent cancellation propagates from parent interrupt before any further child tool dispatch.

### CLI / TUI

- Unit: status bar uses prompt-side context tokens and clamps percentages at `100`.
- Unit: delegated reasoning/tool events update the latest active-subagent summary instead of flooding the transcript.
- Integration: long-running delegated tasks keep a stable status bar and transcript with no duplicate placeholder rows.
- Integration: resumed sessions restore prompt-pressure counters from persisted cache/read/write buckets.

### Gateway

- Unit: delegated start, reasoning, tool batches, and finish messages stay ordered and thread-safe.
- Integration: webhook-like platforms suppress noisy progress; edit-capable platforms receive progressive status without duplicated final delivery.
- Integration: session continuity after mid-run compression persists the compressed transcript and does not lose the summary turn.

### Tool modality: `terminal`

- Integration: parallel-safe delegated terminal/file/web mixes preserve one result per tool call, even when one task panics or is cancelled.
- Integration: approval-gated commands still block correctly when triggered inside a delegated run.
- Integration: background-process output does not corrupt delegated progress rendering.

### Tool modality: `execute_code`

- Integration: delegated children remain blocked from `code_execution` toolsets, both by requested toolsets and inherited parent toolsets.
- Integration: parent `execute_code` runs do not consume delegated child iteration budgets and vice versa.
- Regression: no shared process/session state leaks across delegated code or terminal sessions.

## Remaining gaps vs Hermes

- `edgequake-llm` still does not expose cache-write tokens on the normalized streaming response contract, so session accounting can only show cache-write tokens when the upstream provider path supplies them through higher layers.
- EdgeCrab still does not persist an explicit “last prompt tokens” scalar the way Hermes does. The TUI now computes equivalent prompt pressure from session buckets, but the persistence model could still be tighter.
- Delegated reasoning is currently a live status signal, not a full transcript artifact. This is the right default for UX, but Hermes has more battle-tested history around tuning that noise floor.
