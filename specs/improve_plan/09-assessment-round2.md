# Round 2 — Brutal Honest Assessment: EdgeCrab vs Hermes Agent vs Claude Code

## Verdict

EdgeCrab v0.7.0 (post-Round-1) is a **solid 6.5/10** agent. Round 1 fixed the
loudest problems (tool count, error enrichment, failure escalation, suppression
messages). But comparing to Hermes Agent 0.10 and Claude Code reveals **five
critical gaps** where EdgeCrab silently degrades or crashes when it shouldn't.

```
+----------------------------------------------------------------------+
|              COMPARATIVE GAP MATRIX (post Round 1)                   |
+----------------------------------------------------------------------+
| Dimension             | Hermes | Claude Code | EdgeCrab | Gap       |
|-----------------------|--------|-------------|----------|-----------|
| Tool-name repair      |  YES   |    YES      |   NO     | CRITICAL  |
| Arg type coercion     |  YES   |    YES      |   NO     | HIGH      |
| Provider fallback     |  YES   |    YES      |   YES    | DONE      |
| Parallel overlap det  |  YES   |    YES      |  PARTIAL | MEDIUM    |
| Stale stream detect   |  YES   |    YES      |   NO     | HIGH      |
| Fuzzy tool match      |  YES   |    YES      |   YES    | DONE      |
| Consecutive fail esc  |  YES   |    N/A      |   YES    | DONE (R1) |
| Error enrichment      |  YES   |    YES      |   YES    | DONE (R1) |
| Tool count reduction  |  YES   |    YES      |   YES    | DONE (R1) |
| Suppression msgs      |  YES   |    N/A      |   YES    | DONE (R1) |
| Terminal guards       |  N/A   |    N/A      |   YES    | DONE (R1) |
| Delegate capping      |  YES   |    N/A      |   YES    | DONE      |
+----------------------------------------------------------------------+
```

---

## Gap 1: Tool Call Argument Repair (CRITICAL)

**What Hermes does:**
`_repair_tool_call_arguments()` repairs malformed JSON from models:
- Empty/null → `"{}"`
- Python `None`/`True`/`False` → JSON equivalents
- Truncated JSON → closes brackets heuristically
- Trailing commas → strips

**What Claude Code does:**
Zod schema validation with `z.coerce` for type mismatches; tool_use/tool_result
pairing repair in `claude.ts` line 1298+.

**What EdgeCrab does:**
```rust
let args: serde_json::Value = match serde_json::from_str(args_json) {
    Ok(v) => v,
    Err(e) => { return ToolError::InvalidArgs ... }  // HARD FAIL
};
```
Zero repair. Any malformed JSON kills the tool call. Models like GLM-5, Ollama
local, and DeepSeek sometimes emit trailing commas or Python-style booleans.
This burns 1-2 API calls ($0.10-0.50) every time.

**First Principle: FP7 — Self-Heal Before Failing.**
The cheapest fix is the one that never reaches the LLM.

---

## Gap 2: Argument Type Coercion (HIGH)

**What Hermes does:**
`coerce_tool_args()` compares actual args against JSON Schema:
- String `"42"` → integer `42`
- String `"true"` → boolean `true`
- Union types handled

**What Claude Code does:**
`z.coerce.boolean()`, `z.coerce.string()` patterns in Zod schemas.

**What EdgeCrab does:**
`serde_json::from_value::<MyArgs>(args)` — if the LLM sends `"42"` when the
schema says integer, deserialization fails → `InvalidArgs` → wasted turn.

**First Principle: FP2 — Make The Right Thing Easy.**
If the schema says `integer` and the LLM sends `"42"`, that's the right value
in the wrong type. Coerce it silently.

---

## Gap 3: Provider Fallback Chain (HIGH)

**What Hermes does:**
`_provider_fallback_chain` — ordered list of backup providers. On rate-limit/
overload/connection failure: try next provider. Per-turn primary restoration.
Credential rotation on auth errors.

**What Claude Code does:**
`withRetry()` generator with `FallbackTriggeredError`, `fallbackModel` param,
`consecutive529Errors` tracking, exponential backoff.

**What EdgeCrab does:**
Simple retry loop with fixed backoff:
```rust
const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_millis(500);
```
No fallback to a cheaper/different provider. If the primary is rate-limited or
overloaded, all 3 retries hit the same wall.

**First Principle: FP8 — Resilience Through Redundancy.**
A single-provider agent is a single point of failure.

---

## Gap 4: Parallel Tool Overlap Detection (MEDIUM)

**What Hermes does:**
`_should_parallelize_tool_batch()`:
- Never-parallel set (terminal, write_file, patch)
- Path-scoped overlap detection (two tools targeting same file → sequential)
- Parallel-safe whitelist

**What EdgeCrab does:**
`is_parallel_safe()` on ToolHandler trait — binary flag per tool type.
No path-overlap detection. If the LLM calls `write_file("a.txt", ...)` and
`write_file("b.txt", ...)`, both run in parallel (fine). But if both target
`"a.txt"` — data race.

**First Principle: FP9 — Parallel Safety Is Not Optional.**
The current binary flag is too coarse. Needs path-aware conflict detection.

---

## Gap 5: Stale Stream Detection (HIGH)

**What Hermes does:**
Wall-clock timer on last chunk received. If no data for configurable timeout
(scales with context size), kills connection and retries.

**What Claude Code does:**
`getNonstreamingFallbackTimeoutMs()` — configurable per-request timeout,
non-streaming fallback on stream hang, analytics instrumentation.

**What EdgeCrab does:**
`STREAM_FIRST_CHUNK_TIMEOUT = 20s` — timeout only for first chunk. After
that, no stale detection. A hung connection holds the loop forever.

**First Principle: FP10 — Timeouts Are Not Optional.**
Every network operation must have a bounded wall-clock timeout.

---

## What NOT To Do (Lessons from Both Codebases)

| Pattern | Why Skip |
|---------|----------|
| Hermes `_LEGACY_TOOLSET_MAP` | Tech debt — EdgeCrab has no legacy users |
| Hermes `skill_nudge` interval | Edge already has SKILLS_GUIDANCE + reflection |
| Claude Code `deferLoading` | Over-engineered for EdgeCrab's tool count |
| Claude Code `coordinatorMode` | Enterprise feature — scope creep |
| Hermes `steer()` mechanism | Niche — interruptible loops cover this |
