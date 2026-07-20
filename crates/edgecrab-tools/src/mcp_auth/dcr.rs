//! OAuth 2.0 Dynamic Client Registration (RFC 7591) for MCP public clients.
//!
//! MCP Authorization (2025-11-25 / 2026-07-28) uses native public clients with
//! PKCE. Loopback redirect URIs must include an explicit port for many MCP
//! authorization servers — port `0` means "any ephemeral port" (RFC 8252 §7.3).
//! Portless URIs like `http://localhost/callback` are commonly rejected.

use serde::{Deserialize, Serialize};

use super::discovery::{AsMetadata, DiscoverError};

/// Preferred loopback redirect for MCP native clients.
///
/// Port `:0` is the dynamic-port sentinel accepted by MCP OAuth servers that
/// reject portless `http://localhost/callback` / `http://127.0.0.1/callback`.
/// EdgeCrab binds an ephemeral port at login and substitutes it in the
/// authorize + token exchange (RFC 8252 §7.3).
pub const DEFAULT_MCP_REDIRECT_URL: &str = "http://localhost:0/callback";

/// Primary redirect URIs for DCR (localhost:0 first).
pub const DEFAULT_MCP_REDIRECT_URIS: &[&str] =
    &["http://localhost:0/callback", "http://127.0.0.1:0/callback"];

/// Request body for RFC 7591 registration.
#[derive(Debug, Clone, Serialize)]
pub struct DcrRequest {
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
    pub token_endpoint_auth_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl DcrRequest {
    /// Public native MCP client (PKCE + auth-code + refresh).
    pub fn mcp_native_public(scopes: &[String]) -> Self {
        Self {
            client_name: "EdgeCrab".into(),
            redirect_uris: DEFAULT_MCP_REDIRECT_URIS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            grant_types: vec!["authorization_code".into(), "refresh_token".into()],
            response_types: vec!["code".into()],
            token_endpoint_auth_method: "none".into(),
            application_type: Some("native".into()),
            scope: if scopes.is_empty() {
                None
            } else {
                Some(scopes.join(" "))
            },
        }
    }
}

/// Successful DCR response (subset).
#[derive(Debug, Clone, Deserialize)]
pub struct DcrClient {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_id_issued_at: Option<u64>,
    /// Echoed redirect URIs when the AS returns them.
    #[serde(default)]
    pub redirect_uris: Vec<String>,
}

/// Register a public native client at the AS `registration_endpoint`.
///
/// Retries alternate loopback redirect sets when the AS rejects with
/// `invalid_client_metadata` about `redirect_uri`.
pub async fn register_oauth_client(
    client: &reqwest::Client,
    as_meta: &AsMetadata,
    req: DcrRequest,
) -> Result<DcrClient, DiscoverError> {
    let endpoint = as_meta
        .registration_endpoint
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DiscoverError::message("AS metadata has no registration_endpoint"))?;

    let attempts = dcr_redirect_attempts(&req.redirect_uris);
    let mut last_err = DiscoverError::message("DCR failed");

    for redirect_uris in attempts {
        let mut attempt = req.clone();
        attempt.redirect_uris = redirect_uris;
        match post_dcr(client, endpoint, &attempt).await {
            Ok(parsed) => return Ok(parsed),
            Err(err) if is_redirect_uri_rejected(&err) => {
                tracing::info!(
                    error = %err,
                    redirects = ?attempt.redirect_uris,
                    "DCR rejected redirect_uris; retrying alternate loopback set"
                );
                last_err = err;
            }
            Err(err) => return Err(err),
        }
    }
    Err(last_err)
}

fn dcr_redirect_attempts(initial: &[String]) -> Vec<Vec<String>> {
    let localhost0 = vec!["http://localhost:0/callback".to_string()];
    let both0 = vec![
        "http://localhost:0/callback".to_string(),
        "http://127.0.0.1:0/callback".to_string(),
    ];
    let ip0 = vec!["http://127.0.0.1:0/callback".to_string()];
    // Last-resort shapes some servers accept.
    let localhost_root = vec!["http://localhost/".to_string()];

    let mut attempts = Vec::new();
    if !initial.is_empty() {
        attempts.push(initial.to_vec());
    }
    for candidate in [localhost0, both0, ip0, localhost_root] {
        if !attempts.iter().any(|a| a == &candidate) {
            attempts.push(candidate);
        }
    }
    attempts
}

fn is_redirect_uri_rejected(err: &DiscoverError) -> bool {
    match err {
        DiscoverError::Http { status, body, .. } if (400..500).contains(status) => {
            let lower = body.to_ascii_lowercase();
            lower.contains("redirect_uri")
                || lower.contains("invalid_client_metadata")
                || lower.contains("redirect uri")
        }
        _ => false,
    }
}

async fn post_dcr(
    client: &reqwest::Client,
    endpoint: &str,
    req: &DcrRequest,
) -> Result<DcrClient, DiscoverError> {
    let response = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(req)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(DiscoverError::Http {
            status: status.as_u16(),
            url: endpoint.to_string(),
            body: body.chars().take(300).collect(),
        });
    }
    let parsed: DcrClient = serde_json::from_str(&body)?;
    if parsed.client_id.trim().is_empty() {
        return Err(DiscoverError::message("DCR response missing client_id"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dcr_request_uses_port_zero_localhost() {
        let req = DcrRequest::mcp_native_public(&["mcp:read".into(), "offline_access".into()]);
        let json = serde_json::to_value(&req).expect("json");
        assert_eq!(json["token_endpoint_auth_method"], "none");
        assert_eq!(json["application_type"], "native");
        assert_eq!(json["client_name"], "EdgeCrab");
        assert_eq!(json["redirect_uris"][0], "http://localhost:0/callback");
        assert!(
            json["grant_types"]
                .as_array()
                .is_some_and(|g| g.iter().any(|v| v == "refresh_token"))
        );
        assert!(
            json["scope"]
                .as_str()
                .is_some_and(|s| s.contains("offline_access"))
        );
    }

    #[test]
    fn dcr_response_parses() {
        let raw = r#"{"client_id":"edgecrab-abc","client_id_issued_at":1}"#;
        let client: DcrClient = serde_json::from_str(raw).expect("parse");
        assert_eq!(client.client_id, "edgecrab-abc");
        assert!(client.client_secret.is_none());
    }

    #[test]
    fn redirect_rejection_detected() {
        let err = DiscoverError::Http {
            status: 400,
            url: "https://example.com/register".into(),
            body: r#"{"error":"invalid_client_metadata","error_description":"redirect_uri is not allowed for MCP OAuth: http://127.0.0.1/callback"}"#.into(),
        };
        assert!(is_redirect_uri_rejected(&err));
    }

    #[test]
    fn redirect_attempts_lead_with_localhost_zero() {
        let attempts = dcr_redirect_attempts(&[]);
        assert_eq!(attempts[0][0], "http://localhost:0/callback");
    }
}
