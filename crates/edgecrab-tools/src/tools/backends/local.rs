//! # Local execution backend
//!
//! Runs commands directly on the host machine.
//!
//! ## Features implemented (gap/backend/GAP-001)
//!
//! ### B-03: Env-var blocklist
//! Strips all Edgecrab/provider API keys and secrets from the subprocess
//! environment before `sh -c` is called. This prevents the agent from
//! discovering credentials via `printenv` or `env`.
//!
//! The blocklist is a compile-time static list (HARDCODED_BLOCKLIST) merged
//! with any vars matching common provider naming conventions.
//!
//! ### B-02: Persistent shell
//! Optionally keeps a long-lived `sh -s` (or `bash`) process alive across
//! `execute()` calls. Uses an in-process sentinel protocol over stdin/stdout
//! pipes — no temp-file polling, no filesystem I/O:
//!
//! ```text
//!   stdin ──→ printf '__FENCE_START_<uuid>__\n' ; {cmd} ; printf '__FENCE_END_%d\n' $?
//!   stdout ←── {cmd output lines}
//!            ←── __FENCE_END_0   (exit code embedded in sentinel)
//!   stderr ←── {separate stderr pipe, drained to ring buffer}
//! ```
//!
//! ### Interrupt handling
//! `CancellationToken` is checked after each read. When cancelled, the shell
//! subprocess receives SIGTERM (then SIGKILL after 2 s) and `execute()` returns
//! exit code 130.

use std::collections::HashSet;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use edgecrab_types::ToolError;

use super::{BackendKind, ExecOutput, ExecutionBackend};

// ─── Env-var blocklist (B-03) ─────────────────────────────────────────

/// Hard-coded set of env-var names that must NEVER reach a subprocess.
///
/// Derived from edgecrab's own provider registry pattern + hermes-agent's
/// tools/environments/local.py blocklist. Rust lets us make this compile-time.
///
/// WHY a static slice: ~60 names fit in a BST/hash built once at process start
/// (via OnceLock). New providers → add here + get compile-time coverage.
const HARDCODED_BLOCKLIST: &[&str] = &[
    // OpenAI / compatible
    "OPENAI_API_KEY",
    "OPENAI_API_BASE",
    "OPENAI_BASE_URL",
    "OPENAI_ORG_ID",
    "OPENAI_ORGANIZATION",
    // Anthropic
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    // OpenRouter
    "OPENROUTER_API_KEY",
    // Google / Gemini
    "GOOGLE_API_KEY",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "VERTEX_PROJECT",
    "VERTEX_LOCATION",
    // DeepSeek
    "DEEPSEEK_API_KEY",
    // Mistral
    "MISTRAL_API_KEY",
    // Groq
    "GROQ_API_KEY",
    // Together AI
    "TOGETHER_API_KEY",
    // Perplexity
    "PERPLEXITY_API_KEY",
    // Cohere
    "COHERE_API_KEY",
    // Fireworks AI
    "FIREWORKS_API_KEY",
    // xAI (Grok)
    "XAI_API_KEY",
    // Parallel AI
    "PARALLEL_API_KEY",
    // GitHub Copilot
    "GITHUB_TOKEN",
    "GITHUB_COPILOT_TOKEN",
    // Helicone (observability proxy)
    "HELICONE_API_KEY",
    // Modal
    "MODAL_TOKEN_ID",
    "MODAL_TOKEN_SECRET",
    // Daytona
    "DAYTONA_API_KEY",
    // Firecrawl
    "FIRECRAWL_API_KEY",
    "FIRECRAWL_API_URL",
    // Edgecrab-specific
    "EDGECRAB_API_KEY",
    // Messaging platform tokens
    "TELEGRAM_BOT_TOKEN",
    "DISCORD_BOT_TOKEN",
    "SLACK_BOT_TOKEN",
    "SLACK_APP_TOKEN",
    "WHATSAPP_API_KEY",
    "SIGNAL_API_KEY",
    "MATRIX_ACCESS_TOKEN",
    "MATTERMOST_TOKEN",
    "DINGTALK_APP_SECRET",
    // Database / cloud
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AZURE_OPENAI_API_KEY",
    "AZURE_OPENAI_ENDPOINT",
    "GCP_SA_KEY",
    // Generic patterns checked dynamically
    // (vars ending in _API_KEY, _SECRET, _TOKEN are stripped too)
];

/// Prefix suffixes that indicate a secret regardless of var name.
const SECRET_SUFFIXES: &[&str] = &["_API_KEY", "_SECRET", "_TOKEN", "_PASSWORD", "_PASSWD"];

/// Build the complete blocklist as a `HashSet` (built once per process).
fn build_blocklist() -> HashSet<String> {
    let mut set: HashSet<String> = HARDCODED_BLOCKLIST.iter().map(|s| s.to_string()).collect();

    // Also block anything in the current env that looks like a secret
    for (k, _) in env::vars() {
        let upper = k.to_uppercase();
        if SECRET_SUFFIXES.iter().any(|s| upper.ends_with(s)) {
            set.insert(k);
        }
    }

    set
}

/// Returns a static reference to the blocklist, built once.
fn blocklist() -> &'static HashSet<String> {
    static BL: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
    BL.get_or_init(build_blocklist)
}

/// A minimal `PATH` injected when the current process PATH is absent or does
/// not contain `/usr/bin`.
///
/// WHY: cron jobs, systemd units, and Docker entrypoints often inherit an
/// empty or stripped PATH from the process supervisor.  Without `/usr/bin` in
/// PATH even basic tools like `ls`, `cat`, `git` are not found.  This mirrors
/// hermes-agent's `_SANE_PATH` constant in `tools/environments/local.py`.
const SANE_PATH: &str = "/opt/homebrew/bin:/opt/homebrew/sbin\
    :/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Construct a filtered environment: current env minus the blocklist.
///
/// Returns (key, value) pairs safe to pass to `Command::envs()`.
pub(crate) fn safe_env() -> impl Iterator<Item = (String, String)> {
    let bl = blocklist();
    env::vars().filter(move |(k, _)| !bl.contains(k) || is_env_passthrough(k))
}

// ─── Env-var passthrough registry (B-03b) ────────────────────────────

/// Session-scoped set of env-var names that must pass through the blocklist.
///
/// Two sources populate this set:
/// 1. **Skill declarations** — when a skill is loaded and its frontmatter
///    lists `required_environment_variables`, call `register_env_passthrough()`
///    so those vars survive `safe_env()` stripping.
/// 2. **Force-prefix** — if the operator sets `_EDGECRAB_FORCE_<VAR>=1`
///    (or any non-empty value), `<VAR>` is allowed through unconditionally.
///    Mirrors hermes-agent's `_HERMES_FORCE_<VAR>` mechanism.
///
/// Config-based passthrough (`terminal.env_passthrough` in `config.yaml`)
/// is handled by the caller: pass the list to `register_env_passthrough()`
/// once at startup.
fn passthrough_registry() -> &'static std::sync::RwLock<HashSet<String>> {
    static REG: std::sync::OnceLock<std::sync::RwLock<HashSet<String>>> =
        std::sync::OnceLock::new();
    REG.get_or_init(|| std::sync::RwLock::new(HashSet::new()))
}

/// Register env-var names as allowed to pass through the security blocklist.
///
/// Idempotent: registering the same name twice is harmless.
pub fn register_env_passthrough<I, S>(names: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut guard = passthrough_registry()
        .write()
        .expect("passthrough registry write lock");
    for name in names {
        guard.insert(name.as_ref().to_string());
    }
}

/// Clear the session-scoped passthrough registry.
///
/// Call on session reset to revoke env-var grants from previously loaded skills.
pub fn clear_env_passthrough() {
    passthrough_registry()
        .write()
        .expect("passthrough registry write lock")
        .clear();
}

/// Returns `true` if `key` is allowed to pass through the env-var blocklist.
///
/// Two checks (first match wins):
///  1. `_EDGECRAB_FORCE_<key>` is set as a non-empty env var.
///  2. `key` is in the session-scoped passthrough registry.
fn is_env_passthrough(key: &str) -> bool {
    // Force-prefix mechanism — operator opt-in per variable
    let force_key = format!("_EDGECRAB_FORCE_{key}");
    if env::var(&force_key).is_ok_and(|v| !v.is_empty()) {
        return true;
    }
    passthrough_registry()
        .read()
        .expect("passthrough registry read lock")
        .contains(key)
}

/// Return the `PATH` that should be set on a subprocess.
///
/// If the current process has a non-empty `PATH` that already contains
/// `/usr/bin` we return it unchanged.  Otherwise we fall back to
/// `SANE_PATH` so that common tools are always discoverable.
fn subprocess_path() -> String {
    let path = env::var("PATH").unwrap_or_default();
    if !path.is_empty() && path.contains("/usr/bin") {
        path
    } else {
        SANE_PATH.to_string()
    }
}

// ─── Sentinel protocol (B-02) ─────────────────────────────────────────

const FENCE_END_PREFIX: &str = "__EDGECRAB_FENCE_END_";
const FENCE_END_SUFFIX: &str = "__";

fn fence_end_sentinel(fence_id: &str, exit_code: &str) -> String {
    format!("{FENCE_END_PREFIX}{fence_id}_{exit_code}{FENCE_END_SUFFIX}")
}

fn parse_fence_end(line: &str, fence_id: &str) -> Option<i32> {
    let prefix = format!("{FENCE_END_PREFIX}{fence_id}_");
    if let Some(rest) = line.strip_prefix(&prefix) {
        let code_str = rest.strip_suffix(FENCE_END_SUFFIX).unwrap_or(rest);
        return code_str.trim().parse().ok();
    }
    None
}

// ─── PersistentShell ─────────────────────────────────────────────────

/// A long-lived `sh -s` process shared across `execute()` calls.
///
/// WHY native async over file-based IPC (hermes-agent uses temp files):
/// Rust's `tokio::process` gives `ChildStdin` as `AsyncWrite` and
/// `ChildStdout` as `AsyncRead`. We can drive the fence protocol entirely
/// through in-process buffers — zero filesystem I/O, zero polling.
struct PersistentShell {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    dead: Arc<AtomicBool>,
}

impl PersistentShell {
    async fn spawn(task_id: &str) -> Result<Self, ToolError> {
        // WHY bash: POSIX sh treats `exit` as a special built-in that cannot
        // be overridden by shell functions. Bash (without --posix) respects
        // function overrides for special built-ins, allowing us to redefine
        // `exit` so it calls `return` instead — keeping the persistent shell alive
        // while still reporting the correct exit code via `$?`.
        //
        // Fallback: if `bash` is not in PATH we fall back to `sh`, which will
        // correctly handle oneshot execution (persistent shell dies on `exit`
        // but `LocalBackend::execute()` will transparently restart it).
        let shell_exe = which::which("bash").unwrap_or_else(|_| std::path::PathBuf::from("sh"));

        let mut cmd = Command::new(&shell_exe);
        // `--norc --noprofile` prevents startup scripts from printing to stdout
        // and interfering with our sentinel protocol.
        if shell_exe.ends_with("bash") {
            cmd.arg("--norc").arg("--noprofile");
        }
        cmd.arg("-s")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .env_clear()
            .envs(safe_env())
            .env("TERM", "dumb")
            .env("LC_ALL", "C.UTF-8")
            .env("PATH", subprocess_path())
            .env("EDGECRAB_TASK_ID", task_id)
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("Failed to spawn persistent shell: {e}"),
        })?;

        let mut stdin = child.stdin.take().expect("stdin is piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));

        // Override the `exit` built-in so commands that call `exit N` don't
        // kill the persistent shell process. Instead `exit` becomes `return`,
        // which exits the calling function / compound command with code N while
        // leaving the shell process alive.
        // WHY: hermes-agent uses the exact same pattern in its PersistentShellMixin.
        let init_script = "exit() { return \"${1:-0}\"; }\n";
        stdin
            .write_all(init_script.as_bytes())
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Shell init write failed: {e}"),
            })?;
        stdin
            .flush()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Shell init flush failed: {e}"),
            })?;

        Ok(Self {
            child,
            stdin,
            stdout,
            dead: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Execute one command inside the persistent shell.
    ///
    /// Protocol (single-step):
    ///   1. Write: `{cmd}\nprintf '{sentinel}_%d\n' $?\n`
    ///   2. Read lines until sentinel found → extract exit code
    async fn run(
        &mut self,
        command: &str,
        fence_id: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        if self.dead.load(Ordering::Relaxed) {
            return Err(ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: "Persistent shell is dead; restart required".into(),
            });
        }

        // Remove literal newlines from command so it's a single line
        let safe_cmd = command.replace('\n', " ; ");
        let sentinel = fence_end_sentinel(fence_id, "%d");

        // WHY no subshell wrapper: we define exit() as a function in spawn() so
        // `exit N` calls `return N` (exits the function frame), NOT the shell.
        // This lets environment changes (`export`, `cd`) persist across calls
        // — the same semantics as hermes-agent's PersistentShellMixin.
        //
        // WHY `2>&1`: stderr is merged into stdout so callers see all output.
        // We cannot use a separate stderr pipe with a persistent shell because
        // we have no way to synchronize which stderr bytes belong to which call.
        //
        // WHY `\n` before sentinel: if the command output has no trailing
        // newline, `BufReader::read_line()` would merge it with the sentinel
        // into a single line that would never match, causing the reader to block
        // until the timeout fires. The extra leading \n is harmless (produces
        // at most an empty trailing element in out_lines).
        let script = format!("{{ {safe_cmd}; }} 2>&1\nprintf '\\n{sentinel}\\n' $?\n");

        // Send script
        self.stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Shell stdin write failed: {e}"),
            })?;
        self.stdin
            .flush()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "terminal".into(),
                message: format!("Shell stdin flush failed: {e}"),
            })?;

        // Collect output until sentinel
        let deadline = tokio::time::Instant::now() + timeout;
        let stall_deadline =
            crate::command_interaction::macos_prompt_stall_timeout(command, &BackendKind::Local)
                .map(|duration| tokio::time::Instant::now() + duration);
        let mut out_lines: Vec<String> = Vec::new();
        let mut exit_code: Option<i32> = None;
        let mut buf = String::new();
        let mut saw_nonempty_output = false;

        loop {
            if cancel.is_cancelled() {
                self.kill().await;
                return Ok(ExecOutput {
                    stdout: out_lines.join("\n"),
                    stderr: String::new(),
                    exit_code: 130,
                });
            }

            if !saw_nonempty_output
                && stall_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
            {
                self.kill().await;
                return Ok(ExecOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 124,
                });
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                self.kill().await;
                return Ok(ExecOutput {
                    stdout: out_lines.join("\n"),
                    stderr: String::new(),
                    exit_code: 124,
                });
            }

            let read_fut = self.stdout.read_line(&mut buf);
            tokio::select! {
                res = tokio::time::timeout(remaining.min(Duration::from_millis(500)), read_fut) => {
                    match res {
                        Ok(Ok(0)) => {
                            // EOF — shell exited
                            self.dead.store(true, Ordering::Relaxed);
                            break;
                        }
                        Ok(Ok(_)) => {
                            let line = buf.trim_end_matches('\n').to_string();
                            buf.clear();
                            if let Some(code) = parse_fence_end(&line, fence_id) {
                                exit_code = Some(code);
                                break;
                            } else {
                                if !line.is_empty() {
                                    saw_nonempty_output = true;
                                }
                                out_lines.push(line);
                            }
                        }
                        Ok(Err(e)) => {
                            self.dead.store(true, Ordering::Relaxed);
                            return Err(ToolError::ExecutionFailed {
                                tool: "terminal".into(),
                                message: format!("Shell read error: {e}"),
                            });
                        }
                        Err(_) => {
                            // Inner timeout — check outer and loop
                            continue;
                        }
                    }
                }
                _ = cancel.cancelled() => {
                    self.kill().await;
                    return Ok(ExecOutput {
                        stdout: out_lines.join("\n"),
                        stderr: String::new(),
                        exit_code: 130,
                    });
                }
            }
        }

        Ok(ExecOutput {
            stdout: out_lines.join("\n"),
            stderr: String::new(),
            exit_code: exit_code.unwrap_or(0),
        })
    }

    async fn kill(&mut self) {
        self.dead.store(true, Ordering::Relaxed);
        let _ = self.child.kill().await;
    }
}

// ─── LocalBackend ─────────────────────────────────────────────────────

/// Local (host) execution backend.
///
/// Uses a persistent shell by default for stateful workdir/env persistence.
/// Falls back to one-shot `sh -c` if the persistent shell is unavailable.
pub struct LocalBackend {
    task_id: String,
    shell: TokioMutex<Option<PersistentShell>>,
}

impl LocalBackend {
    pub fn new(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            shell: TokioMutex::new(None),
        }
    }

    /// Ensure the persistent shell is alive. Create if needed.
    async fn ensure_shell(&self) -> Result<(), ToolError> {
        let mut guard = self.shell.lock().await;
        if guard
            .as_ref()
            .map(|s| !s.dead.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            return Ok(());
        }
        *guard = Some(PersistentShell::spawn(&self.task_id).await?);
        Ok(())
    }

    /// One-shot execution (fallback when persistent shell unavailable).
    async fn oneshot(
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        let cwd_path = std::path::Path::new(cwd);
        let stall_deadline =
            crate::command_interaction::macos_prompt_stall_timeout(command, &BackendKind::Local)
                .map(|duration| tokio::time::Instant::now() + duration);
        let saw_nonempty_output = Arc::new(AtomicBool::new(false));

        // Build imperatively so we can conditionally inject a sane PATH.
        let mut sh_cmd = Command::new("sh");
        sh_cmd
            .arg("-c")
            .arg(command)
            .current_dir(cwd_path)
            .env_clear()
            .envs(safe_env())
            .env("TERM", "dumb")
            .env("LC_ALL", "C.UTF-8")
            .env("PATH", subprocess_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = sh_cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("spawn failed: {e}"),
        })?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let stdout_task =
            tokio::spawn(drain_oneshot_pipe(stdout, Arc::clone(&saw_nonempty_output)));
        let stderr_task =
            tokio::spawn(drain_oneshot_pipe(stderr, Arc::clone(&saw_nonempty_output)));

        let deadline = tokio::time::Instant::now() + timeout;
        let exit_code = loop {
            if cancel.is_cancelled() {
                let _ = child.kill().await;
                let _ = child.wait().await;
                break 130;
            }

            if !saw_nonempty_output.load(Ordering::Relaxed)
                && stall_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
            {
                let _ = child.kill().await;
                let _ = child.wait().await;
                break 124;
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                let _ = child.kill().await;
                let _ = child.wait().await;
                break 124;
            }

            match tokio::time::timeout(remaining.min(Duration::from_millis(500)), child.wait())
                .await
            {
                Ok(Ok(status)) => break status.code().unwrap_or(-1),
                Ok(Err(e)) => {
                    return Err(ToolError::ExecutionFailed {
                        tool: "terminal".into(),
                        message: format!("wait failed: {e}"),
                    });
                }
                Err(_) => continue,
            }
        };

        Ok(ExecOutput {
            stdout: join_oneshot_pipe(stdout_task).await?,
            stderr: join_oneshot_pipe(stderr_task).await?,
            exit_code,
        })
    }
}

async fn drain_oneshot_pipe<R>(
    mut reader: R,
    saw_nonempty_output: Arc<AtomicBool>,
) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let read = reader.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        saw_nonempty_output.store(true, Ordering::Relaxed);
        bytes.extend_from_slice(&buf[..read]);
    }
    Ok(bytes)
}

async fn join_oneshot_pipe(
    task: tokio::task::JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<String, ToolError> {
    let bytes = task
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("output task failed: {e}"),
        })?
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!("output read failed: {e}"),
        })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[async_trait]
impl ExecutionBackend for LocalBackend {
    async fn execute(
        &self,
        command: &str,
        cwd: &str,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<ExecOutput, ToolError> {
        // Try persistent shell first
        let shell_init = self.ensure_shell().await;
        if shell_init.is_ok() {
            let fence_id = uuid::Uuid::new_v4().simple().to_string();

            // Build command with optional cwd change
            let full_cmd = if !cwd.is_empty() && cwd != "." {
                format!("cd {:?} && {}", cwd, command)
            } else {
                command.to_string()
            };

            let mut guard = self.shell.lock().await;
            if let Some(shell) = guard.as_mut() {
                let result = shell
                    .run(&full_cmd, &fence_id, timeout, cancel.clone())
                    .await;
                match result {
                    Ok(out) => return Ok(out),
                    Err(e) => {
                        debug!("Persistent shell error, falling back to oneshot: {e}");
                        // Mark dead and fall through to oneshot
                        shell.dead.store(true, Ordering::Relaxed);
                    }
                }
            }
        }

        // Fallback: one-shot
        warn!(
            "LocalBackend[{}]: using oneshot (persistent shell unavailable)",
            self.task_id
        );
        Self::oneshot(command, cwd, timeout, cancel).await
    }

    async fn cleanup(&self) -> Result<(), ToolError> {
        let mut guard = self.shell.lock().await;
        if let Some(shell) = guard.as_mut() {
            shell.kill().await;
        }
        *guard = None;
        Ok(())
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Local
    }

    /// LocalBackend is always considered healthy because `ensure_shell()`
    /// automatically restarts a dead persistent shell on the next `execute()`.
    async fn is_healthy(&self) -> bool {
        true
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cancel() -> CancellationToken {
        CancellationToken::new()
    }

    #[tokio::test]
    async fn blocklist_contains_openai() {
        let bl = blocklist();
        assert!(bl.contains("OPENAI_API_KEY"));
        assert!(bl.contains("ANTHROPIC_API_KEY"));
        assert!(bl.contains("GITHUB_TOKEN"));
    }

    #[tokio::test]
    async fn safe_env_excludes_blocked() {
        // Set a blocked var in current process for this test
        unsafe { env::set_var("OPENAI_API_KEY", "sk-test-12345") };
        let env_map: std::collections::HashMap<_, _> = safe_env().collect();
        // It should NOT appear in the subprocess env
        assert!(
            !env_map.contains_key("OPENAI_API_KEY"),
            "OPENAI_API_KEY must be blocked"
        );
        unsafe { env::remove_var("OPENAI_API_KEY") };
    }

    #[tokio::test]
    async fn local_backend_echo() {
        let b = LocalBackend::new("test-echo");
        let out = b
            .execute("echo hello", "/tmp", Duration::from_secs(5), cancel())
            .await
            .expect("execute");
        assert!(out.stdout.contains("hello"));
        assert_eq!(out.exit_code, 0);
    }

    #[tokio::test]
    async fn local_backend_exit_code() {
        let b = LocalBackend::new("test-exit");
        let out = b
            .execute("exit 42", "/tmp", Duration::from_secs(5), cancel())
            .await
            .expect("execute");
        assert_eq!(out.exit_code, 42);
    }

    #[tokio::test]
    async fn local_backend_timeout() {
        let b = LocalBackend::new("test-timeout");
        let out = b
            .execute("sleep 60", "/tmp", Duration::from_millis(200), cancel())
            .await
            .expect("execute");
        assert_eq!(out.exit_code, 124, "expected timeout exit code");
    }

    #[tokio::test]
    async fn local_backend_cancel() {
        let token = CancellationToken::new();
        let b = LocalBackend::new("test-cancel");
        let token_clone = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            token_clone.cancel();
        });
        let out = b
            .execute("sleep 60", "/tmp", Duration::from_secs(10), token)
            .await
            .expect("execute");
        assert_eq!(out.exit_code, 130, "expected cancelled exit code");
    }

    #[tokio::test]
    async fn local_backend_persistent_state() {
        // Verify that variables set in one call persist to the next (persistent shell)
        let b = LocalBackend::new("test-persistent");
        let _out1 = b
            .execute("export MY_VAR=42", "/tmp", Duration::from_secs(5), cancel())
            .await
            .expect("set var");
        let out2 = b
            .execute("echo $MY_VAR", "/tmp", Duration::from_secs(5), cancel())
            .await
            .expect("read var");
        assert!(
            out2.stdout.contains("42"),
            "persistent shell should retain MY_VAR; got: {:?}",
            out2.stdout
        );
    }

    #[tokio::test]
    async fn local_backend_multiline_output() {
        let b = LocalBackend::new("test-multiline");
        let out = b
            .execute(
                "printf 'a\\nb\\nc\\n'",
                "/tmp",
                Duration::from_secs(5),
                cancel(),
            )
            .await
            .expect("execute");
        // All three lines appear
        assert!(out.stdout.contains('a'));
        assert!(out.stdout.contains('b'));
        assert!(out.stdout.contains('c'));
        assert_eq!(out.exit_code, 0);
    }

    #[tokio::test]
    async fn local_backend_cleanup_idempotent() {
        let b = LocalBackend::new("test-cleanup");
        let _ = b
            .execute("echo x", "/tmp", Duration::from_secs(5), cancel())
            .await;
        b.cleanup().await.expect("first cleanup");
        b.cleanup().await.expect("second cleanup (idempotent)");
    }

    // ─── Env passthrough tests ────────────────────────────────────────

    /// Mutex that serialises tests which mutate the global passthrough registry
    /// (same pattern as SUDO_ENV_LOCK in mod.rs).
    static PASSTHROUGH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn passthrough_registry_register_and_clear() {
        let _guard = PASSTHROUGH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env_passthrough();
        assert!(!is_env_passthrough("MY_CUSTOM_KEY"));
        register_env_passthrough(&["MY_CUSTOM_KEY".to_string()]);
        assert!(is_env_passthrough("MY_CUSTOM_KEY"));
        clear_env_passthrough();
        assert!(!is_env_passthrough("MY_CUSTOM_KEY"));
    }

    #[test]
    fn passthrough_registry_idempotent() {
        let _guard = PASSTHROUGH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env_passthrough();
        register_env_passthrough(&["IDEMPOTENT_KEY".to_string(), "IDEMPOTENT_KEY".to_string()]);
        {
            let reg = passthrough_registry().read().unwrap();
            assert_eq!(reg.iter().filter(|k| *k == "IDEMPOTENT_KEY").count(), 1);
        }
        clear_env_passthrough();
    }

    #[test]
    fn safe_env_passes_through_registered_secret() {
        let _guard = PASSTHROUGH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env_passthrough();
        // MY_FORCED_API_KEY matches SECRET_SUFFIXES — would normally be stripped
        unsafe { env::set_var("MY_FORCED_API_KEY", "secret-value") };
        register_env_passthrough(&["MY_FORCED_API_KEY".to_string()]);
        let env_map: std::collections::HashMap<_, _> = safe_env().collect();
        assert!(
            env_map.contains_key("MY_FORCED_API_KEY"),
            "registered key should pass through safe_env()"
        );
        unsafe { env::remove_var("MY_FORCED_API_KEY") };
        clear_env_passthrough();
    }

    #[test]
    fn force_prefix_bypasses_blocklist() {
        let _guard = PASSTHROUGH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_env_passthrough();
        // Set the force-prefix env var for VAR_API_KEY
        unsafe { env::set_var("_EDGECRAB_FORCE_VAR_API_KEY", "1") };
        assert!(
            is_env_passthrough("VAR_API_KEY"),
            "_EDGECRAB_FORCE_<VAR> should make is_env_passthrough return true"
        );
        unsafe { env::remove_var("_EDGECRAB_FORCE_VAR_API_KEY") };
    }

    #[tokio::test]
    async fn parse_fence_end_valid() {
        let fence = "abc123";
        let line = fence_end_sentinel(fence, "0");
        assert_eq!(parse_fence_end(&line, fence), Some(0));

        let line2 = fence_end_sentinel(fence, "42");
        assert_eq!(parse_fence_end(&line2, fence), Some(42));
    }

    #[tokio::test]
    async fn parse_fence_end_wrong_id() {
        let line = fence_end_sentinel("abc", "0");
        assert_eq!(parse_fence_end(&line, "xyz"), None);
    }

    #[tokio::test]
    async fn exec_output_format() {
        let o = ExecOutput {
            stdout: "hello\n".into(),
            stderr: "warn\n".into(),
            exit_code: 1,
        };
        let s = o.format(usize::MAX, usize::MAX);
        assert!(s.contains("hello"));
        assert!(s.contains("[stderr]"));
        assert!(s.contains("[exit code: 1]"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn oneshot_respects_cwd() {
        use tempfile::TempDir;
        let dir = TempDir::new().expect("tmpdir");
        let path_str = dir.path().to_str().expect("utf8").to_string();
        let b = LocalBackend::new("test-cwd");
        let out = b
            .execute("pwd", &path_str, Duration::from_secs(5), cancel())
            .await
            .expect("execute");
        assert!(out.stdout.contains(dir.path().to_str().expect("utf8")));
    }
}
