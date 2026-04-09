use std::ffi::OsString;
use std::fmt;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, anyhow};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

const UPDATE_CHECK_FILE: &str = "update-check.json";
const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/raphaelmansuy/edgecrab/releases/latest";
const RELEASE_REQUEST_TIMEOUT_SECS: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallMethod {
    Npm,
    Pypi,
    Cargo,
    Brew,
    Source,
    Binary,
}

impl InstallMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pypi => "pypi",
            Self::Cargo => "cargo",
            Self::Brew => "brew",
            Self::Source => "source",
            Self::Binary => "binary",
        }
    }
}

impl fmt::Display for InstallMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PypiInstaller {
    Pip,
    Pipx,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallContext {
    pub method: InstallMethod,
    pub executable: PathBuf,
    pub canonical_executable: PathBuf,
    pub source_root: Option<PathBuf>,
    pub wrapper_package_version: Option<String>,
    pub wrapper_binary_version: Option<String>,
    pub pypi_installer: Option<PypiInstaller>,
    pub python_executable: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl ReleaseVersion {
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim().trim_start_matches('v');
        let core = trimmed.split_once('-').map_or(trimmed, |(head, _)| head);
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: ReleaseVersion,
    pub version_string: String,
    pub tag_name: String,
    pub html_url: String,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct UpdateStatus {
    pub install: InstallContext,
    pub current_version_string: String,
    pub latest_release: Option<ReleaseInfo>,
    pub newer_available: bool,
    pub checked_at: Option<DateTime<Utc>>,
    pub from_cache: bool,
    pub cache_stale: bool,
}

#[derive(Debug, Clone)]
pub struct StartupUpdateState {
    pub cached_notice: Option<String>,
    pub should_refresh: bool,
    pub known_latest_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RefreshOutcome {
    pub startup_notice: Option<String>,
}

#[derive(Debug, Clone)]
pub enum UpdatePlan {
    Managed { steps: Vec<CommandStep> },
    Guidance { details: String },
    NoUpdate,
}

#[derive(Debug, Clone)]
pub struct CommandStep {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedUpdateState {
    checked_at: i64,
    latest_release: Option<CachedReleaseInfo>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedReleaseInfo {
    version: String,
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubLatestRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
    prerelease: bool,
    draft: bool,
}

pub fn should_check_for_updates(config: &edgecrab_core::AppConfig) -> bool {
    config.display.check_for_updates
}

pub fn update_check_interval(config: &edgecrab_core::AppConfig) -> Duration {
    Duration::from_secs(config.display.update_check_interval_hours.max(1) * 60 * 60)
}

pub fn detect_install_context() -> InstallContext {
    let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("edgecrab"));
    let canonical_executable = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.clone());

    let method_from_env = std::env::var("EDGECRAB_INSTALL_METHOD")
        .ok()
        .and_then(|value| parse_install_method(&value));
    let pypi_installer = std::env::var("EDGECRAB_PYPI_INSTALLER")
        .ok()
        .and_then(|value| parse_pypi_installer(&value));
    let python_executable = std::env::var_os("EDGECRAB_PYTHON_EXECUTABLE").map(PathBuf::from);
    let wrapper_package_version = std::env::var("EDGECRAB_WRAPPER_VERSION").ok();
    let wrapper_binary_version = std::env::var("EDGECRAB_BINARY_VERSION").ok();

    let source_root = find_source_checkout_root(&canonical_executable);
    let method = method_from_env.unwrap_or_else(|| {
        infer_install_method(
            &canonical_executable,
            source_root.as_deref(),
            pypi_installer.is_some(),
        )
    });

    InstallContext {
        method,
        executable,
        canonical_executable,
        source_root,
        wrapper_package_version,
        wrapper_binary_version,
        pypi_installer,
        python_executable,
    }
}

pub fn cached_startup_state(config: &edgecrab_core::AppConfig) -> StartupUpdateState {
    if !should_check_for_updates(config) {
        return StartupUpdateState {
            cached_notice: None,
            should_refresh: false,
            known_latest_version: None,
        };
    }

    let install = detect_install_context();
    let current_version_string = env!("CARGO_PKG_VERSION").to_string();
    let current_version = ReleaseVersion::parse(&current_version_string)
        .expect("CARGO_PKG_VERSION is always valid semver");
    let interval = update_check_interval(config);

    match load_cached_update_state() {
        Some(cache) => {
            let checked_at = timestamp_to_utc(cache.checked_at);
            let cache_stale = checked_at
                .map(|ts| cache_is_stale(ts, interval))
                .unwrap_or(true);
            let latest_release = cache
                .latest_release
                .and_then(cached_release_to_release_info);
            let status = UpdateStatus {
                install,
                current_version_string,
                newer_available: latest_release
                    .as_ref()
                    .is_some_and(|release| release.version > current_version),
                latest_release,
                checked_at,
                from_cache: true,
                cache_stale,
            };
            StartupUpdateState {
                cached_notice: render_startup_notice(&status),
                should_refresh: cache_stale,
                known_latest_version: status
                    .latest_release
                    .as_ref()
                    .map(|release| release.version_string.clone()),
            }
        }
        None => StartupUpdateState {
            cached_notice: None,
            should_refresh: true,
            known_latest_version: None,
        },
    }
}

pub async fn refresh_update_status(
    config: &edgecrab_core::AppConfig,
    previous_latest_version: Option<&str>,
) -> anyhow::Result<RefreshOutcome> {
    let status = resolve_update_status(config, false).await?;
    let startup_notice = if status
        .latest_release
        .as_ref()
        .map(|release| release.version_string.as_str())
        != previous_latest_version
    {
        render_startup_notice(&status)
    } else {
        None
    };
    Ok(RefreshOutcome { startup_notice })
}

pub async fn resolve_update_status(
    config: &edgecrab_core::AppConfig,
    force_refresh: bool,
) -> anyhow::Result<UpdateStatus> {
    let install = detect_install_context();
    let current_version_string = env!("CARGO_PKG_VERSION").to_string();
    let current_version = ReleaseVersion::parse(&current_version_string)
        .expect("CARGO_PKG_VERSION is always valid semver");
    let interval = update_check_interval(config);

    if !force_refresh
        && let Some(cache) = load_cached_update_state()
        && let Some(checked_at) = timestamp_to_utc(cache.checked_at)
        && !cache_is_stale(checked_at, interval)
    {
        let latest_release = cache
            .latest_release
            .and_then(cached_release_to_release_info);
        return Ok(UpdateStatus {
            install,
            current_version_string,
            newer_available: latest_release
                .as_ref()
                .is_some_and(|release| release.version > current_version),
            latest_release,
            checked_at: Some(checked_at),
            from_cache: true,
            cache_stale: false,
        });
    }

    let fetched = fetch_latest_release().await;
    let checked_at = Utc::now();

    match fetched {
        Ok(latest_release) => {
            persist_cached_update_state(CachedUpdateState {
                checked_at: checked_at.timestamp(),
                latest_release: latest_release.as_ref().map(release_info_to_cached),
                last_error: None,
            })?;
            Ok(UpdateStatus {
                install,
                current_version_string,
                newer_available: latest_release
                    .as_ref()
                    .is_some_and(|release| release.version > current_version),
                latest_release,
                checked_at: Some(checked_at),
                from_cache: false,
                cache_stale: false,
            })
        }
        Err(fetch_error) => {
            if let Some(cache) = load_cached_update_state() {
                let latest_release = cache
                    .latest_release
                    .and_then(cached_release_to_release_info);
                let checked_at = timestamp_to_utc(cache.checked_at);
                Ok(UpdateStatus {
                    install,
                    current_version_string,
                    newer_available: latest_release
                        .as_ref()
                        .is_some_and(|release| release.version > current_version),
                    latest_release,
                    checked_at,
                    from_cache: true,
                    cache_stale: true,
                })
            } else {
                Err(fetch_error)
            }
        }
    }
}

pub fn render_update_report(status: &UpdateStatus) -> String {
    let mut lines = vec![
        format!("EdgeCrab v{}", status.current_version_string),
        format!("Install method: {}", status.install.method),
        format!(
            "Executable: {}",
            status.install.canonical_executable.display()
        ),
    ];

    if let Some(release) = &status.latest_release {
        lines.push(format!("Latest release: {}", release.version_string));
        if let Some(published_at) = release.published_at {
            lines.push(format!(
                "Published: {}",
                published_at
                    .with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M %Z")
            ));
        }
        lines.push(format!("Release URL: {}", release.html_url));
    } else {
        lines.push("Latest release: unavailable".into());
    }

    if let Some(checked_at) = status.checked_at {
        let source = if status.from_cache {
            if status.cache_stale {
                "cached, stale"
            } else {
                "cached"
            }
        } else {
            "live"
        };
        lines.push(format!(
            "Checked: {} ({source})",
            checked_at.with_timezone(&Local).format("%Y-%m-%d %H:%M %Z")
        ));
    }

    if !status.newer_available {
        lines.push("Status: up to date.".into());
        return lines.join("\n");
    }

    lines.push(format!(
        "Status: update available -> {}",
        status
            .latest_release
            .as_ref()
            .map(|release| release.version_string.as_str())
            .unwrap_or("?")
    ));

    match build_update_plan(status) {
        UpdatePlan::Managed { steps } => {
            lines.push("Action: run `edgecrab update` to apply it automatically.".into());
            lines.push("Planned command(s):".into());
            for step in steps {
                lines.push(format!("  {}", step.display));
            }
        }
        UpdatePlan::Guidance { details } => {
            lines.push("Action: manual update guidance.".into());
            lines.push(details);
        }
        UpdatePlan::NoUpdate => {}
    }

    lines.join("\n")
}

pub fn render_startup_notice(status: &UpdateStatus) -> Option<String> {
    if !status.newer_available {
        return None;
    }

    let latest = status.latest_release.as_ref()?;
    Some(format!(
        "Update available: EdgeCrab v{} -> v{}. Run `edgecrab update`.",
        status.current_version_string, latest.version_string
    ))
}

pub fn build_update_plan(status: &UpdateStatus) -> UpdatePlan {
    let Some(release) = &status.latest_release else {
        return UpdatePlan::Guidance {
            details: "Latest release information is unavailable. Re-run `edgecrab update --check` when network access is available.".into(),
        };
    };

    if !status.newer_available {
        return UpdatePlan::NoUpdate;
    }

    match status.install.method {
        InstallMethod::Npm => UpdatePlan::Managed {
            steps: vec![CommandStep {
                display: format!("npm install -g edgecrab-cli@{}", release.version_string),
                program: OsString::from("npm"),
                args: vec![
                    OsString::from("install"),
                    OsString::from("-g"),
                    OsString::from(format!("edgecrab-cli@{}", release.version_string)),
                ],
            }],
        },
        InstallMethod::Pypi => build_pypi_update_plan(&status.install, &release.version_string),
        InstallMethod::Cargo => UpdatePlan::Managed {
            steps: vec![CommandStep {
                display: format!(
                    "cargo install edgecrab-cli --locked --force --version {}",
                    release.version_string
                ),
                program: OsString::from("cargo"),
                args: vec![
                    OsString::from("install"),
                    OsString::from("edgecrab-cli"),
                    OsString::from("--locked"),
                    OsString::from("--force"),
                    OsString::from("--version"),
                    OsString::from(release.version_string.clone()),
                ],
            }],
        },
        InstallMethod::Brew => UpdatePlan::Managed {
            steps: vec![
                CommandStep {
                    display: "brew update".into(),
                    program: OsString::from("brew"),
                    args: vec![OsString::from("update")],
                },
                CommandStep {
                    display: "brew upgrade edgecrab".into(),
                    program: OsString::from("brew"),
                    args: vec![OsString::from("upgrade"), OsString::from("edgecrab")],
                },
            ],
        },
        InstallMethod::Source => UpdatePlan::Guidance {
            details: render_source_guidance(&status.install, release),
        },
        InstallMethod::Binary => UpdatePlan::Guidance {
            details: format!(
                "Download the latest archive from {}\nReplace the current binary in place after the command exits.",
                release.html_url
            ),
        },
    }
}

pub fn execute_update_plan(plan: &UpdatePlan) -> anyhow::Result<()> {
    match plan {
        UpdatePlan::Managed { steps } => {
            for step in steps {
                let status = Command::new(&step.program)
                    .args(&step.args)
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .with_context(|| format!("failed to start `{}`", step.display))?;
                if !status.success() {
                    return Err(anyhow!("command failed: {}", step.display));
                }
            }
            Ok(())
        }
        UpdatePlan::Guidance { .. } => Err(anyhow!(
            "automatic update is not available for this install method"
        )),
        UpdatePlan::NoUpdate => Ok(()),
    }
}

pub async fn run_update_command(
    config: &edgecrab_core::AppConfig,
    check_only: bool,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let status = resolve_update_status(config, true).await?;
    writeln!(out, "{}", render_update_report(&status))?;

    if check_only || !status.newer_available {
        return Ok(());
    }

    let plan = build_update_plan(&status);
    match &plan {
        UpdatePlan::Managed { .. } => {
            writeln!(out)?;
            writeln!(out, "Applying update...")?;
            execute_update_plan(&plan)?;
        }
        UpdatePlan::Guidance { details } => {
            writeln!(out)?;
            writeln!(out, "{details}")?;
        }
        UpdatePlan::NoUpdate => {}
    }
    Ok(())
}

pub fn print_cached_cli_notice(config: &edgecrab_core::AppConfig) {
    if !std::io::stderr().is_terminal() || !std::io::stdout().is_terminal() {
        return;
    }
    if let Some(notice) = cached_startup_state(config).cached_notice {
        eprintln!("{notice}");
    }
}

fn parse_install_method(value: &str) -> Option<InstallMethod> {
    match value.trim().to_ascii_lowercase().as_str() {
        "npm" => Some(InstallMethod::Npm),
        "pypi" | "pip" | "pipx" => Some(InstallMethod::Pypi),
        "cargo" => Some(InstallMethod::Cargo),
        "brew" | "homebrew" => Some(InstallMethod::Brew),
        "source" => Some(InstallMethod::Source),
        "binary" => Some(InstallMethod::Binary),
        _ => None,
    }
}

fn parse_pypi_installer(value: &str) -> Option<PypiInstaller> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pip" => Some(PypiInstaller::Pip),
        "pipx" => Some(PypiInstaller::Pipx),
        _ => None,
    }
}

fn infer_install_method(
    executable: &Path,
    source_root: Option<&Path>,
    has_pypi_hint: bool,
) -> InstallMethod {
    if path_looks_like_npm_install(executable) {
        return InstallMethod::Npm;
    }
    if has_pypi_hint || path_looks_like_pypi_install(executable) {
        return InstallMethod::Pypi;
    }
    if path_looks_like_homebrew_install(executable) {
        return InstallMethod::Brew;
    }
    if path_looks_like_cargo_install(executable) {
        return InstallMethod::Cargo;
    }
    if source_root.is_some() {
        return InstallMethod::Source;
    }
    InstallMethod::Binary
}

fn path_looks_like_npm_install(path: &Path) -> bool {
    path.to_string_lossy()
        .contains("node_modules/edgecrab-cli/bin/")
}

fn path_looks_like_pypi_install(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.contains("site-packages/edgecrab_cli/_bin/")
        || text.contains("dist-packages/edgecrab_cli/_bin/")
}

fn path_looks_like_homebrew_install(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.contains("/Cellar/edgecrab/") || text.contains("/Homebrew/Cellar/edgecrab/")
}

fn path_looks_like_cargo_install(path: &Path) -> bool {
    let cargo_bin = cargo_home_bin_dir();
    path.starts_with(&cargo_bin)
}

fn cargo_home_bin_dir() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".cargo")))
        .unwrap_or_else(|| PathBuf::from(".cargo"))
        .join("bin")
}

fn find_source_checkout_root(executable: &Path) -> Option<PathBuf> {
    for ancestor in executable.ancestors() {
        if ancestor.join(".git").exists() && ancestor.join("Cargo.toml").exists() {
            return Some(ancestor.to_path_buf());
        }
        if ancestor.file_name().is_some_and(|name| name == "target")
            && let Some(repo_root) = ancestor.parent()
            && repo_root.join(".git").exists()
            && repo_root.join("Cargo.toml").exists()
        {
            return Some(repo_root.to_path_buf());
        }
    }
    None
}

fn render_source_guidance(install: &InstallContext, release: &ReleaseInfo) -> String {
    if let Some(repo_root) = &install.source_root {
        let branch = git_output(repo_root, &["rev-parse", "--abbrev-ref", "HEAD"])
            .unwrap_or_else(|| "(unknown)".into());
        let commit = git_output(repo_root, &["rev-parse", "--short", "HEAD"])
            .unwrap_or_else(|| "(unknown)".into());
        let dirty = git_output(repo_root, &["status", "--short"])
            .map(|out| if out.is_empty() { "clean" } else { "dirty" })
            .unwrap_or("unknown");
        format!(
            "Source checkout detected.\nRepo: {}\nBranch: {}\nCommit: {} ({})\nNext steps:\n  git -C {} fetch --tags\n  git -C {} checkout v{}\n  cargo build --release -p edgecrab-cli",
            repo_root.display(),
            branch,
            commit,
            dirty,
            repo_root.display(),
            repo_root.display(),
            release.version_string,
        )
    } else {
        format!(
            "Source checkout guidance unavailable. Fetch the latest tag from {} and rebuild from source.",
            release.html_url
        )
    }
}

fn build_pypi_update_plan(install: &InstallContext, version: &str) -> UpdatePlan {
    match install.pypi_installer {
        Some(PypiInstaller::Pipx) => UpdatePlan::Managed {
            steps: vec![CommandStep {
                display: "pipx upgrade edgecrab-cli".into(),
                program: OsString::from("pipx"),
                args: vec![OsString::from("upgrade"), OsString::from("edgecrab-cli")],
            }],
        },
        _ => {
            let python = install
                .python_executable
                .clone()
                .unwrap_or_else(|| PathBuf::from("python3"));
            UpdatePlan::Managed {
                steps: vec![CommandStep {
                    display: format!(
                        "{} -m pip install --upgrade edgecrab-cli=={}",
                        python.display(),
                        version
                    ),
                    program: python.into_os_string(),
                    args: vec![
                        OsString::from("-m"),
                        OsString::from("pip"),
                        OsString::from("install"),
                        OsString::from("--upgrade"),
                        OsString::from(format!("edgecrab-cli=={version}")),
                    ],
                }],
            }
        }
    }
}

async fn fetch_latest_release() -> anyhow::Result<Option<ReleaseInfo>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(RELEASE_REQUEST_TIMEOUT_SECS))
        .build()
        .context("failed to build update-check HTTP client")?;

    let response = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header(reqwest::header::USER_AGENT, "edgecrab-updater")
        .send()
        .await
        .context("failed to fetch latest GitHub release")?
        .error_for_status()
        .context("latest GitHub release request failed")?;

    let payload: GithubLatestRelease = response
        .json()
        .await
        .context("failed to decode latest GitHub release payload")?;
    if payload.draft || payload.prerelease {
        return Ok(None);
    }

    let version = ReleaseVersion::parse(&payload.tag_name)
        .ok_or_else(|| anyhow!("invalid release tag: {}", payload.tag_name))?;
    let version_string = version.to_string();
    let published_at = payload
        .published_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));

    Ok(Some(ReleaseInfo {
        version,
        version_string,
        tag_name: payload.tag_name,
        html_url: payload.html_url,
        published_at,
    }))
}

fn cache_is_stale(checked_at: DateTime<Utc>, max_age: Duration) -> bool {
    Utc::now()
        .signed_duration_since(checked_at)
        .to_std()
        .map_or(true, |elapsed| elapsed >= max_age)
}

fn update_check_path() -> PathBuf {
    edgecrab_core::edgecrab_home().join(UPDATE_CHECK_FILE)
}

fn load_cached_update_state() -> Option<CachedUpdateState> {
    let path = update_check_path();
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn persist_cached_update_state(state: CachedUpdateState) -> anyhow::Result<()> {
    let path = update_check_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let raw =
        serde_json::to_string_pretty(&state).context("failed to serialize update-check state")?;
    std::fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn release_info_to_cached(release: &ReleaseInfo) -> CachedReleaseInfo {
    CachedReleaseInfo {
        version: release.version_string.clone(),
        tag_name: release.tag_name.clone(),
        html_url: release.html_url.clone(),
        published_at: release.published_at.map(|ts| ts.to_rfc3339()),
    }
}

fn cached_release_to_release_info(cached: CachedReleaseInfo) -> Option<ReleaseInfo> {
    let version = ReleaseVersion::parse(&cached.version)?;
    let published_at = cached
        .published_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    Some(ReleaseInfo {
        version,
        version_string: cached.version,
        tag_name: cached.tag_name,
        html_url: cached.html_url,
        published_at,
    })
}

fn timestamp_to_utc(value: i64) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(value, 0)
}

fn git_output(repo: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_version() {
        let version = ReleaseVersion::parse("v1.2.3").expect("parse version");
        assert_eq!(version.to_string(), "1.2.3");
    }

    #[test]
    fn rejects_invalid_release_version() {
        assert!(ReleaseVersion::parse("v1.2").is_none());
        assert!(ReleaseVersion::parse("foo").is_none());
    }

    #[test]
    fn prefers_homebrew_detection() {
        let method = infer_install_method(
            Path::new("/opt/homebrew/Cellar/edgecrab/0.1.4/bin/edgecrab"),
            None,
            false,
        );
        assert_eq!(method, InstallMethod::Brew);
    }

    #[test]
    fn detects_cargo_install_path() {
        let cargo_bin = cargo_home_bin_dir();
        let method = infer_install_method(&cargo_bin.join("edgecrab"), None, false);
        assert_eq!(method, InstallMethod::Cargo);
    }

    #[test]
    fn npm_update_plan_uses_pinned_package_version() {
        let status = sample_status(InstallMethod::Npm);
        match build_update_plan(&status) {
            UpdatePlan::Managed { steps } => {
                assert_eq!(steps.len(), 1);
                assert!(steps[0].display.contains("edgecrab-cli@0.2.0"));
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn pypi_pipx_update_plan_uses_pipx() {
        let mut status = sample_status(InstallMethod::Pypi);
        status.install.pypi_installer = Some(PypiInstaller::Pipx);
        match build_update_plan(&status) {
            UpdatePlan::Managed { steps } => {
                assert_eq!(steps[0].display, "pipx upgrade edgecrab-cli");
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn startup_notice_only_when_newer_release_exists() {
        let status = sample_status(InstallMethod::Cargo);
        let notice = render_startup_notice(&status).expect("startup notice");
        assert!(notice.contains("Update available"));

        let mut current = status.clone();
        current.newer_available = false;
        assert!(render_startup_notice(&current).is_none());
    }

    fn sample_status(method: InstallMethod) -> UpdateStatus {
        let latest_release = ReleaseInfo {
            version: ReleaseVersion::parse("0.2.0").expect("latest version"),
            version_string: "0.2.0".into(),
            tag_name: "v0.2.0".into(),
            html_url: "https://github.com/raphaelmansuy/edgecrab/releases/tag/v0.2.0".into(),
            published_at: None,
        };
        UpdateStatus {
            install: InstallContext {
                method,
                executable: PathBuf::from("/tmp/edgecrab"),
                canonical_executable: PathBuf::from("/tmp/edgecrab"),
                source_root: None,
                wrapper_package_version: None,
                wrapper_binary_version: None,
                pypi_installer: None,
                python_executable: None,
            },
            current_version_string: "0.1.4".into(),
            latest_release: Some(latest_release),
            newer_available: true,
            checked_at: None,
            from_cache: false,
            cache_stale: false,
        }
    }
}
