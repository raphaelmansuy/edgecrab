# 17 — Round 4 Assessment: Brutal Honest "Code Is Law" Audit

> EdgeCrab vs Hermes Agent vs Claude Code. First Principles only.
> Date: 2025-04-20 | Branch: feat/agent-harness-next-release

---

## Executive Summary

EdgeCrab's agent loop is robust (FP1-FP12 all implemented). But 5 gaps
remain where hermes-agent and claude-code patterns would measurably improve
reliability, cost efficiency, and developer experience. This document
identifies those gaps with first-principles reasoning and proposes surgical
fixes.

---

## The 32 KiB Write Limit: First Principles Analysis

```
+----------------------------------------------------------------------+
|                    WHY THE LIMIT EXISTS                               |
+----------------------------------------------------------------------+
|                                                                      |
|  TRANSPORT CONSTRAINT (not filesystem):                              |
|                                                                      |
|    LLM Provider                                                      |
|        |                                                             |
|        v                                                             |
|    SSE Stream  --->  JSON tool_call  --->  { "content": "..." }      |
|                                                                      |
|  The model must emit the ENTIRE content string in one streamed       |
|  tool call argument. At >32 KiB:                                     |
|                                                                      |
|    - Truncated JSON mid-stream (provider timeout)                    |
|    - Malformed UTF-8 at chunk boundary                               |
|    - input_json_delta lost on EOF without trailing newline           |
|    - Provider rate-limit on large single-response payloads           |
|                                                                      |
|  RESULT: Silent data loss > Correct file write                       |
|                                                                      |
+----------------------------------------------------------------------+
|                                                                      |
|  COMPARISON:                                                         |
|                                                                      |
|    Hermes Agent  : NO limit. Accepts truncation risk.                |
|    Claude Code   : NO limit. Output truncation only.                 |
|    EdgeCrab      : 32 KiB hard limit. Scaffold + patch workflow.     |
|                                                                      |
|  VERDICT: EdgeCrab is CORRECT by first principles but TOO RIGID.     |
|                                                                      |
|  BETTER: Make configurable. Default 32 KiB. Allow override.         |
|  WHY: Some providers (local LLMs, high-bandwidth) handle >32 KiB.   |
|  The right default protects; the override empowers.                  |
|                                                                      |
+----------------------------------------------------------------------+
```

---

## Gap Matrix: Round 4

```
+-----+--------------------------------------------+----------+---------+----------+
| #   | Pattern                                    | Hermes   | Claude  | EdgeCrab |
|     |                                            | Agent    | Code    |          |
+-----+--------------------------------------------+----------+---------+----------+
|     | === ALREADY IMPLEMENTED (Rounds 1-3) ===   |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
|  1  | Minimum viable toolset (FP1)               | A-       | B+      | A        |
|  2  | Strict schemas, no content:null (FP2)       | B+       | A-      | A        |
|  3  | Error enrichment with schema hints (FP3)    | B        | B       | A+       |
|  4  | Suppression feedback loops (FP4)            | B        | B+      | A        |
|  5  | Terminal anti-pattern detection (FP5)        | B-       | B       | A        |
|  6  | Consecutive failure escalation (FP6)         | C        | B       | A        |
|  7  | Self-heal malformed JSON (FP7)              | C        | C       | A+       |
|  8  | Provider fallback (FP8)                      | B        | C       | A        |
|  9  | Path-aware parallel safety (FP9)             | C        | B-      | A+       |
| 10  | Stale stream timeout (FP10)                  | B-       | B       | A        |
| 11  | Duplicate tool call detection (FP11)          | C        | C       | A        |
| 12  | Compression circuit breaker (FP12)            | C        | A-      | A        |
+-----+--------------------------------------------+----------+---------+----------+
|     |                                            |          |         |          |
|     | === ROUND 4 GAPS ===                        |          |         |          |
|     |                                            |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| G11 | Read staleness + dedup cache                | YES      | YES     | PARTIAL  |
|     | (mtime tracking, skip unchanged re-reads)   | (full)   | (hash)  | (mtime)  |
+-----+--------------------------------------------+----------+---------+----------+
| G12 | Consecutive read loop detection             | YES      | NO      | NO       |
|     | (same path+offset+limit 3x = warn, 4x=err) | (4-tier) |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| G13 | Dynamic schema cross-ref filtering          | YES      | NO      | NO       |
|     | (strip refs to unavailable tools)            | (post-f) |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| G14 | Memory write injection scanning             | YES      | YES     | NO       |
|     | (block exfil/inject before persist)          | (regex)  | (hooks) |          |
+-----+--------------------------------------------+----------+---------+----------+
| G15 | Configurable write payload limit            | N/A      | N/A     | NO       |
|     | (override 32 KiB default per config)         | (no lim) | (no lim)| (hard)   |
+-----+--------------------------------------------+----------+---------+----------+
| G16 | Compression dedup reset                     | YES      | NO      | NO       |
|     | (clear read cache after compression)         | (reset)  |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| G17 | Iterative summary preservation              | YES      | YES     | PARTIAL  |
|     | (carry forward prior summaries)              | (full)   | (full)  | (detect) |
+-----+--------------------------------------------+----------+---------+----------+
```

---

## First Principles Stack (Round 4 Additions)

```
+----------------------------------------------------------------------+
|                  FIRST PRINCIPLES STACK (Round 4)                     |
+----------------------------------------------------------------------+
|  FP1-FP12: [Rounds 1-3 — all implemented]                           |
|                                                                      |
|  FP13: Don't Re-Read What Hasn't Changed                            |
|        "Unchanged file = wasted context tokens"                      |
|        => mtime-based dedup in read_file + consecutive loop detect   |
|        => Cross-ref: Hermes _read_tracker + dedup dict               |
|                                                                      |
|  FP14: Schema Must Reflect Reality                                   |
|        "Tool descriptions referencing unavailable tools = hallucin"   |
|        => Strip cross-refs when referenced tool is disabled          |
|        => Cross-ref: Hermes get_tool_definitions() post-filter       |
|                                                                      |
|  FP15: Trust Boundary at Memory Write                                |
|        "Memory is injected into system prompt = injection surface"    |
|        => Scan content before persist, block exfil/inject patterns   |
|        => Cross-ref: Hermes memory_tool.py injection scanning        |
|                                                                      |
|  FP16: Defaults Protect, Overrides Empower                           |
|        "Hard limits that can't be overridden punish power users"     |
|        => Configurable write limit with safe 32 KiB default          |
|                                                                      |
|  FP17: Compression Must Not Poison Read Cache                        |
|        "After compression, cached reads point to removed context"    |
|        => Reset read tracker snapshots after compression event       |
|        => Cross-ref: Hermes reset_file_dedup() in compressor         |
|                                                                      |
+----------------------------------------------------------------------+
```

---

## SOLID Audit: What Violations Remain

| Principle | Violation Found | Fix |
|-----------|----------------|-----|
| **S**RP | `edit_contract.rs` owns both limit constants AND enforcement logic. Constants should be in config. | Move limit to `AppConfig`, keep enforcement in `edit_contract.rs` |
| **O**CP | Adding new write limit requires recompilation. | Make limit configurable via `config.yaml` |
| **L**SP | `read_file` returns different error types for "loop detected" vs "file not found". Both should be `ToolError`. | Already correct — both are `ToolError` variants |
| **I**SP | `ToolHandler` trait has `path_arguments()` even for non-file tools. | Default impl returns `&[]` — acceptable |
| **D**IP | `edit_contract.rs` hard-depends on `MAX_MUTATION_PAYLOAD_BYTES` constant. | Inject from config via `ToolContext` |

---

## DRY Audit: What Duplications Remain

| Duplication | Files | Fix |
|-------------|-------|-----|
| `MAX_MUTATION_PAYLOAD_BYTES` referenced in 3 places: edit_contract, prompt_builder, file_write schema | edit_contract.rs, prompt_builder.rs, file_write.rs | Single source: edit_contract exports, others import |
| Read-before-write check logic in both write_file and patch | file_write.rs, file_patch.rs | Already uses shared `read_tracker` module — acceptable |
| Injection scanning patterns exist in `edgecrab-security` but memory tool doesn't use them | security crate, memory.rs | Unify: memory tool should call security crate scanner |

---

## Implementation Specs (Round 4)

See individual spec documents:
- [18-read-dedup-loop.md](18-read-dedup-loop.md) — G11+G12: Read dedup + consecutive loop detection (FP13)
- [19-schema-cross-ref.md](19-schema-cross-ref.md) — G13: Dynamic schema cross-ref filtering (FP14)
- [20-memory-injection-scan.md](20-memory-injection-scan.md) — G14: Memory write injection scanning (FP15)
- [21-configurable-write-limit.md](21-configurable-write-limit.md) — G15+G16: Configurable write limit + compression dedup reset (FP16+FP17)

---

## Scorecard (Post Round 4)

```
+-----+--------------------------------------------+-----+-----+-----+
| FP  | Principle                                  | EC  | HA  | CC  |
+-----+--------------------------------------------+-----+-----+-----+
| FP1 | Minimum Viable Toolset                     | A   | A-  | B+  |
| FP2 | Right Thing Easy, Wrong Thing Impossible    | A   | B+  | A-  |
| FP3 | Every Error Contains Its Own Fix            | A+  | B   | B   |
| FP4 | Feedback Loops Must Close                   | A   | B   | B+  |
| FP5 | Escape Hatch != Easy Path                   | A   | B-  | B   |
| FP6 | Escalate When Stuck                         | A   | C   | B   |
| FP7 | Self-Heal Before Failing                    | A+  | C   | C   |
| FP8 | Resilience Through Redundancy               | A   | B   | C   |
| FP9 | Parallel Safety Is Not Optional             | A+  | C   | B-  |
| FP10| Timeouts Are Not Optional                   | A   | B-  | B   |
| FP11| Detect Loops, Don't Ride Them               | A   | C   | C   |
| FP12| Fail Once, Learn; Fail Thrice, Stop         | A   | C   | A-  |
| FP13| Don't Re-Read What Hasn't Changed           | A   | A   | A-  |
| FP14| Schema Must Reflect Reality                 | A   | A-  | B   |
| FP15| Trust Boundary at Memory Write              | A   | A   | A-  |
| FP16| Defaults Protect, Overrides Empower         | A   | N/A | N/A |
| FP17| Compression Must Not Poison Read Cache      | A   | A   | B   |
+-----+--------------------------------------------+-----+-----+-----+
|     | OVERALL                                    | A+  | B   | B   |
+-----+--------------------------------------------+-----+-----+-----+
```
