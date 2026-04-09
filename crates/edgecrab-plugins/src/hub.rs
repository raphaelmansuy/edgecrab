use std::cmp::Ordering;
use std::path::{Path, PathBuf};
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
        description: "Hermes-compatible plugins and skills",
    },
    CuratedSource {
        name: "community",
        url: "https://plugins.edgecrab.sh/index.json",
        trust_level: TrustLevel::Community,
        description: "EdgeCrab community plugin directory",
    },
];

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
}

#[derive(Debug, Clone)]
pub struct ResolvedInstallSource {
    pub kind: InstallSourceKind,
    pub display: String,
    pub trust_level: TrustLevel,
    pub plugin_name_hint: String,
}

pub async fn search_hub(
    config: &PluginsConfig,
    query: &str,
    limit: usize,
) -> Result<Vec<PluginSearchResult>, PluginError> {
    let sources = configured_sources(config);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| PluginError::Hub(error.to_string()))?;

    let mut results = Vec::new();
    for source in sources {
        let index = fetch_index(config, &client, &source).await?;
        for plugin in index.plugins {
            let score = search_score(query, &plugin);
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
    results.truncate(limit);
    Ok(results)
}

pub fn clear_hub_cache(config: &PluginsConfig) -> Result<usize, PluginError> {
    let dir = hub_cache_dir(config);
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            std::fs::remove_file(path)?;
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
    if trimmed.starts_with("github:") || looks_like_github_source(trimmed) {
        let path = trimmed.trim_start_matches("github:");
        return ResolvedInstallSource {
            kind: InstallSourceKind::GitHub,
            display: format!("github:{path}"),
            trust_level: TrustLevel::Unverified,
            plugin_name_hint: path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("plugin")
                .to_string(),
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
        trust_level: TrustLevel::Unverified,
        plugin_name_hint,
    }
}

pub async fn materialize_source_to_dir(
    source: &str,
    destination: &Path,
) -> Result<ResolvedInstallSource, PluginError> {
    let resolved = resolve_install_source(source);
    match resolved.kind {
        InstallSourceKind::Local => {
            let raw = resolved.display.trim_start_matches("local:");
            copy_dir(Path::new(raw), destination)?;
        }
        InstallSourceKind::GitHub => {
            let github_path = resolved.display.trim_start_matches("github:");
            download_github_tree(github_path, destination).await?;
        }
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
        .map_err(|error| PluginError::Hub(error.to_string()))?;
    let response = response
        .error_for_status()
        .map_err(|error| PluginError::Hub(error.to_string()))?;
    let index = response
        .json::<HubIndex>()
        .await
        .map_err(|error| PluginError::Hub(error.to_string()))?;
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cached = CachedIndex {
        fetched_at: chrono::Utc::now().timestamp(),
        index: index.clone(),
    };
    let json =
        serde_json::to_string(&cached).map_err(|error| PluginError::Hub(error.to_string()))?;
    std::fs::write(cache_path, json)?;
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

    let client = reqwest::Client::builder()
        .user_agent("edgecrab-plugin-hub")
        .build()
        .map_err(|error| PluginError::Hub(error.to_string()))?;
    let mut pending = vec![(root.to_string(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        if depth > 2 {
            continue;
        }
        let url = format!("https://api.github.com/repos/{owner}/{repo}/contents/{dir}");
        let entries = client
            .get(url)
            .send()
            .await
            .map_err(|error| PluginError::Hub(error.to_string()))?
            .error_for_status()
            .map_err(|error| PluginError::Hub(error.to_string()))?
            .json::<Vec<GitHubEntry>>()
            .await
            .map_err(|error| PluginError::Hub(error.to_string()))?;

        for entry in entries {
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
                    let bytes = client
                        .get(
                            entry
                                .download_url
                                .ok_or_else(|| PluginError::Hub("missing download url".into()))?,
                        )
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

#[derive(Debug, Deserialize)]
struct GitHubEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    download_url: Option<String>,
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
            .to_string();
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
}
