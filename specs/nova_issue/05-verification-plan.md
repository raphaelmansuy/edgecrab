# Bedrock Nova Premature Stop — Verification Plan

Cross-ref: [04-implementation-plan.md](04-implementation-plan.md)

## Focused checks

1. Unit tests for `completion_assessor.rs` pass.
2. The premature-stop regression test proves the outcome is `Incomplete`.
3. A normal post-tool final answer is still classified as `Completed`.
4. `cargo test -p edgecrab-core completion_assessor` passes.
5. If the touched slice compiles cleanly, run a narrow crate check for
   `edgecrab-core`.

## Pass criteria

The fix is accepted when EdgeCrab's completion gate no longer treats
"Now I'll ..." style post-tool narration as a completed run.