use std::path::Path;

use edgecrab_types::ToolError;
use regex::Regex;
use serde_json::Value;

use super::pending_store::{
    SUBSYSTEM_SKILLS, PendingWriteRecord, discard_pending, get_pending, list_pending, stage_write,
};
use super::usage::{bump_patch, format_usage_summary, is_pinned, set_pinned};

pub fn skills_write_approval_enabled(config_enabled: bool) -> bool {
    config_enabled
}

fn stage_skill_write(
    home: &Path,
    payload: Value,
    summary: &str,
    origin: &str,
) -> PendingWriteRecord {
    let action = payload
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    stage_write(home, SUBSYSTEM_SKILLS, payload, summary, origin, &action)
}

use std::sync::LazyLock;

static FM_DESC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^description:\s*(.+)$").expect("regex"));

pub fn skill_gist(payload: &Value) -> String {
    let action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
    match action {
        "create" | "edit" => {
            let content = payload
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let size = if content.len() >= 1024 {
                format!("{} KB", content.len() / 1024 + 1)
            } else {
                format!("{} chars", content.len())
            };
            let verb = if action == "create" {
                "create"
            } else {
                "rewrite"
            };
            if let Some(desc) = frontmatter_description(content) {
                format!("{verb} '{name}' — {desc} ({size})")
            } else {
                format!("{verb} '{name}' ({size})")
            }
        }
        "patch" => {
            let target = payload
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("SKILL.md");
            format!("patch '{name}' {target}")
        }
        "write_file" => {
            let fp = payload
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("write {fp} in '{name}'")
        }
        "remove_file" => {
            let fp = payload
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("remove {fp} from '{name}'")
        }
        "delete" => format!("delete skill '{name}'"),
        other => format!("{other} '{name}'"),
    }
}

fn frontmatter_description(content: &str) -> Option<String> {
    FM_DESC.captures(content).and_then(|c| c.get(1)).map(|m| {
        m.as_str()
            .trim()
            .trim_matches(['\'', '"'])
            .chars()
            .take(140)
            .collect()
    })
}

pub enum SkillManageGate {
    Allow,
    Staged(String),
}

pub fn maybe_gate_skill_manage(
    home: &Path,
    payload: Value,
    write_approval: bool,
) -> SkillManageGate {
    if !write_approval {
        return SkillManageGate::Allow;
    }
    let gist = skill_gist(&payload);
    let record = stage_skill_write(home, payload, &gist, "foreground");
    SkillManageGate::Staged(format!(
        "Staged for approval (skills.write_approval is on). Not yet saved — review with /skills pending.\n\
         Pending id: {}\n{}",
        record.id, gist
    ))
}

pub async fn apply_skill_manage_payload(home: &Path, payload: &Value) -> Result<String, ToolError> {
    let action = payload
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidArgs {
            tool: "skill_manage".into(),
            message: "missing action".into(),
        })?;
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidArgs {
            tool: "skill_manage".into(),
            message: "missing name".into(),
        })?;
    let skills_base = home.join("skills");
    let category = payload.get("category").and_then(|v| v.as_str());
    let skill_dir = if action == "create" {
        if let Some(cat) = category {
            skills_base.join(cat).join(name)
        } else {
            skills_base.join(name)
        }
    } else {
        crate::tools::skills::find_skill_dir_public(&skills_base, name)
            .unwrap_or_else(|| skills_base.join(name))
    };

    match action {
        "create" | "edit" => {
            let content = payload
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skill_manage".into(),
                    message: "content required".into(),
                })?;
            tokio::fs::create_dir_all(&skill_dir)
                .await
                .map_err(|e| ToolError::Other(format!("Cannot create skill dir: {e}")))?;
            let skill_path = skill_dir.join("SKILL.md");
            tokio::fs::write(&skill_path, content)
                .await
                .map_err(|e| ToolError::Other(format!("Cannot write SKILL.md: {e}")))?;
            if action == "edit" {
                bump_patch(home, name);
            }
            Ok(format!(
                "Skill '{name}' {action}d at {}",
                skill_path.display()
            ))
        }
        "delete" => {
            if !skill_dir.exists() {
                return Err(ToolError::NotFound(format!(
                    "Skill '{name}' does not exist"
                )));
            }
            tokio::fs::remove_dir_all(&skill_dir)
                .await
                .map_err(|e| ToolError::Other(format!("Cannot delete skill: {e}")))?;
            Ok(format!("Skill '{name}' deleted."))
        }
        "patch" => {
            let old = payload
                .get("old_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skill_manage".into(),
                    message: "old_string required".into(),
                })?;
            let new = payload
                .get("new_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skill_manage".into(),
                    message: "new_string required".into(),
                })?;
            let replace_all = payload
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let path = skill_dir.join("SKILL.md");
            if !path.is_file() {
                return Err(ToolError::NotFound(format!(
                    "Skill '{name}' does not exist"
                )));
            }
            let current = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| ToolError::Other(format!("Cannot read SKILL.md: {e}")))?;
            let count = current.matches(old).count();
            if count == 0 {
                return Err(ToolError::InvalidArgs {
                    tool: "skill_manage".into(),
                    message: format!(
                        "old_string not found in skill '{name}'. Verify the exact text including whitespace."
                    ),
                });
            }
            if count > 1 && !replace_all {
                return Err(ToolError::InvalidArgs {
                    tool: "skill_manage".into(),
                    message: format!(
                        "old_string matches {count} times in skill '{name}'. Use replace_all=true to replace all, or provide more context."
                    ),
                });
            }
            let updated = if replace_all {
                current.replace(old, new)
            } else {
                current.replacen(old, new, 1)
            };
            tokio::fs::write(&path, updated)
                .await
                .map_err(|e| ToolError::Other(e.to_string()))?;
            bump_patch(home, name);
            let replaced = if replace_all { count } else { 1 };
            Ok(format!(
                "Skill '{name}' patched: replaced {replaced} occurrence(s)."
            ))
        }
        "write_file" => {
            let fp = payload
                .get("file_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skill_manage".into(),
                    message: "file_path required".into(),
                })?;
            let fc = payload
                .get("file_content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skill_manage".into(),
                    message: "file_content required".into(),
                })?;
            validate_supporting_path(fp)?;
            if !skill_dir.exists() {
                return Err(ToolError::NotFound(format!(
                    "Skill '{name}' does not exist. Create it first."
                )));
            }
            let file_path = skill_dir.join(fp);
            if let Some(parent) = file_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| ToolError::Other(e.to_string()))?;
            }
            tokio::fs::write(&file_path, fc)
                .await
                .map_err(|e| ToolError::Other(e.to_string()))?;
            Ok(format!("Wrote '{fp}' in skill '{name}'."))
        }
        "remove_file" => {
            let fp = payload
                .get("file_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skill_manage".into(),
                    message: "file_path required".into(),
                })?;
            validate_supporting_path(fp)?;
            let file_path = skill_dir.join(fp);
            if !file_path.is_file() {
                return Err(ToolError::NotFound(format!(
                    "File '{fp}' not found in skill '{name}'"
                )));
            }
            tokio::fs::remove_file(&file_path)
                .await
                .map_err(|e| ToolError::Other(e.to_string()))?;
            Ok(format!("Removed '{fp}' from skill '{name}'."))
        }
        other => Err(ToolError::InvalidArgs {
            tool: "skill_manage".into(),
            message: format!("unknown action '{other}'"),
        }),
    }
}

fn validate_supporting_path(fp: &str) -> Result<(), ToolError> {
    if fp.contains("..") {
        return Err(ToolError::PermissionDenied(
            "Path traversal not allowed".into(),
        ));
    }
    let allowed = ["references/", "templates/", "scripts/", "assets/"];
    if !allowed.iter().any(|p| fp.starts_with(p)) {
        return Err(ToolError::PermissionDenied(format!(
            "file_path must be under references/, templates/, scripts/, or assets/ (got {fp})"
        )));
    }
    Ok(())
}

pub async fn apply_pending_skill_write(home: &Path, id: &str) -> Result<String, ToolError> {
    let record = get_pending(home, SUBSYSTEM_SKILLS, id)
        .ok_or_else(|| ToolError::NotFound(format!("Pending {id} not found")))?;
    let result = apply_skill_manage_payload(home, &record.payload).await?;
    let _ = discard_pending(home, SUBSYSTEM_SKILLS, id);
    Ok(result)
}

pub fn format_skills_pending_state(
    home: &Path,
    write_approval: bool,
    inline_shell: bool,
) -> String {
    format!(
        "skills.write_approval = {}\nskills.inline_shell = {}\n\n{}",
        if write_approval { "on" } else { "off" },
        if inline_shell { "on" } else { "off" },
        format_skills_pending_list(home)
    )
}

fn format_skills_pending_list(home: &Path) -> String {
    let records = list_pending(home, SUBSYSTEM_SKILLS);
    if records.is_empty() {
        return "No pending skills writes.".into();
    }
    let mut lines = vec![format!("Pending skills writes ({}):", records.len())];
    for r in records {
        lines.push(format!("  {}  {}", r.id, r.summary));
    }
    lines.push(String::new());
    lines.push("Apply: /skills approve <id>   Reject: /skills reject <id>".into());
    lines.push("Review diff: /skills diff <id>".into());
    lines.join("\n")
}

pub fn skill_pending_diff(home: &Path, id: &str) -> Option<String> {
    let record = get_pending(home, SUBSYSTEM_SKILLS, id)?;
    let payload = &record.payload;
    let action = payload.get("action")?.as_str()?;
    let name = payload.get("name")?.as_str()?;
    let skills_base = home.join("skills");
    let skill_dir = crate::tools::skills::find_skill_dir_public(&skills_base, name)
        .unwrap_or_else(|| skills_base.join(name));

    match action {
        "create" => payload
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        "edit" => {
            let new = payload.get("content")?.as_str()?;
            let current = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap_or_default();
            Some(unified_diff("SKILL.md", &current, new))
        }
        "patch" => {
            let old = payload.get("old_string")?.as_str()?;
            let new = payload.get("new_string")?.as_str()?;
            let current = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap_or_default();
            let updated = if payload
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                current.replace(old, new)
            } else {
                current.replacen(old, new, 1).to_string()
            };
            Some(unified_diff("SKILL.md", &current, &updated))
        }
        _ => Some(format!(
            "Action: {action}\n{}",
            serde_json::to_string_pretty(payload).unwrap_or_default()
        )),
    }
}

fn unified_diff(path: &str, old: &str, new: &str) -> String {
    if old == new {
        return format!("(no changes for {path})");
    }
    format!("--- {path} (current)\n+++ {path} (pending)\n\n[current]\n{old}\n\n[pending]\n{new}")
}

/// Slash handler context for `/skills` governance subcommands (approval, inline shell).
pub struct SkillsSubcommandContext<'a> {
    pub write_approval: bool,
    pub inline_shell: bool,
    pub set_write_approval: Option<&'a dyn Fn(bool) -> Result<(), String>>,
    pub set_inline_shell: Option<&'a dyn Fn(bool) -> Result<(), String>>,
}

/// Handle `/skills pending|approve|reject|approval|inline-shell|diff|usage` — returns `None` if not matched.
pub fn handle_skills_pending_subcommand(
    home: &Path,
    args: &str,
    ctx: &SkillsSubcommandContext<'_>,
) -> Option<String> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let first = tokens.first()?.to_ascii_lowercase();
    match first.as_str() {
        "pending" => Some(format_skills_pending_list(home)),
        "approve" | "apply" => Some(approve_pending(home, tokens.get(1).copied())),
        "reject" | "deny" | "drop" => Some(reject_pending(home, tokens.get(1).copied())),
        "diff" => tokens
            .get(1)
            .and_then(|id| skill_pending_diff(home, id))
            .or_else(|| Some("Usage: /skills diff <id>".into())),
        "approval" | "mode" => Some(set_approval_mode(
            tokens.get(1).copied(),
            ctx.write_approval,
            ctx.set_write_approval,
        )),
        "inline-shell" | "inline_shell" | "shell" => Some(set_inline_shell_mode(
            tokens.get(1).copied(),
            ctx.inline_shell,
            ctx.set_inline_shell,
        )),
        "usage" | "stats" => Some(format_usage_summary(home, 15)),
        "pin" => tokens.get(1).map(|name| {
            if set_pinned(home, name, true) {
                format!("Pinned skill '{name}' (excluded from /curator stale).")
            } else {
                "Usage: /skills pin <skill-name>".into()
            }
        }),
        "unpin" => tokens.get(1).map(|name| {
            if is_pinned(home, name) && set_pinned(home, name, false) {
                format!("Unpinned skill '{name}'.")
            } else {
                format!("Skill '{name}' is not pinned.")
            }
        }),
        _ => None,
    }
}

fn approve_pending(home: &Path, target: Option<&str>) -> String {
    let Some(target) = target else {
        return "Usage: /skills approve <id|all>".into();
    };
    if target.eq_ignore_ascii_case("all") {
        return approve_all_pending(home);
    }
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => match handle.block_on(apply_pending_skill_write(home, target)) {
            Ok(msg) => msg,
            Err(e) => format!("Approve failed: {e}"),
        },
        Err(_) => format!("Cannot approve inline (no async runtime). Pending id: {target}"),
    }
}

fn approve_all_pending(home: &Path) -> String {
    let records = list_pending(home, SUBSYSTEM_SKILLS);
    if records.is_empty() {
        return "No pending skills writes.".into();
    }
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => return "Cannot approve inline (no async runtime).".into(),
    };
    let mut ok = 0u32;
    let mut errors = Vec::new();
    for record in records {
        match handle.block_on(apply_pending_skill_write(home, &record.id)) {
            Ok(_) => ok += 1,
            Err(e) => errors.push(format!("{}: {e}", record.id)),
        }
    }
    if errors.is_empty() {
        format!("Approved {ok} pending skill write(s).")
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
        return "Usage: /skills reject <id|all>".into();
    };
    if target.eq_ignore_ascii_case("all") {
        let ids: Vec<String> = list_pending(home, SUBSYSTEM_SKILLS)
            .into_iter()
            .map(|r| r.id)
            .collect();
        if ids.is_empty() {
            return "No pending skills writes.".into();
        }
        let n = ids.len();
        for id in ids {
            let _ = discard_pending(home, SUBSYSTEM_SKILLS, &id);
        }
        return format!("Rejected {n} pending skill write(s).");
    }
    if discard_pending(home, SUBSYSTEM_SKILLS, target) {
        format!("Rejected pending write {target}.")
    } else {
        format!("No pending write with id {target}.")
    }
}

fn set_approval_mode(
    mode: Option<&str>,
    current: bool,
    set_write_approval: Option<&dyn Fn(bool) -> Result<(), String>>,
) -> String {
    let Some(mode) = mode.map(str::trim).filter(|m| !m.is_empty()) else {
        return format!(
            "skills.write_approval is {}.\nUsage: /skills approval on|off",
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
                    "skills.write_approval set to '{}'.",
                    if enabled { "on" } else { "off" }
                );
            }
            Err(e) => return format!("Failed to set skills.write_approval: {e}"),
        }
    }
    format!(
        "Set skills.write_approval: {} in ~/.edgecrab/config.yaml (or use /config).",
        enabled
    )
}

fn set_inline_shell_mode(
    mode: Option<&str>,
    current: bool,
    set_inline_shell: Option<&dyn Fn(bool) -> Result<(), String>>,
) -> String {
    let Some(mode) = mode.map(str::trim).filter(|m| !m.is_empty()) else {
        return format!(
            "skills.inline_shell is {}.\nUsage: /skills inline-shell on|off",
            if current { "on" } else { "off" }
        );
    };
    let enabled = match mode.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" | "enable" | "enabled" => true,
        "off" | "false" | "no" | "0" | "disable" | "disabled" => false,
        other => return format!("Unknown mode '{other}'. Use on|off."),
    };
    if let Some(set) = set_inline_shell {
        match set(enabled) {
            Ok(()) => {
                return format!(
                    "skills.inline_shell set to '{}'.",
                    if enabled { "on" } else { "off" }
                );
            }
            Err(e) => return format!("Failed to set skills.inline_shell: {e}"),
        }
    }
    format!(
        "Set skills.inline_shell: {} in ~/.edgecrab/config.yaml (or use /config).",
        enabled
    )
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stages_and_lists_pending() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let payload =
            json!({"action":"create","name":"demo","content":"---\ndescription: x\n---\n"});
        let record = stage_skill_write(dir.path(), payload, "create demo", "foreground");
        let pending = list_pending(dir.path(), SUBSYSTEM_SKILLS);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, record.id);
    }

    #[test]
    fn gate_stages_when_write_approval_on() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let payload = json!({"action":"create","name":"demo","content":"body"});
        match maybe_gate_skill_manage(dir.path(), payload, true) {
            SkillManageGate::Staged(msg) => assert!(msg.contains("Pending id:")),
            SkillManageGate::Allow => panic!("expected staged"),
        }
        assert_eq!(list_pending(dir.path(), SUBSYSTEM_SKILLS).len(), 1);
    }

    #[test]
    fn gate_allows_when_write_approval_off() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let payload = json!({"action":"create","name":"demo","content":"body"});
        match maybe_gate_skill_manage(dir.path(), payload, false) {
            SkillManageGate::Allow => {}
            SkillManageGate::Staged(_) => panic!("expected allow"),
        }
        assert!(list_pending(dir.path(), SUBSYSTEM_SKILLS).is_empty());
    }

    #[test]
    fn pending_subcommand_routes() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let ctx = SkillsSubcommandContext {
            write_approval: false,
            inline_shell: false,
            set_write_approval: None,
            set_inline_shell: None,
        };
        assert!(handle_skills_pending_subcommand(dir.path(), "list", &ctx).is_none());
        assert!(handle_skills_pending_subcommand(dir.path(), "pending", &ctx).is_some());
    }
}
