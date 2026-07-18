# Proof: Tool Progressive Load (July 2026)

Builds on [indexed-schema-disclosure.md](./indexed-schema-disclosure.md).

## Law

1. **DRY materialize** — `materialize_tool_names` is the single write path for wire promotion (tool_search + prefetch).
2. **Pack load** — `tool_search(toolset=…)` materializes up to 8 deferred tools from a registered toolset.
3. **Auto mode** — `schema_mode: auto` (product default) → Compact when enabled tools ≤ 14, else Indexed.
4. **Local hot fidelity** — local providers keep full schemas for hot/`tool_search`; materialized long-tail stays compact.
5. **Turn-start prefetch** — Indexed only: BM25 top-3 from user text silently materializes before first LLM call (no system-prompt mutation).
6. **Cache law** — prefetch and materialize never rebuild `cached_system_prompt`.

## Code anchors

| Piece | Path |
|-------|------|
| Materialize helper | `crates/edgecrab-tools/src/tool_schema_index.rs` |
| toolset / query / names | `crates/edgecrab-tools/src/tools/tool_search.rs` |
| Auto resolve | `crates/edgecrab-tools/src/schema_mode.rs` — `resolve_effective_schema_mode` |
| Prefetch | `crates/edgecrab-tools/src/tool_search_bm25.rs` — `prefetch_tools_for_user_message` |
| Session wire | `crates/edgecrab-core/src/conversation.rs` |
| E2E | `crates/edgecrab-core/tests/indexed_tool_disclosure_e2e.rs` |

## Verification

```bash
cargo test -p edgecrab-tools --lib tool_search
cargo test -p edgecrab-tools --lib tool_schema_index
cargo test -p edgecrab-tools --lib schema_mode
cargo test -p edgecrab-core --test indexed_tool_disclosure_e2e
cargo test -p edgecrab-core --test unknown_tool_recovery_e2e
cargo test -p edgecrab-core --test context_cache_efficiency_e2e
```
