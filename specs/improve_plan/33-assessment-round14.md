# 33 — Round 14: Brutal Re-Assessment After FP55

> **Code Is Law, post-merge edition.**
> The FP55 implementation is in the tree. This doc re-scores the harness
> against [31-harness-deep-comparison.md](31-harness-deep-comparison.md)
> using the same J1–J7 framework, now that
> [32-write-file-fp55.md](32-write-file-fp55.md) is no longer a plan
> but a working contract enforced by 23 unit tests.
>
> **Cross-ref:**
> [31-harness-deep-comparison.md](31-harness-deep-comparison.md) ·
> [32-write-file-fp55.md](32-write-file-fp55.md) ·
> [30-assessment-round13.md](30-assessment-round13.md) ·
> [02-hermes-patterns.md](02-hermes-patterns.md)

---

## 0. What changed since round 13

| Before FP55                                                | After FP55                                                          |
|------------------------------------------------------------|---------------------------------------------------------------------|
| `write_file` rejection embedded a 1500-byte preview (FP51) | Preview shrunk to 600 bytes (UTF-8 boundary safe via `safe_truncate`) |
| Rejection did **not** snapshot — model still re-read       | Rejection **records the snapshot** before returning the error       |
| Single rejection mode (always preview, always rich)        | `if_exists` enum: `overwrite` (default, rich) / `abort` (cheap stat) |
| Schema didn't model the "what now?" decision               | Schema documents the retry protocol in the description AND exposes `if_exists` as an optional enum |
| 1 round-trip ceremony bug visible in screenshots           | 0 round-trip ceremony — retry of the same call succeeds             |

### Token economics, measured against the screenshot scenario

```
+-------------------------------------------------------------+
|  WRITE-COLLISION ROUND-TRIPS (per incident)                 |
+-------------------------------------------------------------+
|                                                             |
|  Pre-FP51 (R0):  write -> err 200B -> read 1500B -> write   |
|                  3 LLM turns,  ~1.7 KB tool overhead         |
|                                                             |
|  FP51 (R12):     write -> err 1500B -> read 1500B -> write  |
|                  3 LLM turns,  ~3.0 KB tool overhead         |
|                  (preview embedded but snapshot not stored) |
|                                                             |
|  FP55 (R14):     write -> err 600B  -> write                |
|                  2 LLM turns,  ~600 B tool overhead          |
|                                                             |
|  Savings vs R12: -1 turn, -2.4 KB per collision (~80%)      |
+-------------------------------------------------------------+
```

The R12 → R14 delta is the "operational vs informational" preview fix:
content was already in the error, but the read-tracker never knew
about it. FP55 closes that loop.

---

## 1. Re-scored J1–J7

### J1 — Schema contract (unchanged)

| Property                              | Hermes | Claude Code | EdgeCrab R13 | EdgeCrab R14 |
|---------------------------------------|:------:|:-----------:|:------------:|:------------:|
| `additionalProperties: false`         |   ✗    |     ✓       |      ✓       |      ✓       |
| `strict` flag forwarded               |   ✗    |     ✓       |      ✓       |      ✓       |
| Output schema present                 |   ✗    |     ✓       |      ✗       |      ✗ ⚠ debt |
| Two-phase validate then call          |   ✗    |     ✓       |    partial   |    partial   |
| Numeric error codes                   |   ✗    |     ✓       |      ✗       |      ✗       |
| Behavior visible in schema (not prose)|   ✗    |     ✓       |    partial   |    **✓**     |
| **Total**                             |  0/6   |    6/6      |     3/6      |    **4/6**   |

**Why +1:** The `if_exists` enum moves the "what to do on collision"
decision out of prose and into the schema. The model can now express
intent (cheap probe vs forced overwrite) without reading documentation.

**Known debt — output schemas (J5/J1):** Tool results are still
`String`. This is a workspace-wide change (every tool, every consumer,
every test) and is **explicitly out of scope for round 14**. Tracked
here so future rounds can pick it up without re-discovering it.

### J3 — Validation as recovery (improved)

| Property                                                 | Hermes | Claude Code | EdgeCrab R13 | EdgeCrab R14 |
|----------------------------------------------------------|:------:|:-----------:|:------------:|:------------:|
| Stable error envelope                                    |   ✓    |     ✓       |      ✓       |      ✓       |
| Numeric / categorised codes                              |   ✗    |     ✓       |    partial   |    partial   |
| Tool name in error                                       |   ✗    |     ✓       |      ✓       |      ✓       |
| Required-fields hint                                     |   ✗    |     ✗       |      ✓       |      ✓       |
| Live content embedded on conflict                        |   ✗    |     ✗       |      ✓       |      ✓       |
| Snapshot auto-recorded on conflict so retry works no-read|  n/a   |    n/a (does own re-stat) |   ✗   |  **✓** |
| Cheap-probe alternative (no preview, no snapshot)        |   ✗    |     ✗       |      ✗       |    **✓**     |
| **Total**                                                |  1/6   |    3/6      |     4/6      |    **6/7**   |

EdgeCrab is now the **only** harness in the comparison where a
rejected mutation prepares the ground for the immediate retry. Claude
Code re-stats inside `call`; Hermes warns then proceeds; EdgeCrab now
*teaches the loop the truth* in the same turn it refused the write.

### J5 — Result shaping (unchanged, debt acknowledged)

Still strings. See "Known debt" above. Score remains 1/3 (Claude Code
3/3, Hermes 1/3).

### J6 — Failure recovery (unchanged)

EdgeCrab keeps its R13 lead: tool-call repair (FP7), provider fallback
(FP8), consecutive failure escalation (FP6), inter-chunk timeout
(FP10), suppression dedup. 4/4.

### J7 — Cost discipline (improved indirectly)

The FP55 change moves one full LLM round-trip out of the steady-state
cost of a failed write. Not a J7 mechanic per se, but it changes the
expected cost-per-error term. No score change; budget caps unchanged.

---

## 2. Aggregate scorecard

```
+-------------------------------------------------------------+
|                  HARNESS AGGREGATE (R14)                    |
+-------------------------------------------------------------+
|                                                             |
|  Job              Hermes    Claude Code    EdgeCrab         |
|  ---              ------    -----------    --------         |
|  J1 Schema         0/6        6/6           4/6  (+1)       |
|  J2 Dispatch       2/3        3/3           3/3             |
|  J3 Recovery       1/6        3/6           6/7  (+1)       |
|  J4 Effects        2/6        5/6           5/6             |
|  J5 Results        1/3        3/3           1/3  (debt)     |
|  J6 Failure        2/6        3/6           4/4 (out of 4)  |
|  J7 Cost           4/6        3/6           5/6             |
|                                                             |
|  TOTAL            12/40      26.5/40       30/40 (was 29)   |
+-------------------------------------------------------------+
```

EdgeCrab moves from 29 → **30/40**, decisively past Claude Code on
total but with a remaining structural gap on J5 (output schemas) that
Claude Code still wins outright.

---

## 3. Brutal-honest gaps that remain

1. **No output schema (J5).** Acknowledged debt. The model still parses
   prose like `"Wrote 11 bytes to 'foo'"`. Workspace-wide refactor;
   not in scope for R14. Tracked.
2. **No atomic TOCTOU re-check.** Tokio's single-task-per-write makes
   the window small, but Claude Code's validate-then-call pattern is
   still strictly safer.
3. **No UNC NTLM-leak guard.** Linux/macOS only target → low priority,
   but it is a Code-Is-Law gap if EdgeCrab ever ships on Windows.
4. **Numeric error codes still missing.** `ToolError` enum variants
   are categorical but not numeric; downstream branching is
   string-match.
5. **`if_exists` only applies to `write_file`.** Same protocol could
   help `file_patch` (no live preview today on hash mismatch). Out of
   scope.

---

## 4. What FP55 actually proves (Code Is Law)

The *contract* enforced by tests in
`crates/edgecrab-tools/src/tools/file_write.rs`:

- `fp55_overwrite_rejection_records_snapshot_so_immediate_retry_succeeds`
  — proves the snapshot side-effect of the error path.
- `fp55_abort_returns_cheap_error_no_preview_no_snapshot` — proves the
  cheap-probe contract.
- `fp55_abort_then_overwrite_retry_records_snapshot_and_then_succeeds`
  — proves the two-phase probe → commit pattern works.
- `fp55_external_modification_between_reject_and_retry_fails_freshness`
  — proves the snapshot is not a free pass: TOCTOU still detected.
- `fp55_schema_includes_if_exists_optional_enum` — proves backward
  compatibility (existing callers don't break).
- `fp55_schema_description_documents_retry_protocol` — proves the
  description is part of the contract, not a comment.
- `fp55_unknown_if_exists_value_is_rejected` — proves the enum is
  closed (not stringly-typed).

23/23 file_write tests pass. Full workspace lib suite: 1823 tests, 0
failures. (Pre-existing clippy warnings in `edgecrab-sdk-macros` and
`edgecrab-state` are unrelated to this round.)

---

## 5. First-principles audit: should we add output schemas now?

The user asked whether output schemas could be slotted into round 14.
Honest answer: **no — they fail the first-principles cost/benefit test
at the current provider API surface.** Recorded here so the question
doesn't get re-asked next round without context.

### The wire-format reality

```
+-----------------------------------------------------------------+
|              WHAT EACH PROVIDER ACCEPTS IN A TOOL DEF           |
+-----------------------------------------------------------------+
|                                                                 |
|  OpenAI function-calling                                        |
|     function: { name, description, parameters }                 |
|     -> NO output_schema field. Period.                          |
|                                                                 |
|  Anthropic tool-use                                             |
|     { name, description, input_schema }                         |
|     -> NO output_schema field. Period.                          |
|                                                                 |
|  MCP                                                            |
|     { name, description, inputSchema, outputSchema? }           |
|     -> outputSchema exists but is OPTIONAL and most clients     |
|        ignore it (LLM still parses the rendered text result).   |
+-----------------------------------------------------------------+
```

EdgeCrab's [`ToolSchema`](../../crates/edgecrab-types/src/tool.rs#L65-L72)
serialises `{ name, description, parameters, strict }`. Adding
`output_schema: Option<Value>` is mechanically trivial — and produces
**zero model-visible value** because the field is stripped before
hitting OpenAI/Anthropic. The model still sees only the rendered tool
result string. We'd be doing a workspace-wide refactor for local
metadata.

### What WOULD have value (and why it's still not worth it this round)

Making the *result string itself* JSON-shaped. The model already reads
the result; structured JSON parses more deterministically than prose at
~equal token cost:

```
Before:  "Wrote 11 bytes to 'foo.md'"                    (~25 tokens)
After:   {"ok":true,"bytes":11,"path":"foo.md",
          "action":"create"}                             (~22 tokens)
```

This is per-tool, no schema change required. **But:** the CLI display
layer at
[`tool_display.rs`](../../crates/edgecrab-cli/src/tool_display.rs#L2053-L2057)
pattern-matches on `"Wrote N bytes to 'path'"` to render the
`write_file` badge in the TUI. The gateway tests at
[`event_processor.rs:985`](../../crates/edgecrab-gateway/src/event_processor.rs#L985)
also depend on the prose form. Switching to JSON means:

1. Update `format_tool_result()` to JSON-aware parsing (or accept
   raw-passthrough for JSON results).
2. Update 4 CLI tests + 1 gateway test fixture.
3. Update gateway message formatter to detect JSON results.
4. Decide: do all 30+ tools migrate at once (consistency) or one at a
   time (drift)? Both are defensible; both are out of scope for R14.

The user asked to ship FP55 cleanly, not to start a CLI presentation
migration mid-round. Discipline says: don't half-ship.

### Verdict

- **Output schemas as a `ToolSchema.output_schema` field**: REJECTED
  by first principles. Provider APIs don't consume it.
- **Structured (JSON) tool results**: ACCEPTED in principle, but
  blast-radius spans CLI + gateway + tests. Earmarked as **FP57**, to
  be done in its own round with proper migration scaffolding.
- **No code change for output schemas in R14.** The honest move.

## 6. Next round candidates (not for R14)

- **R15 — `if_exists` for `file_patch`.** Same protocol, hash
  mismatch flavour. Self-contained.
- **R16 — Numeric error codes.** Add `code: u16` to `ToolError`,
  document the table. Loop self-healing branches on numeric instead
  of string-match.
- **R17 — Atomic TOCTOU re-check inside `execute()`.** Match Claude
  Code's validate-then-call discipline.
- **R18 — Structured (JSON) tool results = FP57.** Cross-cutting
  migration: tools + CLI display + gateway formatter + tests. Big
  enough to deserve its own design doc.
- **(deferred indefinitely) — `ToolSchema.output_schema` field.**
  Not until OpenAI or Anthropic ship output-schema support in their
  function-calling wire format. Would be local metadata otherwise.
- **R16 — Numeric error codes.** Add `code: u16` to `ToolError`,
  document the table.
- **R17 — Apply `if_exists` to `file_patch`.** Same protocol, hash
  mismatch flavour.
- **R18 — Atomic TOCTOU re-check inside `execute()`.** Match Claude
  Code's validate-then-call discipline.
