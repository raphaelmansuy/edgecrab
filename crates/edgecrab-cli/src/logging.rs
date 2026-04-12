use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use anyhow::Context;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::{EnvFilter, fmt, util::SubscriberInitExt};

const DEFAULT_MAX_LOG_MB: u64 = 10;
const DEFAULT_BACKUP_COUNT: usize = 5;
const DEFAULT_LOG_TAIL_LINES: usize = 120;

static LOG_FILTER_RELOAD: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> =
    OnceLock::new();

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevelSetting {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevelSetting {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" | "err" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN ",
            Self::Info => "INFO ",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }

    fn to_filter(self) -> LevelFilter {
        match self {
            Self::Error => LevelFilter::ERROR,
            Self::Warn => LevelFilter::WARN,
            Self::Info => LevelFilter::INFO,
            Self::Debug => LevelFilter::DEBUG,
            Self::Trace => LevelFilter::TRACE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogFileInfo {
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
    pub modified: Option<SystemTime>,
}

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

    let base_filter = base_env_filter(home, debug);
    let (filter_layer, reload_handle) = reload::Layer::new(base_filter);
    let _ = LOG_FILTER_RELOAD.set(reload_handle);
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
        .with(filter_layer)
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

pub fn logs_dir_for(home: &Path) -> PathBuf {
    home.join("logs")
}

pub fn default_log_level(home: &Path) -> LogLevelSetting {
    let config_path = home.join("config.yaml");
    if config_path.exists() {
        edgecrab_core::AppConfig::load_from(&config_path)
            .ok()
            .and_then(|config| LogLevelSetting::parse(&config.logging.level))
            .unwrap_or_default()
    } else {
        LogLevelSetting::default()
    }
}

pub fn effective_log_level(home: &Path, debug: bool) -> LogLevelSetting {
    let saved = std::env::var("EDGECRAB_LOG_LEVEL")
        .ok()
        .and_then(|value| LogLevelSetting::parse(&value))
        .unwrap_or_else(|| default_log_level(home));
    if debug {
        saved.max(LogLevelSetting::Debug)
    } else {
        saved
    }
}

pub fn persist_log_level(home: &Path, level: LogLevelSetting) -> anyhow::Result<()> {
    let config_path = home.join("config.yaml");
    let mut config = if config_path.exists() {
        edgecrab_core::AppConfig::load_from(&config_path).unwrap_or_default()
    } else {
        edgecrab_core::AppConfig::default()
    };
    config.logging.level = level.as_str().to_string();
    config
        .save_to(&config_path)
        .context("failed to persist logging level")
}

pub fn reload_runtime_log_level(level: LogLevelSetting) -> anyhow::Result<bool> {
    let Some(handle) = LOG_FILTER_RELOAD.get() else {
        return Ok(false);
    };
    handle
        .reload(base_filter_for_level(level))
        .context("failed to reload runtime logging filter")?;
    Ok(true)
}

pub fn list_log_files(home: &Path) -> anyhow::Result<Vec<LogFileInfo>> {
    let dir = logs_dir_for(home);
    let mut entries = Vec::new();
    if !dir.exists() {
        return Ok(entries);
    }

    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let metadata = entry.metadata().ok();
        entries.push(LogFileInfo {
            name: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string(),
            path,
            size_bytes: metadata.as_ref().map_or(0, std::fs::Metadata::len),
            modified: metadata.and_then(|meta| meta.modified().ok()),
        });
    }

    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

pub fn resolve_log_path(home: &Path, name: Option<&str>) -> anyhow::Result<PathBuf> {
    let paths = list_log_files(home)?
        .into_iter()
        .map(|entry| entry.path)
        .collect::<Vec<_>>();
    if paths.is_empty() {
        anyhow::bail!("No log files found in {}", logs_dir_for(home).display());
    }

    if let Some(name) = name {
        let normalized = name.trim_end_matches(".log");
        let mut matches = paths
            .into_iter()
            .filter(|path| {
                let stem = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default();
                let file = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default();
                stem == normalized || file == name || stem.starts_with(normalized)
            })
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        return match matches.len() {
            0 => Err(anyhow::anyhow!("No log file matching '{name}'.")),
            1 => Ok(matches.remove(0)),
            _ => Err(anyhow::anyhow!(
                "Ambiguous log name '{name}'. Matches: {}",
                matches
                    .iter()
                    .map(|path| path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        };
    }

    for preferred in [
        logs_dir_for(home).join("gateway.log"),
        logs_dir_for(home).join("agent.log"),
        logs_dir_for(home).join("acp.log"),
        logs_dir_for(home).join("errors.log"),
    ] {
        if preferred.exists() {
            return Ok(preferred);
        }
    }

    if paths.len() == 1 {
        return Ok(paths[0].clone());
    }

    Err(anyhow::anyhow!(
        "Multiple log files found. Use `edgecrab logs list` or specify one by name."
    ))
}

pub fn read_file_text(path: &Path) -> anyhow::Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn read_last_lines(path: &Path, lines: usize) -> anyhow::Result<String> {
    #[cfg(unix)]
    {
        let output = std::process::Command::new("tail")
            .arg("-n")
            .arg(lines.to_string())
            .arg(path)
            .output();
        if let Ok(output) = output
            && output.status.success()
        {
            return String::from_utf8(output.stdout)
                .map_err(|err| anyhow::anyhow!("failed to decode {}: {err}", path.display()));
        }
    }

    let content = read_file_text(path)?;
    let total = content.lines().count();
    let skip = total.saturating_sub(lines);
    Ok(content.lines().skip(skip).collect::<Vec<_>>().join("\n"))
}

pub fn tail_preview(path: &Path, lines: usize) -> anyhow::Result<String> {
    let requested = lines.max(1);
    let content = read_last_lines(path, requested)?;
    if content.trim().is_empty() {
        return Ok("(log file is currently empty)".into());
    }
    Ok(content)
}

pub fn format_log_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

pub fn default_tail_lines() -> usize {
    DEFAULT_LOG_TAIL_LINES
}

fn base_env_filter(home: &Path, debug: bool) -> EnvFilter {
    let default_level = effective_log_level(home, debug).to_filter();
    // WHY ignore ambient RUST_LOG here: persistent EdgeCrab file logs should
    // stay debuggable even when the parent shell exports restrictive filters
    // like `RUST_LOG=warn`. EdgeCrab uses its own override knob instead.
    let filter = if let Ok(spec) = std::env::var("EDGECRAB_LOG_FILTER") {
        EnvFilter::builder()
            .with_default_directive(default_level.into())
            .parse_lossy(spec)
    } else {
        base_filter_for_level(effective_log_level(home, debug))
    };
    add_noisy_module_directives(filter)
}

fn base_filter_for_level(level: LogLevelSetting) -> EnvFilter {
    let filter = EnvFilter::builder()
        .with_default_directive(level.to_filter().into())
        .parse_lossy("");
    add_noisy_module_directives(filter)
}

fn add_noisy_module_directives(mut filter: EnvFilter) -> EnvFilter {
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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|err| {
                panic!("failed to create log directory {}: {err}", parent.display())
            });
        }
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
    fn log_level_setting_parses_aliases() {
        assert_eq!(
            LogLevelSetting::parse("warning"),
            Some(LogLevelSetting::Warn)
        );
        assert_eq!(
            LogLevelSetting::parse("DEBUG"),
            Some(LogLevelSetting::Debug)
        );
        assert_eq!(LogLevelSetting::parse("nope"), None);
    }

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

    #[test]
    fn persist_log_level_round_trips_through_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        persist_log_level(temp.path(), LogLevelSetting::Trace).expect("persist");
        assert_eq!(default_log_level(temp.path()), LogLevelSetting::Trace);
    }

    #[test]
    fn list_log_files_sorts_entries_by_name() {
        let temp = tempfile::tempdir().expect("tempdir");
        let log_dir = logs_dir_for(temp.path());
        fs::create_dir_all(&log_dir).expect("create logs");
        fs::write(log_dir.join("zeta.log"), "z").expect("write zeta");
        fs::write(log_dir.join("alpha.log"), "a").expect("write alpha");

        let files = list_log_files(temp.path()).expect("list files");
        assert_eq!(
            files.into_iter().map(|file| file.name).collect::<Vec<_>>(),
            vec!["alpha.log", "zeta.log"]
        );
    }

    #[test]
    fn resolve_log_path_prefers_agent_when_present() {
        let temp = tempfile::tempdir().expect("tempdir");
        let log_dir = logs_dir_for(temp.path());
        fs::create_dir_all(&log_dir).expect("create logs");
        fs::write(log_dir.join("agent.log"), "agent").expect("write agent");
        fs::write(log_dir.join("errors.log"), "errors").expect("write errors");

        let resolved = resolve_log_path(temp.path(), None).expect("resolve default");
        assert_eq!(resolved, log_dir.join("agent.log"));
    }
}
