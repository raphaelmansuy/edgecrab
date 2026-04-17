//! # terminal — Execute shell commands
//!
//! WHY shell execution: The agent needs to compile, test, install, and
//! run programs. This is the most powerful (and dangerous) tool — all
//! security checks go through edgecrab-security's command scanner.
//!
//! ## Backend dispatch (gap/backend)
//!
//! Commands are dispatched through the pluggable backend system:
//!   - Local (default)  — persistent shell + env-var blocklist (B-02, B-03)
//!   - Docker           — bollard container per task (B-01a)
//!   - SSH              — openssh ControlMaster channel (B-04)
//!   - Modal            — Modal cloud sandbox REST API (B-01b)
//!   - Daytona          — persistent cloud sandbox via Daytona SDK helper
//!   - Singularity      — Apptainer/Singularity instance with persistent overlay
//!
//! The active backend is selected by `ctx.config.terminal_backend` which maps
//! to `EDGECRAB_TERMINAL_BACKEND` env var or `config.yaml terminal.backend`.
//!
//! Backend instances are cached in a global `DashMap<task_id, Arc<dyn ExecutionBackend>>`
//! so the persistent shell / Docker container / SSH session is reused across
//! consecutive `execute()` calls within the same task.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

use edgecrab_types::{ToolError, ToolSchema};

use crate::describe_execution_filesystem;
use crate::registry::{ToolContext, ToolHandler};
use crate::tools::backend_pool::{get_or_create_backend, resolve_workdir};
#[cfg(test)]
use crate::tools::backends::ExecutionBackend;
use crate::tools::backends::redact_output;
use crate::tools::checkpoint::ensure_checkpoint;

fn validate_backend_workdir_visibility(
    backend: &crate::tools::backends::BackendKind,
    cwd: &str,
) -> Result<(), ToolError> {
    let cwd_path = std::path::Path::new(cwd);
    if !cwd_path.is_absolute() {
        return Ok(());
    }

    match backend {
        crate::tools::backends::BackendKind::Modal
        | crate::tools::backends::BackendKind::Daytona
            if cwd_path.exists() =>
        {
            let cfg = crate::config_ref::AppConfigRef {
                terminal_backend: backend.clone(),
                ..Default::default()
            };
            let fs = describe_execution_filesystem(&cfg, cwd_path);
            let allowed_roots = fs.file_roots_display();
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!(
                    "The {} backend cannot access host workspace path '{}'. File tools in this session are rooted at {}. Use the local or docker backend for local files, or sync the workspace into the remote sandbox first.",
                    backend, cwd, allowed_roots
                ),
            });
        }
        _ => {}
    }

    Ok(())
}

fn terminal_result_header(
    backend: &crate::tools::backends::BackendKind,
    cwd: &str,
    exit_code: i32,
) -> String {
    let status = if exit_code == 0 { "success" } else { "error" };
    format!(
        "[terminal_result status={status} backend={} cwd={} exit_code={exit_code}]",
        backend, cwd
    )
}

pub async fn cleanup_all_backends() -> usize {
    crate::tools::backend_pool::cleanup_all_backends().await
}

pub async fn cleanup_backend_for_task(task_id: &str) -> bool {
    crate::tools::backend_pool::cleanup_backend_for_task(task_id).await
}

pub async fn cleanup_inactive_backends(max_idle: Duration) -> usize {
    crate::tools::backend_pool::cleanup_inactive_backends(max_idle).await
}

// ─── TerminalTool ─────────────────────────────────────────────────────

pub struct TerminalTool;

#[derive(Deserialize)]
struct Args {
    command: String,
    #[serde(default)]
    background: bool,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    pty: bool,
    /// Optional per-command working directory override.
    ///
    /// If relative, resolved against the task working directory (ctx.cwd).
    /// If omitted, the task working directory is used.
    workdir: Option<String>,
}

fn default_timeout() -> u64 {
    120
}

fn effective_timeout_seconds(args: &Args) -> u64 {
    args.timeout.unwrap_or(args.timeout_seconds)
}

#[async_trait]
impl ToolHandler for TerminalTool {
    fn name(&self) -> &'static str {
        "terminal"
    }

    fn toolset(&self) -> &'static str {
        "terminal"
    }

    fn emoji(&self) -> &'static str {
        "💻"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "terminal".into(),
            description: "Execute a shell command and return combined stdout+stderr output. \
                          Commands run in a persistent bash shell, so state like environment \
                          variables, `cd`, and shell functions persist across consecutive calls. \
                          Do not use cat/head/tail to read files — use `read_file` instead. \
                          Do not use grep/rg/find/ls for repo discovery — use `search_files` instead. \
                          Do not use sed/awk for file edits — use `patch`/`apply_patch` instead. \
                          Use `workdir` to override the working directory for a single call. \
                          Supports legacy background=true calls, which delegate to the shared background process path. \
                          For long-running processes (servers, watchers) use background=true or `run_process`. \
                          Do not use shell heredocs (`<<EOF`) in terminal. For file creation or edits, \
                          use `write_file`, `patch`, or `apply_patch` instead of embedding file content \
                          inside the command string. \
                          PTY mode is available on the local backend for line-oriented interactive CLIs that need TTY semantics or prompt-style output. \
                          PTY calls are per-command and do not reuse the persistent shell. Full-screen terminal UIs remain unsupported because the agent does not observe screen state. \
                          The exit code is appended to the output as `exit code: N`; non-zero \
                          means the command failed."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute. Multi-line scripts are supported."
                    },
                    "background": {
                        "type": "boolean",
                        "description": "Background mode for servers/watchers. Returns immediately with a process id."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Maximum seconds to wait for the command to finish (default: 120, max: 600)."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout alias in seconds."
                    },
                    "pty": {
                        "type": "boolean",
                        "description": "Allocate a PTY for local interactive CLI commands that need TTY semantics or prompt-style output. Local backend only. Full-screen TUIs remain unsupported."
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Override working directory for this command only. \
                                        Absolute paths are used as-is; relative paths are \
                                        resolved from the task working directory."
                    }
                },
                "required": ["command"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "terminal".into(),
            message: e.to_string(),
        })?;

        if args.background {
            let started = crate::tools::process::start_background_process(
                "terminal",
                &args.command,
                args.workdir.as_deref(),
                args.pty,
                Vec::new(), // terminal tool doesn't support watch_patterns
                ctx,
            )
            .await?;
            return Ok(
                if args.timeout.is_some() || args.timeout_seconds != default_timeout() {
                    format!(
                        "{started}\n[terminal note] background mode ignores terminal timeout; use process(action=\"wait\", session_id=..., timeout=...) to block for completion."
                    )
                } else {
                    started
                },
            );
        }

        // Auto-checkpoint before potentially destructive commands
        if is_destructive_command(&args.command) {
            ensure_checkpoint(ctx, &destructive_checkpoint_label(&args.command));
        }

        // Security: scan command for dangerous patterns via Aho-Corasick scanner
        if let Some(reasons) = crate::approval_runtime::command_approval_reasons(ctx, &args.command)
        {
            crate::approval_runtime::request_command_approval(ctx, &args.command, reasons).await?;
        }

        crate::command_interaction::guard_terminal_command(
            &args.command,
            &ctx.config.terminal_backend,
            args.pty,
        )?;

        let timeout = Duration::from_secs(effective_timeout_seconds(&args).min(600)); // hard cap 10 min

        if args.pty && ctx.config.terminal_backend != crate::tools::backends::BackendKind::Local {
            return Err(
                ToolError::capability_denied(
                    "terminal",
                    "pty_backend_unsupported",
                    format!(
                        "PTY mode is only available on the local terminal backend. The active backend is {}.",
                        ctx.config.terminal_backend
                    ),
                )
                .with_suggested_action(
                    "Switch to the local backend for PTY commands, or rerun the command without PTY."
                        .to_string(),
                ),
            );
        }

        // Resolve the working directory: per-command workdir overrides ctx.cwd.
        let cwd = resolve_workdir(ctx, args.workdir.as_deref());
        validate_backend_workdir_visibility(&ctx.config.terminal_backend, &cwd)?;

        // Get or create backend for this task
        // Sudo transform: rewrite `sudo` → `sudo -S -p ''` when SUDO_PASSWORD
        // is set so commands can run non-interactively. This matches the
        // `_transform_sudo_command()`.  The persistent shell cannot pipe an
        // arbitrary stdin mid-execution, so we wrap the password delivery in a
        // process-substitution heredoc that is invisible in /proc cmdline args.
        let (effective_command, sudo_prefix) =
            crate::tools::backends::transform_sudo(&args.command);
        let final_command = if let Some(prefix) = sudo_prefix {
            // Escape the password for use inside a single-quoted shell string.
            // Using `printf '%s\n'` avoids `echo` which may interpret escape
            // sequences on some systems.
            let escaped = prefix.trim_end_matches('\n').replace('\'', "'\\''");
            format!(
                "{{ printf '%s\\n' '{}'; }} | {}",
                escaped, effective_command
            )
        } else {
            effective_command
        };

        // Execute via backend with up to 3 retries on transient infrastructure
        // errors. This matches the existing exponential-backoff retry logic: wait
        // 2^attempt seconds between attempts (2 s, 4 s, 8 s).
        const MAX_RETRIES: u32 = 3;
        let mut attempt = 0u32;
        let exec_output = if args.pty {
            crate::local_pty::execute_foreground(
                "terminal",
                &final_command,
                &cwd,
                timeout,
                ctx.cancel.clone(),
            )
            .await?
        } else {
            let backend = get_or_create_backend(ctx).await?;
            loop {
                let result = backend
                    .execute(&final_command, &cwd, timeout, ctx.cancel.clone())
                    .await;

                // Check for retryable errors before consuming the value.
                if let Err(ref e) = result {
                    if attempt < MAX_RETRIES && is_backend_retryable(e) {
                        let wait = Duration::from_secs(1u64 << (attempt + 1)); // 2, 4, 8 s
                        tracing::warn!(
                            task_id = %ctx.task_id,
                            attempt = attempt + 1,
                            wait_secs = wait.as_secs(),
                            error = %e,
                            "terminal backend error; retrying",
                        );
                        attempt += 1;
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                }

                match result {
                    Ok(out) => break out,
                    Err(e) => return Err(e),
                }
            }
        };

        crate::command_interaction::rewrite_terminal_exec_result(
            &args.command,
            &ctx.config.terminal_backend,
            timeout,
            &exec_output,
        )?;

        // Format output (includes stdout/stderr/exit-code)
        let max_stdout = ctx.config.max_terminal_output;
        let max_stderr = ctx.config.max_terminal_output / 4;
        let mut result = exec_output.format(max_stdout, max_stderr);

        // Strip ANSI escape codes for clean LLM consumption.
        result = strip_ansi_escapes::strip_str(&result);

        // Redact secrets and credentials before the output reaches the LLM.
        // This keeps terminal output handling aligned with the shared redaction policy.
        result = redact_output(&result);

        let header =
            terminal_result_header(&ctx.config.terminal_backend, &cwd, exec_output.exit_code);
        result = if result.is_empty() {
            header
        } else {
            format!("{header}\n{result}")
        };

        Ok(result)
    }
}

/// Returns true if the command is likely to mutate files or history.
/// These commands get an auto-checkpoint before execution.
fn is_destructive_command(cmd: &str) -> bool {
    const DESTRUCTIVE_PREFIXES: &[&str] = &[
        "rm ",
        "rm\t",
        "rmdir ",
        "mv ",
        "mv\t",
        "shred ",
        "truncate ",
        "git reset",
        "git clean",
        "git checkout",
        "git restore",
        "git rebase",
        "git merge",
        "git cherry-pick",
        "git revert",
        "sed -i",
        "sed -i'",
        "awk -i",
    ];
    let has_overwrite_redirect = cmd.contains(" > ")
        || cmd.starts_with('>')
        || cmd.contains("\t> ")
        || (cmd.contains('>') && !cmd.contains(">>"));

    let cmd_lower = cmd.to_lowercase();
    DESTRUCTIVE_PREFIXES.iter().any(|p| {
        cmd_lower.starts_with(p)
            || cmd_lower.contains(&format!("; {p}"))
            || cmd_lower.contains(&format!("&& {p}"))
    }) || has_overwrite_redirect
}

fn destructive_checkpoint_label(command: &str) -> String {
    let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("before terminal: {}", crate::safe_truncate(&normalized, 80))
}

/// Returns `true` if the backend error is worth retrying automatically.
///
/// Retryable: backend temporarily unavailable (Docker pulling, SSH reconnect).
/// NOT retried: Timeout (same timeout → same result), PermissionDenied (security
/// policy), InvalidArgs (model must fix schema), ExecutionFailed (legitimate
/// non-zero exit — retrying won't change the result).
fn is_backend_retryable(err: &ToolError) -> bool {
    matches!(err, ToolError::Unavailable { .. } | ToolError::Other(_))
}

inventory::submit!(&TerminalTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_table::ProcessTable;
    use crate::registry::{ApprovalRequest, ApprovalResponse};
    use crate::tools::backend_pool::{
        BackendCacheEntry, backend_cache, now_epoch_secs, prepare_backend_config,
    };
    use edgecrab_types::ToolError;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    static TERMINAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct FakeBackend {
        kind: crate::tools::backends::BackendKind,
        cleanup_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ExecutionBackend for FakeBackend {
        async fn execute(
            &self,
            _command: &str,
            _cwd: &str,
            _timeout: Duration,
            _cancel: CancellationToken,
        ) -> Result<crate::tools::backends::ExecOutput, ToolError> {
            Ok(crate::tools::backends::ExecOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            })
        }

        async fn cleanup(&self) -> Result<(), ToolError> {
            self.cleanup_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn kind(&self) -> crate::tools::backends::BackendKind {
            self.kind.clone()
        }
    }

    fn ctx_in(dir: &std::path::Path) -> ToolContext {
        let mut ctx = ToolContext::test_context();
        ctx.task_id = format!("terminal-test-{}", uuid::Uuid::new_v4().simple());
        ctx.cwd = dir.to_path_buf();
        ctx
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn terminal_echo() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = TerminalTool
            .execute(json!({"command": "echo hello world"}), &ctx)
            .await
            .expect("terminal");

        assert!(result.contains("hello world"));
        let _ = cleanup_backend_for_task(&ctx.task_id).await;
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn terminal_exit_code() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = TerminalTool
            .execute(json!({"command": "exit 42"}), &ctx)
            .await
            .expect("terminal");

        assert!(result.contains("exit code: 42"));
        let _ = cleanup_backend_for_task(&ctx.task_id).await;
    }

    #[test]
    fn destructive_checkpoint_label_preserves_utf8_boundaries() {
        let label = destructive_checkpoint_label("rm -rf 你好你好你好你好你好你好你好你好你好");
        assert!(label.starts_with("before terminal: "));
        assert!(std::str::from_utf8(label.as_bytes()).is_ok());
        assert!(!label.ends_with('\u{FFFD}'));
    }

    #[test]
    fn destructive_checkpoint_label_normalizes_whitespace() {
        let label = destructive_checkpoint_label("rm   -rf\t./tmp\nand-more");
        assert_eq!(label, "before terminal: rm -rf ./tmp and-more");
    }

    #[cfg_attr(
        windows,
        ignore = "PTY and POSIX shell syntax not available on Windows"
    )]
    #[tokio::test]
    async fn terminal_pty_reports_tty_to_child() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = TerminalTool
            .execute(
                json!({"command": "[ -t 0 ] && [ -t 1 ] && printf tty-ok || printf no-tty", "pty": true}),
                &ctx,
            )
            .await;

        let output = result.expect("pty terminal");
        assert!(output.contains("tty-ok"), "got: {output}");
    }

    #[tokio::test]
    async fn terminal_background_delegates_to_shared_process_path() {
        let dir = TempDir::new().expect("tmpdir");
        let mut ctx = ctx_in(dir.path());
        ctx.process_table = Some(Arc::new(ProcessTable::new()));

        let result = TerminalTool
            .execute(json!({"command": "echo hello", "background": true}), &ctx)
            .await
            .expect("background process");

        assert!(result.contains("Process started"), "got: {result}");
        assert!(result.contains("id=proc-"), "got: {result}");
    }

    #[tokio::test]
    async fn terminal_pty_rejects_remote_backend() {
        let dir = TempDir::new().expect("tmpdir");
        let mut ctx = ctx_in(dir.path());
        ctx.config.terminal_backend = crate::tools::backends::BackendKind::Modal;

        let result = TerminalTool
            .execute(json!({"command": "printf ok", "pty": true}), &ctx)
            .await;

        let err = result.expect_err("remote PTY should fail");
        let ToolError::CapabilityDenied { code, message, .. } = err else {
            panic!("expected capability denied");
        };
        assert_eq!(code, "pty_backend_unsupported");
        assert!(message.contains("local terminal backend"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn terminal_cwd_respected() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("marker.txt"), "found").expect("write");

        let ctx = ctx_in(dir.path());
        let result = TerminalTool
            .execute(json!({"command": "cat marker.txt"}), &ctx)
            .await
            .expect("terminal");

        assert!(result.contains("found"));
        let _ = cleanup_backend_for_task(&ctx.task_id).await;
    }

    #[test]
    fn prepare_backend_config_mounts_workspace_for_docker() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("tmpdir");
        let mut ctx = ctx_in(dir.path());
        ctx.config.terminal_backend = crate::tools::backends::BackendKind::Docker;

        let cfg = prepare_backend_config(&ctx);
        let mount = cfg
            .docker
            .workspace_mount
            .as_ref()
            .expect("docker workspace mount");
        assert_eq!(mount.container_path, "/workspace");
        assert_eq!(
            std::path::Path::new(&mount.host_path),
            dir.path(),
            "task cwd should be the mounted host workspace"
        );
    }

    #[test]
    fn modal_backend_rejects_host_workspace_paths() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("tmpdir");
        let err = validate_backend_workdir_visibility(
            &crate::tools::backends::BackendKind::Modal,
            &dir.path().to_string_lossy(),
        )
        .expect_err("modal should reject host cwd");
        assert!(
            err.to_string()
                .contains("cannot access host workspace path")
        );
        assert!(
            err.to_string()
                .contains("File tools in this session are rooted at")
        );
    }

    #[test]
    fn terminal_result_header_is_machine_readable() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let header = terminal_result_header(
            &crate::tools::backends::BackendKind::Docker,
            "/workspace/demo",
            0,
        );
        assert_eq!(
            header,
            "[terminal_result status=success backend=docker cwd=/workspace/demo exit_code=0]"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn terminal_requests_approval_for_dangerous_command() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("tmpdir");
        let mut ctx = ctx_in(dir.path());
        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        ctx.approval_tx = Some(approval_tx);

        let approver = tokio::spawn(async move {
            let request = approval_rx.recv().await.expect("approval request");
            assert!(request.full_command.contains("rm -rf"));
            let _ = request.response_tx.send(ApprovalResponse::Once);
        });

        let result = TerminalTool
            .execute(json!({"command": "rm -rf /tmp/edgecrab-danger-test"}), &ctx)
            .await
            .expect("terminal");

        approver.await.expect("approver task");
        assert!(result.contains("exit code: 0"));
        let _ = cleanup_backend_for_task(&ctx.task_id).await;
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn terminal_rejects_tty_ui_commands() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let err = TerminalTool
            .execute(json!({"command": "vim Cargo.toml"}), &ctx)
            .await
            .expect_err("tty ui should be rejected");

        let ToolError::CapabilityDenied { message, code, .. } = err else {
            panic!("expected capability denied");
        };
        assert_eq!(code, "non_interactive_terminal_required");
        assert!(message.contains("interactive terminal UI"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn cleanup_inactive_backends_removes_idle_entries() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = cleanup_all_backends().await;
        let cleanup_calls = Arc::new(AtomicUsize::new(0));
        let backend: Arc<dyn ExecutionBackend> = Arc::new(FakeBackend {
            kind: crate::tools::backends::BackendKind::Local,
            cleanup_calls: cleanup_calls.clone(),
        });
        let entry = Arc::new(BackendCacheEntry::new(backend));
        entry
            .last_used_epoch_secs
            .store(now_epoch_secs().saturating_sub(600), Ordering::Relaxed);
        backend_cache().insert("idle-test".into(), entry);

        let cleaned = cleanup_inactive_backends(Duration::from_secs(300)).await;
        assert_eq!(cleaned, 1);
        assert_eq!(cleanup_calls.load(Ordering::Relaxed), 1);
        assert!(!backend_cache().contains_key("idle-test"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn cleanup_inactive_backends_preserves_in_use_entries() {
        let _guard = TERMINAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = cleanup_all_backends().await;
        let cleanup_calls = Arc::new(AtomicUsize::new(0));
        let backend: Arc<dyn ExecutionBackend> = Arc::new(FakeBackend {
            kind: crate::tools::backends::BackendKind::Local,
            cleanup_calls: cleanup_calls.clone(),
        });
        let held_clone = backend.clone();
        let entry = Arc::new(BackendCacheEntry::new(backend));
        entry
            .last_used_epoch_secs
            .store(now_epoch_secs().saturating_sub(600), Ordering::Relaxed);
        backend_cache().insert("busy-test".into(), entry);

        let cleaned = cleanup_inactive_backends(Duration::from_secs(300)).await;
        assert_eq!(cleaned, 0);
        assert_eq!(cleanup_calls.load(Ordering::Relaxed), 0);
        assert!(backend_cache().contains_key("busy-test"));

        drop(held_clone);
        let _ = cleanup_all_backends().await;
    }
}
