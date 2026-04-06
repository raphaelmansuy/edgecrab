use std::path::{Path, PathBuf};

use edgecrab_types::ToolError;

use crate::tools::backends::shell_quote;

pub(crate) const BACKEND_TMP_ROOT: &str = "/tmp/edgecrab-tmp";

fn resolve_edgecrab_home() -> PathBuf {
    std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if cfg!(test) {
                return std::env::temp_dir()
                    .join(format!("edgecrab-test-home-{}", std::process::id()));
            }
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".edgecrab")
        })
}

pub(crate) fn shared_tmp_dir(edgecrab_home: &Path) -> PathBuf {
    edgecrab_home.join("tmp").join("files")
}

pub(crate) fn default_shared_tmp_dir() -> PathBuf {
    shared_tmp_dir(&resolve_edgecrab_home())
}

pub(crate) fn ensure_default_shared_tmp_dir() -> Result<PathBuf, ToolError> {
    let dir = default_shared_tmp_dir();
    std::fs::create_dir_all(&dir).map_err(|e| ToolError::ExecutionFailed {
        tool: "terminal".into(),
        message: format!(
            "Failed to create EdgeCrab shared temp root '{}': {e}",
            dir.display()
        ),
    })?;
    Ok(dir)
}

pub(crate) fn temp_env_pairs(tmp_root: &str) -> [(String, String); 4] {
    [
        ("EDGECRAB_TMPDIR".into(), tmp_root.into()),
        ("TMPDIR".into(), tmp_root.into()),
        ("TMP".into(), tmp_root.into()),
        ("TEMP".into(), tmp_root.into()),
    ]
}

pub(crate) fn wrap_command_with_tmp_env(command: &str, tmp_root: &str) -> String {
    let quoted = shell_quote(tmp_root);
    format!(
        "mkdir -p {quoted} && export EDGECRAB_TMPDIR={quoted} TMPDIR={quoted} TMP={quoted} TEMP={quoted} && {command}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_env_pairs_populates_standard_keys() {
        let vars = temp_env_pairs("/tmp/edgecrab-tmp");
        assert!(
            vars.iter()
                .any(|(k, v)| k == "TMPDIR" && v == "/tmp/edgecrab-tmp")
        );
        assert!(
            vars.iter()
                .any(|(k, v)| k == "EDGECRAB_TMPDIR" && v == "/tmp/edgecrab-tmp")
        );
    }

    #[test]
    fn wrap_command_with_tmp_env_exports_and_executes() {
        let wrapped = wrap_command_with_tmp_env("python3 script.py", "/tmp/edgecrab-tmp");
        assert!(wrapped.contains("mkdir -p '/tmp/edgecrab-tmp'"));
        assert!(wrapped.contains("TMPDIR='/tmp/edgecrab-tmp'"));
        assert!(wrapped.ends_with("python3 script.py"));
    }
}
