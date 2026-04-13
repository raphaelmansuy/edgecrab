# ADR-001: Tool Result Spill-to-Artifact

**Status:** Accepted
**Date:** 2026-04-12
**Authors:** EdgeCrab Team
**Crates:** `edgecrab-core`, `edgecrab-tools`, `edgecrab-state`

---

## 1. Problem Statement

In the ReAct agent loop, every tool result is appended verbatim to
`session.messages` as a `Message::tool_result`. These strings flow directly
into the next LLM API call.

**Three failure modes:**

```
+-------------------------------------------------------------------+
| Failure Mode 1: Context Window Overflow                           |
|                                                                   |
|   grep returns 80 KB   +----> messages grow to 200K tokens        |
|   web_search 60 KB     |      +----> exceeds 128K window          |
|   file_read 2 MB       +      +----> API 400 error / truncation   |
+-------------------------------------------------------------------+

+-------------------------------------------------------------------+
| Failure Mode 2: Context Pollution (Silent Quality Degradation)    |
|                                                                   |
|   Tool A: 30 KB grep output  \                                    |
|   Tool B: 20 KB file read     +---> LLM attention diluted         |
|   Tool C: 15 KB terminal      /     across noise, misses key info |
|   Actual signal: 500 bytes          quality drops silently         |
+-------------------------------------------------------------------+

+-------------------------------------------------------------------+
| Failure Mode 3: Token Burn (Cost Amplification)                   |
|                                                                   |
|   Each loop iteration re-sends ALL prior tool results             |
|   50K of stale tool output * 10 iterations = 500K wasted tokens   |
|   At $3/M input tokens = $1.50 burned per conversation            |
+-------------------------------------------------------------------+
```

**Current mitigations are insufficient:**

| Existing Defense          | When It Fires         | Gap                                |
|---------------------------|-----------------------|------------------------------------|
| `prune_tool_outputs()`    | During compression    | Only at 50% context — too late     |
| `max_terminal_output`     | Per-tool (100K chars) | Still enormous; other tools: 0 cap |
| `CONTEXT_FILE_MAX_CHARS`  | System prompt only    | Does not cover tool results        |
| Per-tool truncation       | Ad-hoc in some tools  | Inconsistent; most tools unlimited |

---

## 2. Decision

Implement a **Tool Result Spill-to-Artifact** post-processor in the
conversation dispatch pipeline. When a tool result exceeds a configurable
byte threshold, the full result is written to a session-scoped artifact file
on disk, and only a **head preview + metadata stub** is injected into the
message history.

**Feature is gated by `tools.result_spill` config flag — enabled by default.**

### 2.1 First Principles

1. **Nothing is lost.** The full result is always persisted (file + DB metadata).
2. **Agent retains access.** The artifact file lives under `cwd`, readable via
   existing `read_file`, `file_search`, `terminal` tools — zero new tools needed.
3. **Minimal cognitive load.** The stub tells the agent: line count, byte count,
   file path, and the first N lines — enough to decide next action.
4. **Zero breaking changes.** The `ToolHandler` trait signature stays
   `Result<String, ToolError>`. Spill is a transparent post-processing step.
5. **Compression-friendly.** A 120-byte stub replaces a 50KB result. When
   compression fires later, it prunes a stub instead of a megabyte.
6. **DRY.** One spill function, one call site in the dispatch pipeline.

### 2.2 Architecture

```
+------------------+     +------------------+     +-------------------+
|   ToolHandler    |     |   Spill Gate     |     |  Session Messages |
|   .execute()     |---->|   (post-proc)    |---->|  .push(msg)       |
|                  |     |                  |     |                   |
| Returns Ok(text) |     | len > threshold? |     | Stub OR original  |
+------------------+     +--------+---------+     +-------------------+
                                  |
                          YES     |     NO
                     +------------+--------+
                     |                     |
              +------v------+       (pass through)
              | Write full  |
              | result to   |
              | artifact    |
              | file on disk|
              +------+------+
                     |
              +------v------+
              | Build stub: |
              | head lines  |
              | + metadata  |
              | + file path |
              +------+------+
                     |
              +------v------+
              | Record in   |
              | DB artifacts|
              | table       |
              +-------------+
```

### 2.3 Spill Stub Format

```
[tool_result_spill]
tool: file_search
lines: 2847
bytes: 98304
artifact: .edgecrab-artifacts/session-abc123/file_search_001.md
showing: 80/2847 lines (first 3%)

--- BEGIN PREVIEW (80 lines) ---
<first 80 lines of the original output>
--- END PREVIEW ---

Full result saved to: .edgecrab-artifacts/session-abc123/file_search_001.md
Use read_file or file_search to explore the full content.
```

### 2.4 Artifact File Location

```
{cwd}/.edgecrab-artifacts/{session_id}/{tool_name}_{seq:03}.md

Example:
/home/user/project/.edgecrab-artifacts/ses_a1b2c3/file_search_001.md
/home/user/project/.edgecrab-artifacts/ses_a1b2c3/terminal_002.md
```

**Why under `cwd`:**
- Agent read_file tool validates paths relative to cwd — guaranteed accessible.
- Session-scoped subdirectory prevents cross-session leakage.
- `.edgecrab-artifacts` prefix is easy to `.gitignore`.

### 2.5 Configuration

```yaml
# ~/.edgecrab/config.yaml
tools:
  result_spill: true               # Feature gate (default: true)
  result_spill_threshold: 16384    # Bytes; results > this -> spill (default: 16KB)
  result_spill_preview_lines: 80   # Lines to keep in the stub (default: 80)
```

Environment variable overrides:
- `EDGECRAB_TOOL_RESULT_SPILL=true|false`
- `EDGECRAB_TOOL_RESULT_SPILL_THRESHOLD=16384`
- `EDGECRAB_TOOL_RESULT_SPILL_PREVIEW_LINES=80`

### 2.6 Database Schema Extension

```sql
-- Migration v6 -> v7
CREATE TABLE IF NOT EXISTS artifacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    tool_call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    artifact_path TEXT NOT NULL,
    original_bytes INTEGER NOT NULL,
    original_lines INTEGER NOT NULL,
    preview_lines INTEGER NOT NULL,
    created_at REAL NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_artifacts_session ON artifacts(session_id);
```

---

## 3. Edge Cases and Roadblocks

### 3.1 Edge Cases

| # | Edge Case | Handling |
|---|-----------|----------|
| 1 | Result exactly at threshold | `>` strictly greater — no spill at boundary |
| 2 | Tool result is an error (JSON) | Never spill errors — always short, always inline |
| 3 | Binary/non-UTF8 in result | `safe_truncate()` handles UTF-8 boundary |
| 4 | Result has 0 newlines (single line) | Preview = first `threshold` bytes, line count = 1 |
| 5 | Session ID not yet assigned | Use temp UUID; rename artifact dir once assigned |
| 6 | Parallel tool calls write concurrently | Atomic sequence counter per session (AtomicU32) |
| 7 | Disk full / write permission denied | Log warning, fall back to inline (no spill) |
| 8 | `cwd` is read-only (e.g., `/`) | Fall back to `$TMPDIR/edgecrab-artifacts/` |
| 9 | Agent reads the artifact file | Works — read_file validates path, artifact under cwd |
| 10 | Compression prunes the stub later | Fine — stub is tiny, compression handles normally |
| 11 | Feature disabled mid-session | Checked per-call; disabling stops new spills |
| 12 | Sub-agent delegation results | Flow through same dispatch — spill applies |
| 13 | Tool result is valid JSON object | Spill as-is; agent can read_file to parse |
| 14 | Very long lines (no newlines) | Preview falls back to byte-based slicing |
| 15 | Session search (FTS5) | Stub is indexed; full artifact via DB lookup |

### 3.2 Roadblocks and Mitigations

| Roadblock | Impact | Mitigation |
|-----------|--------|------------|
| R1: ToolHandler trait change | Breaking change | No change. Spill is post-processing |
| R2: Artifact dir not gitignored | Accidental commits | Auto-append to .gitignore on first spill |
| R3: Artifact files accumulate | Disk bloat | Session cleanup deletes artifacts |
| R4: Path safety validation | Must pass validate_path | Artifact under cwd, already allowed |
| R5: DB migration required | Schema v6 to v7 | Additive-only; old sessions unaffected |
| R6: FTS5 sync for artifacts | Full result not searchable | Store path in artifacts table |
| R7: Concurrent test interference | Test collisions | Each test uses TempDir |
| R8: Gateway mode different cwd | No project cwd | Fall back to edgecrab_home/artifacts/ |
| R9: ACP integration | Must not break | Transparent — ACP sees stub as string |
| R10: Prompt cache invalidation | No system prompt changes needed | No system prompt changes |

### 3.3 Security

| Concern | Mitigation |
|---------|------------|
| Path traversal via tool_name | Sanitize: alphanumeric + underscore only |
| Secrets in artifact content | Same scope as tool output — no new exposure |
| Prompt injection in artifacts | Content from tool output, not user context files |
| Symlink attack on artifact dir | create_dir_all + atomic file creation |

---

## 4. Alternatives Considered

| Alternative | Why Rejected |
|-------------|--------------|
| Truncate all results at hard cap | Loses data. Agent cannot recover |
| Compress results with LLM inline | Expensive extra LLM call per tool |
| Store in HashMap not file | Process crash loses data. Agent cant read_file |
| New ToolResult struct replacing String | Breaking change to 30+ tools |
| Let compression handle it | Fires at 50% context — too late |
| Per-tool opt-in spill | Inconsistent. Every new tool must implement |

---

## 5. Implementation Plan

### Phase 1: Core Spill Module (edgecrab-core/src/tool_result_spill.rs)

- SpillConfig struct (threshold, preview_lines, enabled)
- maybe_spill() function returning SpillOutcome enum
- Artifact file writer (atomic, session-scoped dir)
- Stub builder (header + preview lines + footer)
- sanitize_tool_name() for safe filenames
- Per-session atomic sequence counter

### Phase 2: Config Integration (edgecrab-core/src/config.rs)

- Add to ToolsConfig: result_spill, result_spill_threshold, result_spill_preview_lines
- Add env overrides in apply_env_overrides()
- Propagate to AppConfigRef

### Phase 3: Conversation Loop Integration (edgecrab-core/src/conversation.rs)

- In dispatch_single_tool(), call maybe_spill() after obtaining result
- Replace result string with stub if spilled
- Log spill event via tracing

### Phase 3b: Compression Integration (edgecrab-core/src/compression.rs)

- Add PruneSpillContext struct carrying session_id, cwd, SpillConfig, SpillSequence
- Update compress_with_llm() to accept Optional spill context
- Update prune_tool_outputs() to spill large results to artifacts when context provided
- Fall back to generic placeholder when below spill threshold or context unavailable
- Wire PruneSpillContext at compression call site in execute_loop
- Pass None in force_compress (manual /compress)

### Phase 4: Database Schema Extension (edgecrab-state)

- Add v7 migration with artifacts table
- Add insert_artifact() to SessionDb

### Phase 5: Tests

- Unit tests for spill module (15+ cases)
- Integration test in conversation.rs
- Config deserialization tests
- DB migration test

### Dependency Graph

```
    Phase 1 ---+
               |
    Phase 2 ---+---> Phase 3 ---> Phase 5
               |
    Phase 4 ---+
```

---

## 6. File Change Summary

| File | Change | Description |
|------|--------|-------------|
| edgecrab-core/src/tool_result_spill.rs | NEW | Spill module (SpillConfig, maybe_spill, SpillOutcome) |
| edgecrab-core/src/lib.rs | MODIFY | Add `pub mod tool_result_spill` |
| edgecrab-core/src/config.rs | MODIFY | Add result_spill fields to ToolsConfig + env overrides |
| edgecrab-core/src/conversation.rs | MODIFY | Call maybe_spill() in dispatch; pass PruneSpillContext to compression |
| edgecrab-core/src/compression.rs | MODIFY | Accept PruneSpillContext; spill during prune_tool_outputs |
| edgecrab-core/src/agent.rs | MODIFY | Propagate spill config; pass None in force_compress |
| edgecrab-tools/src/config_ref.rs | MODIFY | Add fields to AppConfigRef |
| edgecrab-state/src/schema.sql | MODIFY | Add artifacts table (Phase 4 — pending) |
| edgecrab-state/src/lib.rs | MODIFY | Add insert_artifact(), migration (Phase 4 — pending) |

---

## 7. Rollback Plan

Set `tools.result_spill: false`. Disabling:
- Stops all new spills immediately
- Old artifact files remain on disk (harmless)
- Old stubs in message history still reference valid files
- No schema rollback needed (additive table)

---

## 8. Success Metrics

| Metric | Target |
|--------|--------|
| No tool result > 16KB in session.messages | 100% when enabled |
| Agent reads spilled artifacts successfully | Verified in tests |
| Zero breaking changes to ToolHandler trait | Compile-time guarantee |
| Full test suite passes | 0 failures |
| Token savings on large conversations | >40% reduction in input tokens |

---

## 9. Cross-References

- conversation.rs:3125 — dispatch_single_tool()
- conversation.rs:3003-3048 — result to message insertion
- compression.rs:440 — prune_tool_outputs(messages, spill_ctx)
- compression.rs:364 — compress_with_llm(messages, params, provider, spill_ctx)
- compression.rs:88 — PruneSpillContext struct
- message.rs:40 — Message::tool_result()
- config_ref.rs:55 — AppConfigRef
- config.rs:653 — ToolsConfig
- schema.sql — current schema v6

---

## 10. Compression Integration

**Added:** 2026-04-12

### 10.1 Motivation

Before this enhancement, `prune_tool_outputs()` in `compression.rs`
replaced every tool result larger than 200 chars with a generic
`[tool output pruned — reclaimed context window space]` placeholder.
This aggressively reclaims context but **permanently destroys** the data
— the agent cannot retrieve it later.

With the spill-to-artifact infrastructure already in place, compression
can now leverage it: large tool results pruned during compression are
spilled to artifact files instead of being replaced with a generic
placeholder. The agent retains `read_file` access to the full original
output through the artifact path in the stub.

### 10.2 Architecture

```
compress_with_llm(messages, params, provider, spill_ctx?)
    │
    ├── prune_tool_outputs(messages, spill_ctx?)
    │       │
    │       ├── tool result.len() <= 200 chars?  ──> clone as-is
    │       │
    │       ├── spill_ctx provided & enabled?
    │       │       │
    │       │       ├── result > spill threshold? ──> write artifact + stub
    │       │       │
    │       │       └── result <= spill threshold ──> generic placeholder
    │       │
    │       └── no spill_ctx ──> generic placeholder
    │
    ├── boundary determination (token-budget walk)
    ├── LLM summarization (or structural fallback)
    ├── assemble head + summary + tail
    └── orphan pair sanitization
```

### 10.3 Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| `spill_ctx` is `Option` | Backward compatible — `None` = old behavior (placeholder) |
| Spill fires only above spill threshold | Results between 200 chars and spill threshold still get placeholder |
| `force_compress()` passes `None` | Manual /compress has no reliable cwd/session_id context |
| Spill shares same `SpillSequence` | Dispatch + compression both increment same counter — no filename collisions |
| Compression spill uses same artifact dir | All artifacts for a session are co-located |

### 10.4 Call Site Summary

| Call Site | Module | spill_ctx |
|-----------|--------|-----------|
| `execute_loop` auto-compression | conversation.rs | `Some(PruneSpillContext { ... })` |
| `force_compress` (/compress) | agent.rs | `None` — falls back to placeholder |

### 10.5 Two-Layer Defense Model

```
Layer 1: Real-time dispatch (conversation.rs dispatch_single_tool)
  └── Tool returns 50KB → maybe_spill() → stub in messages immediately
  └── Threshold: tools.result_spill_threshold (default 16KB)

Layer 2: Compression phase (compression.rs prune_tool_outputs)
  └── Any surviving >200-char tool results from before spill was enabled,
      or results that were below the dispatch threshold but above 200 chars:
      └── If spill_ctx available: attempt spill to artifact
      └── Otherwise: generic placeholder
```

This dual-layer approach ensures that even if a tool result was inlined
during dispatch (e.g., because it was 8KB — below the 16KB threshold),
it can still be captured to an artifact during compression rather than
being fully destroyed.

### 10.6 Tests Added

| Test | Verifies |
|------|----------|
| `prune_tool_outputs_spills_when_context_provided` | Spill stub replaces large results during compression |
| `prune_tool_outputs_falls_back_to_placeholder_when_below_spill_threshold` | Below spill threshold → generic placeholder |
| `prune_tool_outputs_skips_spill_when_disabled` | Feature gate respected during compression |
