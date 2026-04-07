# EdgeCrab LSP Specification

> **Goal**: Design an LSP subsystem that definitively exceeds Claude Code's 9-operation LSP
> implementation. Ground every decision in First Principles, enforce DRY and SOLID throughout,
> and use the best available Rust components.

---

## Why This Spec Exists

Claude Code (as of the edgecrab-vs-claude-v2 analysis) scores **10/10** on LSP tooling.
EdgeCrab scores **0/10** — the capability does not exist. This spec closes the gap and
goes further: 20+ operations vs 9, enrichment features Claude Code cannot offer, and
a layered architecture that is safe to maintain and extend.

---

## Feature Comparison: Target State

| Capability                       | Claude Code | EdgeCrab Target | Notes                              |
|----------------------------------|:-----------:|:---------------:|------------------------------------|
| Go to definition                 | ✅          | ✅              | Tier 1 parity                      |
| Find references                  | ✅          | ✅              | Tier 1 parity                      |
| Hover info                       | ✅          | ✅              | Tier 1 parity                      |
| Document symbols                 | ✅          | ✅              | Tier 1 parity                      |
| Workspace symbols                | ✅          | ✅              | Tier 1 parity                      |
| Go to implementation             | ✅          | ✅              | Tier 1 parity                      |
| Prepare call hierarchy           | ✅          | ✅              | Tier 1 parity                      |
| Incoming calls                   | ✅          | ✅              | Tier 1 parity                      |
| Outgoing calls                   | ✅          | ✅              | Tier 1 parity                      |
| Code actions                     | ❌          | ✅              | Tier 2 — auto-fix suggestions      |
| Apply workspace edit             | ❌          | ✅              | Tier 2 — agent applies code actions|
| Rename symbol                    | ❌          | ✅              | Tier 2 — cross-file rename         |
| Format document                  | ❌          | ✅              | Tier 2 — whole-file formatting     |
| Format range                     | ❌          | ✅              | Tier 2 — selection formatting      |
| Inlay hints                      | ❌          | ✅              | Tier 2 — type annotations          |
| Semantic tokens                  | ❌          | ✅              | Tier 2 — deep symbol understanding |
| Signature help                   | ❌          | ✅              | Tier 2 — function parameter hints  |
| Type hierarchy                   | ❌          | ✅              | Tier 2 — super/subtypes            |
| Pull diagnostics (document)      | ❌          | ✅              | Tier 2 — LSP 3.17                  |
| Pull diagnostics (workspace)     | ❌          | ✅              | Tier 2 — LSP 3.17                  |
| Linked editing range             | ❌          | ✅              | Tier 2 — rename-as-you-type mirror |
| LLM-enriched diagnostics         | ❌          | ✅              | Tier 3 — EdgeCrab unique           |
| LLM-guided code action selection | ❌          | ✅              | Tier 3 — EdgeCrab unique           |
| Workspace-wide type error scan   | ❌          | ✅              | Tier 3 — EdgeCrab unique           |

**Score when complete**: EdgeCrab 24 / Claude Code 9.

---

## Design Principles

### First Principles

Strip away assumptions. What must be true for an AI agent to reason about code using LSP?

1. **Files must be open in the server's logical document context** → document sync layer required
2. **The agent must know which server covers which file** → routing / multi-server manager required
3. **The server process must be alive** → lifecycle management required
4. **Results must be interpretable by a language model** → serialization / rendering layer required
5. **No tool call must block the executor thread** → `async` throughout required

These five axioms drive every structural decision.

### DRY

- One `DocumentSyncGuard` RAII type manages open/close for ALL operations — no tool duplicates sync logic
- One `PositionEncoder` converts filesystem offsets ↔ LSP `Position` — no duplication
- One `LspServerPool` spawns, monitors, and restarts server processes — not replicated per tool
- One `CapabilityCheck` macro gates operations — no copy-paste guards per tool

### SOLID

| Principle | Application |
|-----------|-------------|
| **S** — Single Responsibility | `LspServerManager` manages processes; `DocumentSyncLayer` manages open docs; `DiagnosticCache` caches pushes; `CapabilityRouter` dispatches operations — four classes, four responsibilities |
| **O** — Open/Closed | New operations = new `ToolHandler` + `inventory::submit!`. Zero modification to core |
| **L** — Liskov Substitution | Any `LspClientHandle` impl (real or mock) can be substituted. Tests use `MockLspHandle` |
| **I** — Interface Segregation | Tools receive `LspToolContext` — a minimal subset of `ToolContext`. No tool sees the whole application state |
| **D** — Dependency Inversion | Tools depend on `Arc<dyn LspClientHandle>`, not on `ConcreteServerProcess` |

---

## Crate Design

A new workspace crate `edgecrab-lsp` is introduced (see Architecture doc for rationale).

```
crates/
  edgecrab-lsp/
    Cargo.toml
    src/
      lib.rs
      manager.rs         ← LspServerManager: process pool + routing
      sync.rs            ← DocumentSyncLayer: open/change/close/save
      diagnostics.rs     ← DiagnosticCache: push-model cache
      capability.rs      ← CapabilityRouter + CapabilityCheck macro
      position.rs        ← PositionEncoder: offset ↔ LspPosition
      enrichment.rs      ← LLM enrichment of LSP results
      tools/
        mod.rs
        goto_definition.rs
        find_references.rs
        hover.rs
        document_symbols.rs
        workspace_symbols.rs
        goto_implementation.rs
        call_hierarchy.rs
        code_actions.rs
        rename.rs
        format.rs
        inlay_hints.rs
        semantic_tokens.rs
        signature_help.rs
        type_hierarchy.rs
        diagnostics_pull.rs
        enriched_diagnostics.rs
```

---

## Document Index

| Document | Description |
|----------|-------------|
| [architecture.md](./architecture.md) | ASCII diagrams, crate design, async-lsp integration, process lifecycle |
| [operations.md](./operations.md) | All 20+ operations with Rust code examples |
| [adr/ADR-001-async-lsp-vs-tower-lsp.md](./adr/ADR-001-async-lsp-vs-tower-lsp.md) | Library selection |
| [adr/ADR-002-separate-crate.md](./adr/ADR-002-separate-crate.md) | New crate vs edgecrab-tools |
| [adr/ADR-003-document-sync-strategy.md](./adr/ADR-003-document-sync-strategy.md) | Full vs incremental sync |
| [adr/ADR-004-server-lifecycle.md](./adr/ADR-004-server-lifecycle.md) | Process management |
| [adr/ADR-005-diagnostic-model.md](./adr/ADR-005-diagnostic-model.md) | Push vs pull diagnostics |
| [adr/ADR-006-llm-enrichment.md](./adr/ADR-006-llm-enrichment.md) | LLM-enriched diagnostics arch |
| [roadblocks.md](./roadblocks.md) | Honest blockers with mitigations |

---

## Cargo Changes Required

```toml
# workspace Cargo.toml — [workspace.dependencies]
async-lsp  = { version = "0.2.3", features = ["omni-trait", "tokio"] }
lsp-types  = { version = "0.97.0", features = ["proposed"] }

# workspace Cargo.toml — [workspace.members]  (addition)
"crates/edgecrab-lsp",
```

```toml
# crates/edgecrab-lsp/Cargo.toml
[package]
name    = "edgecrab-lsp"
version = "0.1.0"
edition.workspace = true

[dependencies]
async-lsp      = { workspace = true }
lsp-types      = { workspace = true }
tokio          = { workspace = true }
async-trait    = { workspace = true }
serde          = { workspace = true }
serde_json     = { workspace = true }
dashmap        = { workspace = true }
anyhow         = { workspace = true }
thiserror      = { workspace = true }
tracing        = { workspace = true }
url            = "2"

edgecrab-types    = { path = "../edgecrab-types" }
edgecrab-security = { path = "../edgecrab-security" }
edgecrab-tools    = { path = "../edgecrab-tools" }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util"] }
```
