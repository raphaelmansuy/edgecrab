use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::path::PathBuf;

use anyhow::{Context, anyhow, bail};
use chrono::Utc;
use edgecrab_core::AppConfig;
use edgecrab_tools::tools::mcp_client::{read_mcp_token_status, remove_mcp_token, write_mcp_token};
use edgequake_llm::providers::vscode::token::TokenManager;
use serde::{Deserialize, Serialize};

use crate::cli_args::AuthCommand;
use crate::{gateway_setup, mcp_oauth, mcp_support};

#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthTarget {
    Copilot,
    Mcp(String),
    Provider(&'static ProviderAuthSpec),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProviderAuthSpec {
    canonical: &'static str,
    aliases: &'static [&'static str],
    env_vars: &'static [&'static str],
    description: &'static str,
    interactive_login: bool,
}

const PROVIDER_AUTH_SPECS: &[ProviderAuthSpec] = &[
    ProviderAuthSpec {
        canonical: "openai",
        aliases: &[],
        env_vars: &["OPENAI_API_KEY"],
        description: "OpenAI API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "anthropic",
        aliases: &[],
        env_vars: &["ANTHROPIC_API_KEY"],
        description: "Anthropic API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "gemini",
        aliases: &["google"],
        env_vars: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        description: "Google Gemini API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "openrouter",
        aliases: &[],
        env_vars: &["OPENROUTER_API_KEY"],
        description: "OpenRouter API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "xai",
        aliases: &[],
        env_vars: &["XAI_API_KEY"],
        description: "xAI API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "mistral",
        aliases: &[],
        env_vars: &["MISTRAL_API_KEY"],
        description: "Mistral API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "groq",
        aliases: &[],
        env_vars: &["GROQ_API_KEY"],
        description: "Groq API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "cohere",
        aliases: &[],
        env_vars: &["COHERE_API_KEY"],
        description: "Cohere API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "perplexity",
        aliases: &[],
        env_vars: &["PERPLEXITY_API_KEY"],
        description: "Perplexity API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "deepseek",
        aliases: &[],
        env_vars: &["DEEPSEEK_API_KEY"],
        description: "DeepSeek API key",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "huggingface",
        aliases: &["hf"],
        env_vars: &["HUGGING_FACE_HUB_TOKEN", "HUGGINGFACE_API_KEY"],
        description: "Hugging Face token",
        interactive_login: false,
    },
    ProviderAuthSpec {
        canonical: "zai",
        aliases: &[],
        env_vars: &["ZAI_API_KEY"],
        description: "Z.AI / GLM API key",
        interactive_login: false,
    },
];

const AUTH_STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AuthStore {
    version: u32,
    active_provider: Option<String>,
    providers: BTreeMap<String, ProviderAuthState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderAuthState {
    auth_type: String,
    env_vars: Vec<String>,
    api_key: Option<String>,
    source: String,
    updated_at: String,
}

pub async fn run(command: AuthCommand) -> anyhow::Result<()> {
    let report = run_capture(command).await?;
    if !report.trim().is_empty() {
        println!("{report}");
    }
    Ok(())
}

pub async fn run_capture(command: AuthCommand) -> anyhow::Result<String> {
    match command {
        AuthCommand::List => list_targets().await,
        AuthCommand::Status { target } => status_target(target.as_deref()).await,
        AuthCommand::Add { target, token } => add_target(&target, token.as_deref()).await,
        AuthCommand::Login { target } => login_target_capture(&target).await,
        AuthCommand::Remove { target } => remove_target(&target).await,
        AuthCommand::Reset { target } => reset_target(target.as_deref()).await,
    }
}

pub async fn login_target(raw_target: &str) -> anyhow::Result<()> {
    let report = login_target_capture(raw_target).await?;
    if !report.trim().is_empty() {
        println!("{report}");
    }
    Ok(())
}

pub async fn login_target_capture(raw_target: &str) -> anyhow::Result<String> {
    match resolve_target(raw_target)? {
        AuthTarget::Copilot => {
            let manager = TokenManager::new()?;
            if manager.import_vscode_token().await? {
                let mut out = String::from("Imported the GitHub token from VS Code Copilot.");
                if manager.get_valid_copilot_token().await.is_ok() {
                    out.push_str("\nFetched and cached a fresh Copilot access token.");
                }
                Ok(out)
            } else {
                bail!(
                    "VS Code Copilot token not found. Run `edgecrab setup` for device-code auth or use `edgecrab auth add copilot --token <github-token>`"
                )
            }
        }
        AuthTarget::Mcp(name) => {
            let summary = mcp_oauth::login_mcp_server(&name, |_| {}).await?;
            Ok(summary)
        }
        AuthTarget::Provider(spec) => {
            if spec.interactive_login {
                bail!(
                    "interactive login is not implemented for '{}'; use `edgecrab auth add provider/{} --token <secret>`",
                    spec.canonical,
                    spec.canonical,
                );
            }
            bail!(
                "'{}' uses env-backed credentials, not an interactive login flow. Use `edgecrab auth add provider/{} --token <secret>`",
                spec.canonical,
                spec.canonical,
            )
        }
    }
}

pub async fn logout_target(raw_target: Option<&str>) -> anyhow::Result<()> {
    let report = logout_target_capture(raw_target).await?;
    if !report.trim().is_empty() {
        println!("{report}");
    }
    Ok(())
}

pub async fn logout_target_capture(raw_target: Option<&str>) -> anyhow::Result<String> {
    match raw_target {
        Some(target) => remove_target(target).await,
        None => reset_all().await,
    }
}

pub fn command_from_slash_args(args: &str) -> Result<AuthCommand, String> {
    let parts = crate::mcp_support::parse_inline_command_tokens(args.trim())?;
    match parts.first().map(String::as_str) {
        None => Ok(AuthCommand::List),
        Some("list") => Ok(AuthCommand::List),
        Some("status") => Ok(AuthCommand::Status {
            target: parts.get(1).cloned(),
        }),
        Some("login") => {
            let Some(target) = parts.get(1).cloned() else {
                return Err(auth_usage().into());
            };
            Ok(AuthCommand::Login { target })
        }
        Some("remove") | Some("logout") | Some("rm") => {
            let Some(target) = parts.get(1).cloned() else {
                return Err(auth_usage().into());
            };
            Ok(AuthCommand::Remove { target })
        }
        Some("reset") => Ok(AuthCommand::Reset {
            target: parts.get(1).cloned(),
        }),
        Some("add") => {
            let Some(target) = parts.get(1).cloned() else {
                return Err(auth_usage().into());
            };
            let token = parse_named_token_arg(&parts[2..], "token")?;
            Ok(AuthCommand::Add { target, token })
        }
        Some(_) => Err(auth_usage().into()),
    }
}

pub fn login_target_from_slash_args(args: &str) -> Result<String, String> {
    let parts = crate::mcp_support::parse_inline_command_tokens(args.trim())?;
    match parts.as_slice() {
        [target] if !target.trim().is_empty() => Ok(target.clone()),
        _ => Err("Usage: /login <target>\nTargets: copilot, provider/<name>, mcp/<server>, or a configured MCP server name".into()),
    }
}

pub fn logout_target_from_slash_args(args: &str) -> Result<Option<String>, String> {
    let parts = crate::mcp_support::parse_inline_command_tokens(args.trim())?;
    match parts.as_slice() {
        [] => Ok(None),
        [target] if !target.trim().is_empty() => Ok(Some(target.clone())),
        _ => Err("Usage: /logout [target]".into()),
    }
}

async fn list_targets() -> anyhow::Result<String> {
    let config = AppConfig::load()?;
    let manager = TokenManager::new()?;
    let store = auth_store()?;
    let has_github = manager.has_github_token().await;
    let has_copilot = manager.has_copilot_token().await;
    let vscode_import = manager.try_load_vscode_github_token().await.is_some();

    let mut out = String::from("Auth targets\n");
    writeln!(
        out,
        "copilot  github-cache={} copilot-cache={} vscode-import={} env-github-token={}",
        yes_no(has_github),
        yes_no(has_copilot),
        yes_no(vscode_import),
        yes_no(
            std::env::var("GITHUB_TOKEN")
                .ok()
                .is_some_and(|v| !v.trim().is_empty())
        )
    )?;

    for spec in PROVIDER_AUTH_SPECS {
        let stored = store.providers.contains_key(spec.canonical);
        let active = store.active_provider.as_deref() == Some(spec.canonical);
        writeln!(
            out,
            "provider/{}  env={} present={} auth-store={} active={} ({})",
            spec.canonical,
            spec.env_vars.join(","),
            yes_no(spec.env_vars.iter().any(|key| env_var_is_set(key))),
            yes_no(stored),
            yes_no(active),
            spec.description,
        )?;
    }

    if config.mcp_servers.is_empty() {
        out.push_str("No MCP servers configured.\n");
        return Ok(out.trim_end().to_string());
    }

    for name in config.mcp_servers.keys() {
        let guide = mcp_support::render_mcp_auth_guide(name)?;
        let auth = first_value(&guide, "auth").unwrap_or_else(|| "none".into());
        let cache = read_mcp_token_status(name);
        let cached =
            cache.is_some_and(|status| status.has_access_token || status.has_refresh_token);
        writeln!(
            out,
            "mcp/{name}  auth={auth} cached-token={}",
            yes_no(cached)
        )?;
    }

    Ok(out.trim_end().to_string())
}

async fn status_target(raw_target: Option<&str>) -> anyhow::Result<String> {
    match raw_target {
        None => list_targets().await,
        Some(target) => match resolve_target(target)? {
            AuthTarget::Copilot => show_copilot_status().await,
            AuthTarget::Mcp(name) => mcp_support::render_mcp_auth_guide(&name),
            AuthTarget::Provider(spec) => show_provider_status(spec),
        },
    }
}

async fn add_target(raw_target: &str, token: Option<&str>) -> anyhow::Result<String> {
    match resolve_target(raw_target)? {
        AuthTarget::Copilot => {
            let token = token.ok_or_else(|| {
                anyhow!(
                    "`edgecrab auth add copilot` requires `--token <github-token>` or use `edgecrab auth login copilot`"
                )
            })?;
            let manager = TokenManager::new()?;
            manager.save_github_token(token.trim().to_string()).await?;
            let _ = manager.get_valid_copilot_token().await;
            Ok("Saved the GitHub token for Copilot.".into())
        }
        AuthTarget::Mcp(name) => {
            let token = token.ok_or_else(|| {
                anyhow!("`edgecrab auth add {raw_target}` requires `--token <bearer-token>`")
            })?;
            write_mcp_token(&name, token.trim())
                .with_context(|| format!("failed to write token for MCP server '{name}'"))?;
            Ok(format!("Stored bearer token for MCP server '{name}'."))
        }
        AuthTarget::Provider(spec) => {
            let token = token.ok_or_else(|| {
                anyhow!(
                    "`edgecrab auth add provider/{}` requires `--token <secret>`",
                    spec.canonical,
                )
            })?;
            for env_var in spec.env_vars {
                gateway_setup::save_env_key(env_var, token.trim()).with_context(|| {
                    format!(
                        "failed to write {env_var} to {}",
                        gateway_setup::env_file_path().display()
                    )
                })?;
            }
            write_provider_auth_state(
                spec.canonical,
                ProviderAuthState {
                    auth_type: "api_key".into(),
                    env_vars: spec
                        .env_vars
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    api_key: Some(token.trim().to_string()),
                    source: "edgecrab-auth".into(),
                    updated_at: Utc::now().to_rfc3339(),
                },
            )?;
            Ok(format!(
                "Saved {} to {} and {}.",
                spec.description,
                spec.env_vars.join(", "),
                auth_store_path().display(),
            ))
        }
    }
}

async fn remove_target(raw_target: &str) -> anyhow::Result<String> {
    match resolve_target(raw_target)? {
        AuthTarget::Copilot => {
            let manager = TokenManager::new()?;
            manager.clear_tokens().await?;
            Ok("Cleared EdgeCrab's cached Copilot tokens.".into())
        }
        AuthTarget::Mcp(name) => {
            remove_mcp_token(&name);
            Ok(format!("Removed cached token for MCP server '{name}'."))
        }
        AuthTarget::Provider(spec) => {
            for env_var in spec.env_vars {
                gateway_setup::remove_env_key(env_var).with_context(|| {
                    format!(
                        "failed to remove {env_var} from {}",
                        gateway_setup::env_file_path().display()
                    )
                })?;
            }
            remove_provider_auth_state(spec.canonical)?;
            Ok(format!(
                "Removed {} from {} and {}.",
                spec.description,
                spec.env_vars.join(", "),
                auth_store_path().display(),
            ))
        }
    }
}

async fn reset_target(raw_target: Option<&str>) -> anyhow::Result<String> {
    match raw_target {
        Some(target) => remove_target(target).await,
        None => reset_all().await,
    }
}

async fn reset_all() -> anyhow::Result<String> {
    let config = AppConfig::load()?;
    let manager = TokenManager::new()?;
    manager.clear_tokens().await?;
    for name in config.mcp_servers.keys() {
        remove_mcp_token(name);
    }
    for spec in PROVIDER_AUTH_SPECS {
        for env_var in spec.env_vars {
            gateway_setup::remove_env_key(env_var)?;
        }
    }
    clear_provider_auth_store()?;
    Ok("Cleared EdgeCrab-managed Copilot, provider, and MCP auth caches.".into())
}

async fn show_copilot_status() -> anyhow::Result<String> {
    let manager = TokenManager::new()?;
    let mut out = String::from("copilot\n");
    writeln!(
        out,
        "github-cache:   {}",
        yes_no(manager.has_github_token().await)
    )?;
    writeln!(
        out,
        "copilot-cache:  {}",
        yes_no(manager.has_copilot_token().await)
    )?;
    writeln!(
        out,
        "vscode-import:  {}",
        yes_no(manager.try_load_vscode_github_token().await.is_some())
    )?;
    writeln!(
        out,
        "env-github-token: {}",
        yes_no(
            std::env::var("GITHUB_TOKEN")
                .ok()
                .is_some_and(|v| !v.trim().is_empty())
        )
    )?;
    writeln!(out, "Local cache path: {}", copilot_cache_dir()?.display())?;
    Ok(out.trim_end().to_string())
}

fn show_provider_status(spec: &'static ProviderAuthSpec) -> anyhow::Result<String> {
    let store = auth_store()?;
    let stored = store.providers.get(spec.canonical);
    let mut out = format!("provider/{}\n", spec.canonical);
    writeln!(out, "description: {}", spec.description)?;
    writeln!(
        out,
        "env-file:    {}",
        gateway_setup::env_file_path().display()
    )?;
    writeln!(out, "auth-store:  {}", auth_store_path().display())?;
    writeln!(
        out,
        "active:      {}",
        yes_no(store.active_provider.as_deref() == Some(spec.canonical))
    )?;
    for env_var in spec.env_vars {
        writeln!(out, "{env_var}: {}", yes_no(env_var_is_set(env_var)))?;
    }
    writeln!(out, "stored:      {}", yes_no(stored.is_some()))?;
    if let Some(stored) = stored {
        writeln!(out, "source:      {}", stored.source)?;
        writeln!(out, "updated-at:  {}", stored.updated_at)?;
    }
    Ok(out.trim_end().to_string())
}

fn resolve_target(raw_target: &str) -> anyhow::Result<AuthTarget> {
    let target = raw_target.trim();
    if target.is_empty() {
        bail!("auth target cannot be empty");
    }
    if target.eq_ignore_ascii_case("copilot") {
        return Ok(AuthTarget::Copilot);
    }

    let config = AppConfig::load()?;
    if let Some(name) = target.strip_prefix("mcp/") {
        if config.mcp_servers.contains_key(name) {
            return Ok(AuthTarget::Mcp(name.to_string()));
        }
        bail!("unknown MCP server '{name}'");
    }
    if config.mcp_servers.contains_key(target) {
        return Ok(AuthTarget::Mcp(target.to_string()));
    }
    if let Some(name) = target.strip_prefix("provider/") {
        if let Some(spec) = resolve_provider(name) {
            return Ok(AuthTarget::Provider(spec));
        }
        bail!("unknown provider auth target '{name}'");
    }
    if let Some(spec) = resolve_provider(target) {
        return Ok(AuthTarget::Provider(spec));
    }

    bail!(
        "unknown auth target '{target}' (expected `copilot`, `provider/<name>`, `mcp/<server>`, or a configured MCP server name)"
    )
}

fn resolve_provider(name: &str) -> Option<&'static ProviderAuthSpec> {
    PROVIDER_AUTH_SPECS.iter().find(|spec| {
        spec.canonical.eq_ignore_ascii_case(name)
            || spec
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

fn parse_named_token_arg(parts: &[String], key: &str) -> Result<Option<String>, String> {
    let mut idx = 0usize;
    let mut token = None;
    while idx < parts.len() {
        let current = &parts[idx];
        if current == &format!("--{key}") {
            let Some(value) = parts.get(idx + 1) else {
                return Err(format!("Missing value for --{key}"));
            };
            if token.replace(value.clone()).is_some() {
                return Err(format!("Duplicate --{key} option"));
            }
            idx += 2;
            continue;
        }
        if let Some(value) = current.strip_prefix(&format!("--{key}=")) {
            if token.replace(value.to_string()).is_some() {
                return Err(format!("Duplicate --{key} option"));
            }
            idx += 1;
            continue;
        }
        return Err(format!("Unexpected argument: {current}"));
    }
    Ok(token)
}

fn env_var_is_set(env_var: &str) -> bool {
    std::env::var(env_var)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn first_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.strip_prefix(&format!("{key}: "))
            .map(str::trim)
            .map(str::to_string)
    })
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn copilot_cache_dir() -> anyhow::Result<std::path::PathBuf> {
    dirs::config_dir()
        .map(|base| base.join("edgequake").join("copilot"))
        .ok_or_else(|| anyhow!("failed to resolve the local config directory"))
}

fn auth_usage() -> &'static str {
    "Usage: /auth [list|status [target]|add <target> --token <secret>|login <target>|remove <target>|reset [target]]\nTargets: copilot, provider/<name>, mcp/<server>, or a configured MCP server name"
}

fn auth_store_path() -> PathBuf {
    edgecrab_core::edgecrab_home().join("auth.json")
}

fn auth_lock_path() -> PathBuf {
    edgecrab_core::edgecrab_home().join("auth.lock")
}

fn with_auth_store_lock<T>(f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    let lock_path = auth_lock_path();
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open {}", lock_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        // WHY advisory lock: auth mutations can come from multiple EdgeCrab
        // processes. Keep auth.json read-modify-write cycles atomic.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("failed to lock {}", lock_path.display()));
        }
    }

    let result = f();

    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("failed to unlock {}", lock_path.display()));
        }
    }

    result
}

fn auth_store() -> anyhow::Result<AuthStore> {
    with_auth_store_lock(read_auth_store_unlocked)
}

fn read_auth_store_unlocked() -> anyhow::Result<AuthStore> {
    let path = auth_store_path();
    if !path.exists() {
        return Ok(AuthStore {
            version: AUTH_STORE_VERSION,
            ..Default::default()
        });
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut store: AuthStore = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if store.version == 0 {
        store.version = AUTH_STORE_VERSION;
    }
    Ok(store)
}

fn save_auth_store(mut store: AuthStore) -> anyhow::Result<()> {
    let path = auth_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    store.version = AUTH_STORE_VERSION;
    let tmp = path.with_extension("json.tmp");
    let payload = serde_json::to_string_pretty(&store)?;
    std::fs::write(&tmp, payload).with_context(|| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

fn write_provider_auth_state(provider_id: &str, state: ProviderAuthState) -> anyhow::Result<()> {
    with_auth_store_lock(|| {
        let mut store = read_auth_store_unlocked()?;
        store.providers.insert(provider_id.to_string(), state);
        store.active_provider = Some(provider_id.to_string());
        save_auth_store(store)
    })
}

fn remove_provider_auth_state(provider_id: &str) -> anyhow::Result<()> {
    with_auth_store_lock(|| {
        let mut store = read_auth_store_unlocked()?;
        store.providers.remove(provider_id);
        if store.active_provider.as_deref() == Some(provider_id) {
            store.active_provider = None;
        }
        save_auth_store(store)
    })
}

fn clear_provider_auth_store() -> anyhow::Result<()> {
    with_auth_store_lock(|| {
        let mut store = read_auth_store_unlocked()?;
        store.providers.clear();
        store.active_provider = None;
        save_auth_store(store)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_auth_add_from_slash_args() {
        let command = command_from_slash_args("add provider/openai --token sk-test").unwrap();
        match command {
            AuthCommand::Add { target, token } => {
                assert_eq!(target, "provider/openai");
                assert_eq!(token.as_deref(), Some("sk-test"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_logout_shortcut_from_slash_args() {
        let target = logout_target_from_slash_args("copilot").unwrap();
        assert_eq!(target.as_deref(), Some("copilot"));
    }

    #[test]
    fn resolves_provider_alias() {
        let target = resolve_provider("google").expect("provider alias");
        assert_eq!(target.canonical, "gemini");
    }

    #[test]
    #[serial_test::serial(edgecrab_home_env)]
    fn provider_auth_store_round_trip_tracks_active_provider() {
        let _lock = crate::gateway_catalog::lock_test_env();
        let dir = tempfile::tempdir().expect("tempdir");
        unsafe {
            std::env::set_var("EDGECRAB_HOME", dir.path());
        }

        write_provider_auth_state(
            "openai",
            ProviderAuthState {
                auth_type: "api_key".into(),
                env_vars: vec!["OPENAI_API_KEY".into()],
                api_key: Some("sk-test".into()),
                source: ".env".into(),
                updated_at: "2026-04-10T00:00:00Z".into(),
            },
        )
        .expect("write provider state");

        let store = auth_store().expect("auth store");
        assert_eq!(store.active_provider.as_deref(), Some("openai"));
        assert!(store.providers.contains_key("openai"));

        remove_provider_auth_state("openai").expect("remove provider state");
        let store = auth_store().expect("auth store");
        assert!(store.active_provider.is_none());
        assert!(!store.providers.contains_key("openai"));

        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }
}
