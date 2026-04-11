use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, anyhow};

use crate::cli_args::LogsCommand;

pub fn run(command: LogsCommand) -> anyhow::Result<()> {
    match command {
        LogsCommand::List => list_logs(),
        LogsCommand::Path { name } => {
            if let Some(name) = name {
                println!("{}", resolve_log_path(Some(&name))?.display());
            } else {
                println!("{}", logs_dir().display());
            }
            Ok(())
        }
        LogsCommand::Show { name, lines } => show_log(name.as_deref(), lines),
        LogsCommand::Tail { name, lines } => tail_log(name.as_deref(), lines),
    }
}

fn logs_dir() -> PathBuf {
    edgecrab_core::edgecrab_home().join("logs")
}

fn list_log_paths() -> anyhow::Result<Vec<PathBuf>> {
    let dir = logs_dir();
    let mut paths = Vec::new();
    if !dir.exists() {
        return Ok(paths);
    }
    for entry in
        std::fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn resolve_log_path(name: Option<&str>) -> anyhow::Result<PathBuf> {
    let paths = list_log_paths()?;
    if paths.is_empty() {
        anyhow::bail!("No log files found in {}", logs_dir().display());
    }

    if let Some(name) = name {
        let normalized = name.trim_end_matches(".log");
        let mut matches = paths
            .into_iter()
            .filter(|path| {
                let stem = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default();
                let file = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default();
                stem == normalized || file == name || stem.starts_with(normalized)
            })
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        return match matches.len() {
            0 => Err(anyhow!("No log file matching '{name}'.")),
            1 => Ok(matches.remove(0)),
            _ => Err(anyhow!(
                "Ambiguous log name '{name}'. Matches: {}",
                matches
                    .iter()
                    .map(|path| path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        };
    }

    let gateway = logs_dir().join("gateway.log");
    if gateway.exists() {
        return Ok(gateway);
    }
    if paths.len() == 1 {
        return Ok(paths[0].clone());
    }
    Err(anyhow!(
        "Multiple log files found. Use `edgecrab logs list` or specify one by name."
    ))
}

fn list_logs() -> anyhow::Result<()> {
    let paths = list_log_paths()?;
    if paths.is_empty() {
        println!("No log files found in {}", logs_dir().display());
        return Ok(());
    }

    println!("Log files:");
    for path in paths {
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        println!(
            "  {:20} {:>8} bytes  {}",
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default(),
            size,
            path.display()
        );
    }
    Ok(())
}

fn show_log(name: Option<&str>, lines: usize) -> anyhow::Result<()> {
    let path = resolve_log_path(name)?;
    println!("{}", read_last_lines(&path, lines)?);
    Ok(())
}

fn tail_log(name: Option<&str>, lines: usize) -> anyhow::Result<()> {
    let path = resolve_log_path(name)?;
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

fn read_last_lines(path: &Path, lines: usize) -> anyhow::Result<String> {
    #[cfg(unix)]
    {
        let output = ProcessCommand::new("tail")
            .arg("-n")
            .arg(lines.to_string())
            .arg(path)
            .output();
        if let Ok(output) = output
            && output.status.success()
        {
            return String::from_utf8(output.stdout)
                .map_err(|err| anyhow!("failed to decode {}: {err}", path.display()));
        }
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let total = content.lines().count();
    let skip = total.saturating_sub(lines);
    Ok(content.lines().skip(skip).collect::<Vec<_>>().join("\n"))
}
