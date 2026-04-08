# EdgeCrab LSP — Architecture

---

## 1. System Overview

EdgeCrab acts as an **LSP client**. It spawns one language server process per language,
keeps documents open in each server, and exposes the LSP operations as agent tools via
the normal `ToolHandler` / `inventory::submit!` registration pattern.

```
 ┌─────────────────────────────────────────────────────────────────────────────┐
 │                         edgecrab-core (Agent Loop)                         │
 │                                                                             │
 │   model requests tool call   ─────────────────────────────────────────►   │
 │   "lsp_hover" {"file":... }                                                │
 └────────────────────────────────────────┬────────────────────────────────────┘
                                          │  ToolRegistry.dispatch()
                                          ▼
 ┌────────────────────────────────────────────────────────────────────────────┐
 │                         edgecrab-lsp  (new crate)                         │
 │                                                                            │
 │  ┌──────────────────┐   ┌──────────────────┐   ┌─────────────────────┐   │
 │  │  LspServerManager│   │ DocumentSyncLayer │   │  DiagnosticCache    │   │
 │  │                  │   │                  │   │                     │   │
 │  │  DashMap<        │   │  DashMap<        │   │  DashMap<           │   │
 │  │  LanguageId,     │   │  Url,            │   │  Url,               │   │
 │  │  ServerHandle>   │   │  OpenDocument>   │   │  Vec<Diagnostic>>   │   │
 │  └────────┬─────────┘   └────────┬─────────┘   └─────────────────────┘   │
 │           │                      │                                         │
 │           └──────────────────────┼──────────────► CapabilityRouter        │
 │                                  │                (checks caps before      │
 │  ┌───────────────────────────────▼────────────┐   dispatching ops)        │
 │  │  PositionEncoder  (UTF-16 ↔ byte offset)   │                           │
 │  └───────────────────────────────────────────-┘                           │
 │                                                                            │
 │  tools/                                                                    │
 │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐ │
 │  │ GotoDefinition│ │FindReferences│ │   Hover      │ │  CodeActions     │ │
 │  │   ToolHandler │ │  ToolHandler │ │  ToolHandler │ │  ToolHandler     │ │
 │  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────────┘ │
 │       ┊ (+ 16 more registered via inventory::submit!)                     │
 └───────────────────────────────────────────────┬────────────────────────────┘
                                                 │
           async-lsp MainLoop + ServerSocket      │
                                                 ▼
 ┌─────────────────────────────────────────────────────────────────────────────┐
 │                 Language Server Processes (per language)                    │
 │                                                                             │
 │   ┌───────────────┐   ┌───────────────┐   ┌───────────────┐               │
 │   │  rust-analyzer │   │   typescript- │   │   clangd      │               │
 │   │  (Rust files)  │   │   language-   │   │  (C/C++ files)│               │
 │   │                │   │   server      │   │               │               │
 │   └───────────────┘   └───────────────┘   └───────────────┘               │
 └─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Component Responsibilities

### 2a. `LspServerManager` (`manager.rs`)

**Single responsibility**: lifecycle of language server child processes.

```
State machine per server:

  ┌──────────┐   spawn()    ┌─────────────┐   initialized   ┌─────────┐
  │  Absent  │ ──────────► │Initializing │ ──────────────► │  Ready  │
  └──────────┘             └─────────────┘                 └────┬────┘
                                                                 │
                                                          crash/exit
                                                                 ▼
  ┌──────────┐   backoff    ┌─────────────┐
  │ Crashed  │ ◄────────── │  Restarting │  (exponential backoff: 1s, 2s, 4s, 8s, cap 60s)
  └──────────┘             └─────────────┘
       │
       │  max_restarts exceeded
       ▼
  ┌──────────┐
  │  Failed  │  (tool returns LspError::ServerUnavailable, model can request manual restart)
  └──────────┘
```

```rust
// manager.rs
pub struct LspServerManager {
    servers:       DashMap<LanguageId, Arc<Mutex<ServerState>>>,
    config:        Arc<LspConfig>,
    restart_tx:    tokio::sync::mpsc::Sender<LanguageId>,
}

pub struct ServerState {
    pub status:       ServerStatus,
    pub server_socket: Option<ServerSocket>,    // async-lsp
    pub capabilities: Option<ServerCapabilities>,
    pub root_uri:     Url,
    pub restart_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServerStatus {
    Absent,
    Initializing,
    Ready,
    Crashed { at: std::time::Instant },
    Restarting { attempt: u32, next_at: std::time::Instant },
    Failed,
}

impl LspServerManager {
    /// Get-or-start the server for a given language.
    /// Returns only when status is Ready or propagates LspError.
    pub async fn get_ready(
        &self,
        lang: &LanguageId,
    ) -> Result<Arc<Mutex<ServerState>>, LspError> { ... }
}
```

### 2b. `DocumentSyncLayer` (`sync.rs`)

**Single responsibility**: keep the server's document model consistent with the real filesystem.

Rules:
1. A file must be opened with `textDocument/didOpen` before any request can use it.
2. Changes (from the file_write tool or otherwise) must be sent via `textDocument/didChange`.
3. Files must be closed with `textDocument/didClose` when no longer needed to free server memory.

```rust
// sync.rs
pub struct DocumentSyncLayer {
    open_docs:  DashMap<Url, OpenDocument>,
    version_counter: DashMap<Url, i32>,
}

pub struct OpenDocument {
    pub uri:         Url,
    pub language_id: LanguageId,
    pub version:     i32,
    pub text:        String,
}

/// RAII guard — opens the document on creation, closes on drop.
pub struct DocumentSyncGuard<'a> {
    layer:      &'a DocumentSyncLayer,
    server:     ServerSocket,
    uri:        Url,
    already_open: bool,
}

impl Drop for DocumentSyncGuard<'_> {
    fn drop(&mut self) {
        if !self.already_open {
            // fire-and-forget close
            let _ = self.server.notify::<DidCloseTextDocument>(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: self.uri.clone() },
            });
        }
    }
}
```

### 2c. `DiagnosticCache` (`diagnostics.rs`)

**Single responsibility**: store diagnostics pushed by servers (push model) and serve them to
Tier 3 enrichment tools.

```rust
pub struct DiagnosticCache {
    cache: DashMap<Url, CachedDiagnostics>,
}

pub struct CachedDiagnostics {
    pub diagnostics: Vec<Diagnostic>,
    pub received_at: std::time::Instant,
    pub server_id:   LanguageId,
}

impl DiagnosticCache {
    pub fn update(&self, uri: Url, diags: Vec<Diagnostic>) { ... }
    pub fn get(&self, uri: &Url) -> Option<Vec<Diagnostic>> { ... }
    pub fn clear_file(&self, uri: &Url) { ... }       // matches Claude Code clearDeliveredDiagnosticsForFile
    pub fn all_errors(&self) -> Vec<(Url, Diagnostic)> { ... }  // workspace-wide scan
}
```

### 2d. `CapabilityRouter` (`capability.rs`)

**Single responsibility**: check server capabilities before dispatching — no tool must send a
request that the server did not advertise.

```rust
/// Macro — wraps a capability check. If the server lacks the capability,
/// returns a structured "not supported" JSON value instead of an error.
macro_rules! require_capability {
    ($caps:expr, $field:expr, $op_name:expr) => {
        if $field.is_none() {
            return Ok(serde_json::json!({
                "supported": false,
                "reason": format!("Server does not advertise {} capability", $op_name),
            }));
        }
    };
}
```

### 2e. `PositionEncoder` (`position.rs`)

LSP positions are UTF-16 code-unit offsets by default. Rust strings are UTF-8. This mismatch
causes subtle bugs. One encoder, used everywhere.

```rust
pub struct PositionEncoder;

impl PositionEncoder {
    /// Convert a (line, character) LSP Position to a byte offset in `text`.
    pub fn to_byte_offset(text: &str, pos: Position) -> Option<usize> { ... }

    /// Convert a byte offset to an LSP Position.
    pub fn to_position(text: &str, byte_offset: usize) -> Option<Position> { ... }

    /// Convert an LSP Range to a byte range.
    pub fn to_byte_range(text: &str, range: Range) -> Option<std::ops::Range<usize>> { ... }
}
```

---

## 3. async-lsp Integration

`async-lsp` uses a **tower-like layered service** model. EdgeCrab acts as the **client** side:
it spawns servers and holds `ServerSocket` handles to send requests.

```
┌────────────────────────────────────────────────────────────────────────────┐
│  EdgeCrab process                                                           │
│                                                                             │
│   LspServerManager::spawn_server("rust-analyzer")                          │
│        │                                                                    │
│        │  tokio::process::Command — pipes stdin/stdout                     │
│        ▼                                                                    │
│   ┌─────────────────────────────────────────────────────────┐              │
│   │  async-lsp MainLoop  (runs in a spawned tokio task)     │              │
│   │                                                         │              │
│   │   ┌──────────────┐    ┌──────────────────────────────┐ │              │
│   │   │  ClientSocket│    │  Router (notification handler)│ │              │
│   │   │  (send reqs  │    │                              │ │              │
│   │   │   to agent)  │    │  publishDiagnostics → cache  │ │              │
│   │   └──────────────┘    │  window/logMessage → tracing  │ │              │
│   │                       └──────────────────────────────┘ │              │
│   └─────────────────────┬───────────────────────────────────┘              │
│                         │  ServerSocket (held by LspServerManager)         │
│                         ▼                                                   │
│      tool calls invoke server_socket.request::<GotoDefinition>(params)     │
└────────────────────────────────────────────────────────────────────────────┘
          │  stdin/stdout  (JSON-RPC 2.0)
          ▼
┌────────────────────┐
│  rust-analyzer     │
│  child process     │
└────────────────────┘
```

### Spawn pattern (Rust)

```rust
use async_lsp::MainLoop;
use async_lsp::router::Router;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::client_socket::ClientSocket;
use tower::ServiceBuilder;

/// Spawn a language server and return a handle (ServerSocket + task join handle).
pub async fn spawn_server(
    command:  &str,
    args:     &[&str],
    root_uri: Url,
    diag_cache: Arc<DiagnosticCache>,
) -> anyhow::Result<(ServerSocket, tokio::task::JoinHandle<()>)> {
    let mut child = tokio::process::Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())  // prevent stderr/stdout mix
        .spawn()?;

    let stdin  = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    // Build the client service stack (handles incoming notifications from the server).
    let client_service = ServiceBuilder::new()
        .layer(TracingLayer::default())
        .layer(CatchUnwindLayer::default())
        .layer(ConcurrencyLayer::default())
        .service(
            Router::from_language_client(EdgeCrabClientHandler {
                diag_cache: diag_cache.clone(),
            })
        );

    let (mainloop, server_socket) = MainLoop::new_client(client_service);

    let task = tokio::spawn(async move {
        mainloop
            .run_buffered(stdout, stdin)
            .await
            .ok();
    });

    // Send initialize
    let init_result = server_socket.request::<lsp_types::request::Initialize>(
        lsp_types::InitializeParams {
            root_uri: Some(root_uri.clone()),
            capabilities: edge_client_capabilities(),
            ..Default::default()
        }
    ).await?;

    // Send initialized notification (required by protocol)
    server_socket.notify::<lsp_types::notification::Initialized>(
        lsp_types::InitializedParams {}
    )?;

    Ok((server_socket, task))
}

/// Declare what EdgeCrab supports as a client — drives what servers will send.
fn edge_client_capabilities() -> lsp_types::ClientCapabilities {
    use lsp_types::*;
    ClientCapabilities {
        text_document: Some(TextDocumentClientCapabilities {
            publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                related_information: Some(true),
                tag_support: None,
                version_support: Some(true),
                ..Default::default()
            }),
            hover: Some(HoverClientCapabilities {
                content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                ..Default::default()
            }),
            inlay_hint: Some(InlayHintClientCapabilities {
                resolve_support: None,
                dynamic_registration: Some(false),
            }),
            semantic_tokens: Some(SemanticTokensClientCapabilities {
                requests: SemanticTokensClientCapabilitiesRequests {
                    full: Some(SemanticTokensFullOptions::Bool(true)),
                    range: Some(true),
                    delta: None,
                },
                formats: vec![TokenFormat::RELATIVE],
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}
```

---

## 4. Integration with edgecrab-tools

All LSP tools follow the existing `ToolHandler` pattern and are registered at compile time.
The tools receive an `LspToolContext` (a minimal wrapper around the shared `LspServerManager`).

```rust
// tools/hover.rs

use edgecrab_tools::registry::{ToolContext, ToolHandler, ToolError};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct LspHoverTool;

#[async_trait]
impl ToolHandler for LspHoverTool {
    fn name(&self) -> &'static str { "lsp_hover" }
    fn toolset(&self) -> &'static str { "lsp" }
    fn emoji(&self) -> &'static str { "🔍" }

    fn schema(&self) -> edgecrab_types::ToolSchema {
        edgecrab_types::ToolSchema {
            name: "lsp_hover".into(),
            description: "Return type info and documentation for the symbol at a file position. \
                          Uses the language server for the file's language.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file":   { "type": "string", "description": "Absolute path to the file" },
                    "line":   { "type": "integer", "description": "0-based line number" },
                    "column": { "type": "integer", "description": "0-based column (UTF-8 bytes)" }
                },
                "required": ["file", "line", "column"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let lsp = ctx.lsp_manager()
            .ok_or_else(|| ToolError::Internal("LSP subsystem not initialised".into()))?;

        let file   = args["file"].as_str().ok_or_else(|| ToolError::InvalidArgs {
            tool: "lsp_hover".into(), message: "missing 'file'".into()
        })?;
        let line   = args["line"].as_u64().unwrap_or(0) as u32;
        let column = args["column"].as_u64().unwrap_or(0) as u32;

        // Validate path (edgecrab-security)
        edgecrab_security::path_safety::validate_path(file, ctx.config.workspace_root())?;

        let uri    = Url::from_file_path(file).map_err(|_| ToolError::Internal("bad path".into()))?;
        let lang   = detect_language(file);
        let server = lsp.get_ready(&lang).await.map_err(|e| ToolError::Internal(e.to_string()))?;
        let state  = server.lock().await;
        let socket = state.server_socket.as_ref().ok_or_else(|| ToolError::Internal("no socket".into()))?;

        // Ensure file is open
        let _guard = lsp.sync.ensure_open(socket, uri.clone(), &lang).await?;

        // Check capability
        let caps = state.capabilities.as_ref();
        if caps.map(|c| c.hover_provider.is_none()).unwrap_or(true) {
            return Ok(json!({ "supported": false, "reason": "server has no hover capability" }));
        }

        // Encode position (UTF-8 column → LSP UTF-16)
        let text   = std::fs::read_to_string(file).map_err(|e| ToolError::Internal(e.to_string()))?;
        let lsp_pos = PositionEncoder::to_position_from_byte_col(&text, line as usize, column as usize)
            .unwrap_or(Position { line, character: column });

        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position:      lsp_pos,
            },
            work_done_progress_params: Default::default(),
        };

        let result: Option<Hover> = socket.request::<lsp_types::request::HoverRequest>(params)
            .await
            .map_err(|e| ToolError::Internal(e.to_string()))?;

        match result {
            None => Ok(json!({ "found": false })),
            Some(hover) => {
                let content = match &hover.contents {
                    HoverContents::Markup(m) => m.value.clone(),
                    HoverContents::Scalar(s) => format_marked_string(s),
                    HoverContents::Array(arr) => arr.iter().map(format_marked_string).collect::<Vec<_>>().join("\n\n"),
                };
                Ok(json!({
                    "found":   true,
                    "content": content,
                    "range":   hover.range.map(|r| json!({
                        "start": { "line": r.start.line, "character": r.start.character },
                        "end":   { "line": r.end.line,   "character": r.end.character },
                    }))
                }))
            }
        }
    }
}

inventory::submit!(edgecrab_tools::registry::RegisteredTool { handler: &LspHoverTool });
```

---

## 5. File Event Integration (notify crate)

EdgeCrab's workspace already lists `notify` as a dependency. The LSP layer hooks into file
change events to push `textDocument/didChange` and `textDocument/didSave` automatically,
keeping server state consistent without tools having to manually trigger sync.

```
 file_write tool writes file
         │
         ▼
 notify Watcher detects fs event
         │   (debounced 50ms per file)
         ▼
 LspFileEventHandler::on_change(path)
         │
         ├─► DocumentSyncLayer::send_did_change(path)  ─► server gets content update
         └─► DiagnosticCache::clear_file(uri)           ─► stale diagnostics evicted
```

---

## 6. LLM Enrichment Layer (`enrichment.rs`)

Unique to EdgeCrab. Not present in Claude Code.

```
 LlmEnrichedDiagnostics tool
         │
         ├─► DiagnosticCache.all_errors()  → collects raw LSP diagnostics
         │
         ├─► PositionEncoder.to_byte_range() → extracts source context (±5 lines)
         │
         ├─► format_for_llm()  → structured text:
         │       "ERROR [rustc E0507]: cannot move out of `*self` (move occurs ...)"
         │       "Context:\n  fn foo(&self) { let x = *self; }\n                          ^^^"
         │
         └─► auxiliary_client.explain(diagnostic_text)
                 → "This error occurs because Rust's ownership rules prevent..."
                 → returns explanation + suggested_fix
```

---

## 7. Security Integration

All LSP tool paths go through `edgecrab_security::path_safety::validate_path()` before the
file is opened in the server. This prevents LSP operations from being weaponised to read
files outside the workspace root (a real threat when the model generates file arguments).

Language server binaries are also subject to an allowlist — servers are only spawned if
they appear in the user's `~/.edgecrab/config.yaml` under `lsp.servers`. No arbitrary
binary is executed based on model output alone.
