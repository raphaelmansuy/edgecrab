use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use edgequake_llm::providers::vscode::token::TokenManager;

use crate::gateway_cmd;

#[derive(Debug, Clone)]
pub struct UninstallOptions {
    pub dry_run: bool,
    pub purge_data: bool,
    pub purge_auth_cache: bool,
    pub remove_binary: bool,
    pub yes: bool,
}

#[derive(Debug, Clone)]
enum Action {
    StopGateway(u32),
    RemoveFile(PathBuf),
    RemoveDir(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmationMode {
    Interactive,
    RequireYes,
}

pub async fn run(options: UninstallOptions) -> anyhow::Result<()> {
    let report = run_with_mode(options, ConfirmationMode::Interactive).await?;
    if !report.trim().is_empty() {
        println!("{report}");
    }
    Ok(())
}

pub async fn run_capture(options: UninstallOptions) -> anyhow::Result<String> {
    run_with_mode(options, ConfirmationMode::RequireYes).await
}

pub fn options_from_slash_args(args: &str) -> Result<UninstallOptions, String> {
    let parts = crate::mcp_support::parse_inline_command_tokens(args.trim())?;
    let mut options = UninstallOptions {
        dry_run: parts.is_empty(),
        purge_data: false,
        purge_auth_cache: false,
        remove_binary: false,
        yes: false,
    };

    for part in parts {
        match part.as_str() {
            "--dry-run" | "dry-run" | "plan" => options.dry_run = true,
            "--purge-data" => options.purge_data = true,
            "--purge-auth-cache" => options.purge_auth_cache = true,
            "--remove-binary" => options.remove_binary = true,
            "--yes" | "-y" => options.yes = true,
            "help" | "--help" => return Err(uninstall_usage().into()),
            _ => {
                return Err(format!(
                    "Unexpected uninstall argument: {part}\n{}",
                    uninstall_usage()
                ));
            }
        }
    }

    Ok(options)
}

async fn run_with_mode(
    options: UninstallOptions,
    confirmation_mode: ConfirmationMode,
) -> anyhow::Result<String> {
    let plan = build_plan(&options)?;
    let mut out = render_plan(&options, &plan)?;

    if options.dry_run {
        return Ok(out);
    }

    if !options.yes {
        match confirmation_mode {
            ConfirmationMode::Interactive => {
                if !confirm()? {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str("Cancelled.");
                    return Ok(out);
                }
            }
            ConfirmationMode::RequireYes => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(
                    "Refusing to execute uninstall from a non-interactive surface without `--yes`.\nRerun `/uninstall --dry-run ...` to preview or `/uninstall --yes ...` to execute.",
                );
                return Ok(out);
            }
        }
    }

    let execution = execute_capture(plan).await?;
    if !execution.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&execution);
    }
    Ok(out)
}

fn build_plan(options: &UninstallOptions) -> anyhow::Result<Vec<Action>> {
    let mut plan = Vec::new();
    if let Ok(status) = gateway_cmd::snapshot()
        && status.running
        && let Some(pid) = status.pid
    {
        plan.push(Action::StopGateway(pid));
    }

    for alias in discover_profile_wrappers()? {
        plan.push(Action::RemoveFile(alias));
    }

    if options.purge_data {
        let home = edgecrab_core::edgecrab_home();
        if home.exists() {
            plan.push(Action::RemoveDir(home));
        }
    }

    if options.purge_auth_cache
        && let Some(path) = copilot_cache_dir()
        && path.exists()
    {
        plan.push(Action::RemoveDir(path));
    }

    if options.remove_binary {
        let binary = std::env::current_exe().context("failed to resolve current executable")?;
        if binary.exists() {
            plan.push(Action::RemoveFile(binary));
        }
    }

    Ok(plan)
}

fn render_plan(options: &UninstallOptions, plan: &[Action]) -> anyhow::Result<String> {
    let mut out = String::new();
    writeln!(out, "EdgeCrab uninstall")?;
    writeln!(out, "dry-run:          {}", yes_no(options.dry_run))?;
    writeln!(out, "purge-data:       {}", yes_no(options.purge_data))?;
    writeln!(
        out,
        "purge-auth-cache: {}",
        yes_no(options.purge_auth_cache)
    )?;
    writeln!(out, "remove-binary:    {}", yes_no(options.remove_binary))?;

    if plan.is_empty() {
        writeln!(
            out,
            "No EdgeCrab-managed artifacts matched this uninstall plan."
        )?;
        return Ok(out.trim_end().to_string());
    }

    writeln!(out, "Planned actions:")?;
    for action in plan {
        match action {
            Action::StopGateway(pid) => writeln!(out, "  stop gateway pid {pid}")?,
            Action::RemoveFile(path) => writeln!(out, "  remove file {}", path.display())?,
            Action::RemoveDir(path) => writeln!(out, "  remove directory {}", path.display())?,
        }
    }
    Ok(out.trim_end().to_string())
}

async fn execute_capture(plan: Vec<Action>) -> anyhow::Result<String> {
    let mut out = String::new();
    for action in plan {
        match action {
            Action::StopGateway(_) => {
                let report = gateway_cmd::run_capture(
                    gateway_cmd::GatewayAction::Stop,
                    &crate::cli_args::CliArgs::parse_from(["edgecrab"]),
                )
                .await?;
                if !report.trim().is_empty() {
                    writeln!(out, "{report}")?;
                }
            }
            Action::RemoveFile(path) => {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
                writeln!(out, "Removed {}", path.display())?;
            }
            Action::RemoveDir(path) => {
                fs::remove_dir_all(&path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
                writeln!(out, "Removed {}", path.display())?;
            }
        }
    }
    Ok(out.trim_end().to_string())
}

fn confirm() -> anyhow::Result<bool> {
    let mut stdout = io::stdout();
    write!(stdout, "Type 'yes' to confirm: ")?;
    stdout.flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim() == "yes")
}

fn discover_profile_wrappers() -> anyhow::Result<Vec<PathBuf>> {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".local")
        .join("bin");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut wrappers = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if content.contains("edgecrab --profile") {
            wrappers.push(path);
        }
    }
    wrappers.sort();
    Ok(wrappers)
}

fn copilot_cache_dir() -> Option<PathBuf> {
    let _ = TokenManager::new().ok()?;
    dirs::config_dir().map(|base| base.join("edgequake").join("copilot"))
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn uninstall_usage() -> &'static str {
    "Usage: /uninstall [--dry-run] [--purge-data] [--purge-auth-cache] [--remove-binary] [--yes]\nNo args defaults to a dry-run plan."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_uninstall_defaults_to_dry_run() {
        let options = options_from_slash_args("").unwrap();
        assert!(options.dry_run);
        assert!(!options.yes);
    }

    #[test]
    fn slash_uninstall_parses_flags() {
        let options = options_from_slash_args("--purge-data --purge-auth-cache --yes").unwrap();
        assert!(!options.dry_run);
        assert!(options.purge_data);
        assert!(options.purge_auth_cache);
        assert!(options.yes);
    }
}
