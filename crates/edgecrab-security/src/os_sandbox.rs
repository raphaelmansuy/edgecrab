//! Optional OS-level sandbox wrappers for local terminal execution (Wave 2 / 018 P0).
//!
//! - macOS: `sandbox-exec` (Seatbelt)
//! - Linux: `bubblewrap` (`bwrap`)
//!
//! `deny_default` switches from soft allow-default profiles to deny-default
//! with explicit allows (trust meter). Default remains soft until operators opt in.

use std::path::Path;
use std::process::Command;

/// Sandbox mode from `security.os_sandbox.mode` in config.yaml.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OsSandboxMode {
    #[default]
    Off,
    Seatbelt,
    Bubblewrap,
}

impl OsSandboxMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "seatbelt" | "sandbox-exec" => Self::Seatbelt,
            "bubblewrap" | "bwrap" => Self::Bubblewrap,
            _ => Self::Off,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Seatbelt => "seatbelt",
            Self::Bubblewrap => "bubblewrap",
        }
    }
}

/// Serialize a wrapped invocation as a single shell line (for `sh -c` backends).
pub fn wrapped_to_shell_line(w: &WrappedCommand) -> String {
    let escape = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let mut out = escape(&w.program);
    for arg in &w.args {
        out.push(' ');
        out.push_str(&escape(arg));
    }
    out
}

/// Result of wrapping a shell command for sandboxed execution.
#[derive(Debug, Clone)]
pub struct WrappedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub passthrough: bool,
    pub warning: Option<String>,
}

/// Wrap `cmd` for sandbox execution. When the sandbox binary is missing,
/// returns the original `sh -c` invocation with a warning.
pub fn wrap_command(
    mode: OsSandboxMode,
    cmd: &str,
    cwd: &Path,
    deny_network: bool,
    deny_default: bool,
) -> WrappedCommand {
    match mode {
        OsSandboxMode::Off => passthrough(cmd, None),
        OsSandboxMode::Seatbelt => wrap_seatbelt(cmd, cwd, deny_network, deny_default),
        OsSandboxMode::Bubblewrap => wrap_bubblewrap(cmd, cwd, deny_network, deny_default),
    }
}

fn passthrough(cmd: &str, warning: Option<String>) -> WrappedCommand {
    WrappedCommand {
        program: "sh".into(),
        args: vec!["-c".into(), cmd.to_string()],
        passthrough: true,
        warning,
    }
}

fn binary_on_path(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn wrap_seatbelt(cmd: &str, cwd: &Path, deny_network: bool, deny_default: bool) -> WrappedCommand {
    if !binary_on_path("sandbox-exec") {
        return passthrough(
            cmd,
            Some("sandbox-exec not found on PATH — running without OS sandbox".into()),
        );
    }

    let cwd_str = cwd.to_string_lossy();
    let profile = if deny_default {
        // Deny-default: only explicit allows (018 P0 trust cliff).
        let mut p = String::from("(version 1)\n(deny default)\n");
        p.push_str("(allow process-exec)\n");
        p.push_str("(allow process-fork)\n");
        p.push_str("(allow signal)\n");
        p.push_str("(allow sysctl-read)\n");
        p.push_str("(allow mach-lookup)\n");
        p.push_str("(allow file-read*)\n");
        p.push_str(&format!(
            "(allow file-write* (subpath \"{cwd_str}\"))\n"
        ));
        if !deny_network {
            p.push_str("(allow network*)\n");
        }
        p
    } else {
        // Soft (legacy): allow-default with optional network deny.
        let mut p = String::from("(version 1)\n(allow default)\n");
        if deny_network {
            p.push_str("(deny network)\n");
        }
        p.push_str(&format!(
            "(allow file-read*)\n(allow file-write* (subpath \"{cwd_str}\"))\n"
        ));
        p
    };

    WrappedCommand {
        program: "sandbox-exec".into(),
        args: vec![
            "-p".into(),
            profile,
            "sh".into(),
            "-c".into(),
            cmd.to_string(),
        ],
        passthrough: false,
        warning: None,
    }
}

fn wrap_bubblewrap(
    cmd: &str,
    cwd: &Path,
    deny_network: bool,
    deny_default: bool,
) -> WrappedCommand {
    if !binary_on_path("bwrap") {
        return passthrough(
            cmd,
            Some("bwrap not found on PATH — running without OS sandbox".into()),
        );
    }

    let cwd_str = cwd.to_string_lossy().to_string();
    let mut args = Vec::new();
    if deny_default {
        // Tighter isolation: unshare user/pid/ipc + bind only essentials + cwd RW.
        args.extend([
            "--unshare-user".into(),
            "--unshare-pid".into(),
            "--unshare-ipc".into(),
            "--die-with-parent".into(),
            "--dev".into(),
            "--proc".into(),
            "/proc".into(),
            "--ro-bind".into(),
            "/usr".into(),
            "/usr".into(),
            "--ro-bind".into(),
            "/bin".into(),
            "/bin".into(),
            "--ro-bind".into(),
            "/lib".into(),
            "/lib".into(),
            "--bind".into(),
            cwd_str.clone(),
            cwd_str.clone(),
            "--chdir".into(),
            cwd_str,
            "sh".into(),
            "-c".into(),
            cmd.to_string(),
        ]);
        // Always unshare net under deny_default unless explicitly allowing network.
        if deny_network {
            args.insert(0, "--unshare-net".into());
        } else {
            // Still isolate other namespaces; network stays shared when deny_network=false.
        }
    } else {
        args.extend([
            "--die-with-parent".into(),
            "--dev".into(),
            "--proc".into(),
            "/proc".into(),
            "--ro-bind".into(),
            "/usr".into(),
            "/usr".into(),
            "--ro-bind".into(),
            "/bin".into(),
            "/bin".into(),
            "--ro-bind".into(),
            "/lib".into(),
            "/lib".into(),
            "--bind".into(),
            cwd_str.clone(),
            cwd_str.clone(),
            "--chdir".into(),
            cwd_str,
            "sh".into(),
            "-c".into(),
            cmd.to_string(),
        ]);
        if deny_network {
            args.insert(0, "--unshare-net".into());
        }
    }

    WrappedCommand {
        program: "bwrap".into(),
        args,
        passthrough: false,
        warning: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parse_variants() {
        assert_eq!(OsSandboxMode::parse("off"), OsSandboxMode::Off);
        assert_eq!(OsSandboxMode::parse("SEATBELT"), OsSandboxMode::Seatbelt);
        assert_eq!(OsSandboxMode::parse("bwrap"), OsSandboxMode::Bubblewrap);
        assert_eq!(OsSandboxMode::parse("unknown"), OsSandboxMode::Off);
    }

    #[test]
    fn wrap_off_passthrough() {
        let w = wrap_command(OsSandboxMode::Off, "echo hi", Path::new("/tmp"), false, false);
        assert!(w.passthrough);
        assert_eq!(w.program, "sh");
        assert_eq!(w.args, vec!["-c", "echo hi"]);
    }

    #[test]
    fn wrap_missing_binary_passthrough_with_warning() {
        let w = wrap_command(
            OsSandboxMode::Seatbelt,
            "echo hi",
            Path::new("/tmp"),
            true,
            false,
        );
        if !binary_on_path("sandbox-exec") {
            assert!(w.passthrough);
            assert!(w.warning.is_some());
        } else {
            assert_eq!(w.program, "sandbox-exec");
        }
    }

    #[test]
    fn seatbelt_deny_default_profile_contains_deny_default() {
        if !binary_on_path("sandbox-exec") {
            return;
        }
        let w = wrap_command(
            OsSandboxMode::Seatbelt,
            "echo hi",
            Path::new("/tmp/project"),
            true,
            true,
        );
        assert!(!w.passthrough);
        let profile = w.args.iter().find(|a| a.contains("(deny default)")).cloned();
        assert!(
            profile.is_some(),
            "deny_default Seatbelt profile must include (deny default)"
        );
        let profile = profile.unwrap();
        assert!(profile.contains("(allow process-exec)"));
        assert!(profile.contains("/tmp/project"));
        assert!(!profile.contains("(allow default)"));
    }

    #[test]
    fn seatbelt_soft_profile_allows_default() {
        if !binary_on_path("sandbox-exec") {
            return;
        }
        let w = wrap_command(
            OsSandboxMode::Seatbelt,
            "echo hi",
            Path::new("/tmp"),
            true,
            false,
        );
        let profile = w
            .args
            .iter()
            .find(|a| a.contains("(version 1)"))
            .cloned()
            .expect("profile");
        assert!(profile.contains("(allow default)"));
    }

    #[test]
    fn bubblewrap_deny_default_adds_unshare_user() {
        if !binary_on_path("bwrap") {
            return;
        }
        let w = wrap_command(
            OsSandboxMode::Bubblewrap,
            "echo hi",
            Path::new("/tmp"),
            true,
            true,
        );
        assert!(w.args.iter().any(|a| a == "--unshare-user"));
        assert!(w.args.iter().any(|a| a == "--unshare-net"));
    }
}
