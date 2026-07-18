# 018 — Force-Multiply EdgeCrab Leads (July 2026)

**Date:** 2026-07-18  
**Thesis:** Binding Constraint ([arXiv:2605.23950](https://arxiv.org/abs/2605.23950)) — deepen harness controllers where EdgeCrab already leads; do not chase Hermes breadth or Pi package culture.

## First principles

```text
observe → plan → act → observe
LLM = stochastic policy
Harness = controller (context, tools, orchestration, verification)
```

| Law | Meaning |
|-----|---------|
| **One Done** | One `RunOutcome` from one `CompletionPolicy` path |
| **Cache law** | Goals/steers/hooks/footers → `messages` only; never mutate `cached_system_prompt` mid-turn |
| **Evidence > vibes** | `Completed` requires class-appropriate evidence in `VerificationSummary` |
| **Armed defaults** | Guardrails hard-stop ON; VisualUx strict when classified |
| **DRY / SOLID** | One owner per concern; extract before adding; no parallel judge stacks |

## Lead basins (roadmap budget)

| Lead | SoT |
|------|-----|
| L1 Integrity / VERIFY | `completion_assessor`, `turn_epilogue`, `harness` types, `GoalContract` |
| L2 Cache / cost | `prompt_builder`, `prompt_cache_policy`, doctor SLO |
| L3 Indexed tools | `tool_schema_index`, `tool_input_examples` (hot set = 5) |
| L4 Loop physics | `harness_loop_policy`, `turn_dispatch_policy`, guardrails |
| L5 Trust defaults | path/SSRF/command scan, `PreviewConfig`, `prepare_tool_result_body` |
| L6 Binary + gateway | one binary, ACP, `--json-stream`, lifecycle hooks |

## Document map

| Doc                                                     | Purpose                                                                                                                                                        |
| ---------------------------------------------------------| ----------------------------------------------------------------------------------------------------------------------------------------------------------------|
| [001-peer-field-2026-07](./001-peer-field-2026-07.md)   | Pi / Hermes / Claude Code latest                                                                                                                               |
| [002-stale-claims-jun016](./002-stale-claims-jun016.md) | Jun 016 vs July code                                                                                                                                           |
| [003-dry-solid-ownership](./003-dry-solid-ownership.md) | Crate matrix + ban list                                                                                                                                        |
| [004-implementation-plan](./004-implementation-plan.md) | W1–W4 workstreams                                                                                                                                              |
| [005-session-forensics-game005-2026-07-18](./005-session-forensics-game005-2026-07-18.md) | game005 VisualUx forensics (ERR_EMPTY / EADDRINUSE / false Completed) |
| [006-session-forensics-pptx-4bcc4a8f](./006-session-forensics-pptx-4bcc4a8f.md) | pptx Document thrash (VisualUx misclass / bind race / budget) |
| [007-session-forensics-docx-83c60363](./007-session-forensics-docx-83c60363.md) | docx never-stop (CodeChange misclass / reopen / screencapture) |
| [008-session-forensics-pptx-style-ref-da8b08e4](./008-session-forensics-pptx-style-ref-da8b08e4.md) | pptx style-ref false Done (`ls` of reference ⇒ Completed, empty target) |
| [proof/](./proof/)                                      | Acceptance proofs ([p0](./proof/p0-one-assess-contract.md)–[p9](./proof/p9-document-create-evidence.md), [f1](./proof/f1-no-flaky-heuristics.md)) |

## Explicit non-goals

Spotify/Feishu tools, Electron desktop, Codex app-server clone, full Hermes plugin port, hot set > 5 without meter proof, Anthropic-only `defer_loading` as primary path, MoA as core, Claude 30-event hook zoo.

## Related

- [`../004-sota-harness/`](../004-sota-harness/) — six-meter peer scorecard
- [`../016-harness-assessment/`](../016-harness-assessment/) — Jun 2026 forensics (partially stale)
