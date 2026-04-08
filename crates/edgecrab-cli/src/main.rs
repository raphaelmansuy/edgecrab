//! # edgecrab – AI-native terminal agent
//!
//! Binary entry-point. Routes to subcommands (setup, doctor, migrate, acp)
//! or runs the interactive TUI / quiet mode when no subcommand is given.
//!
//! ```text
//! edgecrab [OPTIONS] [PROMPT]  ← interactive TUI (default)
//! edgecrab setup               ← first-run wizard
//! edgecrab doctor              ← diagnostics
//! edgecrab migrate [--dry-run] ← hermes → edgecrab
//! edgecrab acp                 ← ACP stdio server for editors
//! edgecrab version             ← detailed version info
//! ```

mod acp_setup;
mod app;
mod banner;
mod cli_args;
mod commands;
mod cron_cmd;
mod doctor;
mod edit_diff;
mod fuzzy_selector;
mod gateway_catalog;
mod gateway_cmd;
mod gateway_setup;
mod image_models;
mod markdown_render;
mod mcp_catalog;
mod mcp_oauth;
mod mcp_support;
#[cfg(target_os = "macos")]
mod permissions;
mod plugins;
mod plugins_cmd;
mod profile;
mod runtime;
mod setup;
mod skin_engine;
mod status_cmd;
mod theme;
mod tool_display;
mod vision_models;
mod whatsapp_cmd;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use shell_words::split as shell_split;
use tokio_util::sync::CancellationToken;

use app::App;
use cli_args::{
    AcpCommand, CliArgs, Command, ConfigCommand, CronCommand, GatewayCommand, McpCommand,
    PluginsCommand, ProfileCommand, SessionCommand, SkillsCommand, ToolsCommand,
};
use edgecrab_core::config::McpServerConfig;
use edgecrab_state::SessionDb;
use edgecrab_tools::vision_models::normalize_provider_name;
use edgecrab_tools::{ToolRegistry, resolve_alias};
use runtime::{
    build_agent, build_tool_registry, build_tool_registry_with_mcp_discovery, default_export_path,
    load_runtime, open_state_db, render_markdown_export,
};

/// Create the LLM provider from the model string (or env defaults).
///
/// WHY try real provider first: In production the user has API keys set.
/// Falls back to MockProvider for development/test so the CLI always starts.
///
/// ```text
///   model contains "/"  → parse "provider/model", create explicitly
///       ↓ no slash
///   ProviderFactory::from_env()         ← try env-based auto-detect
///       ↓ fails
///   MockProvider                        ← fallback for dev/test
/// ```
pub(crate) fn create_provider(model: &str) -> Arc<dyn edgequake_llm::LLMProvider> {
    // If model string contains provider/model, honour it explicitly first.
    // This ensures "copilot/gpt-4.1-mini" always uses copilot even when
    // OPENAI_API_KEY, ANTHROPIC_API_KEY etc. are also set.
    if let Some((provider_name, model_name)) = model.split_once('/') {
        let canonical = normalize_provider_name(provider_name);
        tracing::info!(
            provider = canonical,
            model = model_name,
            "creating provider from model string"
        );

        // Special-case vscode-copilot: create_llm_provider always forces proxy mode
        // (localhost:4141). Instead, build the provider directly so it uses the default
        // direct mode (api.githubcopilot.com) and respects VSCODE_COPILOT_DIRECT env var.
        if canonical == "vscode-copilot" {
            match edgequake_llm::VsCodeCopilotProvider::new()
                .model(model_name)
                .with_vision(true) // Enable vision so copilot-vision-request header is sent
                .build()
            {
                Ok(provider) => return Arc::new(provider),
                Err(e) => {
                    tracing::warn!(error = %e, model = model_name, "copilot direct mode failed, trying env auto-detect");
                }
            }
        } else if canonical == "vertexai" {
            // ── Hard VertexAI route ──────────────────────────────────────────────────
            // The user explicitly said "vertexai/<model>".  We MUST NOT silently fall
            // through to from_env() (which would pick up GEMINI_API_KEY and route to
            // Google AI Studio instead) or to MockProvider.  Any failure is surfaced
            // immediately with actionable guidance.
            //
            // Why "vertexai:" prefix: edgequake-llm's factory only calls
            // GeminiProvider::from_env_vertex_ai() when the model string starts with
            // "vertexai:".  split_once('/') strips that context, so we restore it here.
            //
            // Why auto-detect GOOGLE_CLOUD_PROJECT: `gcloud auth login` does NOT export
            // it; from_env_vertex_ai() treats it as required.

            if std::env::var("GOOGLE_CLOUD_PROJECT").is_err() {
                match std::process::Command::new("gcloud")
                    .args(["config", "get-value", "project"])
                    .output()
                {
                    Ok(output) if output.status.success() => {
                        let raw = String::from_utf8_lossy(&output.stdout);
                        let project = raw.trim();
                        if !project.is_empty() && project != "(unset)" {
                            // SAFETY: called before the tokio worker pool is spawned;
                            // no other thread reads env vars at this point.
                            unsafe { std::env::set_var("GOOGLE_CLOUD_PROJECT", project) };
                            tracing::info!(
                                project,
                                "auto-detected GOOGLE_CLOUD_PROJECT from gcloud config"
                            );
                        } else {
                            eprintln!(
                                "error: VertexAI requires a GCP project but gcloud returned \
                                 empty/unset.\n  Fix: gcloud config set project <your-project-id>\n\
                                        or: export GOOGLE_CLOUD_PROJECT=<your-project-id>"
                            );
                            std::process::exit(1);
                        }
                    }
                    Ok(_) => {
                        eprintln!(
                            "error: gcloud exited with a non-zero status while detecting \
                             GOOGLE_CLOUD_PROJECT.\n  Fix: export GOOGLE_CLOUD_PROJECT=<your-project-id>"
                        );
                        std::process::exit(1);
                    }
                    Err(_) => {
                        eprintln!(
                            "error: GOOGLE_CLOUD_PROJECT is not set and gcloud was not found \
                             in PATH.\n  Fix: export GOOGLE_CLOUD_PROJECT=<your-project-id>\n\
                                    or: install the Google Cloud SDK and run gcloud auth login"
                        );
                        std::process::exit(1);
                    }
                }
            }

            let vertex_model = format!("vertexai:{model_name}");

            // Gemini 3.x Preview models (gemini-3-flash-preview, gemini-3.1-pro-preview,
            // gemini-3.1-flash-lite-preview, …) are ONLY available on the Vertex AI
            // global endpoint.  The 2.x GA models work on regional endpoints like
            // us-central1.  edgequake-llm reads GOOGLE_CLOUD_REGION to build the
            // endpoint URL; auto-set it to "global" when the user hasn't set it
            // and the model name indicates a Gemini 3 generation.
            // Source: https://cloud.google.com/vertex-ai/generative-ai/docs/learn/locations
            if model_name.starts_with("gemini-3") && std::env::var("GOOGLE_CLOUD_REGION").is_err() {
                // SAFETY: single-threaded startup, no concurrent env reads.
                unsafe { std::env::set_var("GOOGLE_CLOUD_REGION", "global") };
                tracing::info!(
                    model = model_name,
                    "auto-set GOOGLE_CLOUD_REGION=global (Gemini 3.x is global-endpoint-only)"
                );
            }

            match edgequake_llm::ProviderFactory::create_llm_provider(&canonical, &vertex_model) {
                Ok(provider) => return provider,
                Err(e) => {
                    eprintln!(
                        "error: VertexAI provider failed for model '{model_name}': {e}\n\
                         Fix:\n\
                         \x20  • gcloud auth application-default login\n\
                         \x20  • export GOOGLE_CLOUD_PROJECT=<your-project-id>\n\
                         \x20  • edgecrab doctor    ← full diagnostics"
                    );
                    std::process::exit(1);
                }
            }
        } else if let Ok(provider) =
            edgequake_llm::ProviderFactory::create_llm_provider(&canonical, model_name)
        {
            return provider;
        } else {
            tracing::warn!(
                provider = canonical,
                model = model_name,
                "explicit provider failed, trying env auto-detect"
            );
        }
    }

    // Fallback: environment auto-detection (only reached when no "provider/model" slash
    // syntax was used, or a non-vertexai explicit provider soft-failed).
    if let Ok((llm, _embedding)) = edgequake_llm::ProviderFactory::from_env() {
        return llm;
    }

    tracing::warn!("no provider configured, falling back to mock");
    Arc::new(edgequake_llm::MockProvider::new())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = CliArgs::parse();
    let subcommand = args.command.clone();

    // Initialize tracing
    if args.debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .init();
    }

    let manages_profiles = matches!(
        subcommand,
        Some(Command::Profile { .. }) | Some(Command::Completion { .. })
    );
    if args.profile.is_some() && args.config.is_some() && !manages_profiles {
        anyhow::bail!("--profile and --config cannot be combined on runtime commands");
    }
    if args.config.is_none() && !manages_profiles {
        profile::activate_profile(args.profile.as_deref())?;
    } else if !manages_profiles {
        activate_runtime_home_from_config(args.config.as_deref())?;
    }

    // Route to subcommand if one was given
    if let Some(cmd) = subcommand {
        return run_subcommand(cmd, &args).await;
    }

    // ── Git worktree isolation (-w flag) ─────────────────────────────
    // When -w is set, create a disposable worktree under .worktrees/ in the
    // current repo root and cd into it. This mirrors `hermes -w`.
    if args.worktree {
        match setup_worktree() {
            Ok(wt_path) => {
                std::env::set_current_dir(&wt_path)
                    .with_context(|| format!("failed to cd into worktree {}", wt_path.display()))?;
                eprintln!("🌿 Running in isolated worktree: {}", wt_path.display());
            }
            Err(e) => {
                eprintln!("⚠  Failed to create worktree ({e}), continuing in current directory.");
            }
        }
    }

    // ── Interactive / quiet mode ──────────────────────────────────────

    let mut runtime = load_runtime(
        args.config.as_deref(),
        args.model.as_deref(),
        args.toolset.as_deref(),
    )?;

    // Wire preloaded skills from -s flags into the runtime config
    if !args.skills.is_empty() {
        runtime.config.skills.preloaded = args.skills.clone();
    }

    let model = runtime.config.model.default_model.clone();
    let provider = create_provider(&model);
    let state_db = open_state_db(&runtime.state_db_path)?;
    let tool_registry = build_tool_registry_with_mcp_discovery(&runtime.config).await;

    // ── Resolve session from --session, --continue, or --resume ────
    let resolved_session = resolve_session_flag(&args, &state_db)?;

    let agent = build_agent(
        &runtime,
        provider,
        state_db,
        tool_registry,
        edgecrab_types::Platform::Cli,
        args.quiet,
        resolved_session.clone(),
    )?;
    gateway_cmd::attach_gateway_sender_if_running(&agent, &runtime).await?;

    if let Some(ref session_id) = resolved_session {
        agent
            .restore_session(session_id)
            .await
            .with_context(|| format!("failed to restore session '{session_id}'"))?;
    }

    // Quiet mode: send prompt, print response, exit
    if args.quiet {
        if let Some(prompt) = args.prompt_text() {
            let response = agent.chat(&prompt).await?;
            println!("{}", response);
        } else {
            eprintln!("edgecrab: no prompt provided in quiet mode. Use -q \"your prompt\"");
            std::process::exit(1);
        }
        let _ = edgecrab_tools::tools::terminal::cleanup_all_backends().await;
        return Ok(());
    }

    // Interactive TUI mode
    let mut app = App::new();
    app.set_agent(Arc::clone(&agent));

    // Show banner
    if !args.no_banner {
        app.push_colorful_banner(&model);
    }

    app.set_model(&model);

    if resolved_session.is_some() {
        app.load_messages(agent.messages().await);
    }

    // Handle initial prompt — dispatch to agent via the streaming channel
    if let Some(prompt) = args.prompt_text() {
        // Simulate user typing the initial prompt into the TUI input.
        // The process_input path handles agent dispatch via the mpsc channel.
        app.dispatch_initial_prompt(prompt);
    }

    // ── Background cron scheduler ─────────────────────────────────────
    // Tick due cron jobs while the TUI is open. Results are sent back to
    // the TUI chat via cron_tui_tx so the user sees output without having
    // to check ~/.edgecrab/cron/output/ manually.
    //
    // Timing: first tick fires after 5 seconds (fast enough for jobs created
    // "just now"), then every 60 seconds thereafter.
    let cron_tui_tx = app.cron_sender();
    let cron_stop = CancellationToken::new();
    let cron_stop_guard = cron_stop.clone();
    let cron_args = args.clone();
    tokio::spawn(async move {
        // Short startup delay — TUI has fully rendered before first check.
        // Using 5 s instead of 60 s so one-shot "fire now" jobs are visible quickly.
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
            _ = cron_stop_guard.cancelled() => return,
        }
        loop {
            match cron_cmd::tick_due_jobs(&cron_args, false, None, Some(cron_tui_tx.clone())).await
            {
                Ok(n) if n > 0 => tracing::info!(jobs = n, "cron: ran due jobs"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "cron background tick failed"),
            }
            // Wait 60 s before next check, but honour cancellation immediately.
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}
                _ = cron_stop_guard.cancelled() => break,
            }
        }
        tracing::debug!("cron background scheduler stopped");
    });

    // Run TUI in a blocking task so the tokio runtime stays alive.
    tokio::task::spawn_blocking(move || app::run_tui(&mut app)).await??;

    // Stop the background cron scheduler when the TUI exits
    cron_stop.cancel();
    let _ = edgecrab_tools::tools::terminal::cleanup_all_backends().await;

    Ok(())
}

/// Dispatch to a named subcommand.
async fn run_subcommand(cmd: Command, args: &CliArgs) -> anyhow::Result<()> {
    match cmd {
        Command::Setup { section, force } => {
            setup::run_with_options(section.as_deref(), force)?;
        }

        Command::Doctor => {
            let all_ok = doctor::run(args.config.as_deref()).await?;
            if !all_ok {
                std::process::exit(1);
            }
        }

        Command::Migrate { dry_run } => {
            run_migrate(dry_run)?;
        }

        Command::Acp { command } => match command {
            Some(AcpCommand::Init { workspace, force }) => {
                acp_setup::run_init(workspace, force)?;
            }
            None => {
                run_acp(args).await?;
            }
        },
        Command::Version => {
            run_version();
        }

        Command::Whatsapp => {
            whatsapp_cmd::run(args)?;
        }

        Command::Status => {
            status_cmd::run(args)?;
        }

        Command::Sessions { command } => {
            run_sessions(command, args)?;
        }

        Command::Config { command } => {
            run_config(command, args)?;
        }

        Command::Tools { command } => {
            run_tools(command, args)?;
        }

        Command::Mcp { command } => {
            run_mcp(command, args).await?;
        }

        Command::Plugins { command } => {
            run_plugins(command)?;
        }

        Command::Cron { command } => {
            run_cron(command, args).await?;
        }

        Command::Gateway { command } => {
            run_gateway(command, args).await?;
        }

        Command::Skills { command } => {
            run_skills(command).await?;
        }

        Command::Profile { command } => {
            run_profile(command)?;
        }

        Command::Completion { shell } => {
            profile::print_completion(&shell)?;
        }
    }
    Ok(())
}

/// Run the Hermes → EdgeCrab migrator.
///
/// WHY separate fn: isolates edgecrab-migrate dependency linkage so
/// the ACP/doctor paths don't transitively pull in migration code.
fn run_migrate(dry_run: bool) -> anyhow::Result<()> {
    use dirs::home_dir;
    use edgecrab_migrate::hermes::HermesMigrator;

    let hermes_home = home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".hermes");
    let edgecrab_home = home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".edgecrab");

    if !hermes_home.exists() {
        println!(
            "ℹ  No hermes-agent config found at: {}",
            hermes_home.display()
        );
        println!("   Nothing to migrate.");
        return Ok(());
    }

    if dry_run {
        println!("🔍 Dry-run mode — no files will be written.\n");
    } else {
        println!("🚀 Migrating hermes-agent → EdgeCrab...\n");
    }

    println!("  Source:      {}", hermes_home.display());
    println!("  Destination: {}\n", edgecrab_home.display());

    // In dry-run mode we use a /tmp directory as destination so no files are
    // actually written to the real edgecrab home.
    let (effective_dest, tmp_path) = if dry_run {
        let tmp = std::env::temp_dir().join(format!(
            "edgecrab-migrate-dry-run-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&tmp)?;
        (tmp.clone(), Some(tmp))
    } else {
        std::fs::create_dir_all(&edgecrab_home)?;
        (edgecrab_home.clone(), None)
    };

    let migrator = HermesMigrator::new(hermes_home, effective_dest);
    let report = migrator.migrate_all()?;

    // Cleanup dry-run temp dir
    if let Some(tmp) = tmp_path {
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // Print report
    for item in &report.items {
        use edgecrab_migrate::report::MigrationStatus;
        let icon = match item.status {
            MigrationStatus::Success => "✓",
            MigrationStatus::Skipped => "⟳",
            MigrationStatus::Failed => "✗",
        };
        println!("  {icon} {:12} — {}", item.name, item.detail);
    }

    let succeeded = report
        .items
        .iter()
        .filter(|i| i.status == edgecrab_migrate::report::MigrationStatus::Success)
        .count();
    let failed = report
        .items
        .iter()
        .filter(|i| i.status == edgecrab_migrate::report::MigrationStatus::Failed)
        .count();

    println!();
    if failed == 0 {
        if dry_run {
            println!("✅ Dry-run complete. {succeeded} item(s) would be migrated.");
            println!("   Run without --dry-run to apply.");
        } else {
            println!("✅ Migration complete. {succeeded} item(s) migrated.");
            println!("   Run `edgecrab doctor` to verify the new configuration.");
        }
    } else {
        println!("⚠  Migration completed with {failed} failure(s). Check output above.");
    }

    Ok(())
}

/// Start the ACP stdio server for editor integration.
async fn run_acp(args: &CliArgs) -> anyhow::Result<()> {
    use edgecrab_acp::server::AcpServer;

    let runtime = load_runtime(
        args.config.as_deref(),
        args.model.as_deref(),
        args.toolset.as_deref(),
    )?;
    let model_str = runtime.config.model.default_model.clone();
    let provider = create_provider(&model_str);
    let state_db = open_state_db(&runtime.state_db_path)?;
    let tool_registry = build_tool_registry_with_mcp_discovery(&runtime.config).await;
    let agent = build_agent(
        &runtime,
        provider,
        state_db,
        tool_registry,
        edgecrab_types::Platform::Acp,
        false,
        None,
    )?;
    gateway_cmd::attach_gateway_sender_if_running(&agent, &runtime).await?;

    let mut server = AcpServer::new();
    server.set_agent(agent);
    server.run().await?;
    Ok(())
}

/// Print detailed version and provider information.
fn run_version() {
    print!("{}", render_version_report());
}

fn run_sessions(command: SessionCommand, args: &CliArgs) -> anyhow::Result<()> {
    let runtime = load_runtime(args.config.as_deref(), args.model.as_deref(), None)?;
    let db = open_state_db(&runtime.state_db_path)?;

    match command {
        SessionCommand::List { limit, source } => {
            let sessions = db.list_sessions_rich(source.as_deref(), limit)?;
            if sessions.is_empty() {
                println!("No persisted sessions.");
                return Ok(());
            }
            print_session_rich_list(&sessions);
        }
        SessionCommand::Browse { query, limit } => {
            if let Some(query) = query {
                let results = db.search_sessions_rich(&query, limit)?;
                if results.is_empty() {
                    println!("No sessions matched '{}'.", query);
                    return Ok(());
                }
                for result in results {
                    println!(
                        "{}  {}  score={:.3}",
                        edgecrab_core::safe_truncate(&result.session.id, 12),
                        result.role,
                        result.score,
                    );
                    println!(
                        "  {}  model={}  msgs={}  last_active={}",
                        result.session.title.as_deref().unwrap_or("—"),
                        result.session.model.as_deref().unwrap_or("?"),
                        result.session.message_count,
                        format_timestamp(result.session.last_active),
                    );
                    println!("  match: {}", result.snippet);
                    if !result.session.preview.is_empty() {
                        println!("  preview: {}", result.session.preview);
                    }
                }
            } else {
                let sessions = db.list_sessions_rich(None, limit)?;
                if sessions.is_empty() {
                    println!("No persisted sessions.");
                    return Ok(());
                }
                print_session_rich_list(&sessions);
                println!(
                    "Hint: use `edgecrab sessions browse --query <text>` to search message history."
                );
            }
        }
        SessionCommand::Export { id, output, format } => {
            let session_id = resolve_session_id(&db, &id)?;
            match format.as_str() {
                "jsonl" => {
                    let export = db
                        .export_session_jsonl(&session_id)?
                        .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
                    let jsonl = serde_json::to_string(&export)
                        .map_err(|e| anyhow::anyhow!("JSON serialization failed: {e}"))?;
                    let out_path = output.map(PathBuf::from).unwrap_or_else(|| {
                        default_export_path(
                            "edgecrab-session",
                            edgecrab_core::safe_truncate(&session_id, 8),
                            "jsonl",
                        )
                    });
                    std::fs::write(&out_path, jsonl)?;
                    println!(
                        "Exported {} to {} (JSONL)",
                        edgecrab_core::safe_truncate(&session_id, 12),
                        out_path.display()
                    );
                }
                "markdown" | "md" => {
                    let record = db
                        .get_session(&session_id)?
                        .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
                    let messages = db.get_messages(&session_id)?;
                    let out_path = output.map(PathBuf::from).unwrap_or_else(|| {
                        default_export_path(
                            "edgecrab-session",
                            edgecrab_core::safe_truncate(&session_id, 8),
                            "md",
                        )
                    });
                    let markdown = render_markdown_export(
                        &messages,
                        record.model.as_deref().unwrap_or("unknown"),
                        &session_id,
                    );
                    std::fs::write(&out_path, markdown)?;
                    println!(
                        "Exported {} to {}",
                        &session_id[..session_id.len().min(12)],
                        out_path.display()
                    );
                }
                other => {
                    anyhow::bail!("Unknown export format '{other}'. Use 'markdown' or 'jsonl'.")
                }
            }
        }
        SessionCommand::Delete { id } => {
            let session_id = resolve_session_id(&db, &id)?;
            db.delete_session(&session_id)?;
            println!(
                "Deleted session {}",
                &session_id[..session_id.len().min(12)]
            );
        }
        SessionCommand::Rename { id, title } => {
            let session_id = resolve_session_id(&db, &id)?;
            let new_title = title.join(" ");
            if new_title.is_empty() {
                anyhow::bail!("Usage: edgecrab sessions rename <id> <new title>");
            }
            db.update_session_title(&session_id, &new_title)?;
            println!(
                "Renamed {} → \"{}\"",
                &session_id[..session_id.len().min(12)],
                new_title
            );
        }
        SessionCommand::Prune {
            older_than,
            source,
            yes,
        } => {
            if !yes {
                println!(
                    "This will delete ended sessions older than {} days{}.",
                    older_than,
                    source
                        .as_deref()
                        .map(|s| format!(" (source: {s})"))
                        .unwrap_or_default()
                );
                print!("Continue? [y/N] ");
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            let count = db.prune_sessions(older_than, source.as_deref())?;
            println!("Pruned {count} session(s).");
        }
        SessionCommand::Stats => {
            let stats = db.session_statistics()?;
            println!("Total sessions: {}", stats.total_sessions);
            println!("Total messages: {}", stats.total_messages);
            for (source, count) in &stats.by_source {
                println!("  {source}: {count} sessions");
            }
            let size_mb = stats.db_size_bytes as f64 / (1024.0 * 1024.0);
            println!("Database size: {size_mb:.1} MB");
        }
    }

    Ok(())
}

fn print_session_rich_list(sessions: &[edgecrab_state::SessionRichSummary]) {
    println!(
        "{:<22} {:<16} {:<10} {:<6} {:<14} Preview",
        "Title", "Model", "Source", "Msgs", "Last Active"
    );
    println!("{}", "─".repeat(104));
    for session in sessions {
        let title = session.title.as_deref().unwrap_or("—");
        println!(
            "{:<22} {:<16} {:<10} {:<6} {:<14} {}",
            edgecrab_core::safe_truncate(title, 22),
            edgecrab_core::safe_truncate(session.model.as_deref().unwrap_or("?"), 16),
            edgecrab_core::safe_truncate(&session.source, 10),
            session.message_count,
            edgecrab_core::safe_truncate(&format_timestamp(session.last_active), 14),
            edgecrab_core::safe_truncate(&session.preview, 42),
        );
        println!("  id={}", edgecrab_core::safe_truncate(&session.id, 12));
    }
}

fn run_config(command: ConfigCommand, args: &CliArgs) -> anyhow::Result<()> {
    let runtime = load_runtime(args.config.as_deref(), args.model.as_deref(), None)?;
    match command {
        ConfigCommand::Show => {
            println!("{}", serde_yml::to_string(&runtime.config)?);
        }
        ConfigCommand::Edit => {
            let mut editor = editor_command_from_env()?;
            let config_parent = runtime
                .config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."));
            std::fs::create_dir_all(config_parent).with_context(|| {
                format!(
                    "failed to create config directory {}",
                    config_parent.display()
                )
            })?;
            let display_editor = format_command_for_display(&editor);
            let status = editor
                .arg(&runtime.config_path)
                .status()
                .with_context(|| format!("failed to launch editor: {display_editor}"))?;
            if !status.success() {
                anyhow::bail!("editor exited with status: {status}");
            }
        }
        ConfigCommand::Path => {
            println!("{}", runtime.config_path.display());
        }
        ConfigCommand::EnvPath => {
            let env_path = runtime
                .config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(".env");
            println!("{}", env_path.display());
        }
        ConfigCommand::Set { key, value } => {
            let mut config = runtime.config;
            set_config_value(&mut config, &key, &value)?;
            config.save_to(&runtime.config_path)?;
            println!("Updated {} in {}", key, runtime.config_path.display());
        }
    }
    Ok(())
}

fn run_tools(command: ToolsCommand, args: &CliArgs) -> anyhow::Result<()> {
    let runtime = load_runtime(args.config.as_deref(), args.model.as_deref(), None)?;
    let registry = build_tool_registry();
    match command {
        ToolsCommand::List => {
            for toolset in registry.toolset_names() {
                let tools = registry.tools_in_toolset(toolset);
                let enabled = toolset_enabled(&runtime.config, toolset);
                println!(
                    "[{}] {} ({} tools)",
                    if enabled { "on" } else { "off" },
                    toolset,
                    tools.len()
                );
                println!("  {}", tools.join(", "));
            }
        }
        ToolsCommand::Enable { name } => {
            let mut config = runtime.config;
            let changed = set_toolset_state(&mut config, registry.as_ref(), &name, true)?;
            config.save_to(&runtime.config_path)?;
            println!("Enabled: {}", changed.join(", "));
        }
        ToolsCommand::Disable { name } => {
            let mut config = runtime.config;
            let changed = set_toolset_state(&mut config, registry.as_ref(), &name, false)?;
            config.save_to(&runtime.config_path)?;
            println!("Disabled: {}", changed.join(", "));
        }
    }
    Ok(())
}

async fn run_mcp(command: McpCommand, args: &CliArgs) -> anyhow::Result<()> {
    let runtime = load_runtime(args.config.as_deref(), args.model.as_deref(), None)?;
    let mut config = runtime.config;
    match command {
        McpCommand::List => match edgecrab_tools::tools::mcp_client::configured_servers() {
            Ok(servers) if !servers.is_empty() => {
                for server in servers {
                    let transport = if let Some(url) = &server.url {
                        format!("http {url}")
                    } else {
                        let mut rendered = server.command;
                        if !server.args.is_empty() {
                            rendered.push(' ');
                            rendered.push_str(&server.args.join(" "));
                        }
                        rendered
                    };
                    println!("{}  {}", server.name, transport);
                }
            }
            Ok(_) => {
                println!("No MCP servers configured.");
            }
            Err(_) if config.mcp_servers.is_empty() => {
                println!("No MCP servers configured.");
            }
            Err(e) => return Err(anyhow::anyhow!(e.to_string())),
        },
        McpCommand::Refresh => {
            let entries = mcp_catalog::refresh_official_catalog().await?;
            println!(
                "Refreshed official MCP catalog ({} entries).",
                entries.len()
            );
        }
        McpCommand::Search { query } => {
            let report = mcp_catalog::search_mcp_sources(query.as_deref(), 12).await;
            let has_results = report.groups.iter().any(|group| !group.results.is_empty());
            if !has_results {
                println!("No official MCP entries matched.");
                return Ok(());
            }
            println!(
                "{}",
                mcp_catalog::render_search_report(query.as_deref(), &report)
            );
        }
        McpCommand::View { preset } => {
            if let Some(preset) = mcp_catalog::find_preset(&preset) {
                println!("Preset: {}", preset.id);
                println!("Name:   {}", preset.display_name);
                println!("Why:    {}", preset.description);
                println!("Pkg:    {}", preset.package_name);
                println!("Source: {}", preset.source_url);
                println!("Docs:   {}", preset.homepage);
                println!("Cmd:    {} {}", preset.command, preset.args.join(" "));
                println!("Tags:   {}", preset.tags.join(", "));
                if !preset.required_env.is_empty() {
                    println!("Env:    {}", preset.required_env.join(", "));
                }
                println!("Notes:  {}", preset.notes);
            } else if let Some(entry) =
                mcp_catalog::find_official_catalog_entry_with_refresh(&preset).await
            {
                println!("{}", mcp_catalog::render_official_catalog_entry(&entry));
            } else {
                anyhow::bail!("unknown MCP preset or official catalog entry '{}'", preset);
            }
        }
        McpCommand::Install { preset, name, path } => {
            let cwd = std::env::current_dir().context("cannot determine current directory")?;
            let installed = mcp_catalog::install_preset(
                &mut config,
                &preset,
                name.as_deref(),
                path.as_deref().map(std::path::Path::new),
                &cwd,
            )?;
            config.save_to(&runtime.config_path)?;
            println!("Configured MCP server '{}'.", installed.name);
            if !installed.missing_env.is_empty() {
                println!(
                    "Warning: missing environment variables: {}",
                    installed.missing_env.join(", ")
                );
            }
            println!(
                "Run `edgecrab mcp doctor {}` to verify connectivity and config health.",
                installed.name
            );
        }
        McpCommand::Test { name } => {
            let targets = if let Some(name) = name {
                vec![name]
            } else {
                edgecrab_tools::tools::mcp_client::configured_servers()
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?
                    .into_iter()
                    .map(|server| server.name)
                    .collect::<Vec<_>>()
            };

            if targets.is_empty() {
                println!("No MCP servers configured.");
                return Ok(());
            }

            for target in targets {
                match edgecrab_tools::tools::mcp_client::probe_configured_server(&target).await {
                    Ok(result) => {
                        println!(
                            "{}  ok  transport={} tools={}",
                            result.server_name, result.transport, result.tool_count
                        );
                        for (tool_name, description) in result.tools.iter().take(5) {
                            if description.is_empty() {
                                println!("  - {}", tool_name);
                            } else {
                                println!("  - {} — {}", tool_name, description);
                            }
                        }
                    }
                    Err(err) => {
                        println!("{}  fail  {}", target, err);
                    }
                }
            }
        }
        McpCommand::Doctor { name } => {
            println!(
                "{}",
                mcp_support::render_mcp_doctor_report(name.as_deref()).await?
            );
        }
        McpCommand::Auth { name } => {
            println!("{}", mcp_support::render_mcp_auth_guide(&name)?);
        }
        McpCommand::Login { name } => {
            let summary = mcp_oauth::login_mcp_server(&name, |line| println!("{line}")).await?;
            println!("{summary}");
        }
        McpCommand::Add {
            name,
            command,
            args,
        } => {
            config.mcp_servers.insert(
                name.clone(),
                McpServerConfig {
                    command,
                    args,
                    enabled: true,
                    ..Default::default()
                },
            );
            config.save_to(&runtime.config_path)?;
            println!("Configured MCP server '{}'", name);
        }
        McpCommand::Remove { name } => {
            if config.mcp_servers.remove(&name).is_some() {
                config.save_to(&runtime.config_path)?;
                edgecrab_tools::tools::mcp_client::remove_mcp_token(&name);
                edgecrab_tools::tools::mcp_client::reload_mcp_connections();
                println!("Removed MCP server '{}'", name);
            } else {
                anyhow::bail!("unknown MCP server '{}'", name);
            }
        }
    }
    Ok(())
}

fn run_plugins(command: PluginsCommand) -> anyhow::Result<()> {
    match command {
        PluginsCommand::List => plugins_cmd::run(plugins_cmd::PluginAction::List)?,
        PluginsCommand::Install { repo, name } => {
            plugins_cmd::run(plugins_cmd::PluginAction::Install { repo, name })?
        }
        PluginsCommand::Update { name } => {
            plugins_cmd::run(plugins_cmd::PluginAction::Update { name })?
        }
        PluginsCommand::Remove { name } => {
            plugins_cmd::run(plugins_cmd::PluginAction::Remove { name })?
        }
    }
    Ok(())
}

async fn run_gateway(command: GatewayCommand, args: &CliArgs) -> anyhow::Result<()> {
    match command {
        GatewayCommand::Configure { platform } => {
            gateway_setup::run(args, platform.as_deref())?;
            Ok(())
        }
        _ => {
            let action = match command {
                GatewayCommand::Start { foreground } => {
                    gateway_cmd::GatewayAction::Start { foreground }
                }
                GatewayCommand::Stop => gateway_cmd::GatewayAction::Stop,
                GatewayCommand::Restart => gateway_cmd::GatewayAction::Restart,
                GatewayCommand::Status => gateway_cmd::GatewayAction::Status,
                GatewayCommand::Configure { .. } => unreachable!(),
            };
            gateway_cmd::run(action, args).await
        }
    }
}

/// Manage skills from the CLI (`edgecrab skills list/view/search/install/remove`).
///
/// WHY: Mirrors `hermes skills` subcommand. Allows installing and browsing
/// skill prompts without entering the TUI.
async fn run_skills(command: SkillsCommand) -> anyhow::Result<()> {
    let skills_dir = edgecrab_core::edgecrab_home().join("skills");

    match command {
        SkillsCommand::List => {
            if !skills_dir.exists() {
                println!(
                    "No skills installed. Skills directory: {}",
                    skills_dir.display()
                );
                println!("Install with: edgecrab skills install <repo-or-path>");
                return Ok(());
            }
            let skills = collect_installed_skills(&skills_dir)?;
            if skills.is_empty() {
                println!(
                    "No skills found (no SKILL.md files in {}).",
                    skills_dir.display()
                );
            } else {
                println!("Installed skills ({}):", skills.len());
                for skill in &skills {
                    if skill.description.is_empty() {
                        println!("  {}", skill.identifier);
                    } else {
                        println!("  {} — {}", skill.identifier, skill.description);
                    }
                }
            }
        }

        SkillsCommand::View { name } => {
            let skill = resolve_installed_skill(&skills_dir, &name)?;
            let content = std::fs::read_to_string(&skill.skill_md)?;
            println!("{}", content);
        }

        SkillsCommand::Search { query } => {
            let query_lower = query.to_lowercase();
            let installed_matches: Vec<InstalledSkill> = collect_installed_skills(&skills_dir)?
                .into_iter()
                .filter(|skill| {
                    skill.identifier.to_lowercase().contains(&query_lower)
                        || skill.description.to_lowercase().contains(&query_lower)
                })
                .collect();

            let optional_root = edgecrab_tools::tools::skills_sync::optional_skills_dir()
                .unwrap_or_else(|| edgecrab_core::edgecrab_home().join("optional-skills"));
            let official_matches =
                edgecrab_tools::tools::skills_hub::search_optional_skills(&optional_root, &query);
            let remote_report =
                edgecrab_tools::tools::skills_hub::search_hub(&query, None, 8).await;
            let has_remote_matches = remote_report
                .groups
                .iter()
                .any(|group| !group.results.is_empty());

            if installed_matches.is_empty() && official_matches.is_empty() && !has_remote_matches {
                println!("No skills matching '{}'.", query);
            } else {
                if !installed_matches.is_empty() {
                    println!("Installed matches ({}):", installed_matches.len());
                    for skill in &installed_matches {
                        if skill.description.is_empty() {
                            println!("  {}", skill.identifier);
                        } else {
                            println!("  {} — {}", skill.identifier, skill.description);
                        }
                    }
                }
                if !official_matches.is_empty() {
                    println!("Official matches ({}):", official_matches.len());
                    for skill in &official_matches {
                        println!("  {} — {}", skill.identifier, skill.description);
                    }
                }
                if has_remote_matches {
                    println!(
                        "\n{}",
                        edgecrab_tools::tools::skills_hub::render_search_report(
                            &query,
                            &remote_report
                        )
                    );
                }
            }
        }

        SkillsCommand::Install { source, name } => {
            let source_path = std::path::Path::new(&source);
            if source_path.exists() {
                let bundle = build_local_skill_bundle(source_path, name.as_deref())?;
                let skill_name = bundle.name.clone();
                let message =
                    edgecrab_tools::tools::skills_hub::install_skill(&bundle, &skills_dir, false)
                        .map_err(|e| anyhow::anyhow!(e))?;
                println!("{message}");
                println!("Activate with: edgecrab skills view {skill_name}");
                return Ok(());
            }

            let outcome = edgecrab_tools::tools::skills_hub::install_identifier(
                &source,
                &skills_dir,
                edgecrab_tools::tools::skills_sync::optional_skills_dir().as_deref(),
                false,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
            println!("{}", outcome.message);
            println!("Activate with: edgecrab skills view {}", outcome.skill_name);
        }

        SkillsCommand::Update { name } => {
            let optional_dir = edgecrab_tools::tools::skills_sync::optional_skills_dir();
            if let Some(name) = name {
                let outcome = edgecrab_tools::tools::skills_hub::update_installed_skill(
                    &name,
                    &skills_dir,
                    optional_dir.as_deref(),
                    false,
                )
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
                println!("{}", outcome.message);
                println!("Activate with: edgecrab skills view {}", outcome.skill_name);
            } else {
                let outcomes = edgecrab_tools::tools::skills_hub::update_all_installed_skills(
                    &skills_dir,
                    optional_dir.as_deref(),
                    false,
                )
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
                println!(
                    "{}",
                    edgecrab_tools::tools::skills_hub::render_update_outcomes(&outcomes)
                );
            }
        }

        SkillsCommand::Remove { name } => {
            let skill = resolve_installed_skill(&skills_dir, &name)?;
            std::fs::remove_dir_all(&skill.skill_dir)?;
            println!("Removed skill '{}'.", skill.identifier);
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct InstalledSkill {
    identifier: String,
    skill_dir: PathBuf,
    skill_md: PathBuf,
    description: String,
}

fn collect_installed_skills(skills_dir: &std::path::Path) -> anyhow::Result<Vec<InstalledSkill>> {
    let mut skills = Vec::new();
    if !skills_dir.is_dir() {
        return Ok(skills);
    }

    let mut stack = vec![skills_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with('.'))
                .unwrap_or(false)
            {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                let identifier = path
                    .strip_prefix(skills_dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let description = std::fs::read_to_string(&skill_md)
                    .map(|content| extract_skill_description(&content))
                    .unwrap_or_default();
                skills.push(InstalledSkill {
                    identifier,
                    skill_dir: path,
                    skill_md,
                    description,
                });
            } else {
                stack.push(path);
            }
        }
    }

    skills.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    Ok(skills)
}

fn resolve_installed_skill(
    skills_dir: &std::path::Path,
    query: &str,
) -> anyhow::Result<InstalledSkill> {
    if query.contains("..") || std::path::Path::new(query).is_absolute() {
        anyhow::bail!("Invalid skill path '{}'", query);
    }

    let direct = skills_dir.join(query);
    if direct.join("SKILL.md").is_file() {
        let identifier = direct
            .strip_prefix(skills_dir)
            .unwrap_or(&direct)
            .to_string_lossy()
            .replace('\\', "/");
        let skill_md = direct.join("SKILL.md");
        let description = std::fs::read_to_string(&skill_md)
            .map(|content| extract_skill_description(&content))
            .unwrap_or_default();
        return Ok(InstalledSkill {
            identifier,
            skill_dir: direct,
            skill_md,
            description,
        });
    }

    let matches: Vec<InstalledSkill> = collect_installed_skills(skills_dir)?
        .into_iter()
        .filter(|skill| skill.identifier.split('/').next_back() == Some(query))
        .collect();

    match matches.len() {
        1 => Ok(matches.into_iter().next().expect("single match")),
        0 => anyhow::bail!("Skill '{}' not found.", query),
        _ => {
            let options = matches
                .iter()
                .map(|skill| skill.identifier.clone())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("Skill '{}' is ambiguous. Use one of: {}", query, options);
        }
    }
}

fn extract_skill_description(content: &str) -> String {
    let trimmed = content.trim_start();
    if let Some(frontmatter) = trimmed.strip_prefix("---") {
        if let Some(end) = frontmatter.find("\n---") {
            for line in frontmatter[..end].lines() {
                if let Some(desc) = line.strip_prefix("description:") {
                    return desc.trim().trim_matches('"').trim_matches('\'').to_string();
                }
            }
        }
    }

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("---") {
            continue;
        }
        return trimmed.to_string();
    }

    String::new()
}

fn build_local_skill_bundle(
    source_path: &std::path::Path,
    name_override: Option<&str>,
) -> anyhow::Result<edgecrab_tools::tools::skills_hub::SkillBundle> {
    let skill_name = name_override
        .map(str::to_string)
        .unwrap_or_else(|| derive_skill_name(source_path));

    if skill_name.is_empty()
        || skill_name.contains('/')
        || skill_name.contains('\\')
        || skill_name.contains("..")
    {
        anyhow::bail!(
            "Derived skill name '{}' is unsafe; provide --name",
            skill_name
        );
    }

    let mut files = std::collections::HashMap::new();
    if source_path.is_file() {
        let content = std::fs::read_to_string(source_path)?;
        files.insert("SKILL.md".into(), content);
    } else {
        let skill_md = source_path.join("SKILL.md");
        if !skill_md.is_file() {
            anyhow::bail!("No SKILL.md found in {}", source_path.display());
        }
        collect_local_skill_files(source_path, source_path, &mut files)?;
    }

    Ok(edgecrab_tools::tools::skills_hub::SkillBundle {
        name: skill_name.clone(),
        files,
        source: "local".into(),
        identifier: source_path.display().to_string(),
        trust_level: "trusted".into(),
    })
}

fn derive_skill_name(source_path: &std::path::Path) -> String {
    if source_path.is_file() {
        return source_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_string();
    }
    source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill")
        .to_string()
}

fn collect_local_skill_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    files: &mut std::collections::HashMap<String, String>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_local_skill_files(root, &path, files)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(rel, std::fs::read_to_string(&path)?);
        }
    }
    Ok(())
}

async fn run_cron(command: CronCommand, args: &CliArgs) -> anyhow::Result<()> {
    cron_cmd::run(command, args).await
}

/// Dispatch all `edgecrab profile <sub>` commands.
///
/// WHY separate fn: keeps run_subcommand() slim; ProfileManager owns all I/O.
fn run_profile(command: ProfileCommand) -> anyhow::Result<()> {
    let mgr = profile::ProfileManager::new();
    match command {
        ProfileCommand::List => mgr.list()?,
        ProfileCommand::Use { name } => mgr.use_profile(&name)?,
        ProfileCommand::Create {
            name,
            clone,
            clone_all,
            clone_from,
        } => {
            mgr.create(&name, clone, clone_all, clone_from.as_deref())?;
        }
        ProfileCommand::Delete { name, yes } => mgr.delete(&name, yes)?,
        ProfileCommand::Show { name } => mgr.show(&name)?,
        ProfileCommand::Alias {
            name,
            remove,
            name_override,
        } => {
            mgr.alias(&name, remove, name_override.as_deref())?;
        }
        ProfileCommand::Rename { old_name, new_name } => mgr.rename(&old_name, &new_name)?,
        ProfileCommand::Export { name, output } => mgr.export(&name, output.as_deref())?,
        ProfileCommand::Import { archive, name } => mgr.import(&archive, name.as_deref())?,
    }
    Ok(())
}

/// Resolve a session ID from `--session`, `--continue`, or `--resume` flags.
///
/// Priority: `--session` > `--resume` > `--continue`.
/// `--continue` with no value resumes the most recent CLI session.
/// `--continue "title"` resolves by title (with lineage).
/// `--resume <id-or-title>` resolves by ID prefix or title.
fn resolve_session_flag(args: &CliArgs, db: &SessionDb) -> anyhow::Result<Option<String>> {
    // --session takes precedence (exact ID)
    if let Some(ref id) = args.session {
        return Ok(Some(id.clone()));
    }

    // --resume resolves by ID prefix or title
    if let Some(ref id_or_title) = args.resume {
        match db.resolve_session(id_or_title)? {
            Some(id) => return Ok(Some(id)),
            None => anyhow::bail!("no session matching '{id_or_title}'"),
        }
    }

    // --continue: no value → most recent CLI session; with value → title resolve
    if let Some(ref maybe_title) = args.continue_session {
        match maybe_title {
            Some(title) => {
                // Resolve by title (with lineage support)
                match db.resolve_session(title)? {
                    Some(id) => return Ok(Some(id)),
                    None => anyhow::bail!("no session matching title '{title}'"),
                }
            }
            None => {
                // Most recent CLI session
                let sessions = db.list_sessions_by_source("cli", 1)?;
                match sessions.first() {
                    Some(s) => return Ok(Some(s.id.clone())),
                    None => anyhow::bail!("no previous CLI sessions found"),
                }
            }
        }
    }

    Ok(None)
}

fn resolve_session_id(db: &SessionDb, prefix: &str) -> anyhow::Result<String> {
    // Try the new resolve_session which handles ID prefix + title + lineage
    if let Some(id) = db.resolve_session(prefix)? {
        return Ok(id);
    }
    anyhow::bail!("no session matching '{}'", prefix)
}

fn format_timestamp(ts: f64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts as i64, 0)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "unknown".into())
}

fn parse_bool(value: &str) -> anyhow::Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => anyhow::bail!("expected boolean value, got '{}'", value),
    }
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn runtime_home_for_config_override(config_override: Option<&str>) -> Option<PathBuf> {
    config_override.map(|path| {
        std::path::Path::new(path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf()
    })
}

fn activate_runtime_home_from_config(config_override: Option<&str>) -> anyhow::Result<()> {
    let Some(home) = runtime_home_for_config_override(config_override) else {
        return Ok(());
    };
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("EDGECRAB_HOME", &home);
    }
    Ok(())
}

fn editor_command_from_env() -> anyhow::Result<std::process::Command> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_string());
    parse_editor_command(&editor)
}

fn parse_editor_command(editor: &str) -> anyhow::Result<std::process::Command> {
    let parts = shell_split(editor)
        .map_err(|e| anyhow::anyhow!("invalid $EDITOR/$VISUAL command '{}': {e}", editor))?;
    let (program, args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("$EDITOR/$VISUAL is empty"))?;
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    Ok(cmd)
}

fn format_command_for_display(cmd: &std::process::Command) -> String {
    let mut rendered = cmd.get_program().to_string_lossy().to_string();
    for arg in cmd.get_args() {
        rendered.push(' ');
        rendered.push_str(&arg.to_string_lossy());
    }
    rendered
}

fn provider_environment_hint(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "azure" => "AZURE_OPENAI_API_KEY",
        "bedrock" => "AWS_ACCESS_KEY_ID",
        "copilot" => "GITHUB_TOKEN",
        "gemini" => "GOOGLE_API_KEY",
        "huggingface" => "HUGGINGFACE_API_KEY",
        "lmstudio" => "local, no key",
        "mistral" => "MISTRAL_API_KEY",
        "ollama" => "local, no key",
        "openai" => "OPENAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "vertexai" => "GOOGLE_CLOUD_PROJECT + ADC",
        "xai" => "XAI_API_KEY",
        _ => "Provider configured via model catalog/runtime integration",
    }
}

fn render_version_report() -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let version = env!("CARGO_PKG_VERSION");
    let _ = writeln!(out, "EdgeCrab v{version}");
    let _ = writeln!(out, "Rust {}", env!("CARGO_PKG_RUST_VERSION", "unknown"));
    let _ = writeln!(out);
    let _ = writeln!(out, "Supported providers (from model catalog):");
    for provider in edgecrab_core::ModelCatalog::provider_ids() {
        let label = edgecrab_core::ModelCatalog::provider_label(&provider);
        let hint = provider_environment_hint(&provider);
        let _ = writeln!(out, "  {provider:<14} — {label} ({hint})");
    }
    let _ = writeln!(out);
    let home = setup::edgecrab_home();
    let _ = writeln!(out, "Home:   {}", home.display());
    let _ = writeln!(out, "Config: {}", home.join("config.yaml").display());
    let _ = writeln!(out);
    let _ = writeln!(out, "Links:");
    let _ = writeln!(out, "  Docs:    https://github.com/raphaelmansuy/edgecrab");
    let _ = writeln!(
        out,
        "  Issues:  https://github.com/raphaelmansuy/edgecrab/issues"
    );
    out
}

fn set_config_value(
    config: &mut edgecrab_core::AppConfig,
    key: &str,
    value: &str,
) -> anyhow::Result<()> {
    match key {
        "model.default" => config.model.default_model = value.to_string(),
        "model.max_iterations" => config.model.max_iterations = value.parse()?,
        "model.temperature" => config.model.temperature = Some(value.parse()?),
        "model.streaming" => {
            let enabled = parse_bool(value)?;
            config.model.streaming = enabled;
            config.display.streaming = enabled;
        }
        "display.skin" => config.display.skin = value.to_string(),
        "display.personality" => config.display.personality = value.to_string(),
        "display.show_reasoning" => config.display.show_reasoning = parse_bool(value)?,
        "display.show_status_bar" => config.display.show_status_bar = parse_bool(value)?,
        "display.streaming" => {
            let enabled = parse_bool(value)?;
            config.display.streaming = enabled;
            config.model.streaming = enabled;
        }
        "model.smart_routing.enabled" => config.model.smart_routing.enabled = parse_bool(value)?,
        "model.smart_routing.cheap_model" => {
            config.model.smart_routing.cheap_model = value.to_string()
        }
        "model.smart_routing.cheap_base_url" => {
            config.model.smart_routing.cheap_base_url = Some(value.to_string())
        }
        "model.smart_routing.cheap_api_key_env" => {
            config.model.smart_routing.cheap_api_key_env = Some(value.to_string())
        }
        "moa.enabled" => config.moa.enabled = parse_bool(value)?,
        "moa.aggregator_model" => config.moa.aggregator_model = value.to_string(),
        "moa.reference_models" => config.moa.reference_models = parse_csv(value),
        "memory.enabled" => config.memory.enabled = parse_bool(value)?,
        "skills.enabled" => config.skills.enabled = parse_bool(value)?,
        "timezone" => config.timezone = Some(value.to_string()),
        "gateway.host" => config.gateway.host = value.to_string(),
        "gateway.port" => config.gateway.port = value.parse()?,
        "gateway.webhook_enabled" => config.gateway.webhook_enabled = parse_bool(value)?,
        "gateway.enabled_platforms" => config.gateway.enabled_platforms = parse_csv(value),
        "gateway.whatsapp.enabled" => config.gateway.whatsapp.enabled = parse_bool(value)?,
        "gateway.whatsapp.mode" => config.gateway.whatsapp.mode = value.to_string(),
        "gateway.whatsapp.allowed_users" => {
            config.gateway.whatsapp.allowed_users = parse_csv(value)
        }
        "gateway.whatsapp.bridge_port" => config.gateway.whatsapp.bridge_port = value.parse()?,
        "tools.enabled_toolsets" => config.tools.enabled_toolsets = Some(parse_csv(value)),
        "tools.disabled_toolsets" => config.tools.disabled_toolsets = Some(parse_csv(value)),
        _ => anyhow::bail!("unsupported config key '{}'", key),
    }
    Ok(())
}

fn toolset_enabled(config: &edgecrab_core::AppConfig, toolset: &str) -> bool {
    let enabled = config.tools.enabled_toolsets.as_ref();
    let disabled = config.tools.disabled_toolsets.as_ref();
    let allowed = enabled
        .map(|v| v.iter().any(|s| s == toolset))
        .unwrap_or(true);
    let blocked = disabled
        .map(|v| v.iter().any(|s| s == toolset))
        .unwrap_or(false);
    allowed && !blocked
}

fn set_toolset_state(
    config: &mut edgecrab_core::AppConfig,
    registry: &ToolRegistry,
    name: &str,
    enabled: bool,
) -> anyhow::Result<Vec<String>> {
    let targets: Vec<String> = if let Some(alias) = resolve_alias(name) {
        alias.iter().map(|s| s.to_string()).collect()
    } else if registry.toolset_names().contains(&name) {
        vec![name.to_string()]
    } else {
        anyhow::bail!("'{}' is not a known toolset or alias", name);
    };

    let disabled_sets = config.tools.disabled_toolsets.get_or_insert_with(Vec::new);
    if enabled {
        disabled_sets.retain(|s| !targets.contains(s));
        if let Some(enabled_sets) = &mut config.tools.enabled_toolsets {
            for target in &targets {
                if !enabled_sets.contains(target) {
                    enabled_sets.push(target.clone());
                }
            }
        }
    } else {
        for target in &targets {
            if !disabled_sets.contains(target) {
                disabled_sets.push(target.clone());
            }
        }
        if let Some(enabled_sets) = &mut config.tools.enabled_toolsets {
            enabled_sets.retain(|s| !targets.contains(s));
        }
    }
    Ok(targets)
}

// ── Git worktree helpers ───────────────────────────────────────────────

/// Create a disposable git worktree under `.worktrees/<branch>` in the
/// current repo root and return its path.
///
/// The branch name is derived from a short random hash so parallel
/// invocations each get their own isolated workspace:
///
/// ```text
///   .worktrees/
///   ├── edgecrab-a1b2c3d4/   ← worktree for session 1
///   └── edgecrab-e5f6g7h8/   ← worktree for session 2
/// ```
///
/// Mirrors `hermes -w` which creates worktrees under `.worktrees/`.
fn setup_worktree() -> anyhow::Result<PathBuf> {
    // Verify git is available
    let git_check = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output();

    match git_check {
        Ok(out) if out.status.success() => {}
        Ok(_) => anyhow::bail!("current directory is not inside a git repository"),
        Err(e) => anyhow::bail!("git not found: {e}"),
    }

    // Find the repo root
    let root_out = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    let repo_root = PathBuf::from(String::from_utf8_lossy(&root_out.stdout).trim());

    // Generate a short unique hash for the branch/worktree name
    let hash = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{:08x}", ts as u32 ^ (ts >> 32) as u32)
    };

    let branch_name = format!("edgecrab/edgecrab-{hash}");
    let worktrees_dir = repo_root.join(".worktrees");
    std::fs::create_dir_all(&worktrees_dir).with_context(|| {
        format!(
            "failed to create .worktrees/ dir in {}",
            repo_root.display()
        )
    })?;

    let wt_path = worktrees_dir.join(format!("edgecrab-{hash}"));

    // Create the worktree with a new branch
    let result = std::process::Command::new("git")
        .args(["worktree", "add", "-b", &branch_name])
        .arg(&wt_path)
        .arg("HEAD")
        .current_dir(&repo_root)
        .output()?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    Ok(wt_path)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{
        activate_runtime_home_from_config, parse_editor_command, render_version_report,
        runtime_home_for_config_override, set_config_value,
    };

    #[test]
    fn set_config_value_supports_smart_routing_and_moa_keys() {
        let mut config = edgecrab_core::AppConfig::default();

        set_config_value(&mut config, "model.smart_routing.enabled", "true")
            .expect("enable smart routing");
        set_config_value(
            &mut config,
            "model.smart_routing.cheap_model",
            "copilot/gpt-4.1-mini",
        )
        .expect("set cheap model");
        set_config_value(&mut config, "moa.enabled", "false").expect("disable moa");
        set_config_value(
            &mut config,
            "moa.aggregator_model",
            "anthropic/claude-opus-4.6",
        )
        .expect("set moa aggregator");
        set_config_value(
            &mut config,
            "moa.reference_models",
            "anthropic/claude-opus-4.6,openai/gpt-4.1",
        )
        .expect("set moa refs");

        assert!(config.model.smart_routing.enabled);
        assert_eq!(
            config.model.smart_routing.cheap_model,
            "copilot/gpt-4.1-mini"
        );
        assert!(!config.moa.enabled);
        assert_eq!(config.moa.aggregator_model, "anthropic/claude-opus-4.6");
        assert_eq!(
            config.moa.reference_models,
            vec!["anthropic/claude-opus-4.6", "openai/gpt-4.1"]
        );
    }

    #[test]
    fn set_config_value_supports_status_bar_visibility() {
        let mut config = edgecrab_core::AppConfig::default();
        set_config_value(&mut config, "display.show_status_bar", "false").expect("set status bar");
        assert!(!config.display.show_status_bar);
    }

    #[test]
    fn runtime_home_for_config_override_uses_parent_directory() {
        let home = runtime_home_for_config_override(Some("/tmp/edgecrab-custom/config.yaml"))
            .expect("home from config");
        assert_eq!(home, PathBuf::from("/tmp/edgecrab-custom"));
    }

    #[test]
    fn activate_runtime_home_from_config_sets_edgecrab_home() {
        let previous = std::env::var_os("EDGECRAB_HOME");
        activate_runtime_home_from_config(Some("/tmp/edgecrab-runtime/config.yaml"))
            .expect("activate runtime home");
        assert_eq!(
            std::env::var_os("EDGECRAB_HOME"),
            Some(OsString::from("/tmp/edgecrab-runtime"))
        );
        #[allow(unsafe_code)]
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("EDGECRAB_HOME", value);
            } else {
                std::env::remove_var("EDGECRAB_HOME");
            }
        }
    }

    #[test]
    fn parse_editor_command_supports_editor_arguments() {
        let cmd = parse_editor_command("code --wait").expect("editor command");
        assert_eq!(cmd.get_program().to_string_lossy(), "code");
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(args, vec!["--wait"]);
    }

    #[test]
    fn version_report_covers_catalog_providers() {
        let report = render_version_report();
        for provider in edgecrab_core::ModelCatalog::provider_ids() {
            assert!(
                report.contains(&provider),
                "version report missing provider {provider}"
            );
        }
        assert!(report.contains("EdgeCrab v"));
        assert!(report.contains("Supported providers (from model catalog):"));
    }
}
