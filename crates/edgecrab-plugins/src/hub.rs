use std::cmp::Ordering;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::{HubSource, PluginsConfig};
use crate::error::PluginError;
use crate::types::{PluginKind, TrustLevel};

const CURATED_SOURCES: &[CuratedSource] = &[
    CuratedSource {
        name: "edgecrab-official",
        url: "https://raw.githubusercontent.com/edgecrab/plugins/main/index.json",
        trust_level: TrustLevel::Official,
        description: "Official EdgeCrab plugin registry",
    },
    CuratedSource {
        name: "hermes-plugins",
        url: "https://raw.githubusercontent.com/hermes-agent/plugins/main/index.json",
        trust_level: TrustLevel::Official,
        description: "Hermes Agent compatible plugins and skills",
    },
    CuratedSource {
        name: "clawhub",
        url: "https://clawhub.io/index.json",
        trust_level: TrustLevel::Community,
        description: "ClaWHub plugin directory",
    },
    CuratedSource {
        name: "claude-marketplace",
        url: "https://marketplace.claude.ai/index.json",
        trust_level: TrustLevel::Community,
        description: "Claude marketplace plugin directory",
    },
    CuratedSource {
        name: "lobehub",
        url: "https://lobehub.com/mcp/plugins-store/index.json",
        trust_level: TrustLevel::Community,
        description: "LobeHub plugin registry",
    },
    CuratedSource {
        name: "community",
        url: "https://plugins.edgecrab.sh/index.json",
        trust_level: TrustLevel::Community,
        description: "EdgeCrab community plugin directory",
    },
];

const GITHUB_SKIP_NAMES: &[&str] = &[".", "..", ".git", ".github", ".hub"];
const MAX_ZIP_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct CuratedSource {
    pub name: &'static str,
    pub url: &'static str,
    pub trust_level: TrustLevel,
    pub description: &'static str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HubIndex {
    pub version: String,
    pub generated_at: String,
    pub source_name: String,
    #[serde(default)]
    pub plugins: Vec<HubIndexPlugin>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HubIndexPlugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub kind: PluginKind,
    pub install_url: String,
    pub checksum: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub requires_env: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PluginSearchResult {
    pub source_name: String,
    pub trust_level: TrustLevel,
    pub plugin: HubIndexPlugin,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAuditEntry {
    pub timestamp: String,
    pub action: String,
    pub plugin: String,
    pub source: String,
    pub trust_level: TrustLevel,
    pub checksum: String,
    pub forced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallSourceKind {
    GitHub,
    Local,
    HttpsArchive,
    Hub,
}

#[derive(Debug, Clone)]
pub struct ResolvedInstallSource {
    pub kind: InstallSourceKind,
    pub display: String,
    pub materialized_source: String,
    pub trust_level: TrustLevel,
    pub plugin_name_hint: String,
    pub expected_checksum: Option<String>,
    pub source_name: Option<String>,
}

pub async fn search_hub(
    config: &PluginsConfig,
    query: &str,
    source_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<PluginSearchResult>, PluginError> {
    let sources = configured_sources(config);
    let client = hub_client()?;
    let mut results = Vec::new();
    for source in &sources {
        if !source_matches_filter(source_filter, &source.name) {
            continue;
        }
        let index = fetch_index(config, &client, source).await?;
        for plugin in index.plugins {
            let mut score = search_score(query, &plugin);
            if matches!(source.trust_level, TrustLevel::Official) {
                score += 0.5;
            }
            if score > 0.0 {
                results.push(PluginSearchResult {
                    source_name: source.name.clone(),
                    trust_level: source.trust_level,
                    plugin,
                    score,
                });
            }
        }
    }

    results.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.plugin.name.cmp(&right.plugin.name))
    });
    results.dedup_by(|left, right| left.plugin.name == right.plugin.name);
    results.truncate(limit);
    Ok(results)
}

pub fn hub_source_names(config: &PluginsConfig) -> Vec<String> {
    configured_sources(config)
        .into_iter()
        .map(|source| source.name)
        .collect()
}

pub fn clear_hub_cache(config: &PluginsConfig) -> Result<usize, PluginError> {
    let dir = hub_cache_dir(config);
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().is_file() {
            std::fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

pub fn append_audit_entry(
    config: &PluginsConfig,
    entry: &PluginAuditEntry,
) -> Result<(), PluginError> {
    let path = audit_log_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(entry).map_err(|error| PluginError::Hub(error.to_string()))?
    )?;
    Ok(())
}

pub fn read_audit_entries(
    config: &PluginsConfig,
    lines: usize,
) -> Result<Vec<PluginAuditEntry>, PluginError> {
    let path = audit_log_path(config);
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut entries = Vec::new();
    for line in content.lines().rev().take(lines) {
        if let Ok(entry) = serde_json::from_str(line) {
            entries.push(entry);
        }
    }
    entries.reverse();
    Ok(entries)
}

pub fn resolve_install_source(source: &str) -> ResolvedInstallSource {
    let trimmed = source.trim();
    if let Some(rest) = trimmed.strip_prefix("hub:") {
        let plugin_name_hint = rest
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("plugin")
            .to_string();
        return ResolvedInstallSource {
            kind: InstallSourceKind::Hub,
            display: trimmed.to_string(),
            materialized_source: trimmed.to_string(),
            trust_level: TrustLevel::Community,
            plugin_name_hint,
            expected_checksum: None,
            source_name: None,
        };
    }
    if trimmed.starts_with("https://") {
        let plugin_name_hint = trimmed
            .rsplit('/')
            .next()
            .unwrap_or("plugin.zip")
            .trim_end_matches(".zip")
            .to_string();
        return ResolvedInstallSource {
            kind: InstallSourceKind::HttpsArchive,
            display: trimmed.to_string(),
            materialized_source: trimmed.to_string(),
            trust_level: TrustLevel::Unverified,
            plugin_name_hint,
            expected_checksum: None,
            source_name: None,
        };
    }
    if trimmed.starts_with("github:") || looks_like_github_source(trimmed) {
        let path = trimmed.trim_start_matches("github:");
        return ResolvedInstallSource {
            kind: InstallSourceKind::GitHub,
            display: format!("github:{path}"),
            materialized_source: format!("github:{path}"),
            trust_level: TrustLevel::Unverified,
            plugin_name_hint: path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("plugin")
                .to_string(),
            expected_checksum: None,
            source_name: None,
        };
    }

    let local = trimmed.trim_start_matches("local:");
    let plugin_name_hint = Path::new(local)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("plugin")
        .to_string();
    ResolvedInstallSource {
        kind: InstallSourceKind::Local,
        display: trimmed.to_string(),
        materialized_source: trimmed.to_string(),
        trust_level: TrustLevel::Unverified,
        plugin_name_hint,
        expected_checksum: None,
        source_name: None,
    }
}

pub async fn materialize_source_to_dir(
    config: &PluginsConfig,
    source: &str,
    destination: &Path,
) -> Result<ResolvedInstallSource, PluginError> {
    let parsed = resolve_install_source(source);
    let resolved = match parsed.kind {
        InstallSourceKind::Hub => resolve_hub_source(config, parsed.display.as_str()).await?,
        _ => parsed,
    };

    match resolved.kind {
        InstallSourceKind::Local => {
            let raw = resolved.materialized_source.trim_start_matches("local:");
            copy_dir(Path::new(raw), destination)?;
        }
        InstallSourceKind::GitHub => {
            download_github_tree(
                resolved.materialized_source.trim_start_matches("github:"),
                destination,
            )
            .await?;
        }
        InstallSourceKind::HttpsArchive => {
            download_https_archive(&resolved.materialized_source, destination).await?;
        }
        InstallSourceKind::Hub => unreachable!("hub sources are resolved before download"),
    }

    Ok(resolved)
}

pub fn sha256_dir(dir: &Path) -> Result<String, PluginError> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    for (relative, path) in files {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(std::fs::read(path)?);
        hasher.update([0xff]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn configured_sources(config: &PluginsConfig) -> Vec<RuntimeSource> {
    let mut sources: Vec<RuntimeSource> = CURATED_SOURCES
        .iter()
        .map(|source| RuntimeSource {
            name: source.name.to_string(),
            url: source.url.to_string(),
            trust_level: source.trust_level,
        })
        .collect();
    for source in &config.hub.sources {
        sources.push(runtime_source_from_hub(source));
    }
    sources
}

fn runtime_source_from_hub(source: &HubSource) -> RuntimeSource {
    RuntimeSource {
        name: source.name.clone(),
        url: source.url.clone(),
        trust_level: source.trust_override.unwrap_or(TrustLevel::Community),
    }
}

#[derive(Debug, Clone)]
struct RuntimeSource {
    name: String,
    url: String,
    trust_level: TrustLevel,
}

fn source_matches_filter(filter: Option<&str>, source_name: &str) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let normalized_filter = normalize_source_name(filter);
    let normalized_source = normalize_source_name(source_name);
    normalized_filter == normalized_source
        || normalized_source.contains(&normalized_filter)
        || normalized_filter.contains(&normalized_source)
}

fn normalize_source_name(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "hermes" | "hermes-plugin" | "hermes-plugins" => "hermesplugins".into(),
        other => other
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect(),
    }
}

async fn resolve_hub_source(
    config: &PluginsConfig,
    source: &str,
) -> Result<ResolvedInstallSource, PluginError> {
    let spec = source.trim_start_matches("hub:");
    let (source_name, plugin_name) = spec
        .split_once('/')
        .ok_or_else(|| PluginError::Hub("hub source must be hub:<source>/<plugin>".into()))?;
    let runtime_source = configured_sources(config)
        .into_iter()
        .find(|candidate| candidate.name == source_name)
        .ok_or_else(|| PluginError::Hub(format!("unknown plugin hub source: {source_name}")))?;
    let client = hub_client()?;
    let index = fetch_index(config, &client, &runtime_source).await?;
    let plugin = index
        .plugins
        .into_iter()
        .find(|candidate| candidate.name == plugin_name)
        .ok_or_else(|| PluginError::Hub(format!("plugin '{plugin_name}' not found in {source_name}")))?;
    Ok(ResolvedInstallSource {
        kind: resolve_install_source(&plugin.install_url).kind,
        display: format!("hub:{source_name}/{}", plugin.name),
        materialized_source: plugin.install_url.clone(),
        trust_level: runtime_source.trust_level,
        plugin_name_hint: plugin.name,
        expected_checksum: Some(plugin.checksum),
        source_name: Some(source_name.to_string()),
    })
}

async fn fetch_index(
    config: &PluginsConfig,
    client: &reqwest::Client,
    source: &RuntimeSource,
) -> Result<HubIndex, PluginError> {
    let cache_path = hub_cache_dir(config).join(format!("{}.json", source.name));
    if let Ok(content) = std::fs::read_to_string(&cache_path) {
        if let Ok(cached) = serde_json::from_str::<CachedIndex>(&content) {
            let age = chrono::Utc::now().timestamp() - cached.fetched_at;
            if age <= config.hub.cache_ttl_secs as i64 {
                return Ok(cached.index);
            }
        }
    }

    let response = client
        .get(&source.url)
        .send()
        .await
        .map_err(|error| PluginError::Hub(error.to_string()))?
        .error_for_status()
        .map_err(|error| PluginError::Hub(error.to_string()))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| PluginError::Hub(error.to_string()))?;
    let index = serde_json::from_slice::<HubIndex>(&bytes)
        .map_err(|error| PluginError::Hub(error.to_string()))?;

    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cached = CachedIndex {
        fetched_at: chrono::Utc::now().timestamp(),
        index: index.clone(),
    };
    std::fs::write(
        cache_path,
        serde_json::to_vec(&cached).map_err(|error| PluginError::Hub(error.to_string()))?,
    )?;
    Ok(index)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedIndex {
    fetched_at: i64,
    index: HubIndex,
}

fn search_score(query: &str, plugin: &HubIndexPlugin) -> f32 {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return 1.0;
    }
    let mut score = 0.0;
    let name = plugin.name.to_ascii_lowercase();
    let desc = plugin.description.to_ascii_lowercase();
    if name.contains(&q) {
        score += 2.0;
    }
    if desc.contains(&q) {
        score += 1.0;
    }
    for tag in &plugin.tags {
        if tag.to_ascii_lowercase().contains(&q) {
            score += 1.5;
        }
    }
    score
}

fn hub_client() -> Result<reqwest::Client, PluginError> {
    reqwest::Client::builder()
        .user_agent("edgecrab-plugin-hub/0.2")
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| PluginError::Hub(error.to_string()))
}

fn hub_cache_dir(config: &PluginsConfig) -> PathBuf {
    config.install_dir.join(".hub").join("cache")
}

fn audit_log_path(config: &PluginsConfig) -> PathBuf {
    config.install_dir.join(".hub").join("audit.log")
}

fn looks_like_github_source(source: &str) -> bool {
    if source.starts_with('.') || source.starts_with('/') || source.starts_with('~') {
        return false;
    }
    let parts: Vec<&str> = source.split('/').collect();
    parts.len() >= 3 && !source.contains("://") && parts.iter().all(|part| !part.is_empty())
}

async fn download_github_tree(path: &str, destination: &Path) -> Result<(), PluginError> {
    let path = path.trim_matches('/');
    let mut parts = path.splitn(3, '/');
    let owner = parts
        .next()
        .ok_or_else(|| PluginError::Hub("github source missing owner".into()))?;
    let repo = parts
        .next()
        .ok_or_else(|| PluginError::Hub("github source missing repo".into()))?;
    let root = parts
        .next()
        .ok_or_else(|| PluginError::Hub("github source missing path".into()))?;

    let client = hub_client()?;
    let mut pending = vec![(root.to_string(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        if depth > 2 {
            continue;
        }
        let url = format!("https://api.github.com/repos/{owner}/{repo}/contents/{dir}");
        let mut request = client.get(url);
        if let Some(token) = resolve_github_token() {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let entries = request
            .send()
            .await
            .map_err(|error| PluginError::Hub(error.to_string()))?
            .error_for_status()
            .map_err(|error| PluginError::Hub(error.to_string()))?
            .json::<Vec<GitHubEntry>>()
            .await
            .map_err(|error| PluginError::Hub(error.to_string()))?;

        for entry in entries {
            if GITHUB_SKIP_NAMES.contains(&entry.name.as_str()) {
                continue;
            }
            match entry.kind.as_str() {
                "file" => {
                    let relative = entry
                        .path
                        .strip_prefix(root)
                        .unwrap_or(entry.path.as_str())
                        .trim_start_matches('/');
                    let target = destination.join(relative);
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let mut download = client.get(entry.download_url.ok_or_else(|| {
                        PluginError::Hub("missing GitHub download_url".into())
                    })?);
                    if let Some(token) = resolve_github_token() {
                        download = download.header("Authorization", format!("Bearer {token}"));
                    }
                    let bytes = download
                        .send()
                        .await
                        .map_err(|error| PluginError::Hub(error.to_string()))?
                        .error_for_status()
                        .map_err(|error| PluginError::Hub(error.to_string()))?
                        .bytes()
                        .await
                        .map_err(|error| PluginError::Hub(error.to_string()))?;
                    std::fs::write(target, bytes)?;
                }
                "dir" => pending.push((entry.path, depth + 1)),
                _ => {}
            }
        }
    }
    Ok(())
}

async fn download_https_archive(url: &str, destination: &Path) -> Result<(), PluginError> {
    let client = hub_client()?;
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|error| PluginError::Hub(error.to_string()))?
        .error_for_status()
        .map_err(|error| PluginError::Hub(error.to_string()))?
        .bytes()
        .await
        .map_err(|error| PluginError::Hub(error.to_string()))?;
    if bytes.len() > MAX_ZIP_BYTES {
        return Err(PluginError::Hub(format!(
            "archive exceeds maximum size: {} bytes",
            bytes.len()
        )));
    }

    let cursor = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|error| PluginError::Hub(error.to_string()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| PluginError::Hub(error.to_string()))?;
        let Some(path) = entry.enclosed_name().map(|path| path.to_path_buf()) else {
            continue;
        };
        let components = path.components().collect::<Vec<_>>();
        let relative = if components.len() > 1 {
            components[1..].iter().collect::<PathBuf>()
        } else {
            path.clone()
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(target)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct GitHubEntry {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    path: String,
    download_url: Option<String>,
}

fn resolve_github_token() -> Option<String> {
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        return Some(token);
    }
    if let Ok(output) = Command::new("gh").args(["auth", "token"]).output()
        && output.status.success()
    {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }
    None
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), PluginError> {
    if !source.is_dir() {
        return Err(PluginError::Hub(format!(
            "local plugin source is not a directory: {}",
            source.display()
        )));
    }
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else if path.is_file() {
            std::fs::copy(path, target)?;
        }
    }
    Ok(())
}

fn collect_files(
    root: &Path,
    dir: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), PluginError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| PluginError::Hub(error.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        files.push((relative, path));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_github_sources() {
        let resolved = resolve_install_source("owner/repo/path/to/plugin");
        assert_eq!(resolved.kind, InstallSourceKind::GitHub);
        assert_eq!(resolved.plugin_name_hint, "plugin");
    }

    #[test]
    fn resolves_local_sources() {
        let resolved = resolve_install_source("./plugins/demo");
        assert_eq!(resolved.kind, InstallSourceKind::Local);
        assert_eq!(resolved.plugin_name_hint, "demo");
    }

    #[test]
    fn resolves_hub_sources() {
        let resolved = resolve_install_source("hub:community/demo");
        assert_eq!(resolved.kind, InstallSourceKind::Hub);
        assert_eq!(resolved.plugin_name_hint, "demo");
    }

    #[test]
    fn source_filter_matches_hermes_aliases() {
        assert!(source_matches_filter(Some("hermes"), "hermes-plugins"));
        assert!(source_matches_filter(Some("hermes-plugins"), "hermes-plugins"));
        assert!(!source_matches_filter(Some("edgecrab"), "hermes-plugins"));
    }
}
