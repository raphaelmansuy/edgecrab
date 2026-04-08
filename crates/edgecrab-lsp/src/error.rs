use std::path::{Path, PathBuf};

use edgecrab_types::ToolError;
use lsp_types::Uri;

#[derive(Debug, thiserror::Error)]
pub enum LspError {
    #[error("LSP tools are disabled in config")]
    Disabled,
    #[error("LSP is only supported with the local terminal backend")]
    RemoteBackendUnsupported,
    #[error("No configured language server matches '{path}'")]
    NoServerForFile { path: String },
    #[error("Language server binary '{command}' was not found. {hint}")]
    ServerNotFound { command: String, hint: String },
    #[error("Language server '{server}' is unavailable: {message}")]
    ServerUnavailable { server: String, message: String },
    #[error("File '{path}' is too large for LSP sync ({size} bytes > {limit} bytes)")]
    FileTooLarge { path: String, size: u64, limit: u64 },
    #[error("LSP protocol error: {0}")]
    Protocol(String),
    #[error("LSP JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Path '{0}' cannot be represented as a file URL")]
    InvalidFilePath(String),
    #[error("LSP response for '{method}' was not supported by the server")]
    MethodNotFound { method: String },
    #[error("{0}")]
    Other(String),
}

impl LspError {
    pub fn to_tool_error(&self, tool: &str) -> ToolError {
        match self {
            Self::Disabled => ToolError::Unavailable {
                tool: tool.into(),
                reason: self.to_string(),
            },
            Self::RemoteBackendUnsupported => ToolError::Unavailable {
                tool: tool.into(),
                reason: self.to_string(),
            },
            Self::NoServerForFile { .. } => ToolError::Unavailable {
                tool: tool.into(),
                reason: self.to_string(),
            },
            Self::ServerNotFound { .. } => ToolError::Unavailable {
                tool: tool.into(),
                reason: self.to_string(),
            },
            Self::ServerUnavailable { .. } => ToolError::Unavailable {
                tool: tool.into(),
                reason: self.to_string(),
            },
            Self::FileTooLarge { .. } => ToolError::Other(self.to_string()),
            Self::Protocol(_) | Self::Json(_) | Self::Io(_) | Self::InvalidFilePath(_) => {
                ToolError::ExecutionFailed {
                    tool: tool.into(),
                    message: self.to_string(),
                }
            }
            Self::MethodNotFound { .. } => ToolError::Unavailable {
                tool: tool.into(),
                reason: self.to_string(),
            },
            Self::Other(_) => ToolError::ExecutionFailed {
                tool: tool.into(),
                message: self.to_string(),
            },
        }
    }
}

pub fn path_to_uri(path: &Path) -> Result<Uri, LspError> {
    let url = url::Url::from_file_path(path)
        .map_err(|_| LspError::InvalidFilePath(path.display().to_string()))?;
    url.as_str()
        .parse()
        .map_err(|_| LspError::InvalidFilePath(path.display().to_string()))
}

pub fn uri_to_path(uri: &Uri) -> Result<PathBuf, LspError> {
    let url =
        url::Url::parse(uri.as_str()).map_err(|_| LspError::InvalidFilePath(uri.to_string()))?;
    url.to_file_path()
        .map_err(|_| LspError::InvalidFilePath(uri.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_uri_round_trips_current_platform_path() {
        let path = if cfg!(windows) {
            PathBuf::from(r"C:\edgecrab\src\main.rs")
        } else {
            PathBuf::from("/tmp/edgecrab/src/main.rs")
        };

        let uri = path_to_uri(&path).expect("path to uri");
        let round_trip = uri_to_path(&uri).expect("uri to path");
        assert_eq!(round_trip, path);
    }

    #[cfg(unix)]
    #[test]
    fn uri_to_path_handles_unix_file_uri() {
        let uri: Uri = "file:///tmp/edgecrab/src/main.rs".parse().expect("uri");
        assert_eq!(
            uri_to_path(&uri).expect("path"),
            PathBuf::from("/tmp/edgecrab/src/main.rs")
        );
    }

    #[cfg(windows)]
    #[test]
    fn uri_to_path_handles_windows_file_uri() {
        let uri: Uri = "file:///C:/edgecrab/src/main.rs".parse().expect("uri");
        assert_eq!(
            uri_to_path(&uri).expect("path"),
            PathBuf::from(r"C:\edgecrab\src\main.rs")
        );
    }
}
