use std::path::Path;

use anyhow::{Context, anyhow};
use chrono::{DateTime, Local, Utc};
use edgecrab_core::AppConfig;
use edgecrab_tools::tools::honcho::{
    UserModelEntry, honcho_append_entry, honcho_remove_entry, honcho_store_path,
    honcho_valid_categories, load_store,
};

use crate::cli_args::HonchoCommand;

pub fn run(command: HonchoCommand) -> anyhow::Result<()> {
    match command {
        HonchoCommand::Status => show_status(),
        HonchoCommand::Setup {
            cloud_sync,
            disable,
        } => run_setup(cloud_sync, disable),
        HonchoCommand::Mode { mode } => run_mode(mode.as_deref()),
        HonchoCommand::Tokens {
            context,
            write_frequency,
        } => run_tokens(context, write_frequency),
        HonchoCommand::List => list_entries(None),
        HonchoCommand::Search { query } => list_entries(Some(query.as_str())),
        HonchoCommand::Add { category, content } => add_entry(&category, &content.join(" ")),
        HonchoCommand::Remove { id } => remove_entry(&id),
        HonchoCommand::Identity { file } => run_identity(file.as_deref()),
        HonchoCommand::Path => {
            println!("{}", honcho_store_path()?.display());
            Ok(())
        }
    }
}

fn show_status() -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let store = load_store().map_err(tool_error)?;
    let path = honcho_store_path().map_err(tool_error)?;
    let api_key_present = std::env::var(&config.honcho.api_key_env)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    println!("Honcho status");
    println!("Enabled:          {}", yes_no(config.honcho.enabled));
    println!("Mode:             {}", honcho_mode_label(&config));
    println!("Cloud sync:       {}", yes_no(config.honcho.cloud_sync));
    println!("API key env:      {}", config.honcho.api_key_env);
    println!("API key present:  {}", yes_no(api_key_present));
    println!("API URL:          {}", config.honcho.api_url);
    println!("Context entries:  {}", config.honcho.max_context_entries);
    println!("Write frequency:  {}", config.honcho.write_frequency);
    println!("Store path:       {}", path.display());
    println!("Stored entries:   {}", store.entries.len());
    Ok(())
}

fn run_setup(cloud_sync: bool, disable: bool) -> anyhow::Result<()> {
    let mut config = AppConfig::load()?;
    if disable {
        config.honcho.enabled = false;
        config.honcho.cloud_sync = false;
        config.save()?;
        println!("Honcho disabled in ~/.edgecrab/config.yaml.");
        return Ok(());
    }

    config.honcho.enabled = true;
    if cloud_sync {
        config.honcho.cloud_sync = true;
    }
    config.save()?;

    println!("Honcho enabled.");
    println!("Mode: {}", honcho_mode_label(&config));
    if config.honcho.cloud_sync {
        println!(
            "Cloud sync expects {} in the environment.",
            config.honcho.api_key_env
        );
    }
    Ok(())
}

fn run_mode(mode: Option<&str>) -> anyhow::Result<()> {
    let mut config = AppConfig::load()?;
    match mode.map(str::trim).filter(|value| !value.is_empty()) {
        None => {
            println!("{}", honcho_mode_label(&config));
            Ok(())
        }
        Some("disabled") | Some("off") => {
            config.honcho.enabled = false;
            config.honcho.cloud_sync = false;
            config.save()?;
            println!("disabled");
            Ok(())
        }
        Some("local") => {
            config.honcho.enabled = true;
            config.honcho.cloud_sync = false;
            config.save()?;
            println!("local");
            Ok(())
        }
        Some("hybrid") | Some("honcho") => {
            config.honcho.enabled = true;
            config.honcho.cloud_sync = true;
            config.save()?;
            println!("hybrid");
            Ok(())
        }
        Some(other) => Err(anyhow!(
            "unknown honcho mode '{other}' (expected: disabled, local, hybrid, honcho)"
        )),
    }
}

fn run_tokens(context: Option<usize>, write_frequency: Option<u32>) -> anyhow::Result<()> {
    let mut config = AppConfig::load()?;
    if let Some(context) = context {
        config.honcho.max_context_entries = context;
    }
    if let Some(write_frequency) = write_frequency {
        config.honcho.write_frequency = write_frequency;
    }
    if context.is_some() || write_frequency.is_some() {
        config.save()?;
    }

    println!("Honcho token budget");
    println!("Context entries: {}", config.honcho.max_context_entries);
    println!("Write frequency: {}", config.honcho.write_frequency);
    Ok(())
}

fn list_entries(query: Option<&str>) -> anyhow::Result<()> {
    let mut entries = load_store().map_err(tool_error)?.entries;
    if let Some(query) = query {
        let needle = query.to_ascii_lowercase();
        entries.retain(|entry| {
            entry.category.to_ascii_lowercase().contains(&needle)
                || entry.content.to_ascii_lowercase().contains(&needle)
        });
    }

    if entries.is_empty() {
        println!("No Honcho entries found.");
        return Ok(());
    }

    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    for entry in entries {
        print_entry(&entry);
    }
    Ok(())
}

fn add_entry(category: &str, content: &str) -> anyhow::Result<()> {
    let entry = honcho_append_entry(category, content).map_err(tool_error)?;
    println!("Saved Honcho entry {} [{}]", entry.id, entry.category);
    println!("{}", entry.content);
    Ok(())
}

fn remove_entry(id: &str) -> anyhow::Result<()> {
    match honcho_remove_entry(id).map_err(tool_error)? {
        Some(entry) => {
            println!("Removed Honcho entry {} [{}]", entry.id, entry.category);
            Ok(())
        }
        None => Err(anyhow!("no Honcho entry matches '{id}'")),
    }
}

fn run_identity(file: Option<&str>) -> anyhow::Result<()> {
    match file {
        None => {
            println!("Honcho identity seeding stores context entries in the local Honcho model.");
            println!("Valid categories: {}", honcho_valid_categories().join(", "));
            println!("Use: edgecrab honcho identity <path-to-SOUL.md>");
            Ok(())
        }
        Some(path) => {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read identity file {}", path))?;
            let trimmed = content.trim();
            if trimmed.is_empty() {
                return Err(anyhow!("identity file is empty"));
            }
            let seeded = if trimmed.chars().count() > 500 {
                let prefix = trimmed.chars().take(500).collect::<String>();
                format!("Identity seed from {}: {}", display_name(path), prefix)
            } else {
                format!("Identity seed from {}: {}", display_name(path), trimmed)
            };
            let entry = honcho_append_entry("context", &seeded).map_err(tool_error)?;
            println!("Seeded Honcho identity entry {} from {}", entry.id, path);
            Ok(())
        }
    }
}

fn print_entry(entry: &UserModelEntry) {
    println!(
        "{}  [{}]  uses={}  updated={}",
        entry.id,
        entry.category,
        entry.use_count,
        format_timestamp(entry.updated_at)
    );
    println!("  {}", entry.content);
}

fn format_timestamp(ts: i64) -> String {
    DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| ts.to_string())
}

fn honcho_mode_label(config: &AppConfig) -> &'static str {
    if !config.honcho.enabled {
        "disabled"
    } else if config.honcho.cloud_sync {
        "hybrid"
    } else {
        "local"
    }
}

fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn tool_error(error: edgecrab_types::ToolError) -> anyhow::Error {
    anyhow!(error.to_string())
}
