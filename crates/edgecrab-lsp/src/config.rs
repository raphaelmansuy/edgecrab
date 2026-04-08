#[cfg(unix)]
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use edgecrab_tools::config_ref::LspServerConfigRef;

use crate::error::LspError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedServerCommand {
    pub program: PathBuf,
    pub args_prefix: Vec<String>,
}

impl ResolvedServerCommand {
    pub fn display(&self) -> String {
        let mut parts = vec![self.program.display().to_string()];
        parts.extend(self.args_prefix.iter().cloned());
        parts.join(" ")
    }
}

pub fn install_hint(command: &str) -> Option<&'static str> {
    match command {
        "rust-analyzer" => Some("Install with: rustup component add rust-analyzer"),
        "typescript-language-server" => {
            Some("Install with: npm install -g typescript-language-server typescript")
        }
        "clangd" => Some("Install with: brew install llvm  or  apt install clangd"),
        "pylsp" => Some("Install with: pip install python-lsp-server"),
        "gopls" => Some("Install with: go install golang.org/x/tools/gopls@latest"),
        "jdtls" => Some("Install with: brew install jdtls  or  apt install jdtls"),
        "csharp-ls" => Some("Install with: dotnet tool install --global csharp-ls"),
        "intelephense" => Some("Install with: npm install -g intelephense"),
        "ruby-lsp" => Some("Install with: gem install ruby-lsp"),
        "bash-language-server" => Some("Install with: npm install -g bash-language-server"),
        "vscode-html-language-server" => {
            Some("Install with: npm install -g vscode-langservers-extracted")
        }
        "vscode-css-language-server" => {
            Some("Install with: npm install -g vscode-langservers-extracted")
        }
        "vscode-json-language-server" => {
            Some("Install with: npm install -g vscode-langservers-extracted")
        }
        _ => None,
    }
}

pub fn resolve_server_command(
    server_name: &str,
    command: &str,
    cwd: Option<&Path>,
) -> Result<ResolvedServerCommand, LspError> {
    let mut attempts = Vec::new();
    let mut probe_failures = Vec::new();

    if let Some(program) = resolve_program_path(command, cwd) {
        let resolved = ResolvedServerCommand {
            program,
            args_prefix: Vec::new(),
        };
        attempts.push(format!("direct binary: {}", resolved.display()));
        match probe_launch_spec(command, &resolved) {
            Ok(()) => return Ok(resolved),
            Err(failure) => probe_failures.push(failure),
        }
    } else {
        attempts.push(format!("direct binary '{command}' was not found"));
    }

    for resolved in launcher_fallbacks(command, cwd) {
        attempts.push(format!("launcher: {}", resolved.display()));
        match probe_launch_spec(command, &resolved) {
            Ok(()) => return Ok(resolved),
            Err(failure) => probe_failures.push(failure),
        }
    }

    if probe_failures
        .iter()
        .any(|failure| matches!(failure.kind, ResolveFailureKind::Unavailable))
    {
        return Err(build_unavailable_error(
            server_name,
            command,
            cwd,
            &attempts,
            &probe_failures,
        ));
    }

    Err(build_not_found_error(
        server_name,
        command,
        cwd,
        &attempts,
        &probe_failures,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolveFailureKind {
    NotFound,
    Unavailable,
}

#[derive(Debug, Clone)]
struct ResolveFailure {
    kind: ResolveFailureKind,
    context: String,
    message: String,
}

fn build_not_found_error(
    server_name: &str,
    command: &str,
    cwd: Option<&Path>,
    attempts: &[String],
    failures: &[ResolveFailure],
) -> LspError {
    let mut hint_parts = vec![
        install_hint(command)
            .unwrap_or("Install the server and ensure it is available on PATH")
            .to_string(),
    ];

    if let Some(cwd) = cwd {
        hint_parts.push(format!("Workspace root: {}", cwd.display()));
    }

    hint_parts.push(format!(
        "Override with lsp.servers.{server_name}.command in ~/.edgecrab/config.yaml if your server lives outside the default lookup paths."
    ));

    if uses_launcher_fallbacks(command) {
        hint_parts.push(
            "EdgeCrab also checked project-local binaries and supported launcher fallbacks for this language."
                .to_string(),
        );
    }

    if !attempts.is_empty() {
        hint_parts.push(format!("Tried: {}", attempts.join("; ")));
    }

    let probe_summary = summarize_failures(failures);
    if !probe_summary.is_empty() {
        hint_parts.push(format!("Probe results: {probe_summary}"));
    }

    LspError::ServerNotFound {
        command: command.to_string(),
        hint: hint_parts.join(" "),
    }
}

fn build_unavailable_error(
    server_name: &str,
    command: &str,
    cwd: Option<&Path>,
    attempts: &[String],
    failures: &[ResolveFailure],
) -> LspError {
    let mut message_parts = vec![format!(
        "EdgeCrab found a candidate for '{command}', but it could not be started successfully."
    )];

    if let Some(cwd) = cwd {
        message_parts.push(format!("Workspace root: {}", cwd.display()));
    }

    message_parts.push(format!(
        "If needed, override the launcher with lsp.servers.{server_name}.command in ~/.edgecrab/config.yaml."
    ));

    if !attempts.is_empty() {
        message_parts.push(format!("Tried: {}", attempts.join("; ")));
    }

    let probe_summary = summarize_failures(failures);
    if !probe_summary.is_empty() {
        message_parts.push(format!("Failures: {probe_summary}"));
    }

    LspError::ServerUnavailable {
        server: server_name.to_string(),
        message: message_parts.join(" "),
    }
}

fn summarize_failures(failures: &[ResolveFailure]) -> String {
    failures
        .iter()
        .map(|failure| format!("{} -> {}", failure.context, failure.message))
        .collect::<Vec<_>>()
        .join("; ")
}

fn resolve_program_path(command: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let command_path = Path::new(command);
    if command_path.is_absolute() && is_file(command_path) {
        return Some(command_path.to_path_buf());
    }

    if command_path.components().count() > 1 {
        if let Some(base) = cwd {
            let candidate = base.join(command_path);
            if is_file(&candidate) {
                return Some(candidate);
            }
        }
    }

    if let Some(candidate) = resolve_project_local_binary(command, cwd) {
        return Some(candidate);
    }

    if let Ok(path) = which::which(command) {
        return Some(path);
    }

    for candidate in common_bin_dirs(None)
        .into_iter()
        .map(|dir| dir.join(command))
    {
        if is_file(&candidate) {
            return Some(candidate);
        }
    }

    resolve_via_login_shell(command)
}

fn is_file(path: &Path) -> bool {
    path.is_file()
}

fn common_bin_dirs(home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
    ];

    if let Some(home) = home.or_else(dirs::home_dir) {
        dirs.push(home.join(".cargo/bin"));
        dirs.push(home.join("go/bin"));
        dirs.push(home.join(".local/bin"));
    }

    dirs
}

fn resolve_project_local_binary(command: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let mut current = cwd;
    while let Some(dir) = current {
        for local_dir in ["node_modules/.bin", ".venv/bin", "venv/bin", "vendor/bin"] {
            let candidate = dir.join(local_dir).join(command);
            if is_file(&candidate) {
                return Some(candidate);
            }
        }
        current = dir.parent();
    }
    None
}

/// On Unix, ask a login shell to locate the binary (handles nvm, pyenv, etc.).
/// On Windows, `SHELL` semantics do not apply; binary discovery relies on PATH
/// and the common-dirs scan, so this is a deliberate no-op.
#[cfg(unix)]
fn resolve_via_login_shell(command: &str) -> Option<PathBuf> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| OsStr::new("/bin/sh").to_os_string());
    let quoted = shell_quote(command);
    let output = Command::new(shell)
        .arg("-lc")
        .arg(format!("command -v -- {quoted}"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let candidate = PathBuf::from(path);
    is_file(&candidate).then_some(candidate)
}

#[cfg(not(unix))]
fn resolve_via_login_shell(_command: &str) -> Option<PathBuf> {
    None
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn launcher_fallbacks(command: &str, cwd: Option<&Path>) -> Vec<ResolvedServerCommand> {
    let mut resolved = Vec::new();
    for (launcher, args_prefix) in launcher_templates(command) {
        let Some(program) = resolve_program_path(launcher, cwd) else {
            continue;
        };
        resolved.push(ResolvedServerCommand {
            program,
            args_prefix,
        });
    }
    resolved
}

fn launcher_templates(command: &str) -> Vec<(&'static str, Vec<String>)> {
    match command {
        "typescript-language-server"
        | "intelephense"
        | "bash-language-server"
        | "vscode-html-language-server"
        | "vscode-css-language-server"
        | "vscode-json-language-server" => vec![
            ("pnpm", vec!["exec".to_string(), command.to_string()]),
            ("npx", vec!["--no-install".to_string(), command.to_string()]),
            ("yarn", vec!["exec".to_string(), command.to_string()]),
        ],
        "pylsp" => vec![
            ("python3", vec!["-m".to_string(), "pylsp".to_string()]),
            ("python", vec!["-m".to_string(), "pylsp".to_string()]),
        ],
        "ruby-lsp" => vec![("bundle", vec!["exec".to_string(), "ruby-lsp".to_string()])],
        "csharp-ls" => vec![(
            "dotnet",
            vec![
                "tool".to_string(),
                "run".to_string(),
                "csharp-ls".to_string(),
                "--".to_string(),
            ],
        )],
        _ => Vec::new(),
    }
}

fn uses_launcher_fallbacks(command: &str) -> bool {
    !launcher_templates(command).is_empty()
}

fn probe_launch_spec(
    command: &str,
    resolved: &ResolvedServerCommand,
) -> Result<(), ResolveFailure> {
    let mut probe = Command::new(&resolved.program);
    probe.args(&resolved.args_prefix);
    probe.args(server_probe_args(command));
    probe.env_remove("RUST_LOG");

    let output = probe.output().map_err(|err| ResolveFailure {
        kind: ResolveFailureKind::Unavailable,
        context: resolved.display(),
        message: err.to_string(),
    })?;
    if output.status.success() {
        return Ok(());
    }

    let message = normalize_probe_message(&output.stdout, &output.stderr, output.status.code());
    Err(ResolveFailure {
        kind: classify_probe_failure(command, &message),
        context: resolved.display(),
        message,
    })
}

fn server_probe_args(command: &str) -> &'static [&'static str] {
    match command {
        "jdtls" => &["-version"],
        _ => &["--version"],
    }
}

fn normalize_probe_message(stdout: &[u8], stderr: &[u8], status_code: Option<i32>) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }

    match status_code {
        Some(code) => format!("probe exited with status {code}"),
        None => "probe terminated by signal".to_string(),
    }
}

fn classify_probe_failure(command: &str, message: &str) -> ResolveFailureKind {
    let lower = message.to_ascii_lowercase();
    if command == "rust-analyzer" && message.contains("Unknown binary 'rust-analyzer'") {
        return ResolveFailureKind::NotFound;
    }

    let missing_patterns = [
        "no module named",
        "no such file or directory",
        "not found",
        "command not found",
        "unknown binary",
        "missing packages",
        "could not determine executable to run",
        "cannot find a tool in the manifest file",
        "tool 'csharp-ls' is not currently installed",
        "gem not found",
        "could not find gem",
        "unable to find gem",
    ];
    if missing_patterns
        .iter()
        .any(|pattern| lower.contains(pattern))
    {
        return ResolveFailureKind::NotFound;
    }

    ResolveFailureKind::Unavailable
}

pub fn detect_project_root(file: &Path, cwd: &Path, server: &LspServerConfigRef) -> PathBuf {
    let mut current = file.parent().unwrap_or(cwd).to_path_buf();
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());

    loop {
        if server
            .root_markers
            .iter()
            .any(|marker| directory_has_root_marker(&current, marker))
        {
            return current;
        }

        if current == cwd {
            return current;
        }

        let Some(parent) = current.parent() else {
            return cwd;
        };
        current = parent.to_path_buf();
    }
}

fn directory_has_root_marker(dir: &Path, marker: &str) -> bool {
    if let Some(suffix) = marker.strip_prefix("*.") {
        return std::fs::read_dir(dir)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(Result::ok))
            .any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(&format!(".{suffix}")))
            });
    }

    dir.join(marker).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &Path) {}

    #[test]
    fn install_hint_covers_extended_language_catalog() {
        for command in [
            "rust-analyzer",
            "typescript-language-server",
            "pylsp",
            "gopls",
            "clangd",
            "jdtls",
            "csharp-ls",
            "intelephense",
            "ruby-lsp",
            "bash-language-server",
            "vscode-html-language-server",
            "vscode-css-language-server",
            "vscode-json-language-server",
        ] {
            assert!(
                install_hint(command).is_some(),
                "expected install hint for {command}"
            );
        }
    }

    #[test]
    #[cfg(unix)] // relies on a shell script that cannot be executed directly on Windows
    fn resolve_server_command_finds_relative_binary_from_workspace() {
        let workspace = TempDir::new().expect("workspace");
        let bin_dir = workspace.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("bin dir");
        let script = bin_dir.join("fake-lsp");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").expect("script");
        make_executable(&script);

        let resolved = resolve_server_command("fake", "bin/fake-lsp", Some(workspace.path()))
            .expect("relative command");
        assert_eq!(resolved.program, script);
        assert!(resolved.args_prefix.is_empty());
    }

    #[test]
    fn detect_project_root_supports_glob_root_markers() {
        let workspace = TempDir::new().expect("workspace");
        let nested = workspace.path().join("src");
        std::fs::create_dir_all(&nested).expect("nested");
        std::fs::write(workspace.path().join("demo.sln"), "").expect("solution");
        let file = nested.join("Program.cs");
        std::fs::write(&file, "class Program {}").expect("file");

        let server = LspServerConfigRef {
            root_markers: vec!["*.sln".to_string()],
            ..LspServerConfigRef::default()
        };

        let root = detect_project_root(&file, workspace.path(), &server);
        assert_eq!(root, workspace.path());
    }

    #[test]
    fn resolve_server_command_checks_common_home_bin_locations() {
        let home = TempDir::new().expect("home");
        let bins = common_bin_dirs(Some(home.path().to_path_buf()));
        assert!(bins.contains(&home.path().join(".cargo/bin")));
        assert!(bins.contains(&home.path().join("go/bin")));
        assert!(bins.contains(&home.path().join(".local/bin")));
    }

    #[test]
    #[cfg(unix)] // relies on a shell script that cannot be executed directly on Windows
    fn resolve_server_command_finds_project_local_node_binary() {
        let workspace = TempDir::new().expect("workspace");
        let local_bin = workspace.path().join("node_modules/.bin");
        std::fs::create_dir_all(&local_bin).expect("node bin");
        let server = local_bin.join("typescript-language-server");
        std::fs::write(&server, "#!/bin/sh\nexit 0\n").expect("server");
        make_executable(&server);
        let nested = workspace.path().join("packages/app");
        std::fs::create_dir_all(&nested).expect("nested");

        let resolved =
            resolve_server_command("typescript", "typescript-language-server", Some(&nested))
                .expect("local node_modules binary");

        assert_eq!(resolved.program, server);
        assert!(resolved.args_prefix.is_empty());
    }

    #[test]
    fn launcher_templates_cover_mainstream_languages_without_direct_binaries() {
        assert!(!launcher_templates("typescript-language-server").is_empty());
        assert!(!launcher_templates("pylsp").is_empty());
        assert!(!launcher_templates("ruby-lsp").is_empty());
        assert!(!launcher_templates("csharp-ls").is_empty());
    }

    #[test]
    fn missing_server_error_mentions_config_override_and_attempts() {
        let workspace = TempDir::new().expect("workspace");
        let error = resolve_server_command(
            "typescript",
            "definitely-missing-edgecrab-lsp-server",
            Some(workspace.path()),
        )
        .expect_err("missing server");

        let rendered = error.to_string();
        assert!(rendered.contains("Language server binary"));
        assert!(rendered.contains("Override with lsp.servers.typescript.command"));
        assert!(rendered.contains("Tried:"));
    }
}
