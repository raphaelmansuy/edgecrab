# ADR-003 — Document Sync Strategy: Full vs Incremental

**Status**: Accepted  
**Date**: 2025

---

## Context

LSP requires a client to inform the server of document content. Two strategies:

- `TextDocumentSyncKind::Full` — send the entire file on every change
- `TextDocumentSyncKind::Incremental` — send only the changed ranges

The server advertises which it supports in `serverCapabilities.textDocumentSync`.

---

## Options

### Option A: Always use Full

Every `didChange` notification contains the complete file text.

Pros:
- Simple: no diff computation, no version tracking complexity
- Universally supported by all language servers
- No risk of client/server state divergence

Cons:
- For large files (e.g., generated code, minified JS, large Rust files), sends megabytes per change
- `MAX_LSP_FILE_BYTES = 10,000,000` means up to 10MB per notification

### Option B: Always use Incremental

Compute a diff and send only changed ranges.

Pros:
- Lower bandwidth for large files

Cons:
- EdgeCrab does not maintain an in-memory buffer model — files are on disk
- Computing a proper incremental diff requires: (a) knowing the current server state,
  (b) running a character-level diff (e.g., `similar::ChangeTag`), (c) mapping byte
  ranges to LSP line/character positions (UTF-16 encoding)
- Risk: if our diff is wrong, server state silently diverges → all subsequent operations
  return wrong results

### Option C: Server-Driven (Full default, Incremental when server signals it AND file > threshold)

Follow the server's `textDocumentSync.change` capability field.
Use `Full` unless server explicitly advertises `Incremental` AND file exceeds a threshold (50 KB).

---

## Decision

**Option C** for production correctness with performance opt-in.

Default behaviour:
```rust
fn sync_kind(caps: &ServerCapabilities) -> TextDocumentSyncKind {
    match &caps.text_document_sync {
        Some(TextDocumentSyncCapability::Kind(k)) => *k,
        Some(TextDocumentSyncCapability::Options(o)) => {
            o.change.unwrap_or(TextDocumentSyncKind::FULL)
        }
        None => TextDocumentSyncKind::FULL,
    }
}
```

Incremental diff (only used when server requests it AND file > 50 KB):
```rust
use similar::{TextDiff, ChangeTag};

fn compute_incremental_edits(old: &str, new: &str) -> Vec<TextDocumentContentChangeEvent> {
    // Use word-level diff for performance on large files
    let diff = TextDiff::from_lines(old, new);
    let mut edits = Vec::new();
    // ... convert ChangeTag hunks to TextDocumentContentChangeEvent with Range
    edits
}
```

---

## Consequences

- `DocumentSyncLayer` stores the last-synced content per URI (avoids re-reading disk for diff)
- Version counter is per-URI and strictly monotonically increasing (u32 wrapping is fine at LSP scale)
- If a file is written from outside EdgeCrab (e.g., `cargo fmt` in terminal), the sync layer
  detects stale version via the `notify` file watcher and re-opens with a fresh version
