#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use edgecrab_tools::path_utils::jail_read_path;
use edgecrab_tools::registry::{ToolContext, ToolHandler};
use edgecrab_types::{ToolError, ToolSchema};
use lsp_types::request::{
    CallHierarchyIncomingCalls, CallHierarchyOutgoingCalls, CallHierarchyPrepare,
    CodeActionRequest, CodeActionResolveRequest, DocumentDiagnosticRequest, DocumentSymbolRequest,
    Formatting, GotoDefinition, GotoImplementation, HoverRequest, InlayHintRequest,
    LinkedEditingRange, PrepareRenameRequest, RangeFormatting, References, Rename,
    SemanticTokensFullRequest, SignatureHelpRequest, TypeHierarchyPrepare, TypeHierarchySubtypes,
    TypeHierarchySupertypes, WorkspaceDiagnosticRequest, WorkspaceSymbolRequest,
};
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    CodeAction, CodeActionContext, CodeActionOrCommand, CodeActionParams, CodeActionTriggerKind,
    Diagnostic, DiagnosticSeverity, DocumentDiagnosticParams, DocumentDiagnosticReport,
    DocumentDiagnosticReportResult, DocumentFormattingParams, DocumentRangeFormattingParams,
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, FormattingOptions,
    GotoDefinitionParams, Hover, HoverParams, InlayHint, InlayHintLabel, InlayHintParams,
    LinkedEditingRangeParams, LinkedEditingRanges, Location, Position, PrepareRenameResponse,
    Range, ReferenceContext, ReferenceParams, RenameParams, SemanticTokenType, SemanticTokens,
    SemanticTokensParams, SemanticTokensResult, SignatureHelp, SignatureHelpParams,
    SymbolInformation, TextDocumentIdentifier, TextDocumentPositionParams, TypeHierarchyItem,
    TypeHierarchyPrepareParams, TypeHierarchySubtypesParams, TypeHierarchySupertypesParams, Uri,
    WorkspaceDiagnosticParams, WorkspaceDiagnosticReportResult, WorkspaceSymbolParams,
    WorkspaceSymbolResponse,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::capability::{
    has_bool_or_registration, supports_call_hierarchy, supports_code_action_resolve,
    supports_code_actions, supports_implementation,
};
use crate::edit::{apply_text_edits, apply_workspace_edit};
use crate::enrichment::enrich_diagnostics;
use crate::error::{LspError, path_to_uri, uri_to_path};
use crate::manager::{PreparedServer, runtime_for_ctx};
use crate::render::{
    documentation_to_string, format_diagnostic, format_link, format_location, hover_to_string,
    range_json, symbol_kind_name,
};
use crate::require_capability;
use crate::sync::DocumentSyncGuard;

#[derive(Deserialize)]
struct PositionArgs {
    file: String,
    line: u32,
    column: u32,
}

#[derive(Deserialize)]
struct OptionalRangeArgs {
    file: String,
    #[serde(default)]
    start_line: Option<u32>,
    #[serde(default)]
    start_column: Option<u32>,
    #[serde(default)]
    end_line: Option<u32>,
    #[serde(default)]
    end_column: Option<u32>,
}

#[derive(Deserialize)]
struct CodeActionArgs {
    file: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

#[derive(Deserialize)]
struct ApplyCodeActionArgs {
    action: Value,
}

#[derive(Deserialize)]
struct RenameArgs {
    file: String,
    line: u32,
    column: u32,
    new_name: String,
}

#[derive(Deserialize)]
struct FormatDocumentArgs {
    file: String,
    #[serde(default)]
    tab_size: Option<u32>,
    #[serde(default)]
    insert_spaces: Option<bool>,
}

#[derive(Deserialize)]
struct WorkspaceSymbolsArgs {
    query: String,
}

#[derive(Deserialize)]
struct HierarchyItemArgs {
    item: Value,
}

#[derive(Deserialize)]
struct DiagnosticsPullArgs {
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    workspace: bool,
}

#[derive(Deserialize)]
struct EnrichArgs {
    file: String,
    #[serde(default = "default_severity_filter")]
    severity_filter: String,
}

#[derive(Deserialize)]
struct SelectAndApplyArgs {
    file: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    action_index: usize,
}

fn default_severity_filter() -> String {
    "error".into()
}

struct PreparedDocument {
    runtime: Arc<crate::manager::LspRuntime>,
    server: PreparedServer,
    path: PathBuf,
    uri: Uri,
    _guard: DocumentSyncGuard,
}

#[allow(clippy::result_large_err)]
fn json_response(value: Value) -> Result<String, ToolError> {
    Ok(value.to_string())
}

fn tool_error(tool: &str, error: LspError) -> ToolError {
    error.to_tool_error(tool)
}

fn schema_with_position(name: &str, description: &str) -> ToolSchema {
    ToolSchema {
        name: name.into(),
        description: description.into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "file": { "type": "string", "description": "Workspace-relative or absolute file path" },
                "line": { "type": "integer", "description": "1-based line number" },
                "column": { "type": "integer", "description": "1-based column number" }
            },
            "required": ["file", "line", "column"]
        }),
        strict: None,
    }
}

fn schema_with_range(name: &str, description: &str) -> ToolSchema {
    ToolSchema {
        name: name.into(),
        description: description.into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "file": { "type": "string" },
                "start_line": { "type": "integer" },
                "start_column": { "type": "integer" },
                "end_line": { "type": "integer" },
                "end_column": { "type": "integer" }
            },
            "required": ["file", "start_line", "start_column", "end_line", "end_column"]
        }),
        strict: None,
    }
}

fn lsp_position(line: u32, column: u32) -> Position {
    Position {
        line: line.saturating_sub(1),
        character: column.saturating_sub(1),
    }
}

fn lsp_range(start_line: u32, start_column: u32, end_line: u32, end_column: u32) -> Range {
    Range {
        start: lsp_position(start_line, start_column),
        end: lsp_position(end_line, end_column),
    }
}

fn resolve_file(ctx: &ToolContext, file: &str) -> Result<PathBuf, LspError> {
    let policy = ctx.config.file_path_policy(&ctx.cwd);
    if let Ok(path) = jail_read_path(file, &policy) {
        return Ok(path);
    }
    let canonical = Path::new(file)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(file));
    jail_read_path(&canonical.to_string_lossy(), &policy)
        .map_err(|err| LspError::Other(err.to_string()))
}

async fn prepare_document(ctx: &ToolContext, file: &str) -> Result<PreparedDocument, ToolError> {
    let runtime = runtime_for_ctx(ctx).map_err(|err| tool_error("lsp", err))?;
    let path = resolve_file(ctx, file).map_err(|err| tool_error("lsp", err))?;
    let server = runtime
        .manager
        .server_for_file(&path)
        .await
        .map_err(|err| tool_error("lsp", err))?;
    let guard = runtime
        .sync
        .ensure_open(
            server.connection.clone(),
            &path,
            &server.language_id,
            ctx.config.lsp_file_size_limit_bytes,
        )
        .await
        .map_err(|err| tool_error("lsp", err))?;
    let uri = path_to_uri(&path).map_err(|err| tool_error("lsp", err))?;
    Ok(PreparedDocument {
        runtime,
        server,
        path,
        uri,
        _guard: guard,
    })
}

async fn server_for_item_uri(
    ctx: &ToolContext,
    tool: &str,
    uri: &Uri,
) -> Result<PreparedServer, ToolError> {
    let runtime = runtime_for_ctx(ctx).map_err(|err| tool_error(tool, err))?;
    if let Ok(path) = uri_to_path(uri) {
        return runtime
            .manager
            .server_for_file(&path)
            .await
            .map_err(|err| tool_error(tool, err));
    }
    runtime
        .manager
        .all_workspace_servers()
        .await
        .into_iter()
        .next()
        .ok_or_else(|| ToolError::InvalidArgs {
            tool: tool.into(),
            message: "item.uri must be a file URL".into(),
        })
}

fn full_file_range(text: &str) -> Range {
    let last_line = text.lines().count().saturating_sub(1) as u32;
    let last_column = text
        .lines()
        .last()
        .map(|line| line.chars().count() as u32 + 1)
        .unwrap_or(1);
    lsp_range(1, 1, last_line + 1, last_column)
}

fn parse_optional_range(text: &str, args: &OptionalRangeArgs) -> Range {
    match (
        args.start_line,
        args.start_column,
        args.end_line,
        args.end_column,
    ) {
        (Some(sl), Some(sc), Some(el), Some(ec)) => lsp_range(sl, sc, el, ec),
        _ => full_file_range(text),
    }
}

fn text_document(uri: Uri) -> TextDocumentIdentifier {
    TextDocumentIdentifier { uri }
}

fn position_params(uri: Uri, line: u32, column: u32) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: text_document(uri),
        position: lsp_position(line, column),
    }
}

fn goto_params(uri: Uri, line: u32, column: u32) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: position_params(uri, line, column),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn render_goto(result: Option<lsp_types::GotoDefinitionResponse>) -> Value {
    match result {
        None => json!({ "found": false, "locations": [] }),
        Some(lsp_types::GotoDefinitionResponse::Scalar(location)) => {
            json!({ "found": true, "locations": [format_location(&location)] })
        }
        Some(lsp_types::GotoDefinitionResponse::Array(locations)) => json!({
            "found": !locations.is_empty(),
            "locations": locations.iter().map(format_location).collect::<Vec<_>>()
        }),
        Some(lsp_types::GotoDefinitionResponse::Link(links)) => json!({
            "found": !links.is_empty(),
            "locations": links.iter().map(format_link).collect::<Vec<_>>()
        }),
    }
}

fn render_hover(hover: Option<Hover>) -> Value {
    match hover {
        None => json!({ "found": false }),
        Some(hover) => json!({
            "found": true,
            "content": hover_to_string(&hover.contents),
            "range": hover.range.as_ref().map(range_json),
        }),
    }
}

fn render_document_symbol(symbol: &DocumentSymbol) -> Value {
    json!({
        "name": symbol.name,
        "kind": symbol_kind_name(symbol.kind),
        "range": range_json(&symbol.range),
        "children": symbol.children.as_ref().map(|children| children.iter().map(render_document_symbol).collect::<Vec<_>>()).unwrap_or_default(),
    })
}

fn render_workspace_symbol(symbol: &SymbolInformation) -> Value {
    json!({
        "name": symbol.name,
        "kind": symbol_kind_name(symbol.kind),
        "location": format_location(&symbol.location),
        "container_name": symbol.container_name,
    })
}

fn render_code_action(action: &CodeActionOrCommand, index: usize) -> Value {
    match action {
        CodeActionOrCommand::CodeAction(code_action) => json!({
            "index": index,
            "title": code_action.title,
            "kind": code_action.kind.as_ref().map(|kind| kind.as_str()),
            "preferred": code_action.is_preferred,
            "disabled": code_action.disabled.as_ref().map(|disabled| disabled.reason.clone()),
            "has_edit": code_action.edit.is_some(),
            "action": code_action,
        }),
        CodeActionOrCommand::Command(command) => json!({
            "index": index,
            "title": command.title,
            "command": command.command,
            "action": command,
        }),
    }
}

fn decode_semantic_tokens(
    tokens: &SemanticTokens,
    capabilities: &lsp_types::SemanticTokensServerCapabilities,
) -> Vec<Value> {
    let legend = match capabilities {
        lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(options) => {
            options.legend.clone()
        }
        lsp_types::SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(options) => {
            options.semantic_tokens_options.legend.clone()
        }
    };

    let mut current_line = 0u32;
    let mut current_start = 0u32;
    let mut decoded = Vec::new();
    for token in &tokens.data {
        current_line += token.delta_line;
        current_start = if token.delta_line == 0 {
            current_start + token.delta_start
        } else {
            token.delta_start
        };
        let token_type = legend
            .token_types
            .get(token.token_type as usize)
            .cloned()
            .unwrap_or(SemanticTokenType::new("unknown"));
        let modifiers = legend
            .token_modifiers
            .iter()
            .enumerate()
            .filter_map(|(idx, modifier)| {
                ((token.token_modifiers_bitset & (1u32 << idx)) != 0)
                    .then_some(modifier.as_str().to_string())
            })
            .collect::<Vec<_>>();
        decoded.push(json!({
            "line": current_line + 1,
            "start": current_start + 1,
            "length": token.length,
            "token_type": token_type.as_str(),
            "modifiers": modifiers,
        }));
    }
    decoded
}

async fn fetch_code_actions(
    ctx: &ToolContext,
    args: &CodeActionArgs,
) -> Result<(PreparedDocument, Vec<CodeActionOrCommand>), ToolError> {
    let prepared = prepare_document(ctx, &args.file).await?;
    let range = lsp_range(
        args.start_line,
        args.start_column,
        args.end_line,
        args.end_column,
    );
    let result: Option<Vec<CodeActionOrCommand>> = prepared
        .server
        .connection
        .request::<CodeActionRequest>(CodeActionParams {
            text_document: text_document(prepared.uri.clone()),
            range,
            context: CodeActionContext {
                diagnostics: Vec::<Diagnostic>::new(),
                only: None,
                trigger_kind: Some(CodeActionTriggerKind::INVOKED),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .map_err(|err| tool_error("lsp_code_actions", err))?;
    Ok((prepared, result.unwrap_or_default()))
}

async fn apply_code_action_value(
    ctx: &ToolContext,
    server: &PreparedServer,
    action_value: Value,
) -> Result<Value, ToolError> {
    let mut code_action: CodeAction =
        serde_json::from_value(action_value).map_err(|err| ToolError::InvalidArgs {
            tool: "lsp_apply_code_action".into(),
            message: err.to_string(),
        })?;
    if code_action.edit.is_none() && supports_code_action_resolve(&server.capabilities) {
        code_action = server
            .connection
            .request::<CodeActionResolveRequest>(code_action)
            .await
            .map_err(|err| tool_error("lsp_apply_code_action", err))?;
    }
    let Some(edit) = code_action.edit.as_ref() else {
        return Ok(json!({
            "applied": false,
            "reason": "Code action did not contain a workspace edit",
            "title": code_action.title,
        }));
    };
    let changes =
        apply_workspace_edit(ctx, edit).map_err(|err| tool_error("lsp_apply_code_action", err))?;
    Ok(json!({
        "applied": !changes.is_empty(),
        "title": code_action.title,
        "files_modified": changes,
    }))
}

macro_rules! define_position_tool {
    ($struct_name:ident, $tool_name:literal, $description:literal, $body:expr) => {
        pub struct $struct_name;

        #[async_trait]
        impl ToolHandler for $struct_name {
            fn name(&self) -> &'static str {
                $tool_name
            }

            fn toolset(&self) -> &'static str {
                "lsp"
            }

            fn schema(&self) -> ToolSchema {
                schema_with_position($tool_name, $description)
            }

            fn parallel_safe(&self) -> bool {
                true
            }

            async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
                let args: PositionArgs =
                    serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                        tool: $tool_name.into(),
                        message: err.to_string(),
                    })?;
                let prepared = prepare_document(ctx, &args.file).await?;
                ($body)(args, prepared, ctx).await
            }
        }

        inventory::submit!(&$struct_name as &dyn ToolHandler);
    };
}

define_position_tool!(
    GotoDefinitionTool,
    "lsp_goto_definition",
    "Go to the definition of the symbol at a 1-based line/column position.",
    |args: PositionArgs, prepared: PreparedDocument, _ctx: &ToolContext| async move {
        require_capability!(
            has_bool_or_registration(&prepared.server.capabilities.definition_provider),
            "definition"
        );
        let result = prepared
            .server
            .connection
            .request::<GotoDefinition>(goto_params(prepared.uri.clone(), args.line, args.column))
            .await
            .map_err(|err| tool_error("lsp_goto_definition", err))?;
        json_response(render_goto(result))
    }
);

pub struct FindReferencesTool;
#[async_trait]
impl ToolHandler for FindReferencesTool {
    fn name(&self) -> &'static str {
        "lsp_find_references"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "Find references to the symbol at a 1-based line/column position.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "line": { "type": "integer" },
                    "column": { "type": "integer" },
                    "include_declaration": { "type": "boolean", "default": true }
                },
                "required": ["file", "line", "column"]
            }),
            strict: None,
        }
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        #[derive(Deserialize)]
        struct Args {
            file: String,
            line: u32,
            column: u32,
            #[serde(default = "default_true")]
            include_declaration: bool,
        }
        fn default_true() -> bool {
            true
        }

        let args: Args = serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
            tool: self.name().into(),
            message: err.to_string(),
        })?;
        let prepared = prepare_document(ctx, &args.file).await?;
        require_capability!(
            has_bool_or_registration(&prepared.server.capabilities.references_provider),
            "references"
        );
        let result: Option<Vec<Location>> = prepared
            .server
            .connection
            .request::<References>(ReferenceParams {
                text_document_position: position_params(
                    prepared.uri.clone(),
                    args.line,
                    args.column,
                ),
                context: ReferenceContext {
                    include_declaration: args.include_declaration,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        let locations = result.unwrap_or_default();
        json_response(json!({
            "count": locations.len(),
            "locations": locations.iter().map(format_location).collect::<Vec<_>>(),
        }))
    }
}
inventory::submit!(&FindReferencesTool as &dyn ToolHandler);

define_position_tool!(
    HoverTool,
    "lsp_hover",
    "Show hover information for the symbol at a 1-based line/column position.",
    |args: PositionArgs, prepared: PreparedDocument, _ctx: &ToolContext| async move {
        require_capability!(
            prepared.server.capabilities.hover_provider.is_some(),
            "hover"
        );
        let result = prepared
            .server
            .connection
            .request::<HoverRequest>(HoverParams {
                text_document_position_params: position_params(
                    prepared.uri.clone(),
                    args.line,
                    args.column,
                ),
                work_done_progress_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error("lsp_hover", err))?;
        json_response(render_hover(result))
    }
);

pub struct DocumentSymbolsTool;
#[async_trait]
impl ToolHandler for DocumentSymbolsTool {
    fn name(&self) -> &'static str {
        "lsp_document_symbols"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "List hierarchical document symbols for a file.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "file": { "type": "string" } },
                "required": ["file"]
            }),
            strict: None,
        }
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        #[derive(Deserialize)]
        struct Args {
            file: String,
        }
        let args: Args = serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
            tool: self.name().into(),
            message: err.to_string(),
        })?;
        let prepared = prepare_document(ctx, &args.file).await?;
        require_capability!(
            has_bool_or_registration(&prepared.server.capabilities.document_symbol_provider),
            "document symbols"
        );
        let result: Option<DocumentSymbolResponse> = prepared
            .server
            .connection
            .request::<DocumentSymbolRequest>(DocumentSymbolParams {
                text_document: text_document(prepared.uri.clone()),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        let symbols = match result {
            None => Vec::new(),
            Some(DocumentSymbolResponse::Flat(items)) => {
                items.iter().map(render_workspace_symbol).collect()
            }
            Some(DocumentSymbolResponse::Nested(items)) => {
                items.iter().map(render_document_symbol).collect()
            }
        };
        json_response(json!({ "symbols": symbols }))
    }
}
inventory::submit!(&DocumentSymbolsTool as &dyn ToolHandler);

pub struct WorkspaceSymbolsTool;
#[async_trait]
impl ToolHandler for WorkspaceSymbolsTool {
    fn name(&self) -> &'static str {
        "lsp_workspace_symbols"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "Search workspace symbols across configured language servers.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
            strict: None,
        }
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: WorkspaceSymbolsArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let runtime = runtime_for_ctx(ctx).map_err(|err| tool_error(self.name(), err))?;
        let mut results = Vec::new();
        for server in runtime.manager.all_workspace_servers().await {
            if !has_bool_or_registration(&server.capabilities.workspace_symbol_provider) {
                continue;
            }
            let items: Option<WorkspaceSymbolResponse> = server
                .connection
                .request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams {
                    query: args.query.clone(),
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .map_err(|err| tool_error(self.name(), err))?;
            match items {
                Some(WorkspaceSymbolResponse::Flat(items)) => {
                    results.extend(items.iter().map(render_workspace_symbol));
                }
                Some(WorkspaceSymbolResponse::Nested(items)) => {
                    results.extend(items.iter().map(|item| {
                        json!({
                            "name": item.name,
                            "kind": symbol_kind_name(item.kind),
                            "location": match &item.location {
                                lsp_types::OneOf::Left(location) => Some(json!({
                                    "uri": location.uri.to_string(),
                                    "range": range_json(&location.range),
                                })),
                                lsp_types::OneOf::Right(location) => Some(json!({
                                    "uri": location.uri.to_string(),
                                    "range": null,
                                })),
                            },
                            "container_name": item.container_name,
                        })
                    }));
                }
                None => {}
            }
        }
        json_response(json!({ "symbols": results }))
    }
}
inventory::submit!(&WorkspaceSymbolsTool as &dyn ToolHandler);

define_position_tool!(
    GotoImplementationTool,
    "lsp_goto_implementation",
    "Go to implementations of the symbol at a 1-based line/column position.",
    |args: PositionArgs, prepared: PreparedDocument, _ctx: &ToolContext| async move {
        require_capability!(
            supports_implementation(&prepared.server.capabilities.implementation_provider),
            "implementation"
        );
        let result = prepared
            .server
            .connection
            .request::<GotoImplementation>(goto_params(
                prepared.uri.clone(),
                args.line,
                args.column,
            ))
            .await
            .map_err(|err| tool_error("lsp_goto_implementation", err))?;
        json_response(render_goto(result))
    }
);

define_position_tool!(
    CallHierarchyPrepareTool,
    "lsp_call_hierarchy_prepare",
    "Prepare call hierarchy items for the symbol at a 1-based line/column position.",
    |args: PositionArgs, prepared: PreparedDocument, _ctx: &ToolContext| async move {
        require_capability!(
            supports_call_hierarchy(&prepared.server.capabilities.call_hierarchy_provider),
            "call hierarchy"
        );
        let result: Option<Vec<CallHierarchyItem>> = prepared
            .server
            .connection
            .request::<CallHierarchyPrepare>(CallHierarchyPrepareParams {
                text_document_position_params: position_params(
                    prepared.uri.clone(),
                    args.line,
                    args.column,
                ),
                work_done_progress_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error("lsp_call_hierarchy_prepare", err))?;
        json_response(json!({ "items": result.unwrap_or_default() }))
    }
);

pub struct IncomingCallsTool;
#[async_trait]
impl ToolHandler for IncomingCallsTool {
    fn name(&self) -> &'static str {
        "lsp_incoming_calls"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "List incoming call hierarchy edges for an item returned by lsp_call_hierarchy_prepare.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "item": { "type": "object" } },
                "required": ["item"]
            }),
            strict: None,
        }
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: HierarchyItemArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let item: CallHierarchyItem =
            serde_json::from_value(args.item).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let server = server_for_item_uri(ctx, self.name(), &item.uri).await?;
        require_capability!(
            supports_call_hierarchy(&server.capabilities.call_hierarchy_provider),
            "call hierarchy"
        );
        let calls: Option<Vec<CallHierarchyIncomingCall>> = server
            .connection
            .request::<CallHierarchyIncomingCalls>(CallHierarchyIncomingCallsParams {
                item,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        json_response(json!({ "calls": calls.unwrap_or_default() }))
    }
}
inventory::submit!(&IncomingCallsTool as &dyn ToolHandler);

pub struct OutgoingCallsTool;
#[async_trait]
impl ToolHandler for OutgoingCallsTool {
    fn name(&self) -> &'static str {
        "lsp_outgoing_calls"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "List outgoing call hierarchy edges for an item returned by lsp_call_hierarchy_prepare.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "item": { "type": "object" } },
                "required": ["item"]
            }),
            strict: None,
        }
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: HierarchyItemArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let item: CallHierarchyItem =
            serde_json::from_value(args.item).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let server = server_for_item_uri(ctx, self.name(), &item.uri).await?;
        require_capability!(
            supports_call_hierarchy(&server.capabilities.call_hierarchy_provider),
            "call hierarchy"
        );
        let calls: Option<Vec<CallHierarchyOutgoingCall>> = server
            .connection
            .request::<CallHierarchyOutgoingCalls>(CallHierarchyOutgoingCallsParams {
                item,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        json_response(json!({ "calls": calls.unwrap_or_default() }))
    }
}
inventory::submit!(&OutgoingCallsTool as &dyn ToolHandler);

pub struct CodeActionsTool;
#[async_trait]
impl ToolHandler for CodeActionsTool {
    fn name(&self) -> &'static str {
        "lsp_code_actions"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        schema_with_range(
            self.name(),
            "List code actions for a selected range in a file.",
        )
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: CodeActionArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let (prepared, actions) = fetch_code_actions(ctx, &args).await?;
        require_capability!(
            supports_code_actions(&prepared.server.capabilities.code_action_provider),
            "code actions"
        );
        json_response(json!({
            "actions": actions.iter().enumerate().map(|(index, action)| render_code_action(action, index)).collect::<Vec<_>>()
        }))
    }
}
inventory::submit!(&CodeActionsTool as &dyn ToolHandler);

pub struct ApplyCodeActionTool;
#[async_trait]
impl ToolHandler for ApplyCodeActionTool {
    fn name(&self) -> &'static str {
        "lsp_apply_code_action"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "Apply a code action object returned by lsp_code_actions.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "action": { "type": "object" } },
                "required": ["action"]
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: ApplyCodeActionArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let action_uri = args
            .action
            .get("data")
            .and_then(|data| data.get("uri"))
            .and_then(Value::as_str)
            .and_then(|uri| uri.parse::<Uri>().ok());
        let server = if let Some(uri) = action_uri {
            let path = uri_to_path(&uri).map_err(|_| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: "code action data.uri must be a file URL".into(),
            })?;
            let runtime = runtime_for_ctx(ctx).map_err(|err| tool_error(self.name(), err))?;
            runtime
                .manager
                .server_for_file(&path)
                .await
                .map_err(|err| tool_error(self.name(), err))?
        } else {
            let action: CodeAction =
                serde_json::from_value(args.action.clone()).map_err(|err| {
                    ToolError::InvalidArgs {
                        tool: self.name().into(),
                        message: err.to_string(),
                    }
                })?;
            let edit_uri = action
                .edit
                .as_ref()
                .and_then(|edit| edit.changes.as_ref())
                .and_then(|changes| changes.keys().next())
                .cloned()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: self.name().into(),
                    message: "code action did not provide an identifiable target file".into(),
                })?;
            let path = uri_to_path(&edit_uri).map_err(|_| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: "workspace edit URI must be a file URL".into(),
            })?;
            let runtime = runtime_for_ctx(ctx).map_err(|err| tool_error(self.name(), err))?;
            runtime
                .manager
                .server_for_file(&path)
                .await
                .map_err(|err| tool_error(self.name(), err))?
        };
        json_response(apply_code_action_value(ctx, &server, args.action).await?)
    }
}
inventory::submit!(&ApplyCodeActionTool as &dyn ToolHandler);

pub struct RenameTool;
#[async_trait]
impl ToolHandler for RenameTool {
    fn name(&self) -> &'static str {
        "lsp_rename"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "Rename a symbol across files using the language server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "line": { "type": "integer" },
                    "column": { "type": "integer" },
                    "new_name": { "type": "string" }
                },
                "required": ["file", "line", "column", "new_name"]
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: RenameArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let prepared = prepare_document(ctx, &args.file).await?;
        require_capability!(
            prepared.server.capabilities.rename_provider.is_some(),
            "rename"
        );
        let _prepare: Option<PrepareRenameResponse> = prepared
            .server
            .connection
            .request::<PrepareRenameRequest>(position_params(
                prepared.uri.clone(),
                args.line,
                args.column,
            ))
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        let edit: Option<lsp_types::WorkspaceEdit> = prepared
            .server
            .connection
            .request::<Rename>(RenameParams {
                text_document_position: position_params(
                    prepared.uri.clone(),
                    args.line,
                    args.column,
                ),
                new_name: args.new_name.clone(),
                work_done_progress_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        let changes = edit
            .as_ref()
            .map(|edit| apply_workspace_edit(ctx, edit).map_err(|err| tool_error(self.name(), err)))
            .transpose()?
            .unwrap_or_default();
        json_response(json!({
            "renamed": !changes.is_empty(),
            "new_name": args.new_name,
            "files_modified": changes,
        }))
    }
}
inventory::submit!(&RenameTool as &dyn ToolHandler);

pub struct FormatDocumentTool;
#[async_trait]
impl ToolHandler for FormatDocumentTool {
    fn name(&self) -> &'static str {
        "lsp_format_document"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "Format an entire document using the configured language server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "tab_size": { "type": "integer", "default": 4 },
                    "insert_spaces": { "type": "boolean", "default": true }
                },
                "required": ["file"]
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: FormatDocumentArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let prepared = prepare_document(ctx, &args.file).await?;
        require_capability!(
            has_bool_or_registration(&prepared.server.capabilities.document_formatting_provider),
            "document formatting"
        );
        let edits: Option<Vec<lsp_types::TextEdit>> = prepared
            .server
            .connection
            .request::<Formatting>(DocumentFormattingParams {
                text_document: text_document(prepared.uri.clone()),
                options: FormattingOptions {
                    tab_size: args.tab_size.unwrap_or(4),
                    insert_spaces: args.insert_spaces.unwrap_or(true),
                    ..Default::default()
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        let original =
            std::fs::read_to_string(&prepared.path).map_err(|err| ToolError::ExecutionFailed {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let formatted = edits
            .as_ref()
            .map(|edits| {
                apply_text_edits(&original, edits).map_err(|err| tool_error(self.name(), err))
            })
            .transpose()?
            .unwrap_or_else(|| original.clone());
        std::fs::write(&prepared.path, &formatted).map_err(|err| ToolError::ExecutionFailed {
            tool: self.name().into(),
            message: err.to_string(),
        })?;
        prepared
            .runtime
            .sync
            .refresh_from_disk(
                &prepared.server.connection,
                &prepared.path,
                ctx.config.lsp_file_size_limit_bytes,
            )
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        json_response(json!({
            "formatted": edits.is_some(),
            "changed": original != formatted,
            "diff": similar::TextDiff::from_lines(&original, &formatted).unified_diff().context_radius(2).to_string(),
        }))
    }
}
inventory::submit!(&FormatDocumentTool as &dyn ToolHandler);

pub struct FormatRangeTool;
#[async_trait]
impl ToolHandler for FormatRangeTool {
    fn name(&self) -> &'static str {
        "lsp_format_range"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        schema_with_range(
            self.name(),
            "Format a selected range in a file using the language server.",
        )
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: CodeActionArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let prepared = prepare_document(ctx, &args.file).await?;
        require_capability!(
            has_bool_or_registration(
                &prepared
                    .server
                    .capabilities
                    .document_range_formatting_provider
            ),
            "range formatting"
        );
        let edits: Option<Vec<lsp_types::TextEdit>> = prepared
            .server
            .connection
            .request::<RangeFormatting>(DocumentRangeFormattingParams {
                text_document: text_document(prepared.uri.clone()),
                range: lsp_range(
                    args.start_line,
                    args.start_column,
                    args.end_line,
                    args.end_column,
                ),
                options: FormattingOptions {
                    tab_size: 4,
                    insert_spaces: true,
                    ..Default::default()
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        let original =
            std::fs::read_to_string(&prepared.path).map_err(|err| ToolError::ExecutionFailed {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let formatted = edits
            .as_ref()
            .map(|edits| {
                apply_text_edits(&original, edits).map_err(|err| tool_error(self.name(), err))
            })
            .transpose()?
            .unwrap_or_else(|| original.clone());
        std::fs::write(&prepared.path, &formatted).map_err(|err| ToolError::ExecutionFailed {
            tool: self.name().into(),
            message: err.to_string(),
        })?;
        prepared
            .runtime
            .sync
            .refresh_from_disk(
                &prepared.server.connection,
                &prepared.path,
                ctx.config.lsp_file_size_limit_bytes,
            )
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        json_response(json!({
            "formatted": edits.is_some(),
            "changed": original != formatted,
            "diff": similar::TextDiff::from_lines(&original, &formatted).unified_diff().context_radius(2).to_string(),
        }))
    }
}
inventory::submit!(&FormatRangeTool as &dyn ToolHandler);

pub struct InlayHintsTool;
#[async_trait]
impl ToolHandler for InlayHintsTool {
    fn name(&self) -> &'static str {
        "lsp_inlay_hints"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "List inlay hints for a file or selected range.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "start_line": { "type": "integer" },
                    "start_column": { "type": "integer" },
                    "end_line": { "type": "integer" },
                    "end_column": { "type": "integer" }
                },
                "required": ["file"]
            }),
            strict: None,
        }
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: OptionalRangeArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let prepared = prepare_document(ctx, &args.file).await?;
        require_capability!(
            prepared.server.capabilities.inlay_hint_provider.is_some(),
            "inlay hints"
        );
        let text =
            std::fs::read_to_string(&prepared.path).map_err(|err| ToolError::ExecutionFailed {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let result: Option<Vec<InlayHint>> = prepared
            .server
            .connection
            .request::<InlayHintRequest>(InlayHintParams {
                text_document: text_document(prepared.uri.clone()),
                range: parse_optional_range(&text, &args),
                work_done_progress_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        let hints = result.unwrap_or_default().iter().map(|hint| {
            let label = match &hint.label {
                InlayHintLabel::String(label) => label.clone(),
                InlayHintLabel::LabelParts(parts) => parts.iter().map(|part| part.value.clone()).collect::<String>(),
            };
            json!({
                "position": { "line": hint.position.line + 1, "column": hint.position.character + 1 },
                "label": label,
                "kind": hint.kind.map(|kind| format!("{kind:?}")),
                "tooltip": hint.tooltip.as_ref().map(|tooltip| serde_json::to_string(tooltip).unwrap_or_default()),
            })
        }).collect::<Vec<_>>();
        json_response(json!({ "hints": hints }))
    }
}
inventory::submit!(&InlayHintsTool as &dyn ToolHandler);

pub struct SemanticTokensTool;
#[async_trait]
impl ToolHandler for SemanticTokensTool {
    fn name(&self) -> &'static str {
        "lsp_semantic_tokens"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "Decode full-document semantic tokens for a file.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "file": { "type": "string" } },
                "required": ["file"]
            }),
            strict: None,
        }
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        #[derive(Deserialize)]
        struct Args {
            file: String,
        }
        let args: Args = serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
            tool: self.name().into(),
            message: err.to_string(),
        })?;
        let prepared = prepare_document(ctx, &args.file).await?;
        let provider = prepared
            .server
            .capabilities
            .semantic_tokens_provider
            .clone();
        require_capability!(provider.is_some(), "semantic tokens");
        let result: Option<SemanticTokensResult> = prepared
            .server
            .connection
            .request::<SemanticTokensFullRequest>(SemanticTokensParams {
                text_document: text_document(prepared.uri.clone()),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        let tokens = match result {
            Some(SemanticTokensResult::Tokens(tokens)) => {
                decode_semantic_tokens(&tokens, provider.as_ref().expect("checked above"))
            }
            _ => Vec::new(),
        };
        json_response(json!({ "tokens": tokens }))
    }
}
inventory::submit!(&SemanticTokensTool as &dyn ToolHandler);

define_position_tool!(
    SignatureHelpTool,
    "lsp_signature_help",
    "Show signature help at a 1-based line/column position.",
    |args: PositionArgs, prepared: PreparedDocument, _ctx: &ToolContext| async move {
        require_capability!(
            prepared
                .server
                .capabilities
                .signature_help_provider
                .is_some(),
            "signature help"
        );
        let result: Option<SignatureHelp> = prepared
            .server
            .connection
            .request::<SignatureHelpRequest>(SignatureHelpParams {
                context: None,
                text_document_position_params: position_params(
                    prepared.uri.clone(),
                    args.line,
                    args.column,
                ),
                work_done_progress_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error("lsp_signature_help", err))?;
        match result {
            None => json_response(json!({ "found": false })),
            Some(help) => {
                let active = help.active_signature.unwrap_or(0) as usize;
                let signature = help.signatures.get(active);
                json_response(json!({
                    "found": true,
                    "label": signature.map(|sig| sig.label.clone()),
                    "doc": signature.and_then(|sig| sig.documentation.as_ref()).map(documentation_to_string),
                    "parameters": signature.map(|sig| sig.parameters.as_ref().map(|parameters| {
                        parameters.iter().map(|parameter| json!({ "label": serde_json::to_string(&parameter.label).unwrap_or_default() })).collect::<Vec<_>>()
                    }).unwrap_or_default()),
                    "active_parameter": help.active_parameter,
                }))
            }
        }
    }
);

define_position_tool!(
    TypeHierarchyPrepareTool,
    "lsp_type_hierarchy_prepare",
    "Prepare type hierarchy items for the symbol at a 1-based line/column position.",
    |args: PositionArgs, prepared: PreparedDocument, _ctx: &ToolContext| async move {
        let result: Option<Vec<TypeHierarchyItem>> = prepared
            .server
            .connection
            .request::<TypeHierarchyPrepare>(TypeHierarchyPrepareParams {
                text_document_position_params: position_params(
                    prepared.uri.clone(),
                    args.line,
                    args.column,
                ),
                work_done_progress_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error("lsp_type_hierarchy_prepare", err))?;
        json_response(json!({ "items": result.unwrap_or_default() }))
    }
);

pub struct SupertypesTool;
#[async_trait]
impl ToolHandler for SupertypesTool {
    fn name(&self) -> &'static str {
        "lsp_supertypes"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "List supertypes for a type hierarchy item.".into(),
            parameters: json!({"type":"object","properties":{"item":{"type":"object"}},"required":["item"]}),
            strict: None,
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: HierarchyItemArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let item: TypeHierarchyItem =
            serde_json::from_value(args.item).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let server = server_for_item_uri(ctx, self.name(), &item.uri).await?;
        let result: Option<Vec<TypeHierarchyItem>> = server
            .connection
            .request::<TypeHierarchySupertypes>(TypeHierarchySupertypesParams {
                item,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        json_response(json!({ "items": result.unwrap_or_default() }))
    }
}
inventory::submit!(&SupertypesTool as &dyn ToolHandler);

pub struct SubtypesTool;
#[async_trait]
impl ToolHandler for SubtypesTool {
    fn name(&self) -> &'static str {
        "lsp_subtypes"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "List subtypes for a type hierarchy item.".into(),
            parameters: json!({"type":"object","properties":{"item":{"type":"object"}},"required":["item"]}),
            strict: None,
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: HierarchyItemArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let item: TypeHierarchyItem =
            serde_json::from_value(args.item).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let server = server_for_item_uri(ctx, self.name(), &item.uri).await?;
        let result: Option<Vec<TypeHierarchyItem>> = server
            .connection
            .request::<TypeHierarchySubtypes>(TypeHierarchySubtypesParams {
                item,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        json_response(json!({ "items": result.unwrap_or_default() }))
    }
}
inventory::submit!(&SubtypesTool as &dyn ToolHandler);

pub struct DiagnosticsPullTool;
#[async_trait]
impl ToolHandler for DiagnosticsPullTool {
    fn name(&self) -> &'static str {
        "lsp_diagnostics_pull"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "Pull document or workspace diagnostics from the language server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "workspace": { "type": "boolean", "default": false }
                }
            }),
            strict: None,
        }
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: DiagnosticsPullArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let runtime = runtime_for_ctx(ctx).map_err(|err| tool_error(self.name(), err))?;
        if args.workspace {
            let mut out = Vec::new();
            for server in runtime.manager.all_workspace_servers().await {
                if server.capabilities.diagnostic_provider.is_none() {
                    continue;
                }
                let report: WorkspaceDiagnosticReportResult = server
                    .connection
                    .request::<WorkspaceDiagnosticRequest>(WorkspaceDiagnosticParams {
                        identifier: None,
                        previous_result_ids: Vec::new(),
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    })
                    .await
                    .map_err(|err| tool_error(self.name(), err))?;
                let items = match report {
                    WorkspaceDiagnosticReportResult::Report(report) => report.items,
                    WorkspaceDiagnosticReportResult::Partial(report) => report.items,
                };
                out.push(json!({
                    "server": server.server_name,
                    "items": items,
                }));
            }
            return json_response(json!({ "workspace": true, "reports": out }));
        }

        let file = args.file.ok_or_else(|| ToolError::InvalidArgs {
            tool: self.name().into(),
            message: "file is required unless workspace=true".into(),
        })?;
        let prepared = prepare_document(ctx, &file).await?;
        require_capability!(
            prepared.server.capabilities.diagnostic_provider.is_some(),
            "diagnostics"
        );
        let report: DocumentDiagnosticReportResult = prepared
            .server
            .connection
            .request::<DocumentDiagnosticRequest>(DocumentDiagnosticParams {
                text_document: text_document(prepared.uri.clone()),
                identifier: None,
                previous_result_id: None,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error(self.name(), err))?;
        let diagnostics = match report {
            DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
                full.full_document_diagnostic_report.items
            }
            DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(_)) => {
                Vec::new()
            }
            DocumentDiagnosticReportResult::Partial(_) => Vec::new(),
        };
        json_response(json!({
            "diagnostics": diagnostics.iter().map(format_diagnostic).collect::<Vec<_>>()
        }))
    }
}
inventory::submit!(&DiagnosticsPullTool as &dyn ToolHandler);

define_position_tool!(
    LinkedEditingRangeTool,
    "lsp_linked_editing_range",
    "Return linked editing ranges at a 1-based line/column position.",
    |args: PositionArgs, prepared: PreparedDocument, _ctx: &ToolContext| async move {
        require_capability!(
            prepared
                .server
                .capabilities
                .linked_editing_range_provider
                .is_some(),
            "linked editing range"
        );
        let result: Option<LinkedEditingRanges> = prepared
            .server
            .connection
            .request::<LinkedEditingRange>(LinkedEditingRangeParams {
                text_document_position_params: position_params(
                    prepared.uri.clone(),
                    args.line,
                    args.column,
                ),
                work_done_progress_params: Default::default(),
            })
            .await
            .map_err(|err| tool_error("lsp_linked_editing_range", err))?;
        match result {
            None => json_response(json!({ "ranges": [] })),
            Some(result) => json_response(json!({
                "ranges": result.ranges.iter().map(range_json).collect::<Vec<_>>(),
                "word_pattern": result.word_pattern.map(|pattern: String| pattern),
            })),
        }
    }
);

pub struct EnrichDiagnosticsTool;
#[async_trait]
impl ToolHandler for EnrichDiagnosticsTool {
    fn name(&self) -> &'static str {
        "lsp_enrich_diagnostics"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: "Explain current diagnostics for a file in plain English.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "severity_filter": { "type": "string", "enum": ["error", "warning", "all"], "default": "error" }
                },
                "required": ["file"]
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: EnrichArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let prepared = prepare_document(ctx, &args.file).await?;
        let mut diagnostics = prepared
            .runtime
            .diagnostics
            .get(&prepared.uri)
            .unwrap_or_default();
        if diagnostics.is_empty() && prepared.server.capabilities.diagnostic_provider.is_some() {
            let report: DocumentDiagnosticReportResult = prepared
                .server
                .connection
                .request::<DocumentDiagnosticRequest>(DocumentDiagnosticParams {
                    text_document: text_document(prepared.uri.clone()),
                    identifier: None,
                    previous_result_id: None,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .map_err(|err| tool_error(self.name(), err))?;
            diagnostics = match report {
                DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
                    full.full_document_diagnostic_report.items
                }
                DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(_)) => {
                    Vec::new()
                }
                DocumentDiagnosticReportResult::Partial(_) => Vec::new(),
            };
        }
        let diagnostics: Vec<Diagnostic> = diagnostics
            .into_iter()
            .filter(|diagnostic| match args.severity_filter.as_str() {
                "warning" => diagnostic.severity == Some(DiagnosticSeverity::WARNING),
                "all" => true,
                _ => diagnostic.severity == Some(DiagnosticSeverity::ERROR),
            })
            .collect();
        let text =
            std::fs::read_to_string(&prepared.path).map_err(|err| ToolError::ExecutionFailed {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let enriched = enrich_diagnostics(ctx, &prepared.uri, &text, &diagnostics).await?;
        json_response(json!({ "diagnostics": enriched }))
    }
}
inventory::submit!(&EnrichDiagnosticsTool as &dyn ToolHandler);

pub struct SelectAndApplyActionTool;
#[async_trait]
impl ToolHandler for SelectAndApplyActionTool {
    fn name(&self) -> &'static str {
        "lsp_select_and_apply_action"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description:
                "Select a code action by index for a range, then apply its workspace edit.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "start_line": { "type": "integer" },
                    "start_column": { "type": "integer" },
                    "end_line": { "type": "integer" },
                    "end_column": { "type": "integer" },
                    "action_index": { "type": "integer" }
                },
                "required": ["file", "start_line", "start_column", "end_line", "end_column", "action_index"]
            }),
            strict: None,
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: SelectAndApplyArgs =
            serde_json::from_value(args).map_err(|err| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: err.to_string(),
            })?;
        let code_action_args = CodeActionArgs {
            file: args.file.clone(),
            start_line: args.start_line,
            start_column: args.start_column,
            end_line: args.end_line,
            end_column: args.end_column,
        };
        let (prepared, actions) = fetch_code_actions(ctx, &code_action_args).await?;
        let action = actions
            .get(args.action_index)
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: format!("action_index {} is out of range", args.action_index),
            })?;
        match action {
            CodeActionOrCommand::CodeAction(action) => json_response(
                apply_code_action_value(
                    ctx,
                    &prepared.server,
                    serde_json::to_value(action).map_err(|err| ToolError::ExecutionFailed {
                        tool: self.name().into(),
                        message: err.to_string(),
                    })?,
                )
                .await?,
            ),
            CodeActionOrCommand::Command(command) => json_response(json!({
                "applied": false,
                "reason": "Selected item is a command without a workspace edit",
                "title": command.title,
            })),
        }
    }
}
inventory::submit!(&SelectAndApplyActionTool as &dyn ToolHandler);

pub struct WorkspaceTypeErrorsTool;
#[async_trait]
impl ToolHandler for WorkspaceTypeErrorsTool {
    fn name(&self) -> &'static str {
        "lsp_workspace_type_errors"
    }
    fn toolset(&self) -> &'static str {
        "lsp"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description:
                "Return all cached error-severity diagnostics across the current session workspace."
                    .into(),
            parameters: json!({"type":"object","properties":{}}),
            strict: None,
        }
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    async fn execute(&self, _args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let runtime = runtime_for_ctx(ctx).map_err(|err| tool_error(self.name(), err))?;
        let all = runtime.diagnostics.all_errors();
        let mut files: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for (uri, diagnostic) in all {
            let file = uri.to_string();
            let file = uri_to_path(&uri)
                .ok()
                .map(|path| path.display().to_string())
                .unwrap_or(file);
            files
                .entry(file)
                .or_default()
                .push(format_diagnostic(&diagnostic));
        }
        let total_errors = files.values().map(Vec::len).sum::<usize>();
        json_response(json!({
            "total_errors": total_errors,
            "files": files,
        }))
    }
}
inventory::submit!(&WorkspaceTypeErrorsTool as &dyn ToolHandler);
