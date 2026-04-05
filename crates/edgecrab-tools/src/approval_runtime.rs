//! Shared dangerous-command approval runtime for shell-based tools.
//!
//! WHY shared module: `terminal` and `run_process` both need the same command
//! scan, approval request, session cache, and persistence behavior. Keeping
//! the flow here avoids divergent security semantics across shell tools.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use edgecrab_security::approval::{ApprovalMode, ApprovalPolicy};
use edgecrab_types::ToolError;
use serde_json::json;

use crate::registry::{ApprovalRequest, ApprovalResponse, ToolContext};

fn approval_policy() -> &'static ApprovalPolicy {
    static POLICY: OnceLock<ApprovalPolicy> = OnceLock::new();
    POLICY.get_or_init(|| ApprovalPolicy::new(ApprovalMode::Manual, Vec::new()))
}

fn allowlist_file_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn allowlist_path(edgecrab_home: &Path) -> std::path::PathBuf {
    edgecrab_home.join("command_allowlist.json")
}

fn load_persistent_allowlist(edgecrab_home: &Path) -> Vec<String> {
    let path = allowlist_path(edgecrab_home);
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

fn persist_allowlist_command(edgecrab_home: &Path, command: &str) -> Result<(), ToolError> {
    let _guard = allowlist_file_lock()
        .lock()
        .map_err(|_| ToolError::Other("approval allowlist lock poisoned".into()))?;

    let path = allowlist_path(edgecrab_home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ToolError::Other(format!("failed to create allowlist directory: {e}")))?;
    }

    let mut entries: HashSet<String> = load_persistent_allowlist(edgecrab_home)
        .into_iter()
        .collect();
    entries.insert(command.to_string());

    let mut sorted: Vec<String> = entries.into_iter().collect();
    sorted.sort();

    let payload = serde_json::to_string_pretty(&sorted)
        .map_err(|e| ToolError::Other(format!("failed to serialize allowlist: {e}")))?;
    std::fs::write(path, payload)
        .map_err(|e| ToolError::Other(format!("failed to persist allowlist: {e}")))?;
    Ok(())
}

fn command_preview(command: &str) -> String {
    let single_line = command.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::safe_truncate(&single_line, 80).to_string()
}

pub(crate) fn command_approval_reasons(ctx: &ToolContext, command: &str) -> Option<Vec<String>> {
    let policy = approval_policy();
    policy.load_permanent_allowlist(&load_persistent_allowlist(&ctx.config.edgecrab_home));

    let session_id = ctx
        .session_key
        .clone()
        .unwrap_or_else(|| ctx.session_id.clone());
    let check = policy.check("terminal", &json!({ "command": command }), &session_id);
    if check.needs_approval {
        Some(check.reasons)
    } else {
        None
    }
}

pub(crate) async fn request_command_approval(
    ctx: &ToolContext,
    command: &str,
    reasons: Vec<String>,
) -> Result<(), ToolError> {
    let policy = approval_policy();
    policy.load_permanent_allowlist(&load_persistent_allowlist(&ctx.config.edgecrab_home));

    let session_id = ctx
        .session_key
        .clone()
        .unwrap_or_else(|| ctx.session_id.clone());
    let check = policy.check("terminal", &json!({ "command": command }), &session_id);
    if !check.needs_approval {
        return Ok(());
    }

    let Some(tx) = &ctx.approval_tx else {
        return Err(ToolError::PermissionDenied(format!(
            "Dangerous command requires approval: {}",
            reasons.join(", ")
        )));
    };

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<ApprovalResponse>();
    tx.send(ApprovalRequest {
        command: command_preview(command),
        full_command: command.to_string(),
        reasons,
        response_tx: resp_tx,
    })
    .map_err(|_| {
        ToolError::PermissionDenied(
            "Dangerous command requires approval, but no interactive approver is available.".into(),
        )
    })?;

    let response = tokio::select! {
        _ = ctx.cancel.cancelled() => {
            return Err(ToolError::Other("Interrupted by user".into()));
        }
        result = resp_rx => result.map_err(|_| ToolError::PermissionDenied(
            "Approval request was cancelled before a decision was received.".into(),
        ))?
    };

    match response {
        ApprovalResponse::Once => Ok(()),
        ApprovalResponse::Session => {
            policy.approve_for_session(command, &session_id);
            Ok(())
        }
        ApprovalResponse::Always => {
            policy.approve_for_session(command, &session_id);
            policy.approve_permanently(command);
            persist_allowlist_command(&ctx.config.edgecrab_home, command)?;
            Ok(())
        }
        ApprovalResponse::Deny => Err(ToolError::PermissionDenied(
            "Command denied by user approval policy.".into(),
        )),
    }
}
