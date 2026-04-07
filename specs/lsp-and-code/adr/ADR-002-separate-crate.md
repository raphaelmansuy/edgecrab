# ADR-002 — New `edgecrab-lsp` Crate vs Embedding in `edgecrab-tools`

**Status**: Accepted  
**Date**: 2025  
**Deciders**: EdgeCrab architecture team

---

## Context

LSP integration requires:
- A long-lived server process pool (`LspServerManager`)
- Shared document sync state (`DocumentSyncLayer`)
- A diagnostic cache (`DiagnosticCache`)
- 20+ tool implementations

The question: should this live inside `edgecrab-tools` as additional tool files, or in a
dedicated `edgecrab-lsp` crate?

---

## Options

### Option A: Embed in `edgecrab-tools`

All LSP code goes into `crates/edgecrab-tools/src/tools/lsp/*.rs`.

Pros:
- No new crate to maintain
- Tools automatically available via existing registry

Cons:
- `edgecrab-tools` is already the largest crate; adding ~3,000 lines of LSP infrastructure
  makes it harder to reason about
- `async-lsp` and `lsp-types` become workspace-wide deps even for users who don't need LSP
- Unit testing the `LspServerManager` requires mocking the entire `ToolContext`
- Violates SRP: one crate both registers tools AND manages child processes AND implements
  LSP protocol — three distinct responsibilities

### Option B: Dedicated `edgecrab-lsp` crate

```
crates/edgecrab-lsp/
  Cargo.toml   ← only async-lsp + lsp-types + peer crates
  src/
    lib.rs
    manager.rs
    sync.rs
    diagnostics.rs
    capability.rs
    position.rs
    enrichment.rs
    tools/      ← ToolHandler impls that reference lib types
```

`edgecrab-tools/Cargo.toml` gains: `edgecrab-lsp = { path = "../edgecrab-lsp" }`
(optional feature flag so non-LSP builds stay lean).

Pros:
- `async-lsp` / `lsp-types` only compiled when LSP feature is enabled
- `LspServerManager` can be unit-tested without the tool registry
- Clear dependency boundary: `edgecrab-lsp` → `edgecrab-types + edgecrab-security`;
  other crates have no reverse dependency on LSP code
- Compile isolation: LSP changes don't trigger recompile of `edgecrab-tools`

Cons:
- One more crate to maintain (path in workspace, version in Cargo.toml)
- Tool registration still happens via `inventory::submit!` in `edgecrab-lsp/src/tools/*.rs`,
  which requires `edgecrab-tools` as a dep — creates a circular concern
  → **Resolution**: tool registration structs live in `edgecrab-tools`; `edgecrab-lsp` exports
  `LspServerManager` and operation functions; `edgecrab-tools` calls them. No cycle.

---

## Decision

**Option B: Dedicated `edgecrab-lsp` crate**

The separation enforces SOLID's Single Responsibility Principle at the crate boundary.
The dependency topology is:

```
edgecrab-types
      ↑
edgecrab-security
      ↑
edgecrab-lsp   ←────────────────────────────────────────────┐
      ↑                                                       │
edgecrab-tools  (wraps LspServerManager in ToolHandler impls, no cycle)
      ↑
edgecrab-core
```

The `inventory::submit!` registration still happens inside `edgecrab-lsp/src/tools/*.rs`
because `inventory` is a proc-macro that works across crate boundaries — the inventory is
collected at link time, not at import time.

---

## Consequences

- `edgecrab-lsp` as an **optional workspace crate** (feature-gated in root Cargo.toml)
- `LspServerManager` is shared state held in `ToolContext.lsp` via `Option<Arc<LspServerManager>>`
- The `lsp` field in `ToolContext` is only `Some(_)` when the `lsp` feature is enabled
- Non-LSP builds see no compile-time or runtime overhead
