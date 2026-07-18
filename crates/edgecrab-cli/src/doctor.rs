//! # doctor — Configuration and connectivity diagnostics
//!
//! WHY a doctor command: Users often hit problems because an API key is
//! missing or the config file is malformed. A doctor command gives
//! actionable, colored diagnostic output rather than cryptic errors.
//!
//! ```text
//! edgecrab doctor
//!
//!   Checking configuration...
//!   ✓ Config file:      ~/.edgecrab/config.yaml
//!   ✓ State directory:  ~/.edgecrab/
//!   ✓ Memories:         ~/.edgecrab/memories/ (3 files)
//!   ✗ API key:          OPENAI_API_KEY not set
//!   ✓ GitHub Token:     GITHUB_TOKEN set (Copilot active)
//!   ✓ Provider ping:    copilot/gpt-4.1-mini → ok (140ms)
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use edgecrab_core::{AppConfig, copilot_agent_probe, edgecrab_home};
use edgequake_llm::{ProviderFactory, ProviderType};

use crate::runtime::load_dot_env;

fn copilot_auth_available() -> bool {
    if std::env::var("GITHUB_TOKEN")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("VSCODE_COPILOT_TOKEN")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    {
        return true;
    }

    if let Some(home) = dirs::home_dir() {
        let candidates = [
            home.join(".config/github-copilot/hosts.json"),
            home.join("Library/Application Support/github-copilot/hosts.json"),
            home.join(".config/edgequake/copilot/github_token.json"),
            home.join("Library/Application Support/edgequake/copilot/github_token.json"),
        ];
        return candidates.iter().any(|path| path.exists());
    }

    false
}

/// Result of a single doctor check.
#[derive(Debug)]
pub struct Check {
    pub label: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl Check {
    fn pass(label: &str, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: CheckStatus::Pass,
            detail: detail.into(),
        }
    }
    fn warn(label: &str, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: CheckStatus::Warn,
            detail: detail.into(),
        }
    }
    fn fail(label: &str, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: CheckStatus::Fail,
            detail: detail.into(),
        }
    }
}

/// Run all diagnostic checks and print a report.
///
/// Returns `Ok(true)` if all checks pass/warn, `Ok(false)` if any fail.
pub async fn run(config_override: Option<&str>) -> anyhow::Result<bool> {
    println!("\n🔍 EdgeCrab Doctor — running diagnostics...\n");

    let mut checks = Vec::new();
    let context = DoctorContext::new(config_override);
    load_dot_env(&context.home.join(".env"));

    checks.push(check_config_file(&context.config_path));
    if let Ok(config) = edgecrab_core::AppConfig::load()
        && config.computer_use.enabled
    {
        checks.push(check_computer_use(&config));
    }
    checks.push(check_state_dir(&context.home));
    checks.push(check_memories(&context.home));
    checks.push(check_skills(&context.home));
    if let Ok(config) = edgecrab_core::AppConfig::load() {
        checks.push(check_toolset_policy(&config));
        checks.push(check_schema_mode(&config));
        checks.push(check_prompt_prefix_cache(&config));
        checks.push(check_smart_routing(&config));
        checks.push(check_os_sandbox(&config));
        checks.push(check_os_sandbox_probe(&config));
    }
    checks.extend(check_mcp_servers());
    checks.extend(check_provider_keys());
    checks.extend(check_web_providers());
    checks.push(check_vertexai_adc());
    // macOS FFI permission probing disabled — AEDeterminePermissionToAutomateTarget
    // can hang and create zombie processes on some terminal hosts.
    checks.push(check_provider_ping(&context).await);

    // Termux-specific checks
    if *edgecrab_types::IS_TERMUX {
        checks.push(Check::warn(
            "Termux",
            "Running inside Termux — some features may be unavailable (browser, TTS, STT)",
        ));
        checks.push(check_termux_storage());
    }

    // Print results
    let label_width = checks.iter().map(|c| c.label.len()).max().unwrap_or(20) + 2;

    for check in &checks {
        let icon = match check.status {
            CheckStatus::Pass => "✓",
            CheckStatus::Warn => "⚠",
            CheckStatus::Fail => "✗",
        };
        // Pad label for alignment
        let padded = format!("{}:", check.label);
        println!(
            "  {icon} {padded:<width$} {}",
            check.detail,
            width = label_width
        );
    }

    let failures = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .count();
    let warnings = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Warn)
        .count();

    println!();
    if failures == 0 && warnings == 0 {
        println!("✅ All checks passed — EdgeCrab is ready to use.");
    } else if failures == 0 {
        println!("⚠  {warnings} warning(s) — EdgeCrab should work but review warnings above.");
    } else {
        println!("❌ {failures} failure(s) — fix the issues above before using EdgeCrab.");
        println!("   Run `edgecrab setup` to configure a provider.");
    }
    println!();

    Ok(failures == 0)
}

/// Analyze `harness.jsonl` for spill/perception/exit_reason metrics (spec 015 P2.6).
pub fn run_harness(config_override: Option<&str>) -> anyhow::Result<()> {
    let context = DoctorContext::new(config_override);
    let log_dir = context.home.join("logs");
    println!("\n🔍 EdgeCrab Doctor — harness log analysis\n");
    println!("  Log dir: {}\n", log_dir.display());

    let _ = AppConfig::migrate_profile_preview_from_global(&context.config_path);

    match AppConfig::load_from_with_global_inheritance(&context.config_path) {
        Ok(cfg) if cfg.security.preview.enabled => {
            println!(
                "  ✓ security.preview enabled — ports: {:?}\n",
                cfg.security.preview.allow_localhost_ports
            );
        }
        _ => {
            println!(
                "  ⚠ security.preview disabled — visual tasks cannot browser_navigate localhost.\n\
                 Fix: /config preview on\n\
                 Or add to active profile config.yaml:\n\
                   security:\n\
                     preview:\n\
                       enabled: true\n\
                       allow_localhost_ports: [8000, 8888, 5173, 3000]\n\
                 Note: install-global ~/.edgecrab/config.yaml merges when profile omits preview.\n"
            );
        }
    }

    match edgecrab_core::analyze_harness_file(&log_dir) {
        Ok(report) => {
            println!("{}", edgecrab_core::format_harness_report(&report));
            println!();
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("  ⚠ harness.jsonl not found — run an agent session first.");
            println!("    Expected: {}/harness.jsonl", log_dir.display());
            println!();
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn check_computer_use(config: &edgecrab_core::AppConfig) -> Check {
    use edgecrab_tools::{ComputerUseReportContext, ComputerUseStatusConfig, collect_snapshot};

    let status_cfg = ComputerUseStatusConfig {
        enabled: config.computer_use.enabled,
        keep_last_n_screenshots: config.computer_use.keep_last_n_screenshots,
        confirm_destructive: config.computer_use.confirm_destructive,
        cua_driver_cmd: config.computer_use.cua_driver_cmd.clone(),
    };
    let ctx = ComputerUseReportContext {
        enabled_toolsets: config.tools.enabled_toolsets.clone().unwrap_or_default(),
        disabled_toolsets: config.tools.disabled_toolsets.clone().unwrap_or_default(),
        auxiliary_provider: config.auxiliary.provider.clone(),
        auxiliary_model: config.auxiliary.model.clone(),
        auxiliary_base_url: config.auxiliary.base_url.clone(),
        ..Default::default()
    };
    let snap = collect_snapshot(&status_cfg, &ctx);
    if snap.ready {
        Check::pass(
            "computer_use",
            "ready — driver, toolset, and permissions look good",
        )
    } else if !snap.platform_supported {
        Check::warn(
            "computer_use",
            "enabled in config but this host is not macOS (or noop test backend)",
        )
    } else if !snap.driver_installed {
        Check::fail(
            "computer_use",
            format!(
                "enabled but `{}` not found — run `/computer permissions`",
                snap.driver_cmd
            ),
        )
    } else if !snap.toolset_active {
        Check::warn(
            "computer_use",
            "enabled but computer_use toolset is not active — run `/computer enable`",
        )
    } else {
        Check::warn(
            "computer_use",
            "enabled but Accessibility is not granted — run `/computer open`",
        )
    }
}

fn check_toolset_policy(config: &edgecrab_core::AppConfig) -> Check {
    match &config.tools.enabled_toolsets {
        None => Check::warn(
            "Toolsets",
            "enabled_toolsets unset — all eligible tools load (~15K+ schema tokens). \
             Set tools.enabled_toolsets: [core] in config.yaml (default for new installs)",
        ),
        Some(names) if names.is_empty() => {
            Check::warn("Toolsets", "enabled_toolsets is empty — no tools will load")
        }
        Some(names) if edgecrab_tools::toolsets::contains_all_sentinel(names) => Check::warn(
            "Toolsets",
            "enabled_toolsets includes 'all' — maximum schema payload on every turn",
        ),
        Some(names) => Check::pass("Toolsets", names.join(", ")),
    }
}

fn check_schema_mode(config: &edgecrab_core::AppConfig) -> Check {
    match edgecrab_tools::ToolSchemaMode::parse(&config.tools.schema_mode) {
        edgecrab_tools::ToolSchemaMode::Auto => Check::pass(
            "Schema mode",
            format!(
                "auto — Compact if ≤{} enabled tools, else Indexed (hot + tool_search)",
                edgecrab_tools::AUTO_INDEXED_TOOL_COUNT_THRESHOLD
            ),
        ),
        edgecrab_tools::ToolSchemaMode::Indexed => Check::pass(
            "Schema mode",
            "indexed — hot tools on wire; /context budget shows wire:N deferred:M",
        ),
        edgecrab_tools::ToolSchemaMode::Compact => Check::pass(
            "Schema mode",
            "compact — all enabled tools on wire with shortened descriptions",
        ),
        edgecrab_tools::ToolSchemaMode::Full => Check::pass(
            "Schema mode",
            "full — all enabled tools on wire with verbose descriptions",
        ),
    }
}

fn check_prompt_prefix_cache(config: &edgecrab_core::AppConfig) -> Check {
    let model = config.model.default_model.trim();
    let (provider, model_id) = model.split_once('/').unwrap_or((model, ""));
    let supports = edgecrab_core::prompt_cache_policy::provider_supports_prompt_caching(
        provider, model_id, None,
    );
    let prefix = &config.cache.prompt_prefix;
    let schema_note = match config.tools.schema_mode.as_str() {
        "indexed" => "indexed tools (hot≤5) keep prefix stable",
        other => other,
    };
    match (supports, prefix.enabled) {
        (true, true) => Check::pass(
            "Prompt cache",
            format!(
                "enabled (ttl={}, warm_on_start={}); SLO ≥70% hit after 3+ turns — {schema_note}; watch via /context budget or /cost",
                prefix.ttl, prefix.warm_on_start
            ),
        ),
        (true, false) => Check::warn(
            "Prompt cache",
            "provider supports caching but cache.prompt_prefix.enabled=false — lead meter Cost/turn degraded",
        ),
        (false, _) => Check::pass(
            "Prompt cache",
            format!(
                "not applicable for configured provider ({schema_note}; local KV normalize still applies for lmstudio/ollama)"
            ),
        ),
    }
}

fn check_smart_routing(config: &edgecrab_core::AppConfig) -> Check {
    let sr = &config.model.smart_routing;
    if sr.enabled {
        if sr.cheap_model.trim().is_empty() {
            Check::warn(
                "Smart routing",
                "model.smart_routing.enabled=true but cheap_model is empty — set via /pareto or config",
            )
        } else {
            Check::pass(
                "Smart routing",
                format!(
                    "enabled (cheap_model={}); simple turns route to cheap model — stats in /cost",
                    sr.cheap_model
                ),
            )
        }
    } else {
        Check::pass(
            "Smart routing",
            "disabled (set model.smart_routing.enabled: true for Pareto savings)",
        )
    }
}

/// Soft sandbox: mode ≠ off but deny_default=false (018 P0 trust cliff).
pub fn check_os_sandbox(config: &edgecrab_core::AppConfig) -> Check {
    let mode = config.security.os_sandbox.mode.trim().to_ascii_lowercase();
    let deny_default = config.security.os_sandbox.deny_default;
    match mode.as_str() {
        "off" | "" => Check::pass(
            "OS sandbox",
            "off — local terminal runs unsandboxed (set security.os_sandbox.mode + deny_default for hard profiles)",
        ),
        "seatbelt" | "sandbox-exec" | "bubblewrap" | "bwrap" if !deny_default => Check::warn(
            "OS sandbox",
            format!(
                "soft sandbox (mode={mode}, deny_default=false) — allow-default profile; set security.os_sandbox.deny_default: true for deny-default trust"
            ),
        ),
        other => Check::pass(
            "OS sandbox",
            format!("mode={other}, deny_default={deny_default}"),
        ),
    }
}

/// Probe deny-default wrap for a harmless command (Wave D — measure before Trust L).
pub fn check_os_sandbox_probe(config: &edgecrab_core::AppConfig) -> Check {
    use edgecrab_security::{OsSandboxMode, wrap_command};
    use std::path::Path;

    let mode = OsSandboxMode::parse(&config.security.os_sandbox.mode);
    if mode == OsSandboxMode::Off {
        return Check::pass(
            "OS sandbox probe",
            "skipped — mode=off (enable seatbelt/bubblewrap + deny_default to probe hard profiles)",
        );
    }
    let deny_default = config.security.os_sandbox.deny_default;
    let wrapped = wrap_command(mode, "echo edgecrab-sandbox-probe", Path::new("/tmp"), true, deny_default);
    if wrapped.passthrough {
        return Check::warn(
            "OS sandbox probe",
            format!(
                "sandbox binary missing — passthrough ({})",
                wrapped.warning.unwrap_or_else(|| "no warning".into())
            ),
        );
    }
    if deny_default {
        let profile_ok = wrapped
            .args
            .iter()
            .any(|a| a.contains("(deny default)") || a == "--unshare-user");
        if profile_ok {
            Check::pass(
                "OS sandbox probe",
                format!("deny-default wrap ready (program={})", wrapped.program),
            )
        } else {
            Check::warn(
                "OS sandbox probe",
                "deny_default=true but wrap profile missing expected deny-default markers",
            )
        }
    } else {
        Check::warn(
            "OS sandbox probe",
            "soft wrap only (deny_default=false) — Trust meter stays P until deny_default opt-in",
        )
    }
}

/// Warn when a live session has enough turns but cache hit rate is below 70%.
pub fn check_cache_hit_rate_slo(
    caching_enabled: bool,
    api_call_count: u32,
    hit_rate_pct: Option<f64>,
) -> Option<Check> {
    if !caching_enabled || api_call_count < 3 {
        return None;
    }
    match hit_rate_pct {
        Some(rate) if rate < 70.0 => Some(Check::warn(
            "Cache hit rate",
            format!(
                "{rate:.0}% after {api_call_count} API turns (SLO ≥70%) — check stable/semi prefixes and provider matrix"
            ),
        )),
        Some(rate) => Some(Check::pass(
            "Cache hit rate",
            format!("{rate:.0}% after {api_call_count} API turns (SLO ≥70%)"),
        )),
        None => Some(Check::warn(
            "Cache hit rate",
            format!(
                "no cache_read tokens after {api_call_count} API turns — prefix may be busting every turn"
            ),
        )),
    }
}

fn check_config_file(home: &Path) -> Check {
    let config_path = if home.is_dir() {
        home.join("config.yaml")
    } else {
        home.to_path_buf()
    };
    if config_path.exists() {
        // Try to parse it
        match std::fs::read_to_string(&config_path) {
            Ok(content) if !content.trim().is_empty() => {
                Check::pass("Config file", format!("{}", config_path.display()))
            }
            Ok(_) => Check::warn("Config file", format!("{} (empty!)", config_path.display())),
            Err(e) => Check::fail("Config file", format!("unreadable: {e}")),
        }
    } else {
        Check::warn(
            "Config file",
            format!("{} not found — run `edgecrab setup`", config_path.display()),
        )
    }
}

fn check_state_dir(home: &Path) -> Check {
    if home.exists() {
        // Check writability by attempting to write a temp file
        let probe = home.join(".edgecrab_probe");
        match std::fs::write(&probe, "ok") {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                Check::pass("State directory", format!("{}", home.display()))
            }
            Err(e) => Check::fail("State directory", format!("not writable: {e}")),
        }
    } else {
        // Directory doesn't exist yet — that's okay, setup will create it
        Check::warn(
            "State directory",
            format!("{} will be created on first run", home.display()),
        )
    }
}

fn check_memories(home: &Path) -> Check {
    let mem_dir = home.join("memories");
    if mem_dir.exists() {
        let count = std::fs::read_dir(&mem_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).count())
            .unwrap_or(0);
        Check::pass("Memories", format!("{} ({count} files)", mem_dir.display()))
    } else {
        Check::warn(
            "Memories",
            format!("{} not found — will be created", mem_dir.display()),
        )
    }
}

fn check_skills(home: &Path) -> Check {
    let skills_dir = home.join("skills");
    if skills_dir.exists() {
        let count = count_installed_skills(&skills_dir);
        if count == 0 {
            Check::warn(
                "Skills",
                format!(
                    "{} exists but contains no installed skills",
                    skills_dir.display()
                ),
            )
        } else {
            Check::pass(
                "Skills",
                format!("{} ({count} installed skills)", skills_dir.display()),
            )
        }
    } else {
        Check::warn(
            "Skills",
            format!("{} not found — will be created", skills_dir.display()),
        )
    }
}

fn count_installed_skills(root: &Path) -> usize {
    if !root.is_dir() {
        return 0;
    }

    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
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
            if path.join("SKILL.md").is_file() {
                count += 1;
            } else {
                stack.push(path);
            }
        }
    }

    count
}

fn check_mcp_servers() -> Vec<Check> {
    let servers = match edgecrab_tools::tools::mcp_client::configured_servers() {
        Ok(servers) => servers,
        Err(edgecrab_types::ToolError::Unavailable { .. }) => {
            return vec![Check::warn("MCP", "no MCP servers configured")];
        }
        Err(err) => {
            return vec![Check::fail("MCP", format!("configuration error: {err}"))];
        }
    };

    if servers.is_empty() {
        return vec![Check::warn("MCP", "no MCP servers configured")];
    }

    let mut checks = Vec::new();
    checks.push(Check::pass(
        "MCP",
        format!("{} configured server(s)", servers.len()),
    ));

    for server in servers {
        let label = format!("MCP {}", server.name);
        if let Some(url) = &server.url {
            let token_ok = server.token_from_config || server.token_from_store;
            let detail = if token_ok {
                format!("HTTP {} (auth configured)", url)
            } else {
                format!("HTTP {} (no auth token configured)", url)
            };
            checks.push(if token_ok {
                Check::pass(&label, detail)
            } else {
                Check::warn(&label, detail)
            });
            continue;
        }

        if server.command.trim().is_empty() {
            checks.push(Check::fail(&label, "stdio server missing command"));
            continue;
        }

        match which::which(&server.command) {
            Ok(path) => {
                let mut detail = format!("{} found at {}", server.command, path.display());
                if let Some(cwd) = &server.cwd {
                    detail.push_str(&format!(" | cwd={}", cwd.display()));
                }
                checks.push(Check::pass(&label, detail));
            }
            Err(_) => checks.push(Check::fail(
                &label,
                format!("command '{}' not found on PATH", server.command),
            )),
        }
    }

    checks
}

/// Check VertexAI Application Default Credentials (ADC) and project setup.
///
/// WHY dedicated check: GOOGLE_CLOUD_PROJECT is NOT exported automatically by
/// `gcloud auth login`. Users must set it explicitly or rely on EdgeCrab's
/// auto-detection from `gcloud config get-value project`. This check surfaces
/// misconfiguration early so the user isn't left with a silent MockProvider fallback.
fn check_vertexai_adc() -> Check {
    // 1. Is GOOGLE_CLOUD_PROJECT already set in environment?
    if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT")
        && !project.is_empty()
    {
        // 2. Verify ADC credentials file exists
        let adc_file = dirs_home().map(|h| {
            h.join(".config")
                .join("gcloud")
                .join("application_default_credentials.json")
        });
        let adc_ok = adc_file.as_ref().map(|p| p.exists()).unwrap_or(false);
        return if adc_ok {
            Check::pass(
                "VertexAI ADC",
                format!("project={project}, ADC credentials found — ready"),
            )
        } else {
            Check::warn(
                "VertexAI ADC",
                format!(
                    "project={project} set but no ADC credentials found; \
                         run `gcloud auth application-default login`"
                ),
            )
        };
    }

    // 3. Try gcloud config to detect the project
    match std::process::Command::new("gcloud")
        .args(["config", "get-value", "project"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let raw = String::from_utf8_lossy(&output.stdout);
            let project = raw.trim();
            if project.is_empty() || project == "(unset)" {
                Check::warn(
                    "VertexAI ADC",
                    "gcloud found but no project configured; run \
                     `gcloud config set project <your-project-id>` or \
                     export GOOGLE_CLOUD_PROJECT=<id>",
                )
            } else {
                // Check ADC credentials file
                let adc_ok = dirs_home()
                    .map(|h| {
                        h.join(".config")
                            .join("gcloud")
                            .join("application_default_credentials.json")
                            .exists()
                    })
                    .unwrap_or(false);
                if adc_ok {
                    Check::pass(
                        "VertexAI ADC",
                        format!(
                            "project={project} (via gcloud config), ADC credentials found — \
                             set GOOGLE_CLOUD_PROJECT={project} or use vertexai/<model>"
                        ),
                    )
                } else {
                    Check::warn(
                        "VertexAI ADC",
                        format!(
                            "project={project} (via gcloud config) but no ADC credentials; \
                             run `gcloud auth application-default login`"
                        ),
                    )
                }
            }
        }
        Ok(_) => Check::warn(
            "VertexAI ADC",
            "gcloud exited with error; VertexAI provider unavailable",
        ),
        Err(_) => Check::warn(
            "VertexAI ADC",
            "gcloud not in PATH — VertexAI provider unavailable (install Google Cloud SDK)",
        ),
    }
}

/// Return the user's home directory for ADC path resolution.
fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}

/// Check known provider API keys in environment.
///
/// WHY multiple checks: Users may have some keys set and others not.
/// We report each separately for clarity.
fn check_provider_keys() -> Vec<Check> {
    let providers = [
        (
            "GITHUB_TOKEN",
            "GitHub Copilot (env token or VS Code auth cache)",
        ),
        ("OPENAI_API_KEY", "OpenAI"),
        ("ANTHROPIC_API_KEY", "Anthropic"),
        ("ANTHROPIC_AUTH_TOKEN", "Anthropic-compatible"),
        ("GOOGLE_API_KEY", "Google Gemini"),
        ("NVIDIA_API_KEY", "NVIDIA NIM"),
        ("OPENROUTER_API_KEY", "OpenRouter"),
        ("XAI_API_KEY", "xAI Grok"),
        ("MISTRAL_API_KEY", "Mistral AI"),
        ("DEEPSEEK_API_KEY", "DeepSeek"),
        ("GROQ_API_KEY", "Groq"),
    ];

    let found: Vec<_> = providers
        .iter()
        .filter(|(env, _)| {
            if *env == "GITHUB_TOKEN" {
                copilot_auth_available()
            } else {
                std::env::var(env)
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false)
            }
        })
        .collect();

    if found.is_empty() {
        // Also check for local providers
        let ollama_up = std::env::var("OLLAMA_HOST").is_ok() || check_local_port(11434);
        let lmstudio_up = check_local_port(1234);

        let mut checks = vec![Check::warn(
            "API keys",
            "no provider key set — see `edgecrab setup`",
        )];
        if ollama_up {
            checks.push(Check::pass("Ollama", "running on localhost:11434"));
        }
        if lmstudio_up {
            checks.push(Check::pass("LMStudio", "running on localhost:1234"));
        }
        checks
    } else {
        found
            .iter()
            .map(|(env, name)| {
                // Show partially redacted key for verification
                let val = std::env::var(env).unwrap_or_default();
                let preview = if val.len() > 8 {
                    let head = edgecrab_core::safe_truncate(&val, 4);
                    let tail_start =
                        edgecrab_core::safe_char_start(&val, val.len().saturating_sub(4));
                    format!("{head}...{}", &val[tail_start..])
                } else {
                    "****".to_string()
                };
                Check::pass("API key", format!("{name} [{preview}]"))
            })
            .collect()
    }
}

/// Web search / extract provider readiness (Hermes tools picker + credential parity).
fn check_web_providers() -> Vec<Check> {
    use edgecrab_tools::{
        collect_web_diagnostics, format_extract_doctor_detail, format_search_doctor_detail,
    };

    let report = collect_web_diagnostics();
    let search_detail = format_search_doctor_detail(&report);
    let extract_detail = format_extract_doctor_detail(&report);

    let search_check = if !report.search_ready {
        Check::fail("Web search", search_detail)
    } else if report.configured_search_override.is_some()
        && report.resolved_search_backend.is_none()
    {
        Check::warn(
            "Web search",
            format!("{search_detail} — configured override unavailable"),
        )
    } else {
        Check::pass("Web search", search_detail)
    };

    let extract_check = if report.configured_extract_override.is_some()
        && report.resolved_extract_backend.is_none()
    {
        Check::warn(
            "Web extract",
            format!("{extract_detail} — configured override unavailable"),
        )
    } else if report.paid_extract_configured {
        Check::pass("Web extract", extract_detail)
    } else {
        Check::warn("Web extract", extract_detail)
    };

    vec![search_check, extract_check]
}

/// Check if a local TCP port is listening (for Ollama/LMStudio detection).
fn check_local_port(port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let addr = format!("127.0.0.1:{port}");
    addr.to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_millis(200)).ok())
        .is_some()
}

/// Ping the configured (or best available) provider with a trivial request.
///
/// WHY async: We are already inside a tokio runtime (called from #[tokio::main]).
/// Creating a nested runtime with block_on would panic. Using async/await propagates
/// naturally through the call stack.
#[derive(Debug, Clone)]
struct DoctorContext {
    home: PathBuf,
    config_path: PathBuf,
}

impl DoctorContext {
    fn new(config_override: Option<&str>) -> Self {
        let config_path = config_override
            .map(PathBuf::from)
            .unwrap_or_else(|| edgecrab_home().join("config.yaml"));
        let home = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(edgecrab_home);
        Self { home, config_path }
    }
}

async fn check_provider_ping(context: &DoctorContext) -> Check {
    let configured_model = configured_model(&context.config_path);
    let provider_str = configured_model
        .as_deref()
        .map(describe_configured_provider)
        .unwrap_or_else(detect_best_provider);

    let Some(model) = configured_model else {
        return Check::warn(
            "Provider ping",
            "no provider configured — running in offline/mock mode",
        );
    };

    let Some(provider) = configured_provider(&model) else {
        return Check::warn(
            "Provider ping",
            format!("{provider_str} → unsupported configured provider"),
        );
    };

    let agent_tools_probe = provider == ProviderType::VsCodeCopilot;
    let check_label = if agent_tools_probe {
        "Provider ping (agent tools)"
    } else {
        "Provider ping"
    };

    let start = Instant::now();
    let result: anyhow::Result<()> = async {
        let (_, model_name) = split_model_identifier(&model);
        let (llm, _) = ProviderFactory::create_with_model(provider, model_name.as_deref())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if agent_tools_probe {
            copilot_agent_probe::probe_agent_chat_with_tools(llm.as_ref())
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        } else {
            llm.complete("ping")
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Ok(())
    }
    .await;

    let elapsed = start.elapsed();

    match result {
        Ok(()) => Check::pass(
            check_label,
            format!(
                "{provider_str} → ok ({:.0}ms)",
                elapsed.as_secs_f64() * 1000.0
            ),
        ),
        Err(e) => {
            let display =
                if copilot_agent_probe::is_copilot_model_not_supported_error(&e.to_string()) {
                    format!(
                        "{provider_str} → {e} {}",
                        copilot_agent_probe::MODEL_NOT_SUPPORTED_HINT
                    )
                } else {
                    format!("{provider_str} → {e}")
                };
            if is_configuration_gap(&e) {
                Check::warn(check_label, format!("{provider_str} → not tested ({e})"))
            } else {
                Check::fail(check_label, display)
            }
        }
    }
}

fn configured_model(config_path: &Path) -> Option<String> {
    let config = AppConfig::load_from(config_path).ok()?;
    let model = config.model.default_model.trim();
    if model.is_empty() {
        None
    } else {
        Some(model.to_string())
    }
}

fn describe_configured_provider(model: &str) -> String {
    let (provider, model_name) = split_model_identifier(model);
    match model_name {
        Some(model_name) => format!("{provider}/{model_name}"),
        None => provider,
    }
}

fn configured_provider(model: &str) -> Option<ProviderType> {
    let (provider, _) = split_model_identifier(model);
    let canonical = edgecrab_tools::vision_models::normalize_provider_name(&provider);
    ProviderType::from_str(&canonical)
}

fn split_model_identifier(model: &str) -> (String, Option<String>) {
    match model.split_once('/') {
        Some((provider, model_name)) => (
            provider.trim().to_string(),
            Some(model_name.trim().to_string()),
        ),
        None => (model.trim().to_string(), None),
    }
}

fn is_configuration_gap(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    [
        "not set",
        "missing",
        "required",
        "credentials",
        "api key",
        "project",
        "endpoint",
        "deployment",
        "not configured",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// Determine which provider would be used based on env vars.
fn detect_best_provider() -> String {
    if std::env::var("GITHUB_TOKEN")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        "copilot".into()
    } else if std::env::var("OPENAI_API_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        "openai".into()
    } else if std::env::var("ANTHROPIC_API_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("ANTHROPIC_AUTH_TOKEN")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    {
        "anthropic".into()
    } else if check_local_port(11434) {
        "ollama (local)".into()
    } else {
        "none (mock)".into()
    }
}

/// Termux: check if shared storage has been set up.
fn check_termux_storage() -> Check {
    let storage = std::path::PathBuf::from(
        std::env::var("HOME").unwrap_or_else(|_| "/data/data/com.termux/files/home".into()),
    )
    .join("storage");
    if storage.exists() {
        Check::pass(
            "Termux storage",
            "~/storage linked (termux-setup-storage was run)",
        )
    } else {
        Check::warn(
            "Termux storage",
            "~/storage not found — run `termux-setup-storage` to access device files",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use edgecrab_tools::macos_permissions::MacosConsentState;
    use tempfile::TempDir;

    #[cfg(target_os = "macos")]
    fn permission_check(label: &str, state: MacosConsentState, remedy: &str) -> Check {
        match state {
            MacosConsentState::Granted => Check::pass(label, "granted"),
            MacosConsentState::Denied => {
                Check::warn(label, format!("cached TCC state is denied — {remedy}"))
            }
            MacosConsentState::WouldPrompt => {
                Check::warn(label, format!("macOS would prompt on first use — {remedy}"))
            }
            MacosConsentState::Unknown => {
                Check::warn(label, format!("consent state unknown — {remedy}"))
            }
        }
    }

    #[test]
    fn check_state_dir_nonexistent() {
        let tmp = TempDir::new().expect("tmp");
        let nonexistent = tmp.path().join("does_not_exist");
        let check = check_state_dir(&nonexistent);
        assert_eq!(check.status, CheckStatus::Warn);
    }

    #[test]
    fn check_state_dir_exists() {
        let tmp = TempDir::new().expect("tmp");
        let check = check_state_dir(tmp.path());
        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[test]
    fn check_config_missing() {
        let tmp = TempDir::new().expect("tmp");
        let check = check_config_file(tmp.path());
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("not found"));
    }

    #[test]
    fn check_config_present() {
        let tmp = TempDir::new().expect("tmp");
        let home = tmp.path().to_path_buf();
        std::fs::write(home.join("config.yaml"), "model:\n  default_model: test\n").expect("write");
        let check = check_config_file(&home);
        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[test]
    fn ha25_doctor_harness_reports_spill_gap() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("logs")).expect("logs dir");
        std::fs::write(
            tmp.path()
                .join("logs")
                .join(edgecrab_core::HARNESS_JSON_LOG_NAME),
            "INFO tool_name=read_file harness: tool start\nINFO tool result spilled to artifact\n",
        )
        .expect("write harness");
        let report =
            edgecrab_core::analyze_harness_file(&tmp.path().join("logs")).expect("analyze");
        assert!(report.spill_events >= 1);
        assert_eq!(report.spill_without_read, 1);
    }

    #[test]
    fn check_memories_missing() {
        let tmp = TempDir::new().expect("tmp");
        let check = check_memories(tmp.path());
        assert_eq!(check.status, CheckStatus::Warn);
    }

    #[test]
    fn check_memories_present() {
        let tmp = TempDir::new().expect("tmp");
        std::fs::create_dir(tmp.path().join("memories")).expect("mkdir");
        let check = check_memories(tmp.path());
        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[test]
    fn check_web_providers_reports_search_and_extract() {
        let checks = check_web_providers();
        assert_eq!(checks.len(), 2);
        assert!(checks.iter().any(|c| c.label == "Web search"));
        assert!(checks.iter().any(|c| c.label == "Web extract"));
        // ddgs makes search_ready true even without API keys
        assert!(
            checks
                .iter()
                .find(|c| c.label == "Web search")
                .map(|c| c.status == CheckStatus::Pass)
                .unwrap_or(false)
        );
    }

    #[test]
    fn check_provider_keys_no_keys() {
        // Remove all provider keys temporarily (or just check the function
        // runs without panic — we can't guarantee env state in CI)
        let checks = check_provider_keys();
        assert!(!checks.is_empty());
    }

    #[test]
    #[serial_test::serial(edgecrab_home_env)]
    fn check_mcp_servers_warns_when_absent() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let tmp = TempDir::new().expect("tmp");
        // SAFETY: protected by TEST_ENV_LOCK.
        unsafe { std::env::set_var("EDGECRAB_HOME", tmp.path()) };
        let checks = check_mcp_servers();
        // SAFETY: protected by TEST_ENV_LOCK.
        unsafe { std::env::remove_var("EDGECRAB_HOME") };

        assert_eq!(checks[0].status, CheckStatus::Warn);
    }

    #[test]
    #[serial_test::serial(edgecrab_home_env)]
    fn check_mcp_servers_reports_configured_stdio_server() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let tmp = TempDir::new().expect("tmp");
        std::fs::write(
            tmp.path().join("config.yaml"),
            "mcp_servers:\n  fetch:\n    command: sh\n    args: ['-c', 'exit 0']\n    enabled: true\n",
        )
        .expect("write config");
        // SAFETY: protected by TEST_ENV_LOCK.
        unsafe { std::env::set_var("EDGECRAB_HOME", tmp.path()) };
        let checks = check_mcp_servers();
        // SAFETY: protected by TEST_ENV_LOCK.
        unsafe { std::env::remove_var("EDGECRAB_HOME") };

        assert!(checks.iter().any(|check| check.label == "MCP"));
        assert!(checks.iter().any(|check| check.label == "MCP fetch"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn permission_check_maps_granted_to_pass() {
        let check = permission_check("Accessibility", MacosConsentState::Granted, "fix it");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn permission_check_maps_denied_to_warn() {
        let check = permission_check("Accessibility", MacosConsentState::Denied, "fix it");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("cached TCC state"));
    }

    #[test]
    fn cache_hit_rate_slo_warns_below_seventy() {
        let warn = check_cache_hit_rate_slo(true, 5, Some(40.0)).expect("check");
        assert_eq!(warn.status, CheckStatus::Warn);
        let pass = check_cache_hit_rate_slo(true, 5, Some(85.0)).expect("check");
        assert_eq!(pass.status, CheckStatus::Pass);
        assert!(check_cache_hit_rate_slo(true, 2, Some(10.0)).is_none());
    }

    #[test]
    fn os_sandbox_soft_mode_warns() {
        let mut cfg = edgecrab_core::AppConfig::default();
        cfg.security.os_sandbox.mode = "seatbelt".into();
        cfg.security.os_sandbox.deny_default = false;
        let check = check_os_sandbox(&cfg);
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("soft sandbox"));

        cfg.security.os_sandbox.deny_default = true;
        let hard = check_os_sandbox(&cfg);
        assert_eq!(hard.status, CheckStatus::Pass);

        cfg.security.os_sandbox.mode = "off".into();
        cfg.security.os_sandbox.deny_default = false;
        let off = check_os_sandbox(&cfg);
        assert_eq!(off.status, CheckStatus::Pass);
    }

    #[test]
    fn os_sandbox_probe_skips_when_off() {
        let cfg = edgecrab_core::AppConfig::default();
        let check = check_os_sandbox_probe(&cfg);
        assert_eq!(check.status, CheckStatus::Pass);
        assert!(check.detail.contains("skipped"));
    }

    #[test]
    fn os_sandbox_probe_warns_on_soft_wrap() {
        let mut cfg = edgecrab_core::AppConfig::default();
        cfg.security.os_sandbox.mode = "seatbelt".into();
        cfg.security.os_sandbox.deny_default = false;
        let check = check_os_sandbox_probe(&cfg);
        // Missing binary → warn passthrough; present binary → soft wrap warn.
        assert!(
            matches!(check.status, CheckStatus::Warn | CheckStatus::Pass),
            "{:?}",
            check
        );
    }

    #[test]
    fn is_termux_false_on_desktop() {
        // In normal CI/dev, TERMUX_VERSION is not set
        if std::env::var("TERMUX_VERSION").is_err() {
            assert!(!edgecrab_types::is_termux());
        }
    }

    #[test]
    fn check_termux_storage_returns_check() {
        let check = check_termux_storage();
        // On desktop: ~/storage almost certainly doesn't exist → warn
        // On Termux with setup-storage: exists → pass
        assert!(check.status == CheckStatus::Pass || check.status == CheckStatus::Warn);
    }
}
