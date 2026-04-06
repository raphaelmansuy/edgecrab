//! # SSH execution backend (gap/backend B-04)
//!
//! Executes commands on a remote host via SSH using the `openssh` crate
//! (Tokio-native, ControlMaster multiplexed).
//!
//! ## Design
//!
//! ```text
//!   ┌────────────────────────────────────────────────┐
//!   │  SshBackend                                    │
//!   │                                                │
//!   │  openssh::Session::connect_mux()  ────────┐   │
//!   │  (ControlMaster socket per task)           │   │
//!   │                                            ▼   │
//!   │  ┌───────────────────────────────────────────┐ │
//!   │  │  execute():                               │ │
//!   │  │  1. session.command(["sh", "-c", cmd])   │ │
//!   │  │  2. .output() → stdout, stderr           │ │
//!   │  │  3. parse exit_status                    │ │
//!   │  └───────────────────────────────────────────┘ │
//!   └────────────────────────────────────────────────┘
//! ```
//!
//! ## Why `openssh` vs raw `ssh -o ControlMaster`?
//! - Native Tokio integration: no blocking thread, no `std::process::Command`
//! - `KnownHosts::Accept` / `StrictHostKeyChecking` configurable per run
//! - Automatic ControlMaster socket management (no `/tmp` cleanup required)
//!
//! ## Env-var blocklist
//! SSH does not propagate the local env to the remote by default (sshd's
//! `AcceptEnv`). We still strip secrets from any env we manually export.
//!
//! ## ControlMaster multiplex
//! The first `execute()` call establishes the master connection; subsequent
//! calls reuse the mux socket — so latency for later calls is ~0.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use openssh::{KnownHosts, Session, SessionBuilder};
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tracing::info;

use edgecrab_types::ToolError;

use crate::execution_tmp::{BACKEND_TMP_ROOT, wrap_command_with_tmp_env};

use super::{BackendKind, ExecOutput, ExecutionBackend, SshBackendConfig};

// ─── SshState ─────────────────────────────────────────────────────────

struct SshState {
    session: Session,
    dead: Arc<AtomicBool>,
}

impl SshState {
    async fn new(cfg: &SshBackendConfig, task_id: &str) -> Result<Self, ToolError> {
        let mut builder = SessionBuilder::default();

        if let Some(ref key) = cfg.key_path {
            builder.keyfile(key);
        }

        builder.user(cfg.user.as_deref().unwrap_or("root").to_string());
        builder.port(cfg.port.unwrap_or(22));

        // Use strict / accept depending on config; default relaxed for scratch hosts
        if cfg.strict_host_checking.unwrap_or(false) {
            builder.known_hosts_check(KnownHosts::Strict);
        } else {
            builder.known_hosts_check(KnownHosts::Accept);
        }

        builder.control_directory(std::env::temp_dir().join(format!("edgecrab-ssh-{task_id}")));

        let session =
            builder
                .connect(cfg.host.as_str())
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: format!("SSH connect failed to {}: {e}", cfg.host),
                })?;

        info!("SshBackend: connected to {} (task={task_id})", cfg.host);

        Ok(Self {
            session,
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
                message: "SSH session is dead".into(),
            });
        }

        // Wrap command with optional cd
        let full_cmd = if !cwd.is_empty() && cwd != "." {
            format!("cd {} && {}", shell_escape(cwd), command)
        } else {
            command.to_string()
        };
        let full_cmd = wrap_command_with_tmp_env(&full_cmd, BACKEND_TMP_ROOT);

        let exec_fut = async {
            let mut remote_cmd = self.session.command("sh");
            remote_cmd.arg("-c").arg(&full_cmd);

            remote_cmd
                .output()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: format!("SSH exec error: {e}"),
                })
        };

        tokio::select! {
            res = tokio::time::timeout(timeout, exec_fut) => {
                match res {
                    Ok(Ok(output)) => {
                        let exit_code = output.status.code().unwrap_or(-1);
                        Ok(ExecOutput {
                            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                            exit_code,
                        })
                    }
                    Ok(Err(e)) => Err(e),
                    Err(_) => {
                        // Timed out
                        Ok(ExecOutput {
                            stdout: String::new(),
                            stderr: String::new(),
                            exit_code: 124,
                        })
                    }
                }
            }
            _ = cancel.cancelled() => {
                Ok(ExecOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 130,
                })
            }
        }
    }

    async fn close(self) {
        let _ = self.session.close().await;
    }
}

/// Minimally escape a shell path for `cd <path>` (wrap in single-quotes and
/// escape any internal single-quotes).
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

// ─── SshBackend ───────────────────────────────────────────────────────

pub struct SshBackend {
    config: SshBackendConfig,
    task_id: String,
    state: TokioMutex<Option<Arc<SshState>>>,
}

impl SshBackend {
    pub fn new(task_id: impl Into<String>, config: SshBackendConfig) -> Self {
        Self {
            task_id: task_id.into(),
            config,
            state: TokioMutex::new(None),
        }
    }

    async fn ensure_state(&self) -> Result<Arc<SshState>, ToolError> {
        let mut guard = self.state.lock().await;
        let needs_init = guard
            .as_ref()
            .map(|s| s.dead.load(Ordering::Relaxed))
            .unwrap_or(true);

        if needs_init {
            *guard = Some(Arc::new(SshState::new(&self.config, &self.task_id).await?));
        }
        guard.clone().ok_or_else(|| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: "SSH state missing after init — this is a bug".into(),
        })
    }
}

#[async_trait]
impl ExecutionBackend for SshBackend {
    async fn execute(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        let state = self.ensure_state().await?;
        state.exec(command, cwd, timeout, cancel).await
    }

    async fn execute_oneshot(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        let state = self.ensure_state().await?;
        state.exec(command, cwd, timeout, cancel).await
    }

    async fn cleanup(&self) -> Result<(), ToolError> {
        let mut guard = self.state.lock().await;
        if let Some(state) = guard.take() {
            if let Ok(state) = Arc::try_unwrap(state) {
                state.close().await;
            } else {
                tracing::warn!(
                    task_id = %self.task_id,
                    "SSH backend cleanup deferred because commands are still holding the session"
                );
            }
        }
        Ok(())
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Ssh
    }

    fn supports_remote_execute_code(&self) -> bool {
        true
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("/home/user/my path"), "'/home/user/my path'");
    }

    #[test]
    fn shell_escape_single_quote() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    /// SSH integration tests require a real SSH host; skip gracefully when
    /// the environment variable EDGECRAB_TEST_SSH_HOST is not set.
    async fn ssh_available() -> Option<SshBackendConfig> {
        let host = std::env::var("EDGECRAB_TEST_SSH_HOST").ok()?;
        Some(SshBackendConfig {
            host,
            user: std::env::var("EDGECRAB_TEST_SSH_USER").ok(),
            port: None,
            key_path: std::env::var("EDGECRAB_TEST_SSH_KEY").ok().map(Into::into),
            strict_host_checking: Some(false),
        })
    }

    #[tokio::test]
    async fn ssh_backend_echo() {
        let Some(cfg) = ssh_available().await else {
            eprintln!("SKIP: EDGECRAB_TEST_SSH_HOST not set");
            return;
        };
        let b = SshBackend::new("test-ssh-echo", cfg);
        let out = b
            .execute(
                "echo ssh-hello",
                "/tmp",
                Duration::from_secs(10),
                CancellationToken::new(),
            )
            .await
            .expect("execute");
        assert!(out.stdout.contains("ssh-hello"));
        assert_eq!(out.exit_code, 0);
        b.cleanup().await.expect("cleanup");
    }

    #[tokio::test]
    async fn ssh_backend_exit_code() {
        let Some(cfg) = ssh_available().await else {
            eprintln!("SKIP: EDGECRAB_TEST_SSH_HOST not set");
            return;
        };
        let b = SshBackend::new("test-ssh-exit", cfg);
        let out = b
            .execute(
                "exit 5",
                "/tmp",
                Duration::from_secs(10),
                CancellationToken::new(),
            )
            .await
            .expect("execute");
        assert_eq!(out.exit_code, 5);
        b.cleanup().await.expect("cleanup");
    }
}
