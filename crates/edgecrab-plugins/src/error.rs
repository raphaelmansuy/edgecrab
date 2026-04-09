use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin manifest missing at {path}")]
    MissingManifest { path: PathBuf },
    #[error("invalid plugin manifest {path}: {message}")]
    InvalidManifest { path: PathBuf, message: String },
    #[error("invalid skill manifest {path}: {message}")]
    InvalidSkill { path: PathBuf, message: String },
    #[error("plugin process error: {0}")]
    Process(String),
    #[error("plugin rpc error: {0}")]
    Rpc(String),
    #[error("plugin hub error: {0}")]
    Hub(String),
    #[error("script error: {0}")]
    Script(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),
}
