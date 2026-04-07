//! # gateway_setup — Interactive gateway configuration wizard
//!
//! Exceeds hermes-agent's gateway setup with:
//! - Per-platform guided configuration across all shipped channel adapters
//! - Secure .env file storage for secrets
//! - Shared completeness checks between setup and status
//! - Easy reconfiguration of individual platforms
//!
//! ```text
//! edgecrab gateway configure              ← full interactive wizard
//! edgecrab gateway configure telegram     ← configure Telegram only
//! edgecrab gateway configure discord      ← configure Discord only
//! edgecrab gateway configure slack        ← configure Slack only
//! edgecrab gateway configure signal       ← configure Signal only
//! edgecrab gateway configure whatsapp     ← configure WhatsApp only
//! edgecrab gateway configure sms          ← configure SMS only
//! edgecrab gateway configure feishu       ← configure Feishu only
//! edgecrab gateway configure wecom        ← configure WeCom only
//! ```

use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;

use crossterm::{
    cursor,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{self, ClearType},
};
use dialoguer::{Confirm, Input, Password, Select, theme::ColorfulTheme};
use edgecrab_core::config::ensure_edgecrab_home;

use crate::cli_args::CliArgs;
use crate::gateway_catalog::{
    FieldKind, GatewayPlatformDef, PlatformState, SetupKind, all_platforms,
    collect_platform_diagnostics, find_platform,
};

// ─── Public entry points ──────────────────────────────────────────────

/// Configure gateway platforms given an explicit config path.
///
/// Called from the setup wizard (setup.rs) so it can delegate to the full
/// gateway wizard without duplicating any logic (DRY). Saves the config
/// and prints a brief summary; skips the gateway-specific "next steps" so
/// the setup wizard can display its own completion screen.
pub fn configure_for_path(
    config_path: &std::path::Path,
    platform: Option<&str>,
) -> anyhow::Result<()> {
    let mut config = if config_path.exists() {
        edgecrab_core::AppConfig::load_from(config_path)?
    } else {
        edgecrab_core::AppConfig::default()
    };

    if let Some(p) = platform {
        let p_lower = p.to_ascii_lowercase();
        if find_platform(&p_lower).is_none() {
            let available: Vec<&str> = all_platforms().iter().map(|d| d.id).collect();
            anyhow::bail!("Unknown platform: {p}\nAvailable: {}", available.join(", "));
        }
        configure_single_platform(&p_lower, &mut config)?;
    } else {
        run_full_wizard(&mut config)?;
    }

    config
        .save_to(config_path)
        .map_err(|e| anyhow::anyhow!("failed to save config: {e}"))?;

    println!();
    println!(
        "  Gateway configuration saved to: {}",
        config_path.display()
    );
    println!();
    println!("  Platform status:");
    print_platform_status(&config);

    Ok(())
}

pub fn run(args: &CliArgs, platform: Option<&str>) -> anyhow::Result<()> {
    let config_path = resolve_config_path(args)?;
    let mut config = if config_path.exists() {
        edgecrab_core::AppConfig::load_from(&config_path)?
    } else {
        edgecrab_core::AppConfig::default()
    };

    if let Some(p) = platform {
        let p_lower = p.to_ascii_lowercase();
        if find_platform(&p_lower).is_none() {
            let available: Vec<&str> = all_platforms().iter().map(|d| d.id).collect();
            anyhow::bail!("Unknown platform: {p}\nAvailable: {}", available.join(", "));
        }
        println!();
        println!("╔══════════════════════════════════════════════════════╗");
        println!(
            "║     EdgeCrab Gateway — {} Configuration          ║",
            p_lower.to_ascii_uppercase()
        );
        println!("╚══════════════════════════════════════════════════════╝");
        println!();
        configure_single_platform(&p_lower, &mut config)?;
    } else {
        run_full_wizard(&mut config)?;
    }

    config
        .save_to(&config_path)
        .map_err(|e| anyhow::anyhow!("failed to save config: {e}"))?;

    println!();
    println!(
        "✅ Gateway configuration saved to: {}",
        config_path.display()
    );
    println!();
    println!("  Configuration summary:");
    print_platform_status(&config);
    println!();
    println!("  Next steps:");
    if crate::gateway_cmd::snapshot()
        .map(|s| s.running)
        .unwrap_or(false)
    {
        println!("    edgecrab gateway restart         ← apply updates immediately");
    } else {
        println!("    edgecrab gateway start           ← start the gateway");
    }
    println!("    edgecrab gateway status           ← check gateway status");
    println!("    edgecrab gateway configure <plat> ← reconfigure a platform");
    println!("    edgecrab doctor                   ← verify configuration");

    Ok(())
}

// ─── Full wizard ──────────────────────────────────────────────────────

fn run_full_wizard(config: &mut edgecrab_core::AppConfig) -> anyhow::Result<()> {
    print_banner("EdgeCrab Gateway", "Guided Configuration");
    println!(
        "Connect EdgeCrab to messaging platforms so you can chat with your AI agent from anywhere."
    );
    println!(
        "Use arrows to move, Enter to confirm, and Esc to cancel a menu. Pasted values are normalized automatically."
    );
    println!();
    print_platform_status(config);
    println!();

    let mode = prompt_select(
        "What do you want to configure?",
        &[
            "Recommended setup  — review bind address and select platforms",
            "Platforms only     — choose which platforms to review now",
            "Gateway bind only  — update host and port",
            "Review only        — keep current settings and exit",
        ],
        0,
    )?;

    match mode {
        0 => {
            configure_gateway_bind(config)?;
            configure_platforms_menu(config)?;
        }
        1 => configure_platforms_menu(config)?,
        2 => configure_gateway_bind(config)?,
        _ => {
            println!("  No changes requested. Current settings were left as-is.");
        }
    }

    Ok(())
}

fn configure_gateway_bind(config: &mut edgecrab_core::AppConfig) -> anyhow::Result<()> {
    println!();
    println!("  Network");
    println!("  ───────");
    println!(
        "  Current bind: {}:{}",
        config.gateway.host, config.gateway.port
    );

    if !prompt_yes_no("Update the gateway host and port?", false)? {
        println!("  ✓ Keeping existing bind address");
        return Ok(());
    }

    let host = prompt_nonempty_with_default("Gateway host", &config.gateway.host)?;
    let port = prompt_port_with_default("Gateway port", config.gateway.port)?;
    config.gateway.host = host;
    config.gateway.port = port;

    println!(
        "  ✓ Gateway will bind to {}:{}",
        config.gateway.host, config.gateway.port
    );
    Ok(())
}

fn configure_platforms_menu(config: &mut edgecrab_core::AppConfig) -> anyhow::Result<()> {
    println!();
    println!("  Platforms");
    println!("  ─────────");
    println!("  Press Enter to open the highlighted platform.");
    println!("  Choose Done when you are finished reviewing platforms.");

    loop {
        let diagnostics = collect_platform_diagnostics(config);
        let mut items: Vec<String> = diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "{:<10} {:<18} {}",
                    diagnostic.name,
                    diagnostic.state.label(),
                    diagnostic.description
                )
            })
            .collect();
        items.push("Done".to_string());

        let default_index = diagnostics
            .iter()
            .position(|diagnostic| diagnostic.state.include_by_default())
            .unwrap_or(0);
        let choice = prompt_select_strings("Platform to configure", &items, default_index)?;
        if choice == diagnostics.len() {
            println!("  Finished reviewing platforms.");
            return Ok(());
        }

        let diagnostic = &diagnostics[choice];
        println!();
        println!("  {}", diagnostic.name);
        println!("  {}", "─".repeat(diagnostic.name.len()));
        println!(
            "  Status: {} ({})",
            diagnostic.state.label(),
            diagnostic.detail
        );
        configure_single_platform(diagnostic.id, config)?;

        if !prompt_yes_no("  Review another platform?", true)? {
            println!("  Finished reviewing platforms.");
            return Ok(());
        }
    }
}

// ─── Per-platform configuration ───────────────────────────────────────

fn configure_single_platform(
    platform_id: &str,
    config: &mut edgecrab_core::AppConfig,
) -> anyhow::Result<()> {
    let def = find_platform(platform_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown platform: {platform_id}"))?;

    match def.setup_kind {
        SetupKind::Telegram => configure_telegram(config),
        SetupKind::Discord => configure_discord(config),
        SetupKind::Slack => configure_slack(config),
        SetupKind::Signal => configure_signal(config),
        SetupKind::WhatsApp => configure_whatsapp(config),
        SetupKind::GenericEnv => configure_generic_env_platform(def, config),
        SetupKind::Webhook => configure_webhook(config),
    }
}

fn configure_generic_env_platform(
    def: &GatewayPlatformDef,
    config: &mut edgecrab_core::AppConfig,
) -> anyhow::Result<()> {
    println!();
    println!("  ◆ {} Setup", def.name);
    println!("  {}", "─".repeat(def.name.len() + 7));
    for (index, step) in def.instructions.iter().enumerate() {
        println!("  {}. {}", index + 1, step);
    }
    if !def.instructions.is_empty() {
        println!();
    }

    if config.gateway.platform_enabled(def.id)
        && prompt_yes_no(
            &format!("  Disable {} and keep stored credentials?", def.name),
            false,
        )?
    {
        config.gateway.disable_platform(def.id);
        println!("  ✓ {} disabled", def.name);
        return Ok(());
    }

    for field in def.env_fields.iter().filter(|field| field.required) {
        prompt_and_store_env_field(field)?;
    }

    let optional_fields: Vec<_> = def
        .env_fields
        .iter()
        .filter(|field| !field.required)
        .collect();
    if !optional_fields.is_empty() && prompt_yes_no("  Review optional settings now?", false)? {
        for field in optional_fields {
            prompt_and_store_env_field(field)?;
        }
    }

    if prompt_yes_no(
        &format!("  Enable {} now?", def.name),
        !config.gateway.platform_disabled(def.id),
    )? {
        config.gateway.enable_platform(def.id);
    } else {
        config.gateway.disable_platform(def.id);
    }
    let diagnostic = crate::gateway_catalog::diagnose_platform(def, config);
    println!("  ✓ {} status: {}", def.name, diagnostic.detail);
    Ok(())
}

fn configure_webhook(config: &mut edgecrab_core::AppConfig) -> anyhow::Result<()> {
    println!();
    println!("  ◆ Webhook Setup");
    println!("  ───────────────");
    println!("  Webhook traffic uses the main gateway bind address.");
    println!(
        "  Current endpoint base: http://{}:{}",
        config.gateway.host, config.gateway.port
    );

    let enable = prompt_yes_no("  Enable webhook adapter?", config.gateway.webhook_enabled)?;
    config.gateway.webhook_enabled = enable;
    if enable {
        println!("  ✓ Webhook enabled");
    } else {
        println!("  ✓ Webhook disabled");
    }
    Ok(())
}

fn prompt_and_store_env_field(field: &crate::gateway_catalog::EnvField) -> anyhow::Result<()> {
    println!();
    println!("  {}", field.help);

    let current = std::env::var(field.key).ok();
    let input = match field.kind {
        FieldKind::Secret => {
            let prompt = if current.is_some() {
                format!(
                    "  {} [configured] (enter keeps current, '-' clears)",
                    field.prompt
                )
            } else {
                format!("  {} (enter keeps current, '-' clears)", field.prompt)
            };
            prompt_secret(&prompt)?
        }
        FieldKind::Port => {
            let prompt = format!("  {} (enter keeps current, '-' clears)", field.prompt);
            prompt_optional_port(&prompt, current.as_deref().or(field.default_value))?
        }
        FieldKind::Text => {
            let prompt = format!("  {} (enter keeps current, '-' clears)", field.prompt);
            prompt_optional_text(&prompt, current.as_deref().or(field.default_value))?
        }
    };
    let trimmed = input.trim();

    if trimmed == "-" {
        remove_env_key(field.key)?;
        println!("  ✓ {} cleared", field.key);
        return Ok(());
    }
    if trimmed.is_empty() {
        return Ok(());
    }

    save_env_key(field.key, trimmed)?;
    println!("  ✓ {} saved", field.key);
    Ok(())
}

fn prompt_optional_text(prompt: &str, default: Option<&str>) -> io::Result<String> {
    let theme = wizard_theme();
    let input = Input::<String>::with_theme(&theme).with_prompt(prompt);
    let input = if let Some(default) = default {
        input.default(default.to_string())
    } else {
        input
    };
    input
        .allow_empty(true)
        .interact_text()
        .map_err(dialoguer_to_io)
        .map(sanitize_terminal_input)
}

fn prompt_optional_port(prompt: &str, default: Option<&str>) -> io::Result<String> {
    let theme = wizard_theme();
    let input = Input::<String>::with_theme(&theme).with_prompt(prompt);
    let input = if let Some(default) = default {
        input.default(default.to_string())
    } else {
        input
    };
    input
        .allow_empty(true)
        .validate_with(|value: &String| -> Result<(), &str> {
            let normalized = sanitize_terminal_input(value.clone());
            let trimmed = normalized.trim();
            if trimmed.is_empty() || trimmed == "-" {
                return Ok(());
            }
            match trimmed.parse::<u16>() {
                Ok(0) => Err("Port must be between 1 and 65535"),
                Ok(_) => Ok(()),
                Err(_) => Err("Enter a valid TCP port"),
            }
        })
        .interact_text()
        .map_err(dialoguer_to_io)
        .map(sanitize_terminal_input)
}

fn configure_telegram(config: &mut edgecrab_core::AppConfig) -> anyhow::Result<()> {
    println!();
    println!("  ◆ Telegram Bot Setup");
    println!("  ─────────────────────");
    println!("  1. Open Telegram and message @BotFather");
    println!("  2. Send /newbot and follow the prompts");
    println!("  3. Copy the bot token");
    println!();

    let existing_token = std::env::var("TELEGRAM_BOT_TOKEN").ok();
    if let Some(ref _t) = existing_token {
        println!("  ✓ TELEGRAM_BOT_TOKEN is set in environment");
    }

    if config
        .gateway
        .platform_requested("telegram", config.gateway.telegram.enabled)
        && prompt_yes_no("  Disable Telegram and keep stored credentials?", false)?
    {
        config.gateway.telegram.enabled = false;
        config.gateway.disable_platform("telegram");
        println!("  ✓ Telegram disabled");
        return Ok(());
    }

    let need_token = existing_token.is_none() || prompt_yes_no("  Update bot token?", false)?;

    if need_token && existing_token.is_none() {
        let token = prompt_secret("  Telegram bot token (hidden, '-' clears): ")?;
        if token.trim() == "-" {
            remove_env_key("TELEGRAM_BOT_TOKEN")?;
        } else if !token.trim().is_empty() {
            save_env_key("TELEGRAM_BOT_TOKEN", token.trim())?;
            println!("  ✓ Token saved to .env");
        }
    } else if need_token {
        let token = prompt_secret("  New Telegram bot token (hidden, '-' clears): ")?;
        if token.trim() == "-" {
            remove_env_key("TELEGRAM_BOT_TOKEN")?;
            println!("  ✓ Token cleared from .env");
        } else if !token.trim().is_empty() {
            save_env_key("TELEGRAM_BOT_TOKEN", token.trim())?;
            println!("  ✓ Token updated in .env");
        }
    }

    // Allowed users
    println!();
    println!("  🔒 Security: Restrict who can use your bot");
    println!("    To find your Telegram user ID:");
    println!("    1. Message @userinfobot on Telegram");
    println!("    2. It will reply with your numeric ID (e.g., 123456789)");
    println!();

    let current_users = if config.gateway.telegram.allowed_users.is_empty() {
        "(none — open access)".to_string()
    } else {
        config.gateway.telegram.allowed_users.join(", ")
    };
    println!("  Current allowed users: {current_users}");

    let input =
        prompt_line("  Allowed user IDs (comma-separated, blank keeps current, '-' clears): ")?;
    if input.trim() == "-" {
        config.gateway.telegram.allowed_users.clear();
        remove_env_key("TELEGRAM_ALLOWED_USERS")?;
    } else if !input.trim().is_empty() {
        config.gateway.telegram.allowed_users = parse_csv_values(&input);
        // Also save to env for backward compatibility
        save_env_key(
            "TELEGRAM_ALLOWED_USERS",
            &config.gateway.telegram.allowed_users.join(","),
        )?;
    }

    // Home channel
    println!();
    println!("  📬 Home Channel: where EdgeCrab delivers cron results & notifications.");
    println!("    For DMs, use your Telegram user ID (same as above).");
    println!("    You can also set it later with /sethome in your Telegram chat.");

    let current_home = config
        .gateway
        .telegram
        .home_channel
        .as_deref()
        .unwrap_or("(not set)");
    println!("  Current home channel: {current_home}");

    // Auto-suggest first allowed user as home channel
    if config.gateway.telegram.home_channel.is_none() {
        if let Some(first_user) = config.gateway.telegram.allowed_users.first() {
            if prompt_yes_no(
                &format!("  Use your user ID ({first_user}) as home channel?"),
                true,
            )? {
                config.gateway.telegram.home_channel = Some(first_user.clone());
                save_env_key("TELEGRAM_HOME_CHANNEL", first_user)?;
                println!("  ✓ Home channel set");
            }
        }
    }

    let input = prompt_line("  Home channel ID (blank keeps current, '-' clears): ")?;
    if input.trim() == "-" {
        config.gateway.telegram.home_channel = None;
        remove_env_key("TELEGRAM_HOME_CHANNEL")?;
    } else if !input.trim().is_empty() {
        config.gateway.telegram.home_channel = Some(input.trim().to_string());
        save_env_key("TELEGRAM_HOME_CHANNEL", input.trim())?;
    }

    if prompt_yes_no("  Enable Telegram now?", true)? {
        config.gateway.telegram.enabled = true;
        config.gateway.enable_platform("telegram");
        println!("  ✓ Telegram enabled");
    } else {
        config.gateway.telegram.enabled = false;
        config.gateway.disable_platform("telegram");
        println!("  ✓ Telegram disabled");
    }

    Ok(())
}

fn configure_discord(config: &mut edgecrab_core::AppConfig) -> anyhow::Result<()> {
    println!();
    println!("  ◆ Discord Bot Setup");
    println!("  ────────────────────");
    println!("  1. Go to https://discord.com/developers/applications");
    println!("  2. Create a New Application → Bot → Copy token");
    println!("  3. Enable MESSAGE CONTENT intent under Privileged Gateway Intents");
    println!("  4. Use the OAuth2 URL Generator to invite the bot to your server");
    println!("     Required scopes: bot");
    println!("     Required permissions: Send Messages, Read Message History");
    println!();

    let existing_token = std::env::var("DISCORD_BOT_TOKEN").ok();
    if let Some(ref _t) = existing_token {
        println!("  ✓ DISCORD_BOT_TOKEN is set in environment");
    }

    if config
        .gateway
        .platform_requested("discord", config.gateway.discord.enabled)
        && prompt_yes_no("  Disable Discord and keep stored credentials?", false)?
    {
        config.gateway.discord.enabled = false;
        config.gateway.disable_platform("discord");
        println!("  ✓ Discord disabled");
        return Ok(());
    }

    let need_token = existing_token.is_none() || prompt_yes_no("  Update bot token?", false)?;

    if need_token {
        let token = prompt_secret("  Discord bot token (hidden, '-' clears): ")?;
        if token.trim() == "-" {
            remove_env_key("DISCORD_BOT_TOKEN")?;
            println!("  ✓ Token cleared from .env");
        } else if !token.trim().is_empty() {
            save_env_key("DISCORD_BOT_TOKEN", token.trim())?;
            println!("  ✓ Token saved to .env");
        }
    }

    // Allowed users
    println!();
    println!("  🔒 Security: Restrict who can use your bot");
    println!("    To find your Discord user ID:");
    println!("    1. Enable Developer Mode in Discord settings → Advanced");
    println!("    2. Right-click your name → Copy User ID");
    println!();

    let current_users = if config.gateway.discord.allowed_users.is_empty() {
        "(none — open access)".to_string()
    } else {
        config.gateway.discord.allowed_users.join(", ")
    };
    println!("  Current allowed users: {current_users}");

    let input =
        prompt_line("  Allowed user IDs (comma-separated, blank keeps current, '-' clears): ")?;
    if input.trim() == "-" {
        config.gateway.discord.allowed_users.clear();
        remove_env_key("DISCORD_ALLOWED_USERS")?;
    } else if !input.trim().is_empty() {
        let cleaned: Vec<String> = input.split(',').filter_map(normalize_discord_user_id).fold(
            Vec::new(),
            |mut acc, user| {
                if !acc.contains(&user) {
                    acc.push(user);
                }
                acc
            },
        );
        config.gateway.discord.allowed_users = cleaned.clone();
        save_env_key("DISCORD_ALLOWED_USERS", &cleaned.join(","))?;
    }

    // Home channel
    println!();
    println!("  📬 Home Channel: where EdgeCrab delivers cron results & notifications.");
    println!("    Right-click a channel → Copy Channel ID (Developer Mode required).");
    println!("    You can also set it later with /sethome in your Discord chat.");

    let current_home = config
        .gateway
        .discord
        .home_channel
        .as_deref()
        .unwrap_or("(not set)");
    println!("  Current home channel: {current_home}");

    let input = prompt_line("  Home channel ID (blank keeps current, '-' clears): ")?;
    if input.trim() == "-" {
        config.gateway.discord.home_channel = None;
        remove_env_key("DISCORD_HOME_CHANNEL")?;
    } else if !input.trim().is_empty() {
        config.gateway.discord.home_channel = Some(input.trim().to_string());
        save_env_key("DISCORD_HOME_CHANNEL", input.trim())?;
    }

    if prompt_yes_no("  Enable Discord now?", true)? {
        config.gateway.discord.enabled = true;
        config.gateway.enable_platform("discord");
        println!("  ✓ Discord enabled");
    } else {
        config.gateway.discord.enabled = false;
        config.gateway.disable_platform("discord");
        println!("  ✓ Discord disabled");
    }

    Ok(())
}

fn configure_slack(config: &mut edgecrab_core::AppConfig) -> anyhow::Result<()> {
    println!();
    println!("  ◆ Slack Bot Setup");
    println!("  ──────────────────");
    println!("  1. Go to https://api.slack.com/apps → Create New App (from scratch)");
    println!("  2. Enable Socket Mode: Settings → Socket Mode → Enable");
    println!("     • Create an App-Level Token with 'connections:write' scope");
    println!("  3. Add Bot Token Scopes: Features → OAuth & Permissions");
    println!("     Required: chat:write, app_mentions:read, channels:history,");
    println!("     channels:read, im:history, im:read, im:write, users:read");
    println!("  4. Subscribe to Events: Features → Event Subscriptions → Enable");
    println!("     Required events: message.im, message.channels, app_mention");
    println!("  5. Install to Workspace: Settings → Install App");
    println!("  6. Invite the bot to channels: /invite @YourBot");
    println!();

    let existing_bot = std::env::var("SLACK_BOT_TOKEN").ok();
    if let Some(ref _t) = existing_bot {
        println!("  ✓ SLACK_BOT_TOKEN is set in environment");
    }

    if config
        .gateway
        .platform_requested("slack", config.gateway.slack.enabled)
        && prompt_yes_no("  Disable Slack and keep stored credentials?", false)?
    {
        config.gateway.slack.enabled = false;
        config.gateway.disable_platform("slack");
        println!("  ✓ Slack disabled");
        return Ok(());
    }

    let need_bot_token = existing_bot.is_none() || prompt_yes_no("  Update bot token?", false)?;

    if need_bot_token {
        let token = prompt_secret("  Slack Bot Token (xoxb-..., '-' clears): ")?;
        if token.trim() == "-" {
            remove_env_key("SLACK_BOT_TOKEN")?;
            println!("  ✓ Bot token cleared from .env");
        } else if !token.trim().is_empty() {
            save_env_key("SLACK_BOT_TOKEN", token.trim())?;
            println!("  ✓ Bot token saved to .env");
        }
    }

    let existing_app = std::env::var("SLACK_APP_TOKEN").ok();
    if let Some(ref _t) = existing_app {
        println!("  ✓ SLACK_APP_TOKEN is set in environment");
    }

    let need_app_token = existing_app.is_none() || prompt_yes_no("  Update app token?", false)?;

    if need_app_token {
        let token = prompt_secret("  Slack App Token (xapp-..., '-' clears): ")?;
        if token.trim() == "-" {
            remove_env_key("SLACK_APP_TOKEN")?;
            println!("  ✓ App token cleared from .env");
        } else if !token.trim().is_empty() {
            save_env_key("SLACK_APP_TOKEN", token.trim())?;
            println!("  ✓ App token saved to .env");
        }
    }

    // Allowed users
    println!();
    println!("  🔒 Security: Restrict who can use your bot");
    println!("    To find a Member ID: click a user → View full profile → ⋮ → Copy member ID");
    println!();

    let current_users = if config.gateway.slack.allowed_users.is_empty() {
        "(none — unpaired users denied by default)".to_string()
    } else {
        config.gateway.slack.allowed_users.join(", ")
    };
    println!("  Current allowed users: {current_users}");

    let input =
        prompt_line("  Allowed user IDs (comma-separated, blank keeps current, '-' clears): ")?;
    if input.trim() == "-" {
        config.gateway.slack.allowed_users.clear();
        remove_env_key("SLACK_ALLOWED_USERS")?;
    } else if !input.trim().is_empty() {
        config.gateway.slack.allowed_users = parse_csv_values(&input);
        save_env_key(
            "SLACK_ALLOWED_USERS",
            &config.gateway.slack.allowed_users.join(","),
        )?;
    }

    // Home channel
    println!();
    println!("  📬 Home Channel: where EdgeCrab delivers cron results & notifications.");
    println!("    To get a channel ID: right-click channel name → View channel details → copy ID");
    println!("    You can also set it later with /sethome in a Slack channel.");

    let current_home = config
        .gateway
        .slack
        .home_channel
        .as_deref()
        .unwrap_or("(not set)");
    println!("  Current home channel: {current_home}");

    let input = prompt_line("  Home channel ID (blank keeps current, '-' clears): ")?;
    if input.trim() == "-" {
        config.gateway.slack.home_channel = None;
        remove_env_key("SLACK_HOME_CHANNEL")?;
    } else if !input.trim().is_empty() {
        config.gateway.slack.home_channel = Some(input.trim().to_string());
        save_env_key("SLACK_HOME_CHANNEL", input.trim())?;
    }

    if prompt_yes_no("  Enable Slack now?", true)? {
        config.gateway.slack.enabled = true;
        config.gateway.enable_platform("slack");
        println!("  ✓ Slack enabled");
    } else {
        config.gateway.slack.enabled = false;
        config.gateway.disable_platform("slack");
        println!("  ✓ Slack disabled");
    }

    Ok(())
}

fn configure_signal(config: &mut edgecrab_core::AppConfig) -> anyhow::Result<()> {
    println!();
    println!("  ◆ Signal Setup");
    println!("  ───────────────");
    println!("  Requires signal-cli running as an HTTP daemon:");
    println!("    signal-cli daemon --http 127.0.0.1:8090");
    println!();
    println!("  Install signal-cli: https://github.com/AsamK/signal-cli");
    println!();

    if config
        .gateway
        .platform_requested("signal", config.gateway.signal.enabled)
        && prompt_yes_no("  Disable Signal and keep stored credentials?", false)?
    {
        config.gateway.signal.enabled = false;
        config.gateway.disable_platform("signal");
        println!("  ✓ Signal disabled");
        return Ok(());
    }

    let current_backend = std::env::var("SIGNAL_BACKEND").unwrap_or_else(|_| "cli".to_string());
    println!("  Signal backend mode:");
    println!("    • cli           (local signal-cli binary)");
    println!("    • docker-native (managed container, no Java on host)");
    let backend = prompt_signal_backend(&current_backend)?;
    save_env_key("SIGNAL_BACKEND", backend.as_str())?;

    let current_url = config
        .gateway
        .signal
        .http_url
        .as_deref()
        .or_else(|| std::env::var("SIGNAL_HTTP_URL").ok().as_deref().map(|_| ""))
        .unwrap_or("");

    let _display_url = if current_url.is_empty() {
        "(not set)"
    } else {
        current_url
    };

    let env_url = std::env::var("SIGNAL_HTTP_URL").ok();
    let effective_url = config.gateway.signal.http_url.clone().or(env_url);

    if let Some(ref url) = effective_url {
        println!("  Current HTTP URL: {url}");
    }

    let default_http_url = "http://127.0.0.1:8090";

    let input = prompt_line(&format!(
        "  signal-cli HTTP URL [{}] (blank keeps current, '-' clears): ",
        effective_url.as_deref().unwrap_or(default_http_url)
    ))?;
    let url = if input.trim() == "-" {
        config.gateway.signal.http_url = None;
        remove_env_key("SIGNAL_HTTP_URL")?;
        String::new()
    } else if input.trim().is_empty() {
        effective_url.unwrap_or_else(|| default_http_url.into())
    } else {
        input.trim().to_string()
    };
    if !url.is_empty() {
        config.gateway.signal.http_url = Some(url.clone());
        save_env_key("SIGNAL_HTTP_URL", &url)?;
    }

    if matches!(backend, SignalBackend::DockerNative) {
        println!();
        println!("  Docker-native mode selected (no Java required on host).");
        if !check_docker_installed() {
            println!("  ⚠ Docker is not available.");
            println!("  Install Docker Desktop, then re-run this wizard.");
            println!("  For now, you can still continue in manual mode.");
        } else if prompt_yes_no(
            "  Start/update managed signal-cli native container now?",
            true,
        )? {
            ensure_signal_docker_native_running(&url)?;
        }
    }

    // Account phone number
    let current_account = config
        .gateway
        .signal
        .account
        .clone()
        .or_else(|| std::env::var("SIGNAL_ACCOUNT").ok());
    if let Some(ref acct) = current_account {
        println!("  Current account: {acct}");
    }

    let input = prompt_line("  Signal account phone number (blank keeps current, '-' clears): ")?;
    let account = if input.trim() == "-" {
        config.gateway.signal.account = None;
        remove_env_key("SIGNAL_ACCOUNT")?;
        String::new()
    } else if input.trim().is_empty() {
        current_account.unwrap_or_default()
    } else {
        input.trim().to_string()
    };
    if !account.is_empty() {
        config.gateway.signal.account = Some(account.clone());
        save_env_key("SIGNAL_ACCOUNT", &account)?;
    }

    // ── Step 3b: Detect missing registration and offer link flow ──────────
    if !account.is_empty() {
        if matches!(backend, SignalBackend::DockerNative) && url.is_empty() {
            println!("  ⚠ docker-native mode requires a Signal HTTP URL before linking.");
        }
        let account_state = match backend {
            SignalBackend::Cli => check_signal_account_registered(&account),
            SignalBackend::DockerNative if !url.is_empty() => {
                check_signal_account_registered_rpc(&url, &account)
            }
            SignalBackend::DockerNative => {
                SignalAccountState::Unknown("missing SIGNAL_HTTP_URL".to_string())
            }
        };
        match account_state {
            SignalAccountState::Registered => {
                println!("  ✓ signal-cli account is registered");
            }
            SignalAccountState::NotRegistered => {
                println!();
                println!("  ⚠ signal-cli account is not registered.");
                println!("  Link it to your existing Signal app as a secondary device:");
                println!();
                if prompt_yes_no("  Link signal-cli to your phone now (recommended)?", true)? {
                    match backend {
                        SignalBackend::Cli => {
                            if check_signal_cli_installed() {
                                run_signal_link_flow(&account)?;
                            } else {
                                println!("  ⚠ signal-cli is not available on PATH.");
                                println!("  Install it, then run:");
                                println!("    signal-cli link -n EdgeCrabAgent");
                            }
                        }
                        SignalBackend::DockerNative => {
                            run_signal_link_flow_rpc(&url)?;
                        }
                    }
                } else {
                    println!("  To link manually later:");
                    match backend {
                        SignalBackend::Cli => {
                            println!("    signal-cli link -n EdgeCrabAgent");
                        }
                        SignalBackend::DockerNative => {
                            println!("    Use: edgecrab gateway configure signal");
                            println!("    Then choose docker-native and link now.");
                        }
                    }
                    println!("  Scan the QR code with Signal → Settings → Linked devices.");
                }
            }
            SignalAccountState::Unknown(reason) => {
                println!("  ℹ Could not verify account state: {reason}");
                match backend {
                    SignalBackend::Cli => {
                        println!("  If Signal is not working, run this manually:");
                        println!("    signal-cli link -n EdgeCrabAgent");
                        if prompt_yes_no("  Try linking now anyway?", true)? {
                            run_signal_link_flow(&account)?;
                        }
                    }
                    SignalBackend::DockerNative => {
                        println!(
                            "  If Signal is not working, ensure docker-native daemon is running."
                        );
                        if check_docker_installed()
                            && prompt_yes_no("  Start/update docker-native daemon now?", true)?
                        {
                            ensure_signal_docker_native_running(&url)?;
                        }
                        if prompt_yes_no("  Try QR link now via docker-native JSON-RPC?", true)? {
                            run_signal_link_flow_rpc(&url)?;
                        }
                    }
                }
            }
        }
    }

    // Allowed users
    println!();
    println!("  🔒 Security: Restrict who can message your agent");
    println!("    Use phone numbers in international format (e.g., +1234567890)");
    println!();

    let current_users = if config.gateway.signal.allowed_users.is_empty() {
        "(none — open access)".to_string()
    } else {
        config.gateway.signal.allowed_users.join(", ")
    };
    println!("  Current allowed users: {current_users}");

    let input = prompt_line(
        "  Allowed phone numbers (comma-separated, blank keeps current, '-' clears): ",
    )?;
    if input.trim() == "-" {
        config.gateway.signal.allowed_users.clear();
    } else if !input.trim().is_empty() {
        config.gateway.signal.allowed_users = parse_csv_values(&input);
    }

    if prompt_yes_no(
        "  Enable Signal now?",
        !config
            .gateway
            .signal
            .http_url
            .as_deref()
            .unwrap_or("")
            .is_empty(),
    )? {
        config.gateway.signal.enabled = true;
        config.gateway.enable_platform("signal");
        println!("  ✓ Signal enabled");
    } else {
        config.gateway.signal.enabled = false;
        config.gateway.disable_platform("signal");
        println!("  ✓ Signal disabled");
    }

    Ok(())
}

/// Registration/link state for a signal-cli account.
enum SignalAccountState {
    Registered,
    NotRegistered,
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalBackend {
    Cli,
    DockerNative,
}

impl SignalBackend {
    fn from_input(input: &str, current: &str) -> Self {
        let chosen = if input.trim().is_empty() {
            current.trim()
        } else {
            input.trim()
        }
        .to_ascii_lowercase();
        match chosen.as_str() {
            "docker" | "docker-native" | "native" => SignalBackend::DockerNative,
            _ => SignalBackend::Cli,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            SignalBackend::Cli => "cli",
            SignalBackend::DockerNative => "docker-native",
        }
    }
}

/// Check whether signal-cli has `account` registered/linked locally.
fn check_signal_account_registered(account: &str) -> SignalAccountState {
    let result = signal_cli_command()
        .args(["-a", account, "listAccounts"])
        .output();
    match result {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = format!("{stdout}{stderr}").to_lowercase();
            if combined.contains("not registered")
                || combined.contains("is not registered")
                || combined.contains("no account")
            {
                SignalAccountState::NotRegistered
            } else if out.status.success() {
                SignalAccountState::Registered
            } else {
                SignalAccountState::NotRegistered
            }
        }
        Err(e) => SignalAccountState::Unknown(e.to_string()),
    }
}

fn check_signal_account_registered_rpc(http_url: &str, account: &str) -> SignalAccountState {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "listAccounts",
        "id": "edgecrab-list-accounts"
    });
    let res = signal_rpc_call(http_url, &payload, 15);
    match res {
        Ok(json) => {
            let combined = json.to_string();
            if combined.contains(account) {
                SignalAccountState::Registered
            } else {
                SignalAccountState::NotRegistered
            }
        }
        Err(e) => SignalAccountState::Unknown(e.to_string()),
    }
}

fn check_docker_installed() -> bool {
    std::process::Command::new("docker")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn ensure_signal_docker_native_running(http_url: &str) -> anyhow::Result<()> {
    const CONTAINER: &str = "edgecrab-signal-native";
    const IMAGE: &str = "registry.gitlab.com/packaging/signal-cli/signal-cli-native:latest";

    let port = extract_port_from_http_url(http_url).unwrap_or(8090);

    println!("  Pulling latest native image…");
    let _ = std::process::Command::new("docker")
        .args(["pull", IMAGE])
        .status();

    // Remove existing container (if any) to ensure predictable port mapping.
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", CONTAINER])
        .status();

    println!("  Starting managed container on port {port}…");
    let run_status = std::process::Command::new("docker")
        .args([
            "run",
            "-d",
            "--name",
            CONTAINER,
            "--restart",
            "unless-stopped",
            "--publish",
            &format!("{port}:3000"),
            "--volume",
            "edgecrab-signal-data:/var/lib/signal-cli",
            "--tmpfs",
            "/tmp:exec",
            IMAGE,
            "daemon",
            "--http",
            "0.0.0.0:3000",
            "--receive-mode=on-start",
        ])
        .status();

    match run_status {
        Ok(s) if s.success() => {}
        Ok(s) => anyhow::bail!(
            "Failed to start docker-native signal container (exit {:?})",
            s.code()
        ),
        Err(e) => anyhow::bail!("Failed to start docker-native signal container: {e}"),
    }

    // Wait for readiness.
    for _ in 0..30 {
        if signal_check_daemon(http_url) {
            println!("  ✓ Docker-native Signal daemon is ready at {http_url}");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    anyhow::bail!("Docker-native Signal daemon did not become ready at {http_url}")
}

fn extract_port_from_http_url(http_url: &str) -> Option<u16> {
    let trimmed = http_url.trim().trim_end_matches('/');
    let no_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    let host_port = no_scheme.split('/').next().unwrap_or(no_scheme);
    let port_str = host_port.rsplit(':').next()?;
    port_str.parse::<u16>().ok()
}

fn signal_check_daemon(http_url: &str) -> bool {
    let status = std::process::Command::new("curl")
        .args([
            "--silent",
            "--max-time",
            "3",
            "--output",
            "/dev/null",
            "--write-out",
            "%{http_code}",
            &format!("{}/api/v1/check", http_url.trim_end_matches('/')),
        ])
        .output();
    match status {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim() == "200",
        _ => false,
    }
}

fn signal_rpc_call(
    http_url: &str,
    payload: &serde_json::Value,
    timeout_secs: u64,
) -> anyhow::Result<serde_json::Value> {
    let endpoint = format!("{}/api/v1/rpc", http_url.trim_end_matches('/'));
    let body = payload.to_string();
    let out = std::process::Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            &timeout_secs.to_string(),
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            &body,
            &endpoint,
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("curl failed: {e}"))?;

    if !out.status.success() {
        anyhow::bail!("RPC call failed with exit {:?}", out.status.code());
    }

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if stdout.is_empty() {
        anyhow::bail!("RPC call returned empty response");
    }
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("Invalid JSON-RPC response: {e}. Raw: {stdout}"))?;

    if let Some(err) = json.get("error") {
        anyhow::bail!("JSON-RPC error: {err}");
    }

    Ok(json)
}

/// Check if signal-cli binary is available on PATH.
fn check_signal_cli_installed() -> bool {
    // On some systems signal-cli is present but `--version` fails due Java setup.
    // Fall back to checking if the executable exists on PATH.
    if std::process::Command::new("sh")
        .args(["-lc", "command -v signal-cli >/dev/null 2>&1"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return true;
    }

    signal_cli_command()
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn signal_cli_command() -> std::process::Command {
    let mut cmd = std::process::Command::new("signal-cli");
    if let Some(java_home) = detect_signal_java_home() {
        cmd.env("JAVA_HOME", java_home);
    }
    cmd
}

fn detect_signal_java_home() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        // Helper: check a candidate directory and return it if java -version reports >= min_major.
        let check_java_home = |home: &str, min_major: u32| -> Option<String> {
            let java_bin = format!("{home}/bin/java");
            if !std::path::Path::new(&java_bin).exists() {
                return None;
            }
            if let Ok(out) = std::process::Command::new(&java_bin)
                .arg("-version")
                .output()
            {
                let output = String::from_utf8_lossy(&out.stderr).to_string()
                    + String::from_utf8_lossy(&out.stdout).as_ref();
                if let Some(maj) = parse_java_major_version(&output) {
                    if maj >= min_major {
                        return Some(home.to_string());
                    }
                }
            }
            None
        };

        // 1) Homebrew versioned formulae — check from highest version down.
        //    These are not registered with /usr/libexec/java_home, so must be probed directly.
        let brew_base = "/opt/homebrew/opt";
        let intel_base = "/usr/local/opt";
        for version in ["25", "24", "23", "22", "21"] {
            for base in [brew_base, intel_base] {
                let candidate =
                    format!("{base}/openjdk@{version}/libexec/openjdk.jdk/Contents/Home");
                if let Some(home) = check_java_home(&candidate, 21) {
                    return Some(home);
                }
            }
        }

        // 2) Enumerate all JVMs registered with the macOS JVM framework and pick the
        //    highest version >= 21. /usr/libexec/java_home -v N uses "minimum version"
        //    semantics and may return a lower version, so we probe all installed JVMs.
        if let Ok(out) = std::process::Command::new("/usr/libexec/java_home")
            .args(["--xml"])
            .output()
        {
            // Extract all <JVMHomePath> values from the XML and validate each one.
            let xml = String::from_utf8_lossy(&out.stdout).to_string();
            let mut best: Option<(u32, String)> = None;
            for line in xml.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("<string>") && trimmed.contains("/Contents/Home") {
                    let path = trimmed
                        .trim_start_matches("<string>")
                        .trim_end_matches("</string>")
                        .trim()
                        .to_string();
                    if let Some(home) = check_java_home(&path, 21) {
                        let java_bin = format!("{home}/bin/java");
                        if let Ok(vout) = std::process::Command::new(&java_bin)
                            .arg("-version")
                            .output()
                        {
                            let vout_str = String::from_utf8_lossy(&vout.stderr).to_string()
                                + String::from_utf8_lossy(&vout.stdout).as_ref();
                            if let Some(maj) = parse_java_major_version(&vout_str) {
                                if best.as_ref().is_none_or(|(bv, _)| maj > *bv) {
                                    best = Some((maj, home));
                                }
                            }
                        }
                    }
                }
            }
            if let Some((_, home)) = best {
                return Some(home);
            }
        }

        // 3) Existing JAVA_HOME — only accept if it is actually Java 21+.
        if let Ok(existing) = std::env::var("JAVA_HOME") {
            let existing = existing.trim().to_string();
            if !existing.is_empty() {
                if let Some(home) = check_java_home(&existing, 21) {
                    return Some(home);
                }
            }
        }

        None
    }

    #[cfg(not(target_os = "macos"))]
    {
        std::env::var("JAVA_HOME").ok()
    }
}

/// Parse the major Java version number from the output of `java -version`.
/// Handles both modern (`21`, `23.0.1`) and legacy (`1.8.0_392`) version strings.
#[cfg(target_os = "macos")]
fn parse_java_major_version(version_output: &str) -> Option<u32> {
    for line in version_output.lines() {
        if line.contains("version") {
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    let ver = &line[start + 1..start + 1 + end];
                    let parts: Vec<&str> = ver.split('.').collect();
                    if let Some(first) = parts.first() {
                        if let Ok(n) = first.parse::<u32>() {
                            if n == 1 {
                                // Legacy style: "1.8.0_392" → Java 8
                                if let Some(second) = parts.get(1) {
                                    return second.parse().ok();
                                }
                            }
                            return Some(n);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Run `signal-cli link -n EdgeCrabAgent`, display the QR code URI, and wait
/// for the user to scan it on their phone.
fn run_signal_link_flow(_account: &str) -> anyhow::Result<()> {
    println!();
    println!("  Starting link — signal-cli will output a QR code URI.");
    println!("  On your phone: Signal → Settings → Linked Devices → + → Link New Device");
    println!();

    // signal-cli link can write the URI to stdout or stderr depending on
    // wrapper/runtime behavior, so we listen to both streams.
    let mut child = signal_cli_command()
        .args(["link", "-n", "EdgeCrabAgent"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to run signal-cli link: {e}"))?;

    use std::io::BufRead;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");

    let (tx, rx) = mpsc::channel::<String>();

    {
        let tx_out = tx.clone();
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_out.send(line);
            }
        });
    }
    {
        let tx_err = tx.clone();
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_err.send(line);
            }
        });
    }

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut uri = String::new();
    let mut diagnostics: Vec<String> = Vec::new();

    while Instant::now() < deadline {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(300)) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                diagnostics.push(trimmed.to_string());
            }
            if let Some(found) = extract_signal_link_uri(trimmed) {
                uri = found;
                break;
            }
        }

        if let Some(status) = child.try_wait()? {
            if !status.success() {
                break;
            }
        }
    }

    if uri.starts_with("sgnl://") {
        // Try qrencode for an in-terminal display.
        let qr_ok = std::process::Command::new("qrencode")
            .args(["-t", "ANSIUTF8", &uri])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !qr_ok {
            println!("  Link URI:");
            println!("  {uri}");
            let encoded: String = uri
                .chars()
                .map(|c| match c {
                    'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                    _ => format!("%{:02X}", c as u32),
                })
                .collect();
            println!();
            println!("  Or open in browser for QR code:");
            println!("  https://zxing.org/w/chart?cht=qr&chs=400x400&choe=UTF-8&chl={encoded}");
        }
        println!();
        println!("  Scan the QR code with Signal on your phone, then wait…");
        let status = child.wait()?;
        if status.success() {
            println!("  ✓ Linked! signal-cli is now registered as a secondary device.");
        } else {
            let code = status.code().unwrap_or(-1);
            println!("  ⚠ signal-cli link exited with code {code}.");
            println!(
                "  If linking did not complete, run manually: signal-cli link -n EdgeCrabAgent"
            );
        }
    } else {
        let _ = child.kill();
        let hint = diagnostics
            .iter()
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        if hint.is_empty() {
            println!("  ⚠ signal-cli did not output a link URI within timeout.");
        } else {
            println!("  ⚠ signal-cli did not output a link URI. Last output: {hint}");
        }
        println!("  Run manually: signal-cli link -n EdgeCrabAgent");
        println!("  Then on phone: Signal → Settings → Linked Devices → Link New Device");
    }
    Ok(())
}

fn extract_signal_link_uri(text: &str) -> Option<String> {
    let marker = "sgnl://linkdevice?";
    let start = text.find(marker)?;
    let uri = text[start..].trim();
    if uri.starts_with(marker) {
        Some(uri.to_string())
    } else {
        None
    }
}

fn run_signal_link_flow_rpc(http_url: &str) -> anyhow::Result<()> {
    println!();
    println!("  Starting link via JSON-RPC (docker-native daemon)…");
    println!("  On your phone: Signal → Settings → Linked Devices → + → Link New Device");
    println!();

    let start_req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "startLink",
        "id": "edgecrab-start-link"
    });
    let start_res = signal_rpc_call(http_url, &start_req, 15)?;
    let uri = start_res
        .get("result")
        .and_then(|r| r.get("deviceLinkUri"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if !uri.starts_with("sgnl://") {
        anyhow::bail!("startLink did not return a valid deviceLinkUri");
    }

    // Display QR code in terminal when qrencode is available.
    let qr_ok = std::process::Command::new("qrencode")
        .args(["-t", "ANSIUTF8", &uri])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !qr_ok {
        println!("  Link URI:");
        println!("  {uri}");
        let encoded: String = uri
            .chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                _ => format!("%{:02X}", c as u32),
            })
            .collect();
        println!();
        println!("  Or open in browser for QR code:");
        println!("  https://zxing.org/w/chart?cht=qr&chs=400x400&choe=UTF-8&chl={encoded}");
    }

    println!();
    println!("  Waiting for you to scan the QR code…");

    let finish_req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "finishLink",
        "id": "edgecrab-finish-link",
        "params": {
            "deviceLinkUri": uri,
            "deviceName": "EdgeCrabAgent"
        }
    });
    // finishLink can block while waiting for phone confirmation.
    let _finish_res = signal_rpc_call(http_url, &finish_req, 180)?;

    println!("  ✓ Linked! Signal account is now provisioned in docker-native mode.");
    Ok(())
}

fn configure_whatsapp(config: &mut edgecrab_core::AppConfig) -> anyhow::Result<()> {
    println!();
    println!("  ◆ WhatsApp Setup");
    println!("  ──────────────────");
    println!("  WhatsApp connects via a built-in Baileys bridge (requires Node.js v18+).");
    println!();

    if config
        .gateway
        .platform_requested("whatsapp", config.gateway.whatsapp.enabled)
        && prompt_yes_no("  Disable WhatsApp and keep stored session data?", false)?
    {
        config.gateway.whatsapp.enabled = false;
        config.gateway.disable_platform("whatsapp");
        println!("  ✓ WhatsApp disabled");
        return Ok(());
    }

    // ── Prerequisite checks ──────────────────────────────────────────
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
        println!("  ⚠ Prerequisites missing:");
        if !has_node {
            println!("    ✗ Node.js not found — install from https://nodejs.org (v18+)");
        }
        if !has_npm {
            println!("    ✗ npm not found — usually bundled with Node.js");
        }
        println!();
        println!("  Install Node.js first, then re-run: edgecrab gateway configure whatsapp");
        return Ok(());
    }

    // Show detected versions
    let node_ver = std::process::Command::new("node")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let npm_ver = std::process::Command::new("npm")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    println!("  ✓ Node.js {} / npm {}", node_ver.trim(), npm_ver.trim());
    println!();

    // ── Mode selection ───────────────────────────────────────────────
    println!("  How will you use WhatsApp with EdgeCrab?");
    println!("    1. Separate bot number (recommended for multi-user)");
    println!("    2. Personal number (self-chat — talk to yourself)");
    let current_choice = if config.gateway.whatsapp.mode == "bot" {
        1
    } else {
        2
    };
    config.gateway.whatsapp.mode = prompt_whatsapp_mode(current_choice)?.to_string();

    // ── Allowed users ────────────────────────────────────────────────
    let current_users = if config.gateway.whatsapp.allowed_users.is_empty() {
        "(none)".to_string()
    } else {
        config.gateway.whatsapp.allowed_users.join(", ")
    };
    println!();
    println!("  Allowed users: {current_users}");
    if config.gateway.whatsapp.mode == "bot" {
        println!("  ℹ Phone numbers with country code, no + prefix (e.g. 33614251689)");
    }
    let prompt_text = if config.gateway.whatsapp.mode == "bot" {
        "  Allowed phone numbers (comma-separated, blank keeps current, '-' clears): "
    } else {
        "  Your phone number (blank keeps current, '-' clears): "
    };
    let input = prompt_line(prompt_text)?;
    if input.trim() == "-" {
        config.gateway.whatsapp.allowed_users.clear();
    } else if !input.trim().is_empty() {
        config.gateway.whatsapp.allowed_users = parse_csv_values(&input)
            .into_iter()
            .map(|value| value.trim_start_matches('+').to_string())
            .collect();
    }

    // ── Bridge port ──────────────────────────────────────────────────
    println!();
    let input = prompt_line(&format!(
        "  Bridge port [{}]: ",
        config.gateway.whatsapp.bridge_port
    ))?;
    if let Ok(p) = input.trim().parse::<u16>() {
        config.gateway.whatsapp.bridge_port = p;
    }

    if prompt_yes_no("  Enable WhatsApp now?", true)? {
        config.gateway.whatsapp.enabled = true;
        config.gateway.enable_platform("whatsapp");
    } else {
        config.gateway.whatsapp.enabled = false;
        config.gateway.disable_platform("whatsapp");
        println!("  ✓ WhatsApp disabled");
        return Ok(());
    }

    if config.gateway.whatsapp.session_path.is_none() {
        config.gateway.whatsapp.session_path =
            Some(edgecrab_gateway::whatsapp::WhatsAppAdapter::default_session_path());
    }

    println!(
        "  ✓ WhatsApp enabled (mode: {})",
        config.gateway.whatsapp.mode
    );
    println!();

    // ── Install bridge dependencies before pairing ───────────────────
    let adapter_cfg =
        edgecrab_gateway::whatsapp::WhatsappAdapterConfig::from(&config.gateway.whatsapp);

    match edgecrab_gateway::whatsapp::WhatsAppAdapter::resolve_bridge_assets(&adapter_cfg) {
        Ok(assets) => {
            println!("  Bridge: {}", assets.bridge_script.display());
            println!("  Session: {}", adapter_cfg.session_path.display());

            // Pre-install dependencies so pairing doesn't fail
            if !assets.bridge_dir.join("node_modules").exists() {
                println!();
                println!("  Installing bridge dependencies (first time only)...");
                let install_status = std::process::Command::new("npm")
                    .args(["install", "--ignore-scripts"])
                    .current_dir(&assets.bridge_dir)
                    .status();
                match install_status {
                    Ok(s) if s.success() => {
                        println!("  ✓ Dependencies installed");
                        // Run the sharp prebuilt installer
                        let postinstall = assets.bridge_dir.join("install-sharp-prebuilt.js");
                        if postinstall.exists() {
                            let _ = std::process::Command::new("node")
                                .arg(&postinstall)
                                .current_dir(&assets.bridge_dir)
                                .status();
                        }
                    }
                    Ok(s) => {
                        println!("  ⚠ npm install exited with code {:?}", s.code());
                        println!("  You may need to run manually:");
                        println!(
                            "    cd {} && npm install --ignore-scripts",
                            assets.bridge_dir.display()
                        );
                    }
                    Err(e) => {
                        println!("  ⚠ Failed to run npm install: {e}");
                    }
                }
            }

            // ── Offer to pair ────────────────────────────────────────
            println!();
            let creds_path = adapter_cfg.session_path.join("creds.json");

            if creds_path.exists() {
                println!("  Existing WhatsApp session found.");
                if prompt_yes_no("  Re-pair (scan QR code again)?", false)? {
                    std::fs::remove_dir_all(&adapter_cfg.session_path)?;
                    println!("  Open WhatsApp on the target device and scan the QR code:");
                    edgecrab_gateway::whatsapp::WhatsAppAdapter::pair(&adapter_cfg)?;
                    println!("  ✓ WhatsApp paired successfully");
                }
            } else if prompt_yes_no("  Pair now (scan QR code)?", true)? {
                println!("  Open WhatsApp on the target device and scan the QR code:");
                edgecrab_gateway::whatsapp::WhatsAppAdapter::pair(&adapter_cfg)?;
                println!("  ✓ WhatsApp paired successfully");
            } else {
                println!("  Run `edgecrab whatsapp` later to pair via QR code.");
            }
        }
        Err(e) => {
            println!("  ⚠ Could not locate WhatsApp bridge: {e}");
            println!("  WhatsApp is enabled in config but pairing will need to happen later.");
            println!("  Run: edgecrab whatsapp");
        }
    }

    Ok(())
}

// ─── Status display ───────────────────────────────────────────────────

fn print_platform_status(config: &edgecrab_core::AppConfig) {
    let diagnostics = collect_platform_diagnostics(config);
    let ready = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.state == PlatformState::Ready)
        .count();
    let available = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.state == PlatformState::Available)
        .count();
    let attention = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.state == PlatformState::Incomplete)
        .count();

    println!("  Platform Status:");
    println!("  ────────────────");

    for diagnostic in &diagnostics {
        println!(
            "    {:<10} {:<18} {}",
            diagnostic.name,
            diagnostic.state.label(),
            diagnostic.detail
        );
    }

    println!();
    println!(
        "  Gateway bind: {}:{}",
        config.gateway.host, config.gateway.port
    );
    println!("  Summary: {ready} enabled, {available} ready to enable, {attention} need attention");
}

// ─── .env file management ─────────────────────────────────────────────

fn env_file_path() -> PathBuf {
    edgecrab_core::config::edgecrab_home().join(".env")
}

/// Save a key to ~/.edgecrab/.env, creating or updating as needed.
fn save_env_key(env_var: &str, value: &str) -> anyhow::Result<()> {
    let env_path = env_file_path();
    let home = edgecrab_core::config::edgecrab_home();
    std::fs::create_dir_all(&home)?;

    let entry = format!("{env_var}={value}\n");

    if env_path.exists() {
        let mut existing = String::new();
        std::fs::File::open(&env_path)?.read_to_string(&mut existing)?;

        if existing.lines().any(|line| {
            line.strip_prefix(env_var)
                .is_some_and(|rest| rest.starts_with('='))
        }) {
            // Replace existing key
            let updated: String = existing
                .lines()
                .map(|line| {
                    if line
                        .strip_prefix(env_var)
                        .is_some_and(|rest| rest.starts_with('='))
                    {
                        format!("{env_var}={value}")
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&env_path, format!("{updated}\n"))?;
        } else {
            let mut f = std::fs::OpenOptions::new().append(true).open(&env_path)?;
            f.write_all(entry.as_bytes())?;
        }
    } else {
        std::fs::write(&env_path, entry)?;
    }

    // Also set in current process environment so subsequent checks work.
    // SAFETY: EdgeCrab CLI is single-threaded at setup time; no other
    // threads are reading the environment concurrently.
    unsafe { std::env::set_var(env_var, value) };

    Ok(())
}

fn remove_env_key(env_var: &str) -> anyhow::Result<()> {
    let env_path = env_file_path();
    if !env_path.exists() {
        unsafe { std::env::remove_var(env_var) };
        return Ok(());
    }

    let mut existing = String::new();
    std::fs::File::open(&env_path)?.read_to_string(&mut existing)?;
    let filtered = existing
        .lines()
        .filter(|line| {
            !line
                .strip_prefix(env_var)
                .is_some_and(|rest| rest.starts_with('='))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if filtered.is_empty() {
        std::fs::remove_file(&env_path)?;
    } else {
        std::fs::write(&env_path, format!("{filtered}\n"))?;
    }

    unsafe { std::env::remove_var(env_var) };
    Ok(())
}

// ─── Interactive helpers ──────────────────────────────────────────────

fn resolve_config_path(args: &CliArgs) -> anyhow::Result<PathBuf> {
    match &args.config {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(ensure_edgecrab_home()?.join("config.yaml")),
    }
}

fn prompt_yes_no(question: &str, default: bool) -> io::Result<bool> {
    let theme = wizard_theme();
    Confirm::with_theme(&theme)
        .with_prompt(question)
        .default(default)
        .wait_for_newline(true)
        .interact()
        .map_err(dialoguer_to_io)
}

fn prompt_line(prompt_str: &str) -> io::Result<String> {
    let theme = wizard_theme();
    Input::<String>::with_theme(&theme)
        .with_prompt(normalize_prompt(prompt_str))
        .allow_empty(true)
        .interact_text()
        .map_err(dialoguer_to_io)
        .map(sanitize_terminal_input)
}

fn prompt_secret(prompt_str: &str) -> io::Result<String> {
    println!(
        "  Hidden input. Characters appear as bullets. Paste works. Enter submits. Backspace edits. Ctrl+U clears."
    );

    loop {
        let secret = read_secret_once(prompt_str)?;
        let normalized = sanitize_terminal_input(secret);
        let trimmed = normalized.trim();

        if trimmed.is_empty() {
            println!("  ↺ No new secret entered");
            return Ok(String::new());
        }
        if trimmed == "-" {
            println!("  ✓ Secret marked for clearing");
            return Ok(normalized);
        }

        let confirmation = read_secret_once("  Confirm secret")?;
        let confirmation = sanitize_terminal_input(confirmation);
        if confirmation != normalized {
            println!("  ✗ Secret confirmation did not match. Nothing was changed.");
            continue;
        }

        println!("  ✓ Secret captured ({} chars)", normalized.chars().count());
        return Ok(normalized);
    }
}

fn prompt_select(prompt: &str, items: &[&str], default: usize) -> io::Result<usize> {
    let theme = wizard_theme();
    Select::with_theme(&theme)
        .with_prompt(prompt)
        .default(default)
        .items(items)
        .interact()
        .map_err(dialoguer_to_io)
}

fn prompt_select_strings(prompt: &str, items: &[String], default: usize) -> io::Result<usize> {
    let theme = wizard_theme();
    Select::with_theme(&theme)
        .with_prompt(prompt)
        .default(default)
        .items(items)
        .interact()
        .map_err(dialoguer_to_io)
}

fn prompt_nonempty_with_default(prompt: &str, default: &str) -> io::Result<String> {
    let theme = wizard_theme();
    Input::<String>::with_theme(&theme)
        .with_prompt(prompt)
        .default(default.to_string())
        .validate_with(|value: &String| -> Result<(), &str> {
            if sanitize_terminal_input(value.clone()).is_empty() {
                Err("Value cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact_text()
        .map_err(dialoguer_to_io)
        .map(sanitize_terminal_input)
}

fn prompt_port_with_default(prompt: &str, default: u16) -> io::Result<u16> {
    let theme = wizard_theme();
    Input::<String>::with_theme(&theme)
        .with_prompt(prompt)
        .default(default.to_string())
        .validate_with(|value: &String| -> Result<(), &str> {
            match sanitize_terminal_input(value.clone()).parse::<u16>() {
                Ok(0) => Err("Port must be between 1 and 65535"),
                Ok(_) => Ok(()),
                Err(_) => Err("Enter a valid TCP port"),
            }
        })
        .interact_text()
        .map_err(dialoguer_to_io)
        .and_then(|value| {
            sanitize_terminal_input(value)
                .parse::<u16>()
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))
        })
}

fn prompt_signal_backend(current_backend: &str) -> io::Result<SignalBackend> {
    let items = [
        "cli           — local signal-cli binary",
        "docker-native — managed container, no Java on host",
    ];
    let default = if SignalBackend::from_input(current_backend, current_backend)
        == SignalBackend::DockerNative
    {
        1
    } else {
        0
    };
    match prompt_select("Signal backend", &items, default)? {
        1 => Ok(SignalBackend::DockerNative),
        _ => Ok(SignalBackend::Cli),
    }
}

fn prompt_whatsapp_mode(current_choice: usize) -> io::Result<&'static str> {
    let items = [
        "Separate bot number — recommended for multi-user",
        "Personal number     — self-chat with your own account",
    ];
    match prompt_select("WhatsApp mode", &items, current_choice.saturating_sub(1))? {
        1 => Ok("self-chat"),
        _ => Ok("bot"),
    }
}

fn wizard_theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

fn dialoguer_to_io(err: dialoguer::Error) -> io::Error {
    io::Error::other(err.to_string())
}

fn read_secret_once(prompt_str: &str) -> io::Result<String> {
    if !io::stderr().is_terminal() {
        let theme = wizard_theme();
        return Password::with_theme(&theme)
            .with_prompt(normalize_prompt(prompt_str))
            .allow_empty_password(true)
            .report(false)
            .interact()
            .map_err(dialoguer_to_io);
    }

    let prompt = normalize_prompt(prompt_str);
    let mut stderr = io::stderr();
    let _session = SecretInputSession::start(&mut stderr)?;
    let mut secret = String::new();
    render_secret_line(&mut stderr, &prompt, &secret)?;

    loop {
        match event::read().map_err(io::Error::other)? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Enter => {
                    writeln!(stderr)?;
                    stderr.flush()?;
                    return Ok(secret);
                }
                KeyCode::Backspace => {
                    secret.pop();
                    render_secret_line(&mut stderr, &prompt, &secret)?;
                }
                KeyCode::Esc => {
                    writeln!(stderr)?;
                    stderr.flush()?;
                    return Ok(String::new());
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    secret.clear();
                    render_secret_line(&mut stderr, &prompt, &secret)?;
                }
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    trim_last_secret_word(&mut secret);
                    render_secret_line(&mut stderr, &prompt, &secret)?;
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    secret.push(c);
                    render_secret_line(&mut stderr, &prompt, &secret)?;
                }
                _ => {}
            },
            Event::Paste(text) => {
                secret.push_str(&sanitize_pasted_secret_chunk(&text));
                render_secret_line(&mut stderr, &prompt, &secret)?;
            }
            _ => {}
        }
    }
}

fn render_secret_line(stderr: &mut io::Stderr, prompt: &str, secret: &str) -> io::Result<()> {
    execute!(
        stderr,
        cursor::MoveToColumn(0),
        terminal::Clear(ClearType::CurrentLine)
    )?;

    let preview = secret_preview(secret);
    if preview.is_empty() {
        write!(stderr, "? {prompt} › ")?;
    } else {
        write!(stderr, "? {prompt} › {preview}")?;
    }
    stderr.flush()
}

fn secret_preview(secret: &str) -> String {
    let count = secret.chars().count();
    if count == 0 {
        return String::new();
    }

    let shown = count.min(12);
    let mut mask = "•".repeat(shown);
    if count > shown {
        mask.push('…');
    }
    format!("{mask}  ({count} chars)")
}

fn sanitize_pasted_secret_chunk(text: &str) -> String {
    text.chars()
        .filter(|ch| *ch != '\r' && *ch != '\n')
        .collect()
}

fn trim_last_secret_word(secret: &mut String) {
    while secret.ends_with(' ') {
        secret.pop();
    }
    while secret.chars().last().is_some_and(|ch| !ch.is_whitespace()) {
        secret.pop();
    }
}

struct SecretInputSession;

impl SecretInputSession {
    fn start(stderr: &mut io::Stderr) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(error) = execute!(stderr, EnableBracketedPaste) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(Self)
    }
}

impl Drop for SecretInputSession {
    fn drop(&mut self) {
        let mut stderr = io::stderr();
        let _ = execute!(stderr, DisableBracketedPaste);
        let _ = terminal::disable_raw_mode();
    }
}

fn normalize_prompt(prompt: &str) -> String {
    prompt.trim().trim_end_matches(':').trim().to_string()
}

fn sanitize_terminal_input(value: String) -> String {
    let collapsed = value.replace('\r', "");
    let trimmed = collapsed.trim();
    let unquoted = strip_wrapping_quotes(trimmed);
    unquoted.trim().to_string()
}

fn strip_wrapping_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0];
        let last = bytes[value.len() - 1];
        if first == last && matches!(first, b'"' | b'\'' | b'`') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn parse_csv_values(input: &str) -> Vec<String> {
    let mut values = Vec::new();
    for raw in input.split(',') {
        let candidate = sanitize_terminal_input(raw.to_string());
        if !candidate.is_empty() && !values.contains(&candidate) {
            values.push(candidate);
        }
    }
    values
}

fn normalize_discord_user_id(value: &str) -> Option<String> {
    let mut normalized = sanitize_terminal_input(value.to_string());
    if normalized.starts_with("<@") && normalized.ends_with('>') {
        normalized = normalized
            .trim_start_matches("<@")
            .trim_start_matches('!')
            .trim_end_matches('>')
            .to_string();
    }
    (!normalized.is_empty()).then_some(normalized)
}

fn print_banner(title: &str, subtitle: &str) {
    let content = format!("{title} — {subtitle}");
    let inner_width = content.len() + 2;
    let top = "═".repeat(inner_width);
    let padded = format!(" {content} ");
    println!();
    println!("╔{top}╗");
    println!("║{padded}║");
    println!("╚{top}╝");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    #[serial_test::serial(edgecrab_home_env)]
    fn save_env_key_updates_exact_match_only() {
        let _guard = crate::gateway_catalog::TEST_ENV_LOCK
            .lock()
            .expect("env lock");
        let home = tempdir().expect("temp dir");
        unsafe {
            std::env::set_var("EDGECRAB_HOME", home.path());
        }

        save_env_key("TEST_KEY_EXTRA", "keep").expect("save extra key");
        save_env_key("TEST_KEY", "first").expect("save key");
        save_env_key("TEST_KEY", "second").expect("update key");

        let content = std::fs::read_to_string(env_file_path()).expect("read env");
        assert!(content.contains("TEST_KEY=second"));
        assert!(content.contains("TEST_KEY_EXTRA=keep"));

        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[test]
    #[serial_test::serial(edgecrab_home_env)]
    fn remove_env_key_deletes_only_target_key() {
        let _guard = crate::gateway_catalog::TEST_ENV_LOCK
            .lock()
            .expect("env lock");
        let home = tempdir().expect("temp dir");
        unsafe {
            std::env::set_var("EDGECRAB_HOME", home.path());
        }

        save_env_key("REMOVE_ME", "gone").expect("save remove key");
        save_env_key("REMOVE_ME_TOO", "keep").expect("save keep key");
        remove_env_key("REMOVE_ME").expect("remove env key");

        let content = std::fs::read_to_string(env_file_path()).expect("read env");
        assert!(!content.contains("REMOVE_ME=gone"));
        assert!(content.contains("REMOVE_ME_TOO=keep"));

        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[test]
    fn sanitize_terminal_input_trims_crlf_and_wrapping_quotes() {
        assert_eq!(
            sanitize_terminal_input(" \"token-value\"\r".to_string()),
            "token-value"
        );
        assert_eq!(sanitize_terminal_input("`abc123`".to_string()), "abc123");
        assert_eq!(
            sanitize_terminal_input("  keep-me  ".to_string()),
            "keep-me"
        );
    }

    #[test]
    fn parse_csv_values_deduplicates_and_normalizes_pasted_values() {
        assert_eq!(
            parse_csv_values("  \"alice\" , bob, alice,\r\n, 'carol' "),
            vec!["alice".to_string(), "bob".to_string(), "carol".to_string()]
        );
    }

    #[test]
    fn normalize_discord_user_id_strips_mentions_and_quotes() {
        assert_eq!(
            normalize_discord_user_id("\"<@!123456>\""),
            Some("123456".to_string())
        );
        assert_eq!(
            normalize_discord_user_id("  <@7890>  "),
            Some("7890".to_string())
        );
    }

    #[test]
    fn secret_preview_shows_mask_and_character_count() {
        assert_eq!(secret_preview(""), "");
        assert_eq!(secret_preview("abc"), "•••  (3 chars)");
        assert_eq!(
            secret_preview("123456789012345"),
            "••••••••••••…  (15 chars)"
        );
    }

    #[test]
    fn sanitize_pasted_secret_chunk_strips_newlines_only() {
        assert_eq!(
            sanitize_pasted_secret_chunk("line1\r\nline2\n"),
            "line1line2".to_string()
        );
    }

    #[test]
    fn trim_last_secret_word_removes_trailing_word_and_spaces() {
        let mut secret = "alpha beta".to_string();
        trim_last_secret_word(&mut secret);
        assert_eq!(secret, "alpha ");

        let mut single = "token".to_string();
        trim_last_secret_word(&mut single);
        assert_eq!(single, "");
    }
}
