
# Repo Map / Code Intelligence (Deep Dive)

EdgeCrab provides **code intelligence and context compression** features, but does not (yet) have a dedicated `repo_map` module like EdgeCode. Instead, it uses:

- **Tree-sitter**: Used for code parsing and symbol extraction (see logs for integration updates).
- **Context compression**: [`compression.rs`](../../crates/edgecrab-core/src/compression.rs) summarizes/prunes old messages, including tool outputs and file content, to fit within the model's context window.
- **PromptBuilder**: [`prompt_builder.rs`](../../crates/edgecrab-core/src/prompt_builder.rs) assembles the system prompt from ~12 sources (identity, context files, skills, memory, etc.), but does not inject a full repo map/SymbolGraph by default.

## Code-Intel Features

- **Tool output pruning**: Large file/tool results are replaced with placeholders to save context.
- **LLM-powered summary**: Old context is summarized with a structured template (goal, constraints, progress, files, next steps, etc.).
- **Orphan sanitization**: Ensures tool_call/tool_result pairs are consistent after compression.
- **Protects head/tail**: Always preserves first N and last N messages for continuity.

## Tree-sitter & Symbol Extraction

- Tree-sitter is referenced in logs and is used for code parsing, but there is no dedicated SymbolGraph or repo map generator module as in EdgeCode.
- Symbol extraction is used for context compression and possibly for future code-intel features.

## Limitations & TODOs

- **No dedicated repo map**: There is no `repo_map` or SymbolGraph module; context is compressed but not structured as a project map.
- **No PageRank or symbol ranking**: Unlike EdgeCode, there is no PageRank or symbol importance ranking for code navigation.
- **No session-injected repo map**: The session context does not include a full project structure map by default.
- **Logs**: See `logs/2025-07-15-16-00-beastmode-docs-013-015-review.md` for tree-sitter/code-intel updates.

## Key Code & Docs

- [compression.rs](../../crates/edgecrab-core/src/compression.rs)
- [prompt_builder.rs](../../crates/edgecrab-core/src/prompt_builder.rs)
- [logs/2025-07-15-16-00-beastmode-docs-013-015-review.md](../../logs/2025-07-15-16-00-beastmode-docs-013-015-review.md)

---
**TODOs:**
- Implement a dedicated `repo_map`/SymbolGraph module for project structure awareness
- Inject repo map into session context for LLM
- Add symbol ranking (PageRank, usage frequency) for code navigation
