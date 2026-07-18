# Proof: Progressive-disclosure decisions (hot set / prompt refresh / examples)

First-principles verdicts from the July 2026 brainstorm. Code is law.

## Decisions

| Idea | Decision | Status in code |
|------|----------|----------------|
| Change `INDEXED_HOT_TOOLS` membership | **Shipped (evidence)** — swap `web_search`→`write_file` (session `8d74ce9c` / game001) | Hot: `read_file`, `write_file`, `patch`, `search_files`, `terminal` |
| Mid-session system-prompt deferred-count refresh | **Skip** — violates cache law | No `cached_system_prompt` mutation on materialize |
| Anthropic-native `defer_loading` as core | **Skip** — multi-provider basin uses client Indexed mode | Not implemented as primary path |
| Materialize-time `input_examples` | **Ship** — provider-agnostic | `tool_input_examples.rs` + `MaterializeOutcome` + `tool_search` JSON |
| Prefetch covers create-path without hot `write_file` | **Falsified** — prefetch promoted `skill_manage` while `write_file` stayed deferred | Hot-set swap + create-intent prefetch bias |

## Why skip / defer

1. **Hot set size:** Stay at 5. Create-path evidence swapped membership; do not grow past 5 without new meter proof.
2. **Prompt refresh:** Wire `tools[]` + tool_search results are truth; rewriting deferred count in the system prompt busts prefix cache for cosmetic accuracy.
3. **Native `defer_loading`:** Helps Anthropic only; EdgeCrab defaults include Ollama/LM Studio. Client Indexed mode already implements the same law for all providers.

## What shipped (input_examples)

- Side map [`tool_input_examples.rs`](../../../crates/edgecrab-tools/src/tool_input_examples.rs) (not Anthropic-only wire fields).
- Attached on materialize in [`materialize_tool_names`](../../../crates/edgecrab-tools/src/tool_schema_index.rs).
- Returned in `tool_search` result as `input_examples` + hint (message path — cache-safe).
- Cap: `MAX_INPUT_EXAMPLES_PER_TOOL` = 3.
- Not injected onto turn-1 hot wire (keeps compaction).

## Verification

```bash
cargo test -p edgecrab-tools --lib tool_input_examples
cargo test -p edgecrab-tools --lib materialize_write_file
cargo test -p edgecrab-tools --lib materializes_deferred_tool
cargo test -p edgecrab-core --test indexed_tool_disclosure_e2e
```

## Related

- [indexed-schema-disclosure.md](./indexed-schema-disclosure.md)
- [tool-progressive-load.md](./tool-progressive-load.md)
- [create-path-disclosure.md](./create-path-disclosure.md)
