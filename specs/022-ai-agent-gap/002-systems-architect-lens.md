# 002 — Systems Architect Lens (Re-assessed)

**Authority:** [000-code-is-law.md](000-code-is-law.md)  
**Date:** 2026-07-19

---

## 1. Shapes

### EdgeCrab — compile-time crate DAG (17 crates)

```text
cli | gateway | acp | proxy | migrate | sdk*
              │
           edgecrab-core   (Agent, execute_loop, goals, kanban, oauth, otel)
          ┌───┼───┐
      tools  state  security
          │
        types
```

**Property:** dependency direction is enforceable at compile time.  
**Cost:** slower long-tail extension; PRs span workspace.

### Hermes — monorepo + runtime plugins

```text
cli / hermes_cli / ui-tui / apps/desktop / web
        │
   run_agent + agent/* + tools/* + gateway/*
        │
   plugins/{platforms,memory,model-providers,observability,...}
```

**Property:** add Teams/LINE/mem0 without rebuild.  
**Cost:** soft boundaries; three UI stacks; optional-dep hell.

---

## 2. Runtime topologies

| Topology | EdgeCrab (law) | Hermes (law) |
|----------|----------------|--------------|
| TUI | ratatui in-process | Ink + `tui_gateway` (UI/compute split) |
| Gateway | `edgecrab-gateway` 17 adapters | gateway + 20 plugin platforms |
| ACP | `edgecrab-acp` | `acp_adapter` |
| Provider proxy | `edgecrab-proxy` (OpenAI wire) | portal / proxy patterns |
| Embed | Rust/Node/Python/WASM SDKs | process-first |
| Desktop/Web | — | `apps/desktop`, `web/` |
| Cron | `edgecrab-cron` | `cron/` + blueprints |

**Architect takeaway:** comparing only CLI understates Hermes product surface and EdgeCrab embed surface.

---

## 3. Module quality (law)

| Concern | EC owner | Size | Maturity |
|---------|----------|------|----------|
| Loop | `conversation.rs` | 8051 | Dense; partial extract |
| Prologue | `turn_prologue.rs` | 39 | **Stub** |
| Epilogue | `turn_epilogue.rs` | 734 | Real |
| Pre-dispatch | `turn_dispatch_policy.rs` | 401 | Real, sophisticated |
| Failover | `failover.rs` | 854 | Real |
| Compression | `compression.rs` | 2806 | Real |
| Prompt | `prompt_builder.rs` | 4451 | Large but owned |
| Spill | `artifact_spill.rs` | 913 | Real |
| Goals | `goals/*` | ~1k+ | Real SQLite |

| Concern | H owner | Size | Maturity |
|---------|---------|------|----------|
| Loop | `conversation_loop.py` | 5562 | Focused |
| Prologue | `turn_context.py` | 623 | **Real** |
| Finalizer | `turn_finalizer.py` | 546 | Real |
| Executor | `tool_executor.py` | 1801 | Real concurrent |
| Classifier | `error_classifier.py` | 1621 | Deepest |
| Compressor | `context_compressor.py` | 3486 | Deep |
| Credential pool | `credential_pool.py` | 2459 | Deep |
| Curator | `curator.py` | 2016 | Deep |

**Architect debt EC:** finish prologue extraction; freeze net-new logic landing in `conversation.rs`.  
**Architect debt H:** keep extracting from `run_agent` / loop; manage multi-UI.

---

## 4. State model

| | EdgeCrab | Hermes |
|-|----------|--------|
| Home | `~/.edgecrab` / `EDGECRAB_HOME` | `~/.hermes` |
| Sessions | SQLite WAL + FTS5 | SQLite + rich lifecycle |
| Lineage | `parent_session_id` **present** | `parent_session_id` + compression walk |
| Goals | first-class tables | goals + curator state |
| Processes | per-agent ProcessTable | process_registry + notify |

---

## 5. Recommendations

| Decision | Action | AE |
|----------|--------|-----|
| Desktop clone | DEFER | product |
| Platform plugin ABI | design only if webhook/MCP fails | AE10 |
| Prologue real extract | P0 structure | maintainability |
| Keep security crate | forever | AE7 |
| Session lineage UX | use existing `parent_session_id` in product flows | AE2 |

---

## 6. Scorecard

| Dimension | Score |
|-----------|-------|
| Compile-time modularity | **EC** |
| Runtime extensibility | **H** |
| Multi-surface product | **H** |
| Embeddability | **EC** |
| Harness file structure | **H** (prologue) |
| Pre-dispatch policy structure | **EC** |
| Security isolation | **EC** |
| Single-binary ops | **EC** |
