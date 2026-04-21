use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use axum::Router;
use axum::extract::{Query, State};
use axum::response::Html;
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use edgecrab_tools::tools::mcp_client::{OAuthConfig, configured_servers, write_mcp_oauth_token};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use url::{Host, Url};

#[derive(Debug, Deserialize)]
struct DeviceAuthorizationResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug)]
struct OAuthTokenRecord {
    access_token: String,
    refresh_token: Option<String>,
    expires_at_epoch_secs: Option<u64>,
}

#[derive(Debug)]
struct LoopbackCallbackPayload {
    values: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopbackPortMode {
    Fixed(u16),
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopbackRedirectInfo {
    pub redirect_uri: String,
    pub port_mode: LoopbackPortMode,
}

#[derive(Debug, Clone)]
struct LoopbackRedirectPlan {
    original_url: Url,
    bind_host: String,
    path: String,
    port_mode: LoopbackPortMode,
}

pub async fn login_mcp_server<F>(server_name: &str, mut notify: F) -> anyhow::Result<String>
where
    F: FnMut(String) + Send,
{
    let server = configured_servers()
        .map_err(|err| anyhow!(err.to_string()))?
        .into_iter()
        .find(|server| server.name == server_name)
        .ok_or_else(|| anyhow!("Unknown MCP server '{server_name}'"))?;

    if server.url.is_none() {
        bail!("MCP server '{server_name}' uses stdio and does not support HTTP OAuth login");
    }

    let oauth = server
        .oauth
        .as_ref()
        .ok_or_else(|| anyhow!("MCP server '{server_name}' has no oauth block configured"))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("failed to create OAuth HTTP client")?;

    let grant = oauth.grant_type_label();
    let token = if grant == "device_code" {
        login_with_device_code(server_name, oauth, &client, &mut notify).await?
    } else if grant == "authorization_code" {
        login_with_authorization_code(server_name, oauth, &client, &mut notify).await?
    } else if oauth.device_authorization_url().is_some() {
        login_with_device_code(server_name, oauth, &client, &mut notify).await?
    } else if oauth.authorization_url().is_some() && oauth.redirect_url().is_some() {
        login_with_authorization_code(server_name, oauth, &client, &mut notify).await?
    } else {
        bail!(
            "MCP server '{server_name}' does not expose an interactive OAuth flow. Configure `oauth.device_authorization_url` for device flow or `oauth.authorization_url` + `oauth.redirect_url` for browser login."
        );
    };

    write_mcp_oauth_token(
        server_name,
        &token.access_token,
        token.refresh_token.as_deref(),
        token.expires_at_epoch_secs,
    )
    .with_context(|| format!("failed to persist OAuth token for '{server_name}'"))?;

    let refresh = if token.refresh_token.is_some() {
        "yes"
    } else {
        "no"
    };
    let expiry = token
        .expires_at_epoch_secs
        .map(|secs| secs.to_string())
        .unwrap_or_else(|| "none".into());

    Ok(format!(
        "OAuth login complete for '{server_name}'. Cached access_token=yes refresh_token={refresh} expires_at={expiry}"
    ))
}

async fn login_with_device_code<F>(
    server_name: &str,
    oauth: &OAuthConfig,
    client: &reqwest::Client,
    notify: &mut F,
) -> anyhow::Result<OAuthTokenRecord>
where
    F: FnMut(String) + Send,
{
    let device_authorization_url = oauth.device_authorization_url().ok_or_else(|| {
        anyhow!("MCP server '{server_name}' is missing oauth.device_authorization_url")
    })?;

    let params = build_authorization_request_params(oauth);
    let response = send_oauth_form(client, device_authorization_url, oauth, params)
        .await
        .with_context(|| format!("device authorization request failed for '{server_name}'"))?;
    if !response.status().is_success() {
        bail!(
            render_oauth_error(
                response,
                format!("Device authorization endpoint rejected '{server_name}'")
            )
            .await
        );
    }

    let device: DeviceAuthorizationResponse = response.json().await.with_context(|| {
        format!("device authorization response was invalid for '{server_name}'")
    })?;

    let launch_url = device
        .verification_uri_complete
        .as_deref()
        .unwrap_or(&device.verification_uri);
    if open_url_in_browser(launch_url).is_ok() {
        notify(format!(
            "Opened the OAuth verification page for '{server_name}' in your browser."
        ));
    }
    notify(format!(
        "OAuth device login for '{server_name}': visit {} and enter code {}",
        device.verification_uri, device.user_code
    ));

    let interval_secs = device.interval.unwrap_or(5).max(1);
    let deadline = current_epoch_secs() + device.expires_in.unwrap_or(900);

    loop {
        if current_epoch_secs() >= deadline {
            bail!("OAuth device login timed out for '{server_name}'");
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        let mut params = build_token_request_base(oauth);
        params.push((
            "grant_type".into(),
            "urn:ietf:params:oauth:grant-type:device_code".into(),
        ));
        params.push(("device_code".into(), device.device_code.clone()));

        let response = send_oauth_form(client, oauth.token_url(), oauth, params)
            .await
            .with_context(|| format!("device token polling failed for '{server_name}'"))?;
        if response.status().is_success() {
            return parse_token_response(response)
                .await
                .with_context(|| format!("device token response was invalid for '{server_name}'"));
        }

        let status = response.status();
        let payload: OAuthErrorResponse = response.json().await.unwrap_or(OAuthErrorResponse {
            error: None,
            error_description: None,
        });
        match payload.error.as_deref() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Some(other) => {
                let detail = payload.error_description.unwrap_or_default();
                bail!(
                    "{}",
                    format!("OAuth device login failed for '{server_name}': {other} {detail}")
                        .trim()
                );
            }
            None => bail!("OAuth device login failed for '{server_name}' with status {status}"),
        }
    }
}

async fn login_with_authorization_code<F>(
    server_name: &str,
    oauth: &OAuthConfig,
    client: &reqwest::Client,
    notify: &mut F,
) -> anyhow::Result<OAuthTokenRecord>
where
    F: FnMut(String) + Send,
{
    let authorization_url = oauth
        .authorization_url()
        .ok_or_else(|| anyhow!("MCP server '{server_name}' is missing oauth.authorization_url"))?;
    let redirect_url = oauth
        .redirect_url()
        .ok_or_else(|| anyhow!("MCP server '{server_name}' is missing oauth.redirect_url"))?;
    let callback = LoopbackCallback::bind(redirect_url)
        .await
        .with_context(|| format!("redirect_url is invalid for '{server_name}'"))?;
    let effective_redirect_uri = callback.redirect_uri().to_string();

    let state = random_urlsafe(24);
    let code_verifier = oauth.uses_pkce().then(|| random_urlsafe(48));
    let auth_url = build_authorization_url(
        authorization_url,
        oauth,
        &effective_redirect_uri,
        &state,
        code_verifier.as_deref(),
    )?;

    if callback.uses_dynamic_port() {
        notify(format!(
            "Using dynamic loopback redirect {} for '{server_name}'.",
            callback.redirect_uri()
        ));
    }

    if open_url_in_browser(auth_url.as_str()).is_ok() {
        notify(format!(
            "Opened the OAuth authorization page for '{server_name}' in your browser."
        ));
    }
    notify(format!(
        "OAuth browser login for '{server_name}': if the browser did not open, visit {}",
        auth_url
    ));

    let payload = callback.wait_for_callback().await?;
    if let Some(error) = payload.values.get("error") {
        let detail = payload
            .values
            .get("error_description")
            .cloned()
            .unwrap_or_default();
        bail!(
            "{}",
            format!("OAuth authorization failed for '{server_name}': {error} {detail}").trim()
        );
    }

    let returned_state = payload
        .values
        .get("state")
        .ok_or_else(|| anyhow!("OAuth callback for '{server_name}' did not include state"))?;
    if returned_state != &state {
        bail!("OAuth callback state mismatch for '{server_name}'");
    }

    let code = payload
        .values
        .get("code")
        .ok_or_else(|| anyhow!("OAuth callback for '{server_name}' did not include code"))?;

    let mut params = build_token_request_base(oauth);
    params.push(("grant_type".into(), "authorization_code".into()));
    params.push(("code".into(), code.clone()));
    params.push(("redirect_uri".into(), effective_redirect_uri));
    if let Some(code_verifier) = code_verifier {
        params.push(("code_verifier".into(), code_verifier));
    }

    let response = send_oauth_form(client, oauth.token_url(), oauth, params)
        .await
        .with_context(|| format!("authorization code exchange failed for '{server_name}'"))?;
    if !response.status().is_success() {
        bail!(
            render_oauth_error(
                response,
                format!("Token exchange failed for '{server_name}'")
            )
            .await
        );
    }

    parse_token_response(response).await.with_context(|| {
        format!("authorization code token response was invalid for '{server_name}'")
    })
}

fn build_authorization_request_params(oauth: &OAuthConfig) -> Vec<(String, String)> {
    let mut params = oauth
        .authorization_params()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();

    if !oauth.scopes().is_empty() {
        params.push(("scope".into(), oauth.scopes().join(" ")));
    }
    if let Some(audience) = oauth.audience() {
        params.push(("audience".into(), audience.to_string()));
    }
    if let Some(resource) = oauth.resource() {
        params.push(("resource".into(), resource.to_string()));
    }
    params
}

fn build_token_request_base(oauth: &OAuthConfig) -> Vec<(String, String)> {
    let mut params = oauth
        .extra_params()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    if !oauth.scopes().is_empty() {
        params.push(("scope".into(), oauth.scopes().join(" ")));
    }
    if let Some(audience) = oauth.audience() {
        params.push(("audience".into(), audience.to_string()));
    }
    if let Some(resource) = oauth.resource() {
        params.push(("resource".into(), resource.to_string()));
    }
    params
}

async fn send_oauth_form(
    client: &reqwest::Client,
    url: &str,
    oauth: &OAuthConfig,
    mut params: Vec<(String, String)>,
) -> anyhow::Result<reqwest::Response> {
    let mut request = client.post(url);
    if oauth.uses_basic_auth() {
        request = request.basic_auth(
            oauth.client_id().unwrap_or_default().to_string(),
            oauth.client_secret().map(str::to_string),
        );
    } else {
        if let Some(client_id) = oauth.client_id() {
            params.push(("client_id".into(), client_id.to_string()));
        }
        if oauth.uses_post_auth()
            && let Some(client_secret) = oauth.client_secret()
        {
            params.push(("client_secret".into(), client_secret.to_string()));
        }
    }

    request
        .form(&params)
        .send()
        .await
        .context("OAuth form request failed")
}

async fn parse_token_response(response: reqwest::Response) -> anyhow::Result<OAuthTokenRecord> {
    let token: TokenResponse = response.json().await.context("invalid OAuth token JSON")?;
    if token.access_token.trim().is_empty() {
        bail!("OAuth token response returned an empty access_token");
    }
    Ok(OAuthTokenRecord {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at_epoch_secs: token
            .expires_in
            .as_ref()
            .and_then(parse_expires_in_secs)
            .map(|secs| current_epoch_secs() + secs),
    })
}

fn build_authorization_url(
    authorization_url: &str,
    oauth: &OAuthConfig,
    redirect_url: &str,
    state: &str,
    code_verifier: Option<&str>,
) -> anyhow::Result<Url> {
    let mut url = Url::parse(authorization_url).context("authorization_url is invalid")?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("response_type", "code");
        pairs.append_pair(
            "client_id",
            oauth
                .client_id()
                .ok_or_else(|| anyhow!("oauth.client_id is required for authorization_code"))?,
        );
        pairs.append_pair("redirect_uri", redirect_url);
        pairs.append_pair("state", state);
        if !oauth.scopes().is_empty() {
            pairs.append_pair("scope", &oauth.scopes().join(" "));
        }
        if let Some(audience) = oauth.audience() {
            pairs.append_pair("audience", audience);
        }
        if let Some(resource) = oauth.resource() {
            pairs.append_pair("resource", resource);
        }
        for (key, value) in oauth.authorization_params() {
            pairs.append_pair(key, value);
        }
        if let Some(code_verifier) = code_verifier {
            pairs.append_pair("code_challenge", &pkce_challenge(code_verifier));
            pairs.append_pair("code_challenge_method", "S256");
        }
    }
    Ok(url)
}

fn parse_expires_in_secs(value: &serde_json::Value) -> Option<u64> {
    if let Some(secs) = value.as_u64() {
        return Some(secs);
    }
    value.as_str()?.trim().parse().ok()
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn random_urlsafe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn truthy_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .as_deref()
        .is_some_and(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn browser_launch_suppressed() -> bool {
    cfg!(test)
        || truthy_env("EDGECRAB_DISABLE_BROWSER_OPEN")
        || truthy_env("CI")
        || std::env::var_os("RUST_TEST_THREADS").is_some()
        || std::env::var_os("NEXTEST").is_some()
}

fn open_url_in_browser(url: &str) -> anyhow::Result<()> {
    if browser_launch_suppressed() {
        tracing::debug!("browser launch suppressed for OAuth flow: {url}");
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    command
        .spawn()
        .context("failed to launch the system browser")?;
    Ok(())
}

async fn render_oauth_error(response: reqwest::Response, prefix: String) -> String {
    let status = response.status();
    match response.json::<OAuthErrorResponse>().await {
        Ok(error) => {
            let code = error.error.unwrap_or_else(|| status.to_string());
            let detail = error.error_description.unwrap_or_default();
            format!("{prefix}: {code} {detail}").trim().to_string()
        }
        Err(_) => format!("{prefix}: HTTP {status}"),
    }
}

struct LoopbackCallback {
    redirect_uri: String,
    port_mode: LoopbackPortMode,
    callback_rx: oneshot::Receiver<LoopbackCallbackPayload>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    _task: tokio::task::JoinHandle<()>,
}

impl LoopbackCallback {
    async fn bind(redirect_url: &str) -> anyhow::Result<Self> {
        let plan = analyze_loopback_redirect_plan(redirect_url)?;
        let requested_port = match plan.port_mode {
            LoopbackPortMode::Fixed(port) => port,
            LoopbackPortMode::Dynamic => 0,
        };
        let listener = tokio::net::TcpListener::bind((plan.bind_host.as_str(), requested_port))
            .await
            .with_context(|| match plan.port_mode {
                LoopbackPortMode::Fixed(port) => format!(
                    "failed to bind local callback port {port}; if the port is busy, set `oauth.redirect_url` to a loopback URL without a port or with `:0` so EdgeCrab can allocate one dynamically"
                ),
                LoopbackPortMode::Dynamic => {
                    "failed to bind a dynamic local callback port".to_string()
                }
            })?;
        let actual_port = listener
            .local_addr()
            .context("failed to determine local callback address")?
            .port();
        let redirect_info = build_loopback_redirect_info(&plan, actual_port)?;

        let callback_state = Arc::new(Mutex::new(None));
        let (callback_tx, callback_rx) = oneshot::channel();
        *callback_state.lock().expect("callback lock") = Some(callback_tx);

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let router = Router::new()
            .route(&plan.path, get(loopback_callback_handler))
            .with_state(callback_state);
        let server = axum::serve(listener, router).with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });
        let task = tokio::spawn(async move {
            let _ = server.await;
        });

        Ok(Self {
            redirect_uri: redirect_info.redirect_uri,
            port_mode: redirect_info.port_mode,
            callback_rx,
            shutdown_tx: Some(shutdown_tx),
            _task: task,
        })
    }

    fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    fn uses_dynamic_port(&self) -> bool {
        self.port_mode == LoopbackPortMode::Dynamic
    }

    async fn wait_for_callback(mut self) -> anyhow::Result<LoopbackCallbackPayload> {
        let payload = tokio::time::timeout(Duration::from_secs(180), self.callback_rx)
            .await
            .with_context(|| {
                format!(
                    "timed out waiting for the OAuth browser callback on {}",
                    self.redirect_uri
                )
            })?
            .map_err(|_| anyhow!("OAuth browser callback channel closed unexpectedly"))?;
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        Ok(payload)
    }
}

async fn loopback_callback_handler(
    State(state): State<Arc<Mutex<Option<oneshot::Sender<LoopbackCallbackPayload>>>>>,
    Query(values): Query<HashMap<String, String>>,
) -> Html<&'static str> {
    if let Some(tx) = state.lock().ok().and_then(|mut guard| guard.take()) {
        let _ = tx.send(LoopbackCallbackPayload { values });
    }
    Html("EdgeCrab MCP OAuth login is complete. You can return to the terminal.")
}

pub(crate) fn analyze_loopback_redirect_url(
    redirect_url: &str,
) -> anyhow::Result<LoopbackRedirectInfo> {
    let plan = analyze_loopback_redirect_plan(redirect_url)?;
    let placeholder_port = match plan.port_mode {
        LoopbackPortMode::Fixed(port) => port,
        LoopbackPortMode::Dynamic => 1,
    };
    build_loopback_redirect_info(&plan, placeholder_port)
}

fn analyze_loopback_redirect_plan(redirect_url: &str) -> anyhow::Result<LoopbackRedirectPlan> {
    let redirect_url = Url::parse(redirect_url).context("redirect_url is invalid")?;
    if redirect_url.scheme() != "http" {
        bail!(
            "redirect_url must use an http loopback URL, not {}",
            redirect_url.scheme()
        );
    }
    if redirect_url.fragment().is_some() {
        bail!("redirect_url must not contain a fragment");
    }

    let bind_host = match redirect_url.host() {
        Some(Host::Domain("localhost")) => "localhost".to_string(),
        Some(Host::Ipv4(ip)) if ip.is_loopback() => ip.to_string(),
        Some(Host::Ipv6(ip)) if ip.is_loopback() => ip.to_string(),
        Some(_) => bail!("redirect_url host must be localhost, 127.0.0.1, or ::1"),
        None => bail!("redirect_url is missing a host"),
    };

    let path = match redirect_url.path() {
        "" => "/".to_string(),
        value => value.to_string(),
    };
    let port_mode = match redirect_url.port() {
        Some(0) | None => LoopbackPortMode::Dynamic,
        Some(port) => LoopbackPortMode::Fixed(port),
    };

    Ok(LoopbackRedirectPlan {
        original_url: redirect_url,
        bind_host,
        path,
        port_mode,
    })
}

fn build_loopback_redirect_info(
    plan: &LoopbackRedirectPlan,
    actual_port: u16,
) -> anyhow::Result<LoopbackRedirectInfo> {
    let mut effective_url = plan.original_url.clone();
    effective_url
        .set_port(Some(actual_port))
        .map_err(|_| anyhow!("redirect_url port could not be updated"))?;
    Ok(LoopbackRedirectInfo {
        redirect_uri: effective_url.to_string(),
        port_mode: plan.port_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::extract::{Query, State};
    use axum::response::Redirect;
    use axum::routing::{get, post};
    use axum::{Json, Router, http::StatusCode};
    use tempfile::tempdir;

    #[derive(Clone)]
    struct MockOauthState {
        authorize_calls: Arc<AtomicUsize>,
        token_calls: Arc<AtomicUsize>,
    }

    async fn mock_authorize_endpoint(
        State(state): State<MockOauthState>,
        Query(query): Query<HashMap<String, String>>,
    ) -> Redirect {
        let _ = state.authorize_calls.fetch_add(1, Ordering::SeqCst);
        let redirect_uri = query.get("redirect_uri").expect("redirect_uri").to_string();
        let state_value = query.get("state").expect("state").to_string();
        let separator = if redirect_uri.contains('?') { '&' } else { '?' };
        Redirect::temporary(&format!(
            "{redirect_uri}{separator}code=auth-code-1&state={state_value}"
        ))
    }

    async fn mock_token_endpoint(
        State(state): State<MockOauthState>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        let _ = state.token_calls.fetch_add(1, Ordering::SeqCst);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "access_token": "browser-access-token-1",
                "refresh_token": "browser-refresh-token-1",
                "expires_in": 3600
            })),
        )
    }

    async fn spawn_browser_oauth_server() -> (String, oneshot::Sender<()>, MockOauthState) {
        let state = MockOauthState {
            authorize_calls: Arc::new(AtomicUsize::new(0)),
            token_calls: Arc::new(AtomicUsize::new(0)),
        };
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = Router::new()
            .route("/authorize", get(mock_authorize_endpoint))
            .route("/token", post(mock_token_endpoint))
            .with_state(state.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        (format!("http://{}", addr), shutdown_tx, state)
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests serialize env mutations through lock_test_env().
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn set_value(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests serialize env mutations through lock_test_env().
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: tests serialize env mutations through lock_test_env().
            unsafe {
                if let Some(previous) = self.previous.take() {
                    std::env::set_var(self.key, previous);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    #[test]
    fn pkce_challenge_is_url_safe() {
        let challenge = pkce_challenge("edgecrab-test-verifier");
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
        assert!(!challenge.contains('='));
    }

    #[test]
    fn analyze_loopback_redirect_accepts_dynamic_port_without_explicit_port() {
        let info = analyze_loopback_redirect_url("http://127.0.0.1/callback").expect("info");
        assert_eq!(info.port_mode, LoopbackPortMode::Dynamic);
        assert!(info.redirect_uri.contains("127.0.0.1:1/callback"));
    }

    #[test]
    fn analyze_loopback_redirect_accepts_ipv6_loopback() {
        let info = analyze_loopback_redirect_url("http://[::1]:8123/callback").expect("info");
        assert_eq!(info.port_mode, LoopbackPortMode::Fixed(8123));
        assert_eq!(info.redirect_uri, "http://[::1]:8123/callback");
    }

    #[test]
    fn analyze_loopback_redirect_rejects_non_loopback_host() {
        let err = analyze_loopback_redirect_url("http://example.com/callback")
            .expect_err("non-loopback host should fail");
        assert!(err.to_string().contains("localhost"));
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial(edgecrab_home_env)]
    async fn browser_launch_can_be_disabled_for_tests_and_ci() {
        let _disable_guard = EnvVarGuard::set_value("EDGECRAB_DISABLE_BROWSER_OPEN", "1");
        assert!(browser_launch_suppressed());
        open_url_in_browser("https://example.com/oauth").expect("suppressed browser launch");
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial(edgecrab_home_env)]
    async fn authorization_code_login_supports_dynamic_loopback_redirects() {
        let (base_url, shutdown_tx, state) = spawn_browser_oauth_server().await;
        let home = tempdir().expect("temp home");
        let edgecrab_home = home.path().join(".edgecrab");
        fs::create_dir_all(&edgecrab_home).expect("config dir");
        fs::write(
            edgecrab_home.join("config.yaml"),
            format!(
                "mcp_servers:\n  oauth-browser:\n    url: https://example.com/mcp\n    enabled: true\n    oauth:\n      token_url: {base_url}/token\n      authorization_url: {base_url}/authorize\n      redirect_url: http://127.0.0.1/callback\n      grant_type: authorization_code\n      auth_method: none\n      client_id: edgecrab-browser-client\n      use_pkce: true\n"
            ),
        )
        .expect("config");
        let _edgecrab_home_guard = EnvVarGuard::set("EDGECRAB_HOME", &edgecrab_home);
        let _home_guard = EnvVarGuard::set("HOME", home.path());
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("client");
        let (notice_tx, mut notice_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        let login_task = tokio::spawn(async move {
            login_mcp_server("oauth-browser", |line| {
                let _ = notice_tx.send(line);
            })
            .await
        });

        let mut auth_url = None;
        let mut dynamic_notice = None;
        for _ in 0..4 {
            let line = match tokio::time::timeout(Duration::from_secs(2), notice_rx.recv()).await {
                Ok(Some(line)) => line,
                Ok(None) => {
                    let result = login_task.await.expect("join");
                    panic!("login task ended before emitting notice: {result:?}");
                }
                Err(_) => {
                    panic!("timed out waiting for OAuth login notices");
                }
            };
            if line.contains("Using dynamic loopback redirect") {
                dynamic_notice = Some(line.clone());
            }
            if let Some((_, url)) = line.rsplit_once(" visit ") {
                auth_url = Some(url.to_string());
                break;
            }
        }

        let auth_url = if let Some(auth_url) = auth_url {
            auth_url
        } else if login_task.is_finished() {
            let result = login_task.await.expect("join");
            panic!("login task finished before authorization URL was emitted: {result:?}");
        } else {
            panic!("authorization url");
        };
        let dynamic_notice = dynamic_notice.expect("dynamic redirect notice");
        assert!(dynamic_notice.contains("http://127.0.0.1:"));

        let authorize_response = client.get(&auth_url).send().await.expect("authorize");
        assert!(authorize_response.status().is_redirection());
        let callback_url = authorize_response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .expect("callback location");

        let callback_client = reqwest::Client::new();
        let mut callback_response = None;
        for _ in 0..20 {
            match callback_client.get(&callback_url).send().await {
                Ok(value) => {
                    callback_response = Some(value);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(25)).await,
            }
        }
        let callback_response = callback_response.expect("callback response");
        assert!(callback_response.status().is_success());

        let token_summary = login_task.await.expect("join").expect("login result");
        let _ = shutdown_tx.send(());

        let token_record = fs::read_to_string(edgecrab_home.join("mcp-tokens/oauth-browser.json"))
            .expect("token record");
        assert!(token_summary.contains("OAuth login complete"));
        assert!(token_record.contains("browser-access-token-1"));
        assert!(token_record.contains("browser-refresh-token-1"));
        assert!(state.authorize_calls.load(Ordering::SeqCst) >= 1);
        assert!(state.token_calls.load(Ordering::SeqCst) >= 1);
    }
}
