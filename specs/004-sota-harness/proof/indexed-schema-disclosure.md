# Proof: Indexed Schema Disclosure (July 2026)

## Law

Progressive disclosure for tool schemas:

1. **Hot wire** — at most five CORE tools + `tool_search` (compact schemas).
2. **Deferred discovery** — no per-tool name dump in the system prompt; dynamic zone carries **count + toolset categories** only.
3. **Dictionary** — `tool_search` (`query` preferred, or exact `tool_names`) materializes schemas onto the wire.
4. **Cache** — do not rebuild `cached_system_prompt` after materialize; invent recovery uses message/`tool_search`, not prompt mutation.
5. **Default** — `ToolSchemaMode::default()` and `parse` empty/unknown → `Indexed` (matches config / `AgentConfig`).

## Code anchors

| Piece | Path |
|-------|------|
| Hot set (5) | `crates/edgecrab-tools/src/toolsets.rs` — `INDEXED_HOT_TOOLS` |
| Hot + `tool_search` | `crates/edgecrab-tools/src/tool_schema_index.rs` — `is_hot_tool` |
| Deferred summary | `format_deferred_index(count, categories)` |
| Stable law | `crates/edgecrab-core/src/prompt_builder.rs` — `INDEXED_TOOL_GUIDANCE` |
| Inject site | `crates/edgecrab-core/src/conversation.rs` (session start only) |
| Mode default | `crates/edgecrab-tools/src/schema_mode.rs` |

## Hot set

`read_file`, `write_file`, `patch`, `search_files`, `terminal` (+ `tool_search` via `is_hot_tool`).

Create-path swap (2026-07-17, session `8d74ce9c` / game001): `web_search` deferred; `write_file` hot so create tasks do not thrash on `skill_manage` / heredoc before discovery.

## Related

Unknown-tool invent → `CallToolFirst(tool_search)`: `proof/tool-call-closure-invariant.md`.
Create-path thrash: `proof/create-path-disclosure.md`.

## Verification

```bash
cargo test -p edgecrab-tools --lib tool_schema_index
cargo test -p edgecrab-tools --lib schema_mode
cargo test -p edgecrab-core --lib prompt_builder
cargo test -p edgecrab-core --lib context_budget
cargo test -p edgecrab-core --test unknown_tool_recovery_e2e
cargo test -p edgecrab-core --test context_cache_efficiency_e2e
cargo test -p edgecrab-core --test indexed_tool_disclosure_e2e
```
