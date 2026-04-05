//! Approval policy engine — gates destructive operations.
//!
//! WHY: LLMs can be prompt-injected into running destructive commands.
//! The approval layer sits between tool-call extraction and tool execution,
//! giving the user (or a smart classifier) a chance to veto.
//!
//! ```text
//!   LLM response
//!       │
//!       ▼
//!   ┌──────────────────┐
//!   │ ApprovalPolicy   │──── Off  ──→ execute immediately
//!   │  .check()        │──── Manual ─→ prompt user via ApprovalHandler
//!   │                  │──── Smart ──→ aux LLM classifies, may prompt
//!   └──────────────────┘
//!       │
//!       ▼
//!   tool execution
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::command_scan::{CommandScanner, ScanResult};
use crate::normalize::normalize_command;

/// Decision from the approval flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Denied,
    /// Persist to config `command_allowlist` — survives restarts.
    AlwaysApprove,
    /// Auto-approve identical commands for this session only.
    ApproveForSession,
}

/// Approval mode from config.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalMode {
    #[default]
    Manual,
    Smart,
    Off,
}

/// Outcome of `ApprovalPolicy::check()`.
#[derive(Debug, Clone)]
pub struct ApprovalCheck {
    /// Whether the operation needs user/smart approval.
    pub needs_approval: bool,
    /// Why approval is required (empty if not needed).
    pub reasons: Vec<String>,
    /// The scan result from the command scanner (if a command was scanned).
    pub scan_result: Option<ScanResult>,
}

/// Per-session approval state.
///
/// WHY RwLock+HashMap: Multiple async tasks may check approvals
/// concurrently (parallel tool execution). RwLock gives read-heavy
/// access without contention; writes only happen when the user
/// approves a new pattern.
pub struct ApprovalPolicy {
    mode: ApprovalMode,
    scanner: CommandScanner,
    /// Tool names that always require approval (from config).
    approval_required_tools: HashSet<String>,
    /// Session-scoped auto-approvals: session_id → set of approved commands.
    session_approved: RwLock<HashMap<String, HashSet<String>>>,
    /// Permanent allowlist patterns (from config `command_allowlist`).
    permanent_allowlist: RwLock<HashSet<String>>,
}

impl ApprovalPolicy {
    pub fn new(mode: ApprovalMode, approval_required_tools: Vec<String>) -> Self {
        Self {
            mode,
            scanner: CommandScanner::new(),
            approval_required_tools: approval_required_tools.into_iter().collect(),
            session_approved: RwLock::new(HashMap::new()),
            permanent_allowlist: RwLock::new(HashSet::new()),
        }
    }

    /// Check whether a tool invocation requires approval.
    ///
    /// Returns immediately for `Off` mode. For `Manual`/`Smart`, checks:
    ///   1. Is the tool name in the config's `approval_required` list?
    ///   2. If tool is "terminal", does the command match dangerous patterns?
    ///   3. Has this command been session-approved or permanently allowlisted?
    pub fn check(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        session_id: &str,
    ) -> ApprovalCheck {
        if self.mode == ApprovalMode::Off {
            return ApprovalCheck {
                needs_approval: false,
                reasons: Vec::new(),
                scan_result: None,
            };
        }

        let mut reasons = Vec::new();
        let mut scan_result = None;

        // Check 1: Tool-level approval requirement
        if self.approval_required_tools.contains(tool_name) {
            reasons.push(format!("tool '{tool_name}' requires approval per config"));
        }

        // Check 2: Terminal command scanning
        if tool_name == "terminal" || tool_name == "shell" {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                let result = self.scanner.scan(cmd);
                if result.is_dangerous {
                    for m in &result.matched_patterns {
                        reasons.push(format!("{}: {}", m.category_label(), m.description));
                    }
                    // Check if already approved — clear reasons if so
                    if self.is_approved(cmd, session_id) {
                        reasons.clear();
                    }
                }
                scan_result = Some(result);
            }
        }

        ApprovalCheck {
            needs_approval: !reasons.is_empty(),
            reasons,
            scan_result,
        }
    }

    /// Record a session-scoped approval for a command.
    pub fn approve_for_session(&self, command: &str, session_id: &str) {
        let keys = self.approval_keys_for_command(command);
        if let Ok(mut map) = self.session_approved.write() {
            map.entry(session_id.to_string()).or_default().extend(keys);
        }
    }

    /// Record a permanent approval (add to allowlist).
    pub fn approve_permanently(&self, command: &str) {
        let keys = self.approval_keys_for_command(command);
        if let Ok(mut set) = self.permanent_allowlist.write() {
            set.extend(keys);
        }
    }

    /// Load permanent allowlist patterns from config.
    pub fn load_permanent_allowlist(&self, patterns: &[String]) {
        if let Ok(mut set) = self.permanent_allowlist.write() {
            for p in patterns {
                set.insert(p.clone());
                set.insert(command_key(p));
            }
        }
    }

    /// Check if a command is already approved (session or permanent).
    fn is_approved(&self, command: &str, session_id: &str) -> bool {
        let keys = self.approval_keys_for_command(command);
        if let Ok(set) = self.permanent_allowlist.read() {
            if keys.iter().any(|k| set.contains(k)) {
                return true;
            }
        }
        if let Ok(map) = self.session_approved.read() {
            if let Some(cmds) = map.get(session_id) {
                return keys.iter().any(|k| cmds.contains(k));
            }
        }
        false
    }

    /// Build stable approval keys for a command.
    ///
    /// Keys include a canonical command form and, when dangerous patterns are
    /// matched, a deterministic signature derived from those patterns.
    fn approval_keys_for_command(&self, command: &str) -> Vec<String> {
        let mut keys = vec![command_key(command)];
        let scan = self.scanner.scan(command);
        if let Some(sig) = scan_signature_key(&scan) {
            keys.push(sig);
        }
        keys.sort();
        keys.dedup();
        keys
    }

    pub fn mode(&self) -> ApprovalMode {
        self.mode
    }
}

/// Canonical command key used for robust equality matching.
fn command_key(command: &str) -> String {
    let normalized = normalize_command(command);
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("cmd:{compact}")
}

/// Deterministic dangerous-pattern signature key.
///
/// If multiple dangerous patterns match, they are sorted and joined to produce
/// a stable key independent of command formatting details.
fn scan_signature_key(scan: &ScanResult) -> Option<String> {
    if !scan.is_dangerous {
        return None;
    }

    let mut sigs: Vec<String> = scan
        .matched_patterns
        .iter()
        .map(|m| format!("{}:{}", m.category_label(), m.description.to_lowercase()))
        .collect();
    sigs.sort();
    sigs.dedup();

    if sigs.is_empty() {
        None
    } else {
        Some(format!("sig:{}", sigs.join("|")))
    }
}

/// Helper to get a human-readable label for DangerCategory.
impl crate::command_scan::MatchedPattern {
    pub fn category_label(&self) -> &'static str {
        use crate::command_scan::DangerCategory;
        match self.category {
            DangerCategory::DestructiveFileOps => "destructive-file-ops",
            DangerCategory::PermissionEscalation => "permission-escalation",
            DangerCategory::SystemDamage => "system-damage",
            DangerCategory::SqlDestruction => "sql-destruction",
            DangerCategory::RemoteCodeExecution => "remote-code-exec",
            DangerCategory::ProcessKilling => "process-killing",
            DangerCategory::GatewayProtection => "gateway-protection",
            DangerCategory::FileOverwrite => "file-overwrite",
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> ApprovalPolicy {
        ApprovalPolicy::new(ApprovalMode::Manual, vec!["dangerous_tool".into()])
    }

    fn terminal_args(cmd: &str) -> serde_json::Value {
        serde_json::json!({ "command": cmd })
    }

    #[test]
    fn off_mode_never_gates() {
        let p = ApprovalPolicy::new(ApprovalMode::Off, vec!["terminal".into()]);
        let check = p.check("terminal", &terminal_args("rm -rf /"), "s1");
        assert!(!check.needs_approval);
    }

    #[test]
    fn manual_mode_gates_dangerous_command() {
        let p = policy();
        let check = p.check("terminal", &terminal_args("rm -rf /tmp"), "s1");
        assert!(check.needs_approval);
        assert!(!check.reasons.is_empty());
    }

    #[test]
    fn safe_command_not_gated() {
        let p = policy();
        let check = p.check("terminal", &terminal_args("ls -la"), "s1");
        assert!(!check.needs_approval);
    }

    #[test]
    fn tool_name_in_approval_required_list() {
        let p = policy();
        let check = p.check("dangerous_tool", &serde_json::json!({}), "s1");
        assert!(check.needs_approval);
        assert!(check.reasons[0].contains("requires approval"));
    }

    #[test]
    fn session_approval_bypasses_gate() {
        let p = policy();
        let cmd = "rm -rf /tmp/old";
        let check = p.check("terminal", &terminal_args(cmd), "s1");
        assert!(check.needs_approval);
        p.approve_for_session(cmd, "s1");
        let check = p.check("terminal", &terminal_args(cmd), "s1");
        assert!(!check.needs_approval);
    }

    #[test]
    fn session_approval_does_not_leak_across_sessions() {
        let p = policy();
        let cmd = "rm -rf /tmp/old";
        p.approve_for_session(cmd, "s1");
        let check = p.check("terminal", &terminal_args(cmd), "s2");
        assert!(check.needs_approval);
    }

    #[test]
    fn permanent_allowlist_bypasses_all_sessions() {
        let p = policy();
        let cmd = "rm -rf /tmp/cache";
        p.approve_permanently(cmd);
        let check = p.check("terminal", &terminal_args(cmd), "s1");
        assert!(!check.needs_approval);
        let check = p.check("terminal", &terminal_args(cmd), "s2");
        assert!(!check.needs_approval);
    }

    #[test]
    fn session_approval_is_stable_across_formatting_variants() {
        let p = policy();
        p.approve_for_session("rm -rf /tmp/old", "s1");

        let check = p.check("terminal", &terminal_args("  RM   -RF    /tmp/old  "), "s1");
        assert!(!check.needs_approval);

        let check = p.check(
            "terminal",
            &terminal_args("\x1b[31mrm -rf /tmp/old\x1b[0m"),
            "s1",
        );
        assert!(!check.needs_approval);
    }

    #[test]
    fn pattern_signature_approval_allows_equivalent_dangerous_commands() {
        let p = policy();
        p.approve_for_session("rm -rf /tmp/a", "s1");

        // Same danger signature, different concrete path.
        let check = p.check("terminal", &terminal_args("rm -rf /tmp/b"), "s1");
        assert!(!check.needs_approval);
    }

    #[test]
    fn load_permanent_allowlist_accepts_legacy_raw_command_entries() {
        let p = policy();
        p.load_permanent_allowlist(&["rm -rf /tmp/cache".to_string()]);

        let check = p.check("terminal", &terminal_args("RM   -RF  /tmp/cache"), "s-any");
        assert!(!check.needs_approval);
    }

    #[test]
    fn non_terminal_tool_not_scanned() {
        let p = ApprovalPolicy::new(ApprovalMode::Manual, Vec::new());
        let check = p.check(
            "read_file",
            &serde_json::json!({"path": "/etc/passwd"}),
            "s1",
        );
        assert!(!check.needs_approval);
        assert!(check.scan_result.is_none());
    }
}
