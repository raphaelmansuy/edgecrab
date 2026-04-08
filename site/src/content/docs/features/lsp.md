---
title: Language Server Protocol
description: Semantic code navigation, rename, formatting, diagnostics, and code actions via EdgeCrab's dedicated edgecrab-lsp crate.
sidebar:
  order: 5
---

EdgeCrab includes a dedicated `edgecrab-lsp` crate and a first-class `lsp` toolset. This gives the agent semantic code intelligence instead of relying only on text search.

## Why It Matters

Plain grep can find strings. It cannot reliably answer:

- Where is this symbol actually defined?
- Which references are semantic references instead of comments or strings?
- Can this rename be applied safely across files?
- What fixes does the language server already know how to apply?
- What type errors exist right now in the workspace?

The LSP subsystem answers those questions directly from the language server.

## Tool Surface

EdgeCrab exposes 25 LSP operations.

### Claude-Parity Navigation

- `lsp_goto_definition`
- `lsp_find_references`
- `lsp_hover`
- `lsp_document_symbols`
- `lsp_workspace_symbols`
- `lsp_goto_implementation`
- `lsp_call_hierarchy_prepare`
- `lsp_incoming_calls`
- `lsp_outgoing_calls`

### EdgeCrab Extensions

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

## Built-In Language Coverage

Default server entries cover Rust, TypeScript, JavaScript, Python, Go, C, C++, Java, C#, PHP, Ruby, Bash, HTML, CSS, and JSON.

## Discoverability

EdgeCrab does not rely on the model guessing that these tools exist.

- The `core` and `coding` toolset aliases expand to include `lsp`
- `CORE_TOOLS` and `ACP_TOOLS` both expose the full `lsp_*` surface
- The system prompt adds LSP-first guidance when LSP tools are present

That prompt guidance tells the agent to prefer definitions, references, diagnostics, rename, formatting, and code actions before falling back to grep-style search.

## Configuration

Add or override servers in `~/.edgecrab/config.yaml`:

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
      env: {}
      initialization_options: ~
    python:
      command: pylsp
      args: []
      file_extensions: ["py"]
      language_id: python
      root_markers: ["pyproject.toml", "setup.py", "requirements.txt"]
      env: {}
      initialization_options: ~
```

## How the Agent Uses It

For a semantic coding task, the intended flow is:

1. Resolve the right server for the file
2. Open and sync the document in LSP context
3. Query definitions, references, hover, symbols, or diagnostics
4. Apply semantic actions like rename, format, or code action where supported
5. Fall back to ordinary file tools only when the language is unsupported or the task is purely textual

## Verification

The feature is covered by:

- prompt-builder tests for LSP guidance injection
- alias tests proving `core` and `coding` include `lsp`
- surface tests proving EdgeCrab exposes more than the 9-operation Claude baseline
- end-to-end integration tests against a mock LSP server

That is the standard for this feature: code-backed behavior, not marketing copy.
