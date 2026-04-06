//! # process — Background process management
//!
//! WHY process management: Long-running commands (dev servers, watchers,
//! builds) must not block the agent loop. This tool lets the agent spawn
//! processes in the background and poll/kill them later.
//!
//! ```text
//!   run_process("cargo watch") ──→ ProcessTable.register() ──→ proc-1
//!                                         │
//!                                         ├── PID stored for SIGKILL
//!                                         └── tokio::spawn drains stdout/stderr
//!
//!   get_process_output("proc-1") ──→ ProcessTable.get_output_tail() ──→ buffered log
//!   list_processes               ──→ ProcessTable.list_all()         ──→ [...records]
//!   kill_process("proc-1")       ──→ ProcessTable.kill() + SIGKILL   ──→ Killed
//!   wait_for_process("proc-1")   ──→ poll until exited / timeout     ──→ exit code
//! ```
//!
//! Stdout/stderr are drained by a background tokio task into a ring
//! buffer inside the ProcessRecord, keeping memory bounded even for
//! chatty processes.
//!
//! Shell startup noise (job-control warnings from `sh -lic`) is filtered
//! from the first chunk, mirroring hermes-agent's `_clean_shell_noise()`.
//!
//! The ProcessTable lives on the Agent and is shared via `ToolContext.process_table`.
//!
//! ## Backend routing
//!
//! Local backends use a real host subprocess with live stdout/stderr pipes and
//! stdin injection. Non-local backends (Docker / SSH / Modal / Daytona /
//! Singularity) mirror hermes-agent's `spawn_via_env()` model: launch the
//! command under `nohup sh -lc ...`, redirect output to a sandbox log file,
//! then poll that log and exit-code file through the active execution backend.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio_util::sync::CancellationToken;

use edgecrab_types::{ToolError, ToolSchema};

use crate::process_table::ProcessTable;
use crate::registry::{ToolContext, ToolHandler};
use crate::tools::backend_pool::{get_or_create_backend, resolve_workdir};
use crate::tools::backends::{BackendKind, ExecutionBackend, shell_quote};

/// Shell startup warnings to strip from the first output line.
///
/// These appear when `sh -lic` is used and the shell is not attached to a
/// terminal.  Filtering them prevents confusing the agent.  Mirrors
/// hermes-agent's `ProcessRegistry._SHELL_NOISE_SUBSTRINGS`.
const SHELL_NOISE: &[&str] = &[
    "bash: cannot set terminal process group",
    "bash: no job control in this shell",
    "no job control in this shell",
    "cannot set terminal process group",
    "tcsetattr: Inappropriate ioctl for device",
];

/// Return true if `line` is a shell startup warning that should be dropped.
fn is_shell_noise(line: &str) -> bool {
    SHELL_NOISE.iter().any(|noise| line.contains(noise))
}

// ─── run_process ───────────────────────────────────────────────
//
// WHY "sh -c": Spawning via the shell gives the agent access to
// pipelines, redirects, and environment variable expansion — the
// same semantics as the `terminal` tool for one-shot commands.
// The security scanner in edgecrab-security runs before spawn.

pub struct RunProcessTool;

#[derive(Deserialize)]
struct RunArgs {
    /// Shell command to execute (passed to `sh -c`)
    command: String,
    /// Optional working directory (defaults to current directory)
    cwd: Option<String>,
}

#[async_trait]
impl ToolHandler for RunProcessTool {
    fn name(&self) -> &'static str {
        "run_process"
    }

    fn toolset(&self) -> &'static str {
        "terminal"
    }

    fn emoji(&self) -> &'static str {
        "🚀"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "run_process".into(),
            description: "Spawn a background process. Returns immediately with a process_id. \
                          Use list_processes to poll status and kill_process to stop it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to run in the background"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory (defaults to current dir)"
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
        let args: RunArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "run_process".into(),
            message: e.to_string(),
        })?;

        // Security: scan for dangerous patterns before spawning.
        // WHY: Background processes persist beyond a single tool call, so
        // we must prevent agents from launching persistent malicious processes.
        if let Some(reasons) = crate::approval_runtime::command_approval_reasons(ctx, &args.command)
        {
            crate::approval_runtime::request_command_approval(ctx, &args.command, reasons).await?;
        }

        crate::command_interaction::guard_run_process_command(
            &args.command,
            &ctx.config.terminal_backend,
        )?;

        let cwd = resolve_workdir(ctx, args.cwd.as_deref());

        let Some(ref table) = ctx.process_table else {
            return Err(ToolError::Unavailable {
                tool: "run_process".into(),
                reason: "Process table not available in this context.".into(),
            });
        };

        // Enforce the MAX_PROCESSES cap: evict oldest finished entries first.
        table.prune_if_full().await;

        // Use the session_key from context (gateway: "platform:chat_id", CLI: session_id).
        // Enables has_active_for_session() — mirrors hermes-agent ProcessSession.session_key.
        let session_key = ctx.session_key.clone().unwrap_or_default();

        let process_id = table.register(args.command.clone(), cwd.clone(), session_key);

        if ctx.config.terminal_backend == BackendKind::Local {
            spawn_local_process(&args.command, &cwd, table, process_id).await
        } else {
            let backend = get_or_create_backend(ctx).await?;
            spawn_remote_process(&args.command, &cwd, table, process_id, backend).await
        }
    }
}

async fn spawn_local_process(
    command: &str,
    cwd: &str,
    table: &Arc<ProcessTable>,
    process_id: String,
) -> Result<String, ToolError> {
    let cwd_path = std::path::Path::new(cwd);
    let mut cmd = TokioCommand::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(cwd_path)
        .env_clear()
        .envs(crate::tools::backends::local::safe_env())
        .env("PYTHONUNBUFFERED", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::piped());
    #[cfg(unix)]
    cmd.process_group(0);
    let child_result = cmd.spawn();

    let mut child = match child_result {
        Ok(c) => c,
        Err(e) => {
            table.mark_killed(&process_id).await;
            return Err(ToolError::ExecutionFailed {
                tool: "run_process".into(),
                message: format!("Failed to spawn process: {e}"),
            });
        }
    };

    if let Some(pid) = child.id() {
        table.set_pid(&process_id, pid).await;
    }

    if let Some(child_stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        table.set_stdin_tx(&process_id, stdin_tx).await;
        tokio::spawn(async move {
            let mut stdin = child_stdin;
            while let Some(data) = stdin_rx.recv().await {
                if stdin.write_all(data.as_bytes()).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });
    }

    let table_clone = Arc::clone(table);
    let pid_clone = process_id.clone();
    let stdout = child.stdout.take().map(BufReader::new);
    let stderr = child.stderr.take().map(BufReader::new);

    if let Some(stdout_reader) = stdout {
        let t = Arc::clone(&table_clone);
        let p = pid_clone.clone();
        tokio::spawn(async move {
            drain_reader(stdout_reader, &t, &p).await;
        });
    }

    if let Some(stderr_reader) = stderr {
        let t = Arc::clone(&table_clone);
        let p = pid_clone.clone();
        tokio::spawn(async move {
            drain_reader(stderr_reader, &t, &p).await;
        });
    }

    let table_exit = Arc::clone(table);
    let pid_exit = process_id.clone();
    tokio::spawn(async move {
        match child.wait().await {
            Ok(status) => {
                let code = status.code().unwrap_or(-1);
                table_exit.mark_exited(&pid_exit, code).await;
            }
            Err(_) => {
                table_exit.mark_killed(&pid_exit).await;
            }
        }
    });

    Ok(format!(
        "Process started: {} (id={}). Use list_processes to monitor.",
        command, process_id
    ))
}

async fn spawn_remote_process(
    command: &str,
    cwd: &str,
    table: &Arc<ProcessTable>,
    process_id: String,
    backend: Arc<dyn ExecutionBackend>,
) -> Result<String, ToolError> {
    let base = format!("/tmp/edgecrab-bg-{process_id}");
    let log_path = format!("{base}.log");
    let pid_path = format!("{base}.pid");
    let exit_path = format!("{base}.exit");
    let backend_kind = backend.kind();
    let supervisor = format!(
        "nohup sh -lc {command} > {log} 2>&1 < /dev/null & \
         _edgecrab_pid=$!; \
         printf '%s\\n' \"$_edgecrab_pid\" > {pid}; \
         wait \"$_edgecrab_pid\"; \
         _edgecrab_status=$?; \
         printf '%s\\n' \"$_edgecrab_status\" > {exit}",
        command = shell_quote(command),
        log = shell_quote(&log_path),
        pid = shell_quote(&pid_path),
        exit = shell_quote(&exit_path),
    );

    let launcher = format!(
        "rm -f {log} {pid} {exit}; \
         nohup sh -lc {supervisor} >/dev/null 2>&1 < /dev/null & \
         for _edgecrab_i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do \
           [ -s {pid} ] && break; \
           sleep 0.1; \
         done; \
         cat {pid} 2>/dev/null || true",
        supervisor = shell_quote(&supervisor),
        log = shell_quote(&log_path),
        pid = shell_quote(&pid_path),
        exit = shell_quote(&exit_path),
    );

    let launch = backend
        .execute(
            &launcher,
            cwd,
            Duration::from_secs(10),
            CancellationToken::new(),
        )
        .await?;

    let pid = launch
        .stdout
        .lines()
        .rev()
        .find_map(|line| line.trim().parse::<u32>().ok());

    let Some(pid) = pid else {
        table.mark_killed(&process_id).await;
        return Err(ToolError::ExecutionFailed {
            tool: "run_process".into(),
            message: format!(
                "Failed to start background process in {} backend: {}",
                backend.kind(),
                launch.format(2048, 512)
            ),
        });
    };

    table.set_pid(&process_id, pid).await;

    let (kill_tx, mut kill_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    table.set_remote_kill(&process_id, kill_tx);

    let table_poll = Arc::clone(table);
    let pid_poll = process_id.clone();
    let backend_poll = Arc::clone(&backend);
    let cwd_poll = cwd.to_string();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = kill_rx.recv() => {
                    let kill_cmd = format!(
                        "if [ -s {pid} ]; then \
                           _edgecrab_pid=$(cat {pid} 2>/dev/null); \
                           kill -KILL -- -\"$_edgecrab_pid\" 2>/dev/null || true; \
                           kill -KILL \"$_edgecrab_pid\" 2>/dev/null || true; \
                           printf '137\\n' > {exit} 2>/dev/null || true; \
                         fi",
                        pid = shell_quote(&pid_path),
                        exit = shell_quote(&exit_path),
                    );
                    let _ = backend_poll
                        .execute(&kill_cmd, &cwd_poll, Duration::from_secs(5), CancellationToken::new())
                        .await;
                    table_poll.mark_killed(&pid_poll).await;
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(2)) => {
                    if refresh_remote_output(&backend_poll, &cwd_poll, &table_poll, &pid_poll, &log_path).await.is_err() {
                        table_poll.mark_exited(&pid_poll, -1).await;
                        break;
                    }
                    match read_remote_exit_code(&backend_poll, &cwd_poll, &exit_path).await {
                        Ok(Some(code)) => {
                            let _ = refresh_remote_output(&backend_poll, &cwd_poll, &table_poll, &pid_poll, &log_path).await;
                            table_poll.mark_exited(&pid_poll, code).await;
                            break;
                        }
                        Ok(None) => {}
                        Err(_) => {
                            table_poll.mark_exited(&pid_poll, -1).await;
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(format!(
        "Process started: {} (id={}) via {} backend. Use list_processes to monitor.",
        command, process_id, backend_kind
    ))
}

async fn refresh_remote_output(
    backend: &Arc<dyn ExecutionBackend>,
    cwd: &str,
    table: &ProcessTable,
    process_id: &str,
    log_path: &str,
) -> Result<(), ToolError> {
    let read_cmd = format!("tail -n 500 {} 2>/dev/null || true", shell_quote(log_path));
    let output = backend
        .execute(
            &read_cmd,
            cwd,
            Duration::from_secs(5),
            CancellationToken::new(),
        )
        .await?;
    table
        .replace_output(process_id, normalize_output_lines(&output.stdout))
        .await;
    Ok(())
}

async fn read_remote_exit_code(
    backend: &Arc<dyn ExecutionBackend>,
    cwd: &str,
    exit_path: &str,
) -> Result<Option<i32>, ToolError> {
    let read_cmd = format!(
        "test -s {path} && cat {path} || true",
        path = shell_quote(exit_path)
    );
    let output = backend
        .execute(
            &read_cmd,
            cwd,
            Duration::from_secs(5),
            CancellationToken::new(),
        )
        .await?;
    Ok(output
        .stdout
        .lines()
        .rev()
        .find_map(|line| line.trim().parse::<i32>().ok()))
}

fn normalize_output_lines(output: &str) -> Vec<String> {
    let mut first_lines = true;
    let mut lines = Vec::new();
    for raw in output.lines() {
        let trimmed = raw.trim_end_matches('\r');
        if first_lines && is_shell_noise(trimmed) {
            continue;
        }
        if !trimmed.is_empty() {
            first_lines = false;
        }
        lines.push(trimmed.to_string());
    }
    lines
}

/// Drain a line-buffered async reader into the process ring buffer.
///
/// WHY line-buffered: Line boundaries give meaningful units for display.
/// Tool output (compiler warnings, log lines) is inherently line-structured.
///
/// Shell noise filtering: The first non-empty lines are checked against
/// `SHELL_NOISE` and silently dropped.  This mirrors hermes-agent's
/// `ProcessRegistry._clean_shell_noise()`.
async fn drain_reader(
    mut reader: BufReader<impl tokio::io::AsyncRead + Unpin>,
    table: &ProcessTable,
    process_id: &str,
) {
    let mut first_lines = true; // still scanning the startup noise prefix
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                // Suppress shell startup noise from the leading output.
                if first_lines && is_shell_noise(trimmed) {
                    continue;
                }
                if !trimmed.is_empty() {
                    first_lines = false;
                }
                table
                    .append_output(process_id, vec![trimmed.to_string()])
                    .await;
            }
            Err(_) => break,
        }
    }
}

inventory::submit!(&RunProcessTool as &dyn ToolHandler);

// ─── list_processes ────────────────────────────────────────────

pub struct ListProcessesTool;

#[async_trait]
impl ToolHandler for ListProcessesTool {
    fn name(&self) -> &'static str {
        "list_processes"
    }

    fn toolset(&self) -> &'static str {
        "terminal"
    }

    fn emoji(&self) -> &'static str {
        "📋"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list_processes".into(),
            description: "List background processes started in this session.".into(),
            parameters: json!({"type": "object", "properties": {}}),
            strict: None,
        }
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let Some(ref table) = ctx.process_table else {
            return Ok("No background processes running.".into());
        };

        let records = table.list_all().await;
        if records.is_empty() {
            return Ok("No background processes running.".into());
        }

        let mut lines = vec![format!("Background processes ({}):", records.len())];
        for rec in &records {
            let duration = rec
                .started_at
                .elapsed()
                .map(|d| format!("{}s ago", d.as_secs()))
                .unwrap_or_else(|_| "?".into());
            let pid_str = rec.pid.map(|p| format!(" pid={}", p)).unwrap_or_default();
            lines.push(format!(
                "  {} [{}]{} {} — started {} in {}",
                rec.process_id,
                rec.status.as_str(),
                pid_str,
                rec.command,
                duration,
                rec.cwd
            ));
        }
        Ok(lines.join("\n"))
    }
}

inventory::submit!(&ListProcessesTool as &dyn ToolHandler);

// ─── kill_process ──────────────────────────────────────────────

pub struct KillProcessTool;

#[derive(Deserialize)]
struct KillArgs {
    process_id: String,
}

#[async_trait]
impl ToolHandler for KillProcessTool {
    fn name(&self) -> &'static str {
        "kill_process"
    }

    fn toolset(&self) -> &'static str {
        "terminal"
    }

    fn emoji(&self) -> &'static str {
        "🛑"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "kill_process".into(),
            description: "Kill a background process by its ID.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "process_id": {
                        "type": "string",
                        "description": "ID of the background process to kill"
                    }
                },
                "required": ["process_id"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: KillArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "kill_process".into(),
            message: e.to_string(),
        })?;

        let Some(ref table) = ctx.process_table else {
            return Err(ToolError::NotFound(format!(
                "No process with ID '{}' found (process table unavailable).",
                args.process_id
            )));
        };

        let killed = table.kill(&args.process_id).await;
        if killed {
            Ok(format!("Process '{}' has been killed.", args.process_id))
        } else {
            Err(ToolError::NotFound(format!(
                "No process with ID '{}' found.",
                args.process_id
            )))
        }
    }
}

inventory::submit!(&KillProcessTool as &dyn ToolHandler);

// ─── get_process_output ────────────────────────────────────────
//
// Mirrors hermes-agent's `process(action="poll")`.
// Returns the last N lines of buffered output plus status/exit code.

pub struct GetProcessOutputTool;

#[derive(Deserialize)]
struct GetOutputArgs {
    process_id: String,
    /// Max number of output lines to return (default: 100).
    tail: Option<usize>,
    /// Line offset from the start of the buffer (default: 0 = last `tail` lines).
    /// When > 0, skip the first `offset` lines and return up to `tail` lines.
    /// Mirrors hermes-agent's `process(action="log", offset=K, limit=N)`.
    offset: Option<usize>,
}

#[async_trait]
impl ToolHandler for GetProcessOutputTool {
    fn name(&self) -> &'static str {
        "get_process_output"
    }

    fn toolset(&self) -> &'static str {
        "terminal"
    }

    fn emoji(&self) -> &'static str {
        "📄"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "get_process_output".into(),
            description: "Get buffered output (stdout+stderr) from a background process. \
                          Returns the last `tail` lines plus current status. \
                          Use `offset` > 0 to paginate from the start of the buffer. \
                          Call repeatedly to poll a long-running process."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "process_id": {
                        "type": "string",
                        "description": "ID of the background process (from run_process)"
                    },
                    "tail": {
                        "type": "integer",
                        "description": "Max lines to return (default: 100). Used as the limit when offset=0 (last N lines) or as page size when offset > 0."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Line offset from the start of the buffer. Default: 0 (last `tail` lines). Set to K to skip the first K lines."
                    }
                },
                "required": ["process_id"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: GetOutputArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "get_process_output".into(),
                message: e.to_string(),
            })?;

        let Some(ref table) = ctx.process_table else {
            return Err(ToolError::Unavailable {
                tool: "get_process_output".into(),
                reason: "Process table not available in this context.".into(),
            });
        };

        let tail = args.tail.unwrap_or(100).clamp(1, 500);
        let offset = args.offset.unwrap_or(0);
        match table.get_output_page(&args.process_id, offset, tail).await {
            Some((output, total_lines, status, exit_code)) => {
                let status_str = match (&status, exit_code) {
                    (crate::process_table::ProcessStatus::Exited, Some(code)) => {
                        format!("exited (code {})", code)
                    }
                    (crate::process_table::ProcessStatus::Killed, _) => "killed".into(),
                    _ => "running".into(),
                };
                let showing = output.lines().count();
                if output.is_empty() {
                    Ok(format!(
                        "[{}: {} — no output yet]",
                        args.process_id, status_str
                    ))
                } else {
                    let page_note = format!(" [{} of {} lines]", showing, total_lines);
                    Ok(format!(
                        "[{}: {}{}]\n{}",
                        args.process_id, status_str, page_note, output
                    ))
                }
            }
            None => Err(ToolError::NotFound(format!(
                "No process with ID '{}' found.",
                args.process_id
            ))),
        }
    }
}

inventory::submit!(&GetProcessOutputTool as &dyn ToolHandler);

// ─── wait_for_process ─────────────────────────────────────────
//
// Mirrors hermes-agent's `process(action="wait")`.
// Blocks (with async yield) until the process exits or timeout.

pub struct WaitForProcessTool;

#[derive(Deserialize)]
struct WaitArgs {
    process_id: String,
    /// Timeout in seconds (default: 60, max: 3600).
    timeout_secs: Option<u64>,
}

#[async_trait]
impl ToolHandler for WaitForProcessTool {
    fn name(&self) -> &'static str {
        "wait_for_process"
    }

    fn toolset(&self) -> &'static str {
        "terminal"
    }

    fn emoji(&self) -> &'static str {
        "⏳"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "wait_for_process".into(),
            description: "Wait for a background process to finish, then return its exit code \
                          and last 50 lines of output. Use timeout_secs to cap the wait."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "process_id": {
                        "type": "string",
                        "description": "ID of the background process (from run_process)"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Max seconds to wait (default: 60)"
                    }
                },
                "required": ["process_id"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: WaitArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "wait_for_process".into(),
            message: e.to_string(),
        })?;

        let Some(ref table) = ctx.process_table else {
            return Err(ToolError::Unavailable {
                tool: "wait_for_process".into(),
                reason: "Process table not available in this context.".into(),
            });
        };

        let timeout_secs = args.timeout_secs.unwrap_or(60).clamp(1, 3600);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        // Poll the table every 500ms until the process is no longer Running.
        //
        // WHY tokio::select! with cancel: mirrors hermes-agent's wait() which
        // checks _interrupt_event in its poll loop. Without this, a user Ctrl+C
        // during wait_for_process would not break out until the deadline.
        loop {
            match table.get_output_tail(&args.process_id, 50).await {
                None => {
                    return Err(ToolError::NotFound(format!(
                        "No process with ID '{}' found.",
                        args.process_id
                    )));
                }
                Some((output, status, exit_code)) => {
                    let is_done = status != crate::process_table::ProcessStatus::Running;
                    if is_done {
                        let status_str = match (&status, exit_code) {
                            (crate::process_table::ProcessStatus::Exited, Some(code)) => {
                                format!("exited (code {})", code)
                            }
                            (crate::process_table::ProcessStatus::Killed, _) => "killed".into(),
                            _ => "done".into(),
                        };
                        return Ok(format!("[{}: {}]\n{}", args.process_id, status_str, output));
                    }

                    if std::time::Instant::now() >= deadline {
                        return Ok(format!(
                            "[{}: still running after {}s — use get_process_output to poll]\n{}",
                            args.process_id, timeout_secs, output
                        ));
                    }

                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {},
                        _ = ctx.cancel.cancelled() => {
                            return Ok(format!(
                                "[{}: interrupted — still running — use get_process_output to poll]\n{}",
                                args.process_id, output
                            ));
                        }
                    }
                }
            }
        }
    }
}

inventory::submit!(&WaitForProcessTool as &dyn ToolHandler);

// ─── write_stdin ──────────────────────────────────────────────
//
// Mirrors hermes-agent's `process_registry.write_stdin(session_id, data)`.
// Sends raw bytes to a running process's stdin, enabling interactive control
// (e.g. answering prompts, sending keystrokes to a Python REPL or REPL-based CLI).
//
// WHY `newline` parameter: Most interactive prompts expect input terminated
// with '\n' (like pressing Enter).  Making it explicit avoids silent
// surprises and matches hermes-agent's `submit_stdin()` vs `write_stdin()`.

pub struct WriteStdinTool;

#[derive(Deserialize)]
struct WriteStdinArgs {
    process_id: String,
    /// Text to send to the process stdin.
    data: String,
    /// If true, append a newline (like pressing Enter). Default: true.
    newline: Option<bool>,
}

#[async_trait]
impl ToolHandler for WriteStdinTool {
    fn name(&self) -> &'static str {
        "write_stdin"
    }

    fn toolset(&self) -> &'static str {
        "terminal"
    }

    fn emoji(&self) -> &'static str {
        "⌨️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write_stdin".into(),
            description: "Send text to a running background process's stdin. \
                          Useful for interactive tools (REPLs, prompts). \
                          Set newline=true (default) to simulate pressing Enter."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "process_id": {
                        "type": "string",
                        "description": "ID of the running background process (from run_process)"
                    },
                    "data": {
                        "type": "string",
                        "description": "Text to write to stdin"
                    },
                    "newline": {
                        "type": "boolean",
                        "description": "Append a newline character (like pressing Enter). Default: true"
                    }
                },
                "required": ["process_id", "data"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: WriteStdinArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "write_stdin".into(),
                message: e.to_string(),
            })?;

        let Some(ref table) = ctx.process_table else {
            return Err(ToolError::Unavailable {
                tool: "write_stdin".into(),
                reason: "Process table not available in this context.".into(),
            });
        };

        let append_newline = args.newline.unwrap_or(true);
        let payload = if append_newline {
            format!("{}\n", args.data)
        } else {
            args.data.clone()
        };

        match table.get_stdin_tx(&args.process_id).await {
            None => Err(ToolError::NotFound(format!(
                "No process with ID '{}' found (or stdin is not available).",
                args.process_id
            ))),
            Some(tx) => {
                tx.send(payload).map_err(|_| ToolError::ExecutionFailed {
                    tool: "write_stdin".into(),
                    message: format!(
                        "Process '{}' stdin channel closed — process may have exited.",
                        args.process_id
                    ),
                })?;
                Ok(format!(
                    "Wrote {} bytes to stdin of process '{}'.",
                    args.data.len() + if append_newline { 1 } else { 0 },
                    args.process_id
                ))
            }
        }
    }
}

inventory::submit!(&WriteStdinTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_table::ProcessTable;
    use std::sync::Arc;

    fn ctx_with_table() -> (ToolContext, Arc<ProcessTable>) {
        let table = Arc::new(ProcessTable::new());
        let mut ctx = ToolContext::test_context();
        ctx.process_table = Some(table.clone());
        (ctx, table)
    }

    #[tokio::test]
    async fn list_processes_empty() {
        let ctx = ToolContext::test_context();
        let result = ListProcessesTool
            .execute(json!({}), &ctx)
            .await
            .expect("no error");
        assert!(result.contains("No background processes"));
    }

    #[tokio::test]
    async fn list_processes_shows_entries() {
        let (ctx, table) = ctx_with_table();
        table.register("cargo build", "/tmp", "");
        let result = ListProcessesTool
            .execute(json!({}), &ctx)
            .await
            .expect("no error");
        assert!(result.contains("proc-1"));
        assert!(result.contains("cargo build"));
    }

    #[tokio::test]
    async fn kill_process_success() {
        let (ctx, table) = ctx_with_table();
        table.register("long_running", "/tmp", "");
        let result = KillProcessTool
            .execute(json!({"process_id": "proc-1"}), &ctx)
            .await
            .expect("no error");
        assert!(result.contains("killed"));
    }

    #[tokio::test]
    async fn kill_process_not_found() {
        let (ctx, _table) = ctx_with_table();
        let result = KillProcessTool
            .execute(json!({"process_id": "proc-9999"}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_process_rejects_tty_ui_commands() {
        let (ctx, _table) = ctx_with_table();
        let err = RunProcessTool
            .execute(json!({"command": "top"}), &ctx)
            .await
            .expect_err("tty ui should be rejected");
        let ToolError::CapabilityDenied { message, code, .. } = err else {
            panic!("expected capability denied");
        };
        assert_eq!(code, "background_interactive_terminal_unsupported");
        assert!(message.contains("interactive terminal UI"));
    }

    #[tokio::test]
    async fn run_process_rejects_macos_prompt_commands_in_background() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let (ctx, _table) = ctx_with_table();
        let err = RunProcessTool
            .execute(json!({"command": "memo notes -s \"Title\""}), &ctx)
            .await
            .expect_err("macos automation should be rejected");
        let ToolError::CapabilityDenied { message, code, .. } = err else {
            panic!("expected capability denied");
        };
        assert_eq!(code, "background_macos_consent_unsupported");
        assert!(message.contains("macOS permission dialog"));
    }
}
