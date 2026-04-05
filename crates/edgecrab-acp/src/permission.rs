//! # ACP Permission model — tool approval bridge for editor integration
//!
//! WHY permissions: When the agent wants to run a dangerous command
//! (e.g. `rm -rf`, `git push --force`), the ACP server asks the editor
//! for user approval before executing. This bridges the security system
//! into the editor UI's permission dialog.
//!
//! ```text
//! Agent ──"run: rm -rf /tmp"──► Permission Bridge ──► Editor UI
//!                                       ◄── allow_once ──┘
//! ```

use serde::{Deserialize, Serialize};

/// Outcome of a permission request sent to the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    /// Approve this one invocation.
    AllowOnce,
    /// Approve all future invocations of this pattern.
    AllowAlways,
    /// Deny the request.
    Deny,
}

/// A permission request to send to the editor.
#[derive(Debug, Serialize)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub command: String,
    pub reason: String,
    pub session_id: String,
}

/// Check whether a tool name is in the ACP-safe set (no approval needed).
///
/// Read-only tools like `read_file`, `search_files`, `memory` never need
/// approval. Write tools like `terminal`, `write_file`, `patch` go through
/// the approval flow.
pub fn is_safe_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read_file"
            | "search_files"
            | "web_search"
            | "web_extract"
            | "web_crawl"
            | "session_search"
            | "skills_list"
            | "skill_view"
            | "manage_todo_list"
            | "memory_read"
            | "memory_write"
            | "clarify"
            | "mcp_list_tools"
    )
}

/// Tools that are exposed in ACP mode (coding-focused subset).
///
/// Intentionally excludes messaging, cronjob, TTS, and other
/// non-editor tools per the spec in docs/004_tools_system/003_toolset_composition.md.
pub const ACP_TOOLS: &[&str] = &[
    "web_search",
    "web_extract",
    "web_crawl",
    "terminal",
    "run_process",
    "list_processes",
    "kill_process",
    "read_file",
    "write_file",
    "patch",
    "search_files",
    "skills_list",
    "skill_view",
    "skill_manage",
    "skills_hub",
    "browser_navigate",
    "browser_snapshot",
    "browser_screenshot",
    "browser_click",
    "browser_type",
    "browser_scroll",
    "browser_console",
    "browser_back",
    "browser_press",
    "browser_close",
    "browser_get_images",
    "browser_vision",
    "browser_wait_for",
    "browser_select",
    "browser_hover",
    "manage_todo_list",
    "memory_read",
    "memory_write",
    "session_search",
    "checkpoint",
    "execute_code",
    "delegate_task",
    "mcp_list_tools",
    "mcp_call_tool",
];

/// Check whether a tool is allowed in ACP mode.
pub fn is_acp_tool(tool_name: &str) -> bool {
    ACP_TOOLS.contains(&tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_tools_are_safe() {
        assert!(is_safe_tool("read_file"));
        assert!(is_safe_tool("search_files"));
        assert!(is_safe_tool("web_crawl"));
        assert!(is_safe_tool("memory_read"));
        assert!(is_safe_tool("memory_write"));
    }

    #[test]
    fn write_tools_are_not_safe() {
        assert!(!is_safe_tool("terminal"));
        assert!(!is_safe_tool("write_file"));
        assert!(!is_safe_tool("patch"));
    }

    #[test]
    fn acp_tools_include_coding_set() {
        assert!(is_acp_tool("terminal"));
        assert!(is_acp_tool("read_file"));
        assert!(is_acp_tool("web_crawl"));
        assert!(is_acp_tool("write_file"));
        assert!(is_acp_tool("patch"));
        assert!(is_acp_tool("search_files"));
        assert!(is_acp_tool("skill_manage"));
        assert!(is_acp_tool("skills_hub"));
        assert!(is_acp_tool("browser_wait_for"));
        assert!(is_acp_tool("browser_select"));
        assert!(is_acp_tool("browser_hover"));
        assert!(is_acp_tool("browser_vision"));
    }

    #[test]
    fn acp_tools_exclude_messaging() {
        assert!(!is_acp_tool("send_message"));
        assert!(!is_acp_tool("cronjob"));
        assert!(!is_acp_tool("tts"));
    }

    #[test]
    fn permission_decision_serializes() {
        let json = serde_json::to_string(&PermissionDecision::AllowOnce).expect("serialize");
        assert_eq!(json, "\"allow_once\"");
    }

    #[test]
    fn permission_request_serializes() {
        let req = PermissionRequest {
            tool_name: "terminal".to_string(),
            command: "rm -rf /tmp".to_string(),
            reason: "Dangerous command".to_string(),
            session_id: "abc".to_string(),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains("terminal"));
        assert!(json.contains("rm -rf"));
    }
}
