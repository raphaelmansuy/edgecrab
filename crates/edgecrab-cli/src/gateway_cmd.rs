use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::Context;
use edgecrab_gateway::platform::PlatformAdapter;
use edgecrab_state::SessionDb;
use edgecrab_tools::registry::GatewaySender;
use tokio_util::sync::CancellationToken;

use crate::cli_args::CliArgs;
use crate::create_provider;
use crate::gateway_catalog::{PlatformState, collect_platform_diagnostics};
use crate::runtime::{
    build_agent, build_tool_registry_with_mcp_discovery, load_runtime, open_state_db,
};

#[derive(Debug, Clone, Copy)]
pub enum GatewayAction {
    Start { foreground: bool },
    Stop,
    Restart,
    Status,
}

#[derive(Debug, Clone)]
pub struct GatewayStatus {
    pub pid: Option<u32>,
    pub running: bool,
    pub stale_pid: bool,
    pub log_path: PathBuf,
}

pub async fn attach_gateway_sender_if_running(
    agent: &Arc<edgecrab_core::Agent>,
    runtime: &crate::runtime::RuntimeContext,
) -> anyhow::Result<()> {
    if !snapshot()?.running {
        return Ok(());
    }

    if let Some(sender) = build_standalone_gateway_sender(runtime, agent.state_db().await)? {
        agent.set_gateway_sender(sender).await;
    }

    Ok(())
}

pub async fn run(action: GatewayAction, args: &CliArgs) -> anyhow::Result<()> {
    let result = match action {
        GatewayAction::Start { foreground } => {
            if foreground {
                run_foreground(args).await
            } else {
                start_background(args)
            }
        }
        GatewayAction::Stop => stop_background(),
        GatewayAction::Restart => restart_background(args),
        GatewayAction::Status => status(args),
    };

    if let Err(ref err) = result {
        print_gateway_failure_guidance(action, args, err);
    }

    result
}

fn build_standalone_gateway_sender(
    runtime: &crate::runtime::RuntimeContext,
    state_db: Option<Arc<SessionDb>>,
) -> anyhow::Result<Option<Arc<dyn GatewaySender>>> {
    let adapters = build_outbound_platform_adapters(&runtime.config)?;
    if adapters.is_empty() {
        return Ok(None);
    }

    let mut router = edgecrab_gateway::delivery::DeliveryRouter::new();
    for adapter in adapters {
        router.register(adapter);
    }

    Ok(Some(Arc::new(
        edgecrab_gateway::sender::GatewaySenderBridge::new(Arc::new(router), state_db),
    )))
}

fn build_outbound_platform_adapters(
    config: &edgecrab_core::AppConfig,
) -> anyhow::Result<Vec<Arc<dyn PlatformAdapter>>> {
    let mut adapters: Vec<Arc<dyn PlatformAdapter>> = Vec::new();

    let discord_requested = config
        .gateway
        .platform_requested("discord", config.gateway.discord.enabled);
    {
        let disc = &config.gateway.discord;
        let maybe_token = std::env::var(&disc.token_env).ok();
        match maybe_token {
            Some(token) if disc.enabled || discord_requested => {
                adapters.push(Arc::new(
                    edgecrab_gateway::discord::DiscordAdapter::from_token(
                        token,
                        disc.allowed_users.clone(),
                    )?,
                ));
            }
            _ if discord_requested => {
                tracing::warn!(
                    token_env = %disc.token_env,
                    "Discord enabled but token not found — run `edgecrab gateway configure`"
                );
            }
            _ => {}
        }
    }

    let telegram_requested = config
        .gateway
        .platform_requested("telegram", config.gateway.telegram.enabled);
    {
        let tg = &config.gateway.telegram;
        let maybe_token = std::env::var(&tg.token_env).ok();
        match maybe_token {
            Some(token) if tg.enabled || telegram_requested => {
                adapters.push(Arc::new(
                    edgecrab_gateway::telegram::TelegramAdapter::from_token(
                        token,
                        tg.allowed_users.clone(),
                    )?,
                ));
            }
            _ if telegram_requested => {
                tracing::warn!(
                    token_env = %tg.token_env,
                    "Telegram enabled but token not found — run `edgecrab gateway configure`"
                );
            }
            _ => {}
        }
    }

    let slack_requested = config
        .gateway
        .platform_requested("slack", config.gateway.slack.enabled);
    {
        let sl = &config.gateway.slack;
        let maybe_bot = std::env::var(&sl.bot_token_env).ok();
        let maybe_app = std::env::var(&sl.app_token_env).ok();
        match (maybe_bot, maybe_app) {
            (Some(bot), Some(app)) if sl.enabled || slack_requested => {
                adapters.push(Arc::new(
                    edgecrab_gateway::slack::SlackAdapter::from_tokens(
                        bot,
                        app,
                        sl.allowed_users.clone(),
                    )?,
                ));
            }
            _ if slack_requested => {
                tracing::warn!(
                    bot_env = %sl.bot_token_env,
                    app_env = %sl.app_token_env,
                    "Slack enabled but tokens not found — run `edgecrab gateway configure`"
                );
            }
            _ => {}
        }
    }

    if config.gateway.platform_enabled("feishu") {
        match edgecrab_gateway::feishu::FeishuAdapter::from_env() {
            Some(adapter) => adapters.push(Arc::new(adapter)),
            None => tracing::warn!("Feishu requested but configuration is incomplete"),
        }
    }

    if config.gateway.platform_enabled("wecom") {
        match edgecrab_gateway::wecom::WeComAdapter::from_env() {
            Some(adapter) => adapters.push(Arc::new(adapter)),
            None => tracing::warn!("WeCom requested but configuration is incomplete"),
        }
    }

    let signal_requested = config
        .gateway
        .platform_requested("signal", config.gateway.signal.enabled);
    {
        let sig = &config.gateway.signal;
        match (&sig.http_url, &sig.account) {
            (Some(url), Some(account)) if sig.enabled || signal_requested => {
                if let Err(e) = ensure_signal_cli_daemon(url, account) {
                    tracing::warn!(error = %e, "Could not auto-start signal-cli daemon — continuing anyway");
                }
                adapters.push(Arc::new(
                    edgecrab_gateway::signal::SignalAdapter::from_config(
                        url.clone(),
                        account.clone(),
                        sig.allowed_users.clone(),
                    )?,
                ));
            }
            _ if edgecrab_gateway::signal::SignalAdapter::is_available() => {
                adapters.push(Arc::new(edgecrab_gateway::signal::SignalAdapter::new()?));
            }
            _ if signal_requested => {
                tracing::warn!(
                    "Signal enabled but http_url / account not configured — \
                     run `edgecrab gateway configure` to set them up"
                );
            }
            _ => {}
        }
    }

    if config
        .gateway
        .platform_requested("whatsapp", config.gateway.whatsapp.enabled)
    {
        let wa_cfg =
            edgecrab_gateway::whatsapp::WhatsappAdapterConfig::from(&config.gateway.whatsapp);
        if edgecrab_gateway::whatsapp::WhatsAppAdapter::is_available(&wa_cfg) {
            adapters.push(Arc::new(edgecrab_gateway::whatsapp::WhatsAppAdapter::new(
                wa_cfg,
            )?));
        } else {
            tracing::warn!("WhatsApp enabled but bridge assets or Node.js are unavailable");
        }
    }

    let sms_requested = config.gateway.platform_enabled("sms");
    if sms_requested {
        match edgecrab_gateway::sms::SmsAdapter::from_env() {
            Some(adapter) => adapters.push(Arc::new(adapter)),
            None => tracing::warn!("SMS requested but configuration is incomplete"),
        }
    }

    let matrix_requested = config.gateway.platform_enabled("matrix");
    if matrix_requested {
        match edgecrab_gateway::matrix::MatrixAdapter::from_env() {
            Some(adapter) => adapters.push(Arc::new(adapter)),
            None => tracing::warn!("Matrix requested but configuration is incomplete"),
        }
    }

    let mattermost_requested = config.gateway.platform_enabled("mattermost");
    if mattermost_requested {
        match edgecrab_gateway::mattermost::MattermostAdapter::from_env() {
            Some(adapter) => adapters.push(Arc::new(adapter)),
            None => tracing::warn!("Mattermost requested but configuration is incomplete"),
        }
    }

    let dingtalk_requested = config.gateway.platform_enabled("dingtalk");
    if dingtalk_requested {
        match edgecrab_gateway::dingtalk::DingTalkAdapter::from_env() {
            Some(adapter) => adapters.push(Arc::new(adapter)),
            None => tracing::warn!("DingTalk requested but configuration is incomplete"),
        }
    }

    let ha_requested = config.gateway.platform_enabled("homeassistant");
    if ha_requested {
        match edgecrab_gateway::homeassistant::HomeAssistantAdapter::from_env() {
            Some(adapter) => adapters.push(Arc::new(adapter)),
            None => tracing::warn!("Home Assistant requested but configuration is incomplete"),
        }
    }

    let email_requested = config.gateway.platform_enabled("email");
    if email_requested {
        match edgecrab_gateway::email::EmailAdapter::from_env() {
            Some(adapter) => adapters.push(Arc::new(adapter)),
            None => tracing::warn!("Email requested but configuration is incomplete or invalid"),
        }
    }

    Ok(adapters)
}

async fn run_foreground(args: &CliArgs) -> anyhow::Result<()> {
    let runtime = load_runtime(
        args.config.as_deref(),
        args.model.as_deref(),
        args.toolset.as_deref(),
    )?;
    let provider = create_provider(&runtime.config.model.default_model);
    let state_db = open_state_db(&runtime.state_db_path)?;
    let tool_registry = build_tool_registry_with_mcp_discovery(&runtime.config).await;
    let agent = build_agent(
        &runtime,
        provider,
        state_db,
        tool_registry,
        edgecrab_types::Platform::Webhook,
        false,
        None,
    )?;

    let gateway_cfg = edgecrab_gateway::config::GatewayConfig {
        host: runtime.config.gateway.host.clone(),
        port: runtime.config.gateway.port,
        default_model: runtime.config.model.default_model.clone(),
        session_idle_timeout_secs: runtime.config.gateway.session_timeout_minutes as u64 * 60,
        webhook_enabled: runtime.config.gateway.webhook_enabled,
        ..Default::default()
    };
    let cancel = CancellationToken::new();
    let mut gateway = edgecrab_gateway::run::Gateway::new(gateway_cfg.clone(), cancel.clone());
    if gateway_cfg.webhook_enabled {
        gateway.add_adapter(Arc::new(edgecrab_gateway::webhook::WebhookAdapter::new()));
    }

    for adapter in build_outbound_platform_adapters(&runtime.config)? {
        gateway.add_adapter(adapter);
    }

    for diagnostic in collect_platform_diagnostics(&runtime.config)
        .into_iter()
        .filter(|diagnostic| diagnostic.state == PlatformState::Incomplete)
    {
        tracing::warn!(
            platform = diagnostic.id,
            detail = %diagnostic.detail,
            "Gateway platform configuration is incomplete"
        );
    }

    gateway.set_agent(agent.clone());

    // ── API Server ───────────────────────────────────────────────────────
    let api_server_requested = runtime.config.gateway.platform_enabled("api_server");
    if api_server_requested {
        match edgecrab_gateway::api_server::ApiServerAdapter::from_env() {
            Some(adapter) => {
                tracing::info!("API Server adapter enabled (OpenAI-compatible)");
                gateway.add_adapter(Arc::new(adapter));
            }
            None => tracing::warn!("API Server requested but API_SERVER_ENABLED is not true"),
        }
    }

    write_pid(std::process::id())?;
    let ctrl_c = cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        ctrl_c.cancel();
    });

    // Build a cron delivery sender from the registered adapters.
    // Must be called AFTER all add_adapter() calls so every platform is included.
    let cron_sender = gateway.build_sender().await;
    agent.set_gateway_sender(cron_sender.clone()).await;

    let scheduler_args = args.clone();
    let scheduler_cancel = cancel.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(error) = crate::cron_cmd::tick_due_jobs(&scheduler_args, false, Some(cron_sender.clone()), None).await {
                        tracing::warn!(error = %error, "cron tick failed");
                    }
                }
                _ = scheduler_cancel.cancelled() => break,
            }
        }
    });

    let result = gateway.run().await;
    let _ = remove_pid();
    result
}

fn start_background(args: &CliArgs) -> anyhow::Result<()> {
    // Guard: refuse to start a second instance
    if let Ok(status) = snapshot() {
        if status.running {
            if let Some(pid) = status.pid {
                anyhow::bail!(
                    "Gateway is already running (pid {pid}).\n\
                     Run `edgecrab gateway status` for health details,\n\
                     or `edgecrab gateway restart` to roll it cleanly."
                );
            }
        }
    }

    let current_exe = std::env::current_exe().context("cannot resolve current executable")?;
    let log_path = gateway_log_path()?;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    let stderr_file = log_file.try_clone()?;

    let mut cmd = std::process::Command::new(current_exe);
    if let Some(config) = &args.config {
        cmd.arg("--config").arg(config);
    }
    if let Some(model) = &args.model {
        cmd.arg("--model").arg(model);
    }
    if let Some(toolsets) = &args.toolset {
        if !toolsets.is_empty() {
            cmd.arg("--toolset").arg(toolsets.join(","));
        }
    }
    cmd.args(["gateway", "start", "--foreground"]);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::from(log_file));
    cmd.stderr(Stdio::from(stderr_file));

    let child = cmd
        .spawn()
        .context("failed to start gateway background process")?;
    write_pid(child.id())?;
    print_success_panel(child.id(), &log_path, args);
    Ok(())
}

fn stop_background() -> anyhow::Result<()> {
    let pid = read_pid()?;
    // TERM first — give time for clean shutdown
    let _ = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    } else {
        std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };

    // Wait up to 5 seconds for the process to exit, then KILL
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if !process_running(pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    if process_running(pid) {
        // Force-kill if still alive
        if cfg!(not(windows)) {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    remove_pid()?;
    println!("Stopped gateway pid {pid}");
    Ok(())
}

/// Stop the running gateway (if any) then start a fresh background process.
fn restart_background(args: &CliArgs) -> anyhow::Result<()> {
    match snapshot() {
        Ok(status) if status.running => {
            println!("↻ Restarting gateway");
            println!(
                "  Stopping current process (pid {})…",
                status.pid.unwrap_or(0)
            );
            stop_background()?;
            // Brief pause so the OS releases the TCP port before we re-bind it
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        _ => {
            println!("↻ Restart requested, but gateway is not running.");
            println!("  Starting a fresh background process instead.");
        }
    }
    start_background(args)
}

fn status(args: &CliArgs) -> anyhow::Result<()> {
    let status = snapshot()?;
    let runtime = load_gateway_runtime_snapshot(args);

    println!();
    println!("╔══════════════════════════════════════════════╗");
    println!("║   EdgeCrab Gateway — Runtime Status         ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();

    let process_state = if status.running {
        "✓ running"
    } else if status.stale_pid {
        "⚠ stopped (stale pid file cleaned)"
    } else {
        "○ stopped"
    };
    println!("  Process: {process_state}");

    if let Some(pid) = status.pid {
        println!("  PID: {pid}");
    }

    if let Some(rt) = &runtime {
        println!("  Bind: {}", rt.base_url);
        if status.running {
            let health = check_http_health(&format!("{}/health", rt.base_url));
            let health_label = match health {
                Some(true) => "✓ healthy",
                Some(false) => "⚠ not responding",
                None => "○ unknown",
            };
            println!("  HTTP health: {health_label}");
        }

        if !rt.enabled_platforms.is_empty() {
            println!("  Enabled platforms: {}", rt.enabled_platforms.join(", "));
        } else {
            println!("  Enabled platforms: (none)");
        }

        if !rt.attention_items.is_empty() {
            println!();
            println!("  Attention needed:");
            for item in &rt.attention_items {
                println!("    - {item}");
            }
        }

        println!();
        println!("  Diagnostics:");

        let gw_health = if status.running {
            check_http_health(&format!("{}/health", rt.base_url))
        } else {
            None
        };
        print_check_line(
            "gateway_http",
            gw_health,
            &format!("{}/health", rt.base_url),
        );

        print_check_line(
            "gateway_log",
            Some(status.log_path.exists()),
            &status.log_path.display().to_string(),
        );

        if let Some(signal_url) = &rt.signal_http_url {
            let signal_ok = check_http_health(&format!("{}/api/v1/check", signal_url));
            print_check_line("signal_daemon", signal_ok, signal_url);
        }

        let alerts = recent_log_alerts(&status.log_path, 3);
        if !alerts.is_empty() {
            println!();
            println!("  Recent alerts:");
            for alert in alerts {
                println!("    - {alert}");
            }
        }
    }

    println!("  Log file: {}", status.log_path.display());
    println!();
    println!("  Next steps:");
    if status.running {
        println!("    edgecrab gateway restart    ← apply new config safely");
        println!("    edgecrab gateway stop       ← stop background process");
    } else {
        println!("    edgecrab gateway start      ← launch gateway");
    }
    println!("    edgecrab gateway configure  ← manage platform setup");
    println!("    edgecrab gateway --help     ← command reference");
    println!("    tail -f {}", status.log_path.display());

    Ok(())
}

#[derive(Debug, Clone)]
struct GatewayRuntimeSnapshot {
    base_url: String,
    enabled_platforms: Vec<String>,
    attention_items: Vec<String>,
    signal_http_url: Option<String>,
}

fn load_gateway_runtime_snapshot(args: &CliArgs) -> Option<GatewayRuntimeSnapshot> {
    let runtime = load_runtime(
        args.config.as_deref(),
        args.model.as_deref(),
        args.toolset.as_deref(),
    )
    .ok()?;

    Some(build_gateway_runtime_snapshot(&runtime.config))
}

fn build_gateway_runtime_snapshot(config: &edgecrab_core::AppConfig) -> GatewayRuntimeSnapshot {
    let gw = &config.gateway;
    let base_url = format!("http://{}:{}", gw.host, gw.port);
    let diagnostics = collect_platform_diagnostics(config);

    GatewayRuntimeSnapshot {
        base_url,
        enabled_platforms: diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.active)
            .map(|diagnostic| diagnostic.id.to_string())
            .collect(),
        attention_items: diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.state == PlatformState::Incomplete)
            .map(|diagnostic| format!("{}: {}", diagnostic.name, diagnostic.detail))
            .collect(),
        signal_http_url: gw.signal.http_url.clone(),
    }
}

fn check_http_health(url: &str) -> Option<bool> {
    let out = std::process::Command::new("curl")
        .args([
            "--silent",
            "--max-time",
            "2",
            "--output",
            "/dev/null",
            "--write-out",
            "%{http_code}",
            url,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return Some(false);
    }
    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some(code == "200")
}

fn print_check_line(name: &str, state: Option<bool>, detail: &str) {
    let marker = match state {
        Some(true) => "✓",
        Some(false) => "✗",
        None => "○",
    };
    println!("    {marker} {name:<14} {detail}");
}

fn recent_log_alerts(log_path: &std::path::Path, limit: usize) -> Vec<String> {
    let out = std::process::Command::new("tail")
        .args(["-n", "120", &log_path.display().to_string()])
        .output();

    let Ok(out) = out else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut alerts: Vec<String> = text
        .lines()
        .filter(|line| line.contains(" ERROR ") || line.contains(" WARN "))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect();

    if alerts.len() > limit {
        alerts = alerts.split_off(alerts.len() - limit);
    }

    alerts
}

fn print_gateway_failure_guidance(action: GatewayAction, args: &CliArgs, err: &anyhow::Error) {
    eprintln!();
    eprintln!("Gateway command failed: {err}");
    eprintln!();
    eprintln!("Troubleshooting:");
    eprintln!("  1. edgecrab gateway status");
    eprintln!("  2. edgecrab gateway configure");

    if let Ok(s) = snapshot() {
        eprintln!("  3. tail -n 120 {}", s.log_path.display());
    }

    if let Some(rt) = load_gateway_runtime_snapshot(args) {
        eprintln!("  4. curl -s {}/health", rt.base_url);
        if let Some(signal_url) = rt.signal_http_url {
            eprintln!(
                "  5. curl -s {}/api/v1/check",
                signal_url.trim_end_matches('/')
            );
        }
    }

    match action {
        GatewayAction::Start { .. } | GatewayAction::Restart => {
            eprintln!(
                "  Tip: If a stale process owns the port, run `edgecrab gateway stop` then retry."
            );
        }
        GatewayAction::Status => {
            eprintln!(
                "  Tip: If status is stale, run `edgecrab gateway restart` to refresh runtime state."
            );
        }
        GatewayAction::Stop => {}
    }
}

fn print_success_panel(pid: u32, log_path: &std::path::Path, args: &CliArgs) {
    println!();
    println!("✅ Gateway started");
    println!("   PID: {pid}");
    if let Some(rt) = load_gateway_runtime_snapshot(args) {
        println!("   URL: {}", rt.base_url);
        if !rt.attention_items.is_empty() {
            println!("   Attention:");
            for item in rt.attention_items {
                println!("     - {item}");
            }
        }
    }
    println!("   Logs: {}", log_path.display());
    println!("   Next: edgecrab gateway status");
}

pub fn snapshot() -> anyhow::Result<GatewayStatus> {
    let log_path = gateway_log_path()?;
    match read_pid() {
        Ok(pid) => {
            let running = process_running(pid);
            if !running {
                let _ = remove_pid();
            }
            Ok(GatewayStatus {
                pid: Some(pid),
                running,
                stale_pid: !running,
                log_path,
            })
        }
        Err(_) => Ok(GatewayStatus {
            pid: None,
            running: false,
            stale_pid: false,
            log_path,
        }),
    }
}

fn gateway_pid_path() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().context("cannot resolve home directory")?;
    let dir = home.join(".edgecrab");
    std::fs::create_dir_all(dir.join("logs"))?;
    Ok(dir.join("gateway.pid"))
}

fn gateway_log_path() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().context("cannot resolve home directory")?;
    let dir = home.join(".edgecrab").join("logs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("gateway.log"))
}

fn write_pid(pid: u32) -> anyhow::Result<()> {
    std::fs::write(gateway_pid_path()?, pid.to_string()).context("failed to write gateway pid")
}

fn read_pid() -> anyhow::Result<u32> {
    let pid_path = gateway_pid_path()?;
    let pid = std::fs::read_to_string(&pid_path)
        .with_context(|| format!("failed to read {}", pid_path.display()))?;
    pid.trim().parse().context("invalid pid file")
}

fn remove_pid() -> anyhow::Result<()> {
    let path = gateway_pid_path()?;
    if path.exists() {
        std::fs::remove_file(path).context("failed to remove gateway pid file")?;
    }
    Ok(())
}

fn process_running(pid: u32) -> bool {
    if cfg!(windows) {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .output()
            .ok()
            .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
    } else {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .is_some_and(|s| s.success())
    }
}

// ── signal-cli daemon auto-start ─────────────────────────────────────────────

/// Check whether signal-cli HTTP daemon is already reachable at `http_url`.
fn signal_daemon_reachable(http_url: &str) -> bool {
    let check_url = format!("{}/api/v1/check", http_url.trim_end_matches('/'));
    std::process::Command::new("curl")
        .args(["-sf", "--max-time", "2", &check_url])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Launch `signal-cli daemon --http <bind>` as a detached background process,
/// writing output to `~/.edgecrab/logs/signal-cli.log`.
///
/// Does nothing if the daemon is already reachable.
fn ensure_signal_cli_daemon(http_url: &str, account: &str) -> anyhow::Result<()> {
    if signal_daemon_reachable(http_url) {
        tracing::info!("signal-cli daemon already running at {http_url}");
        return Ok(());
    }

    // Derive bind address from http_url (strip scheme).
    let bind = http_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    // Locate signal-cli binary.
    let signal_cli = which_signal_cli().ok_or_else(|| {
        anyhow::anyhow!(
            "signal-cli not found on PATH. Install it from https://github.com/AsamK/signal-cli"
        )
    })?;

    // Build log path.
    let log_dir = home_dir()?.join("logs");
    std::fs::create_dir_all(&log_dir).context("failed to create ~/.edgecrab/logs directory")?;
    let log_path = log_dir.join("signal-cli.log");
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;

    let mut cmd = std::process::Command::new(&signal_cli);
    // Inject a Java 21+ home if available (needed when host JAVA_HOME points to older JDK).
    if let Some(java_home) = detect_java_home_for_signal() {
        cmd.env("JAVA_HOME", java_home);
    }
    cmd.args(["-a", account, "daemon", "--http", bind]);
    cmd.stdin(Stdio::null());
    cmd.stdout(log_file.try_clone()?);
    cmd.stderr(log_file);

    let child = cmd.spawn().context("failed to spawn signal-cli daemon")?;

    // Write PID so we can stop it later.
    let pid_path = home_dir()?.join("signal-cli.pid");
    std::fs::write(&pid_path, child.id().to_string()).context("failed to write signal-cli.pid")?;

    tracing::info!(
        pid = child.id(),
        bind = %bind,
        log = %log_path.display(),
        "signal-cli daemon started"
    );

    // Give it a moment to start accepting connections.
    std::thread::sleep(std::time::Duration::from_millis(1500));

    if !signal_daemon_reachable(http_url) {
        tracing::warn!(
            "signal-cli daemon started (pid {}) but not yet reachable at {http_url} — \
             it may take a few more seconds to be ready",
            child.id()
        );
    }

    Ok(())
}

fn which_signal_cli() -> Option<std::path::PathBuf> {
    // Check common explicit paths first before relying on PATH.
    for candidate in [
        "/opt/homebrew/bin/signal-cli",
        "/usr/local/bin/signal-cli",
        "/usr/bin/signal-cli",
    ] {
        if std::path::Path::new(candidate).exists() {
            return Some(std::path::PathBuf::from(candidate));
        }
    }
    // Fall back to whatever is on PATH.
    std::process::Command::new("which")
        .arg("signal-cli")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| std::path::PathBuf::from(String::from_utf8_lossy(&o.stdout).trim().to_string()))
}

/// Detect a Java 21+ home directory for use with signal-cli.
/// Priority: Homebrew versioned formulae > macOS JVM framework > existing JAVA_HOME (if ≥21).
fn detect_java_home_for_signal() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let check = |home: &str| -> Option<String> {
            let java_bin = format!("{home}/bin/java");
            if !std::path::Path::new(&java_bin).exists() {
                return None;
            }
            let out = std::process::Command::new(&java_bin)
                .arg("-version")
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&out.stderr).to_string()
                + String::from_utf8_lossy(&out.stdout).as_ref();
            let maj = parse_java_major(text.as_str())?;
            if maj >= 21 {
                Some(home.to_string())
            } else {
                None
            }
        };

        // 1. Homebrew versioned formulae (highest first).
        for ver in ["25", "24", "23", "22", "21"] {
            for base in ["/opt/homebrew/opt", "/usr/local/opt"] {
                let p = format!("{base}/openjdk@{ver}/libexec/openjdk.jdk/Contents/Home");
                if let Some(h) = check(&p) {
                    return Some(h);
                }
            }
        }

        // 2. Registered JVMs from macOS JVM framework (/usr/libexec/java_home --xml).
        if let Ok(out) = std::process::Command::new("/usr/libexec/java_home")
            .arg("--xml")
            .output()
        {
            let xml = String::from_utf8_lossy(&out.stdout).to_string();
            let mut best: Option<(u32, String)> = None;
            for line in xml.lines() {
                let t = line.trim();
                if t.starts_with("<string>") && t.contains("/Contents/Home") {
                    let path = t
                        .trim_start_matches("<string>")
                        .trim_end_matches("</string>")
                        .trim()
                        .to_string();
                    if let Some(h) = check(&path) {
                        let java_bin = format!("{h}/bin/java");
                        if let Ok(vo) = std::process::Command::new(&java_bin)
                            .arg("-version")
                            .output()
                        {
                            let vt = String::from_utf8_lossy(&vo.stderr).to_string()
                                + String::from_utf8_lossy(&vo.stdout).as_ref();
                            if let Some(maj) = parse_java_major(&vt) {
                                if best.as_ref().is_none_or(|(bv, _)| maj > *bv) {
                                    best = Some((maj, h));
                                }
                            }
                        }
                    }
                }
            }
            if let Some((_, h)) = best {
                return Some(h);
            }
        }

        // 3. Existing JAVA_HOME if it's ≥ 21.
        if let Ok(existing) = std::env::var("JAVA_HOME") {
            let existing = existing.trim().to_string();
            if !existing.is_empty() {
                if let Some(h) = check(&existing) {
                    return Some(h);
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

#[cfg(target_os = "macos")]
fn parse_java_major(version_output: &str) -> Option<u32> {
    for line in version_output.lines() {
        if line.contains("version") {
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    let ver = &line[start + 1..start + 1 + end];
                    let parts: Vec<&str> = ver.split('.').collect();
                    if let Some(first) = parts.first() {
                        if let Ok(n) = first.parse::<u32>() {
                            if n == 1 {
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

fn home_dir() -> anyhow::Result<std::path::PathBuf> {
    let h = std::env::var("HOME").context("HOME env var not set")?;
    Ok(std::path::PathBuf::from(h).join(".edgecrab"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_core::AppConfig;

    #[test]
    fn process_running_false_for_impossible_pid() {
        assert!(!process_running(999_999));
    }

    #[test]
    fn runtime_snapshot_reports_env_backed_platforms_and_attention() {
        let _guard = crate::gateway_catalog::TEST_ENV_LOCK
            .lock()
            .expect("env lock");
        let config = AppConfig::default();
        unsafe {
            std::env::set_var("MATRIX_HOMESERVER", "https://matrix.example");
            std::env::set_var("MATRIX_ACCESS_TOKEN", "token");
            std::env::set_var("TWILIO_ACCOUNT_SID", "sid");
            std::env::remove_var("TWILIO_AUTH_TOKEN");
            std::env::remove_var("TWILIO_PHONE_NUMBER");
        }

        let snapshot = build_gateway_runtime_snapshot(&config);
        assert!(
            snapshot
                .enabled_platforms
                .iter()
                .any(|platform| platform == "matrix")
        );
        assert!(
            snapshot
                .enabled_platforms
                .iter()
                .any(|platform| platform == "webhook")
        );
        assert!(
            snapshot
                .attention_items
                .iter()
                .any(|item| item.contains("TWILIO_AUTH_TOKEN"))
        );

        unsafe {
            std::env::remove_var("MATRIX_HOMESERVER");
            std::env::remove_var("MATRIX_ACCESS_TOKEN");
            std::env::remove_var("TWILIO_ACCOUNT_SID");
        }
    }

    #[test]
    fn runtime_snapshot_excludes_explicitly_disabled_typed_platforms() {
        let _guard = crate::gateway_catalog::TEST_ENV_LOCK
            .lock()
            .expect("env lock");
        let mut config = AppConfig::default();
        config.gateway.telegram.enabled = true;
        config.gateway.disable_platform("telegram");
        unsafe {
            std::env::set_var("TELEGRAM_BOT_TOKEN", "token");
        }

        let snapshot = build_gateway_runtime_snapshot(&config);
        assert!(
            !snapshot
                .enabled_platforms
                .iter()
                .any(|platform| platform == "telegram")
        );

        unsafe {
            std::env::remove_var("TELEGRAM_BOT_TOKEN");
        }
    }
}
