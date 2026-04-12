//! macOS-specific permission preflight checks.
//!
//! WHY this exists: heuristic command matching is enough to identify commands
//! that may trigger macOS consent prompts, but on macOS we can do better for a
//! subset of permissions by asking the OS directly before execution.
//!
//! Public macOS APIs are uneven:
//! - Accessibility: `AXIsProcessTrusted()` gives a direct yes/no answer.
//! - Apple Events / Automation has APIs, but their preflight behavior is not
//!   reliable enough for a hot-path TUI permission check, so EdgeCrab stays
//!   heuristic-only there.
//! - Full Disk Access has no equivalent public preflight API, so protected-path
//!   access still relies on capability probing and output rewriting.

use std::sync::OnceLock;

use regex::Regex;

use crate::shell_syntax::parse_heredoc_marker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacosConsentState {
    Granted,
    Denied,
    WouldPrompt,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MacosPermissionPreflight {
    pub automation_target: Option<String>,
    pub automation_state: Option<MacosConsentState>,
    pub accessibility_state: Option<MacosConsentState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutomationTarget {
    label: String,
    bundle_id: String,
}

fn applescript_target_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?is)tell\s+application(?:\s+id)?\s+["']([^"']+)["']"#)
            .expect("valid AppleScript target regex")
    })
}

fn accessibility_ui_scripting_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?is)\bosascript\b.*(?:system\s+events|keystroke\b|key\s+code\b|click\s+(?:button|menu|menu item))"#,
        )
        .expect("valid accessibility regex")
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

fn known_bundle_id(app_name: &str) -> Option<&'static str> {
    match app_name.trim().to_ascii_lowercase().as_str() {
        "notes" => Some("com.apple.Notes"),
        "system events" => Some("com.apple.systemevents"),
        "shortcuts" => Some("com.apple.shortcuts"),
        "finder" => Some("com.apple.finder"),
        "terminal" => Some("com.apple.Terminal"),
        "safari" => Some("com.apple.Safari"),
        "mail" => Some("com.apple.mail"),
        _ => None,
    }
}

fn automation_target_from_applescript(script: &str) -> Option<AutomationTarget> {
    let captures = applescript_target_regex().captures(script)?;
    let raw_target = captures.get(1)?.as_str().trim();
    if raw_target.starts_with("com.") {
        return Some(AutomationTarget {
            label: raw_target.into(),
            bundle_id: raw_target.into(),
        });
    }

    let bundle_id = known_bundle_id(raw_target)?;
    Some(AutomationTarget {
        label: raw_target.into(),
        bundle_id: bundle_id.into(),
    })
}

fn join_non_empty(parts: Vec<String>) -> Option<String> {
    let parts = parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn extract_inline_osascript(command: &str) -> Option<String> {
    let tokens = shell_words::split(command).ok()?;
    let mut expressions = Vec::new();
    let mut idx = 0usize;

    while idx < tokens.len() {
        let token = &tokens[idx];
        let command_name = token.rsplit('/').next().unwrap_or(token);
        if command_name != "osascript" {
            idx += 1;
            continue;
        }

        idx += 1;
        while idx < tokens.len() {
            match tokens[idx].as_str() {
                "-e" => {
                    let expr = tokens.get(idx + 1)?;
                    expressions.push(expr.clone());
                    idx += 2;
                }
                "|" | "||" | "&&" | ";" => break,
                _ => idx += 1,
            }
        }
    }

    join_non_empty(expressions)
}

fn extract_osascript_heredoc(command: &str) -> Option<String> {
    let opener = command.lines().next()?.trim();
    if !opener.contains("<<") {
        return None;
    }

    let opens_osascript = opener.contains("osascript");
    let pipes_to_osascript = opener.contains("| osascript") || opener.contains("|osascript");
    if !opens_osascript && !pipes_to_osascript {
        return None;
    }

    let marker = parse_heredoc_marker(opener)?;
    let allows_tab_indented_terminator = opener.contains("<<-");
    let mut body_lines = Vec::new();
    for line in command.lines().skip(1) {
        let terminator = if allows_tab_indented_terminator {
            line.trim_start_matches('\t')
        } else {
            line
        };
        if terminator == marker {
            break;
        }
        body_lines.push(line);
    }
    let body = body_lines.join("\n");
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    Some(body.to_string())
}

fn extract_piped_literal_osascript(command: &str) -> Option<String> {
    let tokens = shell_words::split(command).ok()?;
    let pipe_idx = tokens.iter().position(|token| token == "|")?;
    let rhs = tokens.get(pipe_idx + 1)?;
    if rhs.rsplit('/').next().unwrap_or(rhs) != "osascript" {
        return None;
    }

    let lhs = &tokens[..pipe_idx];
    match lhs {
        [cmd, script] if cmd == "echo" => Some(script.clone()),
        [cmd, format, script] if cmd == "printf" && format.contains("%s") => Some(script.clone()),
        [cmd, format, first, second]
            if cmd == "printf" && format.contains("%s") && format.contains("\\n") =>
        {
            Some(format!("{first}\n{second}"))
        }
        _ => None,
    }
}

fn extract_literal_applescript(command: &str) -> Option<String> {
    extract_inline_osascript(command)
        .or_else(|| extract_osascript_heredoc(command))
        .or_else(|| extract_piped_literal_osascript(command))
}

fn automation_target_from_command(command: &str) -> Option<AutomationTarget> {
    if memo_notes_regex().is_match(command) {
        return Some(AutomationTarget {
            label: "Notes".into(),
            bundle_id: "com.apple.Notes".into(),
        });
    }
    if shortcuts_run_regex().is_match(command) {
        return Some(AutomationTarget {
            label: "Shortcuts".into(),
            bundle_id: "com.apple.shortcuts".into(),
        });
    }

    extract_literal_applescript(command)
        .and_then(|script| automation_target_from_applescript(&script))
        .or_else(|| automation_target_from_applescript(command))
}

fn command_needs_accessibility(command: &str) -> bool {
    accessibility_ui_scripting_regex().is_match(command)
}

// macOS permission preflight is split by safety:
// - Accessibility (`AXIsProcessTrusted`) is safe and cheap, so we query it.
// - AppleEvents / Automation remains heuristic-only because
//   AEDeterminePermissionToAutomateTarget can hang indefinitely on some hosts.
fn macos_preflight(command: &str) -> MacosPermissionPreflight {
    let mut preflight = MacosPermissionPreflight::default();
    if let Some(target) = automation_target_from_command(command) {
        preflight.automation_target = Some(target.label);
    }
    if command_needs_accessibility(command) {
        preflight.accessibility_state = Some(accessibility_consent_state());
    }
    preflight
}

pub fn preflight_command_permissions(command: &str) -> MacosPermissionPreflight {
    macos_preflight(command)
}

pub fn accessibility_consent_status() -> Option<MacosConsentState> {
    #[cfg(target_os = "macos")]
    {
        Some(accessibility_consent_state())
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn accessibility_consent_state() -> MacosConsentState {
    tracing::debug!("checking macOS Accessibility (AXIsProcessTrusted)");
    let trusted = unsafe { AXIsProcessTrusted() } != 0;
    let state = if trusted {
        MacosConsentState::Granted
    } else {
        MacosConsentState::Denied
    };
    tracing::debug!(trusted, ?state, "AXIsProcessTrusted result");
    state
}

#[cfg(target_os = "macos")]
type Boolean = u8;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> Boolean;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_notes_target_from_memo() {
        let preflight = preflight_command_permissions("memo notes -s 'Title'");
        assert_eq!(preflight.automation_target.as_deref(), Some("Notes"));
        assert!(preflight.automation_state.is_some() || !cfg!(target_os = "macos"));
    }

    #[test]
    fn detects_accessibility_ui_scripting() {
        let preflight = preflight_command_permissions(
            "osascript -e 'tell application \"System Events\" to keystroke \"v\"'",
        );
        assert_eq!(
            preflight.automation_target.as_deref(),
            Some("System Events")
        );
        assert!(preflight.accessibility_state.is_some() || !cfg!(target_os = "macos"));
    }

    #[test]
    fn plain_osascript_delay_has_no_probe() {
        let preflight = preflight_command_permissions("osascript -e 'delay 1'");
        assert_eq!(preflight.automation_target, None);
        assert_eq!(preflight.automation_state, None);
        assert_eq!(preflight.accessibility_state, None);
    }

    #[test]
    fn detects_target_from_multiple_inline_e_segments() {
        let preflight = preflight_command_permissions(
            "osascript -e 'tell application \"Notes\"' -e 'activate'",
        );
        assert_eq!(preflight.automation_target.as_deref(), Some("Notes"));
    }

    #[test]
    fn detects_target_from_osascript_heredoc() {
        let preflight = preflight_command_permissions(
            "osascript <<'APPLESCRIPT'\ntell application \"Notes\" to activate\nAPPLESCRIPT",
        );
        assert_eq!(preflight.automation_target.as_deref(), Some("Notes"));
    }

    #[test]
    fn detects_target_from_piped_heredoc() {
        let preflight = preflight_command_permissions(
            "cat <<'APPLESCRIPT' | osascript\ntell application id \"com.apple.Notes\" to activate\nAPPLESCRIPT",
        );
        assert_eq!(
            preflight.automation_target.as_deref(),
            Some("com.apple.Notes")
        );
    }

    #[test]
    fn detects_target_from_printf_pipe() {
        let preflight = preflight_command_permissions(
            "printf '%s' 'tell application \"Notes\" to activate' | osascript",
        );
        assert_eq!(preflight.automation_target.as_deref(), Some("Notes"));
    }

    #[test]
    fn detects_target_from_tab_stripped_heredoc() {
        let preflight = preflight_command_permissions(
            "osascript <<-APPLESCRIPT\n\ttell application \"Notes\" to activate\n\tAPPLESCRIPT",
        );
        assert_eq!(preflight.automation_target.as_deref(), Some("Notes"));
    }
}
