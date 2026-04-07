# EdgeCrab LSP — Operations Reference

Each operation is listed with: tier, inputs, outputs, capability guard, and a concise Rust
implementation sketch. All tool schemas are expressed as JSON Schema (OpenAI function-calling
format) for compatibility with the `ToolHandler` trait.

---

## Tier 1 — Parity with Claude Code (9 operations)

### `lsp_goto_definition`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.definition_provider` |
| LSP method | `textDocument/definition` |
| Returns | List of `{ uri, range }` |

```rust
async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
    let (file, line, col) = extract_position_args(&args)?;
    let (socket, _guard) = prepare_lsp(ctx, &file).await?;

    let params = GotoDefinitionParams {
        text_document_position_params: tdp(file_url(&file)?, position(line, col)),
        ..Default::default()
    };
    let result: Option<GotoDefinitionResponse> =
        socket.request::<GotoDefinition>(params).await?;

    Ok(match result {
        None => json!({ "found": false }),
        Some(GotoDefinitionResponse::Scalar(loc)) => json!([format_location(&loc)]),
        Some(GotoDefinitionResponse::Array(locs)) => json!(locs.iter().map(format_location).collect::<Vec<_>>()),
        Some(GotoDefinitionResponse::Link(links)) => json!(links.iter().map(format_link).collect::<Vec<_>>()),
    })
}
```

**Schema**:
```json
{
  "type": "object",
  "properties": {
    "file":   { "type": "string" },
    "line":   { "type": "integer" },
    "column": { "type": "integer" }
  },
  "required": ["file", "line", "column"]
}
```

---

### `lsp_find_references`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.references_provider` |
| LSP method | `textDocument/references` |
| Returns | List of `{ uri, range }` |

```rust
let params = ReferenceParams {
    text_document_position: tdp(file_url(&file)?, position(line, col)),
    context: ReferenceContext { include_declaration: true },
    ..Default::default()
};
let refs: Option<Vec<Location>> = socket.request::<References>(params).await?;
let out = refs.unwrap_or_default().iter().map(format_location).collect::<Vec<_>>();
Ok(json!({ "count": out.len(), "locations": out }))
```

**Schema**: same as `lsp_goto_definition` plus optional `include_declaration: bool`.

---

### `lsp_hover`

See Architecture doc §4 for full implementation. Summary:

- Calls `textDocument/hover`
- Returns `{ found, content, range }` where `content` is the markdown/plain-text hover string

---

### `lsp_document_symbols`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.document_symbol_provider` |
| LSP method | `textDocument/documentSymbol` |
| Returns | Hierarchical symbol tree |

```rust
let params = DocumentSymbolParams {
    text_document: TextDocumentIdentifier { uri: file_url(&file)? },
    ..Default::default()
};
let result: Option<DocumentSymbolResponse> =
    socket.request::<DocumentSymbolRequest>(params).await?;

fn render_symbol(sym: &DocumentSymbol, depth: usize) -> Value {
    json!({
        "name":     sym.name,
        "kind":     format!("{:?}", sym.kind),
        "range":    format_range(&sym.range),
        "children": sym.children.as_deref().unwrap_or(&[])
                       .iter().map(|c| render_symbol(c, depth + 1)).collect::<Vec<_>>()
    })
}
```

---

### `lsp_workspace_symbols`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.workspace_symbol_provider` |
| LSP method | `workspace/symbol` |
| Returns | Flat list with `{ name, kind, location, container_name }` |

```rust
let params = WorkspaceSymbolParams {
    query: args["query"].as_str().unwrap_or("").to_string(),
    ..Default::default()
};
let result: Option<Vec<SymbolInformation>> =
    socket.request::<WorkspaceSymbol>(params).await?;
```

**Schema**:
```json
{
  "type": "object",
  "properties": {
    "query": { "type": "string", "description": "Symbol name prefix or fuzzy pattern" }
  },
  "required": ["query"]
}
```

---

### `lsp_goto_implementation`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.implementation_provider` |
| LSP method | `textDocument/implementation` |
| Returns | List of `{ uri, range }` (concrete implementations of interface/trait) |

Identical call pattern to `lsp_goto_definition` — substitute `GotoImplementation` request type.

---

### `lsp_call_hierarchy_prepare`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.call_hierarchy_provider` |
| LSP method | `textDocument/prepareCallHierarchy` |
| Returns | List of `CallHierarchyItem` |

```rust
let params = CallHierarchyPrepareParams {
    text_document_position_params: tdp(file_url(&file)?, position(line, col)),
    ..Default::default()
};
let items: Option<Vec<CallHierarchyItem>> =
    socket.request::<CallHierarchyPrepare>(params).await?;
```

---

### `lsp_incoming_calls`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.call_hierarchy_provider` |
| LSP method | `callHierarchy/incomingCalls` |
| Input | `CallHierarchyItem` (from `lsp_call_hierarchy_prepare`) |
| Returns | Callers with call ranges |

```rust
// The model passes back the item JSON it received from lsp_call_hierarchy_prepare.
let item: CallHierarchyItem = serde_json::from_value(args["item"].clone())?;
let params = CallHierarchyIncomingCallsParams { item, ..Default::default() };
let calls: Option<Vec<CallHierarchyIncomingCall>> =
    socket.request::<CallHierarchyIncomingCalls>(params).await?;
```

---

### `lsp_outgoing_calls`

Mirror of `lsp_incoming_calls` substituting `CallHierarchyOutgoingCalls`.

---

## Tier 2 — Exceeds Claude Code (12 operations)

### `lsp_code_actions`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.code_action_provider` |
| LSP method | `textDocument/codeAction` |
| Returns | List of `{ title, kind, is_preferred, command_or_edit }` |

```rust
let params = CodeActionParams {
    text_document: TextDocumentIdentifier { uri: file_url(&file)? },
    range: range_from_args(&args)?,
    context: CodeActionContext {
        diagnostics: vec![],  // can also pass cached diagnostics here
        only: None,
        trigger_kind: Some(CodeActionTriggerKind::INVOKED),
    },
    ..Default::default()
};
let result: Option<CodeActionResponse> =
    socket.request::<lsp_types::request::CodeActionRequest>(params).await?;

// Render for model: each action has a title and a resolvable edit
let actions = result.unwrap_or_default().iter().map(|item| match item {
    CodeActionOrCommand::CodeAction(a) => json!({
        "title":       a.title,
        "kind":        a.kind.as_ref().map(|k| k.as_str()),
        "preferred":   a.is_preferred,
        "has_edit":    a.edit.is_some(),
    }),
    CodeActionOrCommand::Command(c) => json!({
        "title":   c.title,
        "command": c.command,
    }),
}).collect::<Vec<_>>();
Ok(json!({ "actions": actions }))
```

---

### `lsp_apply_code_action`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.code_action_provider` |
| LSP method | Second call to `textDocument/codeAction` with resolve, then apply `WorkspaceEdit` |
| Returns | List of files modified + diff summary |

```rust
// Receive the action index from the model, resolve its WorkspaceEdit from the server.
let action: CodeAction = serde_json::from_value(args["action"].clone())?;

// Optionally resolve (lazy edit) if edit is not already embedded
let edit = if action.edit.is_some() {
    action.edit.unwrap()
} else {
    let resolved: CodeAction = socket.request::<ResolveCodeAction>(action).await?;
    resolved.edit.ok_or(LspError::EmptyEdit)?
};

// Apply the WorkspaceEdit
let changed = apply_workspace_edit(&edit, ctx.config.workspace_root())?;
Ok(json!({ "files_modified": changed }))
```

`apply_workspace_edit` iterates `edit.changes` (HashMap<Url, Vec<TextEdit>>) or
`edit.document_changes`, sorts edits in reverse order (bottom-up to preserve line indices),
and applies them with `similar` for a diff summary.

---

### `lsp_rename`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.rename_provider` |
| LSP methods | `textDocument/prepareRename` (optional validation) + `textDocument/rename` |
| Returns | Success flag, files modified, rename from→to |

```rust
// First validate
let prep_params = TextDocumentPositionParams {
    text_document: TextDocumentIdentifier { uri: file_url(&file)? },
    position: position(line, col),
};
let _valid: Option<PrepareRenameResponse> =
    socket.request::<PrepareRenameRequest>(prep_params).await?;

// Then rename
let new_name: String = args["new_name"].as_str()
    .ok_or_else(|| ToolError::InvalidArgs { tool: "lsp_rename".into(), message: "missing new_name".into() })?
    .to_string();

let params = RenameParams {
    text_document_position: TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: file_url(&file)? },
        position: position(line, col),
    },
    new_name,
    work_done_progress_params: Default::default(),
};
let edit: Option<WorkspaceEdit> = socket.request::<Rename>(params).await?;
let changed = edit.map(|e| apply_workspace_edit(&e, ctx.config.workspace_root()))
    .transpose()?
    .unwrap_or_default();

Ok(json!({ "renamed": !changed.is_empty(), "files_modified": changed }))
```

**Schema**:
```json
{
  "type": "object",
  "properties": {
    "file":     { "type": "string" },
    "line":     { "type": "integer" },
    "column":   { "type": "integer" },
    "new_name": { "type": "string" }
  },
  "required": ["file", "line", "column", "new_name"]
}
```

---

### `lsp_format_document`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.document_formatting_provider` |
| LSP method | `textDocument/formatting` |
| Returns | Formatted content written to disk, diff summary |

```rust
let params = DocumentFormattingParams {
    text_document: TextDocumentIdentifier { uri: file_url(&file)? },
    options: FormattingOptions {
        tab_size:       args["tab_size"].as_u64().unwrap_or(4) as u32,
        insert_spaces:  args["insert_spaces"].as_bool().unwrap_or(true),
        ..Default::default()
    },
    ..Default::default()
};
let edits: Option<Vec<TextEdit>> =
    socket.request::<Formatting>(params).await?;

if let Some(edits) = edits {
    let original = std::fs::read_to_string(&file)?;
    let formatted = apply_text_edits(&original, &edits)?;
    std::fs::write(&file, &formatted)?;
    Ok(json!({ "formatted": true, "changed": original != formatted }))
} else {
    Ok(json!({ "formatted": false }))
}
```

---

### `lsp_format_range`

Same as `lsp_format_document` but uses `textDocument/rangeFormatting` with an added `range`
argument and `DocumentRangeFormattingParams`. Capability guard:
`server_capabilities.document_range_formatting_provider`.

---

### `lsp_inlay_hints`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.inlay_hint_provider` (LSP 3.17) |
| LSP method | `textDocument/inlayHint` |
| Returns | List of `{ position, label, kind, tooltip }` |

```rust
use lsp_types::request::InlayHintRequest; // behind "proposed" feature flag

let params = InlayHintParams {
    text_document: TextDocumentIdentifier { uri: file_url(&file)? },
    range: range_from_args(&args)?,  // if not provided, use full file range
    work_done_progress_params: Default::default(),
};
let hints: Option<Vec<InlayHint>> = socket.request::<InlayHintRequest>(params).await?;

let out = hints.unwrap_or_default().iter().map(|h| json!({
    "position": { "line": h.position.line, "character": h.position.character },
    "label":    match &h.label {
        InlayHintLabel::String(s) => s.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().filter_map(|p| p.value.as_deref().map(|s| s.to_string())).collect::<String>(),
    },
    "kind":    h.kind.map(|k| format!("{:?}", k)),
})).collect::<Vec<_>>();
Ok(json!({ "hints": out }))
```

---

### `lsp_semantic_tokens`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.semantic_tokens_provider` |
| LSP method | `textDocument/semanticTokens/full` |
| Returns | Decoded list of `{ line, start, length, token_type, modifiers }` |

```rust
let params = SemanticTokensParams {
    text_document: TextDocumentIdentifier { uri: file_url(&file)? },
    ..Default::default()
};
let result: Option<SemanticTokensResult> =
    socket.request::<SemanticTokensFullRequest>(params).await?;

if let Some(SemanticTokensResult::Tokens(tokens)) = result {
    // Tokens are stored as relative offsets — decode them
    let decoded = decode_semantic_tokens(&tokens.data, &get_token_types(&state)?);
    Ok(json!({ "tokens": decoded }))
} else {
    Ok(json!({ "tokens": [] }))
}
```

`decode_semantic_tokens` converts the compact relative-offset encoding into absolute
(line, char) positions. This is a pure function that processes `Vec<SemanticToken>`.

---

### `lsp_signature_help`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.signature_help_provider` |
| LSP method | `textDocument/signatureHelp` |
| Returns | Active signature, parameter labels, documentation |

```rust
let result: Option<SignatureHelp> =
    socket.request::<SignatureHelpRequest>(SignatureHelpParams {
        context: None,
        text_document_position_params: tdp(file_url(&file)?, position(line, col)),
        ..Default::default()
    }).await?;

match result {
    None => Ok(json!({ "found": false })),
    Some(sh) => {
        let active = sh.active_signature.unwrap_or(0) as usize;
        let sig    = sh.signatures.get(active);
        Ok(json!({
            "found":      true,
            "label":      sig.map(|s| &s.label),
            "doc":        sig.and_then(|s| s.documentation.as_ref()).map(doc_to_string),
            "parameters": sig.map(|s| s.parameters.as_deref().unwrap_or(&[])
                                .iter().map(|p| json!({ "label": &p.label }))
                                .collect::<Vec<_>>()),
            "active_parameter": sh.active_parameter,
        }))
    }
}
```

---

### `lsp_type_hierarchy_prepare`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.type_hierarchy_provider` |
| LSP method | `textDocument/prepareTypeHierarchy` |
| Returns | List of `TypeHierarchyItem` |

Uses `lsp_types::request::TypeHierarchyPrepare` — behind `proposed` feature flag.

---

### `lsp_supertypes` / `lsp_subtypes`

Use `typeHierarchy/supertypes` and `typeHierarchy/subtypes` respectively.
Both accept a `TypeHierarchyItem` (from `lsp_type_hierarchy_prepare`).

---

### `lsp_diagnostics_pull`

| Field | Value |
|-------|-------|
| Capability guard | `server_capabilities.diagnostic_provider` (LSP 3.17) |
| LSP method | `textDocument/diagnostic` |
| Returns | List of `{ range, severity, code, message, source, related }` |

Two variants:
- `lsp_diagnostics_pull { "file": "..." }` — document diagnostics
- `lsp_diagnostics_pull { "workspace": true }` — workspace diagnostics (`workspace/diagnostic`)

This is the **pull model**. It complements the push model (publishDiagnostics notifications
stored in `DiagnosticCache`). Pull is preferred when the agent needs fresh diagnostics on
demand without waiting for server-triggered push.

```rust
// Document variant
let params = DocumentDiagnosticParams {
    text_document: TextDocumentIdentifier { uri: file_url(&file)? },
    identifier: None,
    previous_result_id: None,
    ..Default::default()
};
let result: DocumentDiagnosticReport =
    socket.request::<DocumentDiagnosticRequest>(params).await?;

let diags = match result {
    DocumentDiagnosticReport::Full(f)     => f.result.items,
    DocumentDiagnosticReport::Unchanged(_) => vec![],
};
Ok(json!({ "diagnostics": diags.iter().map(format_diagnostic).collect::<Vec<_>>() }))
```

---

## Tier 3 — EdgeCrab Unique (3 operations)

### `lsp_enrich_diagnostics`

Fetches current diagnostics (push cache or pull) for a file, then asks the auxiliary LLM
to explain each error in plain English. Returns explanations the model can directly act on.

```
Algorithm:
  1. Read DiagnosticCache for file (or pull if empty)
  2. For each error-severity diagnostic:
     a. Extract ±5 lines of source context via PositionEncoder
     b. Format: "ERROR [{code}] at line {L}: {message}\nContext:\n{source}"
  3. Batch-call auxiliary_llm.explain(batch)  (single LLM request per file)
  4. Return [{ original_diagnostic, explanation, suggested_fix }]
```

**Schema**:
```json
{
  "type": "object",
  "properties": {
    "file":           { "type": "string" },
    "severity_filter": {
      "type": "string",
      "enum": ["error", "warning", "all"],
      "default": "error"
    }
  },
  "required": ["file"]
}
```

---

### `lsp_select_and_apply_action`

Lists code actions for a range, asks the agent to choose the best one by title/description,
and applies the associated `WorkspaceEdit`. This creates a tight tool loop:

```
lsp_code_actions   → list of { index, title, kind }
model picks index
lsp_select_and_apply_action { action_index: N }   → applies and returns diff
```

---

### `lsp_workspace_type_errors`

Collects ALL diagnostics from the `DiagnosticCache` across all open files, filters to
`DiagnosticSeverity::ERROR`, and returns a deduplicated, file-sorted list. Enables the
agent to assess project health in one call instead of file-by-file querying.

```rust
async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
    let lsp = ctx.lsp_manager().ok_or_else(|| ...)?;
    let all = lsp.diag_cache.all_errors();  // Vec<(Url, Diagnostic)>
    let mut by_file: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for (uri, diag) in all {
        by_file.entry(uri.path().to_string())
            .or_default()
            .push(format_diagnostic(&diag));
    }
    Ok(json!({
        "total_errors": by_file.values().map(|v| v.len()).sum::<usize>(),
        "files":        by_file,
    }))
}
```

---

## Common Helpers

```rust
/// Build a TextDocumentPositionParams (used by every request)
fn tdp(uri: Url, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri }, position }
}

/// Build a Position from 0-based line and column
fn position(line: u32, col: u32) -> Position {
    Position { line, character: col }
}

/// Format a Location for model output
fn format_location(loc: &Location) -> Value {
    json!({
        "file":  loc.uri.path(),
        "start": { "line": loc.range.start.line, "column": loc.range.start.character },
        "end":   { "line": loc.range.end.line,   "column": loc.range.end.character },
    })
}

/// Format a Diagnostic for model output
fn format_diagnostic(d: &Diagnostic) -> Value {
    json!({
        "severity": d.severity.map(|s| format!("{:?}", s)),
        "code":     d.code.as_ref().map(|c| match c {
            NumberOrString::Number(n) => n.to_string(),
            NumberOrString::String(s) => s.clone(),
        }),
        "message":  d.message,
        "range":    format_range(&d.range),
        "source":   d.source,
    })
}
```

---

## Max File Size Guard

Claude Code enforces `MAX_LSP_FILE_SIZE_BYTES = 10_000_000`. EdgeCrab matches this at sync time:

```rust
const MAX_LSP_FILE_BYTES: u64 = 10_000_000;

fn read_file_for_sync(path: &str) -> Result<String, LspError> {
    let meta = std::fs::metadata(path).map_err(LspError::Io)?;
    if meta.len() > MAX_LSP_FILE_BYTES {
        return Err(LspError::FileTooLarge { path: path.to_string(), size: meta.len() });
    }
    std::fs::read_to_string(path).map_err(LspError::Io)
}
```
