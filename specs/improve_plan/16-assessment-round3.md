# 16 — Round 3 Assessment: EdgeCrab vs Hermes Agent vs Claude Code

> Brutal honest assessment. Code is law. First Principles only.

## Research Summary

### Hermes Agent (Python, ~3000 tests)
Explored `run_agent.py`, `model_tools.py`, `tools/`, `gateway/`, `agent/`.
Found **16 new patterns** not in EdgeCrab at time of analysis.

### Claude Code (TypeScript, internal tooling)
Explored `src/Tool.ts`, `src/tools.ts`, `src/coordinator/`, `src/query/`,
`src/services/compact/`, `src/utils/`, `src/constants/`, `src/hooks/`.
Found **20+ architectural patterns** not in EdgeCrab at time of analysis.

---

## Gap Matrix: What EdgeCrab NOW Has vs Missing

```
+-----+--------------------------------------------+----------+---------+----------+
| #   | Pattern                                    | Hermes   | Claude  | EdgeCrab |
|     |                                            | Agent    | Code    |          |
+-----+--------------------------------------------+----------+---------+----------+
|  1  | Tool output spill to disk                  | YES (3L) | YES     | YES      |
|  2  | Per-turn token tracking (5 buckets)         | YES      | YES     | YES      |
|  3  | Anthropic prompt cache markers              | YES      | YES     | YES      |
|  4  | Structured 8-section compression template   | YES      | N/A     | YES      |
|  5  | Consecutive failure escalation              | NO       | NO      | YES(FP6) |
|  6  | Tool call JSON repair                       | NO       | NO      | YES(FP7) |
|  7  | Schema-aware type coercion                  | NO       | NO      | YES(FP7) |
|  8  | Provider fallback on retryable errors       | PARTIAL  | NO      | YES(FP8) |
|  9  | Path-aware parallel tool dispatch           | NO       | NO      | YES(FP9) |
| 10  | Stale stream inter-chunk timeout            | NO       | NO      | YES(FP10)|
| 11  | Fuzzy tool name matching                    | NO       | NO      | YES      |
| 12  | Error enrichment with schema hints          | NO       | NO      | YES(FP3) |
+-----+--------------------------------------------+----------+---------+----------+
|     |                                            |          |         |          |
|     | === GAPS (EdgeCrab MISSING) ===             |          |         |          |
|     |                                            |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| G1  | Duplicate tool call detection              | NO       | NO      | YES(FP11)|
|     | (identical name+args consecutive turns)     |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| G2  | Path-PREFIX overlap in parallel dispatch    | NO       | NO      | YES(FP9) |
|     | ("src/" + "src/main.rs" must serialize)     |          | (exact) |          |
+-----+--------------------------------------------+----------+---------+----------+
| G3  | Compression circuit breaker                 | NO       | YES(3x) | YES(FP12)|
|     | (stop retrying after N failures)            |          |         |          |
+-----+--------------------------------------------+----------+---------+----------+
| G4  | Deferred tool pool / tool search            | NO       | YES     | NO       |
+-----+--------------------------------------------+----------+---------+----------+
| G5  | Per-tool permission denial tracking         | NO       | YES     | NO       |
+-----+--------------------------------------------+----------+---------+----------+
| G6  | Microcompact (per-turn selective clearing)  | NO       | YES     | NO       |
+-----+--------------------------------------------+----------+---------+----------+
| G7  | MCP dynamic tool re-sync                    | YES      | YES     | NO       |
+-----+--------------------------------------------+----------+---------+----------+
| G8  | Error classification enum                   | YES      | NO      | NO       |
+-----+--------------------------------------------+----------+---------+----------+
| G9  | Rich hooks/event system                     | NO       | YES     | NO       |
+-----+--------------------------------------------+----------+---------+----------+
| G10 | Coordinator/multi-worker mode               | NO       | YES     | NO       |
+-----+--------------------------------------------+----------+---------+----------+
```

## Priority Analysis: What to Implement NOW

Only G1-G3 meet all of:
- **Directly improve agent loop reliability** (not new features)
- **Small, surgical changes** (under 100 LOC each)
- **Testable without external services**
- **Zero breaking changes**

G4-G10 are **feature additions** requiring multi-week effort. They are deferred.

---

## Round 3 Implementation Targets

### Spec 17: Duplicate Tool Call Detection (FP11)

**WHY**: When the LLM emits the exact same tool call (same name + same args)
across consecutive turns, it is stuck in a loop. Each loop iteration burns
budget for zero progress. This is the #2 agent failure mode after consecutive
errors (which FP6 already handles).

**First Principle FP11**: *"Detect loops, don't ride them"*
> If the same tool(name, args_hash) appears in consecutive turns with identical
> results, inject a redirect rather than executing it again.

**Approach**: Track `(tool_name, args_hash)` of the last N tool calls per
session. When a new call matches the previous, inject a system message:
`"You already called {name} with the same arguments and got: {summary}.
Try a different approach."` — skip execution, return cached result.

### Spec 18: Path-Prefix Overlap (FP9 hardening)

**WHY**: `can_parallelize_in_batch` uses exact string match via
`HashSet.contains(path)`. This means `write_file("src/")` and
`write_file("src/main.rs")` are considered non-overlapping and dispatched
in parallel. One is a parent of the other — they MUST serialize.

**Approach**: Replace `claimed_paths.contains(path)` with prefix-aware
overlap check: `claimed ⊂ new || new ⊂ claimed`.

### Spec 19: Compression Circuit Breaker (FP12)

**WHY**: If LLM-based compression fails (model error, timeout, format issue),
the structural fallback runs. But on next iteration, it tries LLM again.
If the LLM keeps failing, we burn tokens on compression attempts.
Claude Code caps this at 3 consecutive failures.

**First Principle FP12**: *"Fail once, learn; fail thrice, stop"*
> After 3 consecutive compression failures, disable LLM compression for the
> rest of the session. Use structural fallback only.

---

## First Principles Stack Update

```
+----------------------------------------------------------------------+
|                     FIRST PRINCIPLES STACK (Round 3)                 |
+----------------------------------------------------------------------+
|  FP1-FP10: [Round 1+2 — all implemented, see specs 01-15]           |
|                                                                      |
|  FP11: Detect Loops, Don't Ride Them  [Round 3]                     |
|        "Same tool+args twice = agent stuck, not progress"            |
|        => Duplicate tool call detection + cached result return       |
|                                                                      |
|  FP12: Fail Once, Learn; Fail Thrice, Stop  [Round 3]               |
|        "Repeated compression failures waste budget for zero gain"    |
|        => Compression circuit breaker after 3 consecutive failures   |
|                                                                      |
+----------------------------------------------------------------------+
```

## Scorecard (Post Round 3)

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
+-----+--------------------------------------------+-----+-----+-----+
|     | OVERALL                                    | A   | B-  | B   |
+-----+--------------------------------------------+-----+-----+-----+
```

EC = EdgeCrab, HA = Hermes Agent, CC = Claude Code

---

## Implementation Status (Round 3 Complete)

| Gap | Implementation | File | Tests | Status |
|-----|---------------|------|-------|--------|
| G1 | `DuplicateToolCallDetector` struct (~55 LOC) | `conversation.rs` | 5 unit tests | DONE |
| G2 | `paths_overlap()` prefix-aware check (~40 LOC) | `registry.rs` | 7 unit tests | DONE |
| G3 | Compression circuit breaker + `compress_structural_only()` (~35 LOC) | `conversation.rs` + `compression.rs` | 3 unit tests | DONE |

**Total new tests**: 15 (dedup: 5, paths: 7, compression: 3)
**Total workspace tests**: 2,438+ passing, 0 failures
**Clippy**: Clean on modified crates (pre-existing warnings in `edgecrab-state` unrelated)

### Key Architectural Decisions

1. **Dedup tracker uses 2-turn sliding window** — `prev_turn` + `current_turn` HashMaps
   rotated on `end_turn()`. Detects same-tool-same-args across consecutive turns without
   unbounded memory growth.

2. **Path overlap uses normalized prefix comparison** — strips trailing slashes, then checks
   both `new_path.starts_with(claimed/)` and `claimed.starts_with(new_path/)`. Prevents
   false positives like `"src"` matching `"src2"`.

3. **Circuit breaker falls back to structural compression** — after 3 consecutive LLM
   compression failures, `compress_structural_only()` runs phases 1,2,5,6 of the normal
   pipeline but uses `build_summary()` (statistical) instead of `llm_summarize()`. No API
   call, deterministic, always succeeds.
