use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use edgecrab_tools::registry::ToolContext;
use edgecrab_tools::tools::backends::BackendKind;
use lsp_types::{PositionEncodingKind, ServerCapabilities, Uri};
use tokio::sync::Mutex;

use crate::config::{detect_project_root, resolve_server_command};
use crate::diagnostics::DiagnosticCache;
use crate::error::{LspError, path_to_uri};
use crate::protocol::{ServerConnection, SpawnParams, SpawnedServer};
use crate::sync::DocumentSyncLayer;

#[derive(Clone)]
pub struct PreparedServer {
    pub server_name: String,
    pub language_id: String,
    pub connection: ServerConnection,
    pub capabilities: ServerCapabilities,
    pub root_uri: Uri,
    pub position_encoding: PositionEncodingKind,
}

#[derive(Default)]
struct ServerSlot {
    session: Option<PreparedServer>,
    restart_count: u32,
}

pub struct LspServerManager {
    cwd: PathBuf,
    config: edgecrab_tools::AppConfigRef,
    diagnostics: Arc<DiagnosticCache>,
    servers: DashMap<String, Arc<Mutex<ServerSlot>>>,
}

pub struct LspRuntime {
    pub manager: Arc<LspServerManager>,
    pub sync: Arc<DocumentSyncLayer>,
    pub diagnostics: Arc<DiagnosticCache>,
}

fn runtimes() -> &'static DashMap<String, Arc<LspRuntime>> {
    static RUNTIMES: std::sync::OnceLock<DashMap<String, Arc<LspRuntime>>> =
        std::sync::OnceLock::new();
    RUNTIMES.get_or_init(DashMap::default)
}

pub fn runtime_for_ctx(ctx: &ToolContext) -> Result<Arc<LspRuntime>, LspError> {
    if !ctx.config.lsp_enabled {
        return Err(LspError::Disabled);
    }
    if !matches!(ctx.config.terminal_backend, BackendKind::Local) {
        return Err(LspError::RemoteBackendUnsupported);
    }

    let key = format!("{}::{}", ctx.session_id, ctx.cwd.display());
    if let Some(existing) = runtimes().get(&key) {
        return Ok(existing.value().clone());
    }

    let diagnostics = Arc::new(DiagnosticCache::default());
    let runtime = Arc::new(LspRuntime {
        manager: Arc::new(LspServerManager {
            cwd: ctx.cwd.clone(),
            config: ctx.config.clone(),
            diagnostics: Arc::clone(&diagnostics),
            servers: DashMap::new(),
        }),
        sync: Arc::new(DocumentSyncLayer::default()),
        diagnostics,
    });
    runtimes().insert(key, Arc::clone(&runtime));
    Ok(runtime)
}

impl LspServerManager {
    pub async fn server_for_file(&self, path: &Path) -> Result<PreparedServer, LspError> {
        let extension = path
            .extension()
            .and_then(|part| part.to_str())
            .ok_or_else(|| LspError::NoServerForFile {
                path: path.display().to_string(),
            })?;
        let (server_name, server_cfg) = self
            .config
            .lsp_server_for_extension(extension)
            .ok_or_else(|| LspError::NoServerForFile {
                path: path.display().to_string(),
            })?;

        let root_dir = detect_project_root(path, &self.cwd, server_cfg);
        let root_uri = path_to_uri(&root_dir)?;
        let slot_key = format!("{server_name}::{}", root_uri.as_str());
        let slot = self
            .servers
            .entry(slot_key)
            .or_insert_with(|| Arc::new(Mutex::new(ServerSlot::default())))
            .clone();

        let mut slot = slot.lock().await;
        let needs_restart = slot
            .session
            .as_ref()
            .is_none_or(|session| !session.connection.is_alive());
        if needs_restart {
            let resolved_command =
                resolve_server_command(server_name, &server_cfg.command, Some(&root_dir))?;
            let mut launch_args = resolved_command.args_prefix.clone();
            launch_args.extend(server_cfg.args.clone());
            let SpawnedServer {
                connection,
                capabilities,
                root_uri,
                position_encoding,
            } = ServerConnection::spawn(SpawnParams {
                server_id: server_name,
                command: &resolved_command.program,
                args: &launch_args,
                env: &server_cfg.env,
                root_dir: &root_dir,
                root_uri: root_uri.clone(),
                init_options: server_cfg.initialization_options.clone(),
                diagnostics: Arc::clone(&self.diagnostics),
            })
            .await?;
            slot.restart_count += 1;
            slot.session = Some(PreparedServer {
                server_name: server_name.to_string(),
                language_id: server_cfg.language_id.clone(),
                connection,
                capabilities,
                root_uri,
                position_encoding,
            });
        }

        slot.session
            .clone()
            .ok_or_else(|| LspError::ServerUnavailable {
                server: server_name.to_string(),
                message: "language server failed to initialize".into(),
            })
    }

    pub async fn all_workspace_servers(&self) -> Vec<PreparedServer> {
        let mut items = Vec::new();
        for cfg in self.config.lsp_servers.values() {
            if let Some(ext) = cfg.file_extensions.first() {
                let candidate = self.cwd.join(format!("__edgecrab_probe__.{ext}"));
                if let Ok(server) = self.server_for_file(&candidate).await {
                    items.push(server);
                }
            }
        }
        items.sort_by(|a, b| a.server_name.cmp(&b.server_name));
        items.dedup_by(|a, b| a.server_name == b.server_name && a.root_uri == b.root_uri);
        items
    }
}
