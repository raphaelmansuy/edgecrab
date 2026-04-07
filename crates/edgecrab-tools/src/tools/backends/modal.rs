//! # Modal execution backend (gap/backend B-01b)
//!
//! Runs commands inside a Modal sandbox via the Modal REST API.
//!
//! ## API flow
//!
//! ```text
//!   ┌─────────────────────────────────────────────┐
//!   │  ModalBackend                               │
//!   │                                             │
//!   │  1. init():          POST /sandboxes        │
//!   │     → sandbox_id                            │
//!   │                                             │
//!   │  2. execute():       POST /sandboxes/       │
//!   │                           {id}/commands    │
//!   │     → { stdout, stderr, exit_code }         │
//!   │                                             │
//!   │  3. cleanup():       DELETE /sandboxes/{id} │
//!   └─────────────────────────────────────────────┘
//! ```
//!
//! ## Authentication
//! Token ID + Token Secret are sent as HTTP Basic auth, exactly as the
//! Modal Python SDK does (`token_id:token_secret`).
//!
//! ## Env-var blocklist
//! `safe_env()` vars are passed in the `env` array of the sandbox create
//! request. This propagates the same blocklist as the local backend.
//!
//! ## Limitations
//! Modal does not yet offer a public stable sandbox REST API for external
//! clients; this implementation follows the documented experimental surface.
//! The Modal CLI and Python SDK remain the primary supported interface.
//! This backend is provided as a best-effort integration — set the
//! environment variable `EDGECRAB_MODAL_BASE_URL` to override the endpoint
//! if it changes.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STD};
use chrono::{DateTime, Utc};
use reqwest::Client;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use edgecrab_types::ToolError;

use crate::execution_tmp::{BACKEND_TMP_ROOT, temp_env_pairs, wrap_command_with_tmp_env};

use super::local::safe_env;
use super::{
    BackendKind, ExecOutput, ExecutionBackend, ModalBackendConfig, ModalTransportMode, shell_quote,
};

// ─── API types ────────────────────────────────────────────────────────

/// Default Modal sandbox REST endpoint.
const DEFAULT_BASE_URL: &str = "https://api.modal.com/v1";
const DEFAULT_TOOL_GATEWAY_SCHEME: &str = "https";
const MANAGED_POLL_INTERVAL: Duration = Duration::from_millis(250);
const MANAGED_TIMEOUT_GRACE: Duration = Duration::from_secs(10);
const NOUS_ACCESS_TOKEN_REFRESH_SKEW_SECONDS: i64 = 120;
const DIRECT_SNAPSHOT_NAMESPACE: &str = "direct";
const SNAPSHOT_TIMEOUT_SECS: u64 = 60;
const DIRECT_FILE_SYNC_TIMEOUT_SECS: u64 = 15;
const DEFAULT_REMOTE_EDGECRAB_HOME: &str = "/root/.edgecrab";
const MAX_DIRECT_SYNC_FILE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Serialize)]
struct CreateSandboxRequest {
    image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<u64>,
    env: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct CreateSandboxResponse {
    sandbox_id: String,
}

#[derive(Debug, Serialize)]
struct RunCommandRequest {
    command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RunCommandResponse {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

#[derive(Debug, Serialize)]
struct ManagedCreateSandboxRequest {
    image: String,
    cwd: String,
    cpu: u32,
    #[serde(rename = "memoryMiB")]
    memory_mib: u32,
    #[serde(rename = "timeoutMs")]
    timeout_ms: u64,
    #[serde(rename = "idleTimeoutMs")]
    idle_timeout_ms: u64,
    #[serde(rename = "persistentFilesystem")]
    persistent_filesystem: bool,
    #[serde(rename = "logicalKey")]
    logical_key: String,
    #[serde(rename = "diskMiB", skip_serializing_if = "Option::is_none")]
    disk_mib: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ManagedCreateSandboxResponse {
    id: String,
}

#[derive(Debug, Serialize)]
struct ManagedExecRequest {
    #[serde(rename = "execId")]
    exec_id: String,
    command: String,
    cwd: String,
    #[serde(rename = "timeoutMs")]
    timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
struct ManagedExecResponse {
    #[serde(rename = "execId")]
    exec_id: Option<String>,
    status: Option<String>,
    output: Option<String>,
    returncode: Option<i32>,
}

#[derive(Debug)]
enum ModalState {
    Direct(DirectModalState),
    Managed(ManagedModalState),
}

#[derive(Debug)]
struct DirectModalState {
    client: Client,
    base_url: String,
    task_id: String,
    sandbox_id: String,
    token_id: String,
    token_secret: String,
    persistent_filesystem: bool,
    sync_cache: TokioMutex<HashMap<String, FileFingerprint>>,
    dead: Arc<AtomicBool>,
}

#[derive(Debug)]
struct ManagedModalState {
    client: Client,
    gateway_origin: String,
    sandbox_id: String,
    persistent_filesystem: bool,
    dead: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct ManagedGatewayConfig {
    gateway_origin: String,
    user_token: String,
}

#[derive(Debug, Clone)]
enum ModalTransportSelection {
    Direct,
    Managed(ManagedGatewayConfig),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    modified_secs: u64,
    modified_nanos: u32,
    size_bytes: u64,
}

#[derive(Debug, Clone)]
struct SyncEntry {
    host_path: PathBuf,
    container_path: String,
    fingerprint: FileFingerprint,
}

fn build_basic_auth_client(token_id: &str, token_secret: &str) -> Result<Client, ToolError> {
    let mut headers = HeaderMap::new();
    let auth_value = format!("{}:{}", token_id, token_secret);
    let encoded = BASE64_STD.encode(auth_value.as_bytes());
    let auth_header = HeaderValue::from_str(&format!("Basic {encoded}")).map_err(|e| {
        ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("Invalid Modal auth header: {e}"),
        }
    })?;
    headers.insert(reqwest::header::AUTHORIZATION, auth_header);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("Failed to build reqwest client: {e}"),
        })
}

fn build_bearer_client(user_token: &str) -> Result<Client, ToolError> {
    let mut headers = HeaderMap::new();
    let auth_header = HeaderValue::from_str(&format!("Bearer {user_token}")).map_err(|e| {
        ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("Invalid managed Modal auth header: {e}"),
        }
    })?;
    headers.insert(reqwest::header::AUTHORIZATION, auth_header);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("Failed to build reqwest client: {e}"),
        })
}

fn nonempty(value: impl Into<String>) -> Option<String> {
    let value = value.into();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn edgecrab_home_dir() -> PathBuf {
    std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".edgecrab")
        })
}

fn auth_json_path() -> PathBuf {
    edgecrab_home_dir().join("auth.json")
}

fn modal_snapshot_store_path() -> PathBuf {
    edgecrab_home_dir().join("modal_snapshots.json")
}

fn load_snapshots() -> HashMap<String, String> {
    let path = modal_snapshot_store_path();
    let Ok(data) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

fn save_snapshots(data: &HashMap<String, String>) {
    let path = modal_snapshot_store_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = std::fs::write(path, json);
    }
}

fn direct_snapshot_key(task_id: &str) -> String {
    format!("{DIRECT_SNAPSHOT_NAMESPACE}:{task_id}")
}

fn get_snapshot_restore_candidate(task_id: &str) -> Option<(String, bool)> {
    let snapshots = load_snapshots();

    let namespaced = snapshots.get(&direct_snapshot_key(task_id)).cloned();
    if let Some(snapshot_id) = namespaced.filter(|value| !value.trim().is_empty()) {
        return Some((snapshot_id, false));
    }

    snapshots
        .get(task_id)
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .map(|snapshot_id| (snapshot_id, true))
}

fn store_direct_snapshot(task_id: &str, snapshot_id: &str) {
    let mut snapshots = load_snapshots();
    snapshots.insert(direct_snapshot_key(task_id), snapshot_id.to_string());
    snapshots.remove(task_id);
    save_snapshots(&snapshots);
}

fn delete_direct_snapshot(task_id: &str, snapshot_id: Option<&str>) {
    let mut snapshots = load_snapshots();
    let mut changed = false;

    for key in [direct_snapshot_key(task_id), task_id.to_string()] {
        let Some(value) = snapshots.get(&key).cloned() else {
            continue;
        };
        if snapshot_id.is_none_or(|expected| expected == value) {
            snapshots.remove(&key);
            changed = true;
        }
    }

    if changed {
        save_snapshots(&snapshots);
    }
}

fn read_nous_access_token_from_auth_store() -> Option<String> {
    let data = std::fs::read_to_string(auth_json_path()).ok()?;
    let json: serde_json::Value = serde_json::from_str(&data).ok()?;
    let nous = json.get("providers")?.get("nous")?;
    let token = nonempty(nous.get("access_token")?.as_str()?.to_string())?;

    let expires_at = nous.get("expires_at").and_then(|value| value.as_str());
    let expires = expires_at.and_then(|value| DateTime::parse_from_rfc3339(value).ok());
    if let Some(expires) = expires {
        let remaining = expires.with_timezone(&Utc) - Utc::now();
        if remaining.num_seconds() <= NOUS_ACCESS_TOKEN_REFRESH_SKEW_SECONDS {
            return None;
        }
    }

    Some(token)
}

fn file_fingerprint(path: &Path) -> Option<FileFingerprint> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified = metadata.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(FileFingerprint {
        modified_secs: since_epoch.as_secs(),
        modified_nanos: since_epoch.subsec_nanos(),
        size_bytes: metadata.len(),
    })
}

fn collect_sync_entries(
    host_root: &Path,
    container_root: &str,
    entries: &mut Vec<SyncEntry>,
) -> std::io::Result<()> {
    if !host_root.exists() {
        return Ok(());
    }

    for dir_entry in std::fs::read_dir(host_root)? {
        let dir_entry = dir_entry?;
        let path = dir_entry.path();
        let file_type = dir_entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let rel = path.strip_prefix(host_root).unwrap_or(&path);
            let next_container = format!(
                "{}/{}",
                container_root.trim_end_matches('/'),
                rel.to_string_lossy().replace('\\', "/"),
            );
            collect_sync_entries(&path, &next_container, entries)?;
            continue;
        }
        let Some(fingerprint) = file_fingerprint(&path) else {
            continue;
        };
        let rel = path.strip_prefix(host_root).unwrap_or(&path);
        let container_path = format!(
            "{}/{}",
            container_root.trim_end_matches('/'),
            rel.to_string_lossy().replace('\\', "/"),
        );
        entries.push(SyncEntry {
            host_path: path,
            container_path,
            fingerprint,
        });
    }

    Ok(())
}

fn modal_sync_entries() -> Vec<SyncEntry> {
    let home = edgecrab_home_dir();
    let remote_home = Path::new(DEFAULT_REMOTE_EDGECRAB_HOME);
    let mut entries = Vec::new();

    let auth_path = home.join("auth.json");
    if let Some(fingerprint) = file_fingerprint(&auth_path) {
        entries.push(SyncEntry {
            host_path: auth_path,
            container_path: remote_home.join("auth.json").to_string_lossy().into_owned(),
            fingerprint,
        });
    }

    for dir_name in [
        "skills",
        "optional-skills",
        "images",
        "image_cache",
        "gateway_media",
    ] {
        let host_dir = home.join(dir_name);
        let container_dir = remote_home.join(dir_name);
        let _ = collect_sync_entries(&host_dir, &container_dir.to_string_lossy(), &mut entries);
    }

    entries.sort_by(|a, b| a.container_path.cmp(&b.container_path));
    entries
}

fn tool_gateway_scheme() -> Result<String, ToolError> {
    match std::env::var("TOOL_GATEWAY_SCHEME") {
        Ok(value) if !value.trim().is_empty() => {
            let scheme = value.trim().to_ascii_lowercase();
            if matches!(scheme.as_str(), "http" | "https") {
                Ok(scheme)
            } else {
                Err(ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: "TOOL_GATEWAY_SCHEME must be 'http' or 'https'.".into(),
                })
            }
        }
        _ => Ok(DEFAULT_TOOL_GATEWAY_SCHEME.into()),
    }
}

fn resolve_direct_credentials(cfg: &ModalBackendConfig) -> Option<(String, String)> {
    let token_id = nonempty(cfg.token_id.clone()).or_else(|| std::env::var("MODAL_TOKEN_ID").ok());
    let token_secret =
        nonempty(cfg.token_secret.clone()).or_else(|| std::env::var("MODAL_TOKEN_SECRET").ok());
    match (token_id, token_secret) {
        (Some(token_id), Some(token_secret)) => Some((token_id, token_secret)),
        _ => None,
    }
}

fn resolve_managed_gateway(
    cfg: &ModalBackendConfig,
) -> Result<Option<ManagedGatewayConfig>, ToolError> {
    let gateway_origin = cfg
        .managed_gateway_url
        .clone()
        .and_then(nonempty)
        .or_else(|| std::env::var("MODAL_GATEWAY_URL").ok().and_then(nonempty))
        .or_else(|| std::env::var("TOOL_GATEWAY_URL").ok().and_then(nonempty))
        .map(|origin| origin.trim_end_matches('/').to_string());

    let gateway_origin = match gateway_origin {
        Some(origin) => Some(origin),
        None => {
            let domain = std::env::var("TOOL_GATEWAY_DOMAIN").ok().and_then(nonempty);
            domain.map(|domain| {
                let scheme =
                    tool_gateway_scheme().unwrap_or_else(|_| DEFAULT_TOOL_GATEWAY_SCHEME.into());
                format!("{scheme}://modal-gateway.{}", domain.trim_matches('/'))
            })
        }
    };

    let user_token = cfg
        .managed_user_token
        .clone()
        .and_then(nonempty)
        .or_else(|| {
            std::env::var("TOOL_GATEWAY_USER_TOKEN")
                .ok()
                .and_then(nonempty)
        })
        .or_else(read_nous_access_token_from_auth_store);

    Ok(match (gateway_origin, user_token) {
        (Some(gateway_origin), Some(user_token)) => Some(ManagedGatewayConfig {
            gateway_origin,
            user_token,
        }),
        _ => None,
    })
}

fn resolve_transport(cfg: &ModalBackendConfig) -> Result<ModalTransportSelection, ToolError> {
    let direct = resolve_direct_credentials(cfg);
    let managed = resolve_managed_gateway(cfg)?;

    match cfg.mode {
        ModalTransportMode::Direct => direct
            .map(|_| ModalTransportSelection::Direct)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Modal backend is configured for direct mode, but no direct Modal credentials were found. Set `terminal.modal.token_id`/`token_secret` or MODAL_TOKEN_ID/MODAL_TOKEN_SECRET.".into(),
            }),
        ModalTransportMode::Managed => managed
            .map(ModalTransportSelection::Managed)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Modal backend is configured for managed mode, but the managed Modal gateway is unavailable. Set `terminal.modal.managed_gateway_url` and a managed user token, or provide TOOL_GATEWAY_DOMAIN/TOOL_GATEWAY_USER_TOKEN.".into(),
            }),
        ModalTransportMode::Auto => {
            if direct.is_some() {
                Ok(ModalTransportSelection::Direct)
            } else if let Some(managed) = managed {
                Ok(ModalTransportSelection::Managed(managed))
            } else {
                Err(ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: "Modal backend selected but no direct Modal credentials or managed Modal gateway configuration was found.".into(),
                })
            }
        }
    }
}

fn managed_exec_output(body: ManagedExecResponse) -> ExecOutput {
    let status = body.status.unwrap_or_else(|| "completed".into());
    let exit_code = body
        .returncode
        .unwrap_or_else(|| if status == "timeout" { 124 } else { 1 });
    ExecOutput {
        stdout: body.output.unwrap_or_default(),
        stderr: String::new(),
        exit_code,
    }
}

fn terminal_managed_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled" | "timeout")
}

impl DirectModalState {
    async fn create_sandbox(
        client: &Client,
        base_url: &str,
        image: String,
        env: &HashMap<String, String>,
    ) -> Result<CreateSandboxResponse, ToolError> {
        let payload = CreateSandboxRequest {
            image,
            timeout: Some(3600),
            env: env.clone(),
        };

        let url = format!("{base_url}/sandboxes");
        let resp = client.post(&url).json(&payload).send().await.map_err(|e| {
            ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Modal create sandbox request failed: {e}"),
            }
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Modal create sandbox failed ({status}): {body}"),
            });
        }

        resp.json().await.map_err(|e| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("Modal create sandbox JSON parse fail: {e}"),
        })
    }

    async fn new(cfg: &ModalBackendConfig, task_id: &str) -> Result<Self, ToolError> {
        let base_url =
            std::env::var("EDGECRAB_MODAL_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());

        let (token_id, token_secret) =
            resolve_direct_credentials(cfg).ok_or_else(|| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Direct Modal transport selected without credentials.".into(),
            })?;
        let client = build_basic_auth_client(&token_id, &token_secret)?;

        // Build env map (filtered)
        let env: HashMap<String, String> = safe_env()
            .chain(std::iter::once(("EDGECRAB_TASK_ID".into(), task_id.into())))
            .chain(std::iter::once((
                "EDGECRAB_HOME".into(),
                DEFAULT_REMOTE_EDGECRAB_HOME.into(),
            )))
            .chain(temp_env_pairs(BACKEND_TMP_ROOT))
            .collect();

        let restored_snapshot = cfg
            .persistent_filesystem
            .then(|| get_snapshot_restore_candidate(task_id))
            .flatten();
        let (created, restored_snapshot_id) = if let Some((snapshot_id, from_legacy_key)) =
            restored_snapshot
        {
            match Self::create_sandbox(&client, &base_url, snapshot_id.clone(), &env).await {
                Ok(created) => {
                    if from_legacy_key {
                        store_direct_snapshot(task_id, &snapshot_id);
                    }
                    (created, Some(snapshot_id))
                }
                Err(err) => {
                    warn!(
                        task_id,
                        snapshot_id = %snapshot_id,
                        error = %err,
                        "Modal direct snapshot restore failed; retrying with base image"
                    );
                    delete_direct_snapshot(task_id, Some(&snapshot_id));
                    (
                        Self::create_sandbox(&client, &base_url, cfg.image.clone(), &env).await?,
                        None,
                    )
                }
            }
        } else {
            (
                Self::create_sandbox(&client, &base_url, cfg.image.clone(), &env).await?,
                None,
            )
        };

        info!(
            "ModalBackend: created sandbox {} (task={task_id})",
            created.sandbox_id
        );
        if let Some(snapshot_id) = restored_snapshot_id {
            info!(
                "ModalBackend: restored direct sandbox {} from snapshot {}",
                created.sandbox_id, snapshot_id
            );
        }

        Ok(Self {
            client,
            base_url,
            task_id: task_id.to_string(),
            sandbox_id: created.sandbox_id,
            token_id,
            token_secret,
            persistent_filesystem: cfg.persistent_filesystem,
            sync_cache: TokioMutex::new(HashMap::new()),
            dead: Arc::new(AtomicBool::new(false)),
        })
    }

    async fn run_command(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        if self.dead.load(Ordering::Relaxed) {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Modal sandbox is terminated".into(),
            });
        }

        let full_command = if !cwd.is_empty() && cwd != "." {
            format!("cd {} && {}", shell_quote(cwd), command)
        } else {
            command.to_string()
        };
        let full_command = wrap_command_with_tmp_env(&full_command, BACKEND_TMP_ROOT);

        let payload = RunCommandRequest {
            command: vec!["sh".into(), "-c".into(), full_command],
            timeout: Some(timeout.as_secs()),
        };

        let url = format!("{}/sandboxes/{}/commands", self.base_url, self.sandbox_id);

        let send_fut = self.client.post(&url).json(&payload).send();

        let resp = tokio::select! {
            res = tokio::time::timeout(timeout + Duration::from_secs(5), send_fut) => {
                match res {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => return Err(ToolError::ExecutionFailed {
                        tool: "terminal".into(),
                        message: format!("Modal run command request failed: {e}"),
                    }),
                    Err(_) => return Ok(ExecOutput {
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 124,
                    }),
                }
            }
            _ = cancel.cancelled() => {
                return Ok(ExecOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 130,
                });
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // 408 / 504 → treat as timeout
            if status.as_u16() == 408 || status.as_u16() == 504 {
                return Ok(ExecOutput {
                    stdout: String::new(),
                    stderr: body,
                    exit_code: 124,
                });
            }
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Modal run command failed ({status}): {body}"),
            });
        }

        let run_resp: RunCommandResponse =
            resp.json().await.map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Modal run command JSON parse fail: {e}"),
            })?;

        Ok(ExecOutput {
            stdout: run_resp.stdout,
            stderr: run_resp.stderr,
            exit_code: run_resp.exit_code,
        })
    }

    async fn push_file_to_sandbox(&self, entry: &SyncEntry) -> Result<bool, ToolError> {
        {
            let cache = self.sync_cache.lock().await;
            if cache.get(&entry.container_path) == Some(&entry.fingerprint) {
                return Ok(false);
            }
        }

        let content = std::fs::read(&entry.host_path).map_err(|e| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!(
                "Failed to read file for Modal sync ({}): {e}",
                entry.host_path.display()
            ),
        })?;
        if content.len() > MAX_DIRECT_SYNC_FILE_BYTES {
            warn!(
                path = %entry.host_path.display(),
                size_bytes = content.len(),
                "Skipping oversized direct Modal file sync candidate"
            );
            return Ok(false);
        }

        let parent = Path::new(&entry.container_path)
            .parent()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/".into());
        let command = if content.is_empty() {
            format!(
                "mkdir -p {} && : > {}",
                shell_quote(&parent),
                shell_quote(&entry.container_path)
            )
        } else {
            let encoded = BASE64_STD.encode(content);
            format!(
                "mkdir -p {} && printf %s {} | base64 -d > {}",
                shell_quote(&parent),
                shell_quote(&encoded),
                shell_quote(&entry.container_path)
            )
        };

        let output = self
            .run_command(
                &command,
                ".",
                Duration::from_secs(DIRECT_FILE_SYNC_TIMEOUT_SECS),
                CancellationToken::new(),
            )
            .await?;
        if output.exit_code != 0 {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!(
                    "Modal direct file sync failed for {} with exit code {}",
                    entry.container_path, output.exit_code
                ),
            });
        }

        let mut cache = self.sync_cache.lock().await;
        cache.insert(entry.container_path.clone(), entry.fingerprint);
        Ok(true)
    }

    async fn sync_files(&self) {
        for entry in modal_sync_entries() {
            if let Err(err) = self.push_file_to_sandbox(&entry).await {
                warn!(
                    path = %entry.host_path.display(),
                    container_path = %entry.container_path,
                    error = %err,
                    "Modal direct file sync failed"
                );
            }
        }
    }

    async fn exec(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        self.sync_files().await;
        self.run_command(command, cwd, timeout, cancel).await
    }

    async fn snapshot_filesystem(&self) -> Result<Option<String>, ToolError> {
        if !self.persistent_filesystem {
            return Ok(None);
        }

        let helper = std::env::var("EDGECRAB_MODAL_SNAPSHOT_HELPER")
            .ok()
            .and_then(nonempty);
        let mut command = if let Some(helper) = helper {
            let mut cmd = Command::new(helper);
            cmd.arg(&self.sandbox_id)
                .arg(SNAPSHOT_TIMEOUT_SECS.to_string());
            cmd
        } else {
            let mut cmd = Command::new("python3");
            cmd.arg("-c").arg(
                r#"
import sys
import modal

sandbox = modal.Sandbox.from_id(sys.argv[1])
image = sandbox.snapshot_filesystem(timeout=int(sys.argv[2]))
snapshot_id = getattr(image, "object_id", None) or getattr(image, "image_id", None)
if not snapshot_id:
    raise RuntimeError("Modal snapshot helper did not return an image id")
print(snapshot_id)
"#,
            );
            cmd.arg(&self.sandbox_id)
                .arg(SNAPSHOT_TIMEOUT_SECS.to_string());
            cmd
        };

        let output = command
            .env("MODAL_TOKEN_ID", &self.token_id)
            .env("MODAL_TOKEN_SECRET", &self.token_secret)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Modal direct snapshot helper failed to start: {e}"),
            })?;

        if !output.status.success() {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!(
                    "Modal direct snapshot helper failed (status {}): {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }

        Ok(nonempty(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    }

    async fn terminate(&self) {
        if self.persistent_filesystem {
            match self.snapshot_filesystem().await {
                Ok(Some(snapshot_id)) => {
                    store_direct_snapshot(&self.task_id, &snapshot_id);
                    info!(
                        "ModalBackend: saved direct snapshot {} for task {}",
                        snapshot_id, self.task_id
                    );
                }
                Ok(None) => {}
                Err(err) => {
                    warn!(task_id = %self.task_id, error = %err, "Modal direct snapshot failed");
                }
            }
        }

        let url = format!("{}/sandboxes/{}", self.base_url, self.sandbox_id);
        let _ = self.client.delete(&url).send().await;
        self.dead.store(true, Ordering::Relaxed);
    }
}

impl ManagedModalState {
    async fn new(
        cfg: &ModalBackendConfig,
        task_id: &str,
        gateway: ManagedGatewayConfig,
    ) -> Result<Self, ToolError> {
        let client = build_bearer_client(&gateway.user_token)?;
        let payload = ManagedCreateSandboxRequest {
            image: cfg.image.clone(),
            cwd: "/root".into(),
            cpu: cfg.cpu.max(1),
            memory_mib: cfg.memory_mb.max(1),
            timeout_ms: 3_600_000,
            idle_timeout_ms: 300_000,
            persistent_filesystem: cfg.persistent_filesystem,
            logical_key: task_id.to_string(),
            disk_mib: (cfg.disk_mb > 0).then_some(cfg.disk_mb),
        };
        let idempotency_key = uuid::Uuid::new_v4().to_string();

        let url = format!("{}/v1/sandboxes", gateway.gateway_origin);
        let resp = client
            .post(&url)
            .header("x-idempotency-key", idempotency_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Managed Modal create sandbox request failed: {e}"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Managed Modal create sandbox failed ({status}): {body}"),
            });
        }

        let created: ManagedCreateSandboxResponse =
            resp.json().await.map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Managed Modal create sandbox JSON parse fail: {e}"),
            })?;

        info!(
            "ModalBackend: created managed sandbox {} (task={task_id})",
            created.id
        );

        Ok(Self {
            client,
            gateway_origin: gateway.gateway_origin,
            sandbox_id: created.id,
            persistent_filesystem: cfg.persistent_filesystem,
            dead: Arc::new(AtomicBool::new(false)),
        })
    }

    async fn execute_once(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        if self.dead.load(Ordering::Relaxed) {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Managed Modal sandbox is terminated".into(),
            });
        }

        let exec_id = uuid::Uuid::new_v4().to_string();
        let payload = ManagedExecRequest {
            exec_id: exec_id.clone(),
            command: command.to_string(),
            cwd: if cwd.is_empty() {
                "/root".into()
            } else {
                cwd.into()
            },
            timeout_ms: timeout.as_millis().max(1).min(u128::from(u64::MAX)) as u64,
        };
        let url = format!(
            "{}/v1/sandboxes/{}/execs",
            self.gateway_origin, self.sandbox_id
        );

        let resp = tokio::select! {
            res = self.client.post(&url).json(&payload).send() => {
                res.map_err(|e| ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: format!("Managed Modal exec start failed: {e}"),
                })?
            }
            _ = cancel.cancelled() => {
                return Ok(ExecOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 130,
                });
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Managed Modal exec start failed ({status}): {body}"),
            });
        }

        let start: ManagedExecResponse =
            resp.json().await.map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Managed Modal exec start JSON parse fail: {e}"),
            })?;

        if let Some(status) = start.status.as_deref() {
            if terminal_managed_status(status) {
                return Ok(managed_exec_output(start));
            }
        }

        if start.exec_id.as_deref() != Some(exec_id.as_str()) {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Managed Modal exec start did not return the expected exec id.".into(),
            });
        }

        let deadline = tokio::time::Instant::now() + timeout + MANAGED_TIMEOUT_GRACE;
        let poll_url = format!(
            "{}/v1/sandboxes/{}/execs/{}",
            self.gateway_origin, self.sandbox_id, exec_id
        );
        let cancel_url = format!("{poll_url}/cancel");

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = self.client.post(&cancel_url).send().await;
                    return Ok(ExecOutput {
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 130,
                    });
                }
                _ = tokio::time::sleep(MANAGED_POLL_INTERVAL) => {}
            }

            if tokio::time::Instant::now() >= deadline {
                let _ = self.client.post(&cancel_url).send().await;
                return Ok(ExecOutput {
                    stdout: String::new(),
                    stderr: format!(
                        "Managed Modal exec timed out after {}s",
                        timeout.as_secs().max(1)
                    ),
                    exit_code: 124,
                });
            }

            let resp = self.client.get(&poll_url).send().await.map_err(|e| {
                ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: format!("Managed Modal exec poll failed: {e}"),
                }
            })?;

            if resp.status().as_u16() == 404 {
                return Err(ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: "Managed Modal exec not found.".into(),
                });
            }
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: format!("Managed Modal exec poll failed ({status}): {body}"),
                });
            }

            let body: ManagedExecResponse =
                resp.json().await.map_err(|e| ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: format!("Managed Modal exec poll JSON parse fail: {e}"),
                })?;
            if let Some(status) = body.status.as_deref() {
                if terminal_managed_status(status) {
                    return Ok(managed_exec_output(body));
                }
            }
        }
    }

    async fn terminate(&self) {
        let url = format!(
            "{}/v1/sandboxes/{}/terminate",
            self.gateway_origin, self.sandbox_id
        );
        let _ = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "snapshotBeforeTerminate": self.persistent_filesystem
            }))
            .send()
            .await;
        self.dead.store(true, Ordering::Relaxed);
    }
}

// ─── ModalBackend ─────────────────────────────────────────────────────

pub struct ModalBackend {
    config: ModalBackendConfig,
    task_id: String,
    state: TokioMutex<Option<Arc<ModalState>>>,
}

impl ModalBackend {
    pub fn new(task_id: impl Into<String>, config: ModalBackendConfig) -> Self {
        Self {
            task_id: task_id.into(),
            config,
            state: TokioMutex::new(None),
        }
    }

    async fn ensure_state(&self) -> Result<Arc<ModalState>, ToolError> {
        let mut guard = self.state.lock().await;
        let needs_init = guard
            .as_ref()
            .map(|s| match s.as_ref() {
                ModalState::Direct(state) => state.dead.load(Ordering::Relaxed),
                ModalState::Managed(state) => state.dead.load(Ordering::Relaxed),
            })
            .unwrap_or(true);
        if needs_init {
            *guard = Some(Arc::new(match resolve_transport(&self.config)? {
                ModalTransportSelection::Direct => {
                    ModalState::Direct(DirectModalState::new(&self.config, &self.task_id).await?)
                }
                ModalTransportSelection::Managed(gateway) => ModalState::Managed(
                    ManagedModalState::new(&self.config, &self.task_id, gateway).await?,
                ),
            }));
        }
        guard.clone().ok_or_else(|| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: "Modal state missing after init — this is a bug".into(),
        })
    }
}

#[async_trait]
impl ExecutionBackend for ModalBackend {
    async fn execute(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        let state = self.ensure_state().await?;
        match state.as_ref() {
            ModalState::Direct(state) => state.exec(command, cwd, timeout, cancel).await,
            ModalState::Managed(state) => state.execute_once(command, cwd, timeout, cancel).await,
        }
    }

    async fn execute_oneshot(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        let state = self.ensure_state().await?;
        match state.as_ref() {
            ModalState::Direct(state) => state.exec(command, cwd, timeout, cancel).await,
            ModalState::Managed(state) => state.execute_once(command, cwd, timeout, cancel).await,
        }
    }

    async fn cleanup(&self) -> Result<(), ToolError> {
        let mut guard = self.state.lock().await;
        if let Some(state) = guard.take() {
            if let Ok(state) = Arc::try_unwrap(state) {
                match state {
                    ModalState::Direct(state) => state.terminate().await,
                    ModalState::Managed(state) => state.terminate().await,
                }
            } else {
                warn!(
                    task_id = %self.task_id,
                    "Modal backend cleanup deferred because commands are still holding the sandbox"
                );
            }
        }
        Ok(())
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Modal
    }

    fn supports_remote_execute_code(&self) -> bool {
        true
    }

    async fn is_healthy(&self) -> bool {
        let guard = self.state.lock().await;
        match guard.as_ref().map(Arc::as_ref) {
            Some(ModalState::Direct(state)) => !state.dead.load(Ordering::Relaxed),
            Some(ModalState::Managed(state)) => !state.dead.load(Ordering::Relaxed),
            None => false,
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome;

    #[test]
    fn base64_via_crate_foobar() {
        // Verify the base64 crate works for our use case
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        assert_eq!(STANDARD.encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(STANDARD.encode(b"foo"), "Zm9v");
    }

    /// Modal integration tests require valid credentials in env vars.
    fn modal_available() -> Option<ModalBackendConfig> {
        let token_id = std::env::var("MODAL_TOKEN_ID").ok()?;
        let token_secret = std::env::var("MODAL_TOKEN_SECRET").ok()?;
        Some(ModalBackendConfig {
            image: "python:3.11-slim".into(),
            mode: ModalTransportMode::Direct,
            token_id,
            token_secret,
            ..ModalBackendConfig::default()
        })
    }

    #[test]
    fn auto_transport_prefers_direct_credentials() {
        let cfg = ModalBackendConfig {
            mode: ModalTransportMode::Auto,
            token_id: "modal-id".into(),
            token_secret: "modal-secret".into(),
            managed_gateway_url: Some("https://modal-gateway.example.com".into()),
            managed_user_token: Some("managed-token".into()),
            ..ModalBackendConfig::default()
        };

        match resolve_transport(&cfg).expect("resolve transport") {
            ModalTransportSelection::Direct => {}
            other => panic!("expected direct transport, got {other:?}"),
        }
    }

    #[test]
    fn managed_gateway_reads_nous_token_from_auth_store() {
        let home = TestEdgecrabHome::new();
        let auth_path = home.path().join("auth.json");
        std::fs::write(
            &auth_path,
            serde_json::json!({
                "providers": {
                    "nous": {
                        "access_token": "fresh-n-token",
                        "expires_at": "2099-01-01T00:00:00Z"
                    }
                }
            })
            .to_string(),
        )
        .expect("write auth json");

        unsafe { std::env::set_var("MODAL_GATEWAY_URL", "https://modal-gateway.example.com") };
        unsafe { std::env::remove_var("TOOL_GATEWAY_USER_TOKEN") };

        let gateway = resolve_managed_gateway(&ModalBackendConfig::default())
            .expect("resolve gateway")
            .expect("managed gateway");

        assert_eq!(gateway.gateway_origin, "https://modal-gateway.example.com");
        assert_eq!(gateway.user_token, "fresh-n-token");

        unsafe { std::env::remove_var("MODAL_GATEWAY_URL") };
    }

    #[test]
    fn direct_mode_without_credentials_fails_descriptively() {
        let cfg = ModalBackendConfig {
            mode: ModalTransportMode::Direct,
            ..ModalBackendConfig::default()
        };

        let err = resolve_transport(&cfg).expect_err("missing direct creds should fail");
        match err {
            ToolError::ExecutionFailed { message, .. } => {
                assert!(message.contains("direct mode"), "got: {message}");
            }
            other => panic!("expected execution failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn modal_backend_echo() {
        let Some(cfg) = modal_available() else {
            eprintln!("SKIP: MODAL_TOKEN_ID/MODAL_TOKEN_SECRET not set");
            return;
        };
        let b = ModalBackend::new("test-modal-echo", cfg);
        let out = b
            .execute(
                "echo modal-hello",
                "/",
                Duration::from_secs(60),
                CancellationToken::new(),
            )
            .await
            .expect("execute");
        assert!(out.stdout.contains("modal-hello"));
        assert_eq!(out.exit_code, 0);
        b.cleanup().await.expect("cleanup");
    }
}
