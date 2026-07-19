//! Safety gates for `computer_use` (mirrors Hermes `tool.py`).

use std::collections::HashSet;

use edgecrab_types::ToolError;

use crate::approval_runtime;
use crate::registry::{ApprovalRequest, ApprovalResponse, ToolContext};

/// Actions that read, not mutate.
pub const SAFE_ACTIONS: &[&str] = &["capture", "wait", "list_apps"];

/// Actions that mutate user-visible state.
pub const DESTRUCTIVE_ACTIONS: &[&str] = &[
    "click",
    "double_click",
    "right_click",
    "middle_click",
    "drag",
    "scroll",
    "type",
    "key",
    "set_value",
    "focus_app",
    "launch_app",
    "navigate",
];

static BLOCKED_KEY_COMBOS: &[&[&str]] = &[
    &["cmd", "shift", "backspace"],
    &["cmd", "option", "backspace"],
    &["cmd", "ctrl", "q"],
    &["cmd", "shift", "q"],
    &["cmd", "option", "shift", "q"],
];

static KEY_ALIASES: &[(&str, &str)] = &[
    ("command", "cmd"),
    ("control", "ctrl"),
    ("alt", "option"),
    ("⌘", "cmd"),
    ("⌥", "option"),
];

static BLOCKED_TYPE_PATTERNS: &[&str] = &[
    r"(?i)curl\s+[^|]*\|\s*bash",
    r"(?i)curl\s+[^|]*\|\s*sh",
    r"(?i)wget\s+[^|]*\|\s*bash",
    r"(?i)\bsudo\s+rm\s+-[rf]",
    r"(?i)\brm\s+-rf\s+/\s*$",
    r"(?i):\s*\(\)\s*\{\s*:\|:\s*&\s*\}",
];

fn canon_key_combo(keys: &str) -> HashSet<String> {
    keys.split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .map(|p| {
            KEY_ALIASES
                .iter()
                .find(|(k, _)| *k == p)
                .map(|(_, v)| (*v).to_string())
                .unwrap_or(p)
        })
        .collect()
}

pub fn blocked_type_pattern(text: &str) -> Option<String> {
    for pat in BLOCKED_TYPE_PATTERNS {
        if regex::Regex::new(pat)
            .ok()
            .is_some_and(|re| re.is_match(text))
        {
            return Some((*pat).to_string());
        }
    }
    None
}

pub fn blocked_key_combo(keys: &str) -> Option<Vec<String>> {
    let combo = canon_key_combo(keys);
    for blocked in BLOCKED_KEY_COMBOS {
        let blocked_set: HashSet<String> = blocked.iter().map(|s| (*s).to_string()).collect();
        if blocked_set.is_subset(&combo) {
            return Some(blocked.iter().map(|s| (*s).to_string()).collect());
        }
    }
    None
}

pub fn is_destructive(action: &str) -> bool {
    DESTRUCTIVE_ACTIONS.contains(&action)
}

pub fn summarize_action(action: &str, args: &serde_json::Value) -> String {
    match action {
        "click" | "double_click" | "right_click" | "middle_click" => {
            if let Some(n) = args.get("element").and_then(|v| v.as_u64()) {
                return format!("{action} element #{n}");
            }
            if let Some(coord) = args.get("coordinate").and_then(|v| v.as_array())
                && coord.len() >= 2
            {
                return format!("{action} at [{}, {}]", coord[0], coord[1]);
            }
            action.to_string()
        }
        "drag" => {
            let src = args
                .get("from_element")
                .or_else(|| args.get("from_coordinate"));
            let dst = args.get("to_element").or_else(|| args.get("to_coordinate"));
            format!("drag {src:?} → {dst:?}")
        }
        "scroll" => format!(
            "scroll {} x{}",
            args.get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("?"),
            args.get("amount").and_then(|v| v.as_i64()).unwrap_or(3)
        ),
        "type" => {
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let preview = crate::safe_truncate(text, 60);
            if text.len() > 60 {
                format!("type {preview:?}...")
            } else {
                format!("type {preview:?}")
            }
        }
        "key" => format!(
            "key {:?}",
            args.get("keys").and_then(|v| v.as_str()).unwrap_or("")
        ),
        "focus_app" => {
            let app = args.get("app").and_then(|v| v.as_str()).unwrap_or("");
            if args.get("raise_window").and_then(|v| v.as_bool()) == Some(true) {
                format!("focus {app:?} (raise)")
            } else {
                format!("focus {app:?}")
            }
        }
        other => other.to_string(),
    }
}

pub async fn ensure_destructive_approved(
    ctx: &ToolContext,
    action: &str,
    args: &serde_json::Value,
) -> Result<(), ToolError> {
    if SAFE_ACTIONS.contains(&action) || !is_destructive(action) {
        return Ok(());
    }
    if !ctx.config.computer_use_confirm_destructive {
        return Ok(());
    }
    let session = ctx
        .session_key
        .clone()
        .unwrap_or_else(|| ctx.session_id.clone());
    if approval_runtime::yolo_enabled_for_session(&session) {
        return Ok(());
    }
    if approval_runtime::computer_use_action_approved(&session, action) {
        return Ok(());
    }
    let Some(tx) = &ctx.approval_tx else {
        // Gateway/CLI without UI: allow (Hermes default when no callback).
        return Ok(());
    };
    let summary = summarize_action(action, args);
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ApprovalRequest {
        command: summary.clone(),
        full_command: format!("computer_use {action} {args}"),
        reasons: vec![format!("Destructive computer_use action: {summary}")],
        kind: crate::registry::ApprovalKind::Terminal,
        response_tx: resp_tx,
    })
    .map_err(|_| {
        ToolError::PermissionDenied(
            "computer_use action requires approval, but no approver is available.".into(),
        )
    })?;
    let response = tokio::select! {
        _ = ctx.cancel.cancelled() => {
            return Err(ToolError::Other("Interrupted by user".into()));
        }
        result = resp_rx => result.map_err(|_| ToolError::PermissionDenied(
            "Approval request was cancelled.".into(),
        ))?
    };
    match response {
        ApprovalResponse::Once => Ok(()),
        ApprovalResponse::Session => {
            approval_runtime::approve_computer_use_action_for_session(&session, action);
            Ok(())
        }
        ApprovalResponse::Always => {
            approval_runtime::set_yolo_for_session(&session, true);
            Ok(())
        }
        ApprovalResponse::Deny => Err(ToolError::PermissionDenied(
            "computer_use action denied by user.".into(),
        )),
    }
}
