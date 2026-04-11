//! # setup — Interactive setup wizard with reconfiguration support
//!
//! Supports first-run and reconfiguration workflows:
//!
//! ```text
//! edgecrab setup                 ← full wizard (reconfigure menu if existing)
//! edgecrab setup model           ← reconfigure model & provider only
//! edgecrab setup tools           ← reconfigure toolsets
//! edgecrab setup gateway         ← configure messaging platforms
//! edgecrab setup agent           ← agent settings (iterations, etc.)
//! edgecrab setup --force         ← overwrite config from scratch
//! ```
//!
//! WHY reconfiguration: Users frequently need to change providers,
//! update API keys, or enable new platforms. Telling them to delete
//! their config is a poor UX — hermes-agent has a full reconfigure
//! menu and we should too.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use dialoguer::{Confirm, Input, Password, Select, theme::ColorfulTheme};
use dirs::home_dir;

use crate::vision_models::{
    available_vision_model_options, canonical_provider, current_model_supports_vision,
    parse_selection_spec,
};

/// Environment variable → provider name mappings.
///
/// Each entry is (env_var, provider_id, display_name).
/// WHY a slice not a HashMap: order matters — we check in priority order.
const PROVIDER_ENV_MAP: &[(&str, &str, &str)] = &[
    (
        "GITHUB_TOKEN",
        "copilot",
        "GitHub Copilot (free for Copilot subscribers)",
    ),
    ("OPENAI_API_KEY", "openai", "OpenAI (GPT-4.1, GPT-5, o3/o4)"),
    (
        "ANTHROPIC_API_KEY",
        "anthropic",
        "Anthropic (Claude 4.5/4.6)",
    ),
    ("GOOGLE_API_KEY", "gemini", "Google Gemini (2.5/3.x)"),
    ("XAI_API_KEY", "xai", "xAI (Grok 3/4)"),
    ("DEEPSEEK_API_KEY", "deepseek", "DeepSeek (V3, R1)"),
    (
        "HUGGING_FACE_HUB_TOKEN",
        "huggingface",
        "Hugging Face Inference",
    ),
    ("ZAI_API_KEY", "zai", "Z.AI / GLM (4.5-5)"),
    (
        "OPENROUTER_API_KEY",
        "openrouter",
        "OpenRouter (600+ models incl. Nous Hermes 3)",
    ),
];

/// Providers available without any API key (local).
const LOCAL_PROVIDERS: &[(&str, &str)] = &[
    ("ollama", "Ollama (local models, free)"),
    ("lmstudio", "LMStudio (local models, free)"),
];

/// Available setup sections and their labels.
const SETUP_SECTIONS: &[(&str, &str)] = &[
    ("model", "Model & Provider"),
    ("tools", "Toolsets"),
    ("gateway", "Messaging Platforms"),
    ("agent", "Agent Settings"),
];

/// Default model per provider — delegates to `ModelCatalog`.
fn default_model(provider: &str) -> String {
    edgecrab_core::ModelCatalog::default_model_for(provider)
        .unwrap_or_else(|| "ollama/gemma4:latest".to_string())
}

/// Returns the default edgecrab home directory (~/.edgecrab/).
pub fn edgecrab_home() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".edgecrab")
}

// ─── Shared wizard helpers ────────────────────────────────────────────

/// Consistent dialoguer theme for all setup prompts.
/// WHY: Parity with gateway_setup — same look and feel throughout.
fn wizard_theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

/// Convert a dialoguer error to `io::Error` for uniform error handling.
fn dialoguer_to_io(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// Arrow-key selectable menu. Parity with `gateway_setup::prompt_select`.
fn prompt_select(question: &str, items: &[&str], default: usize) -> io::Result<usize> {
    Select::with_theme(&wizard_theme())
        .with_prompt(question)
        .items(items)
        .default(default)
        .interact()
        .map_err(dialoguer_to_io)
}

/// Strip BiDi-override and other control code-points that can spoof terminal
/// output. Shared with gateway_setup; inlined here to avoid a pub export.
fn sanitize_input(s: String) -> String {
    s.chars()
        .filter(|c| {
            !matches!(*c as u32,
                0x00..=0x08 | 0x0B..=0x0C | 0x0E..=0x1F | 0x7F
                | 0x202A..=0x202E | 0x2066..=0x2069)
        })
        .collect()
}

/// Main entry point — dispatches to the right workflow based on options.
///
/// Called from `main.rs` with the parsed CLI args.
pub fn run_with_options(section: Option<&str>, force: bool) -> anyhow::Result<()> {
    let home = edgecrab_home();
    let config_path = home.join("config.yaml");
    let is_existing = config_path.exists();

    // Section-specific reconfiguration
    if let Some(sec) = section {
        if !SETUP_SECTIONS.iter().any(|(k, _)| *k == sec) {
            let valid: Vec<&str> = SETUP_SECTIONS.iter().map(|(k, _)| *k).collect();
            anyhow::bail!(
                "Unknown setup section: {sec}\nAvailable: {}",
                valid.join(", ")
            );
        }
        if !is_existing {
            println!("⚠ No config found. Running full setup first.\n");
            return run_fresh_setup(&home, &config_path);
        }
        return run_section(sec, &home, &config_path);
    }

    // --force: treat as fresh, even if config exists
    if force {
        println!("⚠ --force: overwriting existing configuration.\n");
        return run_fresh_setup(&home, &config_path);
    }

    // Existing config → show reconfiguration menu
    if is_existing {
        return run_reconfigure_menu(&home, &config_path);
    }

    // No config → fresh setup
    run_fresh_setup(&home, &config_path)
}

/// Backward-compatible entry point (used by tests).
#[allow(dead_code)]
pub fn run() -> anyhow::Result<()> {
    run_with_options(None, false)
}

// ─── Fresh Setup ──────────────────────────────────────────────────────

fn run_fresh_setup(home: &Path, config_path: &Path) -> anyhow::Result<()> {
    if offer_openclaw_migration(home)? {
        println!("\n✓ OpenClaw migration completed. Loading the imported EdgeCrab config.\n");
        return run_reconfigure_menu(home, config_path);
    }

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║         Welcome to EdgeCrab — AI terminal agent       ║");
    println!("╚══════════════════════════════════════════════════════╝\n");
    println!("Step 1 of 2: Model & Provider\n");
    println!("You can always re-run: edgecrab setup");
    println!("Or edit directly:     {}\n", config_path.display());

    // Detect already-configured providers from environment
    let detected: Vec<(&str, &str, &str)> = PROVIDER_ENV_MAP
        .iter()
        .filter(|(env, _, _)| std::env::var(env).is_ok())
        .copied()
        .collect();

    let chosen_provider = if detected.is_empty() {
        choose_provider_interactively()?
    } else {
        println!("✓ Detected provider(s) from environment variables:");
        for (env, _id, name) in &detected {
            println!("  • {name}  [${env}]");
        }
        println!();

        if detected.len() == 1 {
            println!("Using: {}", detected[0].2);
            detected[0].1.to_string()
        } else {
            let options: Vec<String> = detected
                .iter()
                .map(|(_env, id, name)| format!("{name} ({id})"))
                .collect();
            let idx = prompt_select(
                "Multiple providers detected — choose one",
                &options.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                0,
            )
            .map_err(|e| anyhow::anyhow!("provider selection: {e}"))?;
            detected[idx].1.to_string()
        }
    };

    let model = default_model(&chosen_provider);
    println!("\n✓ Provider: {chosen_provider}");
    println!("✓ Default model: {model}");

    let mut config = CurrentConfig {
        model: Some(model.clone()),
        provider: Some(chosen_provider.clone()),
        max_iterations: 90,
        streaming: true,
        toolsets: vec![
            "core".into(),
            "web".into(),
            "terminal".into(),
            "memory".into(),
            "skills".into(),
        ],
        gateway_platforms: Vec::new(),
        gateway_host: "127.0.0.1".to_string(),
        gateway_port: 8080,
        save_trajectories: false,
        skip_memory: false,
        skip_context: false,
        vision_provider: None,
        vision_model: None,
        vision_base_url: None,
        vision_api_key_env: None,
        raw: serde_yml::Value::Mapping(Default::default()),
    };

    if prompt_yes_no(
        "Configure vision routing now? (recommended if you use image analysis often)",
        !current_model_supports_vision(&model),
    )? {
        configure_vision_backend(&mut config)?;
    }

    save_current_config(home, config_path, &config)?;

    println!("\n✅  Step 1 complete: model configured ({chosen_provider} / {model})");
    println!("   Config written to: {}", config_path.display());

    // ── Step 2: Messaging Platforms (optional) ──────────────────────────────
    println!("\n──────────────────────────────────────────────────────────");
    println!("Step 2 of 2: Messaging Platforms (optional)\n");
    println!("  Connect EdgeCrab to Telegram, Discord, Slack, and 12 more platforms.");
    println!("  Tip: Nous Hermes 3 (NousResearch) works great inside these channels.");
    println!("  You can skip this step and run `edgecrab gateway configure` later.\n");

    if prompt_yes_no("  Configure messaging platforms now?", false)? {
        crate::gateway_setup::configure_for_path(config_path, None)
            .map_err(|e| anyhow::anyhow!("gateway setup: {e}"))?;
    } else {
        println!("  Skipped. Run `edgecrab gateway configure` when ready.");
    }

    println!("\n============================================================");
    println!("  Setup complete!");
    println!("============================================================");
    println!("  Quick start:");
    println!("    edgecrab                      - interactive chat");
    println!("    edgecrab -q \"explain Rust async\"  - headless query");
    println!("    edgecrab doctor               - verify configuration");
    println!("    edgecrab gateway configure    - add messaging platforms");
    println!("    edgecrab migrate              - import from hermes-agent");
    println!();
    println!("  Using OpenRouter? Try Nous Hermes 3 (NousResearch):");
    println!("    model: nousresearch/hermes-3-llama-3.1-405b");
    println!("    Visit https://openrouter.ai/nousresearch for all variants.\n");

    Ok(())
}

fn offer_openclaw_migration(home: &Path) -> anyhow::Result<bool> {
    use edgecrab_migrate::openclaw::{
        OpenClawMigrationOptions, OpenClawMigrator, OpenClawPreset, SkillConflictMode,
    };

    let Some(source_root) = OpenClawMigrator::default_source_home() else {
        return Ok(false);
    };

    if !prompt_yes_no(
        &format!(
            "Found OpenClaw data at {}. Import it into EdgeCrab before setup?",
            source_root.display()
        ),
        true,
    )? {
        return Ok(false);
    }

    let options = OpenClawMigrationOptions {
        execute: true,
        overwrite: true,
        migrate_secrets: true,
        preset: OpenClawPreset::Full,
        workspace_target: None,
        skill_conflict_mode: SkillConflictMode::Overwrite,
    };
    let migrator = OpenClawMigrator::new(source_root.clone(), home.to_path_buf(), options);
    let report = migrator.migrate_all()?;

    let succeeded = report.success_count();
    let failed = report.failed_count();
    println!();
    println!("Imported OpenClaw source: {}", source_root.display());
    println!("Migrated items: {succeeded}");
    if failed > 0 {
        println!("Failures: {failed}");
    }

    Ok(succeeded > 0)
}

// ─── Reconfigure Menu ─────────────────────────────────────────────────

fn run_reconfigure_menu(home: &Path, config_path: &Path) -> anyhow::Result<()> {
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║         EdgeCrab — Setup & Reconfiguration            ║");
    println!("╚══════════════════════════════════════════════════════╝\n");
    println!("✓ Config loaded from: {}\n", config_path.display());

    // Load current config to show current values
    let current = load_current_config(config_path);

    if let Some(ref model) = current.model {
        println!("  Current model:    {model}");
    }
    if let Some(ref provider) = current.provider {
        println!("  Current provider: {provider}");
    }
    match (
        current.vision_provider.as_deref(),
        current.vision_model.as_deref(),
    ) {
        (Some(provider), Some(model)) => println!("  Current vision:   {provider}/{model}"),
        _ => println!("  Current vision:   auto"),
    }
    println!();

    let menu = [
        "Reconfigure model & provider",
        "Reconfigure toolsets",
        "Configure messaging platforms (gateway)",
        "Configure agent settings",
        "Full setup (reconfigure everything)",
        "Exit",
    ];

    let choice = prompt_select("What would you like to configure?", &menu, 5)
        .map_err(anyhow::Error::from)?;

    match choice {
        0 => run_section("model", home, config_path)?,
        1 => run_section("tools", home, config_path)?,
        2 => run_section("gateway", home, config_path)?,
        3 => run_section("agent", home, config_path)?,
        4 => {
            println!();
            run_fresh_setup(home, config_path)?;
        }
        _ => {
            println!("✓ No changes made. Run `edgecrab setup` again when ready.");
        }
    }

    Ok(())
}

// ─── Section-specific Reconfiguration ─────────────────────────────────

fn run_section(section: &str, home: &Path, config_path: &Path) -> anyhow::Result<()> {
    let label = SETUP_SECTIONS
        .iter()
        .find(|(k, _)| *k == section)
        .map(|(_, l)| *l)
        .unwrap_or(section);

    println!("\n◆ Setup — {label}\n");

    let mut current = load_current_config(config_path);

    match section {
        "model" => setup_model_section(&mut current)?,
        "tools" => setup_tools_section(&mut current)?,
        "gateway" => setup_gateway_section(&mut current, config_path)?,
        "agent" => setup_agent_section(&mut current)?,
        _ => anyhow::bail!("Unknown section: {section}"),
    }

    // Write back the updated config
    save_current_config(home, config_path, &current)?;
    println!("\n✅ {label} configuration updated!");
    println!("  Config: {}", config_path.display());

    Ok(())
}

/// Setup the model & provider section.
fn setup_model_section(config: &mut CurrentConfig) -> anyhow::Result<()> {
    if let Some(ref provider) = config.provider {
        println!("  Current provider: {provider}");
    }
    if let Some(ref model) = config.model {
        println!("  Current model:    {model}");
    }
    println!();

    let choice = prompt_yes_no("Change provider?", true)?;
    if choice {
        let provider = choose_provider_interactively()?;
        let model = default_model(&provider);
        config.provider = Some(provider);
        config.model = Some(model);
        println!("  ✓ Provider updated");
    }

    // Allow changing model even if provider unchanged
    let change_model = prompt_yes_no("Change default model?", !choice)?;
    if change_model {
        let current = config.model.as_deref().unwrap_or("(none)");
        let input = prompt_line(&format!("  Enter model name [{current}]: "))?;
        let trimmed = input.trim();
        if !trimmed.is_empty() {
            config.model = Some(trimmed.to_string());
        }
    }

    let current_vision = match (
        config.vision_provider.as_deref(),
        config.vision_model.as_deref(),
    ) {
        (Some(provider), Some(model)) => format!("{provider}/{model}"),
        _ => "auto".to_string(),
    };
    println!("  Current vision routing: {current_vision}");
    if prompt_yes_no("Configure vision routing?", false)? {
        configure_vision_backend(config)?;
    }

    // Max iterations
    let change_iter = prompt_yes_no("Change max iterations?", false)?;
    if change_iter {
        let current = config.max_iterations;
        let input = prompt_line(&format!("  Max iterations [{current}]: "))?;
        if let Ok(n) = input.trim().parse::<u32>() {
            config.max_iterations = n;
        }
    }

    Ok(())
}

fn configure_vision_backend(config: &mut CurrentConfig) -> anyhow::Result<()> {
    let current_model = config.model.as_deref().unwrap_or("unknown");
    let current_supports_vision = current_model_supports_vision(current_model);
    println!();
    println!("  Vision & image analysis");
    if current_supports_vision {
        println!("  ✓ Your current chat model already supports vision.");
    } else {
        println!("  ! Your current chat model is not known to support vision directly.");
        println!("    A dedicated multimodal backend is recommended if you rely on images.");
    }

    let choices = [
        "Auto — use the current chat model when it supports vision",
        "Select a dedicated vision model",
        "Custom OpenAI-compatible endpoint",
    ];
    let default_idx = if current_supports_vision { 0 } else { 1 };
    let choice = prompt_select("Choose vision routing", &choices, default_idx)
        .map_err(|e| anyhow::anyhow!("vision routing selection: {e}"))?;

    match choice {
        0 => {
            config.vision_provider = None;
            config.vision_model = None;
            config.vision_base_url = None;
            config.vision_api_key_env = None;
            println!("  ✓ Vision routing set to auto.");
        }
        1 => {
            let options = available_vision_model_options();
            let mut labels: Vec<String> = options
                .iter()
                .map(|option| format!("{}  —  {}", option.selection_spec, option.detail))
                .collect();
            labels.push("Enter provider/model manually".to_string());
            let idx = prompt_select(
                "Select dedicated vision model",
                &labels
                    .iter()
                    .map(|label| label.as_str())
                    .collect::<Vec<_>>(),
                0,
            )
            .map_err(|e| anyhow::anyhow!("vision model selection: {e}"))?;

            let selected = if idx < options.len() {
                options[idx].selection_spec.clone()
            } else {
                let current = match (
                    config.vision_provider.as_deref(),
                    config.vision_model.as_deref(),
                ) {
                    (Some(provider), Some(model)) => format!("{provider}/{model}"),
                    _ => String::new(),
                };
                let prompt = if current.is_empty() {
                    "  Vision model [provider/model]: ".to_string()
                } else {
                    format!("  Vision model [{current}]: ")
                };
                let input = prompt_line(&prompt)?;
                let manual = if input.trim().is_empty() && !current.is_empty() {
                    current
                } else {
                    input.trim().to_string()
                };
                if manual.is_empty() {
                    anyhow::bail!("vision model cannot be empty");
                }
                manual
            };

            let (provider, model) = parse_selection_spec(&selected)
                .ok_or_else(|| anyhow::anyhow!("invalid vision model '{selected}'"))?;
            config.vision_provider = Some(canonical_provider(&provider));
            config.vision_model = Some(model);
            config.vision_base_url = None;
            config.vision_api_key_env = None;
            println!("  ✓ Dedicated vision model configured: {selected}");
        }
        2 => {
            let base_url = prompt_line("  Base URL [https://api.openai.com/v1]: ")?;
            let base_url = if base_url.trim().is_empty() {
                "https://api.openai.com/v1".to_string()
            } else {
                sanitize_input(base_url.trim().to_string())
            };
            let provider = prompt_line("  Provider label [openai]: ")?;
            let provider = if provider.trim().is_empty() {
                "openai".to_string()
            } else {
                sanitize_input(provider.trim().to_string())
            };
            let model = prompt_line("  Vision model [gpt-4o]: ")?;
            let model = if model.trim().is_empty() {
                "gpt-4o".to_string()
            } else {
                sanitize_input(model.trim().to_string())
            };
            let api_key_env = prompt_line("  API key env var [OPENAI_API_KEY]: ")?;
            let api_key_env = if api_key_env.trim().is_empty() {
                Some("OPENAI_API_KEY".to_string())
            } else {
                Some(sanitize_input(api_key_env.trim().to_string()))
            };
            config.vision_provider = Some(canonical_provider(&provider));
            config.vision_model = Some(model);
            config.vision_base_url = Some(base_url);
            config.vision_api_key_env = api_key_env;
            println!("  ✓ Custom vision endpoint configured.");
        }
        _ => unreachable!("dialoguer select returned out-of-range index"),
    }

    Ok(())
}

/// Setup the toolsets section.
fn setup_tools_section(config: &mut CurrentConfig) -> anyhow::Result<()> {
    let available = [
        "core", "web", "terminal", "memory", "skills", "browser", "voice",
    ];
    println!("  Available toolsets: {}", available.join(", "));
    println!("  Currently enabled: {}", config.toolsets.join(", "));
    println!();

    let change = prompt_yes_no("Reconfigure enabled toolsets?", true)?;
    if change {
        let mut enabled = Vec::new();
        for ts in &available {
            let is_on = config.toolsets.iter().any(|t| t == ts);
            let on = prompt_yes_no(&format!("  Enable {ts}?"), is_on)?;
            if on {
                enabled.push(ts.to_string());
            }
        }
        config.toolsets = enabled;
    }

    Ok(())
}

/// Setup the gateway / messaging platforms section.
///
/// WHY delegates to gateway_setup: the gateway wizard already has rich
/// per-platform flows (token prompts, allowed users, home channels, Signal
/// Docker helpers, WhatsApp QR pairing). Calling it from here eliminates
/// duplication (DRY) and keeps both code-paths automatically in sync.
///
/// WHY full catalog: the old hardcoded 7-platform list missed email, sms,
/// matrix, mattermost, dingtalk, homeassistant, webhook, and api_server.
/// We now use `gateway_catalog::collect_platform_diagnostics` so the wizard
/// always reflects the actual set of supported adapters.
fn setup_gateway_section(config: &mut CurrentConfig, config_path: &Path) -> anyhow::Result<()> {
    // Load AppConfig for rich status display from the full platform catalog.
    let app_config = if config_path.exists() {
        edgecrab_core::AppConfig::load_from(config_path).unwrap_or_default()
    } else {
        edgecrab_core::AppConfig::default()
    };

    // Show all platforms using the full catalog (not a hardcoded subset).
    let diagnostics = crate::gateway_catalog::collect_platform_diagnostics(&app_config);
    println!("  Platform Status:");
    println!("  ────────────────");
    for d in &diagnostics {
        println!("    {:<10} {:<20} {}", d.name, d.state.label(), d.detail);
    }
    println!();
    println!(
        "  Gateway bind: {}:{}",
        app_config.gateway.host, app_config.gateway.port
    );
    println!();

    let action = prompt_select(
        "What do you want to do?",
        &[
            "Configure platforms in detail  (recommended — full per-platform wizard)",
            "Quick enable/disable platforms (toggle on/off, no credential prompts)",
            "Update gateway host/port only",
            "Done — skip gateway setup",
        ],
        0,
    )?;

    match action {
        0 => {
            // Launch the full gateway configure wizard (DRY — reuses gateway_setup).
            // configure_for_path saves the config itself; we reload raw afterwards
            // so that save_current_config does not clobber the detailed platform
            // configs written by the gateway wizard.
            crate::gateway_setup::configure_for_path(config_path, None)?;
            let reloaded = load_current_config(config_path);
            config.gateway_platforms = reloaded.gateway_platforms;
            config.gateway_host = reloaded.gateway_host;
            config.gateway_port = reloaded.gateway_port;
            // IMPORTANT: preserve the detailed gateway sub-keys (telegram.enabled, etc.)
            config.raw = reloaded.raw;
        }
        1 => {
            // Quick toggle using the full catalog — no more hardcoded 7-platform list.
            let mut enabled = Vec::new();
            for d in &diagnostics {
                if d.id == "api_server" || d.id == "webhook" {
                    // env-only adapters: skip yes/no toggle — gateway configure handles them
                    continue;
                }
                let is_on = config.gateway_platforms.iter().any(|p| p == d.id);
                let on = prompt_yes_no(&format!("  Enable {}?", d.name), is_on)?;
                if on {
                    enabled.push(d.id.to_string());
                }
            }
            config.gateway_platforms = enabled;
            println!();
            println!("  For tokens and detailed settings, run:");
            println!("    edgecrab gateway configure");
        }
        2 => {
            let new_host = prompt_line(&format!("  Gateway host [{}]", config.gateway_host))?;
            let trimmed = new_host.trim();
            if !trimmed.is_empty() {
                config.gateway_host = trimmed.to_string();
            }
            let new_port = prompt_line(&format!("  Gateway port [{}]", config.gateway_port))?;
            if let Ok(p) = new_port.trim().parse::<u16>() {
                if p > 0 {
                    config.gateway_port = p;
                }
            }
        }
        _ => {
            println!("  ✓ Gateway setup skipped. Run `edgecrab gateway configure` later.");
        }
    }

    Ok(())
}

/// Setup the agent settings section.
fn setup_agent_section(config: &mut CurrentConfig) -> anyhow::Result<()> {
    println!("  streaming:       {}", config.streaming);
    println!("  save_trajectories: {}", config.save_trajectories);
    println!("  skip_memory:     {}", config.skip_memory);
    println!("  skip_context:    {}", config.skip_context);
    println!();

    let streaming = prompt_yes_no("Enable streaming?", config.streaming)?;
    config.streaming = streaming;

    let trajectories = prompt_yes_no("Save trajectories?", config.save_trajectories)?;
    config.save_trajectories = trajectories;

    let memory = prompt_yes_no("Enable memory?", !config.skip_memory)?;
    config.skip_memory = !memory;

    let context = prompt_yes_no("Load context files?", !config.skip_context)?;
    config.skip_context = !context;

    Ok(())
}

// ─── Config Load/Save Helpers ─────────────────────────────────────────

/// Simplified view of the config for the setup wizard.
///
/// WHY not AppConfig directly: setup.rs lives in edgecrab-cli and we
/// want a lightweight YAML round-trip that preserves unknown keys.
struct CurrentConfig {
    model: Option<String>,
    provider: Option<String>,
    max_iterations: u32,
    streaming: bool,
    toolsets: Vec<String>,
    gateway_platforms: Vec<String>,
    gateway_host: String,
    gateway_port: u16,
    save_trajectories: bool,
    skip_memory: bool,
    skip_context: bool,
    vision_provider: Option<String>,
    vision_model: Option<String>,
    vision_base_url: Option<String>,
    vision_api_key_env: Option<String>,
    /// Raw YAML for round-tripping fields we don't edit.
    raw: serde_yml::Value,
}

fn load_current_config(config_path: &Path) -> CurrentConfig {
    let raw: serde_yml::Value = if config_path.exists() {
        let content = std::fs::read_to_string(config_path).unwrap_or_default();
        serde_yml::from_str(&content).unwrap_or(serde_yml::Value::Mapping(Default::default()))
    } else {
        serde_yml::Value::Mapping(Default::default())
    };

    let model = raw
        .get("model")
        .and_then(|m| m.get("default_model").or_else(|| m.get("default")))
        .and_then(|v| v.as_str())
        .map(String::from);

    let provider = raw
        .get("provider")
        .and_then(|v| v.as_str())
        .map(String::from);

    let max_iterations = raw
        .get("model")
        .and_then(|m| m.get("max_iterations"))
        .and_then(|v| v.as_u64())
        .unwrap_or(90) as u32;

    let streaming = raw
        .get("model")
        .and_then(|m| m.get("streaming"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let toolsets = raw
        .get("tools")
        .and_then(|t| t.get("enabled_toolsets"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| {
            vec![
                "core".into(),
                "web".into(),
                "terminal".into(),
                "memory".into(),
                "skills".into(),
            ]
        });

    let gateway_platforms = raw
        .get("gateway")
        .and_then(|g| g.get("enabled_platforms"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let gateway_host = raw
        .get("gateway")
        .and_then(|g| g.get("host"))
        .and_then(|v| v.as_str())
        .unwrap_or("127.0.0.1")
        .to_string();

    let gateway_port = raw
        .get("gateway")
        .and_then(|g| g.get("port"))
        .and_then(|v| v.as_u64())
        .unwrap_or(8080) as u16;

    let save_trajectories = raw
        .get("agent")
        .and_then(|a| a.get("save_trajectories"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let skip_memory = raw
        .get("agent")
        .and_then(|a| a.get("skip_memory"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let skip_context = raw
        .get("agent")
        .and_then(|a| a.get("skip_context_files"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let vision_provider = raw
        .get("auxiliary")
        .and_then(|aux| aux.get("provider"))
        .and_then(|v| v.as_str())
        .map(canonical_provider);

    let vision_model = raw
        .get("auxiliary")
        .and_then(|aux| aux.get("model"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let vision_base_url = raw
        .get("auxiliary")
        .and_then(|aux| aux.get("base_url"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let vision_api_key_env = raw
        .get("auxiliary")
        .and_then(|aux| aux.get("api_key_env"))
        .and_then(|v| v.as_str())
        .map(String::from);

    CurrentConfig {
        model,
        provider,
        max_iterations,
        streaming,
        toolsets,
        gateway_platforms,
        gateway_host,
        gateway_port,
        save_trajectories,
        skip_memory,
        skip_context,
        vision_provider,
        vision_model,
        vision_base_url,
        vision_api_key_env,
        raw,
    }
}

fn save_current_config(
    home: &Path,
    config_path: &Path,
    config: &CurrentConfig,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(home)?;
    std::fs::create_dir_all(home.join("memories"))?;
    std::fs::create_dir_all(home.join("skills"))?;

    // Build YAML from current values, preserving any unknown keys
    let mut root = match &config.raw {
        serde_yml::Value::Mapping(m) => m.clone(),
        _ => serde_yml::Mapping::new(),
    };

    // Model section
    let mut model_map = root
        .get(serde_yml::Value::String("model".into()))
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default();

    if let Some(ref m) = config.model {
        model_map.insert(
            serde_yml::Value::String("default_model".into()),
            serde_yml::Value::String(m.clone()),
        );
    }
    model_map.insert(
        serde_yml::Value::String("max_iterations".into()),
        serde_yml::Value::Number(config.max_iterations.into()),
    );
    model_map.insert(
        serde_yml::Value::String("streaming".into()),
        serde_yml::Value::Bool(config.streaming),
    );
    root.insert(
        serde_yml::Value::String("model".into()),
        serde_yml::Value::Mapping(model_map),
    );

    // Provider
    if let Some(ref p) = config.provider {
        root.insert(
            serde_yml::Value::String("provider".into()),
            serde_yml::Value::String(p.clone()),
        );
    }

    // Tools section
    let mut tools_map = root
        .get(serde_yml::Value::String("tools".into()))
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default();
    tools_map.insert(
        serde_yml::Value::String("enabled_toolsets".into()),
        serde_yml::Value::Sequence(
            config
                .toolsets
                .iter()
                .map(|t| serde_yml::Value::String(t.clone()))
                .collect(),
        ),
    );
    root.insert(
        serde_yml::Value::String("tools".into()),
        serde_yml::Value::Mapping(tools_map),
    );

    // Gateway section
    let mut gw_map = root
        .get(serde_yml::Value::String("gateway".into()))
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default();
    gw_map.insert(
        serde_yml::Value::String("enabled_platforms".into()),
        serde_yml::Value::Sequence(
            config
                .gateway_platforms
                .iter()
                .map(|p| serde_yml::Value::String(p.clone()))
                .collect(),
        ),
    );
    gw_map.insert(
        serde_yml::Value::String("host".into()),
        serde_yml::Value::String(config.gateway_host.clone()),
    );
    gw_map.insert(
        serde_yml::Value::String("port".into()),
        serde_yml::Value::Number(config.gateway_port.into()),
    );
    root.insert(
        serde_yml::Value::String("gateway".into()),
        serde_yml::Value::Mapping(gw_map),
    );

    // Agent section
    let mut agent_map = root
        .get(serde_yml::Value::String("agent".into()))
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default();
    agent_map.insert(
        serde_yml::Value::String("save_trajectories".into()),
        serde_yml::Value::Bool(config.save_trajectories),
    );
    agent_map.insert(
        serde_yml::Value::String("skip_memory".into()),
        serde_yml::Value::Bool(config.skip_memory),
    );
    agent_map.insert(
        serde_yml::Value::String("skip_context_files".into()),
        serde_yml::Value::Bool(config.skip_context),
    );
    root.insert(
        serde_yml::Value::String("agent".into()),
        serde_yml::Value::Mapping(agent_map),
    );

    let has_auxiliary = config
        .vision_provider
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty())
        || config
            .vision_model
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty())
        || config
            .vision_base_url
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty())
        || config
            .vision_api_key_env
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty());

    if has_auxiliary {
        let mut auxiliary_map = root
            .get(serde_yml::Value::String("auxiliary".into()))
            .and_then(|v| v.as_mapping())
            .cloned()
            .unwrap_or_default();

        if let Some(ref provider) = config.vision_provider {
            auxiliary_map.insert(
                serde_yml::Value::String("provider".into()),
                serde_yml::Value::String(provider.clone()),
            );
        }
        if let Some(ref model) = config.vision_model {
            auxiliary_map.insert(
                serde_yml::Value::String("model".into()),
                serde_yml::Value::String(model.clone()),
            );
        }
        if let Some(ref base_url) = config.vision_base_url {
            auxiliary_map.insert(
                serde_yml::Value::String("base_url".into()),
                serde_yml::Value::String(base_url.clone()),
            );
        }
        if let Some(ref api_key_env) = config.vision_api_key_env {
            auxiliary_map.insert(
                serde_yml::Value::String("api_key_env".into()),
                serde_yml::Value::String(api_key_env.clone()),
            );
        }
        root.insert(
            serde_yml::Value::String("auxiliary".into()),
            serde_yml::Value::Mapping(auxiliary_map),
        );
    } else {
        let auxiliary_key = serde_yml::Value::String("auxiliary".into());
        root.remove(&auxiliary_key);
    }

    let yaml = serde_yml::to_string(&serde_yml::Value::Mapping(root))
        .map_err(|e| anyhow::anyhow!("failed to serialize config: {e}"))?;

    // Prepend a helpful header comment
    let header = "# EdgeCrab configuration\n\
                  # Edit this file to customize your setup.\n\
                  # Run `edgecrab doctor` to validate.\n\n";
    std::fs::write(config_path, format!("{header}{yaml}"))?;

    Ok(())
}

// ─── Interactive Helpers ──────────────────────────────────────────────

/// Interactive provider selection using dialoguer Select (parity with gateway_setup).
///
/// WHY Select: Arrow-key navigation is significantly faster for 11+ options
/// than typing an index number. Detected keys are starred so the user can see
/// what's already set without revealing secrets.
fn choose_provider_interactively() -> anyhow::Result<String> {
    // Build display items: mark detected keys with a star
    let mut items: Vec<String> = PROVIDER_ENV_MAP
        .iter()
        .map(|(env, _id, name)| {
            if std::env::var(env).is_ok() {
                format!("{name}  [key detected]")
            } else {
                name.to_string()
            }
        })
        .collect();
    for (_id, name) in LOCAL_PROVIDERS {
        items.push(format!("{name}  [no key needed]"));
    }

    println!();
    println!("  Nous Hermes 3 (NousResearch) is available via OpenRouter.");
    println!("  Use openrouter.ai to browse all Nous Research model variants.");
    println!();

    let choice = prompt_select(
        "Choose your LLM provider",
        &items.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        0,
    )
    .map_err(|e| anyhow::anyhow!("provider selection: {e}"))?;

    if choice < PROVIDER_ENV_MAP.len() {
        let (env_var, provider_id, name) = PROVIDER_ENV_MAP[choice];
        println!("  Selected: {name}");

        if std::env::var(env_var).is_ok() {
            println!("  ✓ ${env_var} already set in environment");
        } else {
            // Per-provider API key format hints
            let hint: &str = match env_var {
                "OPENAI_API_KEY" => "starts with sk-proj-... or sk-...",
                "ANTHROPIC_API_KEY" => "starts with sk-ant-...",
                "GOOGLE_API_KEY" => "starts with AIza...",
                "OPENROUTER_API_KEY" => "starts with sk-or-v1-...  (get one free at openrouter.ai)",
                "GITHUB_TOKEN" => "starts with ghp_ or gho_",
                "XAI_API_KEY" => "get one at console.x.ai",
                _ => "",
            };
            if !hint.is_empty() {
                println!("  Hint: {hint}");
            }
            let key = prompt_secret(&format!("  {name} API key"))?;
            if !key.trim().is_empty() {
                save_env_key(env_var, key.trim())?;
            }
        }
        Ok(provider_id.to_string())
    } else {
        let local_idx = choice - PROVIDER_ENV_MAP.len();
        let (provider_id, name) = LOCAL_PROVIDERS[local_idx];
        println!("  Selected: {name}");
        println!("  Make sure the service is running locally.");
        Ok(provider_id.to_string())
    }
}

/// Save an API key to the ~/.edgecrab/.env file.
fn save_env_key(env_var: &str, key: &str) -> anyhow::Result<()> {
    let env_path = edgecrab_home().join(".env");
    std::fs::create_dir_all(edgecrab_home())?;
    let entry = format!("{env_var}={key}\n");
    if env_path.exists() {
        use std::io::Read;
        let mut existing = String::new();
        std::fs::File::open(&env_path)?.read_to_string(&mut existing)?;
        if existing.contains(&format!("{env_var}=")) {
            // Replace existing key
            let updated: String = existing
                .lines()
                .map(|line| {
                    if line.starts_with(&format!("{env_var}=")) {
                        format!("{env_var}={key}")
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
    println!("  ✓ Key saved to: {}", env_path.display());
    Ok(())
}

/// Prompt for a yes/no answer using dialoguer Confirm (parity with gateway_setup).
fn prompt_yes_no(question: &str, default: bool) -> io::Result<bool> {
    Confirm::with_theme(&wizard_theme())
        .with_prompt(question)
        .default(default)
        .wait_for_newline(true)
        .interact()
        .map_err(dialoguer_to_io)
}

/// Read a line from stdin using dialoguer Input (styled, parity with gateway_setup).
fn prompt_line(prompt_str: &str) -> io::Result<String> {
    // Strip common trailing patterns that were used for the old raw-stdin style
    let clean = prompt_str
        .trim_end_matches(':')
        .trim_end_matches(']')
        .trim_end_matches('[')
        .trim();
    Input::<String>::with_theme(&wizard_theme())
        .with_prompt(clean)
        .allow_empty(true)
        .interact_text()
        .map_err(dialoguer_to_io)
        .map(sanitize_input)
}

/// Read a secret (API key / token) using dialoguer Password — input is masked.
/// SECURITY: The old implementation used read_line() which exposed keys in
/// cleartext in the terminal. This version masks every character.
fn prompt_secret(prompt_str: &str) -> io::Result<String> {
    let clean = prompt_str
        .trim_end_matches(':')
        .trim_end_matches(" (hidden): ")
        .trim_end_matches(" (hidden)")
        .trim();
    Password::with_theme(&wizard_theme())
        .with_prompt(clean)
        .allow_empty_password(true)
        .interact()
        .map_err(dialoguer_to_io)
}

/// Write the initial config.yaml file (fresh setup only).
#[cfg(test)]
fn write_config(
    home: &Path,
    config_path: &Path,
    provider: &str,
    model: &str,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(home)?;
    std::fs::create_dir_all(home.join("memories"))?;
    std::fs::create_dir_all(home.join("skills"))?;

    let yaml = format!(
        r#"# EdgeCrab configuration
# Edit this file to customize your setup.
# Run `edgecrab doctor` to validate.

model:
  default_model: "{model}"
  max_iterations: 90
  streaming: true

provider: "{provider}"

tools:
  # Toolset names map to the toolset() label each tool declares.
  # Available literal names: file, terminal, web, browser, memory, skills,
  #   core, meta, scheduling, delegation, code_execution, session, mcp,
  #   media, messaging, moa
  # Aliases:  core (→ file+meta+scheduling+delegation+code_execution+session+mcp+messaging+media+browser+moa)
  #           coding (→ file+terminal+search+code_execution)
  #           research (→ web+browser+vision)
  #           all  (→ every registered tool, no filter)
  enabled_toolsets:
    - core       # expands to: file, meta, scheduling, delegation, code_execution, session, mcp, messaging, media, browser, moa
    - web        # web_search, web_extract, web_crawl
    - terminal   # terminal, run_process, list_processes, kill_process
    - memory     # memory_read, memory_write, honcho_*
    - skills     # skills_list, skill_view, skill_manage

gateway:
  host: "127.0.0.1"
  port: 8080
  enabled_platforms: []

agent:
  platform: cli
  skip_context_files: false
  skip_memory: false
  save_trajectories: false
"#
    );

    std::fs::write(config_path, yaml)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_config_creates_file() {
        let tmp = TempDir::new().expect("tmp dir");
        let config_path = tmp.path().join("config.yaml");
        write_config(tmp.path(), &config_path, "copilot", "copilot/gpt-4.1-mini")
            .expect("write config");

        let content = std::fs::read_to_string(&config_path).expect("read config");
        assert!(content.contains("copilot/gpt-4.1-mini"));
        assert!(content.contains("default_model"));
        assert!(tmp.path().join("memories").exists());
        assert!(tmp.path().join("skills").exists());
    }

    #[test]
    fn default_model_coverage() {
        // Values come from the embedded ModelCatalog YAML
        assert_eq!(default_model("copilot"), "copilot/gpt-5.4");
        assert_eq!(default_model("openai"), "openai/gpt-5.4");
        assert_eq!(default_model("anthropic"), "anthropic/claude-opus-4.6");
        assert_eq!(default_model("google"), "google/gemini-2.5-flash");
        assert_eq!(default_model("ollama"), "ollama/gemma4:latest");
        // Unknown providers fall back to the global local default
        assert_eq!(default_model("unknown"), "ollama/gemma4:latest");
    }

    #[test]
    fn load_current_config_from_file() {
        let tmp = TempDir::new().expect("tmp dir");
        let config_path = tmp.path().join("config.yaml");
        write_config(tmp.path(), &config_path, "openai", "openai/gpt-4.1-mini")
            .expect("write config");

        let current = load_current_config(&config_path);
        assert_eq!(current.model.as_deref(), Some("openai/gpt-4.1-mini"));
        assert_eq!(current.provider.as_deref(), Some("openai"));
        assert_eq!(current.max_iterations, 90);
        assert!(current.streaming);
        assert!(current.toolsets.contains(&"core".to_string()));
        assert!(!current.save_trajectories);
    }

    #[test]
    fn load_current_config_missing_file() {
        let config = load_current_config(Path::new("/nonexistent/config.yaml"));
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert_eq!(config.max_iterations, 90);
    }

    #[test]
    fn save_and_reload_round_trip() {
        let tmp = TempDir::new().expect("tmp dir");
        let config_path = tmp.path().join("config.yaml");

        let config = CurrentConfig {
            model: Some("anthropic/claude-sonnet-4-20250514".into()),
            provider: Some("anthropic".into()),
            max_iterations: 50,
            streaming: false,
            toolsets: vec!["core".into(), "web".into()],
            gateway_platforms: vec!["telegram".into(), "slack".into()],
            gateway_host: "0.0.0.0".into(),
            gateway_port: 9090,
            save_trajectories: true,
            skip_memory: true,
            skip_context: false,
            vision_provider: Some("openai".into()),
            vision_model: Some("gpt-4o".into()),
            vision_base_url: None,
            vision_api_key_env: None,
            raw: serde_yml::Value::Mapping(Default::default()),
        };

        save_current_config(tmp.path(), &config_path, &config).expect("save");

        let reloaded = load_current_config(&config_path);
        assert_eq!(
            reloaded.model.as_deref(),
            Some("anthropic/claude-sonnet-4-20250514")
        );
        assert_eq!(reloaded.provider.as_deref(), Some("anthropic"));
        assert_eq!(reloaded.max_iterations, 50);
        assert!(!reloaded.streaming);
        assert_eq!(reloaded.toolsets, vec!["core", "web"]);
        assert_eq!(reloaded.gateway_platforms, vec!["telegram", "slack"]);
        assert_eq!(reloaded.vision_provider.as_deref(), Some("openai"));
        assert_eq!(reloaded.vision_model.as_deref(), Some("gpt-4o"));
        assert_eq!(reloaded.gateway_host, "0.0.0.0");
        assert_eq!(reloaded.gateway_port, 9090);
        assert!(reloaded.save_trajectories);
        assert!(reloaded.skip_memory);
    }

    #[test]
    fn load_current_config_normalizes_vision_provider_aliases() {
        let tmp = TempDir::new().expect("tmp dir");
        let config_path = tmp.path().join("config.yaml");
        std::fs::write(
            &config_path,
            r#"
provider: openai
model:
  default_model: openai/gpt-4.1-mini
auxiliary:
  provider: copilot
  model: gpt-5.4
"#,
        )
        .expect("write config");

        let reloaded = load_current_config(&config_path);
        assert_eq!(reloaded.vision_provider.as_deref(), Some("vscode-copilot"));
        assert_eq!(reloaded.vision_model.as_deref(), Some("gpt-5.4"));
    }

    #[test]
    fn save_preserves_unknown_keys() {
        let tmp = TempDir::new().expect("tmp dir");
        let config_path = tmp.path().join("config.yaml");

        // Write initial config with an extra key
        let yaml = "model:\n  default_model: \"test\"\ncustom_key: preserved\n";
        std::fs::write(&config_path, yaml).unwrap();

        let mut config = load_current_config(&config_path);
        config.model = Some("updated-model".into());

        save_current_config(tmp.path(), &config_path, &config).expect("save");

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("updated-model"), "model should be updated");
        assert!(
            content.contains("custom_key"),
            "unknown key should be preserved"
        );
        assert!(
            content.contains("preserved"),
            "unknown value should be preserved"
        );
    }

    #[test]
    fn setup_sections_are_valid() {
        for (key, label) in SETUP_SECTIONS {
            assert!(!key.is_empty());
            assert!(!label.is_empty());
        }
    }
}
