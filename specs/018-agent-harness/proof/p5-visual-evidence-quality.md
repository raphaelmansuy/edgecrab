# Proof: P5 VisualUx evidence quality

**Date:** 2026-07-18  
**Status:** landed (session forensics game005)  
**Session seed:** `a63fd17c` — `Completed` after navigate-only while vision showed chrome-error

## Law

1. For `TaskClass::VisualUx` with strict verification, `browser_navigate` alone
   does **not** satisfy evidence.
2. Perception tools (`browser_snapshot`, `browser_vision`, `vision` /
   `analyze_image` / `vision_analyze` / `capture_screenshot`) count only when
   content is not a chrome-error / network-error / empty-loader page.
3. `chrome-error://` or “didn’t send any data” bodies never populate
   `VerificationSummary.evidence`.

## Anchors

| Piece | Location |
|-------|----------|
| Tool allowlist | `task_class::is_verification_tool_for_class` (VisualUx) |
| Quality filter | `completion_assessor::visual_perception_evidence_ok` |
| Assess gate | `assess_completion` NeedsVerification when perception missing |

## Verify

```bash
cargo test -p edgecrab-core --lib visual_perception
cargo test -p edgecrab-core --lib navigate_alone_does_not_satisfy_visual_ux
cargo test -p edgecrab-core --lib chrome_error_not_visual_evidence
```
