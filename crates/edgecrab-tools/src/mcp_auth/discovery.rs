//! RFC 9728 Protected Resource Metadata + RFC 8414 AS metadata discovery.

use std::time::Duration;

use edgecrab_security::url_safety::{is_preview_loopback_url, is_safe_url};
use serde::Deserialize;
use url::Url;

use super::dcr::{DEFAULT_MCP_REDIRECT_URL, DcrRequest, register_oauth_client};

/// Options for MCP OAuth discovery.
#[derive(Debug, Clone)]
pub struct DiscoverOpts {
    /// Allow loopback/private hosts (e2e / local MCP).
    pub allow_loopback: bool,
    /// Pre-registered client id (skips DCR when set).
    pub client_id: Option<String>,
    /// Pre-registered client secret.
    pub client_secret: Option<String>,
    /// Override scopes (otherwise WWW-Authenticate → PRM scopes_supported).
    pub scopes: Vec<String>,
    /// Skip Dynamic Client Registration even when registration_endpoint exists.
    pub skip_dcr: bool,
    /// Request `offline_access` when AS advertises it (default true).
    pub prefer_offline_access: bool,
    /// HTTP timeout for discovery calls.
    pub timeout: Duration,
}

impl Default for DiscoverOpts {
    fn default() -> Self {
        Self {
            allow_loopback: false,
            client_id: None,
            client_secret: None,
            scopes: Vec::new(),
            skip_dcr: false,
            prefer_offline_access: true,
            timeout: Duration::from_secs(20),
        }
    }
}

/// Result of discovering OAuth settings for an MCP resource URL.
#[derive(Debug, Clone)]
pub struct DiscoveredMcpOauth {
    /// Canonical MCP resource URI (RFC 8707).
    pub resource: String,
    /// Human-readable name from PRM when present.
    pub resource_name: Option<String>,
    /// Authorization server issuer.
    pub issuer: String,
    pub authorization_url: String,
    pub token_url: String,
    pub device_authorization_url: Option<String>,
    pub registration_endpoint: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    /// `none` | `client_secret_post` | `client_secret_basic`
    pub auth_method: String,
    pub redirect_url: String,
    pub scopes: Vec<String>,
    pub use_pkce: bool,
    pub grant_type: String,
    /// Whether AS advertises `authorization_response_iss_parameter_supported`.
    pub iss_parameter_supported: bool,
}

/// Parsed Bearer challenge from `WWW-Authenticate`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WwwAuthenticateChallenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("{0}")]
    Message(String),
    #[error("SSRF blocked URL: {0}")]
    Ssrf(String),
    #[error("HTTP {status} from {url}: {body}")]
    Http {
        status: u16,
        url: String,
        body: String,
    },
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl DiscoverError {
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

#[derive(Debug, Deserialize)]
struct ProtectedResourceMetadata {
    resource: Option<String>,
    resource_name: Option<String>,
    #[serde(default)]
    authorization_servers: Vec<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AsMetadata {
    pub issuer: Option<String>,
    pub authorization_endpoint: Option<String>,
    pub token_endpoint: Option<String>,
    pub registration_endpoint: Option<String>,
    pub device_authorization_endpoint: Option<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    pub token_endpoint_auth_methods_supported: Vec<String>,
    #[serde(default)]
    pub grant_types_supported: Vec<String>,
    #[serde(default)]
    pub authorization_response_iss_parameter_supported: bool,
}

/// Discover OAuth configuration for an HTTP MCP server URL.
pub async fn discover_mcp_oauth(
    mcp_url: &str,
    opts: DiscoverOpts,
) -> Result<DiscoveredMcpOauth, DiscoverError> {
    let mcp_url = mcp_url.trim();
    validate_discovery_url(mcp_url, opts.allow_loopback)?;

    let client = reqwest::Client::builder()
        .timeout(opts.timeout)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;

    let (challenge, probe_status) = probe_mcp_resource(&client, mcp_url).await?;

    let (prm, prm_url) = fetch_prm_with_fallback(
        &client,
        mcp_url,
        challenge.resource_metadata.as_deref(),
        opts.allow_loopback,
    )
    .await
    .map_err(|err| {
        DiscoverError::message(format!(
            "Protected Resource Metadata discovery failed after probe HTTP {probe_status}: {err}"
        ))
    })?;
    let resource = prm
        .resource
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| canonical_resource(mcp_url));

    let as_issuer = prm
        .authorization_servers
        .into_iter()
        .find(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            DiscoverError::message(format!(
                "Protected Resource Metadata at {prm_url} has no authorization_servers \
                 (probe HTTP {probe_status})"
            ))
        })?;
    validate_discovery_url(&as_issuer, opts.allow_loopback)?;

    let as_meta = fetch_as_metadata(&client, &as_issuer, opts.allow_loopback).await?;
    let token_url = as_meta
        .token_endpoint
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| DiscoverError::message("AS metadata missing token_endpoint"))?;
    let authorization_url = as_meta
        .authorization_endpoint
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| DiscoverError::message("AS metadata missing authorization_endpoint"))?;

    validate_discovery_url(&token_url, opts.allow_loopback)?;
    validate_discovery_url(&authorization_url, opts.allow_loopback)?;
    if let Some(device) = as_meta.device_authorization_endpoint.as_deref() {
        validate_discovery_url(device, opts.allow_loopback)?;
    }
    if let Some(reg) = as_meta.registration_endpoint.as_deref() {
        validate_discovery_url(reg, opts.allow_loopback)?;
    }

    let issuer = as_meta
        .issuer
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| as_issuer.trim_end_matches('/').to_string());

    let scopes = select_scopes(&opts, &challenge, &prm.scopes_supported, &as_meta);
    let use_pkce = as_meta
        .code_challenge_methods_supported
        .iter()
        .any(|m| m.eq_ignore_ascii_case("S256"))
        || as_meta.code_challenge_methods_supported.is_empty();

    let auth_method = select_auth_method(&as_meta, opts.client_secret.is_some());
    // Prefer localhost — many MCP AS reject 127.0.0.1 at registration time.
    let mut redirect_url = DEFAULT_MCP_REDIRECT_URL.to_string();

    let mut client_id = opts.client_id.clone().filter(|s| !s.trim().is_empty());
    let mut client_secret = opts.client_secret.clone().filter(|s| !s.trim().is_empty());

    if client_id.is_none()
        && !opts.skip_dcr
        && as_meta
            .registration_endpoint
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
    {
        let dcr = register_oauth_client(&client, &as_meta, DcrRequest::mcp_native_public(&scopes))
            .await?;
        client_id = Some(dcr.client_id);
        if client_secret.is_none() {
            client_secret = dcr.client_secret;
        }
        // Prefer an AS-echoed localhost:0 (or localhost) redirect when present.
        if let Some(uri) = dcr
            .redirect_uris
            .iter()
            .find(|u| u.contains("localhost:0"))
            .cloned()
            .or_else(|| {
                dcr.redirect_uris
                    .iter()
                    .find(|u| u.contains("localhost"))
                    .cloned()
            })
            .or_else(|| dcr.redirect_uris.first().cloned())
        {
            redirect_url = uri;
        }
    }

    let grant_type = if as_meta
        .grant_types_supported
        .iter()
        .any(|g| g == "authorization_code")
        || as_meta.grant_types_supported.is_empty()
    {
        "authorization_code".to_string()
    } else if as_meta
        .device_authorization_endpoint
        .as_ref()
        .is_some_and(|s| !s.is_empty())
    {
        "device_code".to_string()
    } else {
        "auto".to_string()
    };

    Ok(DiscoveredMcpOauth {
        resource,
        resource_name: prm.resource_name,
        issuer,
        authorization_url,
        token_url,
        device_authorization_url: as_meta.device_authorization_endpoint,
        registration_endpoint: as_meta.registration_endpoint,
        client_id,
        client_secret,
        auth_method,
        redirect_url,
        scopes,
        use_pkce,
        grant_type,
        iss_parameter_supported: as_meta.authorization_response_iss_parameter_supported,
    })
}

/// Suggest a config key from PRM resource_name or URL host.
pub fn suggest_server_name(resource_name: Option<&str>, mcp_url: &str) -> String {
    if let Some(name) = resource_name {
        let slug = slugify(name);
        if !slug.is_empty() {
            return slug;
        }
    }
    Url::parse(mcp_url)
        .ok()
        .and_then(|u| u.host_str().map(slugify))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "mcp-server".into())
}

fn slugify(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.chars().take(48).collect()
}

/// Parse `WWW-Authenticate` Bearer challenge parameters.
pub fn parse_www_authenticate(header: &str) -> WwwAuthenticateChallenge {
    let mut challenge = WwwAuthenticateChallenge::default();
    let trimmed = header.trim();
    let rest = trimmed
        .strip_prefix("Bearer")
        .or_else(|| trimmed.strip_prefix("bearer"))
        .unwrap_or(trimmed)
        .trim()
        .trim_start_matches(',');

    for part in split_auth_params(rest) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = unquote(value.trim());
        match key.as_str() {
            "resource_metadata" => challenge.resource_metadata = Some(value),
            "scope" => challenge.scope = Some(value),
            "error" => challenge.error = Some(value),
            _ => {}
        }
    }
    challenge
}

fn split_auth_params(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_quotes = false;
    let bytes = input.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => in_quotes = !in_quotes,
            b',' if !in_quotes => {
                let slice = input[start..i].trim();
                if !slice.is_empty() {
                    parts.push(slice);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let tail = input[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        v[1..v.len() - 1].replace("\\\"", "\"")
    } else {
        v.to_string()
    }
}

pub(crate) fn validate_discovery_url(url: &str, allow_loopback: bool) -> Result<(), DiscoverError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(DiscoverError::InvalidUrl("empty URL".into()));
    }
    if is_preview_loopback_url(trimmed) {
        if allow_loopback {
            return Ok(());
        }
        return Err(DiscoverError::Ssrf(format!(
            "{trimmed} (pass allow_loopback for local MCP)"
        )));
    }
    match is_safe_url(trimmed) {
        Ok(true) => Ok(()),
        Ok(false) => Err(DiscoverError::Ssrf(trimmed.to_string())),
        Err(e) => Err(DiscoverError::InvalidUrl(e.to_string())),
    }
}

async fn probe_mcp_resource(
    client: &reqwest::Client,
    mcp_url: &str,
) -> Result<(WwwAuthenticateChallenge, u16), DiscoverError> {
    // Prefer POST (MCP JSON-RPC) but many servers also answer GET with 401.
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "edgecrab", "version": env!("CARGO_PKG_VERSION") }
        }
    });
    let response = client
        .post(mcp_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&body)
        .send()
        .await?;
    let status = response.status().as_u16();
    let header = response
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let challenge = parse_www_authenticate(&header);
    Ok((challenge, status))
}

/// Build PRM well-known URLs for an MCP resource (RFC 9728 path insertion).
pub fn prm_candidate_urls(mcp_url: &str, challenge_metadata: Option<&str>) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(meta) = challenge_metadata.filter(|s| !s.trim().is_empty()) {
        urls.push(meta.trim().to_string());
    }
    let Ok(parsed) = Url::parse(mcp_url) else {
        return urls;
    };
    let origin = format!(
        "{}://{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or_default()
    );
    let origin = if let Some(port) = parsed.port() {
        format!("{origin}:{port}")
    } else {
        origin
    };

    let path = parsed.path().trim_end_matches('/');
    if path.is_empty() || path == "/" {
        urls.push(format!("{origin}/.well-known/oauth-protected-resource"));
    } else {
        // RFC 9728: insert /.well-known/oauth-protected-resource before path
        urls.push(format!(
            "{origin}/.well-known/oauth-protected-resource{path}"
        ));
        // Also try origin-root PRM (common deployment)
        urls.push(format!("{origin}/.well-known/oauth-protected-resource"));
        // Path-suffix form used by some MCP hosts
        let trimmed = path.trim_start_matches('/');
        if !trimmed.is_empty() {
            urls.push(format!(
                "{origin}/.well-known/oauth-protected-resource/{trimmed}"
            ));
        }
    }
    urls
}

async fn fetch_as_metadata(
    client: &reqwest::Client,
    as_issuer: &str,
    allow_loopback: bool,
) -> Result<AsMetadata, DiscoverError> {
    let issuer = as_issuer.trim_end_matches('/');
    let candidates = as_metadata_urls(issuer);
    let mut last_err = DiscoverError::message("no AS metadata endpoints succeeded");
    for url in candidates {
        validate_discovery_url(&url, allow_loopback)?;
        match fetch_json::<AsMetadata>(client, &url).await {
            Ok(meta) if meta.token_endpoint.is_some() => return Ok(meta),
            Ok(_) => {
                last_err =
                    DiscoverError::message(format!("AS metadata at {url} missing token_endpoint"));
            }
            Err(err) => last_err = err,
        }
    }
    Err(last_err)
}

pub fn as_metadata_urls(issuer: &str) -> Vec<String> {
    let issuer = issuer.trim_end_matches('/');
    let Ok(parsed) = Url::parse(issuer) else {
        return vec![
            format!("{issuer}/.well-known/oauth-authorization-server"),
            format!("{issuer}/.well-known/openid-configuration"),
        ];
    };
    let path = parsed.path().trim_end_matches('/');
    let mut urls = Vec::new();
    if path.is_empty() || path == "/" {
        urls.push(format!("{issuer}/.well-known/oauth-authorization-server"));
        urls.push(format!("{issuer}/.well-known/openid-configuration"));
    } else {
        // RFC 8414 path-aware well-known
        let origin = origin_of(&parsed);
        urls.push(format!(
            "{origin}/.well-known/oauth-authorization-server{path}"
        ));
        urls.push(format!("{issuer}/.well-known/oauth-authorization-server"));
        urls.push(format!("{origin}/.well-known/openid-configuration{path}"));
        urls.push(format!("{issuer}/.well-known/openid-configuration"));
    }
    urls
}

fn origin_of(parsed: &Url) -> String {
    let host = parsed.host_str().unwrap_or_default();
    match parsed.port() {
        Some(port) => format!("{}://{}:{}", parsed.scheme(), host, port),
        None => format!("{}://{}", parsed.scheme(), host),
    }
}

fn canonical_resource(mcp_url: &str) -> String {
    Url::parse(mcp_url)
        .map(|mut u| {
            u.set_fragment(None);
            // Drop default ports noise; keep path
            let mut s = u.to_string();
            if s.ends_with('/') && u.path() != "/" {
                s.pop();
            }
            s
        })
        .unwrap_or_else(|_| mcp_url.trim_end_matches('/').to_string())
}

fn select_scopes(
    opts: &DiscoverOpts,
    challenge: &WwwAuthenticateChallenge,
    prm_scopes: &[String],
    as_meta: &AsMetadata,
) -> Vec<String> {
    let mut scopes = if !opts.scopes.is_empty() {
        opts.scopes.clone()
    } else if let Some(scope) = challenge.scope.as_ref().filter(|s| !s.trim().is_empty()) {
        scope.split_whitespace().map(|s| s.to_string()).collect()
    } else if !prm_scopes.is_empty() {
        prm_scopes.to_vec()
    } else {
        Vec::new()
    };

    if opts.prefer_offline_access
        && as_meta
            .scopes_supported
            .iter()
            .any(|s| s == "offline_access")
        && !scopes.iter().any(|s| s == "offline_access")
    {
        scopes.push("offline_access".into());
    }
    scopes
}

fn select_auth_method(as_meta: &AsMetadata, has_secret: bool) -> String {
    let methods = &as_meta.token_endpoint_auth_methods_supported;
    if !has_secret && methods.iter().any(|m| m == "none") {
        return "none".into();
    }
    if methods.iter().any(|m| m == "client_secret_basic") && has_secret {
        return "client_secret_basic".into();
    }
    if methods.iter().any(|m| m == "client_secret_post") && has_secret {
        return "client_secret_post".into();
    }
    if methods.iter().any(|m| m == "none") {
        return "none".into();
    }
    if has_secret {
        "client_secret_post".into()
    } else {
        "none".into()
    }
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T, DiscoverError> {
    let response = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(DiscoverError::Http {
            status: status.as_u16(),
            url: url.to_string(),
            body: body.chars().take(200).collect(),
        });
    }
    Ok(serde_json::from_str(&body)?)
}

/// Try each PRM candidate until one succeeds.
async fn fetch_prm_with_fallback(
    client: &reqwest::Client,
    mcp_url: &str,
    challenge_metadata: Option<&str>,
    allow_loopback: bool,
) -> Result<(ProtectedResourceMetadata, String), DiscoverError> {
    let mut last_err = DiscoverError::message("no PRM endpoint succeeded");
    for url in prm_candidate_urls(mcp_url, challenge_metadata) {
        if let Err(err) = validate_discovery_url(&url, allow_loopback) {
            last_err = err;
            continue;
        }
        match fetch_json::<ProtectedResourceMetadata>(client, &url).await {
            Ok(prm) => return Ok((prm, url)),
            Err(err) => last_err = err,
        }
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_www_authenticate_resource_metadata() {
        let header = r#"Bearer resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource", scope="mcp:read""#;
        let c = parse_www_authenticate(header);
        assert_eq!(
            c.resource_metadata.as_deref(),
            Some("https://mcp.example.com/.well-known/oauth-protected-resource")
        );
        assert_eq!(c.scope.as_deref(), Some("mcp:read"));
    }

    #[test]
    fn prm_candidates_for_path_resource() {
        let urls = prm_candidate_urls("https://mcp.example.com/mcp", None);
        assert!(
            urls.iter()
                .any(|u| u == "https://mcp.example.com/.well-known/oauth-protected-resource/mcp"),
            "{urls:?}"
        );
        assert!(
            urls.iter()
                .any(|u| u == "https://mcp.example.com/.well-known/oauth-protected-resource"),
            "{urls:?}"
        );
    }

    #[test]
    fn as_metadata_urls_origin() {
        let urls = as_metadata_urls("https://auth.example.com");
        assert_eq!(
            urls[0],
            "https://auth.example.com/.well-known/oauth-authorization-server"
        );
        assert!(urls.iter().any(|u| u.contains("openid-configuration")));
    }

    #[test]
    fn suggest_name_from_resource() {
        assert_eq!(
            suggest_server_name(Some("Example MCP Wiki"), "https://mcp.example.com/mcp"),
            "example-mcp-wiki"
        );
    }

    #[test]
    fn suggest_name_from_host() {
        assert_eq!(
            suggest_server_name(None, "https://mcp.example.com/mcp"),
            "mcp-example-com"
        );
    }
}
