use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use dashmap::DashMap;
use lsp_types::notification::Initialized;
use lsp_types::{
    ClientCapabilities, GeneralClientCapabilities, InitializeParams, InitializeResult,
    InitializedParams, PositionEncodingKind, ServerCapabilities, Uri,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};
use tracing::{debug, warn};

use crate::diagnostics::DiagnosticCache;
use crate::error::LspError;

#[derive(Clone)]
pub struct ServerConnection {
    inner: Arc<ServerConnectionInner>,
}

struct ServerConnectionInner {
    server_id: String,
    writer: Mutex<ChildStdin>,
    pending: DashMap<i64, oneshot::Sender<Result<Value, LspError>>>,
    next_id: AtomicI64,
    alive: AtomicBool,
}

pub struct SpawnedServer {
    pub connection: ServerConnection,
    pub capabilities: ServerCapabilities,
    pub root_uri: Uri,
    pub position_encoding: PositionEncodingKind,
}

pub struct SpawnParams<'a> {
    pub server_id: &'a str,
    pub command: &'a std::path::Path,
    pub args: &'a [String],
    pub env: &'a HashMap<String, String>,
    pub root_dir: &'a std::path::Path,
    pub root_uri: Uri,
    pub init_options: Option<Value>,
    pub diagnostics: Arc<DiagnosticCache>,
}

impl ServerConnection {
    pub async fn spawn(params: SpawnParams<'_>) -> Result<SpawnedServer, LspError> {
        let SpawnParams {
            server_id,
            command,
            args,
            env,
            root_dir,
            root_uri,
            init_options,
            diagnostics,
        } = params;
        let mut child = Command::new(command);
        child
            .args(args)
            .current_dir(root_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (key, value) in env {
            child.env(key, value);
        }

        let mut child = child.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::Protocol("language server missing stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::Protocol("language server missing stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| LspError::Protocol("language server missing stderr".into()))?;

        let connection = Self {
            inner: Arc::new(ServerConnectionInner {
                server_id: server_id.to_string(),
                writer: Mutex::new(stdin),
                pending: DashMap::new(),
                next_id: AtomicI64::new(1),
                alive: AtomicBool::new(true),
            }),
        };

        spawn_stdout_task(connection.clone(), stdout, diagnostics);
        spawn_stderr_task(server_id.to_string(), stderr);
        spawn_wait_task(connection.clone(), child);

        #[allow(deprecated)]
        let initialize_params = InitializeParams {
            process_id: Some(std::process::id()),
            root_path: None,
            root_uri: Some(root_uri.clone()),
            work_done_progress_params: Default::default(),
            initialization_options: init_options,
            capabilities: ClientCapabilities {
                general: Some(GeneralClientCapabilities {
                    position_encodings: Some(vec![
                        PositionEncodingKind::UTF8,
                        PositionEncodingKind::UTF16,
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            trace: None,
            workspace_folders: Some(vec![lsp_types::WorkspaceFolder {
                uri: root_uri.clone(),
                name: root_dir
                    .file_name()
                    .and_then(|part| part.to_str())
                    .unwrap_or("workspace")
                    .to_string(),
            }]),
            client_info: None,
            locale: None,
        };
        let initialize = connection
            .request_raw("initialize", json!(initialize_params))
            .await?;
        let initialize: InitializeResult = serde_json::from_value(initialize)?;
        connection
            .notify::<Initialized>(InitializedParams {})
            .await?;

        let position_encoding = initialize
            .capabilities
            .position_encoding
            .clone()
            .unwrap_or(PositionEncodingKind::UTF16);

        Ok(SpawnedServer {
            connection,
            capabilities: initialize.capabilities,
            root_uri,
            position_encoding,
        })
    }

    pub fn is_alive(&self) -> bool {
        self.inner.alive.load(Ordering::Relaxed)
    }

    pub async fn request<R>(&self, params: R::Params) -> Result<R::Result, LspError>
    where
        R: lsp_types::request::Request,
        R::Params: serde::Serialize,
        R::Result: DeserializeOwned,
    {
        let value = self
            .request_raw(R::METHOD, serde_json::to_value(params)?)
            .await?;
        serde_json::from_value(value).map_err(LspError::from)
    }

    pub async fn request_raw(&self, method: &str, params: Value) -> Result<Value, LspError> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.inner.pending.insert(id, tx);
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;
        rx.await.unwrap_or_else(|_| {
            Err(LspError::ServerUnavailable {
                server: self.inner.server_id.clone(),
                message: "response channel closed".into(),
            })
        })
    }

    pub async fn notify<N>(&self, params: N::Params) -> Result<(), LspError>
    where
        N: lsp_types::notification::Notification,
        N::Params: serde::Serialize,
    {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "method": N::METHOD,
            "params": params,
        }))
        .await
    }

    async fn write_message(&self, value: &Value) -> Result<(), LspError> {
        let body = serde_json::to_vec(value)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut writer = self.inner.writer.lock().await;
        writer.write_all(header.as_bytes()).await?;
        writer.write_all(&body).await?;
        writer.flush().await?;
        Ok(())
    }
}

fn spawn_stdout_task(
    connection: ServerConnection,
    stdout: impl AsyncRead + Unpin + Send + 'static,
    diagnostics: Arc<DiagnosticCache>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            let message = match read_lsp_message(&mut reader).await {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(err) => {
                    warn!(error = %err, server = %connection.inner.server_id, "lsp stdout reader failed");
                    break;
                }
            };

            let value: Value = match serde_json::from_slice(&message) {
                Ok(value) => value,
                Err(err) => {
                    warn!(error = %err, server = %connection.inner.server_id, "invalid lsp json");
                    continue;
                }
            };

            if let Some(id) = value.get("id").and_then(response_id) {
                if let Some((_, tx)) = connection.inner.pending.remove(&id) {
                    if let Some(error) = value.get("error") {
                        let code = error
                            .get("code")
                            .and_then(Value::as_i64)
                            .unwrap_or_default();
                        let message = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown lsp error")
                            .to_string();
                        let err = if code == -32601 {
                            LspError::MethodNotFound {
                                method: value
                                    .get("method")
                                    .and_then(Value::as_str)
                                    .unwrap_or("unknown")
                                    .to_string(),
                            }
                        } else {
                            LspError::Protocol(format!("{code}: {message}"))
                        };
                        let _ = tx.send(Err(err));
                    } else {
                        let _ = tx.send(Ok(value.get("result").cloned().unwrap_or(Value::Null)));
                    }
                }
                continue;
            }

            let Some(method) = value.get("method").and_then(Value::as_str) else {
                continue;
            };
            match method {
                "textDocument/publishDiagnostics" => {
                    if let Some(params) = value.get("params") {
                        match serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(
                            params.clone(),
                        ) {
                            Ok(params) => diagnostics.update(
                                params.uri,
                                params.diagnostics,
                                connection.inner.server_id.clone(),
                            ),
                            Err(err) => warn!(error = %err, "failed to decode publishDiagnostics"),
                        }
                    }
                }
                "window/logMessage" => {
                    debug!(server = %connection.inner.server_id, message = %value)
                }
                _ => {}
            }
        }

        connection.inner.alive.store(false, Ordering::Relaxed);
        let pending_ids: Vec<i64> = connection
            .inner
            .pending
            .iter()
            .map(|entry| *entry.key())
            .collect();
        for id in pending_ids {
            if let Some((_, tx)) = connection.inner.pending.remove(&id) {
                let _ = tx.send(Err(LspError::ServerUnavailable {
                    server: connection.inner.server_id.clone(),
                    message: "language server exited".into(),
                }));
            }
        }
    });
}

fn spawn_stderr_task(server_id: String, stderr: impl AsyncRead + Unpin + Send + 'static) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            debug!(server = %server_id, stderr = %line.trim_end(), "lsp stderr");
            line.clear();
        }
    });
}

fn spawn_wait_task(connection: ServerConnection, mut child: tokio::process::Child) {
    tokio::spawn(async move {
        let _ = child.wait().await;
        connection.inner.alive.store(false, Ordering::Relaxed);
    });
}

async fn read_lsp_message<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Option<Vec<u8>>, LspError> {
    let mut content_length = None::<usize>;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            return Ok(None);
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = value.trim().parse::<usize>().ok();
        }
    }

    let length =
        content_length.ok_or_else(|| LspError::Protocol("missing Content-Length header".into()))?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

fn response_id(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.parse::<i64>().ok()))
}
