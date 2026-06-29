//! Smart terminal approval — Hermes `tools/approval.py::_smart_approve` parity.
//!
//! When `approvals.mode=smart`, an auxiliary LLM assesses flagged commands before
//! prompting the user. Returns approve / deny / escalate (fall through to manual).

use std::sync::Arc;

use edgecrab_security::approval::ApprovalMode;
use edgequake_llm::{ChatMessage, CompletionOptions, LLMProvider};

use crate::provider_factory::create_provider_for_model;
use crate::registry::ToolContext;

const SMART_APPROVAL_PROMPT: &str = "\
You are a security reviewer for an AI coding agent. A terminal command was flagged by pattern matching as potentially dangerous.

Command: {command}
Flagged reason: {description}

Assess the ACTUAL risk of this command. Many flagged commands are false positives — for example, `python -c \"print('hello')\"` is flagged as \"script execution via -c flag\" but is completely harmless.

Rules:
- APPROVE if the command is clearly safe (benign script execution, safe file operations, development tools, package installs, git operations, etc.)
- DENY if the command could genuinely damage the system (recursive delete of important paths, overwriting system files, fork bombs, wiping disks, dropping databases, etc.)
- ESCALATE if you're uncertain

Respond with exactly one word: APPROVE, DENY, or ESCALATE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartVerdict {
    Approve,
    Deny,
    Escalate,
}

fn resolve_smart_model(ctx: &ToolContext) -> String {
    if let Some(m) = ctx
        .config
        .approvals_smart_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return m.to_string();
    }
    if let Some(m) = ctx
        .config
        .auxiliary_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return m.to_string();
    }
    ctx.config.active_model.clone()
}

fn provider_for_model(model: &str) -> Result<Arc<dyn LLMProvider>, String> {
    let (provider, model_name) = model
        .split_once('/')
        .ok_or_else(|| format!("smart approval requires provider/model form, got '{model}'"))?;
    create_provider_for_model(provider, model_name)
}

fn parse_verdict(text: &str) -> SmartVerdict {
    let upper = text.trim().to_ascii_uppercase();
    if upper.starts_with("APPROVE") {
        SmartVerdict::Approve
    } else if upper.starts_with("DENY") {
        SmartVerdict::Deny
    } else {
        SmartVerdict::Escalate
    }
}

/// Assess a flagged command via auxiliary LLM. Fail-open to escalate on errors.
pub async fn assess_smart_approval(
    command: &str,
    description: &str,
    ctx: &ToolContext,
) -> SmartVerdict {
    if ctx.config.approval_mode != ApprovalMode::Smart {
        return SmartVerdict::Escalate;
    }

    let model = resolve_smart_model(ctx);
    let provider = match provider_for_model(&model) {
        Ok(p) => p,
        Err(err) => {
            tracing::debug!(error = %err, "smart approval: provider setup failed, escalating");
            return SmartVerdict::Escalate;
        }
    };

    let prompt = SMART_APPROVAL_PROMPT
        .replace("{command}", crate::safe_truncate(command, 2000))
        .replace("{description}", crate::safe_truncate(description, 1000));

    let messages = vec![ChatMessage::user(prompt)];
    let options = CompletionOptions {
        max_tokens: Some(16),
        temperature: Some(0.0),
        ..Default::default()
    };

    let response = match provider
        .chat_with_tools(&messages, &[], None, Some(&options))
        .await
    {
        Ok(r) => r,
        Err(err) => {
            tracing::debug!(error = %err, "smart approval: LLM call failed, escalating");
            return SmartVerdict::Escalate;
        }
    };

    let text = response.content;
    let verdict = parse_verdict(&text);
    tracing::debug!(
        command = %crate::safe_truncate(command, 80),
        verdict = ?verdict,
        "smart approval assessment"
    );
    verdict
}

pub fn format_approvals_status(mode: ApprovalMode, smart_model: Option<&str>) -> String {
    let mode_str = match mode {
        ApprovalMode::Manual => "manual",
        ApprovalMode::Smart => "smart",
        ApprovalMode::Off => "off",
    };
    let mut out = format!("approvals.mode = {mode_str}");
    if mode == ApprovalMode::Smart {
        if let Some(m) = smart_model.filter(|s| !s.is_empty()) {
            out.push_str(&format!("\napprovals.smart_model = {m}"));
        } else {
            out.push_str("\nSmart model: auxiliary.model → active session model");
        }
        out.push_str("\n\nSmart mode auto-approves low-risk flagged commands and blocks genuinely dangerous ones.");
    }
    out.push_str("\n\nSet: /approvals mode manual|smart|off");
    out
}

pub fn parse_approval_mode(token: &str) -> Result<ApprovalMode, String> {
    match token.trim().to_ascii_lowercase().as_str() {
        "manual" => Ok(ApprovalMode::Manual),
        "smart" => Ok(ApprovalMode::Smart),
        "off" | "disable" | "disabled" => Ok(ApprovalMode::Off),
        other => Err(format!("Unknown approvals mode '{other}'. Use: manual, smart, off")),
    }
}

pub fn handle_approvals_slash(
    args: &str,
    current_mode: ApprovalMode,
    smart_model: Option<&str>,
    set_mode: Option<&dyn Fn(ApprovalMode) -> Result<(), String>>,
) -> String {
    let trimmed = args.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("status") {
        return format_approvals_status(current_mode, smart_model);
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.first().map(|t| t.eq_ignore_ascii_case("mode")).unwrap_or(false) {
        let Some(mode_token) = tokens.get(1) else {
            return format!(
                "{}\nUsage: /approvals mode manual|smart|off",
                format_approvals_status(current_mode, smart_model)
            );
        };
        let mode = match parse_approval_mode(mode_token) {
            Ok(m) => m,
            Err(e) => return e,
        };
        if let Some(set) = set_mode {
            match set(mode) {
                Ok(()) => {
                    return format!(
                        "approvals.mode set to '{}'.",
                        match mode {
                            ApprovalMode::Manual => "manual",
                            ApprovalMode::Smart => "smart",
                            ApprovalMode::Off => "off",
                        }
                    );
                }
                Err(e) => return format!("Failed to persist approvals.mode: {e}"),
            }
        }
        return format!(
            "Set approvals.mode: {} in ~/.edgecrab/config.yaml",
            match mode {
                ApprovalMode::Manual => "manual",
                ApprovalMode::Smart => "smart",
                ApprovalMode::Off => "off",
            }
        );
    }

    format!(
        "Unknown subcommand. Usage: /approvals [status|mode manual|smart|off]\n\n{}",
        format_approvals_status(current_mode, smart_model)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_verdict_tokens() {
        assert_eq!(parse_verdict("APPROVE"), SmartVerdict::Approve);
        assert_eq!(parse_verdict("deny"), SmartVerdict::Deny);
        assert_eq!(parse_verdict("maybe"), SmartVerdict::Escalate);
    }

    #[test]
    fn parse_mode_tokens() {
        assert_eq!(
            parse_approval_mode("smart").unwrap(),
            ApprovalMode::Smart
        );
        assert!(parse_approval_mode("bogus").is_err());
    }
}
