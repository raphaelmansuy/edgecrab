use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use chrono::Utc;
use edgecrab_core::AppConfig;
use edgecrab_core::oauth::{
    AnthropicOAuthLoginOptions, CodexDeviceLoginOptions, CodexDevicePrompt, OPENAI_CODEX_PROVIDER,
    is_anthropic_oauth_alias, is_openai_codex_alias, login_anthropic_oauth,
    login_codex_device_oauth, read_anthropic_oauth_file, remove_anthropic_oauth_file,
    remove_codex_oauth,
};
use edgecrab_tools::tools::mcp_client::{read_mcp_token_status, remove_mcp_token, write_mcp_token};
use edgequake_llm::providers::vscode::{auth::GitHubAuth, token::TokenManager};
use serde::{Deserialize, Serialize};

use crate::cli_args::AuthCommand;
use crate::{gateway_setup, mcp_oauth, mcp_support};

#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthTarget {
    Copilot,
    Mcp(String),
    NousPortal,
    XaiOAuth,
    AnthropicOAuth,
    OpenaiCodex,
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
        env_vars: &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
        description: "Anthropic-compatible API key",
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
        canonical: "nvidia",
        aliases: &["nim", "nvidia-nim"],
        env_vars: &["NVIDIA_API_KEY"],
        description: "NVIDIA NIM API key",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopilotDevicePrompt {
    pub(crate) open_url: String,
    pub(crate) display_url: String,
    pub(crate) user_code: String,
}

pub(crate) fn render_copilot_device_prompt(prompt: &CopilotDevicePrompt) -> String {
    format!(
        "1. Your browser should open automatically.\n2. If it does not, visit:\n   {}\n\n3. Enter this one-time code:\n\n   {}\n\nTip: drag to select the code with your mouse.\nWaiting for GitHub approval...",
        prompt.display_url, prompt.user_code
    )
}

fn terminal_hyperlink(label: &str, url: &str) -> String {
    // OSC-8 hyperlinks are clickable in modern terminals (including VS Code).
    format!("\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\")
}

fn persist_last_oauth_url(url: &str) -> Option<PathBuf> {
    persist_last_oauth_url_silent(url)
}

/// Persist authorize URL for TUI fallback (no stderr noise — safe under ratatui).
pub fn persist_last_oauth_url_silent(url: &str) -> Option<PathBuf> {
    let path = edgecrab_core::edgecrab_home().join("last_oauth_url.txt");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, format!("{url}\n")).ok()?;
    Some(path)
}

fn print_open_url_block(stderr: &mut io::Stderr, url: &str, open_browser: bool, short_hint: &str) {
    let clickable = terminal_hyperlink("Open authorization page", url);
    let _ = writeln!(stderr, "{short_hint}\n");
    let _ = writeln!(stderr, "{clickable}");
    let _ = writeln!(stderr, "{url}\n");
    if let Some(path) = persist_last_oauth_url(url) {
        let _ = writeln!(
            stderr,
            "Saved URL: {} (fallback if terminal links fail)\n",
            path.display()
        );
    }
    if open_browser {
        let _ = open_auth_url(url);
        let _ = writeln!(stderr, "Browser opened (if supported).");
    }
}

fn friendly_grok_login_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("http 403") || lower.contains("tier_denied") {
        return "xAI rejected OAuth for this account (SuperGrok / X Premium+ required). \
                Try `edgecrab auth add provider/xai --token <key>` if you have an API key, \
                or upgrade at https://x.ai/grok."
            .into();
    }
    if lower.contains("timed out") || lower.contains("callback") {
        return "xAI sign-in timed out waiting for the browser callback. \
                Use the paste flow: edgecrab auth grok start && edgecrab auth grok finish --oauth-code 'CODE'"
            .into();
    }
    if lower.contains("state mismatch") {
        return "xAI sign-in failed: OAuth state mismatch. Run `edgecrab auth add grok` again."
            .into();
    }
    if is_grok_pending_expired_error(message) {
        return "Grok sign-in session expired. In the TUI press Enter to open a fresh x.ai page, \
                or run `edgecrab auth grok start`."
            .into();
    }
    message.to_string()
}

pub(crate) fn render_grok_oauth_steps(authorize_url: &str, finish_hint: &str) -> String {
    format!(
        "1. Open x.ai and sign in (browser should open automatically).\n\
         2. If you see \"Could not establish connection\", copy the code on that page.\n\
         3. {finish_hint}\n\n\
         Sign-in URL:\n   {authorize_url}"
    )
}

fn print_grok_oauth_signin(prompt: &edgecrab_proxy::XaiOAuthAuthorizePrompt, open_browser: bool) {
    let mut stderr = io::stderr();
    let _ = write!(stderr, "\x1b[2J\x1b[H");
    let _ = writeln!(stderr, "xAI Grok sign-in (SuperGrok / X Premium+)");
    let _ = writeln!(stderr, "=======================================\n");

    let finish_hint = if prompt.manual_paste {
        "Paste the code at the code> prompt below (same line, then Enter)."
    } else {
        "Paste the code at code> below, or run: edgecrab auth grok finish --oauth-code 'CODE'"
    };
    let _ = writeln!(
        stderr,
        "{}",
        render_grok_oauth_steps(&prompt.authorize_url, finish_hint)
    );
    let _ = writeln!(stderr);
    let clickable = terminal_hyperlink("Open authorization page", &prompt.authorize_url);
    let _ = writeln!(stderr, "{clickable}\n");
    if let Some(path) = persist_last_oauth_url(&prompt.authorize_url) {
        let _ = writeln!(stderr, "Saved URL: {}\n", path.display());
    }
    if open_browser {
        let _ = open_auth_url(&prompt.authorize_url);
        let _ = writeln!(stderr, "Browser opened (if supported).\n");
    }
    if !prompt.manual_paste {
        let _ = writeln!(stderr, "Waiting for callback on {}", prompt.redirect_uri);
        let _ = writeln!(
            stderr,
            "(Loopback mode — x.ai often cannot reach localhost; prefer `edgecrab auth grok start` + `finish`.)\n"
        );
    }
    let _ = stderr.flush();
}

fn grok_use_paste_flow(_manual_paste: bool, loopback: bool) -> bool {
    !(loopback || oauth_flag(false, "EDGECRAB_AUTH_LOOPBACK"))
}

fn xai_oauth_options(
    no_browser: bool,
    manual_paste: bool,
    loopback: bool,
    oauth_code: Option<String>,
) -> edgecrab_proxy::XaiOAuthLoginOptions {
    let manual_paste = grok_use_paste_flow(manual_paste, loopback);
    let open_browser = !no_browser && oauth_code.is_none();
    let on_authorize = Arc::new(move |prompt: edgecrab_proxy::XaiOAuthAuthorizePrompt| {
        // CLI / suspended-terminal path only — NEVER call under an active ratatui frame
        // (screen clear + OSC-8 hyperlinks corrupt the TUI, see setup_overlays Grok auth).
        print_grok_oauth_signin(&prompt, open_browser);
    });
    edgecrab_proxy::XaiOAuthLoginOptions {
        open_browser,
        manual_paste,
        pasted_code: oauth_code,
        on_authorize: Some(on_authorize),
        ..Default::default()
    }
}

/// TUI-safe OAuth options: no stderr clear, no OSC-8, no browser (caller opens).
///
/// Best practice: the host TUI owns presentation; the OAuth engine only produces a URL + PKCE.
fn xai_oauth_options_for_tui() -> edgecrab_proxy::XaiOAuthLoginOptions {
    edgecrab_proxy::XaiOAuthLoginOptions {
        open_browser: false,
        manual_paste: true,
        pasted_code: None,
        // Empty hook: suppress default eprintln path and print_grok_oauth_signin.
        on_authorize: Some(Arc::new(
            |_prompt: edgecrab_proxy::XaiOAuthAuthorizePrompt| {},
        )),
        ..Default::default()
    }
}

fn resolve_oauth_code(cli: Option<String>) -> Option<String> {
    cli.or_else(|| std::env::var("EDGECRAB_XAI_AUTH_CODE").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn friendly_copilot_login_error(message: &str) -> String {
    if message.contains("expired_token") {
        return "The login code expired before GitHub approval. Run /login again to generate a fresh code.".into();
    }
    if message.contains("access_denied") {
        return "GitHub approval was cancelled. Run /login again when you want to retry.".into();
    }
    message.trim().to_string()
}

fn print_copilot_device_prompt(prompt: &CopilotDevicePrompt) {
    let mut stderr = io::stderr();
    let _ = write!(stderr, "\x1b[2J\x1b[H");
    let _ = writeln!(stderr, "GitHub Copilot sign-in");
    let _ = writeln!(stderr, "=======================\n");
    let _ = writeln!(
        stderr,
        "{}",
        terminal_hyperlink("Open device login page", &prompt.open_url)
    );
    let _ = writeln!(stderr, "{}\n", prompt.display_url);
    let _ = writeln!(stderr, "{}\n", render_copilot_device_prompt(prompt));
    let _ = stderr.flush();
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
        AuthCommand::Add {
            target,
            token,
            no_browser,
            manual_paste,
            loopback,
            oauth_code,
        } => {
            add_target(
                &target,
                token,
                no_browser,
                manual_paste,
                loopback,
                oauth_code,
            )
            .await
        }
        AuthCommand::Login {
            target,
            no_browser,
            manual_paste,
            loopback,
            oauth_code,
        } => {
            login_target_capture(
                target.as_deref().unwrap_or("copilot"),
                no_browser,
                manual_paste,
                loopback,
                oauth_code,
            )
            .await
        }
        AuthCommand::Grok { command } => run_grok_auth(command).await,
        AuthCommand::Remove { target } => remove_target(&target).await,
        AuthCommand::Reset { target } => reset_target(target.as_deref()).await,
    }
}

/// True when `auth add` should run a browser/device OAuth flow (no `--token`).
pub fn is_grok_auth_target(raw_target: &str) -> bool {
    matches!(resolve_target(raw_target), Ok(AuthTarget::XaiOAuth))
}

pub fn target_uses_interactive_oauth(raw_target: &str) -> bool {
    matches!(
        resolve_target(raw_target),
        Ok(AuthTarget::NousPortal
            | AuthTarget::XaiOAuth
            | AuthTarget::AnthropicOAuth
            | AuthTarget::OpenaiCodex)
    )
}

pub async fn login_target(raw_target: &str) -> anyhow::Result<()> {
    let report = login_target_capture(raw_target, false, false, false, None).await?;
    if !report.trim().is_empty() {
        println!("{report}");
    }
    Ok(())
}

pub async fn login_target_capture(
    raw_target: &str,
    no_browser: bool,
    manual_paste: bool,
    loopback: bool,
    oauth_code: Option<String>,
) -> anyhow::Result<String> {
    match resolve_target(raw_target)? {
        AuthTarget::Copilot => {
            let manager = TokenManager::new()?;
            let mut out = String::new();

            let auth = GitHubAuth::new()?;
            let access_token = match auth
                .device_code_flow(|code| {
                    let prompt = CopilotDevicePrompt {
                        open_url: code
                            .verification_uri_complete
                            .clone()
                            .unwrap_or_else(|| code.verification_uri.clone()),
                        display_url: code.verification_uri.clone(),
                        user_code: code.user_code.clone(),
                    };
                    print_copilot_device_prompt(&prompt);
                    let _ = open_auth_url(&prompt.open_url);
                })
                .await
            {
                Ok(token) => token,
                Err(err) => {
                    return Err(anyhow!(friendly_copilot_login_error(&err.to_string())));
                }
            };

            manager.save_github_token(access_token).await?;
            manager.get_valid_copilot_token().await?;

            out.push_str("Device login completed and a fresh Copilot token was cached.");
            out.push_str(" If the next prompt fails with user_weekly_rate_limited or user_global_rate_limited, the login succeeded and GitHub is throttling chat usage for the account. If you are not already on Auto, try /model copilot/auto; otherwise wait for the reset window or switch providers.");
            Ok(out)
        }
        AuthTarget::Mcp(name) => {
            let summary = mcp_oauth::login_mcp_server(&name, |_| {}).await?;
            Ok(summary)
        }
        AuthTarget::NousPortal => login_nous_portal_capture(None).await,
        AuthTarget::XaiOAuth => {
            login_xai_oauth_capture(no_browser, manual_paste, loopback, oauth_code).await
        }
        AuthTarget::AnthropicOAuth => login_anthropic_oauth_capture(no_browser).await,
        AuthTarget::OpenaiCodex => login_openai_codex_capture().await,
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

async fn login_nous_portal_capture(label: Option<&str>) -> anyhow::Result<String> {
    let msg = edgecrab_proxy::login_nous_portal(
        None,
        &edgecrab_proxy::NousDeviceLoginOptions::default(),
        label,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(msg)
}

fn print_claude_oauth_signin(authorize_url: &str, open_browser: bool) {
    let mut stderr = io::stderr();
    let _ = write!(stderr, "\x1b[2J\x1b[H");
    let _ = writeln!(stderr, "Claude Pro / Max OAuth");
    let _ = writeln!(stderr, "=====================\n");
    print_open_url_block(
        &mut stderr,
        authorize_url,
        open_browser,
        "Open this URL in your browser:",
    );
    let _ = writeln!(
        stderr,
        "After approving, paste the authorization code at the prompt below.\n"
    );
    let _ = stderr.flush();
}

async fn login_anthropic_oauth_capture(no_browser: bool) -> anyhow::Result<String> {
    let no_browser = oauth_flag(no_browser, "EDGECRAB_AUTH_NO_BROWSER");
    let open_browser = !no_browser;
    let on_authorize = Arc::new(move |url: &str| {
        print_claude_oauth_signin(url, open_browser);
    });
    let msg = login_anthropic_oauth(&AnthropicOAuthLoginOptions {
        open_browser,
        on_authorize: Some(on_authorize),
    })
    .await
    .map_err(anyhow::Error::msg)?;
    Ok(msg)
}

fn print_codex_device_prompt(prompt: &CodexDevicePrompt) {
    let mut stderr = io::stderr();
    let _ = write!(stderr, "\x1b[2J\x1b[H");
    let _ = writeln!(stderr, "ChatGPT Pro / Codex sign-in");
    let _ = writeln!(stderr, "============================\n");
    print_open_url_block(
        &mut stderr,
        &prompt.sign_in_url,
        false,
        "1. Open this URL in your browser:",
    );
    let _ = writeln!(stderr, "2. Enter this one-time code:\n");
    let _ = writeln!(stderr, "   {}\n", prompt.user_code);
    let _ = writeln!(stderr, "Waiting for sign-in...");
    let _ = stderr.flush();
}

async fn login_openai_codex_capture() -> anyhow::Result<String> {
    let on_device_code = Arc::new(|prompt: CodexDevicePrompt| {
        print_codex_device_prompt(&prompt);
        let _ = open_auth_url(&prompt.sign_in_url);
    });
    let msg = login_codex_device_oauth(
        None,
        &CodexDeviceLoginOptions {
            on_device_code: Some(on_device_code),
        },
    )
    .await
    .map_err(anyhow::Error::msg)?;
    Ok(msg)
}

async fn run_grok_auth(command: crate::cli_args::GrokAuthCommand) -> anyhow::Result<String> {
    use crate::cli_args::GrokAuthCommand;
    match command {
        GrokAuthCommand::Start { no_browser } => grok_auth_start(no_browser).await,
        GrokAuthCommand::Finish { oauth_code } => {
            grok_auth_finish(resolve_oauth_code(oauth_code)).await
        }
    }
}

pub fn grok_pending_path() -> std::path::PathBuf {
    edgecrab_proxy::default_xai_pending_path()
}

/// Non-expired pending session (stale `oauth-pending/xai-grok.json` is removed).
pub fn grok_load_valid_pending() -> Option<(String, std::path::PathBuf)> {
    let path = grok_pending_path();
    let session = edgecrab_proxy::peek_xai_pending_session(Some(&path))?;
    Some((session.authorize_url, path))
}

pub fn grok_has_valid_pending_session() -> bool {
    grok_load_valid_pending().is_some()
}

pub fn is_grok_pending_expired_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("pending grok oauth session expired")
        || lower.contains("no pending grok oauth session")
        || lower.contains("run `edgecrab auth grok start`")
}

/// Begin Grok OAuth (TUI step 1) — silent; TUI opens browser and renders status.
pub async fn grok_auth_start_for_ui(
    no_browser: bool,
) -> anyhow::Result<(String, std::path::PathBuf)> {
    let _no_browser = oauth_flag(no_browser, "EDGECRAB_AUTH_NO_BROWSER");
    // Always TUI-safe options (no stderr clear / OSC-8). Browser is opened by the App.
    let opts = xai_oauth_options_for_tui();
    let started = edgecrab_proxy::start_xai_oauth_login(&opts)
        .await
        .map_err(|e| anyhow::anyhow!("{}", friendly_grok_login_error(&e.to_string())))?;
    let _ = persist_last_oauth_url_silent(&started.authorize_url);
    Ok((started.authorize_url, started.pending_path))
}

/// Complete Grok OAuth with a pasted code (TUI step 2).
pub async fn grok_auth_finish_for_ui(code: String) -> anyhow::Result<String> {
    grok_auth_finish_for_ui_with_pending(code, None).await
}

/// Complete Grok OAuth using the PKCE session path captured at step 1.
pub async fn grok_auth_finish_for_ui_with_pending(
    code: String,
    pending_path: Option<std::path::PathBuf>,
) -> anyhow::Result<String> {
    let code = code.trim().to_string();
    if code.is_empty() {
        anyhow::bail!("authorization code is empty");
    }
    // Silent finish options — no authorize-print side effects.
    let opts = xai_oauth_options_for_tui();
    let pending = pending_path.as_deref();
    edgecrab_proxy::login_xai_oauth_finish(None, Some(code), pending, &opts)
        .await
        .map_err(|e| anyhow::anyhow!("{}", friendly_grok_login_error(&e.to_string())))
}

async fn grok_auth_start(no_browser: bool) -> anyhow::Result<String> {
    let (authorize_url, path) = grok_auth_start_for_ui(no_browser).await?;
    Ok(format!(
        "Grok sign-in started.\n\
         Session: {path}\n\
         Next: edgecrab auth grok finish --oauth-code 'PASTE_CODE_FROM_X_AI'\n\
         Or in TUI: /login grok\n\
         URL: {authorize_url}",
        path = path.display(),
    ))
}

async fn grok_auth_finish(oauth_code: Option<String>) -> anyhow::Result<String> {
    let code = resolve_oauth_code(oauth_code)
        .ok_or_else(|| anyhow::anyhow!("missing --oauth-code (or run /login grok in the TUI)"))?;
    grok_auth_finish_for_ui(code).await
}

pub fn open_grok_authorize_url(url: &str) -> anyhow::Result<()> {
    open_auth_url(url)
}

fn open_auth_url(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(url).status();

    #[cfg(all(unix, not(target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(url).status();

    #[cfg(windows)]
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .status();

    status
        .map_err(|e| anyhow::anyhow!("failed to launch browser: {e}"))?
        .success()
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("browser launcher exited with an error (url: {url})"))
}

/// Mask a code for on-screen status (first/last 4 chars).
pub fn mask_grok_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    if chars.len() <= 12 {
        return "••••".to_string();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().skip(chars.len().saturating_sub(4)).collect();
    format!("{head}…{tail} ({} chars)", chars.len())
}

/// Parse clipboard or pasted text into an xAI authorization code.
pub fn extract_grok_auth_code(input: &str) -> anyhow::Result<String> {
    edgecrab_proxy::extract_xai_oauth_code_from_paste(input)
        .map_err(|e| anyhow::anyhow!("{}", friendly_grok_login_error(&e.to_string())))
}

/// Read authorization code from the system clipboard (macOS/Linux/Windows).
pub fn grok_read_clipboard_code() -> anyhow::Result<String> {
    let text = arboard::Clipboard::new()
        .and_then(|mut cb| cb.get_text())
        .map_err(|e| anyhow::anyhow!("clipboard: {e}"))?;
    extract_grok_auth_code(&text)
}

const GROK_FINISH_PROMPT: &str = "\
Copy the authorization code from the x.ai page (the long token).\n\
Paste it on the line below and press Enter.\n\
\n\
Tips:\n\
  • Code only — not the full browser URL\n\
  • If the page says \"Could not establish connection\", that is normal\n\
  • Clipboard also works: return to the overlay and press p\n";

/// Suspend the TUI and read one line from the real terminal (reliable Enter handling).
pub fn prompt_and_read_grok_code(stdout: &mut impl io::Write) -> anyhow::Result<String> {
    use std::io::{self, BufRead};

    let title = "SuperGrok — paste authorization code";
    writeln!(stdout, "\n{title}")?;
    writeln!(stdout, "{}", "─".repeat(title.chars().count().min(48)))?;
    writeln!(stdout)?;
    for line in GROK_FINISH_PROMPT.lines() {
        writeln!(stdout, "{line}")?;
    }
    writeln!(stdout)?;
    write!(stdout, "code> ")?;
    stdout.flush()?;
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| anyhow::anyhow!("stdin: {e}"))?;
    extract_grok_auth_code(&line)
}

async fn login_xai_oauth_capture(
    no_browser: bool,
    manual_paste: bool,
    loopback: bool,
    oauth_code: Option<String>,
) -> anyhow::Result<String> {
    let no_browser = oauth_flag(no_browser, "EDGECRAB_AUTH_NO_BROWSER");
    let manual_paste = oauth_flag(manual_paste, "EDGECRAB_AUTH_MANUAL_PASTE");
    let loopback = oauth_flag(loopback, "EDGECRAB_AUTH_LOOPBACK");
    let oauth_code = resolve_oauth_code(oauth_code);
    let msg = edgecrab_proxy::login_xai_oauth(
        None,
        &xai_oauth_options(no_browser, manual_paste, loopback, oauth_code),
    )
    .await
    .map_err(|e| anyhow::anyhow!("{}", friendly_grok_login_error(&e.to_string())))?;
    Ok(msg)
}

fn oauth_flag(cli: bool, env_key: &str) -> bool {
    cli || std::env::var(env_key)
        .ok()
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
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
        Some("login") => Ok(AuthCommand::Login {
            target: parts.get(1).cloned(),
            no_browser: false,
            manual_paste: false,
            loopback: false,
            oauth_code: None,
        }),
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
            Ok(AuthCommand::Add {
                target,
                token,
                no_browser: false,
                manual_paste: false,
                loopback: false,
                oauth_code: None,
            })
        }
        Some(_) => Err(auth_usage().into()),
    }
}

pub fn login_target_from_slash_args(args: &str) -> Result<String, String> {
    let parts = crate::mcp_support::parse_inline_command_tokens(args.trim())?;
    match parts.as_slice() {
        [] => Ok("copilot".into()),
        [target] if !target.trim().is_empty() => Ok(target.clone()),
        _ => Err("Usage: /login [target]\nTargets: copilot, grok, claude-pro, chatgpt-pro, nous, provider/<name>, mcp/<server>\nDefault target: copilot".into()),
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

    {
        let path = edgecrab_proxy::default_auth_path();
        let probe = edgecrab_proxy::probe_oauth_auth(&edgecrab_proxy::RECIPE_NOUS);
        let nous_ready = matches!(probe, edgecrab_proxy::AuthProbe::Ready);
        writeln!(
            out,
            "nous       auth-file={} oauth={} ({})",
            yes_no(path.exists() && nous_ready),
            yes_no(nous_ready),
            edgecrab_proxy::RECIPE_NOUS.display_name,
        )?;
        let xai_probe = edgecrab_proxy::probe_oauth_auth(&edgecrab_proxy::RECIPE_XAI);
        let xai_ready = matches!(xai_probe, edgecrab_proxy::AuthProbe::Ready);
        writeln!(
            out,
            "grok       auth-file={} oauth={} ({})",
            yes_no(path.exists() && xai_ready),
            yes_no(xai_ready),
            edgecrab_proxy::RECIPE_XAI.display_name,
        )?;
    }

    let claude_oauth = read_anthropic_oauth_file().ok().flatten().is_some();
    writeln!(
        out,
        "claude-pro oauth-file={} ({})",
        yes_no(claude_oauth),
        edgecrab_core::oauth::anthropic_oauth_path().display(),
    )?;

    let codex_path = edgecrab_core::oauth::auth_store::default_auth_path();
    let codex_oauth = edgecrab_core::oauth::codex_has_credentials(&codex_path);
    writeln!(
        out,
        "chatgpt-pro auth-file={} oauth={}",
        yes_no(codex_oauth),
        codex_path.display(),
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
            AuthTarget::NousPortal => show_nous_status(),
            AuthTarget::XaiOAuth => show_xai_oauth_status(),
            AuthTarget::AnthropicOAuth => show_anthropic_oauth_status(),
            AuthTarget::OpenaiCodex => show_openai_codex_status(),
            AuthTarget::Provider(spec) => show_provider_status(spec),
        },
    }
}

async fn add_target(
    raw_target: &str,
    token: Option<String>,
    no_browser: bool,
    manual_paste: bool,
    loopback: bool,
    oauth_code: Option<String>,
) -> anyhow::Result<String> {
    match resolve_target(raw_target)? {
        AuthTarget::NousPortal => {
            if token.is_some() {
                bail!(
                    "Nous Portal uses OAuth device login, not a static token. Run `edgecrab auth add nous` or `edgecrab auth login nous`"
                );
            }
            login_nous_portal_capture(None).await
        }
        AuthTarget::XaiOAuth => {
            if token.is_some() {
                bail!(
                    "xAI Grok uses browser OAuth (SuperGrok / X Premium+), not a static token. Run `edgecrab auth add grok` or `edgecrab auth add xai-oauth`"
                );
            }
            login_xai_oauth_capture(no_browser, manual_paste, loopback, oauth_code).await
        }
        AuthTarget::AnthropicOAuth => {
            if token.is_some() {
                bail!(
                    "Claude Pro uses browser OAuth (paste authorization code), not a static token. Run `edgecrab auth add claude-pro`"
                );
            }
            login_anthropic_oauth_capture(no_browser).await
        }
        AuthTarget::OpenaiCodex => {
            if token.is_some() {
                bail!(
                    "ChatGPT Pro / Codex uses device-code OAuth, not a static token. Run `edgecrab auth add chatgpt-pro`"
                );
            }
            login_openai_codex_capture().await
        }
        AuthTarget::Copilot => {
            let token = token.as_deref();
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
            let token = token.as_deref().ok_or_else(|| {
                anyhow!("`edgecrab auth add {raw_target}` requires `--token <bearer-token>`")
            })?;
            write_mcp_token(&name, token.trim())
                .with_context(|| format!("failed to write token for MCP server '{name}'"))?;
            Ok(format!("Stored bearer token for MCP server '{name}'."))
        }
        AuthTarget::Provider(spec) => {
            let token = token.as_deref().ok_or_else(|| {
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
        AuthTarget::NousPortal => {
            let path = edgecrab_proxy::default_auth_path();
            edgecrab_proxy::remove_provider_state(&path, "nous")
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(format!(
                "Removed Nous Portal credentials from {}.",
                path.display()
            ))
        }
        AuthTarget::XaiOAuth => {
            let path = edgecrab_proxy::default_auth_path();
            edgecrab_proxy::remove_provider_state(&path, edgecrab_proxy::XAI_OAUTH_PROVIDER)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(format!(
                "Removed xAI Grok OAuth credentials from {}.",
                path.display()
            ))
        }
        AuthTarget::AnthropicOAuth => {
            remove_anthropic_oauth_file().map_err(anyhow::Error::msg)?;
            Ok(format!(
                "Removed Claude Pro OAuth credentials from {}.",
                edgecrab_core::oauth::anthropic_oauth_path().display()
            ))
        }
        AuthTarget::OpenaiCodex => {
            remove_codex_oauth(None).map_err(anyhow::Error::msg)?;
            Ok(format!(
                "Removed ChatGPT Pro / Codex OAuth from {}.",
                edgecrab_core::oauth::auth_store::default_auth_path().display()
            ))
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
    clear_proxy_oauth_providers()?;
    let _ = remove_anthropic_oauth_file();
    let _ = remove_codex_oauth(None);
    Ok("Cleared EdgeCrab-managed Copilot, Claude/Codex OAuth, provider, proxy OAuth (nous/grok), and MCP auth caches.".into())
}

fn clear_proxy_oauth_providers() -> anyhow::Result<()> {
    let path = edgecrab_proxy::default_auth_path();
    for provider in [
        "nous",
        edgecrab_proxy::XAI_OAUTH_PROVIDER,
        OPENAI_CODEX_PROVIDER,
    ] {
        let _ = edgecrab_proxy::remove_provider_state(&path, provider);
    }
    Ok(())
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

fn show_nous_status() -> anyhow::Result<String> {
    let path = edgecrab_proxy::default_auth_path();
    let probe = edgecrab_proxy::probe_oauth_auth(&edgecrab_proxy::RECIPE_NOUS);
    let mut out = String::from("nous (Nous Portal OAuth)\n");
    writeln!(out, "auth-file:  {}", path.display())?;
    writeln!(
        out,
        "status:     {}",
        edgecrab_proxy::auth_probe_message(&edgecrab_proxy::RECIPE_NOUS, probe)
    )?;
    writeln!(
        out,
        "login:      edgecrab auth add nous  |  edgecrab auth login nous"
    )?;
    writeln!(
        out,
        "proxy:      edgecrab proxy enable nous && edgecrab proxy start --provider nous"
    )?;
    Ok(out.trim_end().to_string())
}

fn show_anthropic_oauth_status() -> anyhow::Result<String> {
    let path = edgecrab_core::oauth::anthropic_oauth_path();
    let creds = read_anthropic_oauth_file().map_err(anyhow::Error::msg)?;
    let mut out = String::from("claude-pro / anthropic (Claude Pro / Max OAuth)\n");
    writeln!(out, "oauth-file: {}", path.display())?;
    writeln!(
        out,
        "logged-in:  {}",
        yes_no(creds.as_ref().is_some_and(|c| !c.access_token.is_empty()))
    )?;
    if let Some(c) = creds {
        writeln!(
            out,
            "expires-at: {}",
            if c.expires_at_ms > 0 {
                c.expires_at_ms.to_string()
            } else {
                "unknown".into()
            }
        )?;
    }
    writeln!(
        out,
        "login:      edgecrab auth add claude-pro  |  edgecrab auth login claude-pro"
    )?;
    writeln!(
        out,
        "model:      anthropic/claude-sonnet-4  (OAuth token used when ANTHROPIC_API_KEY unset)"
    )?;
    Ok(out.trim_end().to_string())
}

fn show_openai_codex_status() -> anyhow::Result<String> {
    let path = edgecrab_core::oauth::auth_store::default_auth_path();
    let ready = edgecrab_core::oauth::codex_has_credentials(&path);
    let mut out = String::from("chatgpt-pro / openai-codex (ChatGPT Pro device OAuth)\n");
    writeln!(out, "auth-file:  {}", path.display())?;
    writeln!(out, "logged-in:  {}", yes_no(ready))?;
    writeln!(
        out,
        "login:      edgecrab auth add chatgpt-pro  |  edgecrab auth login chatgpt-pro"
    )?;
    Ok(out.trim_end().to_string())
}

fn show_xai_oauth_status() -> anyhow::Result<String> {
    let path = edgecrab_proxy::default_auth_path();
    let probe = edgecrab_proxy::probe_oauth_auth(&edgecrab_proxy::RECIPE_XAI);
    let mut out = String::from("grok / xai-oauth (SuperGrok / X Premium+)\n");
    writeln!(out, "auth-file:  {}", path.display())?;
    writeln!(
        out,
        "status:     {}",
        edgecrab_proxy::auth_probe_message(&edgecrab_proxy::RECIPE_XAI, probe)
    )?;
    writeln!(
        out,
        "login:      edgecrab auth add grok  |  edgecrab auth add xai-oauth"
    )?;
    writeln!(
        out,
        "remote:     EDGECRAB_AUTH_NO_BROWSER=1 or EDGECRAB_AUTH_MANUAL_PASTE=1"
    )?;
    writeln!(
        out,
        "proxy:      edgecrab proxy enable grok && edgecrab proxy start --provider xai"
    )?;
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
    if matches!(target, "nous" | "nous-portal" | "nous_portal") {
        return Ok(AuthTarget::NousPortal);
    }
    if is_xai_oauth_target(target) {
        return Ok(AuthTarget::XaiOAuth);
    }
    if is_anthropic_oauth_alias(target) {
        return Ok(AuthTarget::AnthropicOAuth);
    }
    if is_openai_codex_alias(target) {
        return Ok(AuthTarget::OpenaiCodex);
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
        "unknown auth target '{target}' (expected `copilot`, `nous`, `grok`, `claude-pro`, `chatgpt-pro`, `provider/<name>`, `mcp/<server>`, or a configured MCP server name)"
    )
}

fn is_xai_oauth_target(target: &str) -> bool {
    edgecrab_core::oauth::is_xai_oauth_alias(target)
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
    "Usage: /auth [list|status [target]|add <target> [--token <secret>]|login [target]|remove <target>|reset [target]]\nTargets: copilot, nous, grok, claude-pro, chatgpt-pro, provider/<name>, mcp/<server>, or a configured MCP server name"
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

/// True when `/model` Ctrl+D can clear managed credentials for a catalog provider slug.
pub fn provider_disconnect_supported(catalog_provider: &str) -> bool {
    let canonical = edgecrab_core::normalize_discovery_provider(catalog_provider);
    if resolve_provider(&canonical).is_some() {
        return true;
    }
    is_anthropic_oauth_alias(&canonical)
        || is_openai_codex_alias(&canonical)
        || is_xai_oauth_target(&canonical)
}

/// Synchronous provider disconnect for the TUI model picker (Hermes `model.disconnect` parity).
pub fn disconnect_catalog_provider(catalog_provider: &str) -> Result<String, String> {
    let canonical = edgecrab_core::normalize_discovery_provider(catalog_provider);
    if let Some(spec) = resolve_provider(&canonical) {
        for env_var in spec.env_vars {
            gateway_setup::remove_env_key(env_var).map_err(|e| e.to_string())?;
        }
        remove_provider_auth_state(spec.canonical).map_err(|e| e.to_string())?;
        return Ok(format!(
            "Removed {} from {} and {}.",
            spec.description,
            spec.env_vars.join(", "),
            auth_store_path().display()
        ));
    }
    if is_anthropic_oauth_alias(&canonical) {
        remove_anthropic_oauth_file().map_err(|e| e.to_string())?;
        return Ok(format!(
            "Removed Claude Pro OAuth credentials from {}.",
            edgecrab_core::oauth::anthropic_oauth_path().display()
        ));
    }
    if is_openai_codex_alias(&canonical) {
        remove_codex_oauth(None).map_err(|e| e.to_string())?;
        return Ok(format!(
            "Removed ChatGPT Pro / Codex OAuth from {}.",
            edgecrab_core::oauth::auth_store::default_auth_path().display()
        ));
    }
    if is_xai_oauth_target(&canonical) {
        let path = edgecrab_proxy::default_auth_path();
        edgecrab_proxy::remove_provider_state(&path, edgecrab_proxy::XAI_OAUTH_PROVIDER)
            .map_err(|e| e.to_string())?;
        return Ok(format!(
            "Removed xAI Grok OAuth credentials from {}.",
            path.display()
        ));
    }
    Err(format!(
        "No managed credentials to disconnect for provider '{catalog_provider}'."
    ))
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
            AuthCommand::Add {
                target,
                token,
                no_browser,
                manual_paste,
                loopback,
                oauth_code,
            } => {
                assert_eq!(target, "provider/openai");
                assert_eq!(token.as_deref(), Some("sk-test"));
                assert!(!manual_paste);
                assert!(!loopback);
                assert!(oauth_code.is_none());
                assert!(!no_browser);
                assert!(!manual_paste);
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
    fn login_without_target_defaults_to_copilot() {
        let target = login_target_from_slash_args("").unwrap();
        assert_eq!(target, "copilot");
    }

    #[test]
    fn resolves_provider_alias() {
        let target = resolve_provider("google").expect("provider alias");
        assert_eq!(target.canonical, "gemini");
    }

    #[test]
    fn resolves_nous_auth_target() {
        let target = resolve_target("nous").expect("nous target");
        assert!(matches!(target, AuthTarget::NousPortal));
    }

    #[test]
    fn resolves_grok_auth_target() {
        let target = resolve_target("grok").expect("grok target");
        assert!(matches!(target, AuthTarget::XaiOAuth));
        let target = resolve_target("xai-oauth").expect("xai-oauth target");
        assert!(matches!(target, AuthTarget::XaiOAuth));
    }

    #[test]
    fn extract_grok_code_from_url_or_token() {
        let url = "http://127.0.0.1:56121/callback?code=abcDEF123&state=s";
        let code = extract_grok_auth_code(url).expect("from url");
        assert_eq!(code, "abcDEF123");
        let raw = extract_grok_auth_code("XRG_tntFEcKoU8").expect("raw");
        assert_eq!(raw, "XRG_tntFEcKoU8");
    }

    #[test]
    fn mask_grok_code_hides_middle() {
        let masked = mask_grok_code("abcdefghijklmnop");
        assert!(masked.contains("abcd"));
        assert!(masked.contains("mnop"));
    }

    #[test]
    fn resolves_claude_pro_auth_target() {
        let target = resolve_target("claude-pro").expect("claude-pro");
        assert!(matches!(target, AuthTarget::AnthropicOAuth));
        let target = resolve_target("anthropic").expect("anthropic oauth");
        assert!(matches!(target, AuthTarget::AnthropicOAuth));
    }

    #[test]
    fn resolves_chatgpt_pro_auth_target() {
        let target = resolve_target("chatgpt-pro").expect("chatgpt-pro");
        assert!(matches!(target, AuthTarget::OpenaiCodex));
        let target = resolve_target("openai-codex").expect("openai-codex");
        assert!(matches!(target, AuthTarget::OpenaiCodex));
    }

    #[test]
    fn target_uses_interactive_oauth_for_claude_and_codex() {
        assert!(target_uses_interactive_oauth("claude-pro"));
        assert!(target_uses_interactive_oauth("chatgpt-pro"));
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

    #[test]
    fn catalog_provider_disconnect_supported_for_known_providers() {
        assert!(provider_disconnect_supported("openai"));
        assert!(provider_disconnect_supported("anthropic"));
        assert!(!provider_disconnect_supported("totally-unknown-provider"));
    }

    #[test]
    fn copilot_device_prompt_is_compact_and_copyable() {
        let prompt = CopilotDevicePrompt {
            open_url: "https://github.com/login/device?user_code=ABCD-EFGH".into(),
            display_url: "https://github.com/login/device".into(),
            user_code: "ABCD-EFGH".into(),
        };

        let rendered = render_copilot_device_prompt(&prompt);
        assert!(rendered.contains("https://github.com/login/device"));
        assert!(rendered.contains("ABCD-EFGH"));
        assert!(rendered.contains("drag to select the code with your mouse"));
        for line in rendered.lines() {
            assert!(line.chars().count() <= 72, "line too wide: {line}");
        }
    }

    #[test]
    fn friendly_copilot_login_errors_cover_common_device_flow_cases() {
        assert_eq!(
            friendly_copilot_login_error("expired_token"),
            "The login code expired before GitHub approval. Run /login again to generate a fresh code."
        );
        assert_eq!(
            friendly_copilot_login_error("access_denied"),
            "GitHub approval was cancelled. Run /login again when you want to retry."
        );
        assert_eq!(
            friendly_copilot_login_error("network hiccup"),
            "network hiccup"
        );
    }
}
