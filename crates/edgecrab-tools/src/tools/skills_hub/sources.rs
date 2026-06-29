//! Registry source adapters — ClawHub, browse.sh, skills.sh install, agentskills.io federation.
//!
//! Hermes-parity search/install with EdgeCrab-first defaults: explicit community trust
//! warnings, path-safe ZIP extraction, and federation via `/.well-known/skills/index.json`.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Component, Path};
use std::time::Duration;

use futures::future::join_all;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use super::{
    HubSourceInfo, SOURCE_TIMEOUT_SECS, SearchGroup, SkillBundle, SkillMeta, ensure_safe_url,
    fetch_github_bundle, hub_client,
};

const CLAWHUB_BASE: &str = "https://clawhub.ai/api/v1";
const BROWSE_SH_CATALOG: &str = "https://browse.sh/api/skills";
const BROWSE_SH_DETAIL: &str = "https://browse.sh/api/skills";
const REGISTRY_CACHE_TTL_SECS: i64 = 15 * 60;

/// Public registry sources searched in parallel alongside curated GitHub trees.
pub const REGISTRY_SOURCES: &[RegistrySource] = &[
    RegistrySource {
        id: "clawhub",
        label: "ClawHub",
        origin: "https://clawhub.ai",
        trust_level: "community",
        warning: Some(
            "Community marketplace — verify skill content before install (ClawHub vetting is limited).",
        ),
    },
    RegistrySource {
        id: "browse-sh",
        label: "browse.sh",
        origin: "https://browse.sh",
        trust_level: "community",
        warning: None,
    },
    RegistrySource {
        id: "lobehub",
        label: "LobeHub",
        origin: "https://chat-agents.lobehub.com",
        trust_level: "community",
        warning: None,
    },
    RegistrySource {
        id: "claude-marketplace",
        label: "Claude Marketplace",
        origin: "https://github.com/anthropics/skills",
        trust_level: "trusted",
        warning: None,
    },
    RegistrySource {
        id: "agentskills.io",
        label: "agentskills.io",
        origin: "https://agentskills.io",
        trust_level: "trusted",
        warning: None,
    },
];

#[derive(Debug, Clone, Copy)]
pub struct RegistrySource {
    pub id: &'static str,
    pub label: &'static str,
    pub origin: &'static str,
    pub trust_level: &'static str,
    pub warning: Option<&'static str>,
}

pub fn registry_source_summaries() -> Vec<HubSourceInfo> {
    REGISTRY_SOURCES
        .iter()
        .map(|source| HubSourceInfo {
            id: source.id.to_string(),
            label: source.label.to_string(),
            origin: source.origin.to_string(),
            trust_level: source.trust_level.to_string(),
        })
        .collect()
}

pub fn registry_filter_includes_any(filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    filter == "registry"
        || filter == "clawhub"
        || filter == "browse-sh"
        || filter == "browse.sh"
        || filter == "lobehub"
        || filter == "claude-marketplace"
        || filter == "claude_marketplace"
        || filter == "marketplace"
        || filter == "agentskills"
        || filter == "agentskills.io"
}

fn registry_source_included(source: &RegistrySource, filter: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    if filter.is_empty() || filter == "all" || filter == "registry" {
        return true;
    }
    filter == source.id
        || filter == source.label.to_lowercase()
        || (filter == "clawhub" && source.id == "clawhub")
        || (filter == "browse.sh" && source.id == "browse-sh")
        || (filter == "browse-sh" && source.id == "browse-sh")
        || (filter == "lobehub" && source.id == "lobehub")
        || (filter == "claude-marketplace" && source.id == "claude-marketplace")
        || (filter == "claude_marketplace" && source.id == "claude-marketplace")
        || (filter == "marketplace" && source.id == "claude-marketplace")
        || (filter == "agentskills" && source.id == "agentskills.io")
        || (filter == "agentskills.io" && source.id == "agentskills.io")
}

pub async fn search_registry_sources(
    query: &str,
    filter: &str,
    limit_per_source: usize,
) -> Vec<SearchGroup> {
    let limit = limit_per_source.clamp(1, 20);
    let client = match hub_client() {
        Ok(client) => client,
        Err(error) => {
            return vec![SearchGroup {
                source: HubSourceInfo {
                    id: "registry".into(),
                    label: "Registry Sources".into(),
                    origin: "remote".into(),
                    trust_level: "n/a".into(),
                },
                results: Vec::new(),
                notice: Some(error),
            }];
        }
    };

    let futures = REGISTRY_SOURCES
        .iter()
        .filter(|source| registry_source_included(source, filter))
        .map(|source| search_one_registry(&client, source, query, limit));

    join_all(futures).await
}

async fn search_one_registry(
    client: &reqwest::Client,
    source: &RegistrySource,
    query: &str,
    limit: usize,
) -> SearchGroup {
    let summary = HubSourceInfo {
        id: source.id.to_string(),
        label: source.label.to_string(),
        origin: source.origin.to_string(),
        trust_level: source.trust_level.to_string(),
    };
    let notice = source.warning.map(|w| w.to_string());

    let result = match source.id {
        "clawhub" => search_clawhub(client, query, limit).await,
        "browse-sh" => search_browse_sh(client, query, limit).await,
        "lobehub" => search_lobehub(client, query, limit).await,
        "claude-marketplace" => search_claude_marketplace(client, query, limit).await,
        "agentskills.io" => search_agentskills_federation(client, query, limit).await,
        _ => Ok(Vec::new()),
    };

    match result {
        Ok(results) => SearchGroup {
            source: summary,
            results,
            notice,
        },
        Err(error) => SearchGroup {
            source: summary,
            results: Vec::new(),
            notice: Some(format!(
                "{}{}",
                notice.map(|n| format!("{n}; ")).unwrap_or_default(),
                error
            )),
        },
    }
}

/// Fetch a skill bundle from a registry-prefixed identifier.
pub async fn fetch_registry_bundle(identifier: &str) -> Result<SkillBundle, String> {
    let normalized = strip_registry_prefix(identifier);
    let client = hub_client()?;

    if normalized.0.eq_ignore_ascii_case("clawhub") {
        return fetch_clawhub_bundle(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("browse-sh")
        || normalized.0.eq_ignore_ascii_case("browse.sh")
    {
        return fetch_browse_sh_bundle(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("skills.sh")
        || normalized.0.eq_ignore_ascii_case("skills-sh")
    {
        return fetch_skills_sh_bundle(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("agentskills.io")
        || normalized.0.eq_ignore_ascii_case("agentskills")
    {
        return fetch_agentskills_bundle(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("lobehub") {
        return fetch_lobehub_bundle(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("claude-marketplace")
        || normalized.0.eq_ignore_ascii_case("claude_marketplace")
    {
        return fetch_claude_marketplace_bundle(&client, &normalized.1).await;
    }

    // Bare slug — try ClawHub then browse.sh (Hermes-style resolution).
    if looks_like_slug(&normalized.1) {
        if let Ok(bundle) = fetch_clawhub_bundle(&client, &normalized.1).await {
            return Ok(bundle);
        }
        if let Ok(bundle) =
            fetch_browse_sh_bundle(&client, &format!("browse-sh/{}", normalized.1)).await
        {
            return Ok(bundle);
        }
    }

    Err(format!(
        "Unknown registry identifier '{identifier}'. Prefix with clawhub:, skills.sh:, browse-sh:, or agentskills.io:"
    ))
}

/// Inspect metadata for a registry identifier (zero install).
pub async fn inspect_registry_skill(identifier: &str) -> Option<SkillMeta> {
    let client = hub_client().ok()?;
    let normalized = strip_registry_prefix(identifier);

    if normalized.0.eq_ignore_ascii_case("clawhub") {
        return inspect_clawhub(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("browse-sh")
        || normalized.0.eq_ignore_ascii_case("browse.sh")
    {
        return inspect_browse_sh(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("skills.sh")
        || normalized.0.eq_ignore_ascii_case("skills-sh")
    {
        return inspect_skills_sh(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("agentskills.io")
        || normalized.0.eq_ignore_ascii_case("agentskills")
    {
        return inspect_agentskills(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("lobehub") {
        return inspect_lobehub(&client, &normalized.1).await;
    }
    if normalized.0.eq_ignore_ascii_case("claude-marketplace")
        || normalized.0.eq_ignore_ascii_case("claude_marketplace")
    {
        return inspect_claude_marketplace(&client, &normalized.1).await;
    }

    inspect_clawhub(&client, &normalized.1)
        .await
        .or(inspect_browse_sh(&client, &normalized.1).await)
}

pub fn strip_registry_prefix(identifier: &str) -> (String, String) {
    let trimmed = identifier.trim().trim_matches('/');
    if let Some((prefix, rest)) = trimmed.split_once(':') {
        return (prefix.to_string(), rest.to_string());
    }
    for prefix in &[
        "clawhub/",
        "skills.sh/",
        "skills-sh/",
        "browse-sh/",
        "browse.sh/",
        "lobehub/",
        "lobehub:",
        "claude-marketplace/",
        "claude-marketplace:",
        "claude_marketplace/",
        "claude_marketplace:",
        "agentskills.io/",
        "agentskills/",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return (prefix.trim_end_matches('/').to_string(), rest.to_string());
        }
    }
    (String::new(), trimmed.to_string())
}

fn looks_like_slug(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('/')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

// ─── Cache helpers ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryCache<T> {
    fetched_at: i64,
    items: T,
}

fn registry_cache_path(name: &str) -> std::path::PathBuf {
    crate::config_ref::resolve_edgecrab_home()
        .join("skills")
        .join(".hub")
        .join("index-cache")
        .join(format!("{name}.json"))
}

fn read_registry_cache<T: for<'de> Deserialize<'de>>(name: &str) -> Option<RegistryCache<T>> {
    let content = std::fs::read_to_string(registry_cache_path(name)).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_registry_cache<T: Serialize>(name: &str, items: T) {
    let path = registry_cache_path(name);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cache = RegistryCache {
        fetched_at: chrono::Utc::now().timestamp(),
        items,
    };
    if let Ok(json) = serde_json::to_string_pretty(&cache) {
        let _ = std::fs::write(path, json);
    }
}

fn cache_fresh(fetched_at: i64) -> bool {
    chrono::Utc::now().timestamp() - fetched_at <= REGISTRY_CACHE_TTL_SECS
}

// ─── ClawHub ───────────────────────────────────────────────────

async fn search_clawhub(
    client: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<SkillMeta>, String> {
    let query = query.trim();
    if query.is_empty() {
        return search_clawhub_listing(client, limit).await;
    }

    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let url = format!("{CLAWHUB_BASE}/skills?search={encoded}&limit={limit}");
    ensure_safe_url(&url)?;
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(SOURCE_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("ClawHub search failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ClawHub returned HTTP {}", resp.status()));
    }
    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("ClawHub JSON parse failed: {e}"))?;
    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(items
        .into_iter()
        .filter_map(clawhub_item_to_meta)
        .take(limit)
        .collect())
}

async fn search_clawhub_listing(
    client: &reqwest::Client,
    limit: usize,
) -> Result<Vec<SkillMeta>, String> {
    if let Some(cache) = read_registry_cache::<Vec<SkillMeta>>("clawhub_listing")
        && cache_fresh(cache.fetched_at)
    {
        return Ok(cache.items.into_iter().take(limit).collect());
    }
    let url = format!("{CLAWHUB_BASE}/skills?limit={limit}");
    ensure_safe_url(&url)?;
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(SOURCE_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("ClawHub listing failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ClawHub returned HTTP {}", resp.status()));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let metas: Vec<SkillMeta> = items.into_iter().filter_map(clawhub_item_to_meta).collect();
    write_registry_cache("clawhub_listing", metas.clone());
    Ok(metas.into_iter().take(limit).collect())
}

fn clawhub_item_to_meta(item: serde_json::Value) -> Option<SkillMeta> {
    let slug = item.get("slug")?.as_str()?.to_string();
    let name = item
        .get("displayName")
        .or_else(|| item.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(&slug)
        .to_string();
    let description = item
        .get("summary")
        .or_else(|| item.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(SkillMeta {
        name,
        description,
        source: "clawhub".into(),
        origin: "https://clawhub.ai".into(),
        identifier: format!("clawhub:{slug}"),
        trust_level: "community".into(),
        repo: None,
        path: None,
        url: Some(format!("https://clawhub.ai/skills/{slug}")),
        tags: normalize_tags(item.get("tags")),
    })
}

async fn inspect_clawhub(client: &reqwest::Client, slug: &str) -> Option<SkillMeta> {
    let slug = slug.split('/').next_back().unwrap_or(slug);
    let url = format!("{CLAWHUB_BASE}/skills/{slug}");
    ensure_safe_url(&url).ok()?;
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: serde_json::Value = resp.json().await.ok()?;
    let skill = data.get("skill").unwrap_or(&data);
    clawhub_item_to_meta(skill.clone()).or_else(|| {
        Some(SkillMeta {
            name: slug.to_string(),
            description: skill
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            source: "clawhub".into(),
            origin: "https://clawhub.ai".into(),
            identifier: format!("clawhub:{slug}"),
            trust_level: "community".into(),
            repo: None,
            path: None,
            url: Some(format!("https://clawhub.ai/skills/{slug}")),
            tags: normalize_tags(skill.get("tags")),
        })
    })
}

async fn fetch_clawhub_bundle(client: &reqwest::Client, slug: &str) -> Result<SkillBundle, String> {
    let slug = slug.split('/').next_back().unwrap_or(slug).to_string();
    let url = format!("{CLAWHUB_BASE}/skills/{slug}");
    ensure_safe_url(&url)?;
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(SOURCE_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("ClawHub inspect failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "ClawHub skill '{slug}' not found (HTTP {})",
            resp.status()
        ));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let skill_data = data.get("skill").unwrap_or(&data);
    let version = resolve_clawhub_version(skill_data, &slug, client).await?;
    let mut files = download_clawhub_zip(client, &slug, &version).await?;
    if !files.contains_key("SKILL.md") {
        files = fetch_clawhub_version_files(client, &slug, &version).await?;
    }
    if !files.contains_key("SKILL.md") {
        return Err(format!(
            "ClawHub skill '{slug}' version '{version}' has no SKILL.md"
        ));
    }
    Ok(SkillBundle {
        name: slug.clone(),
        files,
        source: "clawhub".into(),
        identifier: format!("clawhub:{slug}"),
        trust_level: "community".into(),
    })
}

async fn resolve_clawhub_version(
    skill_data: &serde_json::Value,
    slug: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    if let Some(latest) = skill_data.get("latestVersion") {
        if let Some(v) = latest.as_str()
            && !v.is_empty()
        {
            return Ok(v.to_string());
        }
        if let Some(v) = latest.get("version").and_then(|v| v.as_str())
            && !v.is_empty()
        {
            return Ok(v.to_string());
        }
    }
    if let Some(tags) = skill_data
        .get("tags")
        .and_then(|t| t.get("latest"))
        .and_then(|v| v.as_str())
        && !tags.is_empty()
    {
        return Ok(tags.to_string());
    }
    let url = format!("{CLAWHUB_BASE}/skills/{slug}/versions");
    ensure_safe_url(&url)?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if resp.status().is_success()
        && let Ok(list) = resp.json::<Vec<serde_json::Value>>().await
        && let Some(first) = list
            .first()
            .and_then(|v| v.get("version"))
            .and_then(|v| v.as_str())
    {
        return Ok(first.to_string());
    }
    Err(format!("Could not resolve ClawHub version for '{slug}'"))
}

async fn download_clawhub_zip(
    client: &reqwest::Client,
    slug: &str,
    version: &str,
) -> Result<HashMap<String, String>, String> {
    let url = format!("{CLAWHUB_BASE}/download?slug={slug}&version={version}");
    ensure_safe_url(&url)?;
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("ClawHub download failed: {e}"))?;
    if !resp.status().is_success() {
        return Ok(HashMap::new());
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    extract_zip_text_files(&bytes)
}

async fn fetch_clawhub_version_files(
    client: &reqwest::Client,
    slug: &str,
    version: &str,
) -> Result<HashMap<String, String>, String> {
    let url = format!("{CLAWHUB_BASE}/skills/{slug}/versions/{version}");
    ensure_safe_url(&url)?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Ok(HashMap::new());
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let version_obj = data.get("version").unwrap_or(&data);
    extract_inline_files(version_obj)
}

fn extract_zip_text_files(bytes: &[u8]) -> Result<HashMap<String, String>, String> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| format!("Invalid ZIP: {e}"))?;
    let mut files = HashMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() || entry.size() > 500_000 {
            continue;
        }
        let name = entry.name().to_string();
        let safe = safe_zip_member_path(&name)?;
        let mut buf = String::new();
        if entry.read_to_string(&mut buf).is_ok() {
            files.insert(safe, buf);
        }
    }
    Ok(files)
}

fn safe_zip_member_path(name: &str) -> Result<String, String> {
    let path = Path::new(name);
    let mut normalized = String::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                if !normalized.is_empty() {
                    normalized.push('/');
                }
                normalized.push_str(&part.to_string_lossy());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Unsafe ZIP path '{name}'"));
            }
        }
    }
    if normalized.is_empty() {
        return Err("Empty ZIP member path".into());
    }
    Ok(normalized)
}

fn extract_inline_files(
    version_data: &serde_json::Value,
) -> Result<HashMap<String, String>, String> {
    let mut files = HashMap::new();
    let file_list = version_data.get("files");
    if let Some(map) = file_list.and_then(|f| f.as_object()) {
        for (path, content) in map {
            if let Some(text) = content.as_str() {
                files.insert(path.clone(), text.to_string());
            }
        }
        return Ok(files);
    }
    if let Some(list) = file_list.and_then(|f| f.as_array()) {
        for item in list {
            let path = item
                .get("path")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str());
            if let Some(path) = path
                && let Some(content) = item.get("content").and_then(|v| v.as_str())
            {
                files.insert(path.to_string(), content.to_string());
            }
        }
    }
    Ok(files)
}

// ─── browse.sh ─────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
struct BrowseShItem {
    slug: String,
    name: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn search_browse_sh(
    client: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<SkillMeta>, String> {
    let catalog = load_browse_sh_catalog(client).await?;
    let q = query.trim().to_lowercase();
    Ok(catalog
        .into_iter()
        .filter(|item| {
            if q.is_empty() {
                return true;
            }
            let haystack = format!(
                "{} {} {} {} {} {}",
                item.name,
                item.title,
                item.description,
                item.hostname,
                item.category,
                item.tags.join(" ")
            )
            .to_lowercase();
            q.split_whitespace().all(|token| haystack.contains(token))
        })
        .take(limit)
        .filter_map(browse_sh_to_meta)
        .collect())
}

async fn load_browse_sh_catalog(client: &reqwest::Client) -> Result<Vec<BrowseShItem>, String> {
    if let Some(cache) = read_registry_cache::<Vec<BrowseShItem>>("browse_sh_catalog")
        && cache_fresh(cache.fetched_at)
    {
        return Ok(cache.items);
    }
    ensure_safe_url(BROWSE_SH_CATALOG)?;
    let resp = client
        .get(BROWSE_SH_CATALOG)
        .timeout(Duration::from_secs(SOURCE_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("browse.sh catalog failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("browse.sh returned HTTP {}", resp.status()));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let skills = data
        .get("skills")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let items: Vec<BrowseShItem> = skills
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    write_registry_cache("browse_sh_catalog", items.clone());
    Ok(items)
}

fn browse_sh_to_meta(item: BrowseShItem) -> Option<SkillMeta> {
    if item.slug.is_empty() {
        return None;
    }
    let description = if item.description.is_empty() {
        item.title.clone()
    } else {
        item.description.clone()
    };
    Some(SkillMeta {
        name: if item.name.is_empty() {
            item.slug.clone()
        } else {
            item.name.clone()
        },
        description,
        source: "browse-sh".into(),
        origin: "https://browse.sh".into(),
        identifier: format!("browse-sh:{}", item.slug),
        trust_level: "community".into(),
        repo: None,
        path: None,
        url: Some(format!("https://browse.sh/skills/{}", item.slug)),
        tags: item.tags,
    })
}

async fn inspect_browse_sh(client: &reqwest::Client, identifier: &str) -> Option<SkillMeta> {
    let slug = browse_sh_slug(identifier);
    let catalog = load_browse_sh_catalog(client).await.ok()?;
    catalog.into_iter().find_map(|item| {
        if item.slug == slug {
            browse_sh_to_meta(item)
        } else {
            None
        }
    })
}

async fn fetch_browse_sh_bundle(
    client: &reqwest::Client,
    identifier: &str,
) -> Result<SkillBundle, String> {
    let slug = browse_sh_slug(identifier);
    let catalog = load_browse_sh_catalog(client).await?;
    let item = catalog
        .into_iter()
        .find(|i| i.slug == slug)
        .ok_or_else(|| format!("browse.sh skill '{slug}' not found in catalog"))?;

    let detail_url = format!("{BROWSE_SH_DETAIL}/{slug}");
    ensure_safe_url(&detail_url)?;
    let resp = client
        .get(&detail_url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("browse.sh detail for '{slug}' failed"));
    }
    let detail: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let md_url = detail
        .get("skillMdUrl")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("browse.sh skill '{slug}' has no skillMdUrl"))?;
    ensure_safe_url(md_url)?;
    let md_resp = client.get(md_url).send().await.map_err(|e| e.to_string())?;
    if !md_resp.status().is_success() {
        return Err(format!("Failed to fetch SKILL.md for '{slug}'"));
    }
    let content = md_resp.text().await.map_err(|e| e.to_string())?;
    let name = if item.name.is_empty() {
        slug.clone()
    } else {
        item.name.clone()
    };
    Ok(SkillBundle {
        name,
        files: HashMap::from([("SKILL.md".to_string(), content)]),
        source: "browse-sh".into(),
        identifier: format!("browse-sh:{slug}"),
        trust_level: "community".into(),
    })
}

fn browse_sh_slug(identifier: &str) -> String {
    identifier
        .trim()
        .strip_prefix("browse-sh/")
        .or_else(|| identifier.strip_prefix("browse.sh/"))
        .unwrap_or(identifier)
        .split('/')
        .next_back()
        .unwrap_or(identifier)
        .to_string()
}

// ─── skills.sh install ─────────────────────────────────────────

async fn inspect_skills_sh(client: &reqwest::Client, canonical: &str) -> Option<SkillMeta> {
    let results = super::search_skills_sh_registry(client, canonical, 5)
        .await
        .ok()?;
    results.into_iter().find(|m| {
        m.identifier
            .trim_start_matches("skills.sh:")
            .eq_ignore_ascii_case(canonical)
    })
}

async fn fetch_skills_sh_bundle(
    client: &reqwest::Client,
    canonical: &str,
) -> Result<SkillBundle, String> {
    let canonical = canonical.trim().trim_matches('/');
    let parts: Vec<_> = canonical.split('/').collect();
    if parts.len() < 3 {
        return Err(format!(
            "skills.sh identifier must be owner/repo/skill-path, got '{canonical}'"
        ));
    }
    let repo = format!("{}/{}", parts[0], parts[1]);
    let skill_path = parts[2..].join("/");

    for candidate in [
        format!("skills/{skill_path}"),
        skill_path.clone(),
        format!(".agents/skills/{skill_path}"),
        format!(".claude/skills/{skill_path}"),
    ] {
        let github_id = format!("{repo}/{candidate}");
        if let Ok(bundle) =
            fetch_github_bundle(client, &repo, &candidate, &format!("skills.sh:{canonical}")).await
        {
            return Ok(SkillBundle {
                source: "skills.sh".into(),
                identifier: format!("skills.sh:{canonical}"),
                trust_level: "community".into(),
                ..bundle
            });
        }
        let _ = github_id;
    }

    Err(format!(
        "Could not resolve skills.sh:{canonical} to a GitHub skill directory"
    ))
}

// ─── Claude Marketplace ────────────────────────────────────────

const CLAUDE_MARKETPLACES: &[&str] = &["anthropics/skills", "aiskillstore/marketplace"];

async fn search_claude_marketplace(
    client: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<SkillMeta>, String> {
    let q = query.trim().to_lowercase();
    let mut results = Vec::new();
    for repo in CLAUDE_MARKETPLACES {
        let plugins = load_marketplace_plugins(client, repo).await?;
        for plugin in plugins {
            let name = plugin.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let description = plugin
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let searchable = format!("{name} {description}").to_lowercase();
            if !q.is_empty() && !searchable.contains(&q) {
                continue;
            }
            if let Some(meta) = marketplace_plugin_to_meta(repo, &plugin) {
                results.push(meta);
            }
            if results.len() >= limit {
                return Ok(results);
            }
        }
    }
    Ok(results)
}

async fn load_marketplace_plugins(
    client: &reqwest::Client,
    repo: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let cache_name = format!("claude_marketplace_{}", repo.replace('/', "_"));
    if let Some(cache) = read_registry_cache::<Vec<serde_json::Value>>(&cache_name)
        && cache_fresh(cache.fetched_at)
    {
        return Ok(cache.items);
    }
    let url =
        format!("https://raw.githubusercontent.com/{repo}/HEAD/.claude-plugin/marketplace.json");
    ensure_safe_url(&url)?;
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(SOURCE_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("Claude marketplace index failed for {repo}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Claude marketplace {repo} returned HTTP {}",
            resp.status()
        ));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let plugins = data
        .get("plugins")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    write_registry_cache(&cache_name, plugins.clone());
    Ok(plugins)
}

pub(crate) fn resolve_marketplace_github_id(marketplace_repo: &str, source_path: &str) -> String {
    if source_path.starts_with("./") {
        format!(
            "{marketplace_repo}/{}",
            source_path.trim_start_matches("./")
        )
    } else if source_path.contains('/') {
        source_path.to_string()
    } else {
        format!("{marketplace_repo}/{source_path}")
    }
}

pub(crate) fn marketplace_trust_level(github_id: &str) -> &'static str {
    if github_id.starts_with("anthropics/skills") {
        "trusted"
    } else {
        "community"
    }
}

fn marketplace_plugin_to_meta(
    marketplace_repo: &str,
    plugin: &serde_json::Value,
) -> Option<SkillMeta> {
    let name = plugin.get("name")?.as_str()?.to_string();
    let description = plugin
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source_path = plugin.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let github_id = resolve_marketplace_github_id(marketplace_repo, source_path);
    let (repo, path) = super::parse_github_identifier(&github_id)?;
    Some(SkillMeta {
        name,
        description: description.chars().take(200).collect(),
        source: "claude-marketplace".into(),
        origin: format!("https://github.com/{marketplace_repo}"),
        identifier: format!("claude-marketplace:{github_id}"),
        trust_level: marketplace_trust_level(&github_id).into(),
        repo: Some(repo),
        path: Some(path),
        url: Some(format!("https://github.com/{github_id}")),
        tags: Vec::new(),
    })
}

async fn inspect_claude_marketplace(
    client: &reqwest::Client,
    identifier: &str,
) -> Option<SkillMeta> {
    let github_id = claude_marketplace_github_id(identifier);
    if let Some((repo, path)) = super::parse_github_identifier(&github_id) {
        return Some(SkillMeta {
            name: path.rsplit('/').next().unwrap_or(&github_id).to_string(),
            description: String::new(),
            source: "claude-marketplace".into(),
            origin: format!("https://github.com/{repo}"),
            identifier: format!("claude-marketplace:{github_id}"),
            trust_level: marketplace_trust_level(&github_id).into(),
            repo: Some(repo),
            path: Some(path),
            url: Some(format!("https://github.com/{github_id}")),
            tags: Vec::new(),
        });
    }
    for repo in CLAUDE_MARKETPLACES {
        let plugins = load_marketplace_plugins(client, repo).await.ok()?;
        for plugin in plugins {
            if let Some(meta) = marketplace_plugin_to_meta(repo, &plugin)
                && (meta.identifier.ends_with(identifier)
                    || meta.name.eq_ignore_ascii_case(identifier))
            {
                return Some(meta);
            }
        }
    }
    None
}

async fn fetch_claude_marketplace_bundle(
    client: &reqwest::Client,
    identifier: &str,
) -> Result<SkillBundle, String> {
    let github_id = claude_marketplace_github_id(identifier);
    let (repo, path) = super::parse_github_identifier(&github_id).ok_or_else(|| {
        format!("Invalid Claude marketplace identifier '{identifier}' (expected owner/repo/path)")
    })?;
    let hub_id = format!("claude-marketplace:{github_id}");
    let mut bundle = fetch_github_bundle(client, &repo, &path, &hub_id).await?;
    bundle.source = "claude-marketplace".into();
    bundle.identifier = hub_id;
    bundle.trust_level = marketplace_trust_level(&github_id).into();
    Ok(bundle)
}

fn claude_marketplace_github_id(identifier: &str) -> String {
    let trimmed = identifier.trim();
    let stripped = trimmed
        .strip_prefix("claude-marketplace/")
        .or_else(|| trimmed.strip_prefix("claude_marketplace/"))
        .or_else(|| trimmed.strip_prefix("claude-marketplace:"))
        .or_else(|| trimmed.strip_prefix("claude_marketplace:"))
        .unwrap_or(trimmed);
    stripped.trim_matches('/').to_string()
}

// ─── LobeHub ───────────────────────────────────────────────────

const LOBEHUB_INDEX: &str = "https://chat-agents.lobehub.com/index.json";

async fn search_lobehub(
    client: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<SkillMeta>, String> {
    let index = load_lobehub_index(client).await?;
    let q = query.trim().to_lowercase();
    Ok(index
        .into_iter()
        .filter(|meta| {
            if q.is_empty() {
                return true;
            }
            format!("{} {} {}", meta.name, meta.description, meta.tags.join(" "))
                .to_lowercase()
                .contains(&q)
        })
        .take(limit)
        .collect())
}

async fn load_lobehub_index(client: &reqwest::Client) -> Result<Vec<SkillMeta>, String> {
    if let Some(cache) = read_registry_cache::<Vec<SkillMeta>>("lobehub_index")
        && cache_fresh(cache.fetched_at)
    {
        return Ok(cache.items);
    }
    ensure_safe_url(LOBEHUB_INDEX)?;
    let resp = client
        .get(LOBEHUB_INDEX)
        .timeout(Duration::from_secs(SOURCE_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("LobeHub index failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("LobeHub returned HTTP {}", resp.status()));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let agents = data
        .get("agents")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let metas: Vec<SkillMeta> = agents
        .into_iter()
        .filter_map(lobehub_agent_to_meta)
        .collect();
    write_registry_cache("lobehub_index", metas.clone());
    Ok(metas)
}

fn lobehub_agent_to_meta(agent: serde_json::Value) -> Option<SkillMeta> {
    let meta = agent.get("meta").unwrap_or(&agent);
    let identifier = agent
        .get("identifier")
        .and_then(|v| v.as_str())
        .or_else(|| meta.get("title").and_then(|v| v.as_str()))?
        .to_string();
    let description = meta
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tags = normalize_tags(meta.get("tags"));
    Some(SkillMeta {
        name: identifier.clone(),
        description: description.chars().take(200).collect(),
        source: "lobehub".into(),
        origin: "https://chat-agents.lobehub.com".into(),
        identifier: format!("lobehub:{identifier}"),
        trust_level: "community".into(),
        repo: None,
        path: None,
        url: Some(format!("https://chat-agents.lobehub.com/{identifier}.json")),
        tags,
    })
}

async fn inspect_lobehub(client: &reqwest::Client, identifier: &str) -> Option<SkillMeta> {
    let agent_id = lobehub_slug(identifier);
    let index = load_lobehub_index(client).await.ok()?;
    index.into_iter().find(|m| {
        m.name == agent_id
            || m.identifier.ends_with(&agent_id)
            || m.identifier == format!("lobehub:{agent_id}")
    })
}

async fn fetch_lobehub_bundle(
    client: &reqwest::Client,
    identifier: &str,
) -> Result<SkillBundle, String> {
    let agent_id = lobehub_slug(identifier);
    let url = format!("https://chat-agents.lobehub.com/{agent_id}.json");
    ensure_safe_url(&url)?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("LobeHub agent '{agent_id}' not found"));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let skill_md = lobehub_to_skill_md(&data, &agent_id);
    Ok(SkillBundle {
        name: agent_id.clone(),
        files: HashMap::from([("SKILL.md".to_string(), skill_md)]),
        source: "lobehub".into(),
        identifier: format!("lobehub:{agent_id}"),
        trust_level: "community".into(),
    })
}

fn lobehub_slug(identifier: &str) -> String {
    identifier
        .trim()
        .strip_prefix("lobehub/")
        .or_else(|| identifier.strip_prefix("lobehub:"))
        .unwrap_or(identifier)
        .split('/')
        .next_back()
        .unwrap_or(identifier)
        .to_string()
}

fn lobehub_to_skill_md(agent_data: &serde_json::Value, agent_id: &str) -> String {
    let meta = agent_data.get("meta").unwrap_or(agent_data);
    let title = meta
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or(agent_id);
    let description = meta
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tags = normalize_tags(meta.get("tags"));
    let system_role = agent_data
        .get("config")
        .and_then(|c| c.get("systemRole"))
        .and_then(|v| v.as_str())
        .unwrap_or("(No system role defined)");
    let tag_line = if tags.is_empty() {
        String::new()
    } else {
        format!("    tags: [{}]\n", tags.join(", "))
    };
    format!(
        "---\nname: {agent_id}\ndescription: {}\nmetadata:\n  edgecrab:\n{tag_line}  lobehub:\n    source: lobehub\n---\n\n# {title}\n\n{description}\n\n## Instructions\n\n{system_role}\n",
        description.chars().take(500).collect::<String>()
    )
}

// ─── agentskills.io federation ─────────────────────────────────

const FEDERATION_ENDPOINTS: &[&str] = &["https://agentskills.io"];

async fn search_agentskills_federation(
    client: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<SkillMeta>, String> {
    let mut all = Vec::new();
    for base in FEDERATION_ENDPOINTS {
        if let Ok(mut items) = fetch_well_known_index(client, base).await {
            if !query.trim().is_empty() {
                let q = query.to_lowercase();
                items.retain(|m| {
                    format!("{} {}", m.name, m.description)
                        .to_lowercase()
                        .contains(&q)
                });
            }
            all.extend(items);
        }
    }
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all.truncate(limit);
    Ok(all)
}

async fn fetch_well_known_index(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<Vec<SkillMeta>, String> {
    let cache_key = format!(
        "federation_{}",
        base_url.replace("https://", "").replace('/', "_")
    );
    if let Some(cache) = read_registry_cache::<Vec<SkillMeta>>(&cache_key)
        && cache_fresh(cache.fetched_at)
    {
        return Ok(cache.items);
    }
    let url = format!(
        "{}/.well-known/skills/index.json",
        base_url.trim_end_matches('/')
    );
    ensure_safe_url(&url)?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Federation index HTTP {}", resp.status()));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let skills = data
        .get("skills")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let metas: Vec<SkillMeta> = skills
        .into_iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            Some(SkillMeta {
                name: name.clone(),
                description: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                source: "agentskills.io".into(),
                origin: base_url.to_string(),
                identifier: format!("agentskills.io:{name}"),
                trust_level: "trusted".into(),
                repo: None,
                path: Some(name.clone()),
                url: Some(format!(
                    "{}/.well-known/skills/{}",
                    base_url.trim_end_matches('/'),
                    name
                )),
                tags: vec!["agentskills.io".into(), "federation".into()],
            })
        })
        .collect();
    write_registry_cache(&cache_key, metas.clone());
    Ok(metas)
}

async fn inspect_agentskills(client: &reqwest::Client, name: &str) -> Option<SkillMeta> {
    let name = name.split('/').next_back().unwrap_or(name);
    for base in FEDERATION_ENDPOINTS {
        if let Ok(items) = fetch_well_known_index(client, base).await
            && let Some(meta) = items
                .into_iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
        {
            return Some(meta);
        }
    }
    None
}

async fn fetch_agentskills_bundle(
    client: &reqwest::Client,
    name: &str,
) -> Result<SkillBundle, String> {
    let name = name.split('/').next_back().unwrap_or(name).to_string();
    for base in FEDERATION_ENDPOINTS {
        let url = format!(
            "{}/.well-known/skills/{name}/SKILL.md",
            base.trim_end_matches('/')
        );
        if ensure_safe_url(&url).is_err() {
            continue;
        }
        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !resp.status().is_success() {
            continue;
        }
        let content = resp.text().await.map_err(|e| e.to_string())?;
        return Ok(SkillBundle {
            name: name.clone(),
            files: HashMap::from([("SKILL.md".to_string(), content)]),
            source: "agentskills.io".into(),
            identifier: format!("agentskills.io:{name}"),
            trust_level: "trusted".into(),
        });
    }
    Err(format!(
        "agentskills.io skill '{name}' not found in federation endpoints"
    ))
}

/// Install a skill from a direct URL to SKILL.md (Hermes UrlSource parity).
pub async fn fetch_url_skill_bundle(
    client: &reqwest::Client,
    url: &str,
) -> Result<SkillBundle, String> {
    ensure_safe_url(url)?;
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(SOURCE_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("URL fetch failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("URL returned HTTP {}", resp.status()));
    }
    let content = resp.text().await.map_err(|e| e.to_string())?;
    if !content.trim_start().starts_with("---") && !content.contains("# ") {
        return Err("URL content does not look like a SKILL.md document".into());
    }
    let name = infer_skill_name_from_url_and_content(url, &content);
    Ok(SkillBundle {
        name,
        files: HashMap::from([("SKILL.md".to_string(), content)]),
        source: "url".into(),
        identifier: url.to_string(),
        trust_level: "community".into(),
    })
}

fn infer_skill_name_from_url_and_content(url: &str, content: &str) -> String {
    if let Some(meta) = parse_frontmatter_name(content) {
        return meta;
    }
    if let Ok(parsed) = url::Url::parse(url) {
        let path = parsed.path();
        if path.ends_with("/SKILL.md") {
            let parts: Vec<_> = path.trim_end_matches("/SKILL.md").split('/').collect();
            if let Some(last) = parts.last()
                && !last.is_empty()
            {
                return last.to_string();
            }
        }
        if let Some(last) = parsed.path_segments().and_then(|mut s| s.next_back()) {
            return last.trim_end_matches(".md").to_string();
        }
    }
    "skill".into()
}

fn parse_frontmatter_name(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after = trimmed.strip_prefix("---")?;
    let end = after.find("\n---")?;
    for line in after[..end].lines() {
        if let Some(name) = line.strip_prefix("name:") {
            let val = name.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn normalize_tags(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(serde_json::Value::Object(map)) => map
            .keys()
            .filter(|k| *k != "latest")
            .map(|k| k.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_registry_prefix_handles_colon_and_slash_forms() {
        assert_eq!(
            strip_registry_prefix("clawhub:my-skill"),
            ("clawhub".into(), "my-skill".into())
        );
        assert_eq!(
            strip_registry_prefix("skills.sh:owner/repo/skill"),
            ("skills.sh".into(), "owner/repo/skill".into())
        );
        assert_eq!(
            strip_registry_prefix("browse-sh/automation/airbnb"),
            ("browse-sh".into(), "automation/airbnb".into())
        );
    }

    #[test]
    fn safe_zip_member_rejects_traversal() {
        assert!(safe_zip_member_path("../evil/SKILL.md").is_err());
        assert_eq!(
            safe_zip_member_path("scripts/run.py").unwrap(),
            "scripts/run.py"
        );
    }

    #[test]
    fn registry_filter_matches_aliases() {
        assert!(registry_filter_includes_any("clawhub"));
        assert!(registry_filter_includes_any("browse.sh"));
        assert!(registry_filter_includes_any("lobehub"));
        assert!(registry_source_included(&REGISTRY_SOURCES[0], "clawhub"));
    }

    #[test]
    fn lobehub_slug_strips_prefixes() {
        assert_eq!(lobehub_slug("lobehub:my-agent"), "my-agent");
        assert_eq!(lobehub_slug("lobehub/my-agent"), "my-agent");
        assert_eq!(lobehub_slug("plain-agent"), "plain-agent");
    }

    #[test]
    fn lobehub_to_skill_md_includes_system_role() {
        let agent = serde_json::json!({
            "meta": { "title": "Demo", "description": "A demo agent", "tags": ["test"] },
            "config": { "systemRole": "You are helpful." }
        });
        let md = lobehub_to_skill_md(&agent, "demo-agent");
        assert!(md.contains("name: demo-agent"));
        assert!(md.contains("You are helpful."));
        assert!(md.contains("lobehub:"));
    }

    #[test]
    fn marketplace_github_id_resolution() {
        assert_eq!(
            resolve_marketplace_github_id("anthropics/skills", "./document-skills/pdf"),
            "anthropics/skills/document-skills/pdf"
        );
        assert_eq!(
            resolve_marketplace_github_id("aiskillstore/marketplace", "owner/repo/skill"),
            "owner/repo/skill"
        );
        assert_eq!(
            claude_marketplace_github_id("claude-marketplace:anthropics/skills/foo"),
            "anthropics/skills/foo"
        );
        assert_eq!(marketplace_trust_level("anthropics/skills/x"), "trusted");
        assert_eq!(
            marketplace_trust_level("aiskillstore/marketplace/x"),
            "community"
        );
    }
}
