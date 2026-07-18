//! Sole pre-dispatch decision compositor (018 W3 / P1 — DRY / SOLID).
//!
//! Order: visual storm → port shopping → browser nav repeat → verification theater
//! → theater write → spill-blind write → tool-loop guardrail.
//!
//! Call sites must use [`pre_dispatch_decision`] instead of assembling checks inline.
//! Implementation lives here — `turn_dispatch` only re-exports for compatibility.

use edgecrab_types::Message;

use crate::harness_loop_policy::visual_storm_block_result_with_args;
use crate::turn_dispatch::{TurnDispatchTrackersView, guardrail_before_dispatch};

/// Pre-dispatch mediation — returns a synthetic blocked tool result when execution
/// must not proceed.
pub fn pre_dispatch_decision(
    trackers: &TurnDispatchTrackersView<'_>,
    messages: &[Message],
    tool_name: &str,
    args_json: &str,
    session_id: &str,
) -> Option<String> {
    if let Some(blocked) = visual_storm_block_result_with_args(
        trackers.harness_advisory,
        messages,
        tool_name,
        args_json,
    ) {
        return Some(blocked);
    }
    let class = crate::task_class::classify_from_messages(messages);
    let session_ports = if session_id.is_empty() {
        Vec::new()
    } else {
        edgecrab_tools::dev_server::session_http_server_ports(session_id)
    };
    let serve_dir = preview_serve_directory_from_messages(messages);
    if let Some(blocked) = trackers.harness_advisory.maybe_loopback_port_shopping_block(
        tool_name,
        args_json,
        &session_ports,
        &serve_dir,
    ) {
        return Some(blocked);
    }
    if let Some(blocked) = trackers
        .harness_advisory
        .maybe_repeated_browser_nav_block(tool_name)
    {
        return Some(blocked);
    }
    if let Some(blocked) = trackers
        .harness_advisory
        .maybe_verification_theater_block(class, tool_name)
    {
        return Some(blocked);
    }
    if let Some(blocked) = maybe_theater_write_block(messages, class, tool_name, args_json) {
        return Some(blocked);
    }
    if let Some(blocked) =
        maybe_document_gui_thrash_block(messages, class, tool_name, args_json)
    {
        return Some(blocked);
    }
    if edgecrab_tools::detect_spill_without_read(messages)
        && matches!(tool_name, "write_file" | "patch" | "apply_patch")
    {
        return Some(edgecrab_tools::tool_loop_guardrails::guardrail_block_result(
            &edgecrab_tools::tool_loop_guardrails::ToolGuardrailDecision {
                action: edgecrab_tools::tool_loop_guardrails::GuardrailAction::Block,
                code: "spill_blind_write_block",
                message:
                    "Blocked mutation — read the spilled artifact with read_file before writing."
                        .into(),
                tool_name: tool_name.to_string(),
                count: 1,
            },
        ));
    }
    guardrail_before_dispatch(trackers.tool_guardrail, tool_name, args_json)
}

/// After Document artifact exists, halt open/screencapture GUI thrash (007).
fn maybe_document_gui_thrash_block(
    messages: &[Message],
    task_class: crate::task_class::TaskClass,
    tool_name: &str,
    args_json: &str,
) -> Option<String> {
    if !matches!(task_class, crate::task_class::TaskClass::Document)
        && !crate::task_class::document_artifact_evidence_present(messages)
    {
        return None;
    }
    if !crate::task_class::document_artifact_evidence_present(messages) {
        return None;
    }
    if !matches!(tool_name, "terminal" | "run_process") {
        return None;
    }
    let command = edgecrab_tools::dev_server::command_from_tool_args_json(args_json)?;
    let is_capture = edgecrab_tools::command_invokes_screencapture(&command);
    let is_gui_open = command_is_gui_open_verify(&command);
    if !is_capture && !is_gui_open {
        return None;
    }
    let prior = count_gui_verify_attempts(messages);
    // Screencapture always halted once artifact exists; `open` allowed once then Halt.
    if !is_capture && prior < 1 {
        return None;
    }
    Some(
        edgecrab_tools::tool_loop_guardrails::guardrail_block_result(
            &edgecrab_tools::tool_loop_guardrails::ToolGuardrailDecision {
                action: edgecrab_tools::tool_loop_guardrails::GuardrailAction::Halt,
                code: "document_gui_thrash_halt",
                message: "Halted GUI verify theater — document artifact evidence is already \
                 present. Do not open Word/Pages or screencapture; Done uses filesystem proof."
                    .into(),
                tool_name: tool_name.to_string(),
                count: (prior + 1) as u32,
            },
        ),
    )
}

fn command_is_gui_open_verify(command: &str) -> bool {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    if tokens.is_empty() {
        return false;
    }
    let base = tokens[0].rsplit('/').next().unwrap_or(tokens[0]);
    if base != "open" {
        return false;
    }
    // `open path.docx` / `open -a Word file.docx`
    command.to_ascii_lowercase().contains(".docx")
        || command.to_ascii_lowercase().contains(".pptx")
        || command.to_ascii_lowercase().contains(".pdf")
        || tokens.iter().any(|t| *t == "-a" || *t == "-b")
}

fn count_gui_verify_attempts(messages: &[Message]) -> usize {
    let mut n = 0usize;
    for msg in messages {
        if msg.role != edgecrab_types::Role::Assistant {
            continue;
        }
        let Some(calls) = msg.tool_calls.as_ref() else {
            continue;
        };
        for call in calls {
            if !matches!(
                call.function.name.as_str(),
                "terminal" | "run_process"
            ) {
                continue;
            }
            let Ok(args) = call.parsed_args() else {
                continue;
            };
            let Some(cmd) = args.get("command").and_then(|c| c.as_str()) else {
                continue;
            };
            if edgecrab_tools::command_invokes_screencapture(cmd) || command_is_gui_open_verify(cmd)
            {
                n += 1;
            }
        }
    }
    n
}

/// Block verify/report markdown writes after the first on VisualUx tasks (HA-43).
fn maybe_theater_write_block(
    messages: &[Message],
    task_class: crate::task_class::TaskClass,
    tool_name: &str,
    args_json: &str,
) -> Option<String> {
    if !matches!(task_class, crate::task_class::TaskClass::VisualUx) || tool_name != "write_file" {
        return None;
    }
    let path = serde_json::from_str::<serde_json::Value>(args_json)
        .ok()
        .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(str::to_string))?;

    // Structural smoke thrash: second HTML entrypoint without structured perception.
    if path.to_ascii_lowercase().ends_with(".html")
        && count_html_writes(messages) >= 1
        && !has_structured_perception_ok(messages)
    {
        return Some(
            edgecrab_tools::tool_loop_guardrails::guardrail_block_result(
                &edgecrab_tools::tool_loop_guardrails::ToolGuardrailDecision {
                    action: edgecrab_tools::tool_loop_guardrails::GuardrailAction::Block,
                    code: "visual_smoke_html_thrash",
                    message: "Blocked second HTML entrypoint without browser perception — \
                     navigate/snapshot the product page instead of inventing smoke HTML."
                        .into(),
                    tool_name: tool_name.to_string(),
                    count: (count_html_writes(messages) + 1) as u32,
                },
            ),
        );
    }

    if !crate::completion_assessor::is_verify_theater_basename(&path) {
        return None;
    }
    let existing = count_verify_theater_writes(messages);
    if existing < 1 {
        return None;
    }
    Some(
        edgecrab_tools::tool_loop_guardrails::guardrail_block_result(
            &edgecrab_tools::tool_loop_guardrails::ToolGuardrailDecision {
                action: edgecrab_tools::tool_loop_guardrails::GuardrailAction::Block,
                code: "verify_theater_write_cap",
                message: "Blocked second verification markdown — use browser_snapshot instead."
                    .into(),
                tool_name: tool_name.to_string(),
                count: existing as u32 + 1,
            },
        ),
    )
}

fn count_html_writes(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|msg| {
            msg.role == edgecrab_types::Role::Tool
                && msg.name.as_deref() == Some("write_file")
                && edgecrab_types::parse_tool_error_payload(&msg.text_content()).is_none()
                && write_file_path_from_result(&msg.text_content())
                    .is_some_and(|p| p.to_ascii_lowercase().ends_with(".html"))
        })
        .count()
}

fn write_file_path_from_result(content: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(str::to_string))
}

fn has_structured_perception_ok(messages: &[Message]) -> bool {
    messages.iter().any(|msg| {
        msg.role == edgecrab_types::Role::Tool
            && matches!(
                msg.name.as_deref(),
                Some("browser_snapshot" | "browser_vision" | "browser_navigate")
            )
            && edgecrab_tools::structured_browser_nav_succeeded(&msg.text_content())
                .unwrap_or(false)
    })
}

fn preview_serve_directory_from_messages(messages: &[Message]) -> String {
    let mut paths: Vec<String> = Vec::new();
    let mut blob = String::new();
    for msg in messages {
        let text = msg.text_content();
        blob.push_str(&text);
        blob.push('\n');
        if msg.name.as_deref() == Some("write_file")
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(p) = v.get("path").and_then(|p| p.as_str())
        {
            paths.push(p.to_string());
        }
    }
    let path_refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let from_paths = edgecrab_tools::recovery_catalog::infer_preview_serve_directory(&path_refs);
    if from_paths != "." {
        return from_paths;
    }
    // Path tokens from conversation text only (no free-prose demo/ scan).
    edgecrab_tools::recovery_catalog::infer_preview_serve_directory_from_text(&blob)
}

fn count_verify_theater_writes(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|msg| {
            msg.role == edgecrab_types::Role::Tool
                && msg.name.as_deref() == Some("write_file")
                && edgecrab_types::parse_tool_error_payload(&msg.text_content()).is_none()
                && write_file_path_from_result(&msg.text_content())
                    .is_some_and(|p| crate::completion_assessor::is_verify_theater_basename(&p))
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_advisory::HarnessTurnAdvisory;
    use crate::harness_loop_policy::resolve_guardrail_config;
    use crate::turn_dispatch::TurnDispatchTrackers;
    use edgecrab_types::Message;

    #[test]
    fn compositor_blocks_visual_storm_terminal() {
        let harness = crate::config::HarnessConfig::default();
        let mut trackers = TurnDispatchTrackers::with_harness(3, &harness);
        for _ in 0..6 {
            trackers.harness_advisory.record_tool("terminal");
        }
        let view = TurnDispatchTrackersView {
            harness_advisory: &trackers.harness_advisory,
            tool_guardrail: &trackers.tool_guardrail,
        };
        let messages = vec![Message::user("make demo/games003/index.html beautiful UX")];
        let blocked = pre_dispatch_decision(
            &view,
            &messages,
            "terminal",
            r#"{"command":"ls -la"}"#,
            "",
        );
        assert!(blocked.is_some(), "storm terminal must block");
        let _ = resolve_guardrail_config(&harness);
        let _ = HarnessTurnAdvisory::new();
    }

    #[test]
    fn theater_write_cap_blocks_second_verify_md() {
        let messages = vec![edgecrab_types::Message::tool_result(
            "w1",
            "write_file",
            r#"{"path":"VERIFICATION.md"}"#,
        )];
        let block = maybe_theater_write_block(
            &messages,
            crate::task_class::TaskClass::VisualUx,
            "write_file",
            r#"{"path":"FINAL_REPORT.md"}"#,
        );
        assert!(block.is_some());
    }

    #[test]
    fn document_gui_thrash_halts_screencapture_after_artifact() {
        let messages = vec![
            Message::user("create a word document in ./demo/docx_raphael"),
            Message::assistant_with_tool_calls(
                "",
                vec![edgecrab_types::ToolCall {
                    id: "t1".into(),
                    r#type: "function".into(),
                    function: edgecrab_types::FunctionCall {
                        name: "terminal".into(),
                        arguments: r#"{"command":"python create_doc.py"}"#.into(),
                    },
                    thought_signature: None,
                }],
            ),
            Message::tool_result(
                "t1",
                "terminal",
                r#"{"ok":true,"exit_code":0,"stdout":"Saved ./demo/docx_raphael/Profile.docx"}"#,
            ),
        ];
        assert!(crate::task_class::document_artifact_evidence_present(&messages));
        let block = maybe_document_gui_thrash_block(
            &messages,
            crate::task_class::TaskClass::Document,
            "terminal",
            r#"{"command":"screencapture -x ./demo/docx_raphael/word.png","background":false}"#,
        )
        .expect("screencapture must Halt after .docx exists");
        assert!(
            block.contains("document_gui_thrash_halt") || block.contains("filesystem"),
            "got: {block}"
        );
    }

    #[test]
    fn policy_module_owns_dispatch_body_not_facade() {
        let src = include_str!("turn_dispatch_policy.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(
            production.contains("visual_storm_block_result_with_args"),
            "policy must own storm check"
        );
        assert!(
            production.contains("spill_blind_write_block"),
            "policy must own spill-blind write"
        );
        // Must not call the deprecated turn_dispatch wrapper (string split avoids
        // matching this comment: guardrail_before_dispatch_checked_with_session).
        let facade_call = concat!("guardrail_before_dispatch_checked", "_with_session(");
        assert!(
            !production.contains(facade_call),
            "policy must not delegate to turn_dispatch facade"
        );
    }
}
