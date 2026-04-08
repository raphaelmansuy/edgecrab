use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use edgecrab_types::ToolError;

use crate::ProcessTable;
const DEFAULT_PTY_SIZE: PtySize = PtySize {
    rows: 24,
    cols: 80,
    pixel_width: 0,
    pixel_height: 0,
};

fn shell_program() -> &'static str {
    if cfg!(windows) { "cmd.exe" } else { "sh" }
}

fn apply_shell_command(
    builder: &mut CommandBuilder,
    shell_exe: &std::path::Path,
    command: &str,
    interactive_login: bool,
) {
    if cfg!(windows) {
        builder.arg("/C");
    } else {
        builder.arg(crate::tools::backends::local::shell_command_flag(
            shell_exe,
            interactive_login,
        ));
    }
    builder.arg(command);
}

fn build_local_pty_command(command: &str, cwd: &str) -> CommandBuilder {
    let shell_exe = if cfg!(windows) {
        std::path::PathBuf::from(shell_program())
    } else {
        crate::tools::backends::local::preferred_shell_executable()
    };
    let mut builder = CommandBuilder::new(&shell_exe);
    apply_shell_command(&mut builder, &shell_exe, command, true);
    builder.cwd(cwd);
    builder.env_clear();
    for (key, value) in crate::tools::backends::local::safe_env() {
        builder.env(&key, &value);
    }
    builder.env("PATH", crate::tools::backends::local::subprocess_path());
    builder.env("TERM", "xterm-256color");
    builder.env("PYTHONUNBUFFERED", "1");
    builder
}

fn map_pty_error(
    tool_name: &'static str,
    action: &str,
    error: impl std::fmt::Display,
) -> ToolError {
    ToolError::ExecutionFailed {
        tool: tool_name.into(),
        message: format!("Failed to {action} PTY command: {error}"),
    }
}

pub(crate) async fn execute_foreground(
    tool_name: &'static str,
    command: &str,
    cwd: &str,
    timeout: Duration,
    cancel: CancellationToken,
) -> Result<crate::tools::backends::ExecOutput, ToolError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(DEFAULT_PTY_SIZE)
        .map_err(|e| map_pty_error(tool_name, "open", e))?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| map_pty_error(tool_name, "clone PTY reader", e))?;
    let mut child = pair
        .slave
        .spawn_command(build_local_pty_command(command, cwd))
        .map_err(|e| map_pty_error(tool_name, "spawn", e))?;
    let mut killer = child.clone_killer();
    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if output_tx.send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });

    let deadline = Instant::now() + timeout;
    let mut combined = String::new();
    let exit_code = loop {
        while let Ok(chunk) = output_rx.try_recv() {
            combined.push_str(&String::from_utf8_lossy(&chunk));
        }

        if cancel.is_cancelled() {
            let _ = killer.kill();
            let _ = tokio::task::spawn_blocking(move || child.wait()).await;
            tokio::time::sleep(Duration::from_millis(25)).await;
            while let Ok(chunk) = output_rx.try_recv() {
                combined.push_str(&String::from_utf8_lossy(&chunk));
            }
            break 130;
        }

        if Instant::now() >= deadline {
            let _ = killer.kill();
            let _ = tokio::task::spawn_blocking(move || child.wait()).await;
            tokio::time::sleep(Duration::from_millis(25)).await;
            while let Ok(chunk) = output_rx.try_recv() {
                combined.push_str(&String::from_utf8_lossy(&chunk));
            }
            break 124;
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                tokio::time::sleep(Duration::from_millis(25)).await;
                while let Ok(chunk) = output_rx.try_recv() {
                    combined.push_str(&String::from_utf8_lossy(&chunk));
                }
                break status.exit_code() as i32;
            }
            Ok(None) => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(err) => return Err(map_pty_error(tool_name, "wait on", err)),
        }
    };

    Ok(crate::tools::backends::ExecOutput {
        stdout: combined,
        stderr: String::new(),
        exit_code,
    })
}

pub(crate) async fn spawn_background(
    tool_name: &'static str,
    command: &str,
    cwd: &str,
    table: &Arc<ProcessTable>,
    process_id: String,
) -> Result<String, ToolError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(DEFAULT_PTY_SIZE)
        .map_err(|e| map_pty_error(tool_name, "open", e))?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| map_pty_error(tool_name, "clone PTY reader", e))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| map_pty_error(tool_name, "open PTY writer", e))?;
    let mut child = pair
        .slave
        .spawn_command(build_local_pty_command(command, cwd))
        .map_err(|e| map_pty_error(tool_name, "spawn", e))?;

    if let Some(pid) = child.process_id() {
        table.set_pid(&process_id, pid).await;
    }

    let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    table.set_stdin_tx(&process_id, stdin_tx).await;
    std::thread::spawn(move || {
        while let Some(data) = stdin_rx.blocking_recv() {
            if writer.write_all(data.as_bytes()).is_err() {
                break;
            }
            if writer.flush().is_err() {
                break;
            }
        }
    });

    let table_reader = Arc::clone(table);
    let process_reader = process_id.clone();
    let handle = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buffer[..n]).into_owned();
                    let table_reader = Arc::clone(&table_reader);
                    let process_reader = process_reader.clone();
                    handle.block_on(async move {
                        table_reader
                            .append_output_chunk(&process_reader, &chunk)
                            .await;
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });

    let table_wait = Arc::clone(table);
    let process_wait = process_id.clone();
    let handle = tokio::runtime::Handle::current();
    std::thread::spawn(move || match child.wait() {
        Ok(status) => {
            let code = status.exit_code() as i32;
            let table_wait = Arc::clone(&table_wait);
            let process_wait = process_wait.clone();
            handle.block_on(async move {
                table_wait.mark_exited_if_running(&process_wait, code).await;
            });
        }
        Err(_) => {
            let table_wait = Arc::clone(&table_wait);
            let process_wait = process_wait.clone();
            handle.block_on(async move {
                table_wait.mark_killed_if_running(&process_wait).await;
            });
        }
    });

    Ok(format!(
        "Process started: {} (id={}). Use list_processes to monitor.",
        command, process_id
    ))
}
