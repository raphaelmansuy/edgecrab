use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::Context;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, fmt, util::SubscriberInitExt};

const DEFAULT_MAX_LOG_MB: u64 = 10;
const DEFAULT_BACKUP_COUNT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoggingMode {
    Agent,
    Gateway,
    Acp,
}

impl LoggingMode {
    pub fn primary_log_name(self) -> &'static str {
        match self {
            Self::Agent => "agent.log",
            Self::Gateway => "gateway.log",
            Self::Acp => "acp.log",
        }
    }

    pub fn json_log_name(self) -> &'static str {
        match self {
            Self::Agent => "agent.jsonl",
            Self::Gateway => "gateway.jsonl",
            Self::Acp => "acp.jsonl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StderrMode {
    Off,
    Warn,
    Debug,
}

pub struct LoggingGuards;

pub fn init_logging(
    home: &Path,
    mode: LoggingMode,
    debug: bool,
    stderr_mode: StderrMode,
) -> anyhow::Result<LoggingGuards> {
    let log_dir = home.join("logs");
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create {}", log_dir.display()))?;

    let max_bytes = log_max_bytes();
    let backups = log_backup_count();

    let text_path =
        prepare_log_file_path(&log_dir.join(mode.primary_log_name()), max_bytes, backups)?;
    let error_path = prepare_log_file_path(&log_dir.join("errors.log"), max_bytes / 2, backups)?;
    let json_path = prepare_log_file_path(&log_dir.join(mode.json_log_name()), max_bytes, backups)?;

    let text_writer = make_file_writer(text_path);
    let error_writer = make_file_writer(error_path);
    let json_writer = make_file_writer(json_path);

    let base_filter = base_env_filter(debug);
    let text_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .with_writer(text_writer);

    let error_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .with_writer(error_writer.with_max_level(tracing::Level::WARN));

    let json_layer = fmt::layer()
        .json()
        .with_ansi(false)
        .with_current_span(true)
        .with_span_list(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .with_writer(json_writer);

    let subscriber = tracing_subscriber::registry()
        .with(base_filter)
        .with(text_layer)
        .with(error_layer)
        .with(json_layer);

    match stderr_mode {
        StderrMode::Off => subscriber
            .try_init()
            .context("failed to initialize centralized logging")?,
        StderrMode::Warn => subscriber
            .with(fmt::layer().with_writer(std::io::stderr.with_max_level(tracing::Level::WARN)))
            .try_init()
            .context("failed to initialize centralized logging")?,
        StderrMode::Debug => subscriber
            .with(fmt::layer().with_writer(std::io::stderr.with_max_level(tracing::Level::DEBUG)))
            .try_init()
            .context("failed to initialize centralized logging")?,
    }

    tracing::info!(
        mode = ?mode,
        log_dir = %log_dir.display(),
        primary_log = mode.primary_log_name(),
        json_log = mode.json_log_name(),
        "centralized logging initialized"
    );

    Ok(LoggingGuards)
}

fn base_env_filter(debug: bool) -> EnvFilter {
    let default_level = if debug {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };
    // WHY ignore ambient RUST_LOG here: persistent EdgeCrab file logs should
    // stay debuggable even when the parent shell exports restrictive filters
    // like `RUST_LOG=warn`. EdgeCrab uses its own override knob instead.
    let mut filter = if let Ok(spec) = std::env::var("EDGECRAB_LOG_FILTER") {
        EnvFilter::builder()
            .with_default_directive(default_level.into())
            .parse_lossy(spec)
    } else {
        EnvFilter::builder()
            .with_default_directive(default_level.into())
            .parse_lossy("")
    };
    for noisy in [
        "hyper=warn",
        "h2=warn",
        "reqwest=warn",
        "rustls=warn",
        "tungstenite=warn",
        "tokio_tungstenite=warn",
        "aws_config=warn",
        "aws_smithy_runtime=warn",
    ] {
        filter = filter.add_directive(noisy.parse().expect("valid directive"));
    }
    filter
}

fn log_max_bytes() -> u64 {
    std::env::var("EDGECRAB_LOG_MAX_MB")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_LOG_MB)
        * 1024
        * 1024
}

fn log_backup_count() -> usize {
    std::env::var("EDGECRAB_LOG_BACKUPS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_BACKUP_COUNT)
}

fn prepare_log_file_path(path: &Path, max_bytes: u64, backups: usize) -> anyhow::Result<PathBuf> {
    if let Ok(metadata) = fs::metadata(path)
        && metadata.len() >= max_bytes
    {
        rotate_backups(path, backups)
            .with_context(|| format!("failed rotating {}", path.display()))?;
    }

    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    Ok(path.to_path_buf())
}

fn make_file_writer(path: PathBuf) -> impl Fn() -> std::fs::File + Clone {
    move || {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap_or_else(|err| panic!("failed to open log file {}: {err}", path.display()))
    }
}

fn rotate_backups(path: &Path, backups: usize) -> std::io::Result<()> {
    if backups == 0 || !path.exists() {
        return Ok(());
    }

    let oldest = backup_path(path, backups);
    if oldest.exists() {
        fs::remove_file(oldest)?;
    }

    for idx in (1..=backups.saturating_sub(1)).rev() {
        let from = backup_path(path, idx);
        if from.exists() {
            fs::rename(&from, backup_path(path, idx + 1))?;
        }
    }

    fs::rename(path, backup_path(path, 1))
}

fn backup_path(path: &Path, idx: usize) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("log");
    path.with_file_name(format!("{file_name}.{idx}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_mode_uses_expected_file_names() {
        assert_eq!(LoggingMode::Agent.primary_log_name(), "agent.log");
        assert_eq!(LoggingMode::Gateway.primary_log_name(), "gateway.log");
        assert_eq!(LoggingMode::Acp.primary_log_name(), "acp.log");
        assert_eq!(LoggingMode::Agent.json_log_name(), "agent.jsonl");
    }

    #[test]
    fn backup_path_appends_numeric_suffix() {
        let path = Path::new("/tmp/agent.log");
        assert_eq!(backup_path(path, 2), PathBuf::from("/tmp/agent.log.2"));
    }

    #[test]
    fn rotate_backups_moves_current_file_to_first_backup() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("agent.log");
        fs::write(&path, "current").expect("seed current");

        rotate_backups(&path, 3).expect("rotate");

        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(temp.path().join("agent.log.1")).expect("backup"),
            "current"
        );
    }

    #[test]
    fn init_logging_writes_to_primary_log_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guards = init_logging(temp.path(), LoggingMode::Agent, false, StderrMode::Off)
            .expect("init logging");

        assert!(tracing::enabled!(tracing::Level::INFO));
        tracing::info!(
            test_case = "init_logging_writes_to_primary_log_file",
            "hello log"
        );

        let log_path = temp.path().join("logs").join("agent.log");
        let content = fs::read_to_string(&log_path).expect("read agent.log");
        assert!(
            content.contains("hello log"),
            "agent.log should contain the emitted event, got: {content:?}"
        );
    }
}
