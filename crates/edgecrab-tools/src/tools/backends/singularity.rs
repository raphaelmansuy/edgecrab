//! # Singularity / Apptainer execution backend
//!
//! Executes commands inside a persistent Apptainer/Singularity instance.
//!
//! Design goals:
//! - actionable preflight errors when the runtime is missing
//! - task-scoped persistent instances
//! - persistent overlays when requested
//! - interrupt / timeout handling aligned with the other backends

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tracing::info;

use edgecrab_types::ToolError;

use super::{
    BackendKind, ExecOutput, ExecutionBackend, SingularityBackendConfig, ensure_dir,
    sandbox_root_dir, sanitize_resource_name, shell_quote,
};

fn resolve_executable() -> Result<String, ToolError> {
    if let Ok(explicit) = std::env::var("EDGECRAB_SINGULARITY_BIN") {
        if !explicit.trim().is_empty() {
            return Ok(explicit);
        }
    }

    if let Ok(path) = which::which("apptainer") {
        return Ok(path.to_string_lossy().into_owned());
    }
    if let Ok(path) = which::which("singularity") {
        return Ok(path.to_string_lossy().into_owned());
    }

    Err(ToolError::ExecutionFailed {
        tool: "terminal".into(),
        message: "Singularity backend selected but neither `apptainer` nor `singularity` was found in PATH. Install Apptainer or set EDGECRAB_SINGULARITY_BIN.".into(),
    })
}

async fn verify_executable(executable: &str) -> Result<(), ToolError> {
    let output = Command::new(executable)
        .arg("version")
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("Failed to run `{executable} version`: {e}"),
        })?;

    if output.status.success() {
        return Ok(());
    }

    Err(ToolError::ExecutionFailed {
        tool: "terminal".into(),
        message: format!(
            "`{executable} version` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    })
}

fn overlay_dir(task_id: &str) -> Result<std::path::PathBuf, ToolError> {
    let dir = sandbox_root_dir()
        .join("singularity")
        .join(format!("overlay-{}", sanitize_resource_name(task_id)));
    ensure_dir(&dir, "singularity overlay")?;
    Ok(dir)
}

#[derive(Debug)]
struct SingularityState {
    executable: String,
    instance_name: String,
    dead: Arc<AtomicBool>,
}

impl SingularityState {
    async fn new(cfg: &SingularityBackendConfig, task_id: &str) -> Result<Self, ToolError> {
        let executable = resolve_executable()?;
        verify_executable(&executable).await?;

        let instance_name = format!("edgecrab-{}", sanitize_resource_name(task_id));

        // Make startup idempotent across stale instances.
        let _ = Command::new(&executable)
            .args(["instance", "stop", &instance_name])
            .kill_on_drop(true)
            .output()
            .await;

        let mut cmd = Command::new(&executable);
        cmd.arg("instance")
            .arg("start")
            .arg("--containall")
            .arg("--no-home");

        if cfg.persistent_filesystem {
            let overlay = overlay_dir(task_id)?;
            cmd.arg("--overlay").arg(overlay);
        } else {
            cmd.arg("--writable-tmpfs");
        }

        if cfg.memory_mb > 0 {
            cmd.arg("--memory").arg(format!("{}M", cfg.memory_mb));
        }
        if cfg.cpu > 0.0 {
            cmd.arg("--cpus").arg(cfg.cpu.to_string());
        }

        let output = cmd
            .arg(&cfg.image)
            .arg(&instance_name)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Failed to start Singularity instance: {e}"),
            })?;

        if !output.status.success() {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!(
                    "Singularity instance start failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }

        info!("SingularityBackend: started instance {instance_name}");

        Ok(Self {
            executable,
            instance_name,
            dead: Arc::new(AtomicBool::new(false)),
        })
    }

    async fn exec(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        if self.dead.load(Ordering::Relaxed) {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Singularity instance is stopped".into(),
            });
        }

        let mut workdir = if cwd.is_empty() { "/tmp" } else { cwd }.to_string();
        let mut exec_command = command.to_string();
        if workdir == "~" || workdir.starts_with("~/") {
            exec_command = format!("cd {} && {}", shell_quote(&workdir), exec_command);
            workdir = "/tmp".into();
        }

        let mut cmd = Command::new(&self.executable);
        cmd.arg("exec")
            .arg("--pwd")
            .arg(&workdir)
            .arg(format!("instance://{}", self.instance_name))
            .arg("bash")
            .arg("-c")
            .arg(exec_command)
            .kill_on_drop(true);

        let fut = cmd.output();
        tokio::pin!(fut);

        tokio::select! {
            res = tokio::time::timeout(timeout, &mut fut) => {
                match res {
                    Ok(Ok(output)) => Ok(ExecOutput {
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                        exit_code: output.status.code().unwrap_or(-1),
                    }),
                    Ok(Err(e)) => Err(ToolError::ExecutionFailed {
                        tool: "terminal".into(),
                        message: format!("Singularity exec failed: {e}"),
                    }),
                    Err(_) => Ok(ExecOutput {
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 124,
                    }),
                }
            }
            _ = cancel.cancelled() => Ok(ExecOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 130,
            })
        }
    }

    async fn stop(&self) {
        let _ = Command::new(&self.executable)
            .args(["instance", "stop", &self.instance_name])
            .kill_on_drop(true)
            .output()
            .await;
        self.dead.store(true, Ordering::Relaxed);
    }
}

pub struct SingularityBackend {
    config: SingularityBackendConfig,
    task_id: String,
    state: TokioMutex<Option<SingularityState>>,
}

impl SingularityBackend {
    pub fn new(task_id: impl Into<String>, config: SingularityBackendConfig) -> Self {
        Self {
            config,
            task_id: task_id.into(),
            state: TokioMutex::new(None),
        }
    }

    async fn ensure_state(&self) -> Result<(), ToolError> {
        let mut guard = self.state.lock().await;
        let needs_init = guard
            .as_ref()
            .map(|s| s.dead.load(Ordering::Relaxed))
            .unwrap_or(true);
        if needs_init {
            *guard = Some(SingularityState::new(&self.config, &self.task_id).await?);
        }
        Ok(())
    }
}

#[async_trait]
impl ExecutionBackend for SingularityBackend {
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
            state.exec(command, cwd, timeout, cancel).await
        } else {
            Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Singularity state missing after init".into(),
            })
        }
    }

    async fn cleanup(&self) -> Result<(), ToolError> {
        let mut guard = self.state.lock().await;
        if let Some(state) = guard.take() {
            state.stop().await;
        }
        Ok(())
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Singularity
    }

    async fn is_healthy(&self) -> bool {
        let guard = self.state.lock().await;
        match guard.as_ref() {
            Some(s) => !s.dead.load(Ordering::Relaxed),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn write_fake_runtime(dir: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.path().join("fake-apptainer");
        std::fs::write(&path, body).expect("write fake runtime");
        let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
        path
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn missing_runtime_is_actionable() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("EDGECRAB_SINGULARITY_BIN", "/definitely/missing/apptainer") };
        let err = SingularityState::new(&SingularityBackendConfig::default(), "missing")
            .await
            .expect_err("missing runtime must fail");
        let msg = err.to_string();
        assert!(msg.contains("Failed to run"), "got: {msg}");
        unsafe { std::env::remove_var("EDGECRAB_SINGULARITY_BIN") };
    }

    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn fake_runtime_executes_and_cleans_up() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("runtime.log");
        let fake = write_fake_runtime(
            &dir,
            r#"#!/bin/sh
set -eu
log="${EDGECRAB_SINGULARITY_TEST_LOG:-}"
case "${1:-}" in
  version)
    echo "apptainer 1.0.0"
    ;;
  instance)
    echo "instance:$*" >> "$log"
    ;;
  exec)
    echo "exec:$*" >> "$log"
    last=""
    for arg in "$@"; do last="$arg"; done
    if [ "$last" = "exit 7" ]; then
      exit 7
    fi
    printf 'sg:%s\n' "$last"
    ;;
  *)
    echo "unexpected:$*" >> "$log"
    ;;
esac
"#,
        );

        unsafe {
            std::env::set_var("EDGECRAB_SINGULARITY_BIN", &fake);
            std::env::set_var("EDGECRAB_SINGULARITY_TEST_LOG", &log);
        }

        let backend = SingularityBackend::new("sg-e2e", SingularityBackendConfig::default());
        let out = backend
            .execute(
                "echo hello-singularity",
                "/workspace",
                Duration::from_secs(2),
                CancellationToken::new(),
            )
            .await
            .expect("execute");
        assert!(
            out.stdout.contains("sg:echo hello-singularity"),
            "got: {out:?}"
        );

        let out = backend
            .execute(
                "exit 7",
                "/workspace",
                Duration::from_secs(2),
                CancellationToken::new(),
            )
            .await
            .expect("execute exit");
        assert_eq!(out.exit_code, 7);

        backend.cleanup().await.expect("cleanup");
        let logged = std::fs::read_to_string(&log).expect("read log");
        assert!(logged.contains("instance:instance start"), "got: {logged}");
        assert!(logged.contains("instance:instance stop"), "got: {logged}");

        unsafe {
            std::env::remove_var("EDGECRAB_SINGULARITY_BIN");
            std::env::remove_var("EDGECRAB_SINGULARITY_TEST_LOG");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn fake_runtime_timeout_returns_124() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let fake = write_fake_runtime(
            &dir,
            r#"#!/bin/sh
set -eu
case "${1:-}" in
  version) echo ok ;;
  instance) exit 0 ;;
  exec) sleep 5 ;;
esac
"#,
        );
        unsafe { std::env::set_var("EDGECRAB_SINGULARITY_BIN", &fake) };

        let backend = SingularityBackend::new("sg-timeout", SingularityBackendConfig::default());
        let out = backend
            .execute(
                "sleep forever",
                "/workspace",
                Duration::from_millis(50),
                CancellationToken::new(),
            )
            .await
            .expect("timeout execute");
        assert_eq!(out.exit_code, 124);
        backend.cleanup().await.expect("cleanup");

        unsafe { std::env::remove_var("EDGECRAB_SINGULARITY_BIN") };
    }
}
