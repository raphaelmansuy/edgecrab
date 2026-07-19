//! Task classification for verification epistemology (spec 015 P1.2 / 018 P6 / 006).
//!
//! First principles: path/artifact/contract signals — not vibe keywords
//! ("beautiful", "amazing", "aesthetic", "powerpoint").

use edgecrab_types::{Message, Role};

use crate::config::{HarnessConfig, PreviewConfig};

/// Coarse task class — drives which tool outcomes count as verification evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskClass {
    #[default]
    General,
    /// Visual / UX polish — preview in browser or screenshot, not syntax-only checks.
    VisualUx,
    /// Document / deck / office artifact — verify by file presence, not localhost theater.
    Document,
    /// Code change with compile/test gates.
    CodeChange,
    /// Research / read-heavy — lighter verification bar.
    Research,
    /// Video / Hyperframe / media render — file artifact evidence, not browser thrash (019).
    MediaRender,
}

pub struct TaskClassifier;

impl TaskClassifier {
    pub fn classify(user_message: &str, path_hints: &[String]) -> TaskClass {
        // Document paths win over loose demo/ directory names.
        if has_document_path_signal(user_message, path_hints) {
            return TaskClass::Document;
        }

        // Media render before visual (Hyperframe under demos/ would otherwise be VisualUx).
        if has_media_render_signal(user_message, path_hints) {
            return TaskClass::MediaRender;
        }

        if has_visual_path_signal(user_message, path_hints) {
            return TaskClass::VisualUx;
        }

        if has_code_manifest_signal(user_message, path_hints) {
            return TaskClass::CodeChange;
        }

        let lower = user_message.to_ascii_lowercase();

        // Explicit test/build gates only — not soft verbs like "implement".
        if lower.contains("cargo test")
            || lower.contains("cargo build")
            || lower.contains("npm test")
            || lower.contains("pytest")
        {
            return TaskClass::CodeChange;
        }

        if lower.contains("research") {
            return TaskClass::Research;
        }

        TaskClass::General
    }
}

/// Deterministic media_render signals — product names + media extensions only.
fn has_media_render_signal(user_message: &str, path_hints: &[String]) -> bool {
    let lower = user_message.to_ascii_lowercase();
    if lower.contains("hyperframe") || lower.contains("hyperframes") {
        return true;
    }
    // Explicit render+video/mp4 pairing (not soft "make a video someday").
    if (lower.contains("render") || lower.contains("ffmpeg"))
        && (lower.contains("video")
            || lower.contains(".mp4")
            || lower.contains(".webm")
            || lower.contains(".mov"))
    {
        return true;
    }
    path_hints.iter().any(|p| {
        let pl = p.to_ascii_lowercase();
        pl.ends_with(".mp4") || pl.ends_with(".webm") || pl.ends_with(".mov")
    })
}

fn has_code_manifest_signal(user_message: &str, path_hints: &[String]) -> bool {
    let is_code = |p: &str| {
        let pl = p.to_ascii_lowercase();
        pl.ends_with("cargo.toml")
            || pl.ends_with("package.json")
            || pl.ends_with("pyproject.toml")
            || pl.ends_with(".rs")
            || pl.ends_with(".py")
            || pl.ends_with(".ts")
            || pl.ends_with(".tsx")
            || pl.ends_with(".go")
    };
    if path_hints.iter().any(|p| is_code(p)) {
        return true;
    }
    for token in user_message.split_whitespace() {
        let t = token.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | ',' | ';' | ':'));
        if is_code(t) {
            return true;
        }
    }
    false
}

fn is_document_path(path: &str) -> bool {
    let pl = path.to_ascii_lowercase();
    pl.ends_with(".pptx")
        || pl.ends_with(".ppt")
        || pl.ends_with(".pdf")
        || pl.ends_with(".docx")
        || pl.ends_with(".doc")
        || pl.ends_with(".xlsx")
        || pl.ends_with(".xls")
        || pl.ends_with(".odt")
        || pl.ends_with(".odp")
}

fn has_document_path_signal(user_message: &str, path_hints: &[String]) -> bool {
    if path_hints.iter().any(|p| is_document_path(p)) {
        return true;
    }
    for token in user_message.split_whitespace() {
        let t = token.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | ',' | ';' | ':'));
        if is_document_path(t) {
            return true;
        }
    }
    false
}

/// Web asset under a visual root — not directory name alone (006 pptx forensics).
fn is_web_asset_path(path: &str) -> bool {
    let pl = path.to_ascii_lowercase();
    pl.ends_with(".html")
        || pl.ends_with(".css")
        || pl.ends_with(".htm")
        || pl.contains("index.html")
}

/// VisualUx from landed/hinted **web assets**, not `demo/` directory alone.
fn has_visual_path_signal(user_message: &str, path_hints: &[String]) -> bool {
    let path_is_visual = |p: &str| {
        // Bare web assets anywhere count; under visual roots, require a web asset
        // (demo/pptx_raphael without .html must not arm VisualUx).
        if is_web_asset_path(p) {
            return true;
        }
        false
    };
    if path_hints.iter().any(|p| path_is_visual(p)) {
        return true;
    }
    for token in user_message.split_whitespace() {
        let t = token.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | ',' | '.' | ';' | ':'));
        if path_is_visual(t) {
            return true;
        }
    }
    // Substring scan for explicit web entrypoints only — not bare "demo/".
    let lower = user_message.to_ascii_lowercase();
    lower.contains("index.html")
        || lower.contains(".html")
        || lower.contains("./games/") && lower.contains(".html")
}

/// Classify from conversation history (first user turn + path hints from tool args).
///
/// Priority: landed document → Document; landed visual web → VisualUx; else path/contract.
pub fn classify_from_messages(messages: &[Message]) -> TaskClass {
    if landed_document_artifact(messages) {
        return TaskClass::Document;
    }
    if landed_visual_artifact(messages) {
        return TaskClass::VisualUx;
    }
    let user_text: String = messages
        .iter()
        .filter(|m| m.role == Role::User)
        .map(|m| m.text_content())
        .collect::<Vec<_>>()
        .join("\n");
    let paths = collect_path_hints(messages);
    let mut all_paths = paths;
    all_paths.extend(collect_mutation_paths_from_results(messages));
    TaskClassifier::classify(&user_text, &all_paths)
}

/// True when a successful file mutation landed an office/document artifact.
///
/// Same signals as [`document_artifact_evidence_present`] (007: shell-built
/// `.docx` must flip Document, not stay CodeChange).
fn landed_document_artifact(messages: &[Message]) -> bool {
    document_artifact_evidence_present(messages)
}

/// True when a successful file mutation landed HTML/CSS (served UI), not docs under demo/.
fn landed_visual_artifact(messages: &[Message]) -> bool {
    for msg in messages {
        if msg.role != Role::Tool {
            continue;
        }
        let Some(name) = msg.name.as_deref() else {
            continue;
        };
        if !matches!(name, "write_file" | "patch" | "apply_patch") {
            continue;
        }
        let content = msg.text_content();
        if edgecrab_types::parse_tool_error_payload(&content).is_some() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
            && let Some(p) = v.get("path").and_then(|p| p.as_str())
            && is_visual_artifact_path(p)
        {
            return true;
        }
        if let Some(p) = content
            .lines()
            .find_map(|line| line.strip_prefix("path: ").or_else(|| line.strip_prefix("\"path\":")))
        {
            let cleaned = p.trim().trim_matches(',').trim_matches('"');
            if is_visual_artifact_path(cleaned) {
                return true;
            }
        }
    }
    false
}

fn is_visual_artifact_path(path: &str) -> bool {
    // Landed web assets count; directory alone never does.
    is_web_asset_path(path)
}

fn collect_mutation_paths_from_results(messages: &[Message]) -> Vec<String> {
    let mut paths = Vec::new();
    for msg in messages {
        if msg.role != Role::Tool {
            continue;
        }
        if !matches!(msg.name.as_deref(), Some("write_file" | "patch" | "apply_patch")) {
            continue;
        }
        let text = msg.text_content();
        if edgecrab_types::parse_tool_error_payload(&text).is_some() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
            && v.get("ok").and_then(|o| o.as_bool()) == Some(true)
            && let Some(p) = v.get("path").and_then(|p| p.as_str())
        {
            paths.push(p.to_string());
        }
    }
    paths
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

/// Advisory footer injected once per session for visual/document tasks.
pub fn task_class_advisory(class: TaskClass, cwd: Option<&std::path::Path>) -> Option<String> {
    let base = match class {
        TaskClass::VisualUx => Some(
            "[harness] Task class: visual_ux — verify with browser_navigate to \
             http://127.0.0.1:PORT/ then browser_snapshot or vision_analyze; syntax-only terminal \
             checks are not sufficient. Do not create markdown verification reports — use browser \
             evidence. Enable preview via /config if localhost is blocked."
                .to_string(),
        ),
        TaskClass::Document => Some(
            "[harness] Task class: document — verify the artifact exists on disk \
             (non-zero .pptx/.pdf/.docx bytes). Browser localhost preview and Word/GUI \
             screencapture are not required for Done. Do not thrash open/screencapture."
                .to_string(),
        ),
        TaskClass::CodeChange => Some(
            "[harness] Task class: code_change — run tests or compile checks before claiming done."
                .to_string(),
        ),
        TaskClass::MediaRender => Some(
            "[harness] Task class: media_render — verify by output file existence \
             (non-zero .mp4/.webm/.mov bytes). Browser localhost preview is optional, \
             not required for Done. Do not thrash browser_vision."
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
        TaskClass::Document => Some(
            "confirm artifact path exists with non-zero size (ls/stat); optional PDF/JPG export"
                .into(),
        ),
        TaskClass::MediaRender => Some(
            "confirm render output path exists with non-zero size (mp4/webm/mov)".into(),
        ),
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
        // Perception tools only — navigate alone is not evidence (game005 forensics).
        TaskClass::VisualUx => matches!(
            tool_name,
            "browser_snapshot"
                | "browser_vision"
                | "computer_use"
                | "capture_screenshot"
                | "analyze_image"
                | "vision"
                | "vision_analyze"
        ),
        // Artifact landing + filesystem proof — not browser theater (006 pptx).
        TaskClass::Document => matches!(
            tool_name,
            "write_file"
                | "patch"
                | "apply_patch"
                | "terminal"
                | "run_process"
                | "execute_code"
                | "vision_analyze"
                | "analyze_image"
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
        TaskClass::MediaRender => matches!(
            tool_name,
            "write_file"
                | "patch"
                | "apply_patch"
                | "terminal"
                | "run_process"
                | "execute_code"
        ),
        TaskClass::Research | TaskClass::General => is_general_verification_tool(tool_name),
    }
}

/// Media file extension evidence (deterministic).
pub fn is_media_output_path(path: &str) -> bool {
    let pl = path.to_ascii_lowercase();
    pl.ends_with(".mp4") || pl.ends_with(".webm") || pl.ends_with(".mov")
}

/// True when a non-zero media output was written this session.
pub fn media_artifact_evidence_present(messages: &[Message]) -> bool {
    for path in collect_mutation_paths_from_results(messages) {
        if is_media_output_path(&path) {
            return true;
        }
    }
    for msg in messages {
        if msg.role != Role::Tool {
            continue;
        }
        if !matches!(
            msg.name.as_deref(),
            Some("terminal" | "run_process" | "execute_code")
        ) {
            continue;
        }
        let content = msg.text_content();
        if edgecrab_types::parse_tool_error_payload(&content).is_some() {
            continue;
        }
        // Structured success with media path mention in result is weak; prefer mutations.
        for ext in [".mp4", ".webm", ".mov"] {
            if content.contains(ext) && content.contains("bytes") {
                return true;
            }
        }
    }
    false
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

/// True when messages show a successful **create/mutation** document artifact.
///
/// P9 (008): inspect-only shell (`ls`/`stat`/`find`/…) never contributes evidence.
/// Observation of a style-reference `.pptx` is not Done.
pub fn document_artifact_evidence_present(messages: &[Message]) -> bool {
    for path in collect_mutation_paths_from_results(messages) {
        if is_document_path(&path) {
            return true;
        }
    }
    // Intent on write/patch args is not landing — only successful mutation results
    // above. Do not treat terminal/execute_code `path` args as evidence (P9).
    for msg in messages {
        if msg.role != Role::Tool {
            continue;
        }
        if !matches!(
            msg.name.as_deref(),
            Some("terminal" | "run_process" | "execute_code")
        ) {
            continue;
        }
        let content = msg.text_content();
        if edgecrab_types::parse_tool_error_payload(&content).is_some() {
            continue;
        }
        if !shell_tool_result_succeeded(&content) {
            continue;
        }
        let Some(call_id) = msg.tool_call_id.as_deref() else {
            continue;
        };
        match paired_shell_command(messages, call_id) {
            Some(cmd) if edgecrab_tools::command_is_inspect_only(&cmd) => continue,
            Some(_) => {}
            // execute_code without a shell `command` — treat as non-inspect generator.
            None if msg.name.as_deref() == Some("execute_code")
                && paired_execute_code_present(messages, call_id) => {}
            // Unpaired terminal/run_process — no argv fact ⇒ no create evidence.
            None => continue,
        }
        if !extract_document_paths_from_text(&content).is_empty() {
            return true;
        }
    }
    false
}

/// Paired `command` string for a terminal/run_process tool_call_id.
fn paired_shell_command(messages: &[Message], tool_call_id: &str) -> Option<String> {
    for msg in messages {
        if msg.role != Role::Assistant {
            continue;
        }
        let Some(calls) = msg.tool_calls.as_ref() else {
            continue;
        };
        for call in calls {
            if call.id != tool_call_id {
                continue;
            }
            if let Ok(val) = call.parsed_args()
                && let Some(cmd) = val.get("command").and_then(|v| v.as_str())
            {
                return Some(cmd.to_string());
            }
        }
    }
    None
}

fn paired_execute_code_present(messages: &[Message], tool_call_id: &str) -> bool {
    for msg in messages {
        if msg.role != Role::Assistant {
            continue;
        }
        let Some(calls) = msg.tool_calls.as_ref() else {
            continue;
        };
        for call in calls {
            if call.id != tool_call_id {
                continue;
            }
            if call.function.name != "execute_code" {
                continue;
            }
            if let Ok(val) = call.parsed_args()
                && (val.get("code").is_some() || val.get("source").is_some())
            {
                return true;
            }
        }
    }
    false
}

fn shell_tool_result_succeeded(content: &str) -> bool {
    if edgecrab_tools::terminal_result_succeeded(content) {
        return true;
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        if v.get("ok").and_then(|o| o.as_bool()) == Some(false) {
            return false;
        }
        if let Some(code) = v.get("exit_code").and_then(|c| c.as_i64()) {
            return code == 0;
        }
        // execute_code / JSON ok:true without exit_code
        return v.get("ok").and_then(|o| o.as_bool()) == Some(true);
    }
    false
}

/// Mid-loop Done latch: Document class + artifact evidence (007 never-stop).
pub fn document_done_latch_ready(messages: &[Message]) -> bool {
    classify_from_messages(messages) == TaskClass::Document
        && document_artifact_evidence_present(messages)
}

/// Path-shaped document tokens from tool text / JSON (not vibe substrings alone).
fn extract_document_paths_from_text(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(p) = v.get("path").and_then(|p| p.as_str())
            && is_document_path(p)
        {
            paths.push(p.to_string());
        }
        for key in ["stdout", "stderr", "output", "message", "note"] {
            if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                paths.extend(document_path_tokens(s));
            }
        }
    }
    paths.extend(document_path_tokens(text));
    paths.sort();
    paths.dedup();
    paths
}

fn document_path_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| {
                matches!(c, '`' | '"' | '\'' | ',' | ';' | ':' | '(' | ')' | '[' | ']')
            })
        })
        .filter(|t| is_document_path(t))
        .map(|t| t.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::Message;

    #[test]
    fn ha13_beautiful_demo_html_is_visual_ux() {
        let class = TaskClassifier::classify(
            "Improve game demo/games003 make it more beautiful, amazing UX/UI",
            &["demo/games003/index.html".into()],
        );
        assert_eq!(class, TaskClass::VisualUx);
    }

    #[test]
    fn demo_dir_alone_is_not_visual_ux() {
        let class = TaskClassifier::classify(
            "Create a PPTX in ./demo/pptx_raphael",
            &["demo/pptx_raphael".into()],
        );
        assert_ne!(class, TaskClass::VisualUx);
    }

    #[test]
    fn landed_pptx_is_document() {
        let messages = vec![
            Message::user("Create a PPTX in ./demo/pptx_raphael"),
            Message::tool_result(
                "t1",
                "write_file",
                r#"{"ok":true,"path":"demo/pptx_raphael/Raphael_Mansuy_Profile.pptx"}"#,
            ),
        ];
        assert_eq!(classify_from_messages(&messages), TaskClass::Document);
    }

    #[test]
    fn shell_landed_docx_is_document() {
        // 007/P8: create_doc.py → CodeChange path hint, but terminal saves .docx.
        // P9: generator command must be non-inspect and paired to the tool result.
        let messages = vec![
            Message::user("create a word document in ./demo/docx_raphael"),
            Message::assistant("I'll write a script."),
            Message::tool_result(
                "w1",
                "write_file",
                r#"{"ok":true,"path":"demo/docx_raphael/create_doc.py"}"#,
            ),
            Message::assistant_with_tool_calls(
                "",
                vec![edgecrab_types::ToolCall {
                    id: "t1".into(),
                    r#type: "function".into(),
                    function: edgecrab_types::FunctionCall {
                        name: "terminal".into(),
                        arguments: r#"{"command":"python demo/docx_raphael/create_doc.py"}"#.into(),
                    },
                    thought_signature: None,
                }],
            ),
            Message::tool_result(
                "t1",
                "terminal",
                r#"{"ok":true,"exit_code":0,"stdout":"Saved ./demo/docx_raphael/Raphael_Mansuy_Profile.docx (68454 bytes)"}"#,
            ),
        ];
        assert_eq!(classify_from_messages(&messages), TaskClass::Document);
        assert!(document_done_latch_ready(&messages));
    }

    #[test]
    fn ls_style_reference_pptx_is_not_done() {
        // 008/da8b08e4: ls of Downloads style ref must not fire Document Done.
        let messages = vec![
            Message::user(
                r#"Create a powerpoint respecting this style "/Users/me/Downloads/Raphaël MANSUY — A Profile in Data & AI.pptx" in ./demos/raphael"#,
            ),
            Message::assistant_with_tool_calls(
                "",
                vec![edgecrab_types::ToolCall {
                    id: "functions.terminal:2".into(),
                    r#type: "function".into(),
                    function: edgecrab_types::FunctionCall {
                        name: "terminal".into(),
                        arguments: r#"{"command":"ls -lah \"/Users/me/Downloads/Raphaël MANSUY — A Profile in Data & AI.pptx\""}"#.into(),
                    },
                    thought_signature: None,
                }],
            ),
            Message::tool_result(
                "functions.terminal:2",
                "terminal",
                "[terminal_result status=success backend=local cwd=/tmp exit_code=0]\n\
                 -rw-r--r--@ 1 me  staff   170K Jul 16 23:52 /Users/me/Downloads/Raphaël MANSUY — A Profile in Data & AI.pptx\n",
            ),
        ];
        assert!(!document_artifact_evidence_present(&messages));
        assert!(!document_done_latch_ready(&messages));
    }

    #[test]
    fn inspect_only_find_pptx_is_not_done() {
        let messages = vec![
            Message::user("create a deck in ./demos/raphael"),
            Message::assistant_with_tool_calls(
                "",
                vec![edgecrab_types::ToolCall {
                    id: "t1".into(),
                    r#type: "function".into(),
                    function: edgecrab_types::FunctionCall {
                        name: "terminal".into(),
                        arguments: r#"{"command":"find . -name '*.pptx'"}"#.into(),
                    },
                    thought_signature: None,
                }],
            ),
            Message::tool_result(
                "t1",
                "terminal",
                r#"{"ok":true,"exit_code":0,"stdout":"./old/legacy.pptx\n"}"#,
            ),
        ];
        assert!(!document_artifact_evidence_present(&messages));
        assert!(!document_done_latch_ready(&messages));
    }

    #[test]
    fn write_file_pptx_is_done() {
        let messages = vec![
            Message::user("Create a PPTX in ./demos/raphael"),
            Message::tool_result(
                "t1",
                "write_file",
                r#"{"ok":true,"path":"demos/raphael/Profile.pptx"}"#,
            ),
        ];
        assert!(document_done_latch_ready(&messages));
    }

    #[test]
    fn failed_write_file_pptx_is_not_done() {
        let messages = vec![
            Message::user("Create a PPTX in ./demos/raphael"),
            Message::tool_result(
                "t1",
                "write_file",
                r#"{"ok":false,"path":"demos/raphael/Profile.pptx","error":"denied"}"#,
            ),
        ];
        assert!(!document_artifact_evidence_present(&messages));
        assert!(!document_done_latch_ready(&messages));
    }

    #[test]
    fn py_script_alone_is_not_document() {
        let messages = vec![
            Message::user("create a word document in ./demo/docx_raphael"),
            Message::tool_result(
                "w1",
                "write_file",
                r#"{"ok":true,"path":"demo/docx_raphael/create_doc.py"}"#,
            ),
        ];
        assert_ne!(classify_from_messages(&messages), TaskClass::Document);
    }

    #[test]
    fn pptx_path_hint_is_document() {
        let class = TaskClassifier::classify(
            "build the deck",
            &["demo/pptx_raphael/Profile.pptx".into()],
        );
        assert_eq!(class, TaskClass::Document);
    }

    #[test]
    fn ha20d_visual_ux_advisory_mentions_preview_url() {
        let advisory = task_class_advisory(TaskClass::VisualUx, None).expect("advisory");
        assert!(advisory.contains("127.0.0.1") || advisory.contains("browser_navigate"));
        assert!(advisory.contains("security.preview") || advisory.contains("vision"));
    }

    #[test]
    fn document_advisory_does_not_require_localhost() {
        let advisory = task_class_advisory(TaskClass::Document, None).expect("advisory");
        assert!(advisory.contains("document"));
        assert!(!advisory.contains("browser_navigate"));
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
        assert!(!is_verification_tool_for_class(
            "browser_navigate",
            TaskClass::VisualUx
        ));
        assert!(is_verification_tool_for_class(
            "browser_snapshot",
            TaskClass::VisualUx
        ));
        assert!(is_verification_tool_for_class(
            "browser_vision",
            TaskClass::VisualUx
        ));
    }

    #[test]
    fn write_file_is_verification_for_document() {
        assert!(is_verification_tool_for_class(
            "write_file",
            TaskClass::Document
        ));
        assert!(is_verification_tool_for_class("terminal", TaskClass::Document));
        assert!(!is_verification_tool_for_class(
            "browser_navigate",
            TaskClass::Document
        ));
    }

    #[test]
    fn effective_verification_strict_for_visual_ux_html() {
        let harness = HarnessConfig::default();
        let messages = vec![Message::user("polish demo/race_gamey/index.html UX")];
        assert!(effective_verification_strict(&harness, &messages));
    }

    #[test]
    fn demo_pptx_path_not_strict_visual() {
        let harness = HarnessConfig::default();
        let messages = vec![Message::user("Create a PPTX in ./demo/pptx_raphael")];
        // Without verification_strict config and without VisualUx class → not forced strict.
        assert!(!effective_verification_strict(&harness, &messages));
        assert_ne!(classify_from_messages(&messages), TaskClass::VisualUx);
    }

    #[test]
    fn apply_visual_ux_session_preview_enables_loopback() {
        use edgecrab_security::url_safety::{
            PreviewPolicy, preview_policy_test_guard, set_preview_policy,
        };

        let _guard = preview_policy_test_guard();
        set_preview_policy(PreviewPolicy::default());
        let preview = PreviewConfig {
            enabled: false,
            ..PreviewConfig::default()
        };
        assert!(apply_visual_ux_session_preview(
            TaskClass::VisualUx,
            &preview
        ));
        let policy = edgecrab_security::url_safety::current_preview_policy();
        assert!(policy.enabled);
        assert!(policy.allowed_ports.contains(&8000));
        // Document must not enable preview policy.
        set_preview_policy(PreviewPolicy::default());
        assert!(!apply_visual_ux_session_preview(
            TaskClass::Document,
            &preview
        ));
    }

    #[test]
    fn classify_from_messages_uses_path_not_vibe_keywords() {
        let vibe_only = vec![
            Message::user("make the login page more beautiful"),
            Message::assistant("On it."),
        ];
        assert_eq!(classify_from_messages(&vibe_only), TaskClass::General);

        let with_path = vec![
            Message::user("polish demo/games003/index.html UX"),
            Message::assistant("On it."),
        ];
        assert_eq!(classify_from_messages(&with_path), TaskClass::VisualUx);
    }
}
