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
        url: "https://github.com/raphaelmansuy/edgecrab",
        trust_level: TrustLevel::Official,
        description: "Official EdgeCrab plugin registry",
    },
    CuratedSource {
        name: "hermes-plugins",
        url: "https://github.com/NousResearch/hermes-agent",
        trust_level: TrustLevel::Official,
        description: "Hermes Agent compatible plugins",
    },
    CuratedSource {
        name: "hermes-evey",
        url: "https://github.com/42-evey/hermes-plugins",
        trust_level: TrustLevel::Community,
        description: "Community Hermes plugins from 42-evey",
    },
];

const GITHUB_SKIP_NAMES: &[&str] = &[".", "..", ".git", ".github", ".hub"];
const MAX_ZIP_BYTES: usize = 32 * 1024 * 1024;

const EDGECRAB_OFFICIAL_PLUGIN_ROOTS: &[RepoCatalogRoot] = &[RepoCatalogRoot {
    kind: PluginKind::Hermes,
    location: RepoCatalogLocation::Tree("plugins"),
}];

const EDGECRAB_OFFICIAL_ALL_ROOTS: &[RepoCatalogRoot] = &[
    RepoCatalogRoot {
        kind: PluginKind::Skill,
        location: RepoCatalogLocation::Tree("skills"),
    },
    RepoCatalogRoot {
        kind: PluginKind::Skill,
        location: RepoCatalogLocation::Tree("optional-skills"),
    },
    RepoCatalogRoot {
        kind: PluginKind::Hermes,
        location: RepoCatalogLocation::Tree("plugins"),
    },
];

const HERMES_ALL_ROOTS: &[RepoCatalogRoot] = &[
    RepoCatalogRoot {
        kind: PluginKind::Skill,
        location: RepoCatalogLocation::Tree("skills"),
    },
    RepoCatalogRoot {
        kind: PluginKind::Skill,
        location: RepoCatalogLocation::Tree("optional-skills"),
    },
    RepoCatalogRoot {
        kind: PluginKind::Hermes,
        location: RepoCatalogLocation::Tree("plugins"),
    },
];

const HERMES_PLUGIN_ROOTS: &[RepoCatalogRoot] = &[RepoCatalogRoot {
    kind: PluginKind::Hermes,
    location: RepoCatalogLocation::Tree("plugins"),
}];

const HERMES_EVEY_ROOTS: &[RepoCatalogRoot] = &[RepoCatalogRoot {
    kind: PluginKind::Hermes,
    location: RepoCatalogLocation::RepoRoot,
}];

#[derive(Debug, Clone, Copy)]
pub struct CuratedSource {
    pub name: &'static str,
    pub url: &'static str,
    pub trust_level: TrustLevel,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct RepoBackedSource {
    repo: &'static str,
    roots: &'static [RepoCatalogRoot],
    shared_root_files: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
struct RepoCatalogRoot {
    kind: PluginKind,
    location: RepoCatalogLocation,
}

#[derive(Debug, Clone, Copy)]
enum RepoCatalogLocation {
    Tree(&'static str),
    RepoRoot,
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
pub struct PluginHubSourceInfo {
    pub name: String,
    pub label: String,
    pub trust_level: TrustLevel,
    pub description: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct PluginMeta {
    pub name: String,
    pub identifier: String,
    pub description: String,
    pub version: String,
    pub kind: PluginKind,
    pub origin: String,
    pub trust_level: String,
    pub tags: Vec<String>,
    pub install_url: String,
    pub requires_env: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PluginSearchGroup {
    pub source: PluginHubSourceInfo,
    pub results: Vec<PluginMeta>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PluginSearchReport {
    pub groups: Vec<PluginSearchGroup>,
}

#[derive(Debug, Clone)]
pub struct PluginSearchResult {
    pub identifier: String,
    pub source_name: String,
    pub trust_level: TrustLevel,
    pub plugin: HubIndexPlugin,
    pub score: f32,
}

#[derive(Debug, Clone)]
struct FetchOutcome<T> {
    value: T,
    notice: Option<String>,
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
    pub shared_files: Vec<SharedInstallFile>,
}

#[derive(Debug, Clone)]
pub struct SharedInstallFile {
    pub relative_path: String,
    pub source: SharedInstallFileSource,
}

#[derive(Debug, Clone)]
pub enum SharedInstallFileSource {
    Local(PathBuf),
    GitHub { repo: String, path: String },
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
        if repo_backed_source(&source.name).is_some()
            && repo_backed_plugin_source(&source.name).is_none()
        {
            continue;
        }
        if let Some(mapping) = repo_backed_plugin_source(&source.name) {
            let skill_results =
                search_repo_backed_source(config, &client, source, mapping, query, limit).await?;
            for (score, identifier, plugin) in skill_results {
                results.push(PluginSearchResult {
                    identifier,
                    source_name: source.name.clone(),
                    trust_level: source.trust_level,
                    plugin,
                    score,
                });
            }
            continue;
        }

        let index = fetch_index(config, &client, source).await?.value;
        for plugin in index.plugins {
            let base_score = search_score(query, &plugin);
            if base_score > 0.0 {
                let score = if matches!(source.trust_level, TrustLevel::Official) {
                    base_score + 0.5
                } else {
                    base_score
                };
                results.push(PluginSearchResult {
                    identifier: format!("hub:{}/{}", source.name, plugin.name),
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

pub async fn search_hub_report(
    config: &PluginsConfig,
    query: &str,
    source_filter: Option<&str>,
    limit: usize,
) -> Result<PluginSearchReport, PluginError> {
    let sources = configured_sources(config);
    let client = hub_client()?;
    let mut groups = Vec::new();
    let per_source_limit = limit.max(1);

    for source in &sources {
        if !source_matches_filter(source_filter, &source.name) {
            continue;
        }
        if repo_backed_source(&source.name).is_some()
            && repo_backed_plugin_source(&source.name).is_none()
        {
            continue;
        }

        let source_info = source_info(source);
        let group = if let Some(mapping) = repo_backed_plugin_source(&source.name) {
            match search_repo_backed_meta(config, &client, source, mapping, query, per_source_limit)
                .await
            {
                Ok(outcome) => PluginSearchGroup {
                    source: source_info,
                    results: outcome.value,
                    notice: outcome.notice,
                },
                Err(error) => PluginSearchGroup {
                    source: source_info,
                    results: Vec::new(),
                    notice: Some(error.to_string()),
                },
            }
        } else {
            match fetch_index(config, &client, source).await {
                Ok(outcome) => {
                    let index = outcome.value;
                    let mut results: Vec<(f32, HubIndexPlugin)> = index
                        .plugins
                        .into_iter()
                        .filter_map(|plugin| {
                            let base_score = search_score(query, &plugin);
                            (base_score > 0.0).then_some((
                                if matches!(source.trust_level, TrustLevel::Official) {
                                    base_score + 0.5
                                } else {
                                    base_score
                                },
                                plugin,
                            ))
                        })
                        .collect();
                    results.sort_by(|left, right| {
                        right
                            .0
                            .partial_cmp(&left.0)
                            .unwrap_or(Ordering::Equal)
                            .then_with(|| left.1.name.cmp(&right.1.name))
                    });
                    results.truncate(per_source_limit);

                    PluginSearchGroup {
                        source: source_info,
                        results: results
                            .into_iter()
                            .map(|(_, plugin)| plugin_meta(source, plugin))
                            .collect(),
                        notice: outcome.notice,
                    }
                }
                Err(error) => PluginSearchGroup {
                    source: source_info,
                    results: Vec::new(),
                    notice: Some(error.to_string()),
                },
            }
        };

        if !group.results.is_empty() || group.notice.is_some() {
            groups.push(group);
        }
    }

    Ok(PluginSearchReport { groups })
}

pub fn hub_source_names(config: &PluginsConfig) -> Vec<String> {
    configured_sources(config)
        .into_iter()
        .map(|source| source.name)
        .collect()
}

pub fn hub_source_summaries(config: &PluginsConfig) -> Vec<PluginHubSourceInfo> {
    configured_sources(config)
        .into_iter()
        .filter(|source| {
            repo_backed_source(&source.name).is_none()
                || repo_backed_plugin_source(&source.name).is_some()
        })
        .map(|source| source_info(&source))
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
            shared_files: Vec::new(),
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
            shared_files: Vec::new(),
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
            shared_files: shared_files_for_github_source(path),
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
        shared_files: Vec::new(),
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

pub async fn install_shared_files(
    shared_files: &[SharedInstallFile],
    install_root: &Path,
) -> Result<(), PluginError> {
    if shared_files.is_empty() {
        return Ok(());
    }

    let client = hub_client()?;
    for shared in shared_files {
        let target = install_root.join(&shared.relative_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match &shared.source {
            SharedInstallFileSource::Local(path) => {
                std::fs::copy(path, &target)?;
            }
            SharedInstallFileSource::GitHub { repo, path } => {
                let url = format!("https://raw.githubusercontent.com/{repo}/main/{path}");
                let mut request = client.get(url);
                if let Some(token) = resolve_github_token() {
                    request = request.header("Authorization", format!("Bearer {token}"));
                }
                let bytes = request
                    .send()
                    .await
                    .map_err(|error| PluginError::Hub(error.to_string()))?
                    .error_for_status()
                    .map_err(|error| PluginError::Hub(error.to_string()))?
                    .bytes()
                    .await
                    .map_err(|error| PluginError::Hub(error.to_string()))?;
                std::fs::write(&target, bytes)?;
            }
        }
    }
    Ok(())
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

fn source_info(source: &RuntimeSource) -> PluginHubSourceInfo {
    PluginHubSourceInfo {
        name: source.name.clone(),
        label: source.name.clone(),
        trust_level: source.trust_level,
        description: source.description().to_string(),
        url: source.url.clone(),
    }
}

fn plugin_meta(source: &RuntimeSource, plugin: HubIndexPlugin) -> PluginMeta {
    PluginMeta {
        identifier: format!("hub:{}/{}", source.name, plugin.name),
        name: plugin.name.clone(),
        description: plugin.description.clone(),
        version: plugin.version.clone(),
        kind: plugin.kind,
        origin: plugin
            .homepage
            .clone()
            .unwrap_or_else(|| plugin.install_url.clone()),
        trust_level: format!("{:?}", source.trust_level).to_ascii_lowercase(),
        tags: plugin.tags.clone(),
        install_url: plugin.install_url.clone(),
        requires_env: plugin.requires_env.clone(),
    }
}

fn plugin_meta_from_repo_entry(
    source: &RuntimeSource,
    repo: &str,
    entry: &CachedRepoSourceEntry,
    description: String,
) -> PluginMeta {
    PluginMeta {
        identifier: format!("hub:{}/{}", source.name, entry.repo_path),
        name: entry.name.clone(),
        description,
        version: "skill".into(),
        kind: entry.kind,
        origin: format!("https://github.com/{repo}/tree/main/{}", entry.repo_path),
        trust_level: format!("{:?}", source.trust_level).to_ascii_lowercase(),
        tags: entry.tags.clone(),
        install_url: format!("github:{repo}/{}", entry.repo_path),
        requires_env: entry.requires_env.clone(),
    }
}

#[derive(Debug, Clone)]
struct RuntimeSource {
    name: String,
    url: String,
    trust_level: TrustLevel,
}

impl RuntimeSource {
    fn description(&self) -> &'static str {
        CURATED_SOURCES
            .iter()
            .find(|source| source.name == self.name)
            .map(|source| source.description)
            .unwrap_or("Configured plugin registry")
    }
}

fn repo_backed_source(source_name: &str) -> Option<RepoBackedSource> {
    match source_name {
        "edgecrab-official" => Some(RepoBackedSource {
            repo: "raphaelmansuy/edgecrab",
            roots: EDGECRAB_OFFICIAL_ALL_ROOTS,
            shared_root_files: &[],
        }),
        "hermes-plugins" => Some(RepoBackedSource {
            repo: "NousResearch/hermes-agent",
            roots: HERMES_ALL_ROOTS,
            shared_root_files: &[],
        }),
        "hermes-evey" => Some(RepoBackedSource {
            repo: "42-evey/hermes-plugins",
            roots: HERMES_EVEY_ROOTS,
            shared_root_files: &["evey_utils.py"],
        }),
        _ => None,
    }
}

fn repo_backed_plugin_source(source_name: &str) -> Option<RepoBackedSource> {
    match source_name {
        "edgecrab-official" => Some(RepoBackedSource {
            repo: "raphaelmansuy/edgecrab",
            roots: EDGECRAB_OFFICIAL_PLUGIN_ROOTS,
            shared_root_files: &[],
        }),
        "hermes-plugins" => Some(RepoBackedSource {
            repo: "NousResearch/hermes-agent",
            roots: HERMES_PLUGIN_ROOTS,
            shared_root_files: &[],
        }),
        "hermes-evey" => Some(RepoBackedSource {
            repo: "42-evey/hermes-plugins",
            roots: HERMES_EVEY_ROOTS,
            shared_root_files: &["evey_utils.py"],
        }),
        _ => None,
    }
}

fn repo_backed_source_for_repo(repo: &str) -> Option<RepoBackedSource> {
    CURATED_SOURCES
        .iter()
        .filter_map(|source| repo_backed_source(source.name))
        .find(|mapping| mapping.repo.eq_ignore_ascii_case(repo))
}

fn source_matches_filter(filter: Option<&str>, source_name: &str) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let normalized_filter = normalize_source_name(filter);
    let normalized_source = normalize_source_name(source_name);
    if normalized_filter == "official" {
        return matches!(source_name, "edgecrab-official" | "hermes-plugins");
    }
    normalized_filter == normalized_source
        || normalized_source.contains(&normalized_filter)
        || normalized_filter.contains(&normalized_source)
}

fn normalize_source_name(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "edgecrab" | "edgecrab-official" | "edgecrab-plugin" | "edgecrab-plugins" => {
            "edgecrabofficial".into()
        }
        "official" => "official".into(),
        "hermes" | "hermes-plugin" | "hermes-plugins" => "hermesplugins".into(),
        "hermes-agent" => "hermesplugins".into(),
        "evey" | "42-evey" | "hermes-evey" | "evey-hermes" => "hermesevey".into(),
        other => other
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect(),
    }
}

fn mapping_contains_repo_path(mapping: RepoBackedSource, repo_path: &str) -> bool {
    mapping.roots.iter().any(|root| match root.location {
        RepoCatalogLocation::Tree(prefix) => {
            repo_path == prefix || repo_path.starts_with(&format!("{prefix}/"))
        }
        RepoCatalogLocation::RepoRoot => !repo_path.is_empty() && !repo_path.contains('/'),
    })
}

fn shared_files_for_mapping(mapping: RepoBackedSource) -> Vec<SharedInstallFile> {
    mapping
        .shared_root_files
        .iter()
        .map(|path| SharedInstallFile {
            relative_path: (*path).to_string(),
            source: SharedInstallFileSource::GitHub {
                repo: mapping.repo.to_string(),
                path: (*path).to_string(),
            },
        })
        .collect()
}

fn shared_files_for_github_source(source: &str) -> Vec<SharedInstallFile> {
    let path = source.trim_matches('/');
    let mut parts = path.splitn(3, '/');
    let Some(owner) = parts.next() else {
        return Vec::new();
    };
    let Some(repo) = parts.next() else {
        return Vec::new();
    };
    let Some(repo_path) = parts.next() else {
        return Vec::new();
    };
    let repo = format!("{owner}/{repo}");
    let Some(mapping) = repo_backed_source_for_repo(&repo) else {
        return Vec::new();
    };
    if mapping_contains_repo_path(mapping, repo_path) {
        return shared_files_for_mapping(mapping);
    }
    Vec::new()
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
    if let Some(mapping) = repo_backed_source(source_name) {
        let client = hub_client()?;
        let entry = fetch_repo_source_entries(config, &client, &runtime_source, mapping)
            .await?
            .value
            .into_iter()
            .find(|candidate| candidate.repo_path == plugin_name)
            .ok_or_else(|| {
                PluginError::Hub(format!("plugin '{plugin_name}' not found in {source_name}"))
            })?;
        if !mapping.roots.iter().any(|root| match root.location {
            RepoCatalogLocation::Tree(prefix) => {
                plugin_name == prefix || plugin_name.starts_with(&format!("{prefix}/"))
            }
            RepoCatalogLocation::RepoRoot => !plugin_name.contains('/'),
        }) {
            return Err(PluginError::Hub(format!(
                "plugin '{plugin_name}' not found in {source_name}"
            )));
        }
        return Ok(ResolvedInstallSource {
            kind: InstallSourceKind::GitHub,
            display: format!("hub:{source_name}/{plugin_name}"),
            materialized_source: format!("github:{}/{plugin_name}", mapping.repo),
            trust_level: runtime_source.trust_level,
            plugin_name_hint: entry.name,
            expected_checksum: None,
            source_name: Some(source_name.to_string()),
            shared_files: shared_files_for_mapping(mapping),
        });
    }
    let client = hub_client()?;
    let index = fetch_index(config, &client, &runtime_source).await?.value;
    let plugin = index
        .plugins
        .into_iter()
        .find(|candidate| candidate.name == plugin_name)
        .ok_or_else(|| {
            PluginError::Hub(format!("plugin '{plugin_name}' not found in {source_name}"))
        })?;
    Ok(ResolvedInstallSource {
        kind: resolve_install_source(&plugin.install_url).kind,
        display: format!("hub:{source_name}/{}", plugin.name),
        materialized_source: plugin.install_url.clone(),
        trust_level: runtime_source.trust_level,
        plugin_name_hint: plugin.name,
        expected_checksum: Some(plugin.checksum),
        source_name: Some(source_name.to_string()),
        shared_files: Vec::new(),
    })
}

async fn search_repo_backed_meta(
    config: &PluginsConfig,
    client: &reqwest::Client,
    source: &RuntimeSource,
    mapping: RepoBackedSource,
    query: &str,
    limit: usize,
) -> Result<FetchOutcome<Vec<PluginMeta>>, PluginError> {
    let outcome = ranked_repo_entries(config, client, source, mapping, query, limit).await?;
    Ok(FetchOutcome {
        value: outcome
            .value
            .into_iter()
            .map(|(_, entry, description)| {
                plugin_meta_from_repo_entry(source, mapping.repo, &entry, description)
            })
            .collect(),
        notice: outcome.notice,
    })
}

async fn search_repo_backed_source(
    config: &PluginsConfig,
    client: &reqwest::Client,
    source: &RuntimeSource,
    mapping: RepoBackedSource,
    query: &str,
    limit: usize,
) -> Result<Vec<(f32, String, HubIndexPlugin)>, PluginError> {
    let ranked = ranked_repo_entries(config, client, source, mapping, query, limit).await?;
    Ok(ranked
        .value
        .into_iter()
        .map(|(score, entry, description)| {
            let origin = format!(
                "https://github.com/{}/tree/main/{}",
                mapping.repo, entry.repo_path
            );
            (
                score,
                format!("hub:{}/{}", source.name, entry.repo_path),
                HubIndexPlugin {
                    name: entry.name,
                    version: if entry.kind == PluginKind::Skill {
                        "skill".into()
                    } else {
                        "0.1.0".into()
                    },
                    description,
                    kind: entry.kind,
                    install_url: format!("github:{}/{}", mapping.repo, entry.repo_path),
                    checksum: String::new(),
                    author: String::new(),
                    license: String::new(),
                    homepage: Some(origin),
                    tools: entry.tools,
                    requires_env: entry.requires_env,
                    tags: entry.tags,
                },
            )
        })
        .collect())
}

async fn ranked_repo_entries(
    config: &PluginsConfig,
    client: &reqwest::Client,
    source: &RuntimeSource,
    mapping: RepoBackedSource,
    query: &str,
    limit: usize,
) -> Result<FetchOutcome<Vec<(f32, CachedRepoSourceEntry, String)>>, PluginError> {
    let source_entries = fetch_repo_source_entries(config, client, source, mapping).await?;
    let mut ranked: Vec<(f32, CachedRepoSourceEntry)> = source_entries
        .value
        .iter()
        .cloned()
        .filter_map(|entry| {
            let base_score = search_score_for_repo_entry(query, &entry);
            (base_score > 0.0).then_some((
                if matches!(source.trust_level, TrustLevel::Official) {
                    base_score + 0.5
                } else {
                    base_score
                },
                entry,
            ))
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.1.repo_path.cmp(&right.1.repo_path))
    });
    ranked.truncate(limit.max(1));

    let mut results = Vec::with_capacity(ranked.len());
    for (score, entry) in ranked {
        let description =
            fetch_repo_entry_description(config, client, source, mapping.repo, &entry)
                .await
                .unwrap_or_else(|_| default_repo_entry_description(&entry));
        results.push((score, entry, description));
    }
    Ok(FetchOutcome {
        value: results,
        notice: source_entries.notice,
    })
}

async fn fetch_repo_source_entries(
    config: &PluginsConfig,
    client: &reqwest::Client,
    source: &RuntimeSource,
    mapping: RepoBackedSource,
) -> Result<FetchOutcome<Vec<CachedRepoSourceEntry>>, PluginError> {
    let cache_path = repo_source_cache_path(config, source, mapping);
    let cached = read_repo_source_cache(config, source, mapping);
    if let Some(cached) = cached
        .as_ref()
        .filter(|cached| cache_is_fresh(cached.fetched_at, config))
    {
        return Ok(FetchOutcome {
            value: cached.entries.clone(),
            notice: None,
        });
    }

    let tree_url = format!(
        "https://api.github.com/repos/{}/git/trees/main?recursive=1",
        mapping.repo
    );
    let tree = match async {
        let response = client
            .get(tree_url)
            .send()
            .await
            .map_err(|error| PluginError::Hub(error.to_string()))?
            .error_for_status()
            .map_err(|error| PluginError::Hub(error.to_string()))?;
        response
            .json::<GitTreeResponse>()
            .await
            .map_err(|error| PluginError::Hub(error.to_string()))
    }
    .await
    {
        Ok(tree) => tree,
        Err(error) => {
            if let Some(cached) = cached {
                return Ok(FetchOutcome {
                    value: cached.entries,
                    notice: Some(format!(
                        "{} using stale cached index after refresh failed",
                        source.name
                    )),
                });
            }
            return Err(error);
        }
    };

    let mut entries = Vec::new();
    for item in tree.tree {
        if item.kind != "blob" {
            continue;
        }
        for root in mapping.roots {
            match root.location {
                RepoCatalogLocation::Tree(prefix) => {
                    let prefix = format!("{prefix}/");
                    let Some(relative) = item.path.strip_prefix(&prefix) else {
                        continue;
                    };
                    match root.kind {
                        PluginKind::Skill => {
                            let Some(relative) = relative.strip_suffix("/SKILL.md") else {
                                continue;
                            };
                            if relative.is_empty() {
                                continue;
                            }
                            let repo_path =
                                format!("{}/{}", prefix.trim_end_matches('/'), relative);
                            let name = relative.rsplit('/').next().unwrap_or(relative).to_string();
                            let tags = relative
                                .split('/')
                                .take(relative.split('/').count().saturating_sub(1))
                                .map(ToOwned::to_owned)
                                .collect();
                            entries.push(CachedRepoSourceEntry {
                                name,
                                repo_path,
                                tags,
                                kind: PluginKind::Skill,
                                tools: Vec::new(),
                                requires_env: Vec::new(),
                            });
                        }
                        PluginKind::Hermes => {
                            if !(relative.ends_with("/plugin.yaml")
                                || relative.ends_with("/plugin.yml"))
                            {
                                continue;
                            }
                            let Some(relative_dir) = relative.rsplit_once('/').map(|(dir, _)| dir)
                            else {
                                continue;
                            };
                            if relative_dir.is_empty() {
                                continue;
                            }
                            let repo_path =
                                format!("{}/{}", prefix.trim_end_matches('/'), relative_dir);
                            let name = relative_dir
                                .rsplit('/')
                                .next()
                                .unwrap_or(relative_dir)
                                .to_string();
                            let tags = relative_dir
                                .split('/')
                                .take(relative_dir.split('/').count().saturating_sub(1))
                                .map(ToOwned::to_owned)
                                .collect();
                            entries.push(CachedRepoSourceEntry {
                                name,
                                repo_path,
                                tags,
                                kind: PluginKind::Hermes,
                                tools: Vec::new(),
                                requires_env: Vec::new(),
                            });
                        }
                        _ => {}
                    }
                }
                RepoCatalogLocation::RepoRoot => {
                    if root.kind != PluginKind::Hermes {
                        continue;
                    }
                    let Some((dir_name, file_name)) = item.path.split_once('/') else {
                        continue;
                    };
                    if file_name != "plugin.yaml" && file_name != "plugin.yml" {
                        continue;
                    }
                    if dir_name.is_empty() || dir_name.starts_with('.') {
                        continue;
                    }
                    entries.push(CachedRepoSourceEntry {
                        name: dir_name.to_string(),
                        repo_path: dir_name.to_string(),
                        tags: Vec::new(),
                        kind: PluginKind::Hermes,
                        tools: Vec::new(),
                        requires_env: Vec::new(),
                    });
                }
            }
        }
    }
    entries.sort_by(|left, right| left.repo_path.cmp(&right.repo_path));
    entries.dedup_by(|left, right| left.repo_path == right.repo_path);

    let cached = CachedRepoSource {
        fetched_at: current_epoch_secs(),
        entries: entries.clone(),
    };
    write_cached_json(&cache_path, &cached)?;
    Ok(FetchOutcome {
        value: entries,
        notice: None,
    })
}

fn read_repo_source_cache(
    config: &PluginsConfig,
    source: &RuntimeSource,
    mapping: RepoBackedSource,
) -> Option<CachedRepoSource> {
    read_cached_json::<CachedRepoSource>(&repo_source_cache_path(config, source, mapping))
        .or_else(|| {
            read_cached_json::<CachedRepoSource>(&legacy_repo_source_cache_path(config, source))
        })
        .map(|cached| filter_repo_source_cache(cached, mapping))
}

fn filter_repo_source_cache(
    cached: CachedRepoSource,
    mapping: RepoBackedSource,
) -> CachedRepoSource {
    CachedRepoSource {
        fetched_at: cached.fetched_at,
        entries: filter_repo_entries_for_mapping(cached.entries, mapping),
    }
}

fn filter_repo_entries_for_mapping(
    entries: Vec<CachedRepoSourceEntry>,
    mapping: RepoBackedSource,
) -> Vec<CachedRepoSourceEntry> {
    entries
        .into_iter()
        .filter(|entry| repo_entry_matches_mapping(entry, mapping))
        .collect()
}

fn repo_entry_matches_mapping(entry: &CachedRepoSourceEntry, mapping: RepoBackedSource) -> bool {
    mapping.roots.iter().any(|root| {
        if root.kind != entry.kind {
            return false;
        }
        match root.location {
            RepoCatalogLocation::Tree(prefix) => {
                entry.repo_path == prefix || entry.repo_path.starts_with(&format!("{prefix}/"))
            }
            RepoCatalogLocation::RepoRoot => {
                !entry.repo_path.is_empty() && !entry.repo_path.contains('/')
            }
        }
    })
}

fn search_score_for_repo_entry(query: &str, entry: &CachedRepoSourceEntry) -> f32 {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return 1.0;
    }
    let mut score = 0.0;
    let name = entry.name.to_ascii_lowercase();
    let repo_path = entry.repo_path.to_ascii_lowercase();
    if name.contains(&q) {
        score += 2.0;
    }
    if repo_path.contains(&q) {
        score += 1.0;
    }
    for tag in &entry.tags {
        if tag.to_ascii_lowercase().contains(&q) {
            score += 1.5;
        }
    }
    score
}

async fn fetch_repo_entry_description(
    config: &PluginsConfig,
    client: &reqwest::Client,
    source: &RuntimeSource,
    repo: &str,
    entry: &CachedRepoSourceEntry,
) -> Result<String, PluginError> {
    let cache_path = repo_entry_description_cache_path(config, source, repo, entry);
    let cached = read_cached_json::<CachedRepoDescription>(&cache_path);
    if let Some(cached) = cached
        .as_ref()
        .filter(|cached| cache_is_fresh(cached.fetched_at, config))
    {
        return Ok(cached.description.clone());
    }

    let file_name = if entry.kind == PluginKind::Skill {
        "SKILL.md"
    } else {
        "plugin.yaml"
    };
    let url = format!(
        "https://raw.githubusercontent.com/{repo}/main/{}/{}",
        entry.repo_path, file_name
    );
    let text = match async {
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|error| PluginError::Hub(error.to_string()))?
            .error_for_status()
            .map_err(|error| PluginError::Hub(error.to_string()))?;
        response
            .text()
            .await
            .map_err(|error| PluginError::Hub(error.to_string()))
    }
    .await
    {
        Ok(text) => text,
        Err(error) => {
            if let Some(cached) = cached {
                return Ok(cached.description);
            }
            return Err(error);
        }
    };
    let description = if entry.kind == PluginKind::Skill {
        extract_skill_description(&text)
    } else {
        extract_hermes_plugin_description(&text)
    };
    write_cached_json(
        &cache_path,
        &CachedRepoDescription {
            fetched_at: current_epoch_secs(),
            description: description.clone(),
        },
    )?;
    Ok(description)
}

fn extract_skill_description(markdown: &str) -> String {
    let mut lines = markdown.lines();
    let mut in_frontmatter = false;
    if matches!(lines.clone().next().map(str::trim), Some("---")) {
        in_frontmatter = true;
        lines.next();
    }

    let mut paragraph = Vec::new();
    let mut seen_heading = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
            }
            continue;
        }
        if trimmed.starts_with('#') {
            seen_heading = true;
            continue;
        }
        if trimmed.starts_with("```") {
            break;
        }
        if seen_heading || paragraph.is_empty() {
            paragraph.push(trimmed.trim_start_matches("- ").to_string());
        }
    }
    paragraph.join(" ")
}

fn extract_hermes_plugin_description(yaml: &str) -> String {
    serde_yml::from_str::<serde_json::Value>(yaml)
        .ok()
        .and_then(|value| {
            value
                .get("description")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "Hermes plugin".into())
}

async fn fetch_index(
    config: &PluginsConfig,
    client: &reqwest::Client,
    source: &RuntimeSource,
) -> Result<FetchOutcome<HubIndex>, PluginError> {
    let cache_path = hub_cache_dir(config).join(format!("{}.json", source.name));
    let cached = read_cached_json::<CachedIndex>(&cache_path);
    if let Some(cached) = cached
        .as_ref()
        .filter(|cached| cache_is_fresh(cached.fetched_at, config))
    {
        return Ok(FetchOutcome {
            value: cached.index.clone(),
            notice: None,
        });
    }

    let response = match client
        .get(&source.url)
        .send()
        .await
        .map_err(|error| PluginError::Hub(error.to_string()))
        .and_then(|response| {
            response
                .error_for_status()
                .map_err(|error| PluginError::Hub(error.to_string()))
        }) {
        Ok(response) => response,
        Err(error) => {
            if let Some(cached) = cached {
                return Ok(FetchOutcome {
                    value: cached.index,
                    notice: Some(format!(
                        "{} using stale cached index after refresh failed",
                        source.name
                    )),
                });
            }
            return Err(error);
        }
    };
    let bytes = response
        .bytes()
        .await
        .map_err(|error| PluginError::Hub(error.to_string()))?;
    let index = serde_json::from_slice::<HubIndex>(&bytes)
        .map_err(|error| PluginError::Hub(error.to_string()))?;

    let cached = CachedIndex {
        fetched_at: current_epoch_secs(),
        index: index.clone(),
    };
    write_cached_json(&cache_path, &cached)?;
    Ok(FetchOutcome {
        value: index,
        notice: None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedIndex {
    fetched_at: i64,
    index: HubIndex,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedRepoSource {
    fetched_at: i64,
    #[serde(default)]
    entries: Vec<CachedRepoSourceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedRepoSourceEntry {
    name: String,
    repo_path: String,
    #[serde(default)]
    tags: Vec<String>,
    kind: PluginKind,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    requires_env: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedRepoDescription {
    fetched_at: i64,
    description: String,
}

#[derive(Debug, Deserialize)]
struct GitTreeResponse {
    #[serde(default)]
    tree: Vec<GitTreeEntry>,
}

#[derive(Debug, Deserialize)]
struct GitTreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
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

fn default_repo_entry_description(entry: &CachedRepoSourceEntry) -> String {
    match entry.kind {
        PluginKind::Hermes => "Hermes plugin".into(),
        PluginKind::Skill => "Skill".into(),
        PluginKind::ToolServer => "Tool-server plugin".into(),
        PluginKind::Script => "Script plugin".into(),
    }
}

fn current_epoch_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

fn cache_is_fresh(fetched_at: i64, config: &PluginsConfig) -> bool {
    current_epoch_secs().saturating_sub(fetched_at) <= config.hub.cache_ttl_secs as i64
}

fn read_cached_json<T>(path: &Path) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cached_json<T>(path: &Path, value: &T) -> Result<(), PluginError>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        serde_json::to_vec(value).map_err(|error| PluginError::Hub(error.to_string()))?,
    )?;
    Ok(())
}

fn repo_entry_description_cache_path(
    config: &PluginsConfig,
    source: &RuntimeSource,
    repo: &str,
    entry: &CachedRepoSourceEntry,
) -> PathBuf {
    let key = format!(
        "{}:{}:{}:{:?}",
        source.name, repo, entry.repo_path, entry.kind
    );
    let digest = Sha256::digest(key.as_bytes());
    hub_cache_dir(config)
        .join("descriptions")
        .join(format!("{}-{:x}.json", source.name, digest))
}

fn repo_source_cache_path(
    config: &PluginsConfig,
    source: &RuntimeSource,
    mapping: RepoBackedSource,
) -> PathBuf {
    let digest = Sha256::digest(repo_source_cache_key(mapping).as_bytes());
    hub_cache_dir(config).join(format!("{}-repo-{:x}.json", source.name, digest))
}

fn legacy_repo_source_cache_path(config: &PluginsConfig, source: &RuntimeSource) -> PathBuf {
    hub_cache_dir(config).join(format!("{}-skills.json", source.name))
}

fn repo_source_cache_key(mapping: RepoBackedSource) -> String {
    let mut key = format!("repo={}", mapping.repo);
    for root in mapping.roots {
        let location = match root.location {
            RepoCatalogLocation::Tree(prefix) => format!("tree:{prefix}"),
            RepoCatalogLocation::RepoRoot => "repo-root".into(),
        };
        key.push('|');
        key.push_str(root.kind.as_tag());
        key.push(':');
        key.push_str(&location);
    }
    key
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
                    let mut download =
                        client.get(entry.download_url.ok_or_else(|| {
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
    use tempfile::TempDir;

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
        assert!(source_matches_filter(
            Some("hermes-plugins"),
            "hermes-plugins"
        ));
        assert!(source_matches_filter(Some("evey"), "hermes-evey"));
        assert!(source_matches_filter(Some("42-evey"), "hermes-evey"));
        assert!(!source_matches_filter(Some("edgecrab"), "hermes-plugins"));
    }

    #[test]
    fn source_filter_matches_edgecrab_and_official_aliases() {
        assert!(source_matches_filter(Some("edgecrab"), "edgecrab-official"));
        assert!(source_matches_filter(Some("official"), "edgecrab-official"));
        assert!(source_matches_filter(Some("official"), "hermes-plugins"));
    }

    #[test]
    fn plugin_source_summaries_include_official_repo_sources_with_plugin_roots() {
        let sources = hub_source_summaries(&crate::config::PluginsConfig::default());
        assert!(
            sources
                .iter()
                .any(|source| source.name == "edgecrab-official"),
            "plugin browser should advertise official repo sources when plugin roots exist"
        );
        assert!(
            sources.iter().any(|source| source.name == "hermes-plugins"),
            "hermes plugin source should remain visible"
        );
    }

    #[test]
    fn extracts_skill_description_after_heading() {
        let markdown = "# Demo Skill\n\nUseful summary line.\n\n## Steps\n- One";
        assert_eq!(extract_skill_description(markdown), "Useful summary line.");
    }

    #[tokio::test]
    async fn cached_hermes_repo_index_includes_python_plugin_directories() {
        let temp = TempDir::new().expect("tempdir");
        let config = crate::config::PluginsConfig {
            install_dir: temp.path().join("plugins"),
            ..crate::config::PluginsConfig::default()
        };
        let source = RuntimeSource {
            name: "hermes-plugins".into(),
            url: "https://github.com/NousResearch/hermes-agent".into(),
            trust_level: TrustLevel::Official,
        };
        let mapping = repo_backed_plugin_source("hermes-plugins").expect("plugin mapping");
        let cache_path = repo_source_cache_path(&config, &source, mapping);
        std::fs::create_dir_all(cache_path.parent().expect("hub cache parent")).expect("cache dir");
        let cached = CachedRepoSource {
            fetched_at: chrono::Utc::now().timestamp(),
            entries: vec![CachedRepoSourceEntry {
                name: "holographic".into(),
                repo_path: "plugins/memory/holographic".into(),
                tags: vec!["memory".into()],
                kind: PluginKind::Hermes,
                tools: vec!["fact_store".into()],
                requires_env: Vec::new(),
            }],
        };
        std::fs::write(
            &cache_path,
            serde_json::to_vec(&cached).expect("serialize cache"),
        )
        .expect("write cache");

        let report = search_hub_report(&config, "holographic", Some("hermes"), 10)
            .await
            .expect("search report");
        let result = report
            .groups
            .iter()
            .flat_map(|group| group.results.iter())
            .find(|result| result.name == "holographic")
            .expect("holographic result");

        assert_eq!(result.kind, PluginKind::Hermes);
        assert_eq!(
            result.install_url,
            "github:NousResearch/hermes-agent/plugins/memory/holographic"
        );
    }

    #[tokio::test]
    async fn cached_official_repo_index_includes_python_plugin_directories() {
        let temp = TempDir::new().expect("tempdir");
        let config = crate::config::PluginsConfig {
            install_dir: temp.path().join("plugins"),
            ..crate::config::PluginsConfig::default()
        };
        let source = RuntimeSource {
            name: "edgecrab-official".into(),
            url: "https://github.com/raphaelmansuy/edgecrab".into(),
            trust_level: TrustLevel::Official,
        };
        let mapping = repo_backed_plugin_source("edgecrab-official").expect("plugin mapping");
        let cache_path = repo_source_cache_path(&config, &source, mapping);
        std::fs::create_dir_all(cache_path.parent().expect("hub cache parent")).expect("cache dir");
        let cached = CachedRepoSource {
            fetched_at: chrono::Utc::now().timestamp(),
            entries: vec![CachedRepoSourceEntry {
                name: "calculator".into(),
                repo_path: "plugins/productivity/calculator".into(),
                tags: vec!["productivity".into()],
                kind: PluginKind::Hermes,
                tools: vec!["calculate".into(), "unit_convert".into()],
                requires_env: Vec::new(),
            }],
        };
        std::fs::write(
            &cache_path,
            serde_json::to_vec(&cached).expect("serialize cache"),
        )
        .expect("write cache");

        let report = search_hub_report(&config, "calculator", Some("edgecrab"), 10)
            .await
            .expect("search report");
        let result = report
            .groups
            .iter()
            .flat_map(|group| group.results.iter())
            .find(|result| result.name == "calculator")
            .expect("calculator result");

        assert_eq!(result.kind, PluginKind::Hermes);
        assert_eq!(
            result.install_url,
            "github:raphaelmansuy/edgecrab/plugins/productivity/calculator"
        );
    }

    #[test]
    fn github_repo_root_hermes_sources_include_declared_shared_files() {
        let resolved = resolve_install_source("github:42-evey/hermes-plugins/evey-cache");
        assert_eq!(resolved.kind, InstallSourceKind::GitHub);
        assert_eq!(resolved.plugin_name_hint, "evey-cache");
        assert_eq!(resolved.shared_files.len(), 1);
        assert_eq!(resolved.shared_files[0].relative_path, "evey_utils.py");
        match &resolved.shared_files[0].source {
            SharedInstallFileSource::GitHub { repo, path } => {
                assert_eq!(repo, "42-evey/hermes-plugins");
                assert_eq!(path, "evey_utils.py");
            }
            other => panic!("unexpected shared file source: {other:?}"),
        }
    }

    #[tokio::test]
    async fn cached_evey_repo_index_includes_repo_root_plugin_directories() {
        let temp = TempDir::new().expect("tempdir");
        let config = crate::config::PluginsConfig {
            install_dir: temp.path().join("plugins"),
            ..crate::config::PluginsConfig::default()
        };
        let source = RuntimeSource {
            name: "hermes-evey".into(),
            url: "https://github.com/42-evey/hermes-plugins".into(),
            trust_level: TrustLevel::Community,
        };
        let mapping = repo_backed_plugin_source("hermes-evey").expect("plugin mapping");
        let cache_path = repo_source_cache_path(&config, &source, mapping);
        std::fs::create_dir_all(cache_path.parent().expect("hub cache parent")).expect("cache dir");
        let cached = CachedRepoSource {
            fetched_at: chrono::Utc::now().timestamp(),
            entries: vec![CachedRepoSourceEntry {
                name: "evey-telemetry".into(),
                repo_path: "evey-telemetry".into(),
                tags: Vec::new(),
                kind: PluginKind::Hermes,
                tools: vec!["telemetry_query".into()],
                requires_env: Vec::new(),
            }],
        };
        std::fs::write(
            &cache_path,
            serde_json::to_vec(&cached).expect("serialize cache"),
        )
        .expect("write cache");

        let report = search_hub_report(&config, "telemetry", Some("evey"), 10)
            .await
            .expect("search report");
        let result = report
            .groups
            .iter()
            .flat_map(|group| group.results.iter())
            .find(|result| result.name == "evey-telemetry")
            .expect("evey-telemetry result");

        assert_eq!(result.kind, PluginKind::Hermes);
        assert_eq!(
            result.install_url,
            "github:42-evey/hermes-plugins/evey-telemetry"
        );
    }

    #[tokio::test]
    async fn stale_cached_index_is_used_when_refresh_fails() {
        let temp = TempDir::new().expect("tempdir");
        let mut config = crate::config::PluginsConfig {
            install_dir: temp.path().join("plugins"),
            ..crate::config::PluginsConfig::default()
        };
        config.hub.cache_ttl_secs = 1;

        let source = RuntimeSource {
            name: "custom-source".into(),
            url: "http://127.0.0.1:9/index.json".into(),
            trust_level: TrustLevel::Community,
        };
        let cache_path = hub_cache_dir(&config).join("custom-source.json");
        write_cached_json(
            &cache_path,
            &CachedIndex {
                fetched_at: current_epoch_secs() - 120,
                index: HubIndex {
                    version: "1".into(),
                    generated_at: "2026-04-10T00:00:00Z".into(),
                    source_name: "custom-source".into(),
                    plugins: vec![HubIndexPlugin {
                        name: "cached-plugin".into(),
                        version: "0.1.0".into(),
                        description: "From stale cache".into(),
                        kind: PluginKind::Hermes,
                        install_url: "github:owner/repo/cached-plugin".into(),
                        checksum: "sha256:test".into(),
                        author: String::new(),
                        license: String::new(),
                        homepage: None,
                        tools: Vec::new(),
                        requires_env: Vec::new(),
                        tags: vec!["cached".into()],
                    }],
                },
            },
        )
        .expect("write cache");

        let outcome = fetch_index(&config, &hub_client().expect("client"), &source)
            .await
            .expect("fetch index");

        assert_eq!(outcome.value.plugins.len(), 1);
        assert_eq!(outcome.value.plugins[0].name, "cached-plugin");
        assert!(
            outcome
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("stale cached index")),
            "notice should explain stale cache fallback: {:?}",
            outcome.notice
        );
    }

    #[tokio::test]
    async fn repo_entry_description_uses_fresh_cache_without_network() {
        let temp = TempDir::new().expect("tempdir");
        let config = crate::config::PluginsConfig {
            install_dir: temp.path().join("plugins"),
            ..crate::config::PluginsConfig::default()
        };
        let source = RuntimeSource {
            name: "hermes-plugins".into(),
            url: "https://github.com/NousResearch/hermes-agent".into(),
            trust_level: TrustLevel::Official,
        };
        let entry = CachedRepoSourceEntry {
            name: "secure-reader".into(),
            repo_path: "plugins/security/secure-reader".into(),
            tags: vec!["security".into()],
            kind: PluginKind::Hermes,
            tools: Vec::new(),
            requires_env: Vec::new(),
        };
        let cache_path = repo_entry_description_cache_path(&config, &source, "owner/repo", &entry);
        write_cached_json(
            &cache_path,
            &CachedRepoDescription {
                fetched_at: current_epoch_secs(),
                description: "Cached plugin description".into(),
            },
        )
        .expect("write cached description");

        let description = fetch_repo_entry_description(
            &config,
            &hub_client().expect("client"),
            &source,
            "owner/repo",
            &entry,
        )
        .await
        .expect("description");

        assert_eq!(description, "Cached plugin description");
    }

    #[tokio::test]
    async fn plugin_search_report_ignores_skill_only_repo_results_and_keeps_hermes_plugins_visible()
    {
        let temp = TempDir::new().expect("tempdir");
        let config = crate::config::PluginsConfig {
            install_dir: temp.path().join("plugins"),
            ..crate::config::PluginsConfig::default()
        };
        let cache_dir = hub_cache_dir(&config);
        std::fs::create_dir_all(&cache_dir).expect("cache dir");

        let edgecrab_plugin_source = RuntimeSource {
            name: "edgecrab-official".into(),
            url: "https://github.com/raphaelmansuy/edgecrab".into(),
            trust_level: TrustLevel::Official,
        };
        let edgecrab_plugin_mapping =
            repo_backed_plugin_source("edgecrab-official").expect("edgecrab plugin mapping");
        let edgecrab_plugin_cache = CachedRepoSource {
            fetched_at: chrono::Utc::now().timestamp(),
            entries: vec![CachedRepoSourceEntry {
                name: "calculator".into(),
                repo_path: "plugins/productivity/calculator".into(),
                tags: vec!["productivity".into()],
                kind: PluginKind::Hermes,
                tools: vec!["calculate".into()],
                requires_env: Vec::new(),
            }],
        };
        std::fs::write(
            repo_source_cache_path(&config, &edgecrab_plugin_source, edgecrab_plugin_mapping),
            serde_json::to_vec(&edgecrab_plugin_cache).expect("serialize edgecrab plugin cache"),
        )
        .expect("write edgecrab plugin cache");

        let edgecrab_legacy_cache = CachedRepoSource {
            fetched_at: chrono::Utc::now().timestamp(),
            entries: vec![CachedRepoSourceEntry {
                name: "blackbox".into(),
                repo_path: "optional-skills/autonomous-ai-agents/blackbox".into(),
                tags: vec!["autonomous-ai-agents".into()],
                kind: PluginKind::Skill,
                tools: Vec::new(),
                requires_env: Vec::new(),
            }],
        };
        std::fs::write(
            legacy_repo_source_cache_path(&config, &edgecrab_plugin_source),
            serde_json::to_vec(&edgecrab_legacy_cache).expect("serialize edgecrab legacy cache"),
        )
        .expect("write edgecrab legacy cache");

        let hermes_source = RuntimeSource {
            name: "hermes-plugins".into(),
            url: "https://github.com/NousResearch/hermes-agent".into(),
            trust_level: TrustLevel::Official,
        };
        let hermes_mapping = repo_backed_plugin_source("hermes-plugins").expect("hermes mapping");
        let hermes_cache = CachedRepoSource {
            fetched_at: chrono::Utc::now().timestamp(),
            entries: vec![CachedRepoSourceEntry {
                name: "holographic".into(),
                repo_path: "plugins/memory/holographic".into(),
                tags: vec!["memory".into()],
                kind: PluginKind::Hermes,
                tools: vec!["fact_store".into()],
                requires_env: Vec::new(),
            }],
        };
        std::fs::write(
            repo_source_cache_path(&config, &hermes_source, hermes_mapping),
            serde_json::to_vec(&hermes_cache).expect("serialize hermes cache"),
        )
        .expect("write hermes cache");

        let evey_source = RuntimeSource {
            name: "hermes-evey".into(),
            url: "https://github.com/42-evey/hermes-plugins".into(),
            trust_level: TrustLevel::Community,
        };
        let evey_mapping = repo_backed_plugin_source("hermes-evey").expect("evey mapping");
        let evey_cache = CachedRepoSource {
            fetched_at: chrono::Utc::now().timestamp(),
            entries: Vec::new(),
        };
        std::fs::write(
            repo_source_cache_path(&config, &evey_source, evey_mapping),
            serde_json::to_vec(&evey_cache).expect("serialize evey cache"),
        )
        .expect("write evey cache");

        let report = search_hub_report(&config, "o", None, 12)
            .await
            .expect("search report");

        assert!(
            report
                .groups
                .iter()
                .any(|group| group.source.name == "edgecrab-official"),
            "official repo source should appear once it has plugin roots"
        );
        let names: Vec<&str> = report
            .groups
            .iter()
            .flat_map(|group| group.results.iter().map(|result| result.name.as_str()))
            .collect();
        assert!(
            names.contains(&"calculator"),
            "expected official EdgeCrab plugin to be visible: {names:?}"
        );
        assert!(
            names.contains(&"holographic"),
            "expected real Hermes plugin to remain visible: {names:?}"
        );
        assert!(
            !names.contains(&"blackbox"),
            "skill entry should not leak into plugin browser: {names:?}"
        );
    }

    #[tokio::test]
    #[ignore = "network-backed smoke test"]
    async fn live_official_edgecrab_search_returns_real_plugins() {
        let report = search_hub_report(
            &crate::config::PluginsConfig::default(),
            "calculator",
            Some("edgecrab"),
            5,
        )
        .await
        .expect("live edgecrab search");
        assert!(report.groups.iter().any(|group| !group.results.is_empty()));
        assert!(
            report
                .groups
                .iter()
                .flat_map(|group| &group.results)
                .any(|result| result.name == "calculator")
        );
    }

    #[tokio::test]
    #[ignore = "network-backed smoke test"]
    async fn live_official_hermes_search_returns_real_plugins() {
        let report = search_hub_report(
            &crate::config::PluginsConfig::default(),
            "github",
            Some("hermes"),
            5,
        )
        .await
        .expect("live hermes search");
        assert!(report.groups.iter().any(|group| !group.results.is_empty()));
        assert!(
            report
                .groups
                .iter()
                .flat_map(|group| &group.results)
                .all(|result| result.identifier.starts_with("hub:hermes-plugins/"))
        );
    }
}
