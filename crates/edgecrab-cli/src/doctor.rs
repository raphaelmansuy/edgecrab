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

use edgecrab_core::{AppConfig, edgecrab_home};
#[cfg(target_os = "macos")]
use edgecrab_tools::macos_permissions::MacosConsentState;
use edgequake_llm::{ProviderFactory, ProviderType};

#[cfg(target_os = "macos")]
use crate::permissions::collect_permission_snapshot;
use crate::runtime::load_dot_env;

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
    checks.push(check_state_dir(&context.home));
    checks.push(check_memories(&context.home));
    checks.push(check_skills(&context.home));
    checks.extend(check_provider_keys());
    checks.push(check_vertexai_adc());
    #[cfg(target_os = "macos")]
    checks.extend(check_macos_permissions());
    checks.push(check_provider_ping(&context).await);

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
        let count = std::fs::read_dir(&skills_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).count())
            .unwrap_or(0);
        Check::pass(
            "Skills",
            format!("{} ({count} entries)", skills_dir.display()),
        )
    } else {
        Check::warn(
            "Skills",
            format!("{} not found — will be created", skills_dir.display()),
        )
    }
}

/// Check VertexAI Application Default Credentials (ADC) and project setup.
///
/// WHY dedicated check: GOOGLE_CLOUD_PROJECT is NOT exported automatically by
/// `gcloud auth login`. Users must set it explicitly or rely on EdgeCrab's
/// auto-detection from `gcloud config get-value project`. This check surfaces
/// misconfiguration early so the user isn't left with a silent MockProvider fallback.
fn check_vertexai_adc() -> Check {
    // 1. Is GOOGLE_CLOUD_PROJECT already set in environment?
    if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
        if !project.is_empty() {
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
        ("GITHUB_TOKEN", "GitHub Copilot (copilot/gpt-4.1-mini)"),
        ("OPENAI_API_KEY", "OpenAI"),
        ("ANTHROPIC_API_KEY", "Anthropic"),
        ("GOOGLE_API_KEY", "Google Gemini"),
        ("OPENROUTER_API_KEY", "OpenRouter"),
        ("XAI_API_KEY", "xAI Grok"),
        ("MISTRAL_API_KEY", "Mistral AI"),
    ];

    let found: Vec<_> = providers
        .iter()
        .filter(|(env, _)| std::env::var(env).map(|v| !v.is_empty()).unwrap_or(false))
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

    let start = Instant::now();
    let result: anyhow::Result<String> = async {
        let (_, model_name) = split_model_identifier(&model);
        let (llm, _) = ProviderFactory::create_with_model(provider, model_name.as_deref())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        // Use the simple `complete` API (takes &str prompt directly)
        let resp = llm
            .complete("ping")
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(resp.content.chars().take(40).collect())
    }
    .await;

    let elapsed = start.elapsed();

    match result {
        Ok(_) => Check::pass(
            "Provider ping",
            format!(
                "{provider_str} → ok ({:.0}ms)",
                elapsed.as_secs_f64() * 1000.0
            ),
        ),
        Err(e) => {
            if is_configuration_gap(&e) {
                Check::warn(
                    "Provider ping",
                    format!("{provider_str} → not tested ({e})"),
                )
            } else {
                Check::fail("Provider ping", format!("{provider_str} → {e}"))
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
    let canonical = match provider.as_str() {
        "copilot" => "vscode-copilot",
        other => other,
    };
    ProviderType::from_str(canonical)
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
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        "anthropic".into()
    } else if check_local_port(11434) {
        "ollama (local)".into()
    } else {
        "none (mock)".into()
    }
}

#[cfg(target_os = "macos")]
fn check_macos_permissions() -> Vec<Check> {
    let snapshot = collect_permission_snapshot();
    let mut checks = Vec::new();

    checks.push(match snapshot.host_app {
        Some(host) => {
            let bundle = host.bundle_id.unwrap_or_else(|| "bundle id unknown".into());
            Check::pass("Terminal host", format!("{} ({bundle})", host.display_name))
        }
        None => Check::warn(
            "Terminal host",
            "could not determine the app hosting this terminal session",
        ),
    });

    checks.push(permission_check(
        "Notes Automation",
        snapshot.notes_automation,
        "Grant Automation access or run `/permissions bootstrap` to make macOS expose the consent path.",
    ));
    checks.push(permission_check(
        "System Events Automation",
        snapshot.system_events_automation,
        "Grant Automation access if you use UI scripting, or run `/permissions bootstrap` after opening System Settings.",
    ));
    checks.push(permission_check(
        "Accessibility",
        snapshot.accessibility,
        "Grant Accessibility access if commands send keystrokes or clicks to other apps.",
    ));

    checks
}

#[cfg(target_os = "macos")]
fn permission_check(label: &str, state: MacosConsentState, remediation: &str) -> Check {
    match state {
        MacosConsentState::Granted => Check::pass(label, "granted"),
        MacosConsentState::WouldPrompt => Check::warn(
            label,
            format!("not granted yet; macOS would prompt. {remediation}"),
        ),
        MacosConsentState::Denied => Check::warn(
            label,
            format!("denied or blocked by cached TCC state. {remediation}"),
        ),
        MacosConsentState::Unknown => Check::warn(
            label,
            format!("unknown; target app may not be running yet. {remediation}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
    fn check_provider_keys_no_keys() {
        // Remove all provider keys temporarily (or just check the function
        // runs without panic — we can't guarantee env state in CI)
        let checks = check_provider_keys();
        assert!(!checks.is_empty());
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
}
