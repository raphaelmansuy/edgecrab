//! macOS permission diagnostics and guided bootstrap.
//!
//! WHY this exists: macOS privacy/automation failures are attached to the
//! terminal host application, not to the shell command itself. The right UX is
//! an explicit operator workflow that identifies the host app, inspects known
//! permission state through public APIs, opens the correct Settings panes, and
//! gives exact reset commands when TCC is stuck in a cached deny state.

use std::path::{Path, PathBuf};
use std::process::Command;

use edgecrab_tools::macos_permissions::{MacosConsentState, preflight_command_permissions};

const AUTOMATION_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation";
const ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalHostApp {
    pub display_name: String,
    pub bundle_id: Option<String>,
    pub bundle_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionSnapshot {
    pub supported: bool,
    pub host_app: Option<TerminalHostApp>,
    pub notes_automation: MacosConsentState,
    pub system_events_automation: MacosConsentState,
    pub accessibility: MacosConsentState,
}

pub(crate) fn collect_permission_snapshot() -> PermissionSnapshot {
    if !cfg!(target_os = "macos") {
        return PermissionSnapshot {
            supported: false,
            host_app: None,
            notes_automation: MacosConsentState::Unknown,
            system_events_automation: MacosConsentState::Unknown,
            accessibility: MacosConsentState::Unknown,
        };
    }

    let notes =
        preflight_command_permissions("osascript -e 'tell application \"Notes\" to activate'");
    let system_events = preflight_command_permissions(
        "osascript -e 'tell application \"System Events\" to keystroke \" \"'",
    );

    PermissionSnapshot {
        supported: true,
        host_app: detect_terminal_host_app(),
        notes_automation: notes.automation_state.unwrap_or(MacosConsentState::Unknown),
        system_events_automation: system_events
            .automation_state
            .unwrap_or(MacosConsentState::Unknown),
        accessibility: system_events
            .accessibility_state
            .unwrap_or(MacosConsentState::Unknown),
    }
}

pub(crate) fn run_permissions_command(args: &str) -> String {
    if !cfg!(target_os = "macos") {
        return "macOS permission diagnostics are only available on local macOS.".into();
    }

    let action = args.trim().to_ascii_lowercase();
    match action.as_str() {
        "" | "status" => render_permission_report(&collect_permission_snapshot(), None),
        "open" => {
            let notes = open_permission_settings();
            render_permission_report(&collect_permission_snapshot(), Some(notes))
        }
        "bootstrap" => {
            let mut notes = open_permission_settings();
            match Command::new("open").args(["-ga", "Notes"]).status() {
                Ok(status) if status.success() => {
                    notes.push("Opened Notes in the background so Automation preflight can target a running app.".into());
                }
                Ok(status) => {
                    notes.push(format!(
                        "Opening Notes returned exit status {}. You can launch Notes manually before re-running `/permissions status`.",
                        status
                    ));
                }
                Err(err) => {
                    notes.push(format!(
                        "Could not launch Notes automatically: {err}. Open Notes manually before re-running `/permissions status`."
                    ));
                }
            }
            render_permission_report(&collect_permission_snapshot(), Some(notes))
        }
        "reset" | "fix" => render_reset_instructions(&collect_permission_snapshot()),
        "help" => permission_usage(),
        _ => format!(
            "{}\n\nUnrecognised subcommand: `{}`",
            permission_usage(),
            args.trim()
        ),
    }
}

pub(crate) fn permission_usage() -> String {
    "macOS permissions:\n\
     /permissions, /perm          — show terminal-host permission status\n\
     /permissions open            — open Automation and Accessibility settings\n\
     /permissions bootstrap       — open settings and launch Notes for preflight\n\
     /permissions reset           — show exact `tccutil reset` commands\n\
     /permissions help            — show this help"
        .into()
}

fn render_permission_report(snapshot: &PermissionSnapshot, notes: Option<Vec<String>>) -> String {
    if !snapshot.supported {
        return "macOS permission diagnostics are only available on local macOS.".into();
    }

    let mut lines = vec!["macOS terminal-host permission status:".to_string()];
    match &snapshot.host_app {
        Some(host) => {
            lines.push(format!("Host app: {}", host.display_name));
            if let Some(bundle_id) = &host.bundle_id {
                lines.push(format!("Bundle id: {bundle_id}"));
            } else {
                lines.push("Bundle id: unknown".into());
            }
        }
        None => {
            lines.push("Host app: unknown".into());
        }
    }
    lines.push(format!(
        "Notes Automation: {}",
        consent_label(snapshot.notes_automation)
    ));
    lines.push(format!(
        "System Events Automation: {}",
        consent_label(snapshot.system_events_automation)
    ));
    lines.push(format!(
        "Accessibility: {}",
        consent_label(snapshot.accessibility)
    ));

    lines.push(String::new());
    lines.extend(next_step_lines(snapshot));

    if let Some(notes) = notes {
        if !notes.is_empty() {
            lines.push(String::new());
            lines.push("Actions:".into());
            lines.extend(notes.into_iter().map(|note| format!("- {note}")));
        }
    }

    lines.push(String::new());
    lines.push(render_reset_instructions(snapshot));
    lines.join("\n")
}

fn render_reset_instructions(snapshot: &PermissionSnapshot) -> String {
    let Some(host) = &snapshot.host_app else {
        return "Reset commands: host app bundle id could not be determined automatically. If prompts never appear, reset AppleEvents and Accessibility for the app hosting this terminal with `tccutil reset <service> <bundle-id>`.".into();
    };
    let Some(bundle_id) = &host.bundle_id else {
        return format!(
            "Reset commands: host app `{}` was detected but its bundle id could not be resolved. If prompts never appear, find the bundle id in Finder or `mdls` and run `tccutil reset AppleEvents <bundle-id>` and `tccutil reset Accessibility <bundle-id>`.",
            host.display_name
        );
    };

    format!(
        "Reset commands if macOS is stuck in cached-deny mode:\n\
         tccutil reset AppleEvents {bundle_id}\n\
         tccutil reset Accessibility {bundle_id}\n\
         Then bring {} to the front and rerun `/permissions bootstrap`.",
        host.display_name
    )
}

fn next_step_lines(snapshot: &PermissionSnapshot) -> Vec<String> {
    let host_name = snapshot
        .host_app
        .as_ref()
        .map(|host| host.display_name.as_str())
        .unwrap_or("the terminal host app");

    let mut lines = Vec::new();
    let notes_ready = snapshot.notes_automation == MacosConsentState::Granted;
    let ui_ready = snapshot.system_events_automation == MacosConsentState::Granted
        && snapshot.accessibility == MacosConsentState::Granted;

    if notes_ready {
        lines.push("Notes automation is granted. Apple Notes workflows should run without a consent prompt.".into());
    } else {
        lines.push(format!(
            "Grant Automation access for {host_name} to control Notes. Use `/permissions open` to jump to the right pane."
        ));
        if snapshot.notes_automation == MacosConsentState::Unknown {
            lines.push("Notes may not be running, so macOS cannot answer the preflight definitively. `/permissions bootstrap` launches Notes first.".into());
        }
    }

    if ui_ready {
        lines.push("System Events automation and Accessibility are granted. UI scripting should be allowed.".into());
    } else {
        if snapshot.system_events_automation != MacosConsentState::Granted {
            lines.push(format!(
                "Grant Automation access for {host_name} to control System Events if your workflow uses UI scripting."
            ));
        }
        if snapshot.accessibility != MacosConsentState::Granted {
            lines.push(format!(
                "Grant Accessibility access to {host_name} if commands send keystrokes, clicks, or other UI automation."
            ));
        }
    }

    lines
}

fn open_permission_settings() -> Vec<String> {
    let mut notes = Vec::new();
    for (label, url) in [
        ("Automation", AUTOMATION_SETTINGS_URL),
        ("Accessibility", ACCESSIBILITY_SETTINGS_URL),
    ] {
        match Command::new("open").arg(url).status() {
            Ok(status) if status.success() => {
                notes.push(format!("Opened the {label} privacy settings pane."));
            }
            Ok(status) => {
                notes.push(format!(
                    "Opening the {label} privacy settings pane returned exit status {}.",
                    status
                ));
            }
            Err(err) => {
                notes.push(format!(
                    "Could not open the {label} privacy settings pane automatically: {err}"
                ));
            }
        }
    }
    notes
}

fn consent_label(state: MacosConsentState) -> &'static str {
    match state {
        MacosConsentState::Granted => "granted",
        MacosConsentState::Denied => "denied",
        MacosConsentState::WouldPrompt => "not yet granted (macOS would prompt)",
        MacosConsentState::Unknown => "unknown",
    }
}

fn detect_terminal_host_app() -> Option<TerminalHostApp> {
    detect_host_from_process_tree().or_else(detect_host_from_term_program)
}

fn detect_host_from_process_tree() -> Option<TerminalHostApp> {
    let mut pid = std::process::id();
    for _ in 0..16 {
        let (ppid, command) = read_process_info(pid)?;
        if let Some(bundle_path) = bundle_path_from_executable(&command) {
            let display_name = bundle_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Unknown.app")
                .trim_end_matches(".app")
                .to_string();
            return Some(TerminalHostApp {
                display_name,
                bundle_id: bundle_id_for_app(&bundle_path),
                bundle_path: Some(bundle_path),
            });
        }
        if ppid <= 1 || ppid == pid {
            break;
        }
        pid = ppid;
    }
    None
}

fn detect_host_from_term_program() -> Option<TerminalHostApp> {
    let term_program = std::env::var("TERM_PROGRAM").ok()?;
    let normalized = term_program.to_ascii_lowercase();
    let (display_name, bundle_id) = match normalized.as_str() {
        "apple_terminal" => (
            "Terminal".to_string(),
            Some("com.apple.Terminal".to_string()),
        ),
        "iterm.app" => (
            "iTerm".to_string(),
            Some("com.googlecode.iterm2".to_string()),
        ),
        "wezterm" => (
            "WezTerm".to_string(),
            Some("org.wezfurlong.wezterm".to_string()),
        ),
        "warpterminal" => ("Warp".to_string(), Some("dev.warp.Warp-Stable".to_string())),
        "ghostty" => (
            "Ghostty".to_string(),
            Some("com.mitchellh.ghostty".to_string()),
        ),
        "vscode" => ("VS Code-family terminal".to_string(), None),
        _ => (format!("TERM_PROGRAM={term_program}"), None),
    };
    Some(TerminalHostApp {
        display_name,
        bundle_id,
        bundle_path: None,
    })
}

fn read_process_info(pid: u32) -> Option<(u32, String)> {
    let output = Command::new("ps")
        .args(["-o", "pid=,ppid=,comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_ps_line(&String::from_utf8_lossy(&output.stdout))
}

fn parse_ps_line(line: &str) -> Option<(u32, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let _pid = parts.next()?.parse::<u32>().ok()?;
    let ppid = parts.next()?.parse::<u32>().ok()?;
    let command = parts.collect::<Vec<_>>().join(" ");
    if command.is_empty() {
        return None;
    }
    Some((ppid, command))
}

fn bundle_path_from_executable(command: &str) -> Option<PathBuf> {
    let marker = ".app/Contents/";
    let idx = command.find(marker)?;
    Some(PathBuf::from(&command[..idx + 4]))
}

fn bundle_id_for_app(bundle_path: &Path) -> Option<String> {
    let output = Command::new("mdls")
        .args([
            "-name",
            "kMDItemCFBundleIdentifier",
            "-raw",
            &bundle_path.display().to_string(),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() || value == "(null)" {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ps_line_handles_app_paths_with_spaces() {
        let parsed =
            parse_ps_line("75557     1 /Applications/Visual Studio Code.app/Contents/MacOS/Code")
                .expect("parsed");
        assert_eq!(parsed.0, 1);
        assert_eq!(
            parsed.1,
            "/Applications/Visual Studio Code.app/Contents/MacOS/Code"
        );
    }

    #[test]
    fn bundle_path_extraction_keeps_app_root() {
        let path =
            bundle_path_from_executable("/Applications/Visual Studio Code.app/Contents/MacOS/Code")
                .expect("bundle path");
        assert_eq!(path, PathBuf::from("/Applications/Visual Studio Code.app"));
    }

    #[test]
    fn usage_mentions_bootstrap() {
        let text = permission_usage();
        assert!(text.contains("/permissions bootstrap"));
        assert!(text.contains("/permissions reset"));
    }
}
