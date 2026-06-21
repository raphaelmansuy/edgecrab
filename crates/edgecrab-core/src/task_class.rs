//! Heuristic task classification for verification epistemology (spec 015 P1.2).

use edgecrab_types::{Message, Role};

use crate::config::{HarnessConfig, PreviewConfig};

/// Coarse task class — drives which tool outcomes count as verification evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskClass {
    #[default]
    General,
    /// Visual / UX polish — preview in browser or screenshot, not syntax-only checks.
    VisualUx,
    /// Code change with compile/test gates.
    CodeChange,
    /// Research / read-heavy — lighter verification bar.
    Research,
}

pub struct TaskClassifier;

impl TaskClassifier {
    pub fn classify(user_message: &str, path_hints: &[String]) -> TaskClass {
        let lower = user_message.to_ascii_lowercase();

        let visual_keywords = [
            "beautiful",
            "amazing",
            "ux",
            "ui",
            "design",
            "css",
            "layout",
            "visual",
            "polish",
            "aesthetic",
        ];
        let has_visual_keyword = visual_keywords.iter().any(|k| lower.contains(k));
        let has_demo_path = path_hints.iter().any(|p| {
            let pl = p.to_ascii_lowercase();
            pl.contains("demo/") || pl.contains("games") || pl.contains("/ui/")
        });

        if has_visual_keyword || has_demo_path {
            return TaskClass::VisualUx;
        }

        let research_keywords = ["research", "find out", "explain", "what is", "how does"];
        if research_keywords.iter().any(|k| lower.contains(k)) {
            return TaskClass::Research;
        }

        let code_keywords = [
            "refactor",
            "migrate",
            "implement",
            "fix bug",
            "compile",
            "cargo test",
            "patch",
        ];
        if code_keywords.iter().any(|k| lower.contains(k)) {
            return TaskClass::CodeChange;
        }

        TaskClass::General
    }
}

/// Classify from conversation history (first user turn + path hints from tool args).
pub fn classify_from_messages(messages: &[Message]) -> TaskClass {
    let user_text: String = messages
        .iter()
        .filter(|m| m.role == Role::User)
        .map(|m| m.text_content())
        .collect::<Vec<_>>()
        .join("\n");
    let paths = collect_path_hints(messages);
    TaskClassifier::classify(&user_text, &paths)
}

fn collect_path_hints(messages: &[Message]) -> Vec<String> {
    let mut paths = Vec::new();
    for msg in messages {
        if msg.role != Role::Assistant {
            continue;
        }
        if let Some(calls) = msg.tool_calls.as_ref() {
            for call in calls {
                if let Ok(val) = call.parsed_args()
                    && let Some(path) = val.get("path").and_then(|v| v.as_str())
                {
                    paths.push(path.to_string());
                }
            }
        }
    }
    paths
}

/// Whether strict verification applies for this conversation (VisualUx always strict).
pub fn effective_verification_strict(harness: &HarnessConfig, messages: &[Message]) -> bool {
    harness.verification_strict || classify_from_messages(messages) == TaskClass::VisualUx
}

/// Session-scoped preview enable for visual tasks when profile/global preview is off (P0.9 rank 2).
pub fn apply_visual_ux_session_preview(class: TaskClass, preview: &PreviewConfig) -> bool {
    if class != TaskClass::VisualUx || preview.enabled {
        return false;
    }
    let policy = edgecrab_security::url_safety::PreviewPolicy {
        enabled: true,
        allowed_ports: if preview.allow_localhost_ports.is_empty() {
            vec![8000, 8888, 5173, 3000, 8080]
        } else {
            preview.allow_localhost_ports.clone()
        },
        allow_any_loopback_port: preview.allow_any_loopback_port,
    };
    edgecrab_security::url_safety::set_preview_policy(policy);
    true
}

/// Advisory footer injected once per session for visual tasks.
pub fn task_class_advisory(class: TaskClass, cwd: Option<&std::path::Path>) -> Option<String> {
    let base = match class {
        TaskClass::VisualUx => Some(
            "[harness] Task class: visual_ux — verify with browser_navigate to \
             http://127.0.0.1:PORT/ then browser_snapshot or vision_analyze; syntax-only terminal \
             checks are not sufficient. Do not create markdown verification reports — use browser \
             evidence. Enable preview via /config if localhost is blocked."
                .to_string(),
        ),
        TaskClass::CodeChange => Some(
            "[harness] Task class: code_change — run tests or compile checks before claiming done."
                .to_string(),
        ),
        TaskClass::Research | TaskClass::General => None,
    }?;
    if let Some(targets) = verify_targets_footer(class, cwd) {
        Some(format!("{base}\n[harness] Suggested verify: {targets}"))
    } else {
        Some(base)
    }
}

/// Manifest-derived verify commands (Hermes `_VERIFY_TARGETS` parity).
pub fn verify_targets_footer(class: TaskClass, cwd: Option<&std::path::Path>) -> Option<String> {
    let cwd = cwd?;
    match class {
        TaskClass::CodeChange => detect_coding_verify_commands(cwd),
        TaskClass::VisualUx => {
            if cwd.join("demo").is_dir() || cwd.join("index.html").exists() {
                Some("python3 -m http.server 8000 → browser_navigate http://127.0.0.1:8000/".into())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn detect_coding_verify_commands(cwd: &std::path::Path) -> Option<String> {
    let mut cmds = Vec::new();
    if cwd.join("Cargo.toml").exists() {
        cmds.push("cargo test");
        cmds.push("cargo clippy -- -D warnings");
    }
    if cwd.join("package.json").exists() {
        cmds.push("npm test");
        if cwd.join("tsconfig.json").exists() {
            cmds.push("npx tsc --noEmit");
        }
    }
    if cmds.is_empty() {
        None
    } else {
        Some(cmds.join(" · "))
    }
}

/// Whether a tool result counts as verification evidence for this task class.
pub fn is_verification_tool_for_class(tool_name: &str, class: TaskClass) -> bool {
    match class {
        TaskClass::VisualUx => matches!(
            tool_name,
            "browser_navigate"
                | "browser_snapshot"
                | "computer_use"
                | "capture_screenshot"
                | "analyze_image"
                | "vision"
        ),
        TaskClass::CodeChange => matches!(
            tool_name,
            "terminal"
                | "run_process"
                | "write_file"
                | "patch"
                | "apply_patch"
                | "execute_code"
                | "lsp_apply_code_action"
                | "lsp_rename"
                | "lsp_format_document"
                | "lsp_format_range"
        ),
        TaskClass::Research | TaskClass::General => is_general_verification_tool(tool_name),
    }
}

fn is_general_verification_tool(name: &str) -> bool {
    matches!(
        name,
        "terminal"
            | "run_process"
            | "write_file"
            | "patch"
            | "apply_patch"
            | "execute_code"
            | "delegate_task"
            | "manage_cron_jobs"
            | "checkpoint"
            | "lsp_apply_code_action"
            | "lsp_rename"
            | "lsp_format_document"
            | "lsp_format_range"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::Message;

    #[test]
    fn ha13_beautiful_demo_path_is_visual_ux() {
        let class = TaskClassifier::classify(
            "Improve game demo/games003 make it more beautiful, amazing UX/UI",
            &["demo/games003/index.html".into()],
        );
        assert_eq!(class, TaskClass::VisualUx);
    }

    #[test]
    fn ha20d_visual_ux_advisory_mentions_preview_url() {
        let advisory = task_class_advisory(TaskClass::VisualUx, None).expect("advisory");
        assert!(advisory.contains("127.0.0.1") || advisory.contains("browser_navigate"));
        assert!(advisory.contains("security.preview") || advisory.contains("vision"));
    }

    #[test]
    fn ha20d_code_change_advisory_mentions_test_commands() {
        let dir = std::env::temp_dir().join(format!("edgecrab-task-class-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let advisory = task_class_advisory(TaskClass::CodeChange, Some(&dir)).expect("advisory");
        assert!(advisory.contains("cargo test"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn terminal_not_verification_for_visual_ux() {
        assert!(!is_verification_tool_for_class(
            "terminal",
            TaskClass::VisualUx
        ));
        assert!(is_verification_tool_for_class(
            "browser_navigate",
            TaskClass::VisualUx
        ));
    }

    #[test]
    fn effective_verification_strict_for_visual_ux() {
        let harness = HarnessConfig::default();
        let messages = vec![Message::user("make demo/race_gamey beautiful UX")];
        assert!(effective_verification_strict(&harness, &messages));
    }

    #[test]
    fn apply_visual_ux_session_preview_enables_loopback() {
        use edgecrab_security::url_safety::{
            PreviewPolicy, preview_policy_test_guard, set_preview_policy,
        };

        let _guard = preview_policy_test_guard();
        set_preview_policy(PreviewPolicy::default());
        let preview = PreviewConfig::default();
        assert!(apply_visual_ux_session_preview(
            TaskClass::VisualUx,
            &preview
        ));
        let policy = edgecrab_security::url_safety::current_preview_policy();
        assert!(policy.enabled);
        assert!(policy.allowed_ports.contains(&8000));
    }

    #[test]
    fn classify_from_messages_uses_first_user_text() {
        let messages = vec![
            Message::user("make the login page more beautiful"),
            Message::assistant("On it."),
        ];
        assert_eq!(classify_from_messages(&messages), TaskClass::VisualUx);
    }
}
