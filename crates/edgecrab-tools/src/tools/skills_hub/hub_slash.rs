//! Shared `/skills` hub subcommands — CLI + gateway dispatch (DRY).

use std::path::Path;

use super::{
    InstallGate, add_tap, format_taps_list, index, inspect_hub_skill, install_identifier,
    refresh_unified_index, remove_tap, render_search_report, render_sources_catalog, search_hub,
    uninstall_skill, update_all_installed_skills, update_installed_skill,
};

/// Handle hub-related `/skills` args after config/pending handlers return None.
pub async fn handle_skills_hub_slash(
    args: &str,
    skills_dir: &Path,
    configured_hub_url: Option<&str>,
) -> Option<String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("").trim();

    match cmd {
        "index" => Some(handle_index_subcommand(rest).await),
        "inspect" => handle_inspect(rest, skills_dir).await,
        "review" => handle_review(rest, skills_dir).await,
        "hub" | "search" => Some(handle_search(rest, configured_hub_url).await),
        "install" => handle_install(rest, skills_dir).await,
        "trust" => Some(handle_trust(rest, skills_dir).await),
        "untrust" => Some(handle_untrust(rest)),
        "trusted" | "trusts" => Some(super::format_guard_approvals_list()),
        "check" => Some(handle_check(rest).await),
        "reset" => Some(handle_reset(rest)),
        "opt-out" | "optout" => Some(handle_opt_out(rest)),
        "opt-in" | "optin" => Some(handle_opt_in(rest).await),
        "snapshot" => Some(handle_snapshot(rest, skills_dir).await),
        "update" => handle_update(rest, skills_dir).await,
        "remove" | "uninstall" | "rm" => handle_remove(rest, skills_dir),
        "catalog" | "sources" => Some(render_sources_catalog()),
        "tap" | "taps" => Some(handle_tap_subcommand(rest)),
        "audit" => Some(handle_audit(rest, skills_dir)),
        "lock" | "installed" => Some(super::format_installed_lock()),
        _ => None,
    }
}

fn handle_audit(sub: &str, skills_dir: &Path) -> String {
    let trimmed = sub.trim();
    if trimmed.eq_ignore_ascii_case("log") {
        return super::format_audit_log_tail(20);
    }
    let mut deep = false;
    let mut name_tokens = Vec::new();
    for token in trimmed.split_whitespace() {
        if token == "--deep" {
            deep = true;
        } else {
            name_tokens.push(token);
        }
    }
    let skill_name = name_tokens.first().copied();
    super::audit_installed_hub_skills(skills_dir, skill_name, deep)
}

fn parse_install_operand(raw: &str) -> (String, InstallGate) {
    let mut gate = InstallGate::default();
    let id: Vec<_> = raw
        .split_whitespace()
        .filter(|token| match *token {
            "--force" | "-f" => {
                gate.force = true;
                false
            }
            "--trust" | "--accept-risk" => {
                gate.trust = true;
                false
            }
            _ => true,
        })
        .collect();
    (id.join(" "), gate)
}

fn handle_tap_subcommand(sub: &str) -> String {
    let trimmed = sub.trim();
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let action = parts.next().unwrap_or("list").trim();
    let rest = parts.next().unwrap_or("").trim();

    match action {
        "" | "list" | "ls" => format_taps_list(),
        "add" => {
            let mut tokens = rest.split_whitespace();
            let Some(repo) = tokens.next() else {
                return "Usage: /skills tap add owner/repo [root-path]".into();
            };
            if !repo.contains('/') {
                return "Usage: /skills tap add owner/repo [root-path]".into();
            }
            let root = tokens.next().unwrap_or("skills");
            let url = if root == "skills" && !rest.contains(' ') {
                repo.to_string()
            } else {
                format!("{repo}/{root}")
            };
            let name = repo.replace('/', "-");
            add_tap(&name, &url, "community");
            format!(
                "Added tap '{name}' -> https://github.com/{url}\nSearch: /skills search <query>"
            )
        }
        "remove" | "rm" | "delete" => {
            if rest.is_empty() {
                return "Usage: /skills tap remove <name-or-repo>".into();
            }
            if remove_tap(rest) {
                format!("Removed tap '{rest}'.")
            } else {
                format!("Tap '{rest}' not found.")
            }
        }
        other => format!("Unknown tap subcommand '{other}'. Try: list, add, remove"),
    }
}

pub fn hub_slash_mutates_skills(args: &str) -> bool {
    let trimmed = args.trim();
    let cmd = trimmed.split_whitespace().next().unwrap_or("");
    match cmd {
        "install" | "update" | "remove" | "uninstall" | "rm" | "reset" => true,
        "opt-out" | "optout" if trimmed.contains("--remove") => true,
        "opt-in" | "optin" if trimmed.contains("--sync") => true,
        "snapshot" if trimmed.contains("import") => true,
        _ => false,
    }
}

fn handle_reset(operand: &str) -> String {
    let mut restore = false;
    let name: Vec<_> = operand
        .split_whitespace()
        .filter(|token| {
            if matches!(*token, "--restore" | "-r") {
                restore = true;
                false
            } else {
                true
            }
        })
        .collect();
    let name = name.join(" ");
    if name.is_empty() {
        return "Usage: /skills reset <name> [--restore]\n\
                Example: /skills reset google-workspace --restore"
            .into();
    }
    let result = crate::tools::skills_sync::reset_bundled_skill(&name, restore);
    let mut out = result.message.clone();
    if let Some(summary) = result.sync_summary {
        out.push_str(&format!("\nSync: {summary}"));
    }
    out
}

fn handle_opt_out(operand: &str) -> String {
    let remove = operand.split_whitespace().any(|t| t == "--remove");
    let dry_run = operand.split_whitespace().any(|t| t == "--dry-run");

    let res = crate::tools::skills_sync::set_bundled_skills_opt_out(true);
    if !res.ok {
        return res.message;
    }

    let mut out = res.message;
    if remove {
        let preview = crate::tools::skills_sync::remove_pristine_bundled_skills(dry_run);
        if !preview.removed.is_empty() {
            out.push_str(&format!(
                "\n\n{}: {}",
                if dry_run { "Would remove" } else { "Removed" },
                preview.removed.join(", ")
            ));
        }
        if !preview.skipped.is_empty() {
            out.push_str(&format!(
                "\nKept {} (user-modified or non-bundled).",
                preview.skipped.len()
            ));
        }
        if !dry_run && preview.removed.is_empty() && preview.skipped.is_empty() {
            out.push_str("\n\nNo pristine bundled skills to remove.");
        }
        out.push_str(&format!("\n{}", preview.message));
    } else {
        out.push_str("\nAdd --remove to delete unmodified bundled copies from disk.");
    }
    out
}

async fn handle_opt_in(operand: &str) -> String {
    let sync = operand.split_whitespace().any(|t| t == "--sync");
    let res = crate::tools::skills_sync::set_bundled_skills_opt_out(false);
    if !res.ok {
        return res.message;
    }
    let mut out = res.message;
    if sync {
        if let Some(report) = crate::tools::skills_sync::sync_on_startup() {
            out.push_str(&format!("\nRe-seeded bundled skills: {}", report.summary()));
        } else if crate::tools::skills_sync::is_bundled_skills_opt_out() {
            out.push_str("\nSync skipped — still opted out.");
        } else {
            out.push_str("\nNo bundled skills source found to sync.");
        }
    } else {
        out.push_str("\nRun `/skills opt-in --sync` to re-seed immediately.");
    }
    out
}

async fn handle_snapshot(operand: &str, skills_dir: &Path) -> String {
    let trimmed = operand.trim();
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let action = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("").trim();

    match action {
        "export" => {
            if rest.is_empty() {
                return "Usage: /skills snapshot export <file>\nUse `-` for stdout.".into();
            }
            match super::export_hub_snapshot(rest) {
                Ok(msg) => msg,
                Err(e) => format!("Export failed: {e}"),
            }
        }
        "import" => {
            if rest.is_empty() {
                return "Usage: /skills snapshot import <file> [--force]".into();
            }
            let force = rest.contains("--force");
            let path: Vec<_> = rest
                .split_whitespace()
                .filter(|t| *t != "--force")
                .collect();
            let path = path.join(" ");
            if path.is_empty() {
                return "Usage: /skills snapshot import <file> [--force]".into();
            }
            let optional_dir = crate::tools::skills_sync::optional_skills_dir();
            match super::import_hub_snapshot(&path, force, skills_dir, optional_dir.as_deref())
                .await
            {
                Ok(msg) => {
                    super::notify_hub_skills_mutated();
                    msg
                }
                Err(e) => format!("Import failed: {e}"),
            }
        }
        "" => "Usage: /skills snapshot export <file> | /skills snapshot import <file>".into(),
        other => format!("Unknown snapshot action '{other}'. Try: export, import"),
    }
}

async fn handle_index_subcommand(sub: &str) -> String {
    match sub.trim() {
        "refresh" => match refresh_unified_index().await {
            Ok(msg) => msg,
            Err(e) => {
                let boot = index::bootstrap_index_from_local_caches();
                if boot > 0 {
                    format!(
                        "Remote index unavailable ({e}). Bootstrapped {boot} skills from local hub caches."
                    )
                } else {
                    format!("Index refresh failed: {e}")
                }
            }
        },
        "status" => index::format_index_status(),
        "" => "Usage: /skills index refresh | /skills index status".into(),
        other => format!("Unknown index subcommand '{other}'. Try: refresh, status"),
    }
}

async fn handle_inspect(operand: &str, skills_dir: &Path) -> Option<String> {
    let (identifier, scan) = parse_inspect_operand(operand);
    if identifier.is_empty() {
        return Some(
            "Usage: /skills inspect <identifier> [--scan]\n\
             /skills inspect --scan <identifier>\n\
             --scan runs Skills Guard + lists all bundle files"
                .into(),
        );
    }
    if scan {
        let optional_dir = crate::tools::skills_sync::optional_skills_dir();
        return Some(
            match super::inspect_identifier_scan(identifier, skills_dir, optional_dir.as_deref())
                .await
            {
                Ok(text) => text,
                Err(e) => format!("Scan inspect failed: {e}"),
            },
        );
    }
    match inspect_hub_skill(identifier).await {
        Ok(text) => Some(text),
        Err(e) => Some(format!("Inspect failed: {e}")),
    }
}

async fn handle_review(operand: &str, skills_dir: &Path) -> Option<String> {
    let identifier = operand.trim();
    if identifier.is_empty() {
        return Some(
            "Usage: /skills review <identifier>\n\
             Fetches, scans, and prints the full guard report with file listing.\n\
             In EdgeCrab TUI use the same command to open the interactive file inspector."
                .into(),
        );
    }
    let optional_dir = crate::tools::skills_sync::optional_skills_dir();
    Some(
        match super::inspect_identifier_scan(identifier, skills_dir, optional_dir.as_deref()).await
        {
            Ok(text) => text,
            Err(e) => format!("Review failed: {e}"),
        },
    )
}

pub fn parse_inspect_operand(operand: &str) -> (&str, bool) {
    let trimmed = operand.trim();
    if trimmed.starts_with("--scan ") {
        return (trimmed.trim_start_matches("--scan ").trim(), true);
    }
    if trimmed.ends_with(" --scan") {
        return (trimmed.trim_end_matches(" --scan").trim(), true);
    }
    if trimmed == "--scan" {
        return ("", false);
    }
    (trimmed, false)
}

async fn handle_search(query: &str, configured_hub_url: Option<&str>) -> String {
    if query.is_empty() {
        return format!(
            "{}\n\nTry: /skills search diagram\nOr: /skills index refresh",
            render_sources_catalog()
        );
    }
    let report = search_hub(query, None, 8, configured_hub_url).await;
    format!(
        "{}\nInstall: /skills install <identifier>",
        render_search_report(query, &report)
    )
}

async fn handle_check(operand: &str) -> String {
    let optional_dir = crate::tools::skills_sync::optional_skills_dir();
    let name = operand.trim();
    let filter = if name.is_empty() { None } else { Some(name) };
    let results = super::check_for_skill_updates(optional_dir.as_deref(), filter).await;
    super::format_check_report(&results)
}

async fn handle_install(operand: &str, skills_dir: &Path) -> Option<String> {
    let (identifier, gate) = parse_install_operand(operand);
    if identifier.is_empty() {
        return Some(
            "Usage:\n\
             /skills install <local-path>\n\
             /skills install clawhub:<slug>\n\
             /skills install skills.sh:owner/repo/skill\n\
             /skills install owner/repo/path\n\
             /skills install https://…/SKILL.md\n\
             Flags:\n\
               --force   override caution verdict (community)\n\
               --trust   install despite dangerous verdict (after review)\n\
             Or pre-approve: /skills trust <identifier>"
                .into(),
        );
    }

    if looks_like_local_path(&identifier) {
        return None;
    }

    let optional_dir = crate::tools::skills_sync::optional_skills_dir();
    match install_identifier(&identifier, skills_dir, optional_dir.as_deref(), gate).await {
        Ok(outcome) => Some(format!(
            "{}\nActivate with: /skills view {}",
            outcome.message, outcome.skill_name
        )),
        Err(e) => Some(format!("Install failed: {e}")),
    }
}

async fn handle_trust(operand: &str, _skills_dir: &Path) -> String {
    let identifier = operand.trim();
    if identifier.is_empty() {
        return "Usage: /skills trust <identifier>\n\
                Fetches + scans the skill and records hash-bound approval for dangerous verdicts.\n\
                Then: /skills install <identifier>"
            .into();
    }
    let optional_dir = crate::tools::skills_sync::optional_skills_dir();
    match super::trust_identifier(identifier, optional_dir.as_deref()).await {
        Ok(msg) => msg,
        Err(e) => format!("Trust failed: {e}"),
    }
}

fn handle_untrust(operand: &str) -> String {
    let identifier = operand.trim();
    if identifier.is_empty() {
        return "Usage: /skills untrust <identifier>".into();
    }
    if super::guard_approvals::revoke_guard_approval(identifier) {
        format!("Revoked trust approval for `{identifier}`.")
    } else {
        format!("No trust approval found for `{identifier}`.")
    }
}

async fn handle_update(operand: &str, skills_dir: &Path) -> Option<String> {
    let optional_dir = crate::tools::skills_sync::optional_skills_dir();
    let gate = parse_install_gate_flags(operand);
    let name = operand
        .split_whitespace()
        .filter(|t| !matches!(*t, "--force" | "-f" | "--trust" | "--accept-risk"))
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        match update_all_installed_skills(skills_dir, optional_dir.as_deref(), gate).await {
            Ok(outcomes) => Some(super::render_update_outcomes(&outcomes)),
            Err(e) => Some(format!("Update failed: {e}")),
        }
    } else {
        match update_installed_skill(&name, skills_dir, optional_dir.as_deref(), gate).await {
            Ok(outcome) => Some(format!(
                "{}\nActivate with: /skills view {}",
                outcome.message, outcome.skill_name
            )),
            Err(e) => Some(format!("Update failed: {e}")),
        }
    }
}

fn parse_install_gate_flags(raw: &str) -> InstallGate {
    let mut gate = InstallGate::default();
    for token in raw.split_whitespace() {
        match token {
            "--force" | "-f" => gate.force = true,
            "--trust" | "--accept-risk" => gate.trust = true,
            _ => {}
        }
    }
    gate
}

fn handle_remove(operand: &str, skills_dir: &Path) -> Option<String> {
    if operand.is_empty() {
        return Some("Usage: /skills remove <skill-name>".into());
    }
    if let Ok(msg) = uninstall_skill(operand, skills_dir) {
        return Some(msg);
    }
    let candidates = [
        skills_dir.join(operand),
        skills_dir.join(format!("{operand}.md")),
    ];
    for path in candidates {
        if path.is_file() {
            return match std::fs::remove_file(&path) {
                Ok(_) => Some(format!("Skill '{operand}' removed.")),
                Err(e) => Some(format!("Remove failed: {e}")),
            };
        }
        if path.is_dir() {
            return match std::fs::remove_dir_all(&path) {
                Ok(_) => Some(format!("Skill directory '{operand}' removed.")),
                Err(e) => Some(format!("Remove failed: {e}")),
            };
        }
    }
    Some(format!("Skill '{operand}' not found."))
}

fn looks_like_local_path(operand: &str) -> bool {
    operand.starts_with('/')
        || operand.starts_with("./")
        || operand.starts_with("../")
        || operand.starts_with('~')
        || std::path::Path::new(operand).exists()
}

/// True when operand is a remote/hub identifier (not a local path).
pub fn is_remote_skill_identifier(operand: &str) -> bool {
    if looks_like_local_path(operand) {
        return false;
    }
    if operand.starts_with("http://") || operand.starts_with("https://") {
        return true;
    }
    for prefix in [
        "clawhub:",
        "clawhub/",
        "skills.sh:",
        "skills-sh:",
        "browse-sh:",
        "browse.sh:",
        "lobehub:",
        "lobehub/",
        "claude-marketplace:",
        "claude-marketplace/",
        "claude_marketplace:",
        "claude_marketplace/",
        "agentskills.io:",
        "agentskills:",
        "edgecrab:",
        "hermes-agent:",
        "openai:",
        "anthropics:",
        "official/",
    ] {
        if operand.starts_with(prefix) {
            return true;
        }
    }
    !operand.starts_with('.') && operand.matches('/').count() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_identifier_detects_lobehub_and_github() {
        assert!(is_remote_skill_identifier("lobehub:my-agent"));
        assert!(is_remote_skill_identifier("owner/repo/path/to/skill"));
        assert!(is_remote_skill_identifier("https://example.com/SKILL.md"));
        assert!(!is_remote_skill_identifier("./local/skill.md"));
        assert!(!is_remote_skill_identifier("/abs/path/SKILL.md"));
    }

    #[test]
    fn parse_install_operand_strips_force_and_trust_flags() {
        let (id, gate) = parse_install_operand("clawhub:foo --force --trust");
        assert_eq!(id, "clawhub:foo");
        assert!(gate.force);
        assert!(gate.trust);
    }

    #[test]
    fn parse_inspect_operand_detects_scan_flag() {
        let (id, scan) = parse_inspect_operand("--scan clawhub:foo");
        assert!(scan);
        assert_eq!(id, "clawhub:foo");
        let (id2, scan2) = parse_inspect_operand("clawhub:foo --scan");
        assert!(scan2);
        assert_eq!(id2, "clawhub:foo");
    }

    #[test]
    fn hub_slash_mutates_skills_detects_install() {
        assert!(super::hub_slash_mutates_skills("install clawhub:foo"));
        assert!(super::hub_slash_mutates_skills("update my-skill"));
        assert!(!super::hub_slash_mutates_skills("search diagram"));
        assert!(!super::hub_slash_mutates_skills("tap add owner/repo"));
    }

    #[test]
    fn hub_slash_mutates_skills_detects_reset() {
        assert!(super::hub_slash_mutates_skills(
            "reset google-workspace --restore"
        ));
    }
}
