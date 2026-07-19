//! xAI Grok OAuth PKCE login (`edgecrab auth add xai-oauth` / `edgecrab auth add grok`).

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use crate::backend::auth_file::{default_auth_path, write_provider_state};
use crate::error::ProxyError;
use crate::http_client::build_oauth_http_client;
use crate::oauth::loopback::{
    LOOPBACK_HOST, LoopbackServer, OAuthCallback, validate_loopback_redirect_uri,
};
use edgecrab_core::oauth::pkce::{code_challenge, code_verifier};

use super::refresh::{DEFAULT_XAI_API, XAI_OAUTH_CLIENT_ID, XAI_OAUTH_DISCOVERY_URL};

pub use edgecrab_core::oauth::XAI_OAUTH_PROVIDER;
pub const XAI_OAUTH_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
pub const XAI_OAUTH_REDIRECT_PORT: u16 = 56121;
pub const XAI_OAUTH_REDIRECT_PATH: &str = "/callback";
pub const XAI_OAUTH_REFERRER: &str = "edgecrab";

const PENDING_SESSION_VERSION: u32 = 1;
/// OAuth codes and PKCE verifiers are short-lived; reject stale pending files.
pub const PENDING_SESSION_MAX_AGE_SECS: i64 = 1800;

/// Saved PKCE session between `start` and `finish` (two-step Grok login).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XaiOAuthPendingSession {
    pub version: u32,
    pub verifier: String,
    pub challenge: String,
    pub state: String,
    pub redirect_uri: String,
    pub authorize_url: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub created_at: String,
}

/// Result of `start_xai_oauth_login` — user completes in browser, then runs `finish`.
#[derive(Debug, Clone)]
pub struct XaiOAuthStarted {
    pub authorize_url: String,
    pub pending_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct XaiDiscovery {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub raw: Value,
}

/// Prompt payload for CLI/TUI (mirrors Copilot device-code UX hook).
#[derive(Debug, Clone)]
pub struct XaiOAuthAuthorizePrompt {
    pub authorize_url: String,
    pub redirect_uri: String,
    pub manual_paste: bool,
}

#[derive(Clone, Default)]
pub struct XaiOAuthLoginOptions {
    pub open_browser: bool,
    pub manual_paste: bool,
    pub timeout_secs: u64,
    /// Pre-supplied authorization code (bypasses stdin; for flaky terminals).
    pub pasted_code: Option<String>,
    /// When set, EdgeCrab CLI owns sign-in output and browser open (like Copilot `device_code_flow`).
    pub on_authorize: Option<Arc<dyn Fn(XaiOAuthAuthorizePrompt) + Send + Sync>>,
}

impl std::fmt::Debug for XaiOAuthLoginOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XaiOAuthLoginOptions")
            .field("open_browser", &self.open_browser)
            .field("manual_paste", &self.manual_paste)
            .field("timeout_secs", &self.timeout_secs)
            .field("pasted_code", &self.pasted_code.is_some())
            .field("on_authorize", &self.on_authorize.is_some())
            .finish()
    }
}

fn emit_authorize_prompt(opts: &XaiOAuthLoginOptions, authorize_url: &str, redirect_uri: &str) {
    let prompt = XaiOAuthAuthorizePrompt {
        authorize_url: authorize_url.to_string(),
        redirect_uri: redirect_uri.to_string(),
        manual_paste: opts.manual_paste,
    };
    if let Some(hook) = &opts.on_authorize {
        hook(prompt);
        return;
    }
    if opts.manual_paste {
        eprintln!("\nxAI Grok OAuth (manual paste)");
        eprintln!("===========================\n");
    } else {
        eprintln!("\nxAI Grok OAuth");
        eprintln!("==============\n");
    }
    eprintln!("Open this URL to authorize EdgeCrab with xAI:\n{authorize_url}\n");
    if !opts.manual_paste {
        eprintln!("Waiting for callback on {redirect_uri}");
        eprintln!(
            "(Remote host? SSH tunnel: ssh -L {XAI_OAUTH_REDIRECT_PORT}:127.0.0.1:{XAI_OAUTH_REDIRECT_PORT} user@host)\n"
        );
        if opts.open_browser {
            open_browser(authorize_url);
            eprintln!("Browser opened (if supported).");
        }
    }
}

fn build_http_client(timeout: Duration) -> Result<Client, ProxyError> {
    build_oauth_http_client(timeout)
}

fn validate_xai_https_endpoint(url: &str, field: &str) -> Result<String, ProxyError> {
    let parsed =
        Url::parse(url).map_err(|e| ProxyError::UpstreamAuth(format!("xai {field} parse: {e}")))?;
    if parsed.scheme() != "https" {
        return Err(ProxyError::UpstreamAuth(format!(
            "xAI OIDC discovery returned non-HTTPS {field}"
        )));
    }
    let host = parsed.host_str().unwrap_or("").to_lowercase();
    if host != "x.ai" && !host.ends_with(".x.ai") {
        return Err(ProxyError::UpstreamAuth(format!(
            "xAI OIDC {field} host {host} is not on x.ai"
        )));
    }
    Ok(url.to_string())
}

pub async fn fetch_xai_discovery(client: &Client) -> Result<XaiDiscovery, ProxyError> {
    let resp = client
        .get(XAI_OAUTH_DISCOVERY_URL)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ProxyError::UpstreamAuth(format!("xai OIDC discovery failed: {e}")))?;
    let body = resp
        .text()
        .await
        .map_err(|e| ProxyError::UpstreamAuth(format!("xai discovery body: {e}")))?;
    let payload: Value = serde_json::from_str(&body)
        .map_err(|e| ProxyError::UpstreamAuth(format!("xai discovery JSON: {e}")))?;
    let authorization_endpoint = payload
        .get("authorization_endpoint")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ProxyError::UpstreamAuth("xai discovery missing authorization_endpoint".into())
        })?;
    let token_endpoint = payload
        .get("token_endpoint")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProxyError::UpstreamAuth("xai discovery missing token_endpoint".into()))?;
    Ok(XaiDiscovery {
        authorization_endpoint: validate_xai_https_endpoint(
            authorization_endpoint,
            "authorization_endpoint",
        )?,
        token_endpoint: validate_xai_https_endpoint(token_endpoint, "token_endpoint")?,
        raw: payload,
    })
}

pub fn validate_xai_inference_base_url(value: Option<&str>) -> String {
    let fallback = DEFAULT_XAI_API.trim_end_matches('/').to_string();
    let candidate = value.unwrap_or("").trim().trim_end_matches('/');
    if candidate.is_empty() {
        return fallback;
    }
    let Ok(parsed) = Url::parse(candidate) else {
        return fallback;
    };
    if parsed.scheme() != "https" {
        return fallback;
    }
    let host = parsed.host_str().unwrap_or("").to_lowercase();
    if host != "x.ai" && !host.ends_with(".x.ai") {
        return fallback;
    }
    candidate.to_string()
}

fn build_authorize_url(
    authorization_endpoint: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
    nonce: &str,
) -> String {
    let mut url = Url::parse(authorization_endpoint)
        .unwrap_or_else(|_| Url::parse("https://accounts.x.ai/").expect("fallback"));
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", XAI_OAUTH_CLIENT_ID);
        q.append_pair("redirect_uri", redirect_uri);
        q.append_pair("scope", XAI_OAUTH_SCOPE);
        q.append_pair("code_challenge", code_challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("state", state);
        q.append_pair("nonce", nonce);
        q.append_pair("plan", "generic");
        q.append_pair("referrer", XAI_OAUTH_REFERRER);
    }
    url.to_string()
}

pub async fn exchange_authorization_code(
    client: &Client,
    token_endpoint: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code_challenge: &str,
) -> Result<Value, ProxyError> {
    if code_verifier.is_empty() {
        return Err(ProxyError::UpstreamAuth(
            "xAI token exchange: PKCE code_verifier is empty".into(),
        ));
    }
    // RFC 7636 token request: grant_type, code, redirect_uri, client_id, code_verifier.
    // Do NOT send code_challenge on the token endpoint (authorization only) — some
    // OIDC servers reject or hang on unknown form fields.
    let _ = code_challenge;
    let form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", XAI_OAUTH_CLIENT_ID),
        ("code_verifier", code_verifier),
    ];
    tracing::debug!(
        %token_endpoint,
        code_len = code.len(),
        "xAI token exchange starting"
    );
    let resp = client
        .post(token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|e| ProxyError::UpstreamAuth(format!("xai token exchange failed: {e}")))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| ProxyError::UpstreamAuth(format!("xai token exchange body: {e}")))?;
    if status.as_u16() == 403 {
        return Err(ProxyError::UpstreamAuth(format!(
            "xAI token exchange HTTP 403: {body} — account may lack SuperGrok API access; try XAI_API_KEY or https://x.ai/grok"
        )));
    }
    if !status.is_success() {
        return Err(ProxyError::UpstreamAuth(format!(
            "xAI token exchange HTTP {}: {body}",
            status.as_u16()
        )));
    }
    tracing::info!(status = status.as_u16(), "xAI token exchange OK");
    serde_json::from_str(&body)
        .map_err(|e| ProxyError::UpstreamAuth(format!("xai token exchange JSON: {e}")))
}

fn build_provider_state(
    tokens: Value,
    discovery: &XaiDiscovery,
    redirect_uri: &str,
    base_url: &str,
) -> Value {
    json!({
        "tokens": tokens,
        "discovery": discovery.raw,
        "oauth_discovery": discovery.raw,
        "redirect_uri": redirect_uri,
        "auth_mode": "oauth_pkce",
        "base_url": base_url,
        "last_refresh": Utc::now().to_rfc3339(),
    })
}

fn tokens_from_exchange(payload: &Value) -> Result<Value, ProxyError> {
    let access = payload
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProxyError::UpstreamAuth("xAI exchange missing access_token".into()))?;
    let refresh = payload
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProxyError::UpstreamAuth("xAI exchange missing refresh_token".into()))?;
    Ok(json!({
        "access_token": access,
        "refresh_token": refresh,
        "id_token": payload.get("id_token").and_then(|v| v.as_str()).unwrap_or(""),
        "expires_in": payload.get("expires_in"),
        "token_type": payload.get("token_type").and_then(|v| v.as_str()).unwrap_or("Bearer"),
    }))
}

fn parse_manual_callback_url(
    input: &str,
    expected_redirect: &str,
) -> Result<OAuthCallback, ProxyError> {
    let trimmed = input
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`');
    if trimmed.is_empty() {
        return Err(ProxyError::UpstreamAuth("empty callback paste".into()));
    }
    // Accept callback fragments (e.g. "code=...&state=..." or "?code=...").
    if trimmed.starts_with("?") || trimmed.starts_with("code=") || trimmed.starts_with("error=") {
        let raw = trimmed.trim_start_matches('?');
        let mut code = None;
        let mut state = None;
        let mut error = None;
        let mut error_description = None;
        for (k, v) in url::form_urlencoded::parse(raw.as_bytes()) {
            match k.as_ref() {
                "code" => code = Some(v.into_owned()),
                "state" => state = Some(v.into_owned()),
                "error" => error = Some(v.into_owned()),
                "error_description" => error_description = Some(v.into_owned()),
                _ => {}
            }
        }
        return Ok(OAuthCallback {
            code,
            state,
            error,
            error_description,
        });
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let url = Url::parse(trimmed)
            .map_err(|e| ProxyError::UpstreamAuth(format!("invalid callback URL: {e}")))?;
        let base = Url::parse(expected_redirect)
            .map_err(|e| ProxyError::UpstreamAuth(format!("expected redirect: {e}")))?;
        if url.path() != base.path() {
            return Err(ProxyError::UpstreamAuth(
                "callback URL path mismatch".into(),
            ));
        }
        let mut code = None;
        let mut state = None;
        let mut error = None;
        let mut error_description = None;
        for (k, v) in url.query_pairs() {
            match k.as_ref() {
                "code" => code = Some(v.into_owned()),
                "state" => state = Some(v.into_owned()),
                "error" => error = Some(v.into_owned()),
                "error_description" => error_description = Some(v.into_owned()),
                _ => {}
            }
        }
        return Ok(OAuthCallback {
            code,
            state,
            error,
            error_description,
        });
    }
    Ok(OAuthCallback {
        code: Some(trimmed.to_string()),
        state: None,
        error: None,
        error_description: None,
    })
}

/// Extract the OAuth `code` from a pasted callback URL, query string, or raw token.
pub fn extract_xai_oauth_code_from_paste(input: &str) -> Result<String, ProxyError> {
    let redirect_uri =
        format!("http://{LOOPBACK_HOST}:{XAI_OAUTH_REDIRECT_PORT}{XAI_OAUTH_REDIRECT_PATH}");
    let cb = parse_manual_callback_url(input, &redirect_uri)?;
    cb.code
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProxyError::UpstreamAuth("no authorization code in pasted text".into()))
}

fn normalize_pasted_input(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .replace('\r', "")
}

fn print_stdin_paste_help() {
    eprintln!("\nPaste your authorization code on ONE line, then press Enter.");
    eprintln!(
        "(Code must be on the same line as the prompt — Enter only submits what read_line() receives.)"
    );
    eprintln!("No terminal paste? Use:");
    eprintln!("  edgecrab auth grok finish --oauth-code 'YOUR_CODE'");
}

fn read_pasted_code_from_stdin() -> Result<String, ProxyError> {
    print_stdin_paste_help();

    for attempt in 1..=5 {
        eprint!("code> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => {
                return Err(ProxyError::UpstreamAuth(
                    "stdin closed before authorization code was received".into(),
                ));
            }
            Err(e) => {
                return Err(ProxyError::UpstreamAuth(format!("stdin: {e}")));
            }
            Ok(_) => {
                let normalized = normalize_pasted_input(&line);
                if normalized.is_empty() {
                    if attempt < 5 {
                        eprintln!("(empty line — paste the code on this line, then Enter)");
                        continue;
                    }
                    return Err(ProxyError::UpstreamAuth("empty callback paste".into()));
                }
                return Ok(normalized);
            }
        }
    }

    Err(ProxyError::UpstreamAuth(
        "no authorization code received from stdin after 5 attempts".into(),
    ))
}

fn prompt_manual_paste(redirect_uri: &str) -> Result<OAuthCallback, ProxyError> {
    let code = read_pasted_code_from_stdin()?;
    parse_manual_callback_url(&code, redirect_uri)
}

async fn read_pasted_callback_async(redirect_uri: &str) -> Result<OAuthCallback, ProxyError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    print_stdin_paste_help();
    let mut reader = BufReader::new(tokio::io::stdin());
    for attempt in 1..=5 {
        eprint!("code> ");
        let _ = tokio::io::stdout().flush().await;
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                return Err(ProxyError::UpstreamAuth(
                    "stdin closed before authorization code was received".into(),
                ));
            }
            Err(e) => {
                return Err(ProxyError::UpstreamAuth(format!("stdin: {e}")));
            }
            Ok(_) => {
                let normalized = normalize_pasted_input(&line);
                if normalized.is_empty() {
                    if attempt < 5 {
                        eprintln!("(empty line — paste the code on this line, then Enter)");
                        continue;
                    }
                    return Err(ProxyError::UpstreamAuth("empty callback paste".into()));
                }
                return parse_manual_callback_url(&normalized, redirect_uri);
            }
        }
    }
    Err(ProxyError::UpstreamAuth(
        "no authorization code received from stdin after 5 attempts".into(),
    ))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).status();

    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).status();

    #[cfg(windows)]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .status();
}

pub fn default_xai_pending_path() -> PathBuf {
    edgecrab_core::config::edgecrab_home()
        .join("oauth-pending")
        .join("xai-grok.json")
}

/// Load a non-expired pending session, if any. Stale files are removed (same as `finish`).
pub fn peek_xai_pending_session(path: Option<&Path>) -> Option<XaiOAuthPendingSession> {
    let path = path.map_or_else(default_xai_pending_path, |p| p.to_path_buf());
    load_pending_session(&path).ok()
}

fn write_pending_session(path: &Path, session: &XaiOAuthPendingSession) -> Result<(), ProxyError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ProxyError::UpstreamAuth(format!("oauth-pending dir: {e}")))?;
    }
    let json = serde_json::to_string_pretty(session)
        .map_err(|e| ProxyError::UpstreamAuth(format!("oauth-pending serialize: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| ProxyError::UpstreamAuth(format!("oauth-pending write: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn load_pending_session(path: &Path) -> Result<XaiOAuthPendingSession, ProxyError> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        ProxyError::UpstreamAuth(format!(
            "no pending Grok OAuth session at {} ({e}). Run `edgecrab auth grok start` first.",
            path.display()
        ))
    })?;
    let session: XaiOAuthPendingSession = serde_json::from_str(&raw)
        .map_err(|e| ProxyError::UpstreamAuth(format!("oauth-pending parse: {e}")))?;
    if session.version != PENDING_SESSION_VERSION {
        return Err(ProxyError::UpstreamAuth(
            "pending Grok OAuth session version mismatch — run `edgecrab auth grok start` again"
                .into(),
        ));
    }
    let created = DateTime::parse_from_rfc3339(&session.created_at)
        .map_err(|e| ProxyError::UpstreamAuth(format!("oauth-pending timestamp: {e}")))?;
    let age = Utc::now().signed_duration_since(created.with_timezone(&Utc));
    if age.num_seconds() > PENDING_SESSION_MAX_AGE_SECS {
        let _ = std::fs::remove_file(path);
        let mins = PENDING_SESSION_MAX_AGE_SECS / 60;
        return Err(ProxyError::UpstreamAuth(format!(
            "pending Grok OAuth session expired ({mins} min). Run `edgecrab auth grok start` again."
        )));
    }
    Ok(session)
}

fn clear_pending_session(path: &Path) {
    let _ = std::fs::remove_file(path);
}

async fn prepare_xai_oauth_session(
    client: &Client,
) -> Result<(XaiDiscovery, XaiOAuthPendingSession), ProxyError> {
    let discovery = fetch_xai_discovery(client).await?;
    let verifier = code_verifier();
    let challenge = code_challenge(&verifier);
    let state = uuid::Uuid::new_v4().simple().to_string();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let redirect_uri =
        format!("http://{LOOPBACK_HOST}:{XAI_OAUTH_REDIRECT_PORT}{XAI_OAUTH_REDIRECT_PATH}");
    validate_loopback_redirect_uri(&redirect_uri)?;
    let authorize_url = build_authorize_url(
        &discovery.authorization_endpoint,
        &redirect_uri,
        &challenge,
        &state,
        &nonce,
    );
    let pending = XaiOAuthPendingSession {
        version: PENDING_SESSION_VERSION,
        verifier,
        challenge,
        state,
        redirect_uri,
        authorize_url: authorize_url.clone(),
        authorization_endpoint: discovery.authorization_endpoint.clone(),
        token_endpoint: discovery.token_endpoint.clone(),
        created_at: Utc::now().to_rfc3339(),
    };
    Ok((discovery, pending))
}

async fn exchange_pending_session(
    client: &Client,
    discovery: &XaiDiscovery,
    pending: &XaiOAuthPendingSession,
    callback: OAuthCallback,
) -> Result<Value, ProxyError> {
    if let Some(err) = callback.error.as_deref() {
        let detail = callback.error_description.as_deref().unwrap_or(err);
        return Err(ProxyError::UpstreamAuth(format!(
            "xAI authorization failed: {detail}"
        )));
    }

    let mut callback_state = callback.state.clone();
    if callback_state.is_none() {
        callback_state = Some(pending.state.clone());
    }
    if callback_state.as_deref() != Some(pending.state.as_str()) {
        return Err(ProxyError::UpstreamAuth(
            "xAI authorization: state mismatch".into(),
        ));
    }

    let code = callback
        .code
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProxyError::UpstreamAuth("xAI authorization: missing code".into()))?;

    let payload = exchange_authorization_code(
        client,
        &pending.token_endpoint,
        code,
        &pending.redirect_uri,
        &pending.verifier,
        &pending.challenge,
    )
    .await?;

    let tokens = tokens_from_exchange(&payload)?;
    let base_url = validate_xai_inference_base_url(
        std::env::var("XAI_BASE_URL")
            .ok()
            .as_deref()
            .or(std::env::var("EDGECRAB_XAI_BASE_URL").ok().as_deref()),
    );

    Ok(build_provider_state(
        tokens,
        discovery,
        &pending.redirect_uri,
        &base_url,
    ))
}

/// Step 1: open browser / copy URL; persist PKCE so step 2 can run in a fresh command.
pub async fn start_xai_oauth_login(
    opts: &XaiOAuthLoginOptions,
) -> Result<XaiOAuthStarted, ProxyError> {
    let client = build_http_client(Duration::from_secs(20))?;
    let (discovery, pending) = prepare_xai_oauth_session(&client).await?;
    let path = default_xai_pending_path();
    write_pending_session(&path, &pending)?;
    emit_authorize_prompt(opts, &pending.authorize_url, &pending.redirect_uri);
    let _ = discovery;
    Ok(XaiOAuthStarted {
        authorize_url: pending.authorize_url,
        pending_path: path,
    })
}

/// Step 2: submit authorization code (flag or stdin) and exchange tokens.
pub async fn finish_xai_oauth_login(
    code: Option<String>,
    pending_path: Option<&Path>,
    _opts: &XaiOAuthLoginOptions,
) -> Result<Value, ProxyError> {
    let path = pending_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_xai_pending_path);
    let pending = load_pending_session(&path)?;
    let client = build_http_client(Duration::from_secs(20))?;
    let discovery = XaiDiscovery {
        authorization_endpoint: pending.authorization_endpoint.clone(),
        token_endpoint: pending.token_endpoint.clone(),
        raw: Value::Null,
    };

    let normalized = match code {
        Some(c) => normalize_pasted_input(&c),
        None => read_pasted_code_from_stdin()?,
    };
    let callback = parse_manual_callback_url(&normalized, &pending.redirect_uri)?;
    // Keep pending file until tokens are persisted so a failed write can retry.
    let state = exchange_pending_session(&client, &discovery, &pending, callback).await?;
    Ok(state)
}

pub async fn login_xai_oauth_finish(
    auth_path: Option<&Path>,
    code: Option<String>,
    pending_path: Option<&Path>,
    opts: &XaiOAuthLoginOptions,
) -> Result<String, ProxyError> {
    let path = auth_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_auth_path);
    let pending_file = pending_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_xai_pending_path);
    let state = finish_xai_oauth_login(code, Some(&pending_file), opts).await?;
    persist_xai_oauth(&path, state).await?;
    clear_pending_session(&pending_file);
    Ok(format!(
        "SuperGrok OAuth saved to {}.\n\
         Use /model super-grok/grok-4.5 or restart with that default.",
        path.display()
    ))
}

/// Paste-first login: `start` + wait for code + `finish` (recommended for x.ai manual fallback).
pub async fn xai_oauth_login_paste_first(opts: &XaiOAuthLoginOptions) -> Result<Value, ProxyError> {
    let started = start_xai_oauth_login(opts).await?;
    finish_xai_oauth_login(opts.pasted_code.clone(), Some(&started.pending_path), opts).await
}

/// Run PKCE loopback (or manual-paste) login without persisting.
pub async fn xai_oauth_login(opts: &XaiOAuthLoginOptions) -> Result<Value, ProxyError> {
    if opts.manual_paste {
        return xai_oauth_login_paste_first(opts).await;
    }

    let pending_path = default_xai_pending_path();
    if let Some(code) = opts
        .pasted_code
        .as_ref()
        .map(|s| normalize_pasted_input(s))
        .filter(|s| !s.is_empty())
    {
        if pending_path.exists() {
            return finish_xai_oauth_login(Some(code), Some(&pending_path), opts).await;
        }
        return xai_oauth_login_paste_first(opts).await;
    }

    let timeout = Duration::from_secs(opts.timeout_secs.max(30));
    let client = build_http_client(Duration::from_secs(20))?;
    let discovery = fetch_xai_discovery(&client).await?;

    let verifier = code_verifier();
    let challenge = code_challenge(&verifier);
    let state = uuid::Uuid::new_v4().simple().to_string();
    let nonce = uuid::Uuid::new_v4().simple().to_string();

    let redirect_uri =
        format!("http://{LOOPBACK_HOST}:{XAI_OAUTH_REDIRECT_PORT}{XAI_OAUTH_REDIRECT_PATH}");
    validate_loopback_redirect_uri(&redirect_uri)?;

    let (redirect_uri, callback) = {
        let server =
            LoopbackServer::start(XAI_OAUTH_REDIRECT_PORT, XAI_OAUTH_REDIRECT_PATH).await?;
        let redirect_uri = server.redirect_uri.clone();
        validate_loopback_redirect_uri(&redirect_uri)?;
        let authorize_url = build_authorize_url(
            &discovery.authorization_endpoint,
            &redirect_uri,
            &challenge,
            &state,
            &nonce,
        );

        emit_authorize_prompt(opts, &authorize_url, &redirect_uri);
        eprintln!(
            "If the browser shows \"Could not establish connection\", paste the code below (Enter works immediately — no need to wait for timeout)."
        );

        let uri_for_stdin = redirect_uri.clone();
        let stdin_fut = read_pasted_callback_async(&uri_for_stdin);
        tokio::pin!(stdin_fut);

        let cb = tokio::select! {
            wait = server.wait_for_callback(timeout) => {
                match wait {
                    Ok(cb) => cb,
                    Err(_) => {
                        eprintln!("Loopback timed out (xAI could not reach {redirect_uri}).");
                        prompt_manual_paste(&redirect_uri)?
                    }
                }
            }
            pasted = &mut stdin_fut => {
                pasted?
            }
        };
        server.shutdown().await;
        (redirect_uri, cb)
    };

    let pending = XaiOAuthPendingSession {
        version: PENDING_SESSION_VERSION,
        verifier,
        challenge,
        state: state.clone(),
        redirect_uri: redirect_uri.clone(),
        authorize_url: String::new(),
        authorization_endpoint: discovery.authorization_endpoint.clone(),
        token_endpoint: discovery.token_endpoint.clone(),
        created_at: Utc::now().to_rfc3339(),
    };
    exchange_pending_session(&client, &discovery, &pending, callback).await
}

pub async fn persist_xai_oauth(auth_path: &Path, state: Value) -> Result<(), ProxyError> {
    write_provider_state(auth_path, XAI_OAUTH_PROVIDER, &state)?;
    Ok(())
}

/// Full login + persist to `auth_path` (default `~/.edgecrab/auth.json`).
pub async fn login_xai_oauth(
    auth_path: Option<&Path>,
    opts: &XaiOAuthLoginOptions,
) -> Result<String, ProxyError> {
    let path = auth_path
        .map(Path::to_path_buf)
        .unwrap_or_else(default_auth_path);
    let pending_file = default_xai_pending_path();
    let state = xai_oauth_login(opts).await?;
    persist_xai_oauth(&path, state).await?;
    clear_pending_session(&pending_file);
    Ok(format!(
        "SuperGrok OAuth saved to {}.\n\
         Use /model super-grok/grok-4.5 or restart with that default.",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::routing::post;
    use tokio::net::TcpListener;

    #[test]
    fn exchange_form_includes_pkce_challenge() {
        let v = code_verifier();
        let c = code_challenge(&v);
        assert!(!c.is_empty());
        assert_ne!(v, c);
    }

    #[test]
    fn extract_code_from_callback_url() {
        let url = "http://127.0.0.1:56121/callback?code=abc123&state=xyz";
        let code = extract_xai_oauth_code_from_paste(url).expect("extract");
        assert_eq!(code, "abc123");
    }

    #[test]
    fn extract_code_from_raw_token() {
        let code = extract_xai_oauth_code_from_paste("raw-token_value").expect("extract");
        assert_eq!(code, "raw-token_value");
    }

    #[test]
    fn parse_manual_callback_accepts_code_only() {
        let cb = parse_manual_callback_url("code-only-token", "http://127.0.0.1:56121/callback")
            .expect("parse");
        assert_eq!(cb.code.as_deref(), Some("code-only-token"));
        assert!(cb.state.is_none());
    }

    #[test]
    fn parse_manual_callback_accepts_query_fragment() {
        let cb =
            parse_manual_callback_url("?code=abc123&state=st-1", "http://127.0.0.1:56121/callback")
                .expect("parse");
        assert_eq!(cb.code.as_deref(), Some("abc123"));
        assert_eq!(cb.state.as_deref(), Some("st-1"));
    }

    #[tokio::test]
    async fn token_exchange_round_trip_mock_auth() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let token_url = format!("http://{addr}/token");

        let app = Router::new().route(
            "/token",
            post(
                |form: axum::Form<std::collections::HashMap<String, String>>| async move {
                    assert_eq!(
                        form.get("grant_type").map(String::as_str),
                        Some("authorization_code")
                    );
                    assert!(form.contains_key("code_verifier"));
                    // Token endpoint must not require authorization-only PKCE fields.
                    assert!(!form.contains_key("code_challenge"));
                    assert!(!form.contains_key("code_challenge_method"));
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "access_token": "at-test",
                            "refresh_token": "rt-test",
                            "expires_in": 3600,
                            "token_type": "Bearer"
                        })),
                    )
                },
            ),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        crate::http_client::enable_e2e_direct_http();
        let client = build_http_client(Duration::from_secs(5)).expect("client");
        let verifier = code_verifier();
        let challenge = code_challenge(&verifier);
        let payload = exchange_authorization_code(
            &client,
            &token_url,
            "authcode",
            "http://127.0.0.1:56121/callback",
            &verifier,
            &challenge,
        )
        .await
        .expect("exchange");
        assert_eq!(payload["access_token"], "at-test");
    }

    #[tokio::test]
    async fn full_login_with_simulated_callback() {
        let auth_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let auth_addr = auth_listener.local_addr().expect("addr");
        let discovery = serde_json::json!({
            "authorization_endpoint": format!("http://{auth_addr}/authorize"),
            "token_endpoint": format!("http://{auth_addr}/token"),
        });

        let auth_app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                axum::routing::get({
                    let discovery = discovery.clone();
                    move || async move { axum::Json(discovery.clone()) }
                }),
            )
            .route(
                "/token",
                post(
                    |form: axum::Form<std::collections::HashMap<String, String>>| async move {
                        assert!(form.contains_key("code_verifier"));
                        (
                            axum::http::StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "access_token": "at-mock",
                                "refresh_token": "rt-mock",
                                "expires_in": 3600
                            })),
                        )
                    },
                ),
            );
        tokio::spawn(async move {
            axum::serve(auth_listener, auth_app).await.expect("auth");
        });

        crate::http_client::enable_e2e_direct_http();

        let server = LoopbackServer::start(0, XAI_OAUTH_REDIRECT_PATH)
            .await
            .expect("loopback");
        let redirect = server.redirect_uri.clone();
        let verifier = code_verifier();
        let challenge = code_challenge(&verifier);
        let state = "state-test".to_string();
        let nonce = "nonce-test".to_string();

        let client = build_http_client(Duration::from_secs(5)).expect("client");
        let disc = XaiDiscovery {
            authorization_endpoint: format!("http://{auth_addr}/authorize"),
            token_endpoint: format!("http://{auth_addr}/token"),
            raw: discovery,
        };
        let _url = build_authorize_url(
            &disc.authorization_endpoint,
            &redirect,
            &challenge,
            &state,
            &nonce,
        );

        let callback_url = format!("{redirect}?code=mockcode&state={state}");
        let http = build_http_client(Duration::from_secs(5)).expect("http");
        let _ = http.get(&callback_url).send().await.expect("hit callback");

        let cb = server
            .wait_for_callback(Duration::from_secs(5))
            .await
            .expect("callback");
        server.shutdown().await;

        assert_eq!(cb.code.as_deref(), Some("mockcode"));
        let payload = exchange_authorization_code(
            &client,
            &disc.token_endpoint,
            "mockcode",
            &redirect,
            &verifier,
            &challenge,
        )
        .await
        .expect("exchange");
        let tokens = tokens_from_exchange(&payload).expect("tokens");
        let built = build_provider_state(tokens, &disc, &redirect, DEFAULT_XAI_API);
        assert_eq!(built["auth_mode"], "oauth_pkce");
    }
}
