//! Session capability grants for loopback browser preview (spec 021).
//!
//! When `browser_navigate` hits SSRF / `security.preview`, ask Once/Session/Always/Deny
//! on the existing approval channel, apply session preview grants, then the caller
//! re-validates the URL.

use edgecrab_types::ToolError;

use crate::recovery_catalog::preview_loopback_grant_from_error;
use crate::registry::{ApprovalKind, ApprovalRequest, ApprovalResponse, ToolContext};

/// If `err` carries a grantable preview_loopback recovery, ask the user and apply.
///
/// Returns `Ok(())` when the caller should retry URL validation + navigate.
/// Returns `Err` when denied, not grantable, or no interactive approver is available.
pub async fn maybe_request_preview_loopback_grant(
    ctx: &ToolContext,
    err: ToolError,
) -> Result<(), ToolError> {
    let Some((host, port, url)) = preview_loopback_grant_from_error(&err) else {
        return Err(err);
    };
    request_preview_loopback_grant(ctx, &host, port, &url).await
}

/// Ask the user to allow browser access to a loopback preview URL.
pub async fn request_preview_loopback_grant(
    ctx: &ToolContext,
    host: &str,
    port: u16,
    url: &str,
) -> Result<(), ToolError> {
    let display_url = if url.trim().is_empty() {
        format!("http://{host}:{port}/")
    } else {
        url.to_string()
    };

    let Some(tx) = &ctx.approval_tx else {
        return Err(ToolError::PermissionDenied(format!(
            "Loopback browser navigate to {display_url} requires a session preview grant, \
             but no interactive approver is available. Enable via `/config preview on` \
             or `edgecrab config set security.preview.enabled true`."
        )));
    };

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<ApprovalResponse>();
    tx.send(ApprovalRequest {
        command: format!("preview {display_url}"),
        full_command: display_url.clone(),
        reasons: vec![
            format!("Allow browser_navigate to loopback {host}:{port} for this session?"),
            "Once = this navigate only · Session = until EdgeCrab exits · Always = persist security.preview".into(),
            "Deny = keep SSRF block (recommended if you did not start this server)".into(),
        ],
        kind: ApprovalKind::PreviewLoopback,
        response_tx: resp_tx,
    })
    .map_err(|_| {
        ToolError::PermissionDenied(
            "Preview loopback grant requires approval, but no interactive approver is available."
                .into(),
        )
    })?;

    let response = tokio::select! {
        _ = ctx.cancel.cancelled() => {
            return Err(ToolError::Other("Interrupted by user".into()));
        }
        result = resp_rx => result.map_err(|_| ToolError::PermissionDenied(
            "Preview grant request was cancelled before a decision was received.".into(),
        ))?
    };

    match response {
        ApprovalResponse::Once => {
            edgecrab_security::url_safety::grant_once_preview_loopback(host, port);
            Ok(())
        }
        ApprovalResponse::Session => {
            edgecrab_security::url_safety::grant_session_preview_loopback(host, port);
            Ok(())
        }
        ApprovalResponse::Always => {
            edgecrab_security::url_safety::grant_session_preview_loopback(host, port);
            persist_preview_enabled(ctx, port)?;
            Ok(())
        }
        ApprovalResponse::Deny => Err(ToolError::PermissionDenied(format!(
            "User denied session preview grant for {display_url}. \
             Do not retry the same browser_navigate. Use an alternative approach or ask the user \
             to enable `/config preview on` later."
        ))),
    }
}

fn persist_preview_enabled(ctx: &ToolContext, port: u16) -> Result<(), ToolError> {
    let home = &ctx.config.edgecrab_home;
    let path = home.join("config.yaml");
    let mut doc = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| ToolError::Other(format!("failed to read config.yaml: {e}")))?;
        serde_yml::from_str::<serde_yml::Value>(&raw)
            .unwrap_or(serde_yml::Value::Mapping(Default::default()))
    } else {
        serde_yml::Value::Mapping(Default::default())
    };

    let root = doc
        .as_mapping_mut()
        .ok_or_else(|| ToolError::Other("config.yaml root must be a mapping".into()))?;
    let security = root
        .entry(serde_yml::Value::String("security".into()))
        .or_insert_with(|| serde_yml::Value::Mapping(Default::default()));
    let security_map = security
        .as_mapping_mut()
        .ok_or_else(|| ToolError::Other("security must be a mapping".into()))?;
    let preview = security_map
        .entry(serde_yml::Value::String("preview".into()))
        .or_insert_with(|| serde_yml::Value::Mapping(Default::default()));
    let preview_map = preview
        .as_mapping_mut()
        .ok_or_else(|| ToolError::Other("security.preview must be a mapping".into()))?;
    preview_map.insert(
        serde_yml::Value::String("enabled".into()),
        serde_yml::Value::Bool(true),
    );

    let ports_key = serde_yml::Value::String("allow_localhost_ports".into());
    let mut ports: Vec<u16> = preview_map
        .get(&ports_key)
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_u64().map(|n| n as u16))
                .collect()
        })
        .unwrap_or_else(|| vec![8000, 3000, 5173, 8080, 8888, 5500, 7777, 4173, 5000]);
    if !ports.contains(&port) {
        ports.push(port);
        ports.sort_unstable();
    }
    preview_map.insert(
        ports_key,
        serde_yml::Value::Sequence(
            ports
                .into_iter()
                .map(|p| serde_yml::Value::Number(p.into()))
                .collect(),
        ),
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ToolError::Other(format!("failed to create config dir: {e}")))?;
    }
    let raw = serde_yml::to_string(&doc)
        .map_err(|e| ToolError::Other(format!("failed to serialize config.yaml: {e}")))?;
    std::fs::write(&path, raw)
        .map_err(|e| ToolError::Other(format!("failed to write config.yaml: {e}")))?;

    let mut policy = edgecrab_security::url_safety::current_preview_policy();
    policy.enabled = true;
    if !policy.allowed_ports.contains(&port) {
        policy.allowed_ports.push(port);
        policy.allowed_ports.sort_unstable();
    }
    edgecrab_security::url_safety::set_preview_policy(policy);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery_catalog::browser_navigate_blocked;

    #[test]
    fn preview_grant_payload_parsed_from_recovery() {
        let err = browser_navigate_blocked("http://127.0.0.1:8000/", "SSRF policy", &[]);
        let (host, port, url) = preview_loopback_grant_from_error(&err).expect("grant");
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 8000);
        assert!(url.contains("8000"));
    }
}
