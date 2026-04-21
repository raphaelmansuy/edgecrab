# EdgeCrab Improvement Plan — First Principles Redesign

Cross-reference index for the improvement plan documents.

## Document Map

| # | Document | Focus | Status |
|---|----------|-------|--------|
| 1 | [01-diagnosis.md](01-diagnosis.md) | Root-cause analysis — WHY the agent fails | Reference |
| 2 | [02-hermes-patterns.md](02-hermes-patterns.md) | Hermes Agent patterns to adopt (cross-ref) | Reference |
| 3 | [03-tool-reduction.md](03-tool-reduction.md) | P0: Reduce CORE_TOOLS from 173 to ~40 | Implement |
| 4 | [04-error-guidance.md](04-error-guidance.md) | P1: Actionable error messages | Implement |
| 5 | [05-failure-escalation.md](05-failure-escalation.md) | P2: Consecutive failure → user escalation | Implement |
| 6 | [06-write-file-fix.md](06-write-file-fix.md) | P2: Remove content:null anti-pattern | Implement |
| 7 | [07-terminal-guard.md](07-terminal-guard.md) | P3: Terminal anti-pattern interception | Implement |
| 8 | [08-suppression-messages.md](08-suppression-messages.md) | P3: Actionable suppression feedback | Implement |
| 9 | [09-assessment-round2.md](09-assessment-round2.md) | Round 2: Brutal honest assessment vs Hermes + Claude Code | Reference |
| 10 | [10-tool-call-repair.md](10-tool-call-repair.md) | P0: Self-heal malformed tool call JSON | Implement |
| 11 | [11-type-coercion.md](11-type-coercion.md) | P1: Schema-aware type coercion | Implement |
| 12 | [12-provider-fallback.md](12-provider-fallback.md) | P1: Provider fallback on retryable errors | Implement |
| 13 | [13-parallel-tool-safety.md](13-parallel-tool-safety.md) | P2: Path-aware parallel tool dispatch | Implement |
| 14 | [14-stale-stream-detect.md](14-stale-stream-detect.md) | P1: Inter-chunk stale stream timeout | Implement |
| 15 | [15-final-assessment-round2.md](15-final-assessment-round2.md) | Final: First Principles scorecard vs Hermes + Claude Code | Reference |
| 16 | [16-assessment-round3.md](16-assessment-round3.md) | Round 3: Deep research gap matrix + FP11/FP12 | Reference |
| 17 | [17-assessment-round4.md](17-assessment-round4.md) | Round 4: Brutal honest "Code Is Law" audit + FP13-FP17 | Reference |
| 18 | [18-read-dedup-loop.md](18-read-dedup-loop.md) | FP13: Read dedup + consecutive loop detection ✅ | Implemented |
| 19 | [19-schema-cross-ref.md](19-schema-cross-ref.md) | FP14: Dynamic schema cross-ref filtering ✅ | Implemented |
| 20 | [20-memory-injection-scan.md](20-memory-injection-scan.md) | FP15: Memory write injection scanning ✅ | Implemented |
| 21 | [21-configurable-write-limit.md](21-configurable-write-limit.md) | FP16+FP17: Configurable write limit + compression dedup reset ✅ | Implemented |
| 22 | [22-assessment-round5.md](22-assessment-round5.md) | Round 5: Screenshot bugs + FP18/FP19 rate-limit + error surface | Reference |
| 23 | [23-assessment-round6.md](23-assessment-round6.md) | Round 6: FP20 (N/A Rust) + FP21: Budget warning history purge ✅ | Implemented |
| 24 | [24-assessment-round7.md](24-assessment-round7.md) | Round 7: FP22-FP28 prompt pipeline + injection scanner + model guidance ✅ | Implemented |
| 25 | [25-assessment-round8.md](25-assessment-round8.md) | Round 8 | Reference |
| 26 | [26-assessment-round9.md](26-assessment-round9.md) | Round 9 | Reference |
| 27 | [27-assessment-round10.md](27-assessment-round10.md) | Round 10 | Reference |
| 28 | [28-assessment-round11.md](28-assessment-round11.md) | Round 11 | Reference |
| 29 | [29-assessment-round12.md](29-assessment-round12.md) | Round 12 | Reference |
| 30 | [30-assessment-round13.md](30-assessment-round13.md) | Round 13: FP51-FP54 — write_file preview, secret redaction | Implemented |
| 31 | [31-harness-deep-comparison.md](31-harness-deep-comparison.md) | Round 14 prep: J1–J7 deep harness comparison vs Hermes + Claude Code | Reference |
| 32 | [32-write-file-fp55.md](32-write-file-fp55.md) | FP55: `if_exists` enum + snapshot-on-reject + 600-byte preview ✅ | Implemented |
| 33 | [33-assessment-round14.md](33-assessment-round14.md) | Round 14: brutal re-assessment after FP55 implementation | Reference |

## First Principles Applied

```
+----------------------------------------------------------------------+
|                     FIRST PRINCIPLES STACK                           |
+----------------------------------------------------------------------+
|                                                                      |
|  FP1: Minimum Viable Toolset                                        |
|       "Every extra tool is noise that degrades LLM accuracy"         |
|       => 173 tools -> ~40 tools (match Hermes Agent)                 |
|                                                                      |
|  FP2: Make The Right Thing Easy, Wrong Thing Impossible              |
|       "Schema > prose instructions — LLM follows schema"             |
|       => content:null removed, required fields enforced              |
|                                                                      |
|  FP3: Every Error Must Contain Its Own Fix                           |
|       "Error + corrective example = self-healing loop"               |
|       => required_fields, schema_hint in every InvalidArgs           |
|                                                                      |
|  FP4: Feedback Loops Must Close                                      |
|       "Correction path shorter than error path"                      |
|       => Suppression messages include original error + diff           |
|                                                                      |
|  FP5: Escape Hatch != Easy Path                                     |
|       "terminal should not replace read_file/write_file"             |
|       => Anti-pattern detection in terminal execute()                |
|                                                                      |
|  FP6: Escalate When Stuck, Don't Burn Budget                        |
|       "3 consecutive failures => ask user"                           |
|       => ConsecutiveFailureTracker in conversation loop              |
|                                                                      |
|  FP7: Self-Heal Before Failing  [Round 2]                           |
|       "Repair malformed args locally — never waste an API turn"      |
|       => repair_tool_call_arguments() + coerce_tool_args()           |
|                                                                      |
|  FP8: Resilience Through Redundancy  [Round 2]                      |
|       "Single-provider agent = single point of failure"              |
|       => fallback_model in config + one-shot provider switch         |
|                                                                      |
|  FP9: Parallel Safety Is Not Optional  [Round 2]                    |
|       "Binary flags are too coarse — need path-aware detection"      |
|       => path_arguments() trait method + overlap detection           |
|                                                                      |
|  FP10: Timeouts Are Not Optional  [Round 2]                         |
|        "Every network op must have a bounded wall-clock timeout"     |
|        => STREAM_INTER_CHUNK_TIMEOUT in streaming loop               |
|                                                                      |
|  FP11: Detect Loops, Don't Ride Them  [Round 3]                     |
|        "Same tool+args twice in a row = infinite loop signal"        |
|        => DuplicateToolCallDetector in execute_loop                  |
|                                                                      |
|  FP12: Fail Once, Learn; Fail Thrice, Stop  [Round 3]               |
|        "LLM compression can fail — don't retry forever"              |
|        => Compression circuit breaker (3 failures -> structural)     |
|                                                                      |
|  FP13: Don't Re-Read What Hasn't Changed  [Round 4]                 |
|        "Unchanged file = wasted context tokens"                      |
|        => mtime-based dedup + consecutive read loop detection        |
|                                                                      |
|  FP14: Schema Must Reflect Reality  [Round 4]                       |
|        "Tool descriptions referencing unavailable tools = hallucin"   |
|        => Strip cross-refs when referenced tool is disabled          |
|                                                                      |
|  FP15: Trust Boundary at Memory Write  [Round 4]                    |
|        "Memory is injected into system prompt = injection surface"    |
|        => Scan content before persist, block exfil/inject patterns   |
|                                                                      |
|  FP16: Defaults Protect, Overrides Empower  [Round 4]               |
|        "Hard limits that can't be overridden punish power users"     |
|        => Configurable write limit with safe 32 KiB default          |
|                                                                      |
|  FP17: Compression Must Not Poison Read Cache  [Round 4]            |
|        "After compression, cached reads point to removed context"    |
|        => Reset read tracker snapshots after compression event       |
|                                                                      |
|  FP18: Single-Line Errors Surface Truth  [Round 5]                  |
|        "A truncated error is a silent error"                         |
|        => format_patch_errors() joins multi-error to single line     |
|        => TUI activity feed shows first line — errors must fit       |
|                                                                      |
|  FP19: Retry Delay = Provider Guidance, Not Guess  [Round 5]        |
|        "If provider says wait 1.197s, wait 1.2s not 500ms"          |
|        => parse_retry_after() parses rate-limit error messages       |
|        => Uses provider-stated delay + 200ms safety margin           |
|                                                                      |
|  FP20: N/A — Rust guarantees valid UTF-8 strings  [Round 6]         |
|        "Lone surrogates can't exist in Rust String (type-system)"    |
|        => serde_json rejects invalid UTF-8 at parse time             |
|        => Python-only concern; Rust type system handles this for free |
|                                                                      |
|  FP21: Budget Warnings Are Turn-Scoped Signals  [Round 6]           |
|        "Stale pressure from turn 63 should not pollute turn 70"      |
|        => strip_budget_warnings_from_history() before each API call  |
|        => Strips JSON key, plain text suffix, standalone user msg     |
|        => 8 unit tests; cross-ref Hermes _strip_budget_warnings_…   |
|                                                                      |
|  FP22: Non-Anthropic Models Need Explicit Tool Mandates  [Round 7]  |
|        "GPT/Gemini/Grok narrate instead of acting without nudge"     |
|        => TOOL_USE_ENFORCEMENT_GUIDANCE injected for non-Claude      |
|        => Safe default: inject when model is unknown/empty           |
|                                                                      |
|  FP23: Model-Specific Guidance Beats Generic Prompt  [Round 7]      |
|        "OpenAI o-series and Gemini have distinct failure modes"      |
|        => OPENAI_MODEL_EXECUTION_GUIDANCE (XML) for GPT/codex/o*    |
|        => GOOGLE_MODEL_OPERATIONAL_GUIDANCE for gemini/gemma         |
|                                                                      |
|  FP24: Platform-Aware Cache Keys Prevent Skill Bleed  [Round 7]     |
|        "CLI skill set != Telegram skill set → must not share cache"  |
|        => Skills cache key = (PathBuf, platform_str) not PathBuf     |
|        => Soft LRU cap at 16 entries — oldest evicted first          |
|                                                                      |
|  FP25: Injection Patterns Require Regex, Not Substr  [Round 7]      |
|        "substr('ignore previous') misses IGNORE  PREVIOUS, camel"   |
|        => OnceLock<Vec<Regex>> — compiled once, 12 patterns (+4 new)|
|        => New: hidden_div, translate_execute, exfil_curl, read_secrets|
|                                                                      |
|  FP26: Session Context Must Be Self-Describing  [Round 7]           |
|        "Timestamp alone is not enough for cross-session debugging"   |
|        => Rich timestamp: date + Session ID + Model + Provider       |
|        => Empty fields omitted — no noise when not set               |
|                                                                      |
|  FP27: Skills Prompt Must Signal Its Mandatory Nature  [Round 7]    |
|        "Optional-looking skill block is silently skipped by LLM"    |
|        => <available_skills> XML + '## Skills (mandatory)' header   |
|        => scan-before-reply directive makes the obligation explicit   |
|                                                                      |
|  FP28: Truncation Markers Must Enable Recovery  [Round 7]           |
|        "[...truncated]" gives no actionable info to LLM or dev"     |
|        => Marker includes filename + char counts + file-tool hint    |
|        => LLM can issue targeted file_read to get missing content    |
|                                                                      |
+----------------------------------------------------------------------+
```

## SOLID Principles Applied

| Principle | Application |
|-----------|-------------|
| **S**RP | Each tool does one thing. `write_file` writes, `touch_file` scaffolds |
| **O**CP | New toolsets added via `expand_alias()` without modifying CORE_TOOLS |
| **L**SP | All ToolHandler impls return same error contract (ToolErrorResponse) |
| **I**SP | LLM sees only tools it needs (lazy loading by toolset) |
| **D**IP | Conversation loop depends on ToolHandler trait, not concrete tools |

## DRY Violations Fixed

| Before | After | Location |
|--------|-------|----------|
| CORE_TOOLS duplicates ACP_TOOLS (90%+ overlap) | ACP_TOOLS = CORE_TOOLS minus exclusion list | toolsets.rs |
| `suggested_action` built in 3 places | `build_suggested_action()` centralizer | error.rs |
| Suppression message template duplicated | `suppressed_retry_response()` with shared format | conversation.rs |
