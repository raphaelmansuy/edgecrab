//! # Daytona execution backend
//!
//! This backend drives the Daytona Python SDK through a tiny helper bridge.
//! That keeps the Rust backend self-contained while still using the official
//! SDK semantics Hermes relies on.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::process::Command;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

use edgecrab_types::ToolError;

use super::{BackendKind, DaytonaBackendConfig, ExecOutput, ExecutionBackend};

const DAYTONA_HELPER: &str = r#"
import json
import math
import os
import shlex
import sys

def emit(obj, code=0):
    sys.stdout.write(json.dumps(obj))
    sys.exit(code)

try:
    from daytona import Daytona, CreateSandboxFromImageParams, Resources, SandboxState, DaytonaError
except Exception as e:
    emit({
        "ok": False,
        "error": f"Daytona SDK unavailable: {e}. Install with `pip install daytona` and set DAYTONA_API_KEY."
    }, 1)

action = os.environ["EDGECRAB_DAYTONA_ACTION"]
task_id = os.environ["EDGECRAB_DAYTONA_TASK_ID"]
image = os.environ["EDGECRAB_DAYTONA_IMAGE"]
cpu = max(1, int(os.environ.get("EDGECRAB_DAYTONA_CPU", "1")))
memory_mb = max(1, int(os.environ.get("EDGECRAB_DAYTONA_MEMORY_MB", "5120")))
disk_mb = max(1, int(os.environ.get("EDGECRAB_DAYTONA_DISK_MB", "10240")))
persistent = os.environ.get("EDGECRAB_DAYTONA_PERSISTENT", "1") == "1"

daytona = Daytona()
sandbox_name = f"edgecrab-{task_id}"
labels = {"edgecrab_task_id": task_id}

def _state_name(value):
    return getattr(value, "name", str(value))

def ensure_sandbox():
    sandbox = None
    if persistent:
        try:
            sandbox = daytona.get(sandbox_name)
        except Exception:
            sandbox = None

        if sandbox is None:
            try:
                page = daytona.list(labels=labels, page=1, limit=1)
                items = getattr(page, "items", None) or []
                if items:
                    sandbox = items[0]
            except Exception:
                sandbox = None

    if sandbox is None:
        memory_gib = max(1, math.ceil(memory_mb / 1024))
        disk_gib = max(1, min(10, math.ceil(disk_mb / 1024)))
        resources = Resources(cpu=cpu, memory=memory_gib, disk=disk_gib)
        sandbox = daytona.create(
            CreateSandboxFromImageParams(
                image=image,
                name=sandbox_name,
                labels=labels,
                auto_stop_interval=0,
                resources=resources,
            )
        )

    try:
        state = getattr(sandbox, "state", None)
        if state is not None:
            stopped = {"STOPPED", "ARCHIVED"}
            if _state_name(state) in stopped:
                sandbox.start()
    except Exception:
        pass

    return sandbox

try:
    if action == "exec":
        command = os.environ["EDGECRAB_DAYTONA_COMMAND"]
        cwd = os.environ.get("EDGECRAB_DAYTONA_CWD") or None
        timeout = max(1, int(os.environ.get("EDGECRAB_DAYTONA_TIMEOUT", "60")))
        sandbox = ensure_sandbox()
        wrapped = f"timeout {timeout} sh -c {shlex.quote(command)}"
        response = sandbox.process.exec(wrapped, cwd=cwd)
        emit({
            "ok": True,
            "stdout": getattr(response, "result", "") or "",
            "stderr": "",
            "exit_code": int(getattr(response, "exit_code", 0) or 0),
        })
    elif action == "cleanup":
        sandbox = ensure_sandbox()
        if persistent:
            sandbox.stop()
        else:
            daytona.delete(sandbox)
        emit({"ok": True})
    else:
        emit({"ok": False, "error": f"unknown action: {action}"}, 1)
except Exception as e:
    emit({"ok": False, "error": str(e)}, 1)
"#;

#[derive(Debug, Deserialize)]
struct HelperResponse {
    ok: bool,
    stdout: Option<String>,
    stderr: Option<String>,
    exit_code: Option<i32>,
    error: Option<String>,
}

fn python_executable() -> String {
    std::env::var("EDGECRAB_DAYTONA_PYTHON").unwrap_or_else(|_| "python3".into())
}

async fn run_helper(
    action: &str,
    task_id: &str,
    cfg: &DaytonaBackendConfig,
    command: Option<&str>,
    cwd: Option<&str>,
    timeout: Duration,
    cancel: CancellationToken,
) -> Result<HelperResponse, ToolError> {
    let mut helper = Command::new(python_executable());
    helper.arg("-c").arg(DAYTONA_HELPER);
    helper
        .env("EDGECRAB_DAYTONA_ACTION", action)
        .env("EDGECRAB_DAYTONA_TASK_ID", task_id)
        .env("EDGECRAB_DAYTONA_IMAGE", &cfg.image)
        .env("EDGECRAB_DAYTONA_CPU", cfg.cpu.to_string())
        .env("EDGECRAB_DAYTONA_MEMORY_MB", cfg.memory_mb.to_string())
        .env("EDGECRAB_DAYTONA_DISK_MB", cfg.disk_mb.to_string())
        .env(
            "EDGECRAB_DAYTONA_PERSISTENT",
            if cfg.persistent_filesystem { "1" } else { "0" },
        )
        .kill_on_drop(true);

    if let Some(command) = command {
        helper.env("EDGECRAB_DAYTONA_COMMAND", command);
    }
    if let Some(cwd) = cwd {
        helper.env("EDGECRAB_DAYTONA_CWD", cwd);
    }
    helper.env(
        "EDGECRAB_DAYTONA_TIMEOUT",
        timeout.as_secs().max(1).to_string(),
    );

    let fut = helper.output();
    tokio::pin!(fut);

    let output = tokio::select! {
        res = tokio::time::timeout(timeout + Duration::from_secs(5), &mut fut) => {
            match res {
                Ok(Ok(output)) => output,
                Ok(Err(e)) => return Err(ToolError::ExecutionFailed {
                    tool: "terminal".into(),
                    message: format!("Daytona helper spawn failed: {e}"),
                }),
                Err(_) => {
                    return Ok(HelperResponse {
                        ok: true,
                        stdout: Some(String::new()),
                        stderr: Some(String::new()),
                        exit_code: Some(124),
                        error: None,
                    });
                }
            }
        }
        _ = cancel.cancelled() => {
            return Ok(HelperResponse {
                ok: true,
                stdout: Some(String::new()),
                stderr: Some(String::new()),
                exit_code: Some(130),
                error: None,
            });
        }
    };

    serde_json::from_slice::<HelperResponse>(&output.stdout).map_err(|e| {
        ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!(
                "Daytona helper returned invalid JSON: {e}. stderr={}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }
    })
}

pub struct DaytonaBackend {
    config: DaytonaBackendConfig,
    task_id: String,
    dead: Arc<AtomicBool>,
    cleanup_lock: TokioMutex<()>,
}

impl DaytonaBackend {
    pub fn new(task_id: impl Into<String>, config: DaytonaBackendConfig) -> Self {
        Self {
            config,
            task_id: task_id.into(),
            dead: Arc::new(AtomicBool::new(false)),
            cleanup_lock: TokioMutex::new(()),
        }
    }
}

#[async_trait]
impl ExecutionBackend for DaytonaBackend {
    async fn execute(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        if self.dead.load(Ordering::Relaxed) {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Daytona backend has been cleaned up".into(),
            });
        }

        let response = run_helper(
            "exec",
            &self.task_id,
            &self.config,
            Some(command),
            Some(cwd),
            timeout,
            cancel,
        )
        .await?;

        if !response.ok {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: response
                    .error
                    .unwrap_or_else(|| "Daytona helper failed".into()),
            });
        }

        Ok(ExecOutput {
            stdout: response.stdout.unwrap_or_default(),
            stderr: response.stderr.unwrap_or_default(),
            exit_code: response.exit_code.unwrap_or(0),
        })
    }

    async fn cleanup(&self) -> Result<(), ToolError> {
        let _guard = self.cleanup_lock.lock().await;
        if self.dead.swap(true, Ordering::Relaxed) {
            return Ok(());
        }
        let response = run_helper(
            "cleanup",
            &self.task_id,
            &self.config,
            None,
            None,
            Duration::from_secs(30),
            CancellationToken::new(),
        )
        .await?;

        if response.ok {
            Ok(())
        } else {
            Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: response
                    .error
                    .unwrap_or_else(|| "Daytona cleanup failed".into()),
            })
        }
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Daytona
    }

    async fn is_healthy(&self) -> bool {
        !self.dead.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn write_fake_python(dir: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.path().join("fake-python");
        std::fs::write(&path, body).expect("write fake python");
        let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
        path
    }

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn fake_helper_executes_command() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let fake = write_fake_python(
            &dir,
            r#"#!/bin/sh
set -eu
if [ "${EDGECRAB_DAYTONA_ACTION:-}" = "exec" ]; then
  printf '{"ok":true,"stdout":"day:%s","stderr":"","exit_code":0}' "$EDGECRAB_DAYTONA_COMMAND"
else
  printf '{"ok":true}'
fi
"#,
        );
        unsafe { std::env::set_var("EDGECRAB_DAYTONA_PYTHON", &fake) };

        let backend = DaytonaBackend::new("day-e2e", DaytonaBackendConfig::default());
        let out = backend
            .execute(
                "echo hello-daytona",
                "/workspace",
                Duration::from_secs(2),
                CancellationToken::new(),
            )
            .await
            .expect("execute");
        assert!(
            out.stdout.contains("day:echo hello-daytona"),
            "got: {out:?}"
        );
        backend.cleanup().await.expect("cleanup");

        unsafe { std::env::remove_var("EDGECRAB_DAYTONA_PYTHON") };
    }

    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn fake_helper_cleanup_marks_backend_dead() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("cleanup.log");
        let fake = write_fake_python(
            &dir,
            r#"#!/bin/sh
set -eu
if [ "${EDGECRAB_DAYTONA_ACTION:-}" = "cleanup" ]; then
  printf '%s\n' "${EDGECRAB_DAYTONA_PERSISTENT:-}" >> "${EDGECRAB_TEST_LOG}"
fi
printf '{"ok":true}'
"#,
        );
        unsafe {
            std::env::set_var("EDGECRAB_DAYTONA_PYTHON", &fake);
            std::env::set_var("EDGECRAB_TEST_LOG", &log);
        }

        let backend = DaytonaBackend::new("day-cleanup", DaytonaBackendConfig::default());
        backend.cleanup().await.expect("cleanup");
        assert!(!backend.is_healthy().await);
        let logged = std::fs::read_to_string(&log).expect("read log");
        assert!(logged.contains('1'));

        unsafe {
            std::env::remove_var("EDGECRAB_DAYTONA_PYTHON");
            std::env::remove_var("EDGECRAB_TEST_LOG");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn fake_helper_timeout_returns_124() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let fake = write_fake_python(
            &dir,
            r#"#!/bin/sh
set -eu
sleep 10
"#,
        );
        unsafe { std::env::set_var("EDGECRAB_DAYTONA_PYTHON", &fake) };

        let backend = DaytonaBackend::new("day-timeout", DaytonaBackendConfig::default());
        let out = backend
            .execute(
                "sleep forever",
                "/workspace",
                Duration::from_millis(50),
                CancellationToken::new(),
            )
            .await
            .expect("execute");
        assert_eq!(out.exit_code, 124);
        let _ = backend.cleanup().await;

        unsafe { std::env::remove_var("EDGECRAB_DAYTONA_PYTHON") };
    }
}
