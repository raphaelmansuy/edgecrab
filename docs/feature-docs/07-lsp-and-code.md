# LSP and Semantic Coding

EdgeCrab now ships a dedicated `edgecrab-lsp` workspace crate and a first-class `lsp` toolset. The goal is not just Claude Code parity. The implementation deliberately exceeds the common 9-operation baseline with a broader semantic surface and executable regressions that enforce it.

## What Ships

- Claude-parity navigation:
  - `lsp_goto_definition`
  - `lsp_find_references`
  - `lsp_hover`
  - `lsp_document_symbols`
  - `lsp_workspace_symbols`
  - `lsp_goto_implementation`
  - `lsp_call_hierarchy_prepare`
  - `lsp_incoming_calls`
  - `lsp_outgoing_calls`
- EdgeCrab semantic edit and analysis extensions:
  - `lsp_code_actions`
  - `lsp_apply_code_action`
  - `lsp_rename`
  - `lsp_format_document`
  - `lsp_format_range`
  - `lsp_inlay_hints`
  - `lsp_semantic_tokens`
  - `lsp_signature_help`
  - `lsp_type_hierarchy_prepare`
  - `lsp_supertypes`
  - `lsp_subtypes`
  - `lsp_diagnostics_pull`
  - `lsp_linked_editing_range`
  - `lsp_enrich_diagnostics`
  - `lsp_select_and_apply_action`
  - `lsp_workspace_type_errors`

## Architecture

- `crates/edgecrab-lsp/src/manager.rs`
  - Multi-server lifecycle and routing by file extension / workspace root
- `crates/edgecrab-lsp/src/protocol.rs`
  - Async stdio JSON-RPC client for LSP requests and notifications
- `crates/edgecrab-lsp/src/sync.rs`
  - Shared document open / change / close logic
- `crates/edgecrab-lsp/src/diagnostics.rs`
  - Diagnostic caching and workspace aggregation
- `crates/edgecrab-lsp/src/tools.rs`
  - Inventory-registered `lsp_*` tool handlers exposed to the registry

The `edgecrab-core` prompt builder now injects explicit LSP guidance when these tools are available. That matters because discoverability is not automatic. The agent must be told to prefer semantic operations over grep when the language server can answer precisely.

## Configuration

Top-level config now includes:

```yaml
lsp:
  enabled: true
  file_size_limit_bytes: 10000000
  servers:
    rust:
      command: rust-analyzer
      args: []
      file_extensions: ["rs"]
      language_id: rust
      root_markers: ["Cargo.toml", "rust-project.json"]
```

Built-in server definitions cover Rust, TypeScript, JavaScript, Python, Go, C, C++, Java, C#, PHP, Ruby, Bash, HTML, CSS, and JSON.

## Exposure and Discoverability Guarantees

- `CORE_TOOLS` includes the full `lsp_*` surface
- `ACP_TOOLS` includes the full `lsp_*` surface for editor integrations
- `core` alias expands to `lsp`
- `coding` alias expands to `lsp`
- Prompt guidance tells the agent to use LSP before text search for semantic tasks

## Verification

- Unit and prompt regressions in `crates/edgecrab-core/src/prompt_builder.rs`
- Surface regressions in `crates/edgecrab-tools/tests/core_tools_surface_e2e.rs`
- Alias regressions in `crates/edgecrab-tools/src/toolsets.rs`
- End-to-end LSP integration in `crates/edgecrab-lsp/tests/lsp_tools_integration.rs`

This gives EdgeCrab executable proof that it exceeds the 9-operation Claude baseline on LSP coverage.
