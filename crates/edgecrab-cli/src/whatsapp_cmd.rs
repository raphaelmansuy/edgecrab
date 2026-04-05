use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::Context;

use crate::cli_args::CliArgs;

pub fn run(args: &CliArgs) -> anyhow::Result<()> {
    let config_path = resolve_config_path(args)?;
    let mut config = if config_path.exists() {
        edgecrab_core::AppConfig::load_from(&config_path)?
    } else {
        edgecrab_core::AppConfig::default()
    };

    let mut wa = config.gateway.whatsapp.clone();
    if wa.mode.trim().is_empty() {
        wa.mode = "self-chat".into();
    }

    println!();
    println!("EdgeCrab WhatsApp Setup");
    println!("=======================");

    // Check prerequisites
    let has_node = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let has_npm = std::process::Command::new("npm")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_node || !has_npm {
        println!();
        println!("⚠ Prerequisites missing:");
        if !has_node {
            println!("  ✗ Node.js not found — install from https://nodejs.org (v18+)");
        }
        if !has_npm {
            println!("  ✗ npm not found — usually bundled with Node.js");
        }
        anyhow::bail!("Node.js and npm are required for WhatsApp support");
    }

    let node_ver = std::process::Command::new("node")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    println!("Node.js: {}", node_ver.trim());

    println!();
    println!("How will you use WhatsApp with EdgeCrab?");
    println!("  1. Separate bot number");
    println!("  2. Personal number (self-chat)");
    let current_choice = if wa.mode == "bot" { 1 } else { 2 };
    let choice = prompt_line(&format!("Choose [1/2] (default {current_choice}): "))?;
    wa.mode = match choice.trim() {
        "1" => "bot".into(),
        "2" => "self-chat".into(),
        _ => wa.mode,
    };

    let current_users = if wa.allowed_users.is_empty() {
        "(none)".to_string()
    } else {
        wa.allowed_users.join(",")
    };
    println!();
    println!("Allowed users: {current_users}");
    let prompt = if wa.mode == "bot" {
        "Allowed phone numbers (comma-separated, or * for anyone; blank keeps current): "
    } else {
        "Your phone number (blank keeps current): "
    };
    let allowed_users = prompt_line(prompt)?;
    if !allowed_users.trim().is_empty() {
        wa.allowed_users = allowed_users
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|s| s.trim_start_matches('+').to_string())
            .collect();
    }

    wa.enabled = true;
    config.gateway.whatsapp = wa.clone();
    config.gateway.enable_platform("whatsapp");
    if wa.session_path.is_none() {
        config.gateway.whatsapp.session_path =
            Some(edgecrab_gateway::whatsapp::WhatsAppAdapter::default_session_path());
    }
    config
        .save_to(&config_path)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    let adapter_cfg =
        edgecrab_gateway::whatsapp::WhatsappAdapterConfig::from(&config.gateway.whatsapp);
    let assets = edgecrab_gateway::whatsapp::WhatsAppAdapter::resolve_bridge_assets(&adapter_cfg)?;
    println!();
    println!("Bridge: {}", assets.bridge_script.display());
    println!("Session: {}", adapter_cfg.session_path.display());

    // Pre-install dependencies
    if !assets.bridge_dir.join("node_modules").exists() {
        println!();
        println!("Installing bridge dependencies...");
        let status = std::process::Command::new("npm")
            .args(["install", "--ignore-scripts"])
            .current_dir(&assets.bridge_dir)
            .status()
            .context("failed to run npm install")?;
        if !status.success() {
            anyhow::bail!("npm install failed in {}", assets.bridge_dir.display());
        }
        // Install platform-specific sharp prebuilt
        let postinstall = assets.bridge_dir.join("install-sharp-prebuilt.js");
        if postinstall.exists() {
            let _ = std::process::Command::new("node")
                .arg(&postinstall)
                .current_dir(&assets.bridge_dir)
                .status();
        }
        println!("✓ Dependencies installed");
    }

    let creds_path = adapter_cfg.session_path.join("creds.json");
    if creds_path.exists() {
        let answer = prompt_line("Existing session found. Re-pair? [y/N]: ")?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!();
            println!("WhatsApp is configured.");
            println!("Start the gateway with: edgecrab gateway start");
            return Ok(());
        }
        std::fs::remove_dir_all(&adapter_cfg.session_path).with_context(|| {
            format!(
                "failed to clear existing WhatsApp session at {}",
                adapter_cfg.session_path.display()
            )
        })?;
    }

    println!();
    println!("Open WhatsApp on the target device and scan the QR code when it appears.");
    edgecrab_gateway::whatsapp::WhatsAppAdapter::pair(&adapter_cfg)?;

    println!();
    println!("WhatsApp paired successfully.");
    println!("Start the gateway with: edgecrab gateway start");
    println!("Bridge log: {}", adapter_cfg.log_path.display());
    Ok(())
}

fn resolve_config_path(args: &CliArgs) -> anyhow::Result<PathBuf> {
    match &args.config {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(edgecrab_core::config::ensure_edgecrab_home()?.join("config.yaml")),
    }
}

fn prompt_line(prompt: &str) -> anyhow::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf)
}
