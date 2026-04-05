use crate::cli_args::CliArgs;
use crate::cron_cmd;
use crate::gateway_cmd;
use crate::plugins::PluginManager;
use crate::profile::ProfileManager;
use crate::runtime::{build_tool_registry, load_runtime, open_state_db};

pub fn run(args: &CliArgs) -> anyhow::Result<()> {
    let runtime = load_runtime(
        args.config.as_deref(),
        args.model.as_deref(),
        args.toolset.as_deref(),
    )?;
    let db = open_state_db(&runtime.state_db_path)?;
    let sessions = db.list_sessions(500)?;
    let gateway = gateway_cmd::snapshot()?;
    let cron = cron_cmd::status_snapshot()?;

    let mut plugins = PluginManager::new();
    plugins.discover_all();
    let tools = build_tool_registry();
    let active_profile = ProfileManager::new().active();

    println!("EdgeCrab status");
    println!("Profile: {}", active_profile);
    println!("Config: {}", runtime.config_path.display());
    println!("State DB: {}", runtime.state_db_path.display());
    println!("Model: {}", runtime.config.model.default_model);
    println!("Toolsets: {}", tools.toolset_names().len());
    println!("Sessions: {}", sessions.len());
    println!("Plugins: {}", plugins.plugins().len());
    println!(
        "Gateway platforms: {}",
        if runtime.config.gateway.enabled_platforms.is_empty() {
            "(none)".to_string()
        } else {
            runtime.config.gateway.enabled_platforms.join(", ")
        }
    );
    println!(
        "Gateway: {}",
        if gateway.running {
            format!("running (pid {})", gateway.pid.unwrap_or_default())
        } else if gateway.stale_pid {
            "stopped (stale pid file cleared)".to_string()
        } else {
            "stopped".to_string()
        }
    );
    println!("Gateway log: {}", gateway.log_path.display());
    println!(
        "Cron: {} total, {} active, next={}",
        cron.total_jobs,
        cron.active_jobs,
        format_timestamp(cron.next_run_at)
    );

    Ok(())
}

fn format_timestamp(ts: Option<i64>) -> String {
    ts.and_then(|value| chrono::DateTime::<chrono::Utc>::from_timestamp(value, 0))
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "-".to_string())
}
