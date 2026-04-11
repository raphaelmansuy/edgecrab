# Crate Dependency Graph 🦀

> **Verified against:** `Cargo.toml` (workspace) · each crate's `Cargo.toml`

---

## Why the dependency graph matters

A dependency graph is not just bookkeeping — it is an enforced architectural
constraint. If `edgecrab-tools` could freely import `edgecrab-core`, tools could
spawn agents that spawn more agents in infinite recursion, and a change to the
agent loop would require rebuilding *all* 10 crates. The DAG below shows the
actual structure: every arrow is intentional, and violating it causes a compile
error.

Understanding the graph answers: *"where does this new code go?"*

---

## Full dependency graph

```
  edgecrab-cli ──────────────────────────────────────────────┐
  (binary entry point — depends on everything)               │
        │                                                     │
        ├──► edgecrab-gateway ──────────────────────┐        │
        │         │                                 │        │
        ├──► edgecrab-acp ──────────────────┐       │        │
        │                                  │       │        │
        │                                  └──►  edgecrab-core ◄─┘
        │                                              │
        │                    ┌─────────────────────────┼──────────────┐
        │                    │                         │              │
        │                    ▼                         ▼              ▼
        │            edgecrab-tools           edgecrab-state  edgecrab-security
        │            (ToolRegistry, 91 tools) (SQLite WAL)    (CommandScanner)
        │                    │                         │              │
        │                    └─────────────────────────┼──────────────┘
        │                                              │
        │                                              ▼
        │                                      edgecrab-types
        │                                      (leaf — no internal deps)
        │
        ├──► edgecrab-cron ──────────────────────────► edgecrab-types
        │    (also used by edgecrab-tools)
        │
        └──► edgecrab-migrate ─────────────────────────────────────────►
             edgecrab-types, edgecrab-state
```

---

## Dependency table

| Crate | Internal deps | Notes |
|---|---|---|
| `edgecrab-types` | _(none)_ | Leaf. Every crate imports this. `#![deny(clippy::unwrap_used)]` |
| `edgecrab-security` | `edgecrab-types` | No async, no LLM calls. Stateless checks. |
| `edgecrab-state` | `edgecrab-types` | Only crate that owns raw SQL. |
| `edgecrab-cron` | `edgecrab-types` | Standalone schedule library. |
| `edgecrab-tools` | `edgecrab-types`, `edgecrab-state`, `edgecrab-security` | Defines `SubAgentRunner` trait to avoid importing core. |
| `edgecrab-core` | `edgecrab-types`, `edgecrab-tools`, `edgecrab-state`, `edgecrab-security` | Implements `SubAgentRunner`. Owns the agent loop. |
| `edgecrab-acp` | `edgecrab-core`, `edgecrab-types` | Thin JSON-RPC 2.0 stdio wrapper. |
| `edgecrab-gateway` | `edgecrab-core`, `edgecrab-tools`, `edgecrab-types`, `edgecrab-state`, `edgecrab-security`, `edgecrab-cron` | Widest import set; builds the full messaging stack. |
| `edgecrab-cli` | all crates | Binary entry point; pulls everything. |
| `edgecrab-migrate` | `edgecrab-types`, `edgecrab-state` | One-time migration helper. |

---

## Solving the tools ↔ core circular dependency

Tools need to spawn sub-agents. Sub-agents live in `edgecrab-core`. The naive
import creates a cycle:

```
  edgecrab-core  ──► edgecrab-tools  ──►  edgecrab-core   ✗ CYCLE
```

The solution is **trait-object inversion**:

```
  Step 1: edgecrab-tools defines the contract

      pub trait SubAgentRunner: Send + Sync {
          async fn run_task(&self, goal, ...) -> Result<SubAgentResult, String>;
      }

  Step 2: edgecrab-core implements it

      impl SubAgentRunner for CoreSubAgentRunner { ... }

  Step 3: Agent passes Arc<dyn SubAgentRunner> into ToolContext

      ctx.sub_agent_runner.run_task("do X")  // tools never import core

  Result:

      edgecrab-tools ─► edgecrab-types   (SubAgentRunner trait)
      edgecrab-core  ─► edgecrab-tools   (ToolRegistry, ToolHandler)
                     implements SubAgentRunner
                                       ✓ no cycle
```

The same pattern applies to `GatewaySender`:

```
  edgecrab-tools   defines   GatewaySender trait
  edgecrab-gateway implements GatewaySender
  edgecrab-core    holds     RwLock<Option<Arc<dyn GatewaySender>>>
```

🦀 *Think of the crab's claw (tools) as attached to the body (core) by tendons
(trait objects). The claw can move independently — it does not need to import the
entire nervous system to do its job.*

---

## Compile-time tool registration

Tools do not appear in a hand-maintained list. The
[`inventory`](https://docs.rs/inventory) crate enables compile-time plugin
collection:

```rust
// In any tool file inside edgecrab-tools:
inventory::submit! {
    &ReadFileTool as &dyn ToolHandler
}

// ToolRegistry::new() in registry.rs:
for handler in inventory::iter::<&dyn ToolHandler> {
    tools.insert(handler.name(), *handler);
}
```

**Adding a new tool:** implement `ToolHandler` + call `inventory::submit!` +
recompile. No list, no match arm, no registration function to update.

**Reference:** [`inventory` crate docs](https://docs.rs/inventory/latest/inventory/)

---

## Where to put new code

| Scenario | Target crate |
|---|---|
| New shared type or enum | `edgecrab-types` |
| New path / URL / command / injection check | `edgecrab-security` |
| New SQL query or schema migration | `edgecrab-state` |
| New cron schedule format | `edgecrab-cron` |
| New tool | `edgecrab-tools` |
| Loop behaviour, prompt strategy, compression | `edgecrab-core` |
| New CLI subcommand or TUI feature | `edgecrab-cli` |
| New messaging platform | `edgecrab-gateway` |
| New editor protocol | `edgecrab-acp` |

---

## Tips

> **Tip: Verify no core import crept into tools with `cargo tree`.**
> ```sh
> cargo tree -p edgecrab-tools | grep edgecrab-core
> # Must print nothing
> ```

> **Tip: Keep `edgecrab-types` as lean as possible.**
> Any dependency you add here propagates to all 10 crates. At time of writing the
> only internal dep it allows is `edgequake-llm` for type bridging.

> **Tip: `edgecrab-state` is the only crate allowed to run SQL.**
> If you need a new query, add a method to `SessionDb` — do not add `rusqlite` to
> another crate's `Cargo.toml`.

---

## FAQ

**Q: Why does `edgecrab-gateway` depend on `edgecrab-cron`?**
The gateway serves cron-triggered messages as a delivery target. The `Deliver::Platform`
variant in `edgecrab-cron` names a gateway channel. The gateway resolves that name
to an actual adapter and sends the cron output.

**Q: Can I make a crate that depends on both `edgecrab-core` and `edgecrab-gateway`?**
Yes — `edgecrab-cli` already does. These are not peers that conflict; they are
layers that compose.

---

## Cross-references

- System layers overview → [System Architecture](./001_system_architecture.md)
- Concurrency details → [Concurrency Model](./003_concurrency_model.md)
- Trait objects in tools → [Tool Registry](../004_tools_system/001_tool_registry.md)
