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
use url::Url;

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
    let redirect = Url::parse(redirect_url)
        .with_context(|| format!("redirect_url is invalid for '{server_name}'"))?;
    let callback = LoopbackCallback::bind(&redirect).await?;

    let state = random_urlsafe(24);
    let code_verifier = oauth.uses_pkce().then(|| random_urlsafe(48));
    let auth_url = build_authorization_url(
        authorization_url,
        oauth,
        redirect_url,
        &state,
        code_verifier.as_deref(),
    )?;

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
    params.push(("redirect_uri".into(), redirect_url.to_string()));
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
        if oauth.uses_post_auth() {
            if let Some(client_secret) = oauth.client_secret() {
                params.push(("client_secret".into(), client_secret.to_string()));
            }
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

fn open_url_in_browser(url: &str) -> anyhow::Result<()> {
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
    callback_rx: oneshot::Receiver<LoopbackCallbackPayload>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    _task: tokio::task::JoinHandle<()>,
}

impl LoopbackCallback {
    async fn bind(redirect_url: &Url) -> anyhow::Result<Self> {
        if redirect_url.scheme() != "http" {
            bail!(
                "redirect_url must use http loopback, not {}",
                redirect_url.scheme()
            );
        }
        let host = redirect_url
            .host_str()
            .ok_or_else(|| anyhow!("redirect_url is missing a host"))?;
        if host != "127.0.0.1" && host != "localhost" {
            bail!("redirect_url host must be 127.0.0.1 or localhost");
        }
        let port = redirect_url
            .port_or_known_default()
            .ok_or_else(|| anyhow!("redirect_url is missing a port"))?;
        let path = match redirect_url.path() {
            "" => "/",
            value => value,
        };

        let listener = if host == "localhost" {
            tokio::net::TcpListener::bind(("localhost", port))
                .await
                .with_context(|| format!("failed to bind local callback port {port}"))?
        } else {
            tokio::net::TcpListener::bind(("127.0.0.1", port))
                .await
                .with_context(|| format!("failed to bind local callback port {port}"))?
        };

        let callback_state = Arc::new(Mutex::new(None));
        let (callback_tx, callback_rx) = oneshot::channel();
        *callback_state.lock().expect("callback lock") = Some(callback_tx);

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let router = Router::new()
            .route(path, get(loopback_callback_handler))
            .with_state(callback_state);
        let server = axum::serve(listener, router).with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });
        let task = tokio::spawn(async move {
            let _ = server.await;
        });

        Ok(Self {
            callback_rx,
            shutdown_tx: Some(shutdown_tx),
            _task: task,
        })
    }

    async fn wait_for_callback(mut self) -> anyhow::Result<LoopbackCallbackPayload> {
        let payload = tokio::time::timeout(Duration::from_secs(180), self.callback_rx)
            .await
            .context("timed out waiting for the OAuth browser callback")?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_url_safe() {
        let challenge = pkce_challenge("edgecrab-test-verifier");
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
        assert!(!challenge.contains('='));
    }
}
