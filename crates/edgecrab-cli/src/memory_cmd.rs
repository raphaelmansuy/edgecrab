use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use anyhow::{Context, anyhow};

use crate::cli_args::MemoryCommand;

pub fn run(command: MemoryCommand) -> anyhow::Result<()> {
    match command {
        MemoryCommand::Show { target } => show_memory(target.as_deref()),
        MemoryCommand::Edit { target } => edit_memory(target.as_deref()),
        MemoryCommand::Path { target } => show_paths(target.as_deref()),
    }
}

fn memories_dir() -> PathBuf {
    edgecrab_core::edgecrab_home().join("memories")
}

fn resolve_targets(target: Option<&str>) -> anyhow::Result<Vec<(&'static str, PathBuf)>> {
    let dir = memories_dir();
    match target.unwrap_or("all").trim().to_ascii_lowercase().as_str() {
        "all" | "" => Ok(vec![
            ("MEMORY", dir.join("MEMORY.md")),
            ("USER", dir.join("USER.md")),
        ]),
        "memory" => Ok(vec![("MEMORY", dir.join("MEMORY.md"))]),
        "user" => Ok(vec![("USER", dir.join("USER.md"))]),
        other => Err(anyhow!(
            "Unknown memory target '{other}'. Use: memory, user, all"
        )),
    }
}

fn show_memory(target: Option<&str>) -> anyhow::Result<()> {
    let targets = resolve_targets(target)?;
    for (index, (label, path)) in targets.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("{}  {}", label, path.display());
        println!();
        match std::fs::read_to_string(path) {
            Ok(content) if !content.trim().is_empty() => print!("{content}"),
            Ok(_) => println!("(empty)"),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => println!("(missing)"),
            Err(err) => {
                return Err(err).with_context(|| format!("failed to read {}", path.display()));
            }
        }
    }
    Ok(())
}

fn edit_memory(target: Option<&str>) -> anyhow::Result<()> {
    let target = target.unwrap_or("memory");
    let targets = resolve_targets(Some(target))?;
    if targets.len() != 1 {
        anyhow::bail!("`edgecrab memory edit` accepts one target: memory or user");
    }
    let (_, path) = &targets[0];
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if !path.exists() {
        std::fs::write(path, "").with_context(|| format!("failed to create {}", path.display()))?;
    }

    let mut editor = editor_command_from_env()?;
    editor.arg(path);
    let display = format_command_for_display(&editor);
    let status = editor
        .status()
        .with_context(|| format!("failed to launch editor: {display}"))?;
    if !status.success() {
        anyhow::bail!("editor exited with status: {status}");
    }
    Ok(())
}

fn show_paths(target: Option<&str>) -> anyhow::Result<()> {
    for (_, path) in resolve_targets(target)? {
        println!("{}", path.display());
    }
    Ok(())
}

fn editor_command_from_env() -> anyhow::Result<ProcessCommand> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_string());
    parse_editor_command(&editor)
}

fn parse_editor_command(editor: &str) -> anyhow::Result<ProcessCommand> {
    let parts = shell_words::split(editor)
        .map_err(|err| anyhow!("invalid $EDITOR/$VISUAL command '{}': {err}", editor))?;
    let Some(program) = parts.first() else {
        anyhow::bail!("$EDITOR/$VISUAL is empty");
    };
    let mut command = ProcessCommand::new(program);
    command.args(&parts[1..]);
    Ok(command)
}

fn format_command_for_display(command: &ProcessCommand) -> String {
    let mut parts = Vec::new();
    parts.push(command.get_program().to_string_lossy().to_string());
    parts.extend(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string()),
    );
    parts.join(" ")
}
