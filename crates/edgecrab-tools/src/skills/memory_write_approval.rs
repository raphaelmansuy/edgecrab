//! Memory write-approval gate — Hermes `write_approval.py` parity for MEMORY.md / USER.md.
//!
//! When `memory.write_approval` is on, `memory_write` stages instead of committing;
//! review via `/memory pending|approve|reject|approval`.

use std::path::Path;

use edgecrab_types::ToolError;
use serde::{Deserialize, Serialize};

use super::pending_store::{
    SUBSYSTEM_MEMORY, discard_pending, format_pending_list, get_pending, list_pending, stage_write,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryWritePayload {
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub old_content: Option<String>,
    #[serde(default)]
    pub old_text: Option<String>,
    #[serde(default = "default_target")]
    pub target: String,
}

fn default_action() -> String {
    "add".into()
}

fn default_target() -> String {
    "memory".into()
}

pub fn memory_write_approval_enabled(config_enabled: bool) -> bool {
    config_enabled
}

pub fn memory_write_summary(payload: &MemoryWritePayload) -> String {
    let action = payload.action.as_str();
    let (filename, _) = crate::tools::memory::resolve_memory_target_public(&payload.target);
    match action {
        "add" => {
            let preview = payload
                .content
                .as_deref()
                .unwrap_or("")
                .trim()
                .chars()
                .take(80)
                .collect::<String>();
            format!("add to {filename}: {preview}")
        }
        "replace" => {
            let old = payload
                .old_content
                .as_deref()
                .or(payload.old_text.as_deref())
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>();
            format!("replace in {filename}: '{old}'…")
        }
        "remove" => {
            let old = payload
                .old_content
                .as_deref()
                .or(payload.old_text.as_deref())
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>();
            format!("remove from {filename}: '{old}'…")
        }
        other => format!("{other} {filename}"),
    }
}

pub enum MemoryWriteGate {
    Allow,
    Staged(String),
}

pub fn maybe_gate_memory_write(
    home: &Path,
    payload: MemoryWritePayload,
    write_approval: bool,
) -> MemoryWriteGate {
    if !write_approval {
        return MemoryWriteGate::Allow;
    }
    let summary = memory_write_summary(&payload);
    let action = payload.action.clone();
    let payload_value = serde_json::to_value(&payload).unwrap_or_default();
    let record = stage_write(
        home,
        SUBSYSTEM_MEMORY,
        payload_value,
        &summary,
        "foreground",
        &action,
    );
    MemoryWriteGate::Staged(format!(
        "Staged for approval (memory.write_approval is on). Not yet saved — review with /memory pending.\n\
         Pending id: {}\n{summary}",
        record.id
    ))
}

pub async fn apply_memory_write_payload(
    home: &Path,
    payload: &MemoryWritePayload,
) -> Result<String, ToolError> {
    crate::tools::memory::apply_memory_write_public(home, payload).await
}

pub async fn apply_pending_memory_write(home: &Path, id: &str) -> Result<String, ToolError> {
    let record = get_pending(home, SUBSYSTEM_MEMORY, id)
        .ok_or_else(|| ToolError::NotFound(format!("Pending {id} not found")))?;
    let payload: MemoryWritePayload = serde_json::from_value(record.payload).map_err(|e| {
        ToolError::InvalidArgs {
            tool: "memory_write".into(),
            message: format!("invalid pending payload: {e}"),
        }
    })?;
    let result = apply_memory_write_payload(home, &payload).await?;
    let _ = discard_pending(home, SUBSYSTEM_MEMORY, id);
    Ok(result)
}

pub fn format_memory_pending_state(home: &Path, write_approval: bool) -> String {
    format!(
        "memory.write_approval = {}\n\n{}",
        if write_approval { "on" } else { "off" },
        format_pending_list(home, SUBSYSTEM_MEMORY)
    )
}

pub fn memory_pending_detail(home: &Path, id: &str) -> Option<String> {
    let record = get_pending(home, SUBSYSTEM_MEMORY, id)?;
    let payload: MemoryWritePayload = serde_json::from_value(record.payload.clone()).ok()?;
    let (filename, _) = crate::tools::memory::resolve_memory_target_public(&payload.target);
    let mut out = format!(
        "# Pending memory write {}: {}\n\nAction: {}\nFile: {}\n",
        record.id, record.summary, payload.action, filename
    );
    if let Some(content) = payload.content.as_deref().filter(|c| !c.is_empty()) {
        out.push_str("\n[pending content]\n");
        out.push_str(content);
    }
    if let Some(old) = payload
        .old_content
        .as_deref()
        .or(payload.old_text.as_deref())
        .filter(|c| !c.is_empty())
    {
        out.push_str("\n[match selector]\n");
        out.push_str(old);
    }
    Some(out)
}

/// Slash handler context for `/memory` governance subcommands.
pub struct MemorySubcommandContext<'a> {
    pub write_approval: bool,
    pub set_write_approval: Option<&'a dyn Fn(bool) -> Result<(), String>>,
}

/// Handle `/memory pending|approve|reject|approval|show` — returns `None` if not matched.
pub fn handle_memory_pending_subcommand(
    home: &Path,
    args: &str,
    ctx: &MemorySubcommandContext<'_>,
) -> Option<String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Some(format_memory_pending_state(home, ctx.write_approval));
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let first = tokens[0].to_ascii_lowercase();
    match first.as_str() {
        "pending" => Some(format_pending_list(home, SUBSYSTEM_MEMORY)),
        "approve" | "apply" => Some(approve_pending(home, tokens.get(1).copied())),
        "reject" | "deny" | "drop" => Some(reject_pending(home, tokens.get(1).copied())),
        "approval" | "mode" => Some(set_approval_mode(
            tokens.get(1).copied(),
            ctx.write_approval,
            ctx.set_write_approval,
        )),
        "show" | "view" | "detail" => tokens
            .get(1)
            .and_then(|id| memory_pending_detail(home, id))
            .or_else(|| Some("Usage: /memory show <id>".into())),
        _ => None,
    }
}

fn approve_pending(home: &Path, target: Option<&str>) -> String {
    let Some(target) = target else {
        return "Usage: /memory approve <id|all>".into();
    };
    if target.eq_ignore_ascii_case("all") {
        return approve_all_pending(home);
    }
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => match handle.block_on(apply_pending_memory_write(home, target)) {
            Ok(msg) => msg,
            Err(e) => format!("Approve failed: {e}"),
        },
        Err(_) => format!("Cannot approve inline (no async runtime). Pending id: {target}"),
    }
}

fn approve_all_pending(home: &Path) -> String {
    let records = list_pending(home, SUBSYSTEM_MEMORY);
    if records.is_empty() {
        return "No pending memory writes.".into();
    }
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => return "Cannot approve inline (no async runtime).".into(),
    };
    let mut ok = 0u32;
    let mut errors = Vec::new();
    for record in records {
        match handle.block_on(apply_pending_memory_write(home, &record.id)) {
            Ok(_) => ok += 1,
            Err(e) => errors.push(format!("{}: {e}", record.id)),
        }
    }
    if errors.is_empty() {
        format!("Approved {ok} pending memory write(s).")
    } else {
        format!(
            "Approved {ok}; {} failed:\n{}",
            errors.len(),
            errors.join("\n")
        )
    }
}

fn reject_pending(home: &Path, target: Option<&str>) -> String {
    let Some(target) = target else {
        return "Usage: /memory reject <id|all>".into();
    };
    if target.eq_ignore_ascii_case("all") {
        let ids: Vec<String> = list_pending(home, SUBSYSTEM_MEMORY)
            .into_iter()
            .map(|r| r.id)
            .collect();
        if ids.is_empty() {
            return "No pending memory writes.".into();
        }
        let n = ids.len();
        for id in ids {
            let _ = discard_pending(home, SUBSYSTEM_MEMORY, &id);
        }
        return format!("Rejected {n} pending memory write(s).");
    }
    if discard_pending(home, SUBSYSTEM_MEMORY, target) {
        format!("Rejected pending memory write '{target}'.")
    } else {
        format!("No pending memory write with id '{target}'.")
    }
}

fn set_approval_mode(
    mode: Option<&str>,
    current: bool,
    set_write_approval: Option<&dyn Fn(bool) -> Result<(), String>>,
) -> String {
    let Some(mode) = mode.map(str::trim).filter(|m| !m.is_empty()) else {
        return format!(
            "memory.write_approval is {}.\nUsage: /memory approval on|off",
            if current { "on" } else { "off" }
        );
    };
    let enabled = match mode.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" | "enable" | "enabled" => true,
        "off" | "false" | "no" | "0" | "disable" | "disabled" => false,
        other => return format!("Unknown mode '{other}'. Use on|off."),
    };
    if let Some(set) = set_write_approval {
        match set(enabled) {
            Ok(()) => {
                return format!(
                    "memory.write_approval set to '{}'.",
                    if enabled { "on" } else { "off" }
                );
            }
            Err(e) => return format!("Failed to set memory.write_approval: {e}"),
        }
    }
    format!(
        "Set memory.write_approval: {} in ~/.edgecrab/config.yaml (or use /config).",
        enabled
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome;

    #[test]
    fn gate_stages_when_write_approval_on() {
        let home = TestEdgecrabHome::new();
        let payload = MemoryWritePayload {
            action: "add".into(),
            content: Some("Remember: tea".into()),
            old_content: None,
            old_text: None,
            target: "memory".into(),
        };
        match maybe_gate_memory_write(home.path(), payload, true) {
            MemoryWriteGate::Staged(msg) => assert!(msg.contains("Pending id:")),
            MemoryWriteGate::Allow => panic!("expected staged"),
        }
        assert_eq!(
            list_pending(home.path(), SUBSYSTEM_MEMORY).len(),
            1
        );
    }

    #[test]
    fn pending_subcommand_routes() {
        let home = TestEdgecrabHome::new();
        let ctx = MemorySubcommandContext {
            write_approval: false,
            set_write_approval: None,
        };
        assert!(handle_memory_pending_subcommand(home.path(), "list", &ctx).is_none());
        assert!(handle_memory_pending_subcommand(home.path(), "pending", &ctx).is_some());
        assert!(handle_memory_pending_subcommand(home.path(), "", &ctx).is_some());
    }
}
