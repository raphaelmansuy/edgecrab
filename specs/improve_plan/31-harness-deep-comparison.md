# 31 — Deep Harness Comparison: Hermes vs Claude Code vs EdgeCrab

> **Brutal-honest, first-principles, "Code Is Law" assessment.**
> The contract a tool exposes to the model *is* its spec — if behavior
> isn't in the schema or in the prompt, it doesn't exist for the LLM.
>
> **Cross-ref:** [02-hermes-patterns.md](02-hermes-patterns.md) ·
> [09-assessment-round2.md](09-assessment-round2.md) ·
> [15-final-assessment-round2.md](15-final-assessment-round2.md) ·
> [16-assessment-round3.md](16-assessment-round3.md) ·
> [17-assessment-round4.md](17-assessment-round4.md) ·
> [29-assessment-round12.md](29-assessment-round12.md)
> · [30-assessment-round13.md](30-assessment-round13.md)

## 0. Why redo the comparison?

The previous rounds (R2–R13) compared individual *features*. This round
compares the **harness shape** — the runtime substrate that mediates
tool calls. From first principles, the harness has 7 jobs:

```
+-------------------------------------------------------------+
|                THE 7 JOBS OF AN AGENT HARNESS               |
+-------------------------------------------------------------+
| J1  Schema contract     What the model is told tools are    |
| J2  Dispatch            Routing call -> handler             |
| J3  Validation          Reject malformed args, return fix   |
| J4  Effect mediation    Path jail, freshness, rate limits   |
| J5  Result shaping      What goes back into the loop        |
| J6  Failure recovery    Self-heal, escalate, abort          |
| J7  Cost discipline     Token + iteration + wall-clock caps |
+-------------------------------------------------------------+
```

Each tool carries a J1..J5 contract; the loop owns J6, J7.
A "good" harness makes the *cheap* path always the *correct* path
(FP2 — make the right thing easy, the wrong thing impossible).

---

## 1. The three subjects

| Aspect             | Hermes (`hermes-agent`) | Claude Code (`claude-code-analysis`) | EdgeCrab |
|--------------------|--------------------------|----------------------------------------|----------|
| Language           | Python (sync agent loop) | TypeScript (Ink + React + Zod)        | Rust (async tokio + ratatui) |
| Model interface    | OpenAI-compatible v1      | Anthropic Messages SDK + LSP-aware    | OpenAI-compatible + Anthropic streaming |
| Tool schema source | dict literals             | `lazySchema(z.strictObject())`        | `ToolSchema { strict: Some(true) }` + `inventory!` |
| Tool registry     | `tools/registry.py`       | `tools.ts` factory + `buildTool()`    | `crates/edgecrab-tools/src/registry.rs` |
| Permission model  | env-var gating + path jail | `checkWritePermissionForTool` + per-call decision | session read-tracker + path jail + checkpoint |
| State store       | SQLite WAL + FTS5         | in-memory `readFileState` Map         | SQLite WAL + FTS5 + `read_tracker::DashMap` |
| Editor surface    | TUI (Rich + prompt_toolkit) | Ink (React-in-terminal)             | ratatui + skin engine |

All three are mature ReAct loops. They diverge in **how strictly the
schema is treated as the spec**.

---

## 2. J1 — Schema contract: who is most "Code Is Law"?

### Hermes — schemas are advisory dicts

```python
WRITE_FILE_SCHEMA = {
  "name": "write_file",
  "description": "Write content to a file, completely replacing existing content...",
  "parameters": {
    "type": "object",
    "properties": {
      "path":    {"type": "string"},
      "content": {"type": "string"},
    },
    "required": ["path", "content"],
  },
}
```

- ✗ No `additionalProperties: false`. Models can attach noise fields
  (e.g. `mode`, `if_exists`) and they are silently dropped.
- ✗ No `strict: true` flag — JSON-mode providers won't enforce.
- ✓ Clear required list.
- ✗ Existence semantics live in the runtime, not the schema. The model
  only learns about the freshness guard *by failing*.

**Verdict:** schemas describe inputs but *not contracts*. This is "Law
By Convention," not Code Is Law.

### Claude Code — Zod schemas, strict mode, output schema too

```ts
const inputSchema = lazySchema(() =>
  z.strictObject({
    file_path: z.string().describe('absolute path...'),
    content:   z.string().describe('content to write'),
  }),
)
const outputSchema = lazySchema(() =>
  z.object({
    type: z.enum(['create','update']),
    filePath: z.string(),
    content:  z.string(),
    structuredPatch: z.array(hunkSchema()),
    originalFile:    z.string().nullable(),
    gitDiff:         gitDiffSchema().optional(),
  }),
)

export const FileWriteTool = buildTool({
  name: FILE_WRITE_TOOL_NAME,
  strict: true,
  ...
  async validateInput({ file_path, content }, toolUseContext) {
    // EXPLICIT contract: must read first, mtime must match
    const readTimestamp = toolUseContext.readFileState.get(fullFilePath)
    if (!readTimestamp || readTimestamp.isPartialView) {
      return { result: false, message: 'File has not been read yet...', errorCode: 2 }
    }
    if (lastWriteTime > readTimestamp.timestamp) {
      return { result: false, message: 'File has been modified since read...', errorCode: 3 }
    }
    return { result: true }
  },
})
```

- ✓ `z.strictObject` rejects unknown keys.
- ✓ `strict: true` flag forwarded to provider.
- ✓ Separate **output schema** — the model knows what shape comes back.
- ✓ `validateInput()` is a separate phase before `call()`. Errors carry
  numeric `errorCode` for programmatic handling.
- ✓ The freshness contract is enforced **twice** (`validateInput`
  pre-flight + atomic re-check in `call`) — the second check protects
  against TOCTOU between validate and write.
- ✗ Description prose still carries some semantics
  (`getPreReadInstruction()`), but at least the validator backs it up.

**Verdict:** this *is* Code Is Law. The schema is the contract, the
validator is the kernel, and the prompt is the explainer.

### EdgeCrab — strict schema + jailed dispatch + read-tracker

```rust
ToolSchema {
  name: "write_file".into(),
  description: "...Attempting to write_file an existing non-empty file
                without first reading it in this session will be rejected;
                the rejection error includes the current file content...",
  parameters: json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "path":        {"type": "string", ...},
      "content":     {"type": "string", ...},
      "create_dirs": {"type": "boolean", ...},
    },
    "required": ["path", "content", "create_dirs"]
  }),
  strict: Some(true),
}
```

- ✓ `additionalProperties: false` + `strict: Some(true)`. Same hardness
  as Claude Code on the input side.
- ✓ Per-tool `path_arguments()` trait method feeds parallel-safety
  detection (FP9, see [13-parallel-tool-safety.md](13-parallel-tool-safety.md)).
- ✓ Schema cross-references stripped dynamically when a referenced tool
  is disabled (FP14, see [19-schema-cross-ref.md](19-schema-cross-ref.md)).
- ✓ Read-tracker freshness guard mirrors Claude Code's `readFileState`.
- ✗ **No output schema.** Tool result is `String`. The model has to
  parse human prose like `"Wrote 11 bytes to 'foo'"` rather than
  `{"type":"create","bytes":11}`. This is Law by String Match.
- ✗ `description` still carries crucial behavioral semantics in prose
  (the entire FP51 retry-protocol). The schema doesn't model the
  `if_exists` decision the model has to make.

**Verdict:** strong on inputs, weak on outputs and on encoding the
"what to do when I get an error" decision into the schema itself.

### Scoreboard — J1 schema contract

| Property                  | Hermes | Claude Code | EdgeCrab |
|---------------------------|:------:|:-----------:|:--------:|
| `additionalProperties:false` | ✗ | ✓ | ✓ |
| `strict` flag forwarded     | ✗ | ✓ | ✓ |
| Output schema present        | ✗ | ✓ | ✗ |
| Two-phase validate then call | ✗ | ✓ | partial |
| Numeric error codes          | ✗ | ✓ | ✗ |
| Behavior visible in schema (not prose) | ✗ | ✓ | partial |
| **Total**                    | 0/6 | 6/6 | 3/6 |

---

## 3. J3 — Validation: error messages as a recovery channel

Errors are not failures — they are tool calls in disguise. Their job is
to teach the model how to issue the next call correctly.

### Hermes

```python
return tool_error(str(e))
# ->  json.dumps({"error": "<exception message>"})
```

- ✓ Stable JSON shape — `{"error": "..."}`.
- ✗ No `required_fields`, no schema hint, no recovery example.
- ✗ Drops tool name from error text (handler wraps after the fact).

### Claude Code

```ts
return { result: false, message: '...', errorCode: 2 }
```

- ✓ Numeric error codes — caller can branch.
- ✓ Distinct messages for "not read yet" vs "modified since read".
- ✓ Errors render through `renderToolUseErrorMessage` so model and user
  see consistent text.

### EdgeCrab

`ToolError` enum (`InvalidArgs { tool, message }`, `PermissionDenied`,
`Other`) is rendered to JSON-ish strings. R2 added the
`required_fields`/`schema_hint` guidance (see
[04-error-guidance.md](04-error-guidance.md)). FP51 (R12) added the
**file content preview inside the rejection error**, which is unique
to EdgeCrab — Claude Code returns only the message, Hermes only a string.

But: **EdgeCrab does *not* populate the read-tracker snapshot at the
moment of rejection.** The model sees the content in the error, but
on the immediate retry the freshness guard fails because no snapshot
was recorded. Net effect: the model still issues a `read_file` round
trip — which makes the FP51 preview *informational* but not
*operational*. This is the fix proposed in [32-write-file-fp55.md](32-write-file-fp55.md).

### Scoreboard — J3 error recovery

| Property                          | Hermes | Claude Code | EdgeCrab |
|-----------------------------------|:------:|:-----------:|:--------:|
| Stable error envelope             | ✓ | ✓ | ✓ |
| Numeric / categorised codes       | ✗ | ✓ | partial (enum variants) |
| Tool name in error                | ✗ | ✓ | ✓ |
| Required-fields hint              | ✗ | ✗ | ✓ |
| Live content embedded on conflict | ✗ | ✗ | ✓ (FP51) |
| Snapshot auto-recorded on conflict so retry works without re-read | n/a | n/a (does its own re-stat) | ✗ → FP55 |
| **Total**                         | 1/6 | 3/6 | 4/6 → 5/6 after FP55 |

---

## 4. J4 — Effect mediation

### Path safety

| Concern                          | Hermes | Claude Code | EdgeCrab |
|----------------------------------|--------|-------------|----------|
| Path jail (block escapes)        | ✓ `_check_sensitive_path` | ✓ `expandPath` + `checkWritePermissionForTool` | ✓ `jail_write_path` |
| Allowed-roots config             | ✓ env vars | ✓ permission rules JSON | ✓ `file_allowed_roots` |
| Denylist subtree                 | partial (sensitive list) | ✓ rule matcher | ✓ `path_restrictions` |
| Virtual `/tmp` mapping           | ✗ | ✗ | ✓ `edgecrab_home/tmp/files` |
| UNC path NTLM-leak guard         | ✗ | ✓ | ✗ (Linux/macOS targets) |

EdgeCrab is the **only** harness with a *virtualised* tmp jail —
`/tmp/foo.md` is silently rewritten to `~/.edgecrab/tmp/files/foo.md`
so multi-tenant/test fixtures cannot collide.

### Freshness / TOCTOU

- **Hermes**: stores mtime per `(task_id, file)`; warns at write time.
  Warning, not block — model can still overwrite stale state.
- **Claude Code**: blocks at validate phase AND re-checks atomically
  inside `call`. CRLF/cloud-sync false-positive guard (Windows).
- **EdgeCrab**: blocks at execute phase via
  `read_tracker::guard_file_freshness`. No atomic re-check, but the
  Tokio dispatch is single-task per write so the window is narrow.

### Mutation budget

- **Hermes**: no per-call payload limit on writes.
- **Claude Code**: `maxResultSizeChars: 100_000` on the result, but no
  explicit cap on input `content`.
- **EdgeCrab**: configurable `max_write_payload_bytes` (default 32 KiB
  for new writes, separate guard for overwrites — see
  [21-configurable-write-limit.md](21-configurable-write-limit.md)).

### Auto-checkpoint

- **EdgeCrab** is the only one to call `ensure_checkpoint(ctx, ...)`
  before mutation, enabling `/rollback` undo.
- Hermes and Claude Code rely on git diff history as the rollback
  mechanism (Claude Code emits `gitDiff` inside the result envelope).

### Scoreboard — J4 effect mediation

| Property                  | Hermes | Claude Code | EdgeCrab |
|---------------------------|:------:|:-----------:|:--------:|
| Path jail                 | ✓ | ✓ | ✓ |
| Allowed-roots config      | ✓ | ✓ | ✓ |
| Denylist subtree          | partial | ✓ | ✓ |
| Virtual tmp jail          | ✗ | ✗ | ✓ |
| Atomic TOCTOU re-check    | ✗ | ✓ | ✗ |
| Configurable mutation cap | ✗ | partial | ✓ |
| Auto-checkpoint           | ✗ | ✗ | ✓ |
| Built-in git diff in result | ✗ | ✓ | ✗ |
| **Total**                 | 3.5/8 | 5.5/8 | 6/8 |

---

## 5. J5 — Result shaping

```
+------------------------------------------------------------------+
|                       AFTER A SUCCESSFUL WRITE                  |
+------------------------------------------------------------------+
|                                                                  |
|  Hermes:   "{\"path\":\"foo\",\"bytes_written\":11,\"created\":true}"
|              + optional "_warning" (stale)                      |
|              -- Stable shape, no diff, no skill discovery       |
|                                                                  |
|  Claude:   structured object via outputSchema, includes:        |
|              type: 'create' | 'update'                          |
|              structuredPatch: Hunk[]                             |
|              originalFile:    string | null                      |
|              gitDiff?:        ToolUseDiff                        |
|              + LSP didChange/didSave + skill discovery           |
|              + VSCode notify                                     |
|              -- Rich downstream affordances                      |
|                                                                  |
|  EdgeCrab: "Wrote 11 bytes to 'foo'"  (plain prose)             |
|              -- No diff, no structured fields, model parses prose
|                                                                  |
+------------------------------------------------------------------+
```

### Brutal verdict

EdgeCrab's plain-prose result is the **single biggest "Code Is Law"
gap**. Every downstream behavior the model wants to chain (e.g. "show
the diff", "confirm bytes", "check `type==update`") has to be parsed
out of a sentence. This is fragile and locale-dependent. Claude Code
sets the bar here.

---

## 6. J6 — Failure recovery (loop-level)

| Mechanism                                 | Hermes | Claude Code | EdgeCrab |
|-------------------------------------------|:------:|:-----------:|:--------:|
| Self-heal malformed JSON before re-call   | ✓ via prompt | ✓ Zod parse + fix | ✓ FP7 (`repair_tool_call_arguments`) |
| Type coercion (`"true"` → `true`)         | partial | ✓ | ✓ FP7 |
| Tool name sanitisation (special tokens)   | ✗ | ✗ | ✓ FP54 (Round 13) |
| Duplicate-call detection                  | ✓ via `_read_tracker` | partial | ✓ FP11 |
| Consecutive-failure escalation to user    | ✗ | ✗ | ✓ FP6 |
| Provider fallback on retry-able errors    | ✗ | ✗ | ✓ FP8 |
| Retry-after parsing from rate-limit body  | ✗ | partial | ✓ FP19 |
| Stream stale-chunk timeout                | ✗ | ✓ | ✓ FP10 |
| Compression circuit breaker               | ✗ | ✗ | ✓ FP12 |
| **Total**                                 | 2.5/9 | 3/9 | **9/9** |

This is where EdgeCrab is **categorically ahead**. The R2/R3/R4
investments paid for a harness that *survives* model misbehavior
in ways the other two do not.

---

## 7. J7 — Cost discipline

| Mechanism                          | Hermes | Claude Code | EdgeCrab |
|------------------------------------|:------:|:-----------:|:--------:|
| Per-session iteration budget       | ✓ | ✓ | ✓ |
| Wall-clock cap on streaming        | partial | ✓ | ✓ |
| Read dedup (mtime stub)            | ✓ | partial | ✓ FP13 |
| Compression with circuit breaker   | partial | ✓ | ✓ |
| Memory-write injection scan        | ✗ | ✓ team-mem only | ✓ FP15 |
| Token-cost tracking per call       | partial | ✓ | ✓ |
| **Configurable max write payload** | ✗ | ✗ | ✓ FP16 |

EdgeCrab and Claude Code are roughly tied on cost discipline; Hermes
trails because it has no provider-fallback or retry-after logic.

---

## 8. Aggregate scorecard

```
+---------------------------+--------+-------------+----------+
| Job                       | Hermes | Claude Code | EdgeCrab |
+---------------------------+--------+-------------+----------+
| J1 Schema contract        | 0/6    | 6/6         | 3/6      |
| J3 Error recovery         | 1/6    | 3/6         | 4/6 -> 5 |
| J4 Effect mediation       | 3.5/8  | 5.5/8       | 6/8      |
| J5 Result shaping         | 1/4    | 4/4         | 1/4      |
| J6 Failure recovery       | 2.5/9  | 3/9         | 9/9      |
| J7 Cost discipline        | 4/7    | 5/7         | 6/7      |
+---------------------------+--------+-------------+----------+
| TOTAL                     | 12/40  | 26.5/40     | 29/40 -> 30
+---------------------------+--------+-------------+----------+
```

**Brutal-honest takeaways:**

1. **EdgeCrab is the most resilient harness today** but the *least
   structured at the I/O boundaries* (no output schema, prose results).
2. **Claude Code wins the "Code Is Law" axis** because schemas + Zod
   validators + structured outputs make almost every contract
   machine-checkable.
3. **Hermes is the simplest** and that simplicity costs it both
   structure and resilience. Its strength is the prompt + tool prose
   loop; its weakness is anything beyond happy-path flow.

---

## 9. Why FP55 belongs in the next round

The screenshot embedded in the user request shows the **canonical
cost bug** of EdgeCrab today:

```
write_file foo.md          -> ✗ already exists (1500 bytes preview)
read_file  foo.md          -> 1 line returned   (~50 bytes)
write_file foo.md          -> ✓ Wrote 11297 bytes
```

The first error already gave the model the content; the read was a
**ceremonial** call to convince the freshness guard. Three round trips
where two suffice. **FP55** ([32-write-file-fp55.md](32-write-file-fp55.md))
closes this gap by populating the read-tracker snapshot at the moment
of rejection. The model can immediately retry — no re-read needed.

This single change converts the FP51 preview from informational into
operational. It also bridges EdgeCrab's J3 score from 4 → 5.

---

## 10. What stays unfixed (deliberate non-goals)

- **Output schemas** (J5 gap): a workspace-wide change. Tracked here as
  a known debt; not in scope for this round.
- **Atomic TOCTOU re-check** (J4 gap, single-cell): EdgeCrab's tokio
  dispatch already serialises writes per tool call. Adding a second
  stat would gain milliseconds at the cost of complexity.
- **UNC path NTLM-leak guard** (J4 gap): macOS/Linux primary targets;
  Termux build is also non-Windows. Noted, not implemented.

---

## 11. Cross-references

- Patterns to adopt from Hermes: [02-hermes-patterns.md](02-hermes-patterns.md)
- Round 2 brutal assessment: [09-assessment-round2.md](09-assessment-round2.md)
- Round 4 "Code Is Law" audit: [17-assessment-round4.md](17-assessment-round4.md)
- write_file content-preview baseline (FP51): [29-assessment-round12.md](29-assessment-round12.md)
- Tool name sanitiser (FP54): [30-assessment-round13.md](30-assessment-round13.md)
- **Next: FP55 design** → [32-write-file-fp55.md](32-write-file-fp55.md)
- **Re-assessment after FP55** → [33-assessment-round14.md](33-assessment-round14.md)
