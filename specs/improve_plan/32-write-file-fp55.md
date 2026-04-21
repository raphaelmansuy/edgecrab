# 32 — FP55: write_file existence-check directive + snapshot-on-reject

> **Goal:** make the FP51 content preview *operational* (not just
> informational) so a write-into-existing-file collision needs **two**
> tool calls instead of three. Encode the contract into the schema and
> tool description so the model picks the cheap path by default.
>
> **Cross-ref:** [29-assessment-round12.md §FP51-FP52](29-assessment-round12.md) ·
> [31-harness-deep-comparison.md](31-harness-deep-comparison.md) ·
> [04-error-guidance.md](04-error-guidance.md) ·
> [18-read-dedup-loop.md](18-read-dedup-loop.md)

---

## 1. Problem (verbatim from the screenshot)

```
[12:33:01] write  ./audit_quanta.md     ✗ Invalid arguments for write_file:
                                          './audit_quanta.md' already exists
                                          and has not been read in this session
[12:33:01] read   ./audit_quanta.md     -> 1 line  1| # Audit Report: Quantalogic
[12:33:01] write  ./audit_quanta.md     -> ✓ 11297 bytes  355ms
```

The agent (`openrouter/nvidia/nemotron-3-super-120b`) issued a `write_file`
expecting to create a new file. The path already existed. EdgeCrab
rejected with the FP51 content preview — but the model still had to
issue a second `read_file` round trip to satisfy the freshness guard
before retrying.

**Cost breakdown of one collision today:**

| Step | Output bytes | Tokens (~4 chars) |
|------|--------------|-------------------|
| Reject error w/ preview | ~1700 | ~425 |
| `read_file` call + result | ~120 | ~30 |
| `read_file` extra LLM turn (planning) | ~200 | ~50 |
| Retry `write_file` | ~content | model output |
| **Wasted overhead** | **~2020 bytes** | **~505 tokens** |

The retry itself is unavoidable; the **`read_file` round trip is pure
ceremony** because the rejection error already shipped the content.

---

## 2. First Principles

**FP1 — Make the right thing easy, the wrong thing impossible.**
If the model must always pair a rejection with a read before retrying,
the harness should record the snapshot at rejection time so the read
becomes redundant.

**FP2 — Every error is a recovery instruction.** The rejection text
must explicitly tell the model "snapshot recorded — retry immediately
will succeed."

**FP3 — Schema beats prose.** A new optional `if_exists` enum lets the
model declare intent up-front so the harness can choose between a
*cheap* error (no preview, no snapshot — model wanted a fresh path) and
a *full* error (preview + snapshot — model wanted to overwrite).

**FP4 — Don't break callers without cause.** Adding a brand-new
required field would force every existing call site to update. Make
`if_exists` optional, with a default that matches today's behavior.

**FP5 — Cost is the design constraint.** The combined fix saves
~395 tokens per collision in the *overwrite-intended* case (drops
the read) and ~1400 bytes per collision in the *create-intended*
case (drops the preview).

---

## 3. Design

### 3.1 Schema change

```jsonc
// crates/edgecrab-tools/src/tools/file_write.rs
{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "path":        { "type": "string" },
    "content":     { "type": "string" },
    "create_dirs": { "type": "boolean" },
    "if_exists": {
      "type": "string",
      "enum": ["overwrite", "abort"],
      "description":
        "Intent when the file already exists. \
         'overwrite' (default): if the file exists, the rejection error \
         includes the current content and records a session snapshot so \
         an immediate retry succeeds with no extra read_file. \
         'abort': cheap rejection (~80 bytes) with no content preview \
         and no snapshot — use when intent is to create a NEW file."
    }
  },
  "required": ["path", "content", "create_dirs"]
}
```

**Why `if_exists` is NOT in `required`:** strict-mode OpenAI tool
schemas allow optional fields with `additionalProperties: false`
provided they are simply omitted from `required`. Default value is
applied server-side via `#[serde(default)]`.

**Default = `"overwrite"`** so existing call sites and tests remain
unchanged in behavior.

### 3.2 Args struct

```rust
#[derive(Deserialize)]
struct Args {
    path: String,
    content: String,
    #[serde(default)]
    create_dirs: bool,
    #[serde(default)]
    if_exists: IfExists,
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum IfExists {
    #[default]
    Overwrite,
    Abort,
}
```

### 3.3 Behavior matrix

| File state          | `if_exists`   | Behavior                                         | Wasted bytes |
|---------------------|---------------|--------------------------------------------------|--------------|
| Does not exist      | (any)         | Write succeeds                                   | 0            |
| Exists, empty       | (any)         | Write succeeds — empty scaffold reuse            | 0            |
| Exists, non-empty, snapshot present, fresh | (any) | Write succeeds                                   | 0            |
| Exists, non-empty, no snapshot | `overwrite` | **Reject** with preview + record snapshot. Hint says "retry will succeed". | ~600 |
| Exists, non-empty, no snapshot | `abort`     | **Cheap reject**: stat-only, no preview, no snapshot. | ~120 |
| Exists, non-empty, snapshot stale | (any)    | **Reject** with stale-mtime message; require re-read first. | ~150 |

### 3.4 Description directive (the prompt-level fix)

```text
Creates or fully replaces a file.

WHEN TO USE
- New file: omit `if_exists` or set "abort" to fail cheaply if the
  path is taken.
- Replace existing file: set if_exists="overwrite" (default). If the
  file exists, the rejection includes its current content and records
  a session snapshot — issue the same write_file call again to
  succeed; you do NOT need a separate read_file round trip.

PARTIAL EDITS
- For ANY modification to an existing file, use patch/apply_patch
  instead — far more token-efficient and avoids regenerating
  unchanged content.

CONTRACT
- `content` MUST be a string; use "" for an empty scaffold.
- Hard payload limit: {DEFAULT_MAX_MUTATION_PAYLOAD_BYTES} bytes.
- For larger new files, write a small scaffold then extend with patch.
```

### 3.5 Snapshot-on-reject implementation

```rust
if file_exists && !existing_file_is_empty
   && !crate::read_tracker::has_file_snapshot(&ctx.session_id, &resolved)
{
    match args.if_exists {
        IfExists::Abort => {
            // Cheap rejection — model intent was "create new".
            return Err(ToolError::InvalidArgs {
                tool: "write_file".into(),
                message: format!(
                    "'{path}' already exists ({size} bytes). \
                     Either choose a different path, or call write_file again \
                     with if_exists=\"overwrite\".",
                    path = args.path,
                    size = std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(0)
                ),
            });
        }
        IfExists::Overwrite => {
            // Record snapshot so the immediate retry succeeds.
            let _ = crate::read_tracker::record_file_snapshot(&ctx.session_id, &resolved);

            // Embed content preview (FP51 behavior, narrowed to 600 chars).
            const PREVIEW_LIMIT: usize = 600;
            let current_content = std::fs::read_to_string(&resolved).unwrap_or_default();
            let (preview, truncated) = utf8_safe_truncate(&current_content, PREVIEW_LIMIT);
            let trunc_note = if truncated {
                format!(
                    "\n[...truncated — file has {} total bytes; \
                     read_file gives full content if needed.]",
                    current_content.len()
                )
            } else { String::new() };
            return Err(ToolError::InvalidArgs {
                tool: "write_file".into(),
                message: format!(
                    "'{path}' already exists. Snapshot recorded — \
                     retry the SAME write_file call to overwrite. \
                     For targeted edits prefer patch/apply_patch.\n\
                     \n\
                     Current file content (preview):\n\
                     ---\n\
                     {preview}{trunc_note}\n\
                     ---",
                    path = args.path
                ),
            });
        }
    }
}
```

### 3.6 Why preview shrinks 1500 → 600

- Empirical: FP51 preview was sized for "model needs structure to
  compose a patch." But the model issuing FP55 retries doesn't need
  the structure to *retry the write* — it needs only enough to
  decide whether to retry or to back off.
- 600 chars (~150 tokens) is enough to recognise a file by its first
  few lines. If the model truly needs the full content, the truncation
  hint points to `read_file`.
- Net per-collision saving: ~225 tokens vs FP51, in addition to the
  ~80 tokens saved by skipping `read_file`.

### 3.7 SOLID / DRY mapping

- **SRP:** `record_file_snapshot` already exists in `read_tracker`;
  reused — no new duplication.
- **OCP:** `IfExists` enum is open for extension (`SkipIfExists`,
  `Append`) without modifying callers thanks to `#[serde(default)]`.
- **DIP:** the file_write tool depends on the `read_tracker` module
  abstraction (snapshot + freshness), not on filesystem details.
- **DRY:** UTF-8-safe truncate is extracted into a small helper
  `utf8_safe_truncate(&str, usize) -> (&str, bool)` so it can be
  reused by other tools (e.g. file_patch error messages).

### 3.8 Edge cases — exhaustively enumerated

| # | Scenario | Expected behavior |
|---|----------|-------------------|
| 1 | New file + `if_exists=overwrite` (default) | Create, succeed |
| 2 | New file + `if_exists=abort` | Create, succeed (file didn't exist) |
| 3 | Existing empty file + either mode | Allowed, "Created empty scaffold" message |
| 4 | Existing file + read first + `overwrite` | Succeeds normally |
| 5 | Existing file + no read + `overwrite` | Reject + preview + snapshot recorded |
| 6 | Existing file + no read + `overwrite` + retry | Succeeds (snapshot now present) |
| 7 | Existing file + no read + `abort` | Cheap reject (no preview, no snapshot) |
| 8 | Existing file + no read + `abort` + retry with overwrite | Reject + preview + snapshot, then retry succeeds |
| 9 | Existing file modified externally between reject and retry | Freshness guard rejects with stale-mtime message |
| 10 | Existing file deleted between reject and retry | Write creates new file (snapshot stale; freshness check sees `exists=false` matching old `exists=true` — must trip stale path). See note. |
| 11 | Path is binary file (large) | Preview is UTF-8-safe; non-text bytes truncated at char boundary or ASCII fallback |
| 12 | Multi-byte UTF-8 boundary inside 600-char window | `utf8_safe_truncate` walks back to char boundary |
| 13 | Concurrent write from another session | Snapshot is per-session; cross-session conflicts handled by mtime check on retry |
| 14 | Strict-mode provider rejects extra `if_exists` field | Field is in schema → provider accepts it as enum |
| 15 | Provider sends `IF_EXISTS=Overwrite` (case mismatch) | `#[serde(rename_all="lowercase")]` accepts only lowercase. Mismatched casing → InvalidArgs error with required-fields hint |

> **Note on case 10:** when the file is *deleted* between reject and
> retry, the snapshot recorded at reject time has `exists=true`; the
> retry sees `exists=false`, fails freshness comparison, returns
> "modified since you last read it" — semantically correct (the file
> *was* modified, by deletion) and forces the model to re-read or
> change strategy.

### 3.9 What does NOT change

- `path_arguments()` — still `&["path"]`.
- `enforce_write_payload_limit_with_max` — unchanged.
- `ensure_checkpoint` — still fires before mutation.
- `notify_other_tool_call` semantics — unchanged.
- All existing tests pass with the default `if_exists=overwrite`.

---

## 4. Token economics — concrete forecast

| Scenario | Today | After FP55 | Saving |
|----------|-------|-----------|--------|
| Create-intended collision (`if_exists=abort`) | ~1700 B + 1 read trip = ~2020 B / ~505 tok | ~120 B / ~30 tok | **~475 tok / 94%** |
| Replace-intended collision (`if_exists=overwrite`) | ~2020 B / ~505 tok | ~720 B / ~180 tok | **~325 tok / 64%** |
| Happy path (file doesn't exist) | unchanged | unchanged | 0 |
| Happy path (file existed + read first) | unchanged | unchanged | 0 |

A medium agent run with ~10 file collisions over a session today wastes
~5000 tokens of error chatter. FP55 brings that to ~1500 — a 70%
reduction in collision-driven waste, with **zero impact on the
happy path**.

---

## 5. Test plan

```
crates/edgecrab-tools/src/tools/file_write.rs   (unit tests)
  test_write_new_file_default_mode_succeeds        (case 1)
  test_write_new_file_abort_mode_succeeds          (case 2)
  test_existing_empty_scaffold_allowed_either_mode (case 3)
  test_existing_after_read_overwrite_succeeds      (case 4)
  test_existing_no_read_overwrite_rejects_with_preview_and_records_snapshot
                                                   (case 5 + 6)
  test_existing_retry_after_overwrite_reject_succeeds_without_extra_read
                                                   (case 6 — primary)
  test_existing_no_read_abort_returns_cheap_error_no_preview_no_snapshot
                                                   (case 7)
  test_abort_then_overwrite_retry_succeeds         (case 8)
  test_external_modification_between_reject_and_retry_fails_freshness
                                                   (case 9)
  test_external_deletion_between_reject_and_retry_fails_freshness
                                                   (case 10)
  test_preview_truncates_at_utf8_boundary           (case 12)
  test_schema_includes_if_exists_enum_optional      (case 14)
  test_schema_strict_remains_true
  test_description_contains_directive_phrase
  test_description_documents_retry_protocol
```

E2E coverage already provided by:
- `cargo test -p edgecrab-tools --lib write_file`
- `cargo test --workspace` (cross-tool freshness + read-tracker tests)

Run order: clippy first (catch type errors fast), then targeted file
tests, then full workspace.

---

## 6. Rollout

1. Land changes behind no feature flag — defaults preserve behavior.
2. Update [README.md](README.md) document map: add row 31, 32, 33.
3. Land [33-assessment-round14.md](33-assessment-round14.md) immediately
   so the comparison table reflects the new J3 score (5/6).

---

## 7. Cross-references

- Predecessor FP51 (preview-in-error): [29-assessment-round12.md](29-assessment-round12.md)
- Read-tracker module: `crates/edgecrab-tools/src/read_tracker.rs`
- Harness comparison: [31-harness-deep-comparison.md](31-harness-deep-comparison.md)
- Re-assessment after implementation: [33-assessment-round14.md](33-assessment-round14.md)
