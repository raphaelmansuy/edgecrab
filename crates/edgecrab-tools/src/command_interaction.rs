//! Command interaction heuristics for non-interactive shells.
//!
//! WHY this exists: some commands are unsafe to run blindly from the agent
//! because they either require a real TTY (e.g. `vim`, `top`, interactive
//! `memo`) or may stall behind a macOS privacy / automation consent dialog.
//! The correct fix is not "wait longer" — it is to detect the unsupported
//! interaction mode, fail deterministically, and tell the user what action is
//! required.

use std::sync::OnceLock;
use std::time::Duration;

use edgecrab_types::ToolError;
use regex::Regex;

use crate::macos_permissions::{MacosConsentState, preflight_command_permissions};
use crate::tools::backends::{BackendKind, ExecOutput};

const DEFAULT_MACOS_DIALOG_STALL_SECS: u64 = 15;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CommandInteractionAssessment {
    pub tty_reason: Option<&'static str>,
    pub macos_prompt_reason: Option<&'static str>,
    pub macos_privacy_reason: Option<&'static str>,
}

struct CapabilityBlock {
    code: &'static str,
    message: String,
    suggested_action: Option<String>,
}

fn obvious_tty_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:^|[;&|]\s*)(?:vim|vi|nano|less|more|man|top|htop|watch|fzf|tig|lazygit|tmux|screen)\b")
            .expect("valid tty regex")
    })
}

fn memo_interactive_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\bmemo\s+notes\b.*\s-(?:a|e|d|m)\b").expect("valid memo regex")
    })
}

fn applescript_automation_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?is)\bosascript\b.*(?:tell\s+application(?:\s+id)?\s+["']|system\s+events|keystroke|click\s+(?:button|menu|menu item)|activate\b)"#)
            .expect("valid osascript regex")
    })
}

fn shortcuts_run_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\bshortcuts\s+run\b").expect("valid shortcuts regex"))
}

fn memo_notes_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\bmemo\s+notes\b").expect("valid memo notes regex"))
}

fn host_is_local_macos(backend_kind: &BackendKind) -> bool {
    cfg!(target_os = "macos") && matches!(backend_kind, BackendKind::Local)
}

fn command_preview(command: &str) -> String {
    let single_line = command.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::safe_truncate(&single_line, 120).to_string()
}

fn describe_macos_preflight(command: &str) -> Option<CapabilityBlock> {
    let preflight = preflight_command_permissions(command);

    if let Some(state) = preflight.accessibility_state {
        if matches!(
            state,
            MacosConsentState::Denied | MacosConsentState::WouldPrompt
        ) {
            let (code, action) = if state == MacosConsentState::WouldPrompt {
                ("macos_accessibility_required", "would trigger")
            } else {
                ("macos_accessibility_denied", "was denied")
            };
            return Some(CapabilityBlock {
                code,
                message: format!(
                    "macOS Accessibility access for the terminal host {action}. Grant Accessibility access in System Settings -> Privacy & Security -> Accessibility."
                ),
                suggested_action: Some(
                    "Grant Accessibility access to the terminal host application, then rerun the command."
                        .into(),
                ),
            });
        }
    }

    if let Some(state) = preflight.automation_state {
        if matches!(
            state,
            MacosConsentState::Denied | MacosConsentState::WouldPrompt
        ) {
            let target = preflight
                .automation_target
                .as_deref()
                .unwrap_or("the target app");
            let (code, action) = if state == MacosConsentState::WouldPrompt {
                (
                    "macos_automation_required",
                    "would trigger a consent prompt",
                )
            } else {
                ("macos_automation_denied", "was previously denied")
            };
            return Some(CapabilityBlock {
                code,
                message: format!(
                    "macOS Automation consent for {target} {action}. Bring the terminal host application to the front and allow it under System Settings -> Privacy & Security -> Automation."
                ),
                suggested_action: Some(format!(
                    "Bring the terminal host application to the front, allow Automation access for {target}, then rerun the command."
                )),
            });
        }

        if state == MacosConsentState::Unknown {
            let target = preflight
                .automation_target
                .as_deref()
                .unwrap_or("the target app");
            return Some(CapabilityBlock {
                code: "macos_automation_unknown",
                message: format!(
                    "macOS could not determine Automation consent for {target}. This usually means the target app is not running yet, or AppleEvents preflight cannot inspect the command shape safely. Open the target app first, run `/permissions bootstrap` or `/permissions status`, and rerun only after consent shows as granted. If macOS still never shows a prompt, use `/permissions reset` to reset AppleEvents and Accessibility for the terminal host app."
                ),
                suggested_action: Some(format!(
                    "Open {target}, run `/permissions bootstrap` or `/permissions status`, and retry only after Automation consent is granted."
                )),
            });
        }
    }

    if applescript_automation_regex().is_match(command)
        && preflight.automation_state.is_none()
        && preflight.accessibility_state.is_none()
    {
        return Some(CapabilityBlock {
            code: "macos_applescript_target_unresolved",
            message: "EdgeCrab could not statically determine the AppleScript target from this command. This commonly happens when the script is fed to `osascript` through stdin, a heredoc, or another shell layer. To avoid a hidden macOS consent dialog or a flaky stall, EdgeCrab requires an explicit permission bootstrap first. Prefer inline `osascript -e '...'`, or run `/permissions bootstrap` and retry after consent is granted.".into(),
            suggested_action: Some(
                "Prefer an explicit AppleScript target or run `/permissions bootstrap` before retrying."
                    .into(),
            ),
        });
    }

    None
}

fn memo_command_has_explicit_stdin(command: &str) -> bool {
    command.contains("<<")
        || command.contains("<<<")
        || command.contains(" < ")
        || command.contains("\n<")
        || command.contains("| memo")
        || command.contains("|memo")
}

pub(crate) fn assess_command(command: &str) -> CommandInteractionAssessment {
    let mut assessment = CommandInteractionAssessment::default();

    if obvious_tty_regex().is_match(command) {
        assessment.tty_reason = Some("full-screen terminal UI");
    } else if memo_interactive_regex().is_match(command)
        && !memo_command_has_explicit_stdin(command)
    {
        assessment.tty_reason = Some("interactive memo notes flow");
    }

    if applescript_automation_regex().is_match(command) {
        assessment.macos_prompt_reason = Some("AppleScript automation");
    } else if memo_notes_regex().is_match(command) {
        assessment.macos_prompt_reason = Some("Apple Notes automation");
    } else if shortcuts_run_regex().is_match(command) {
        assessment.macos_prompt_reason = Some("Shortcuts automation");
    }

    let normalized = command.to_ascii_lowercase();
    if normalized.contains("/library/messages")
        || normalized.contains("/library/mail")
        || normalized.contains("/library/safari")
        || normalized.contains("/library/calendars")
        || normalized.contains("/library/addressbook")
    {
        assessment.macos_privacy_reason = Some("protected macOS application data");
    }

    assessment
}

pub(crate) fn guard_terminal_command(
    command: &str,
    backend_kind: &BackendKind,
) -> Result<CommandInteractionAssessment, ToolError> {
    let assessment = assess_command(command);
    if let Some(reason) = assessment.tty_reason {
        return Err(
            ToolError::capability_denied(
                "terminal",
                "non_interactive_terminal_required",
                format!(
                    "Command requires an interactive terminal UI ({reason}), but the terminal tool runs non-interactively. Use non-interactive flags, explicit stdin redirection, or a real terminal session instead.\nCommand: `{}`",
                    command_preview(command)
                ),
            )
            .with_suggested_action(
                "Use a non-interactive command form, explicit stdin redirection, or a real terminal session."
                    .to_string(),
            ),
        );
    }
    if !host_is_local_macos(backend_kind) {
        return Ok(CommandInteractionAssessment {
            tty_reason: assessment.tty_reason,
            macos_prompt_reason: None,
            macos_privacy_reason: None,
        });
    }
    if let Some(preflight) = describe_macos_preflight(command) {
        let mut error = ToolError::capability_denied(
            "terminal",
            preflight.code,
            format!(
                "Command requires macOS consent before it can run safely. {}\nEdgeCrab did not start the command to avoid a hidden GUI prompt or flaky timeout.\nCommand: `{}`",
                preflight.message,
                command_preview(command)
            ),
        );
        if let Some(action) = preflight.suggested_action {
            error = error.with_suggested_action(action);
        }
        return Err(error);
    }
    Ok(assessment)
}

pub(crate) fn guard_run_process_command(
    command: &str,
    backend_kind: &BackendKind,
) -> Result<(), ToolError> {
    let assessment = assess_command(command);
    if let Some(reason) = assessment.tty_reason {
        return Err(
            ToolError::capability_denied(
                "run_process",
                "background_interactive_terminal_unsupported",
                format!(
                    "Command requires an interactive terminal UI ({reason}), which is unsafe to background without a PTY. Use a non-interactive form of the command or run it in a real terminal session.\nCommand: `{}`",
                    command_preview(command)
                ),
            )
            .with_suggested_action(
                "Use a non-interactive command form or run it in a real terminal session instead of `run_process`."
                    .to_string(),
            ),
        );
    }
    if host_is_local_macos(backend_kind) {
        if let Some(reason) = assessment.macos_prompt_reason {
            let preflight = describe_macos_preflight(command);
            let extra = preflight
                .as_ref()
                .map(|block| format!("\nPreflight: {}", block.message))
                .unwrap_or_default();
            let mut error = ToolError::capability_denied(
                "run_process",
                "background_macos_consent_unsupported",
                format!(
                    "Command may block on a macOS permission dialog ({reason}). Background execution would hide that prompt and create flaky behavior. Run it in the foreground with `terminal` after granting the required macOS permission.{extra}\nCommand: `{}`",
                    command_preview(command),
                ),
            );
            error = error.with_suggested_action(
                "Grant the required macOS permission, then rerun the command in the foreground with `terminal`."
                    .to_string(),
            );
            return Err(error);
        }
    }
    Ok(())
}

pub(crate) fn macos_prompt_stall_timeout(
    command: &str,
    backend_kind: &BackendKind,
) -> Option<Duration> {
    if !host_is_local_macos(backend_kind) {
        return None;
    }
    let assessment = assess_command(command);
    assessment.macos_prompt_reason?;
    let preflight = preflight_command_permissions(command);
    if matches!(
        preflight.accessibility_state,
        Some(MacosConsentState::Granted)
    ) && preflight.automation_state.is_none()
    {
        return None;
    }
    if matches!(preflight.automation_state, Some(MacosConsentState::Granted)) {
        return None;
    }
    if matches!(
        preflight.accessibility_state,
        Some(MacosConsentState::Denied | MacosConsentState::WouldPrompt)
    ) {
        return None;
    }
    if matches!(
        preflight.automation_state,
        Some(MacosConsentState::Denied | MacosConsentState::WouldPrompt)
    ) {
        return None;
    }

    let secs = std::env::var("EDGECRAB_MACOS_DIALOG_STALL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_MACOS_DIALOG_STALL_SECS);
    Some(Duration::from_secs(secs))
}

fn format_macos_prompt_timeout(command: &str, reason: &str, stall_timeout: Duration) -> ToolError {
    ToolError::capability_denied(
        "terminal",
        "macos_permission_dialog_stall",
        format!(
            "Command likely stalled waiting for a macOS permission dialog ({reason}). EdgeCrab stopped waiting after {}s instead of hanging until the full command timeout.\nWhat to do: bring the terminal host application (for example VS Code, Terminal, or iTerm) to the front, allow the macOS prompt, then rerun the command. Check System Settings -> Privacy & Security -> Automation / Accessibility / Full Disk Access if needed.\nNo automatic retry was attempted to avoid duplicating side effects.\nCommand: `{}`",
            stall_timeout.as_secs(),
            command_preview(command)
        ),
    )
    .with_suggested_action(
        "Bring the terminal host application to the front, resolve the macOS dialog, then rerun the command."
            .to_string(),
    )
}

fn format_macos_prompt_denied(command: &str, reason: &str, output: &str) -> ToolError {
    ToolError::capability_denied(
        "terminal",
        "macos_automation_denied",
        format!(
            "macOS denied the requested permission ({reason}). Grant access to the terminal host application in System Settings -> Privacy & Security, then rerun the command.\nObserved output: `{}`\nCommand: `{}`",
            crate::safe_truncate(output.trim(), 220),
            command_preview(command)
        ),
    )
    .with_suggested_action(
        "Grant the missing Automation or Accessibility permission to the terminal host application, then rerun the command."
            .to_string(),
    )
}

fn format_macos_privacy_denied(command: &str, reason: &str, output: &str) -> ToolError {
    ToolError::capability_denied(
        "terminal",
        "macos_full_disk_access_denied",
        format!(
            "macOS blocked access to protected data ({reason}). Grant Full Disk Access to the terminal host application in System Settings -> Privacy & Security -> Full Disk Access, then rerun the command.\nObserved output: `{}`\nCommand: `{}`",
            crate::safe_truncate(output.trim(), 220),
            command_preview(command)
        ),
    )
    .with_suggested_action(
        "Grant Full Disk Access to the terminal host application, then rerun the command."
            .to_string(),
    )
}

fn output_mentions_macos_automation_denial(output: &str) -> bool {
    let normalized = output.to_ascii_lowercase();
    normalized.contains("not authorized to send apple events")
        || normalized.contains("not authorised to send apple events")
        || normalized.contains("apple events to") && normalized.contains("not authorized")
        || normalized.contains("(-1743)")
        || normalized.contains("assistive access")
        || normalized.contains("accessibility access")
}

fn output_mentions_permission_denial(output: &str) -> bool {
    let normalized = output.to_ascii_lowercase();
    normalized.contains("operation not permitted")
        || normalized.contains("permission denied")
        || normalized.contains("not authorized")
}

pub(crate) fn rewrite_terminal_exec_result(
    command: &str,
    backend_kind: &BackendKind,
    requested_timeout: Duration,
    exec_output: &ExecOutput,
) -> Result<(), ToolError> {
    if !host_is_local_macos(backend_kind) {
        return Ok(());
    }

    let assessment = assess_command(command);
    let combined_output = if exec_output.stderr.trim().is_empty() {
        exec_output.stdout.trim().to_string()
    } else if exec_output.stdout.trim().is_empty() {
        exec_output.stderr.trim().to_string()
    } else {
        format!(
            "{}\n{}",
            exec_output.stdout.trim(),
            exec_output.stderr.trim()
        )
    };

    if let Some(reason) = assessment.macos_prompt_reason {
        if output_mentions_macos_automation_denial(&combined_output) {
            return Err(format_macos_prompt_denied(
                command,
                reason,
                &combined_output,
            ));
        }

        if let Some(stall_timeout) = macos_prompt_stall_timeout(command, backend_kind) {
            if requested_timeout > stall_timeout
                && exec_output.exit_code == 124
                && exec_output.stdout.trim().is_empty()
                && exec_output.stderr.trim().is_empty()
            {
                return Err(format_macos_prompt_timeout(command, reason, stall_timeout));
            }
        }
    }

    if let Some(reason) = assessment.macos_privacy_reason {
        if output_mentions_permission_denial(&combined_output) {
            return Err(format_macos_privacy_denied(
                command,
                reason,
                &combined_output,
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_obvious_tty_commands() {
        let assessment = assess_command("vim Cargo.toml");
        assert_eq!(assessment.tty_reason, Some("full-screen terminal UI"));
    }

    #[test]
    fn detects_interactive_memo_without_blocking_redirected_input() {
        let blocked = assess_command("memo notes -a");
        assert_eq!(blocked.tty_reason, Some("interactive memo notes flow"));

        let allowed = assess_command("memo notes -a \"Title\" <<'EOF'\nbody\nEOF");
        assert_eq!(allowed.tty_reason, None);
        assert_eq!(allowed.macos_prompt_reason, Some("Apple Notes automation"));
    }

    #[test]
    fn detects_applescript_automation_but_not_plain_delay() {
        let automation = assess_command("osascript -e 'tell application \"Notes\" to activate'");
        assert_eq!(
            automation.macos_prompt_reason,
            Some("AppleScript automation")
        );

        let by_bundle_id =
            assess_command("osascript -e 'tell application id \"com.apple.Notes\" to activate'");
        assert_eq!(
            by_bundle_id.macos_prompt_reason,
            Some("AppleScript automation")
        );

        let inert = assess_command("osascript -e 'delay 1'");
        assert_eq!(inert.macos_prompt_reason, None);
    }

    #[test]
    fn terminal_guard_blocks_tty_commands() {
        let err = guard_terminal_command("top", &BackendKind::Local).expect_err("tty should fail");
        let ToolError::CapabilityDenied { message, code, .. } = err else {
            panic!("expected capability denied");
        };
        assert_eq!(code, "non_interactive_terminal_required");
        assert!(message.contains("interactive terminal UI"));
    }

    #[test]
    fn run_process_guard_blocks_macos_prompt_commands() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let err = guard_run_process_command("memo notes -s \"Title\"", &BackendKind::Local)
            .expect_err("macos automation should fail");
        let ToolError::CapabilityDenied { message, code, .. } = err else {
            panic!("expected capability denied");
        };
        assert_eq!(code, "background_macos_consent_unsupported");
        assert!(message.contains("macOS permission dialog"));
    }

    #[test]
    fn terminal_guard_blocks_unresolved_applescript_automation() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let command = "osascript -e 'tell application id \"com.example.DoesNotExist\" to activate'";
        let err = guard_terminal_command(command, &BackendKind::Local)
            .expect_err("unresolved automation consent should fail fast");
        let ToolError::CapabilityDenied { message, code, .. } = err else {
            panic!("expected capability denied");
        };
        assert_eq!(code, "macos_automation_unknown");
        assert!(message.contains("could not determine Automation consent"));
        assert!(message.contains("/permissions bootstrap"));
    }

    #[test]
    fn rewrites_explicit_macos_denials() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let out = ExecOutput {
            stdout: String::new(),
            stderr: "execution error: Not authorized to send Apple events to Notes. (-1743)".into(),
            exit_code: 1,
        };
        let err = rewrite_terminal_exec_result(
            "osascript -e 'tell application \"Notes\" to activate'",
            &BackendKind::Local,
            Duration::from_secs(120),
            &out,
        )
        .expect_err("should rewrite");
        let ToolError::CapabilityDenied { message, code, .. } = err else {
            panic!("expected capability denied");
        };
        assert_eq!(code, "macos_automation_denied");
        assert!(message.contains("macOS denied"));
        assert!(message.contains("AppleScript automation"));
    }

    #[test]
    fn granted_macos_preflight_disables_stall_timeout() {
        if !cfg!(target_os = "macos") {
            return;
        }

        let timeout = macos_prompt_stall_timeout(
            "osascript -e 'tell application \"Notes\" to activate'",
            &BackendKind::Local,
        );
        let preflight =
            preflight_command_permissions("osascript -e 'tell application \"Notes\" to activate'");
        if matches!(preflight.automation_state, Some(MacosConsentState::Granted)) {
            assert!(timeout.is_none());
        }
    }
}
