use std::process::Command as ProcessCommand;

use anyhow::Context;

use crate::cli_args::LogsCommand;
use crate::logging::{
    format_log_size, format_relative_time, list_log_files, logs_dir_for, read_last_lines,
    resolve_log_path,
};

pub fn run(command: LogsCommand) -> anyhow::Result<()> {
    let home = edgecrab_core::edgecrab_home();
    match command {
        LogsCommand::List => list_logs(&home),
        LogsCommand::Path { name } => {
            if let Some(name) = name {
                println!("{}", resolve_log_path(&home, Some(&name))?.display());
            } else {
                println!("{}", logs_dir_for(&home).display());
            }
            Ok(())
        }
        LogsCommand::Show { name, lines } => show_log(&home, name.as_deref(), lines),
        LogsCommand::Tail { name, lines } => tail_log(&home, name.as_deref(), lines),
    }
}

fn list_logs(home: &std::path::Path) -> anyhow::Result<()> {
    let logs = list_log_files(home)?;
    if logs.is_empty() {
        println!("No log files found in {}", logs_dir_for(home).display());
        return Ok(());
    }

    println!("  {:<24} {:>8}  {:<14}  Path", "Name", "Size", "Modified");
    println!("  {}", "-".repeat(80));
    for log in logs {
        let age = log
            .modified
            .map(format_relative_time)
            .unwrap_or_else(|| "unknown".into());
        println!(
            "  {:<24} {:>8}  {:<14}  {}",
            log.name,
            format_log_size(log.size_bytes),
            age,
            log.path.display()
        );
    }
    Ok(())
}

fn show_log(home: &std::path::Path, name: Option<&str>, lines: usize) -> anyhow::Result<()> {
    let path = resolve_log_path(home, name)?;
    println!("{}", read_last_lines(&path, lines)?);
    Ok(())
}

fn tail_log(home: &std::path::Path, name: Option<&str>, lines: usize) -> anyhow::Result<()> {
    let path = resolve_log_path(home, name)?;
    #[cfg(unix)]
    {
        let status = ProcessCommand::new("tail")
            .arg("-n")
            .arg(lines.to_string())
            .arg("-f")
            .arg(&path)
            .status()
            .with_context(|| format!("failed to launch tail for {}", path.display()))?;
        if !status.success() {
            anyhow::bail!("tail exited with status {status}");
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = lines;
        anyhow::bail!(
            "Live log following is only supported on Unix in this build. Use `edgecrab logs show` instead."
        )
    }
}
