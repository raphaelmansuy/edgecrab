use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::config_ref::AppConfigRef;
use crate::execution_tmp::BACKEND_TMP_ROOT;
use crate::tools::backends::{BackendKind, DockerBackendConfig, DockerWorkspaceMount};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalFilesystemRelation {
    SharedPathNamespace,
    SharedContentMapped {
        host_root: PathBuf,
        terminal_root: PathBuf,
    },
    IsolatedBackend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionFilesystemView {
    workspace_root: PathBuf,
    file_roots: Vec<PathBuf>,
    denied_roots: Vec<PathBuf>,
    file_tools_tmp_root: PathBuf,
    terminal_backend: BackendKind,
    terminal_relation: TerminalFilesystemRelation,
}

impl ExecutionFilesystemView {
    pub fn new(config: &AppConfigRef, cwd: &Path) -> Self {
        let workspace_root = canonical_or_raw(cwd);
        let file_tools_tmp_root = canonical_or_raw(&config.file_tools_tmp_dir());
        let file_roots = collect_roots(
            std::iter::once(cwd.to_path_buf())
                .chain(std::iter::once(file_tools_tmp_root.clone()))
                .chain(
                    config
                        .file_allowed_roots
                        .iter()
                        .map(|root| resolve_root(root, cwd)),
                ),
        );
        let denied_roots = collect_roots(
            config
                .path_restrictions
                .iter()
                .map(|root| resolve_root(root, cwd)),
        );
        let terminal_relation = match config.terminal_backend {
            BackendKind::Local => TerminalFilesystemRelation::SharedPathNamespace,
            BackendKind::Docker => {
                let mount = effective_docker_workspace_mount(cwd, &config.terminal_docker);
                TerminalFilesystemRelation::SharedContentMapped {
                    host_root: canonical_or_raw(Path::new(&mount.host_path)),
                    terminal_root: PathBuf::from(mount.container_path),
                }
            }
            BackendKind::Ssh
            | BackendKind::Modal
            | BackendKind::Daytona
            | BackendKind::Singularity => TerminalFilesystemRelation::IsolatedBackend,
        };

        Self {
            workspace_root,
            file_roots,
            denied_roots,
            file_tools_tmp_root,
            terminal_backend: config.terminal_backend.clone(),
            terminal_relation,
        }
    }

    pub fn render_prompt_block(&self) -> String {
        let file_roots = format_paths(&self.file_roots);
        let denied = if self.denied_roots.is_empty() {
            "none".to_string()
        } else {
            format_paths(&self.denied_roots)
        };
        let tmp_note = format!(
            "File-tool `/tmp` is a virtual alias for `{}`. It is not the host-global `/tmp`, so file tools only see EdgeCrab-owned temp files there.",
            self.file_tools_tmp_root.display()
        );

        let terminal_note = match &self.terminal_relation {
            TerminalFilesystemRelation::SharedPathNamespace => format!(
                "Terminal backend: `{}`. `terminal` uses the host path namespace for normal paths, and EdgeCrab exports `TMPDIR`, `TMP`, `TEMP`, and `EDGECRAB_TMPDIR` as `{}` so temp-aware programs converge on the same agent-owned temp root. Literal `/tmp/...` in shell commands still targets the real host `/tmp`.",
                self.terminal_backend,
                self.file_tools_tmp_root.display()
            ),
            TerminalFilesystemRelation::SharedContentMapped {
                host_root,
                terminal_root,
            } => format!(
                "Terminal backend: `docker`. The shared host tree `{}` is mounted inside the backend at `{}`. EdgeCrab also bind-mounts the shared temp root `{}` to container `/tmp`, so file-tool `/tmp`, docker `/tmp`, and temp-aware programs all converge on the same files.",
                host_root.display(),
                terminal_root.display(),
                self.file_tools_tmp_root.display()
            ),
            TerminalFilesystemRelation::IsolatedBackend => format!(
                "Terminal backend: `{}`. `terminal` uses an isolated backend filesystem. EdgeCrab exports `TMPDIR`, `TMP`, `TEMP`, and `EDGECRAB_TMPDIR` as `{}` inside that backend so temp-aware programs use a stable agent temp root there. Literal `/tmp/...` shell paths remain backend-native and are not shared with host file tools unless you copy them out explicitly.",
                self.terminal_backend, BACKEND_TMP_ROOT
            ),
        };

        let execute_code_note = match &self.terminal_relation {
            TerminalFilesystemRelation::SharedPathNamespace => format!(
                "`execute_code` runs as a host subprocess in `{}`. EdgeCrab injects `TMPDIR`, `TMP`, `TEMP`, and `EDGECRAB_TMPDIR` as `{}`, so Python/Ruby/Node temp APIs share the file-tool temp root. Literal `/tmp/...` paths in code still target the real host `/tmp`. Prefer file tools when you need workspace-root enforcement and path restriction checks.",
                self.workspace_root.display(),
                self.file_tools_tmp_root.display()
            ),
            TerminalFilesystemRelation::SharedContentMapped {
                host_root: _host_root,
                terminal_root,
            } => format!(
                "`execute_code` runs inside the docker backend. Direct script file I/O and `terminal()` inside `execute_code` share the backend filesystem. Docker `/tmp` is bind-mounted to the shared host temp root `{}`, so temp APIs and literal `/tmp/...` paths converge with file-tool `/tmp`. The shared project tree appears inside docker at `{}`.",
                self.file_tools_tmp_root.display(),
                terminal_root.display()
            ),
            TerminalFilesystemRelation::IsolatedBackend => format!(
                "`execute_code` runs inside the `{}` backend. EdgeCrab injects `TMPDIR`, `TMP`, `TEMP`, and `EDGECRAB_TMPDIR` as `{}` there, so temp-aware code uses a stable backend temp root. File tools still operate on the host allow-list rooted at {} and map file-tool `/tmp` to `{}`; backend-only files remain unreadable to host file tools unless you explicitly sync them out.",
                self.terminal_backend,
                BACKEND_TMP_ROOT,
                self.file_roots_display(),
                self.file_tools_tmp_root.display()
            ),
        };

        format!(
            "## Execution Filesystem\n\n\
Workspace root: `{}`\n\
File tools (`read_file`, `write_file`, `patch`, `search_files`) may access: {}.\n\
Blocked subtrees from `security.path_restrictions`: {}.\n\
{}\n\
{}\n\
{}",
            self.workspace_root.display(),
            file_roots,
            denied,
            tmp_note,
            terminal_note,
            execute_code_note
        )
    }

    pub fn file_roots_display(&self) -> String {
        format_paths(&self.file_roots)
    }

    pub fn execute_code_terminal_is_safe(&self) -> bool {
        true
    }
}

pub fn describe_execution_filesystem(config: &AppConfigRef, cwd: &Path) -> ExecutionFilesystemView {
    ExecutionFilesystemView::new(config, cwd)
}

pub fn effective_docker_workspace_mount(
    cwd: &Path,
    docker: &DockerBackendConfig,
) -> DockerWorkspaceMount {
    docker
        .workspace_mount
        .clone()
        .unwrap_or_else(|| DockerWorkspaceMount {
            host_path: cwd.to_string_lossy().into_owned(),
            container_path: "/workspace".into(),
            read_only: false,
        })
}

fn resolve_root(root: &Path, cwd: &Path) -> PathBuf {
    if root.is_absolute() {
        root.to_path_buf()
    } else {
        cwd.join(root)
    }
}

fn canonical_or_raw(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn collect_roots<I>(roots: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for root in roots {
        let resolved = canonical_or_raw(&root);
        if seen.insert(resolved.clone()) {
            out.push(resolved);
        }
    }
    out
}

fn format_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| format!("`{}`", p.display()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn config_for(backend: BackendKind) -> AppConfigRef {
        AppConfigRef {
            terminal_backend: backend,
            ..Default::default()
        }
    }

    #[test]
    fn local_view_marks_tmp_outside_file_roots_by_default() {
        let dir = TempDir::new().expect("tmpdir");
        let view = describe_execution_filesystem(&config_for(BackendKind::Local), dir.path());
        let prompt = view.render_prompt_block();
        assert!(prompt.contains("File-tool `/tmp` is a virtual alias"));
        assert!(prompt.contains("temp-aware programs converge"));
        assert!(
            prompt.contains(
                "Literal `/tmp/...` in shell commands still targets the real host `/tmp`"
            )
        );
    }

    #[test]
    fn docker_view_describes_host_to_container_mapping() {
        let dir = TempDir::new().expect("tmpdir");
        let view = describe_execution_filesystem(&config_for(BackendKind::Docker), dir.path());
        let prompt = view.render_prompt_block();
        assert!(prompt.contains("mounted inside the backend at `/workspace`"));
        assert!(prompt.contains("runs inside the docker backend"));
        assert!(prompt.contains("bind-mounts the shared temp root"));
        assert!(prompt.contains("literal `/tmp/...` paths converge"));
    }

    #[test]
    fn isolated_backend_view_calls_out_execute_code_split() {
        let dir = TempDir::new().expect("tmpdir");
        let view = describe_execution_filesystem(&config_for(BackendKind::Modal), dir.path());
        let prompt = view.render_prompt_block();
        assert!(prompt.contains("isolated backend filesystem"));
        assert!(prompt.contains("runs inside the `modal` backend"));
        assert!(prompt.contains("backend-only files remain unreadable"));
        assert!(prompt.contains("temp-aware programs use a stable agent temp root"));
    }

    #[test]
    fn effective_docker_mount_defaults_to_workspace_mapping() {
        let dir = TempDir::new().expect("tmpdir");
        let mount = effective_docker_workspace_mount(dir.path(), &DockerBackendConfig::default());
        assert_eq!(mount.container_path, "/workspace");
        assert_eq!(Path::new(&mount.host_path), dir.path());
    }
}
