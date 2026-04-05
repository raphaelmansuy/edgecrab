//! # Execution Backends — Pluggable sandbox environments for shell commands
//!
//! This module provides a trait-based abstraction over multiple backends:
//!
//! ```text
//!   BackendKind (from config)
//!       │
//!       ├── Local   → LocalBackend   (env-var blocklist + persistent shell)
//!       ├── Docker  → DockerBackend  (bollard API, cap-drop, bind-mount)
//!       ├── Ssh     → SshBackend     (openssh, multiplexed channel per session)
//!       ├── Modal   → ModalBackend   (Modal REST API via reqwest)
//!       ├── Daytona → DaytonaBackend (Daytona SDK helper bridge)
//!       └── Singularity → SingularityBackend (Apptainer/Singularity CLI)
//! ```

pub mod daytona;
pub mod docker;
pub mod local;
pub mod modal;
pub mod singularity;
#[cfg(unix)]
pub mod ssh;

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use edgecrab_types::ToolError;

// ─── ExecOutput ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ExecOutput {
    pub fn format(&self, max_stdout: usize, max_stderr: usize) -> String {
        let stdout = truncate_output(&self.stdout, max_stdout);
        let stderr = truncate_output(&self.stderr, max_stderr);

        let mut out = String::new();
        if !stdout.is_empty() {
            out.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("[stderr]\n");
            out.push_str(&stderr);
        }
        if self.exit_code != 0 {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("[exit code: {}]", self.exit_code));
        }
        if out.is_empty() {
            out = format!("[command completed with exit code: {}]", self.exit_code);
        }
        out
    }
}

fn truncate_output(s: &str, max_len: usize) -> String {
    if max_len == 0 || s.len() <= max_len {
        return s.to_string();
    }
    // 40 % head + 60 % tail: beginning carries context, end carries the
    // result or error message.  Mirrors hermes-agent terminal_tool.py.
    let head_len = max_len * 2 / 5;
    let tail_len = max_len - head_len;

    // Snap both boundaries to valid UTF-8 char boundaries.
    let head_end = (0..=head_len)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    let tail_start_raw = s.len().saturating_sub(tail_len);
    let tail_start = (tail_start_raw..=s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(s.len());

    // Guard: regions overlap when max_len is tiny relative to content.
    // Fall back to head-only in that edge case.
    if tail_start <= head_end {
        return format!(
            "{}\n\n... [{} bytes omitted] ...",
            &s[..head_end],
            s.len() - head_end,
        );
    }

    let omitted = tail_start - head_end;
    format!(
        "{}\n\n... [{} bytes omitted] ...\n\n{}",
        &s[..head_end],
        omitted,
        &s[tail_start..],
    )
}

// ─── Secret redaction (mirrors hermes-agent agent/redact.py) ─────────

/// Compiled regex patterns used by [`redact_output`].
struct RedactPatterns {
    /// Known API-key format prefixes: `sk-*`, `ghp_*`, `AIza*`, `AKIA*`, …
    prefix: Regex,
    /// ENV-style assignments: `SOME_SECRET=<value>`.
    env_assign: Regex,
    /// HTTP Authorization headers: `Authorization: Bearer <token>`.
    auth_header: Regex,
    /// Database connection-string passwords: `postgres://user:PASSWORD@host`.
    db_connstr: Regex,
    /// PEM private-key blocks.
    privkey: Regex,
}

fn redact_patterns() -> &'static RedactPatterns {
    static P: std::sync::OnceLock<RedactPatterns> = std::sync::OnceLock::new();
    P.get_or_init(|| RedactPatterns {
        // Well-known structured prefixes.  The `regex` crate does not support
        // look-around, so lookarounds are omitted.  These prefixes are specific
        // enough that false positives are rare; over-masking is safe.
        prefix: Regex::new(
            r"(sk-[A-Za-z0-9_-]{10,}|ghp_[A-Za-z0-9]{10,}|github_pat_[A-Za-z0-9_]{10,}|xox[baprs]-[A-Za-z0-9-]{10,}|AIza[A-Za-z0-9_-]{30,}|pplx-[A-Za-z0-9]{10,}|fal_[A-Za-z0-9_-]{10,}|fc-[A-Za-z0-9]{10,}|AKIA[A-Z0-9]{16}|sk_live_[A-Za-z0-9]{10,}|sk_test_[A-Za-z0-9]{10,}|hf_[A-Za-z0-9]{10,}|r8_[A-Za-z0-9]{10,})"
        ).expect("prefix regex"),
        // KEY=VALUE where the key name looks credential-ish.
        // Backreferences are not supported by the `regex` crate, so quote
        // matching is dropped; the value (incl. surrounding quotes) is masked.
        env_assign: Regex::new(
            r#"(?i)([A-Z_]*(?:API_?KEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL|AUTH)[A-Z_]*)\s*=\s*(\S{4,})"#
        ).expect("env_assign regex"),
        auth_header: Regex::new(
            r"(?i)(Authorization:\s*(?:Bearer|Token)\s+)(\S+)"
        ).expect("auth_header regex"),
        db_connstr: Regex::new(
            r"(?i)((?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp)://[^:]+:)([^@\s]+)(@)"
        ).expect("db_connstr regex"),
        privkey: Regex::new(
            r"-----BEGIN[A-Z ]*PRIVATE KEY-----[\s\S]*?-----END[A-Z ]*PRIVATE KEY-----"
        ).expect("privkey regex"),
    })
}

/// Mask a secret value for LLM-safe output.
///
/// Short tokens (< 18 chars) are fully masked as `[REDACTED]`.
/// Longer tokens preserve `first6…last4` for debuggability.
fn mask_token(token: &str) -> String {
    let len = token.chars().count();
    if len < 18 {
        "[REDACTED]".to_string()
    } else {
        let head: String = token.chars().take(6).collect();
        let tail: String = token
            .chars()
            .rev()
            .take(4)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("{head}...[REDACTED]...{tail}")
    }
}

/// Redact secrets and credentials from tool output before it reaches the LLM.
///
/// Applies patterns for well-known API-key formats, ENV-variable assignments,
/// HTTP Authorization headers, database connection strings, and PEM private-key
/// blocks.  Safe to call on any string — non-matching text passes through
/// unchanged.
///
/// Mirrors hermes-agent `agent/redact.py`.
pub(crate) fn redact_output(s: &str) -> String {
    let p = redact_patterns();

    // 1. Structured API-key prefixes (sk-*, ghp_*, AKIA*, …).
    let s = p
        .prefix
        .replace_all(s, |caps: &regex::Captures| mask_token(&caps[1]))
        .into_owned();

    // 2. ENV-variable assignments where the key name signals a credential.
    let s = p
        .env_assign
        .replace_all(&s, |caps: &regex::Captures| -> String {
            format!("{}=[REDACTED]", &caps[1])
        })
        .into_owned();

    // 3. HTTP Authorization headers (Bearer / Token schemes).
    let s = p
        .auth_header
        .replace_all(&s, |caps: &regex::Captures| -> String {
            format!("{}[REDACTED]", &caps[1])
        })
        .into_owned();

    // 4. Database connection-string passwords.
    let s = p
        .db_connstr
        .replace_all(&s, |caps: &regex::Captures| -> String {
            format!("{}[REDACTED]{}", &caps[1], &caps[3])
        })
        .into_owned();

    // 5. PEM private-key blocks.
    p.privkey
        .replace_all(&s, "[PRIVATE KEY REDACTED]")
        .into_owned()
}

// ─── BackendKind ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    #[default]
    Local,
    Docker,
    Ssh,
    Modal,
    Daytona,
    Singularity,
}

impl BackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Docker => "docker",
            Self::Ssh => "ssh",
            Self::Modal => "modal",
            Self::Daytona => "daytona",
            Self::Singularity => "singularity",
        }
    }
}

impl std::fmt::Display for BackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `BackendKind` parsing is infallible — unknown strings fall back to `Local`.
impl std::str::FromStr for BackendKind {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "docker" => Self::Docker,
            "ssh" => Self::Ssh,
            "modal" => Self::Modal,
            "daytona" => Self::Daytona,
            "singularity" | "apptainer" => Self::Singularity,
            _ => Self::Local,
        })
    }
}

// ─── ExecutionBackend trait ───────────────────────────────────────────

#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    async fn execute(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError>;

    async fn cleanup(&self) -> Result<(), ToolError>;

    fn kind(&self) -> BackendKind;

    /// Returns `true` if the backend is still alive and usable.
    ///
    /// WHY: The global backend cache returns backends by `task_id` without
    /// checking liveness.  For Docker/SSH/Modal backends a container or
    /// connection can die underneath the cache entry; any subsequent
    /// `execute()` call would fail with an opaque error and the dead entry
    /// would live in cache forever.  Checking health on every cache hit lets
    /// `get_or_create_backend()` evict dead backends and build a fresh one.
    ///
    /// Default: `true` — safe for `LocalBackend`, which self-heals via
    /// `ensure_shell()`.  Remote/container backends override with their
    /// `dead` flag so the cache can react immediately when the container/
    /// session is known to have exited.
    async fn is_healthy(&self) -> bool {
        true
    }
}

// ─── Config structs ───────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DockerWorkspaceMount {
    pub host_path: String,
    pub container_path: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DockerBackendConfig {
    pub image: String,
    pub workspace_mount: Option<DockerWorkspaceMount>,
    pub pids_limit: Option<i64>,
    pub memory_bytes: Option<i64>,
    pub nano_cpus: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SshBackendConfig {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub key_path: Option<PathBuf>,
    pub strict_host_checking: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModalTransportMode {
    #[default]
    Auto,
    Direct,
    Managed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModalBackendConfig {
    pub mode: ModalTransportMode,
    pub image: String,
    pub token_id: String,
    pub token_secret: String,
    pub managed_gateway_url: Option<String>,
    pub managed_user_token: Option<String>,
    pub cpu: u32,
    pub memory_mb: u32,
    pub disk_mb: u32,
    pub persistent_filesystem: bool,
}

impl Default for ModalBackendConfig {
    fn default() -> Self {
        Self {
            mode: ModalTransportMode::Auto,
            image: "python:3.11-slim".into(),
            token_id: String::new(),
            token_secret: String::new(),
            managed_gateway_url: None,
            managed_user_token: None,
            cpu: 1,
            memory_mb: 5120,
            disk_mb: 0,
            persistent_filesystem: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaytonaBackendConfig {
    pub image: String,
    pub cpu: u32,
    pub memory_mb: u32,
    pub disk_mb: u32,
    pub persistent_filesystem: bool,
}

impl Default for DaytonaBackendConfig {
    fn default() -> Self {
        Self {
            image: "nikolaik/python-nodejs:python3.11-nodejs20".into(),
            cpu: 1,
            memory_mb: 5120,
            disk_mb: 10240,
            persistent_filesystem: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SingularityBackendConfig {
    pub image: String,
    pub cpu: f32,
    pub memory_mb: u32,
    pub disk_mb: u32,
    pub persistent_filesystem: bool,
}

impl Default for SingularityBackendConfig {
    fn default() -> Self {
        Self {
            image: "docker://nikolaik/python-nodejs:python3.11-nodejs20".into(),
            cpu: 1.0,
            memory_mb: 5120,
            disk_mb: 10240,
            persistent_filesystem: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackendConfig {
    pub kind: BackendKind,
    pub task_id: String,
    pub docker: DockerBackendConfig,
    pub ssh: SshBackendConfig,
    pub modal: ModalBackendConfig,
    pub daytona: DaytonaBackendConfig,
    pub singularity: SingularityBackendConfig,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            kind: BackendKind::Local,
            task_id: String::new(),
            docker: DockerBackendConfig::default(),
            ssh: SshBackendConfig::default(),
            modal: ModalBackendConfig::default(),
            daytona: DaytonaBackendConfig::default(),
            singularity: SingularityBackendConfig::default(),
        }
    }
}

/// Resolve the host-side sandbox root shared by backend implementations.
pub(crate) fn sandbox_root_dir() -> PathBuf {
    std::env::var("TERMINAL_SANDBOX_DIR")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("EDGECRAB_HOME")
                .map(PathBuf::from)
                .map(|home| home.join("sandboxes"))
        })
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".edgecrab")
                .join("sandboxes")
        })
}

/// Convert an arbitrary task identifier into a backend-safe resource name.
pub(crate) fn sanitize_resource_name(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "edgecrab".into()
    } else {
        trimmed.to_string()
    }
}

/// Quote a string for safe inclusion in a POSIX shell single-quoted literal.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

pub(crate) fn ensure_dir(path: &Path, label: &str) -> Result<(), ToolError> {
    std::fs::create_dir_all(path).map_err(|e| ToolError::ExecutionFailed {
        tool: "terminal".into(),
        message: format!("Failed to create {label} at {}: {e}", path.display()),
    })
}

// ─── Sudo transform (B-05) ───────────────────────────────────────────

/// Transform `sudo` commands so the password can be passed via stdin.
///
/// Mirrors hermes-agent's `_transform_sudo_command()`.
///
/// Returns `(transformed_command, sudo_prefix)` where:
/// - `transformed_command` replaces every bare `sudo` word with
///   `sudo -S -p ''`, directing sudo to read its password from stdin
///   silently.
/// - `sudo_prefix` is `Some("password\n")` when a password is available and
///   the command contains `sudo`; `None` otherwise.
///
/// The caller should prepend `sudo_prefix` to any stdin data it writes to the
/// subprocess.  For the edgecrab persistent shell (which does not support
/// per-command stdin injection), the caller wraps the command in a heredoc:
/// `{ printf '%s\n' "$SUDO_PASS"; } | sudo -S -p '' actual-cmd`.
///
/// # Security
/// The password is read from the `SUDO_PASSWORD` environment variable which
/// must be set by the operator — it is NEVER passed via command-line arguments
/// or `/proc`-visible strings.
pub fn transform_sudo(command: &str) -> (String, Option<String>) {
    // Fast path: if there's no 'sudo' word in the command, nothing to do.
    if !command.split_whitespace().any(|w| w == "sudo") {
        return (command.to_string(), None);
    }

    let password = std::env::var("SUDO_PASSWORD").unwrap_or_default();
    if password.is_empty() {
        // No password available — return unchanged so sudo fails gracefully
        // with "sudo: a password is required" rather than a confusing error.
        return (command.to_string(), None);
    }

    // Replace every bare `sudo` word with `sudo -S -p ''`.
    // WHY word-boundary replacement: avoids rewriting `visudo`, `sudoers`, etc.
    // We use a simple split/join rather than regex to avoid a regex dependency
    // here; the pattern is straightforward enough.
    let transformed = replace_sudo_word(command);
    // Trailing \n is required: sudo -S reads exactly one line as the password.
    (transformed, Some(format!("{password}\n")))
}

/// Replace bare `sudo` words (not `visudo`, `sudoers`, etc.) with `sudo -S -p ''`.
fn replace_sudo_word(input: &str) -> String {
    // Walk character-by-character tracking word boundaries so we only replace
    // standalone `sudo` tokens without pulling in a regex crate.
    let needle = "sudo";
    let replacement = "sudo -S -p ''";
    let mut result = String::with_capacity(input.len() + 32);
    let bytes = input.as_bytes();
    let n = bytes.len();
    let mut i = 0;

    while i < n {
        // Check if position i starts a word-boundary `sudo` match
        if i + needle.len() <= n && &bytes[i..i + needle.len()] == needle.as_bytes() {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_';
            let after_pos = i + needle.len();
            let after_ok = after_pos >= n
                || (!bytes[after_pos].is_ascii_alphanumeric() && bytes[after_pos] != b'_');
            if before_ok && after_ok {
                result.push_str(replacement);
                i += needle.len();
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

// ─── Factory ─────────────────────────────────────────────────────────

pub async fn build_backend(config: BackendConfig) -> Result<Box<dyn ExecutionBackend>, ToolError> {
    match config.kind {
        BackendKind::Local => Ok(Box::new(local::LocalBackend::new(config.task_id))),
        BackendKind::Docker => Ok(Box::new(docker::DockerBackend::new(
            config.task_id,
            config.docker,
        ))),
        #[cfg(unix)]
        BackendKind::Ssh => Ok(Box::new(ssh::SshBackend::new(config.task_id, config.ssh))),
        #[cfg(not(unix))]
        BackendKind::Ssh => Err(ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: "SSH backend requires a Unix-like OS (not supported on Windows)".into(),
        }),
        BackendKind::Modal => Ok(Box::new(modal::ModalBackend::new(
            config.task_id,
            config.modal,
        ))),
        BackendKind::Daytona => Ok(Box::new(daytona::DaytonaBackend::new(
            config.task_id,
            config.daytona,
        ))),
        BackendKind::Singularity => Ok(Box::new(singularity::SingularityBackend::new(
            config.task_id,
            config.singularity,
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_roundtrip() {
        for (s, expected) in [
            ("local", BackendKind::Local),
            ("docker", BackendKind::Docker),
            ("ssh", BackendKind::Ssh),
            ("modal", BackendKind::Modal),
            ("daytona", BackendKind::Daytona),
            ("singularity", BackendKind::Singularity),
            ("apptainer", BackendKind::Singularity),
            ("unknown", BackendKind::Local),
        ] {
            assert_eq!(
                s.parse::<BackendKind>().expect("infallible"),
                expected,
                "failed for {s}"
            );
        }
    }

    #[test]
    fn exec_output_format_stdout_only() {
        let o = ExecOutput {
            stdout: "hello\n".into(),
            stderr: String::new(),
            exit_code: 0,
        };
        let f = o.format(usize::MAX, usize::MAX);
        assert_eq!(f, "hello\n");
    }

    #[test]
    fn exec_output_format_with_stderr_and_exit() {
        let o = ExecOutput {
            stdout: "out\n".into(),
            stderr: "err\n".into(),
            exit_code: 1,
        };
        let f = o.format(usize::MAX, usize::MAX);
        assert!(f.contains("out\n"));
        assert!(f.contains("[stderr]\nerr\n"));
        assert!(f.contains("[exit code: 1]"));
    }

    #[test]
    fn exec_output_truncation() {
        let long = "a".repeat(200);
        let o = ExecOutput {
            stdout: long.clone(),
            stderr: String::new(),
            exit_code: 0,
        };
        let f = o.format(100, usize::MAX);
        // New impl uses head+tail split; the omit marker says "bytes omitted".
        assert!(f.contains("omitted"), "expected omit marker; got: {f}");
        assert!(
            f.len() < long.len(),
            "truncated output must be shorter than original"
        );
        // Must preserve both head and tail fragments.
        let before_marker = f.split("omitted").next().unwrap_or("");
        let after_marker = f.split("omitted").nth(1).unwrap_or("");
        assert!(!before_marker.is_empty(), "head fragment must be non-empty");
        assert!(
            !after_marker.trim_start_matches(['.', ' ']).is_empty(),
            "tail fragment must be non-empty"
        );
    }

    #[test]
    fn exec_output_empty_becomes_status() {
        let o = ExecOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        };
        let f = o.format(usize::MAX, usize::MAX);
        assert!(f.contains("exit code: 0"), "got: {f}");
    }

    #[test]
    fn truncate_output_exact_boundary() {
        let s: String = "héllo".into();
        let result = truncate_output(&s, 4);
        assert!(result.starts_with("h"));
    }

    #[test]
    fn backend_config_default_is_local() {
        let cfg = BackendConfig::default();
        assert_eq!(cfg.kind, BackendKind::Local);
    }

    #[test]
    fn sanitize_resource_name_normalizes_symbols() {
        assert_eq!(sanitize_resource_name("Task ID:/A B"), "task-id--a-b");
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("it's ok"), "'it'\\''s ok'");
    }

    #[tokio::test]
    async fn build_backend_local() {
        let cfg = BackendConfig {
            kind: BackendKind::Local,
            task_id: "t1".into(),
            ..Default::default()
        };
        let b = build_backend(cfg).await.expect("build");
        assert_eq!(b.kind(), BackendKind::Local);
    }

    // ─── sudo transform tests ─────────────────────────────────────────
    // These tests mutate the process environment via SUDO_PASSWORD.
    // Rust test runner runs tests in parallel by default; guard mutation behind
    // a module-level mutex so these tests are serialized relative to each other.

    static SUDO_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn transform_sudo_no_sudo_word_passthrough() {
        let _g = SUDO_ENV_LOCK.lock().unwrap();
        let (cmd, pfx) = transform_sudo("ls -la /tmp");
        assert_eq!(cmd, "ls -la /tmp");
        assert!(pfx.is_none());
    }

    #[test]
    fn transform_sudo_no_password_passthrough() {
        let _g = SUDO_ENV_LOCK.lock().unwrap();
        // SUDO_PASSWORD not set → command returned unchanged, no prefix
        unsafe { std::env::remove_var("SUDO_PASSWORD") };
        let (cmd, pfx) = transform_sudo("sudo apt-get update");
        assert_eq!(cmd, "sudo apt-get update");
        assert!(pfx.is_none());
    }

    #[test]
    fn transform_sudo_with_password() {
        let _g = SUDO_ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("SUDO_PASSWORD", "s3cr3t") };
        let (cmd, pfx) = transform_sudo("sudo apt-get update");
        assert!(cmd.contains("sudo -S -p ''"), "got: {cmd}");
        assert!(
            !cmd.contains("sudo apt-get"),
            "original sudo must be replaced"
        );
        assert_eq!(pfx.as_deref(), Some("s3cr3t\n"));
        unsafe { std::env::remove_var("SUDO_PASSWORD") };
    }

    #[test]
    fn transform_sudo_does_not_rewrite_visudo() {
        let _g = SUDO_ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("SUDO_PASSWORD", "pass") };
        let (cmd, _) = transform_sudo("visudo -c");
        assert_eq!(cmd, "visudo -c", "visudo must not be rewritten");
        unsafe { std::env::remove_var("SUDO_PASSWORD") };
    }

    #[test]
    fn transform_sudo_multiple_sudo_tokens() {
        let _g = SUDO_ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("SUDO_PASSWORD", "p") };
        let (cmd, pfx) = transform_sudo("sudo echo hi && sudo whoami");
        assert!(cmd.matches("sudo -S -p ''").count() == 2, "got: {cmd}");
        assert!(pfx.is_some());
        unsafe { std::env::remove_var("SUDO_PASSWORD") };
    }

    #[test]
    fn replace_sudo_word_word_boundaries() {
        let out = replace_sudo_word("sudo foo sudoers pseudo");
        // Only bare `sudo` at word boundary is replaced; `sudoers` and `pseudo` must not be
        assert!(out.starts_with("sudo -S -p ''"), "got: {out}");
        assert!(out.contains("sudoers"), "sudoers must be preserved");
        assert!(out.contains("pseudo"), "pseudo must be preserved");
    }

    // ─── redact_output tests ──────────────────────────────────────────

    #[test]
    fn redact_output_passthrough_clean_text() {
        let text = "hello world, nothing secret here";
        assert_eq!(redact_output(text), text);
    }

    #[test]
    fn redact_output_openai_key() {
        let text = "key=sk-abcdefghij1234567890";
        let out = redact_output(text);
        assert!(
            !out.contains("abcdefghij1234567890"),
            "key must be masked; got: {out}"
        );
        assert!(
            out.contains("REDACTED"),
            "must contain REDACTED marker; got: {out}"
        );
    }

    #[test]
    fn redact_output_github_pat() {
        let text = "export token=ghp_ABCDEFGHIJ1234567890";
        let out = redact_output(text);
        assert!(
            !out.contains("ABCDEFGHIJ1234567890"),
            "PAT must be masked; got: {out}"
        );
    }

    #[test]
    fn redact_output_env_assignment() {
        let text = "MY_API_KEY=supersecretvalue123";
        let out = redact_output(text);
        assert!(
            out.contains("MY_API_KEY="),
            "key name must be preserved; got: {out}"
        );
        assert!(
            !out.contains("supersecretvalue123"),
            "value must be masked; got: {out}"
        );
    }

    #[test]
    fn redact_output_auth_header() {
        let text = "Authorization: Bearer eyJhbGciOiJSUzI1NiJ9.payload.sig";
        let out = redact_output(text);
        assert!(
            out.contains("Authorization:"),
            "header name preserved; got: {out}"
        );
        assert!(
            !out.contains("eyJhbGciOiJSUzI1NiJ9"),
            "bearer token masked; got: {out}"
        );
        assert!(
            out.contains("[REDACTED]"),
            "must contain REDACTED; got: {out}"
        );
    }

    #[test]
    fn redact_output_db_connstr() {
        let text = "postgres://admin:topsecret@db.example.com:5432/mydb";
        let out = redact_output(text);
        assert!(
            !out.contains("topsecret"),
            "password must be masked; got: {out}"
        );
        assert!(out.contains("postgres://"), "scheme preserved; got: {out}");
        assert!(out.contains("@"), "@ delimiter preserved; got: {out}");
    }

    #[test]
    fn redact_output_private_key_block() {
        let text = "key data:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\n-----END RSA PRIVATE KEY-----\nend";
        let out = redact_output(text);
        assert!(
            !out.contains("MIIEowIBAAKCAQEA"),
            "key material must be masked; got: {out}"
        );
        assert!(
            out.contains("[PRIVATE KEY REDACTED]"),
            "must have redact label; got: {out}"
        );
    }

    #[test]
    fn redact_output_short_values_not_redacted() {
        // Values with < 4 chars in the ENV= pattern should pass through
        // (avoids masking things like DEBUG=yes, VERBOSE=1)
        let text = "DEBUG=yes\nFLAG=1";
        let out = redact_output(text);
        // The env_assign pattern requires the NAME to contain a secret-keyword;
        // DEBUG and FLAG don't match, so nothing is redacted.
        assert_eq!(out, text, "non-secret env vars must pass through unchanged");
    }

    #[test]
    fn redact_output_long_key_preserves_head_tail() {
        // Tokens >= 18 chars show first-6…last-4.
        let text = "sk-abcdefghijklmnopqrstuvwxyz0123456789";
        let out = redact_output(text);
        // The prefix regex requires `sk-` + 10+ chars and gives group 1 = full match.
        // Masking: len >= 18 → head(6) + "...[REDACTED]..." + tail(4).
        assert!(
            out.contains("sk-abc"),
            "first 6 chars preserved; got: {out}"
        );
        assert!(out.contains("..."), "ellipsis present; got: {out}");
    }
}
