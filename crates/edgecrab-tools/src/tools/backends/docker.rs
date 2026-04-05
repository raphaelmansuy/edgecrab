//! # Docker execution backend (gap/backend B-01a)
//!
//! Runs commands inside a Docker container via the `bollard` crate.
//!
//! ## Design
//!
//! ```text
//!   ┌──────────────────────────────────────────┐
//!   │  LocalBackend (fallback for non-Docker)  │
//!   └──────────────┬───────────────────────────┘
//!                  │
//!   ┌──────────────▼───────────────────────────┐
//!   │  DockerBackend                           │
//!   │  ┌────────────────────────────────────┐  │
//!   │  │  bollard::Docker::connect_with_    │  │
//!   │  │  local_defaults()                  │  │
//!   │  └────────────────────────────────────┘  │
//!   │  ┌────────────────────────────────────┐  │
//!   │  │  Container (lazy-started per task) │  │
//!   │  │  - cap_drop: ALL                   │  │
//!   │  │  - pids_limit: 256                 │  │
//!   │  │  - tmpfs: /tmp                     │  │
//!   │  │  - read_only_rootfs: false         │  │
//!   │  │  - workspace bind-mounted rw       │  │
//!   │  └────────────────────────────────────┘  │
//!   └──────────────────────────────────────────┘
//! ```
//!
//! ## Security properties (matching hermes-agent docker environments)
//!
//! - All Linux capabilities dropped (`CapDrop: ["ALL"]`)
//! - PID limit 256 (prevents fork bombs)
//! - Anonymous tmpfs at `/tmp` (no persistent /tmp across agent sessions)
//! - Env-var blocklist applied to container `Env` list (B-03)
//! - Container is removed (`force: true`) on cleanup
//!
//! ## Persistent container
//!
//! Instead of creating a new container per `execute()` call (expensive),
//! we create one container per task and exec into it. This matches the
//! hermes-agent `DockerEnvironment._start()` pattern.
//!
//! ### Container supervision
//!
//! A Tokio task monitors `wait_container()`. If the container exits
//! unexpectedly it sets `dead = true` so the next `execute()` call
//! creates a fresh container or returns an error.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bollard::Docker;
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, RemoveContainerOptions,
    StartContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use futures::StreamExt;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use edgecrab_types::ToolError;

use super::local::safe_env;
use super::{BackendKind, DockerBackendConfig, DockerWorkspaceMount, ExecOutput, ExecutionBackend};

// ─── DockerState ──────────────────────────────────────────────────────

struct DockerState {
    docker: Docker,
    container_id: String,
    dead: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct WorkspaceMountBinding {
    host_root: std::path::PathBuf,
    container_root: String,
}

impl WorkspaceMountBinding {
    fn from_mount(mount: &DockerWorkspaceMount) -> Result<Self, ToolError> {
        let host_root = std::path::Path::new(&mount.host_path)
            .canonicalize()
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!(
                    "Docker workspace mount host path is not accessible ({}): {e}",
                    mount.host_path
                ),
            })?;
        Ok(Self {
            host_root,
            container_root: mount.container_path.clone(),
        })
    }

    fn map_host_path(&self, cwd: &str) -> Result<String, ToolError> {
        if cwd.is_empty() || cwd == "." {
            return Ok(self.container_root.clone());
        }

        let cwd_path = std::path::Path::new(cwd);
        if cwd_path.is_absolute() {
            if cwd == self.container_root
                || cwd.starts_with(&format!("{}/", self.container_root.trim_end_matches('/')))
            {
                return Ok(cwd.to_string());
            }
            let resolved = cwd_path
                .canonicalize()
                .map_err(|e| ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: format!("Docker workdir is not accessible on the host ({cwd}): {e}"),
                })?;
            let rel =
                resolved
                    .strip_prefix(&self.host_root)
                    .map_err(|_| ToolError::ExecutionFailed {
                        tool: "terminal".into(),
                        message: format!(
                            "Docker backend cannot access host workdir '{cwd}' because it is outside the mounted workspace '{}'.",
                            self.host_root.display()
                        ),
                    })?;
            return Ok(join_container_path(&self.container_root, rel));
        }

        Ok(join_container_path(
            &self.container_root,
            std::path::Path::new(cwd_path),
        ))
    }
}

fn join_container_path(root: &str, rel: &std::path::Path) -> String {
    let rel = rel.to_string_lossy().replace('\\', "/");
    if rel.is_empty() || rel == "." {
        root.trim_end_matches('/').to_string()
    } else {
        format!(
            "{}/{}",
            root.trim_end_matches('/'),
            rel.trim_start_matches('/')
        )
    }
}

impl DockerState {
    async fn new(cfg: &DockerBackendConfig, task_id: &str) -> Result<Self, ToolError> {
        let docker =
            Docker::connect_with_local_defaults().map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Cannot connect to Docker daemon: {e}"),
            })?;

        // Verify Docker is reachable
        docker
            .ping()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Docker ping failed: {e}"),
            })?;

        let container_name = format!("edgecrab-{task_id}");

        // Build env list (filtered via blocklist)
        let safe_envs: Vec<String> = safe_env()
            .map(|(k, v)| format!("{k}={v}"))
            .chain(std::iter::once(format!("EDGECRAB_TASK_ID={task_id}")))
            .chain(std::iter::once("TERM=dumb".into()))
            .chain(std::iter::once("LC_ALL=C.UTF-8".into()))
            .collect();

        // Build mounts
        let mut mounts: Vec<Mount> = Vec::new();

        // Bind-mount workspace if configured
        if let Some(ref workspace) = cfg.workspace_mount {
            mounts.push(Mount {
                target: Some(workspace.container_path.clone()),
                source: Some(workspace.host_path.clone()),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(workspace.read_only),
                ..Default::default()
            });
        }

        // tmpfs at /tmp
        mounts.push(Mount {
            target: Some("/tmp".into()),
            typ: Some(MountTypeEnum::TMPFS),
            tmpfs_options: Some(bollard::models::MountTmpfsOptions {
                size_bytes: Some(128 * 1024 * 1024), // 128 MB
                ..Default::default()
            }),
            ..Default::default()
        });

        let host_config = HostConfig {
            cap_drop: Some(vec!["ALL".into()]),
            pids_limit: Some(cfg.pids_limit.unwrap_or(256)),
            memory: cfg.memory_bytes,
            nano_cpus: cfg.nano_cpus,
            mounts: Some(mounts),
            ..Default::default()
        };

        let container_config: ContainerConfig<String> = ContainerConfig {
            image: Some(cfg.image.clone()),
            env: Some(safe_envs),
            host_config: Some(host_config),
            // Keep container running via infinite sleep
            cmd: Some(vec![
                "sh".into(),
                "-c".into(),
                "while true; do sleep 3600; done".into(),
            ]),
            working_dir: cfg
                .workspace_mount
                .as_ref()
                .map(|m| m.container_path.clone()),
            tty: Some(false),
            attach_stdin: Some(false),
            attach_stdout: Some(false),
            attach_stderr: Some(false),
            ..Default::default()
        };

        let create_opts = CreateContainerOptions {
            name: container_name.as_str(),
            platform: None,
        };

        // Remove any leftover container from a previous run
        let _ = docker
            .remove_container(
                &container_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        let created = docker
            .create_container(Some(create_opts), container_config)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Docker create container failed: {e}"),
            })?;

        let container_id = created.id.clone();

        docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Docker start container failed: {e}"),
            })?;

        info!("DockerBackend: started container {container_id} ({container_name})");

        let dead = Arc::new(AtomicBool::new(false));

        // Background task: watch for unexpected container exit
        {
            let docker_clone = docker.clone();
            let container_id_clone = container_id.clone();
            let dead_clone = dead.clone();
            tokio::spawn(async move {
                let _ = docker_clone
                    .wait_container::<String>(&container_id_clone, None)
                    .next()
                    .await;
                dead_clone.store(true, Ordering::Relaxed);
                debug!("DockerBackend: container {container_id_clone} exited");
            });
        }

        Ok(Self {
            docker,
            container_id,
            dead,
        })
    }

    async fn exec(
        &self,
        command: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        if self.dead.load(Ordering::Relaxed) {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Docker container is dead".into(),
            });
        }

        let exec_create = self
            .docker
            .create_exec(
                &self.container_id,
                CreateExecOptions::<String> {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    cmd: Some(vec!["sh".into(), "-c".into(), command.to_string()]),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Docker exec create failed: {e}"),
            })?;

        let exec_id = exec_create.id;

        // Start exec (attach to output)
        let start_result = self
            .docker
            .start_exec(&exec_id, None::<StartExecOptions>)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Docker exec start failed: {e}"),
            })?;

        // Collect streamed output
        let mut stdout_parts: Vec<String> = Vec::new();
        let mut stderr_parts: Vec<String> = Vec::new();

        if let StartExecResults::Attached { mut output, .. } = start_result {
            let collect_fut = async {
                while let Some(msg_result) = output.next().await {
                    match msg_result {
                        Ok(bollard::container::LogOutput::StdOut { message }) => {
                            stdout_parts.push(String::from_utf8_lossy(&message).into_owned());
                        }
                        Ok(bollard::container::LogOutput::StdErr { message }) => {
                            stderr_parts.push(String::from_utf8_lossy(&message).into_owned());
                        }
                        Ok(bollard::container::LogOutput::Console { message }) => {
                            stdout_parts.push(String::from_utf8_lossy(&message).into_owned());
                        }
                        Ok(_) => {}
                        Err(e) => {
                            warn!("Docker exec stream error: {e}");
                            break;
                        }
                    }
                }
            };

            tokio::select! {
                _ = tokio::time::timeout(timeout, collect_fut) => {}
                _ = cancel.cancelled() => {
                    // Kill the exec
                    let _ = self.docker.inspect_exec(&exec_id).await;
                    return Ok(ExecOutput {
                        stdout: stdout_parts.join(""),
                        stderr: stderr_parts.join(""),
                        exit_code: 130,
                    });
                }
            }
        }

        // Get exit code from exec inspection
        let inspect =
            self.docker
                .inspect_exec(&exec_id)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: format!("Docker exec inspect failed: {e}"),
                })?;

        let exit_code = inspect.exit_code.unwrap_or(0) as i32;

        Ok(ExecOutput {
            stdout: stdout_parts.join(""),
            stderr: stderr_parts.join(""),
            exit_code,
        })
    }

    async fn remove(&self) {
        let _ = self
            .docker
            .remove_container(
                &self.container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;
    }
}

// ─── DockerBackend ────────────────────────────────────────────────────

pub struct DockerBackend {
    config: DockerBackendConfig,
    task_id: String,
    state: TokioMutex<Option<DockerState>>,
    workspace_binding: Option<WorkspaceMountBinding>,
}

impl DockerBackend {
    pub fn new(task_id: impl Into<String>, config: DockerBackendConfig) -> Self {
        let workspace_binding = config
            .workspace_mount
            .as_ref()
            .and_then(|mount| WorkspaceMountBinding::from_mount(mount).ok());
        Self {
            task_id: task_id.into(),
            config,
            state: TokioMutex::new(None),
            workspace_binding,
        }
    }

    async fn ensure_state(&self) -> Result<(), ToolError> {
        let mut guard = self.state.lock().await;
        let needs_init = match &*guard {
            None => true,
            Some(s) => s.dead.load(Ordering::Relaxed),
        };
        if needs_init {
            *guard = Some(DockerState::new(&self.config, &self.task_id).await?);
        }
        Ok(())
    }
}

#[async_trait]
impl ExecutionBackend for DockerBackend {
    async fn execute(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        self.ensure_state().await?;
        let guard = self.state.lock().await;
        if let Some(state) = &*guard {
            let effective_command = if let Some(binding) = &self.workspace_binding {
                let mapped_cwd = binding.map_host_path(cwd)?;
                if mapped_cwd == binding.container_root {
                    command.to_string()
                } else {
                    format!("cd {:?} && {}", mapped_cwd, command)
                }
            } else {
                command.to_string()
            };
            state.exec(&effective_command, timeout, cancel).await
        } else {
            Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Docker state not available after init — this is a bug".into(),
            })
        }
    }

    async fn cleanup(&self) -> Result<(), ToolError> {
        let mut guard = self.state.lock().await;
        if let Some(state) = guard.take() {
            state.remove().await;
        }
        Ok(())
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Docker
    }

    async fn is_healthy(&self) -> bool {
        let guard = self.state.lock().await;
        match guard.as_ref() {
            Some(s) => !s.dead.load(Ordering::Relaxed),
            None => false,
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::tools::backends::DockerBackendConfig;

    fn default_config() -> DockerBackendConfig {
        DockerBackendConfig {
            image: "alpine:latest".into(),
            workspace_mount: None,
            pids_limit: Some(64),
            memory_bytes: Some(256 * 1024 * 1024),
            nano_cpus: None,
        }
    }

    #[test]
    fn workspace_binding_maps_host_paths_into_container_paths() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let nested = dir.path().join("src").join("bin");
        std::fs::create_dir_all(&nested).expect("mkdir");

        let binding = WorkspaceMountBinding::from_mount(&DockerWorkspaceMount {
            host_path: dir.path().to_string_lossy().into_owned(),
            container_path: "/workspace".into(),
            read_only: false,
        })
        .expect("binding");

        let mapped = binding
            .map_host_path(&nested.to_string_lossy())
            .expect("map nested cwd");
        assert_eq!(mapped, "/workspace/src/bin");
    }

    #[test]
    fn workspace_binding_rejects_paths_outside_mount() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let outside = tempfile::tempdir().expect("outside");

        let binding = WorkspaceMountBinding::from_mount(&DockerWorkspaceMount {
            host_path: dir.path().to_string_lossy().into_owned(),
            container_path: "/workspace".into(),
            read_only: false,
        })
        .expect("binding");

        let err = binding
            .map_host_path(&outside.path().to_string_lossy())
            .expect_err("outside cwd must fail");
        assert!(err.to_string().contains("outside the mounted workspace"));
    }

    /// Skip test if Docker is unavailable or the test image isn't pulled.
    async fn docker_ready() -> bool {
        match Docker::connect_with_local_defaults() {
            Ok(d) => d.ping().await.is_ok(),
            Err(_) => false,
        }
    }

    /// Helper: run docker test and skip gracefully on daemon/image errors.
    ///
    /// Returns `Some(ExecOutput)` on success, `None` if skipped.
    async fn try_exec(b: &DockerBackend, cmd: &str) -> Option<ExecOutput> {
        match b
            .execute(
                cmd,
                "/workspace",
                Duration::from_secs(30),
                CancellationToken::new(),
            )
            .await
        {
            Ok(out) => Some(out),
            Err(ToolError::ExecutionFailed { ref message, .. })
                if message.contains("No such image")
                    || message.contains("daemon")
                    || message.contains("ping") =>
            {
                eprintln!("SKIP (Docker): {message}");
                None
            }
            Err(e) => panic!("unexpected Docker error: {e:?}"),
        }
    }

    #[tokio::test]
    async fn docker_backend_echo() {
        if !docker_ready().await {
            eprintln!("SKIP: Docker daemon not available");
            return;
        }
        let b = DockerBackend::new("test-docker-echo", default_config());
        let Some(out) = try_exec(&b, "echo hello-docker").await else {
            return;
        };
        assert!(out.stdout.contains("hello-docker"), "got: {}", out.stdout);
        assert_eq!(out.exit_code, 0);
        b.cleanup().await.expect("cleanup");
    }

    #[tokio::test]
    async fn docker_backend_exit_code() {
        if !docker_ready().await {
            eprintln!("SKIP: Docker daemon not available");
            return;
        }
        let b = DockerBackend::new("test-docker-exit", default_config());
        let Some(out) = try_exec(&b, "exit 7").await else {
            return;
        };
        assert_eq!(out.exit_code, 7);
        b.cleanup().await.expect("cleanup");
    }

    #[tokio::test]
    async fn docker_backend_stderr_capture() {
        if !docker_ready().await {
            eprintln!("SKIP: Docker daemon not available");
            return;
        }
        let b = DockerBackend::new("test-docker-stderr", default_config());
        let Some(out) = try_exec(&b, "echo err >&2 && echo out").await else {
            return;
        };
        assert!(out.stdout.contains("out"));
        assert!(out.stderr.contains("err"));
        b.cleanup().await.expect("cleanup");
    }
}
