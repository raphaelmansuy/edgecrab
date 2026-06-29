//! Unified skills index — instant search without hammering live registries.
//!
//! EdgeCrab exceeds Hermes first-principles here:
//! - Seeds from the public Hermes skills-index when available (resolved GitHub paths)
//! - Persists a **local unified index** at `~/.edgecrab/skills/.hub/unified-index.json`
//! - **Merges live search hits** back into the index (self-improving cache)
//! - When the index is warm, skips redundant live API calls (github/clawhub/skills-sh)

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{HubSourceInfo, SearchGroup, SearchReport, SkillBundle, SkillMeta};

/// Public Hermes CI-built catalog (resolved install paths). Best-effort fetch.
const REMOTE_INDEX_URL: &str = "https://hermes-agent.nousresearch.com/docs/api/skills-index.json";
const INDEX_TTL_SECS: i64 = 6 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UnifiedIndexFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    fetched_at: i64,
    #[serde(default)]
    source: String,
    #[serde(default)]
    skills: Vec<IndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IndexEntry {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    source: String,
    identifier: String,
    #[serde(default)]
    trust_level: String,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    resolved_github_id: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

fn index_path() -> PathBuf {
    crate::config_ref::resolve_edgecrab_home()
        .join("skills")
        .join(".hub")
        .join("unified-index.json")
}

fn read_index_file() -> Option<UnifiedIndexFile> {
    let content = std::fs::read_to_string(index_path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_index_file(index: &UnifiedIndexFile) -> Result<(), String> {
    let path = index_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

fn index_is_fresh(index: &UnifiedIndexFile) -> bool {
    if index.skills.is_empty() {
        return false;
    }
    chrono::Utc::now().timestamp() - index.fetched_at <= INDEX_TTL_SECS
}

fn entry_to_meta(entry: &IndexEntry) -> SkillMeta {
    SkillMeta {
        name: entry.name.clone(),
        description: entry.description.clone(),
        source: if entry.source.is_empty() {
            "unified-index".into()
        } else {
            entry.source.clone()
        },
        origin: "local unified index".into(),
        identifier: entry.identifier.clone(),
        trust_level: if entry.trust_level.is_empty() {
            "community".into()
        } else {
            entry.trust_level.clone()
        },
        repo: entry.repo.clone(),
        path: entry.path.clone(),
        url: entry.url.clone(),
        tags: entry.tags.clone(),
    }
}

fn meta_to_entry(meta: &SkillMeta, resolved_github_id: Option<String>) -> IndexEntry {
    IndexEntry {
        name: meta.name.clone(),
        description: meta.description.clone(),
        source: meta.source.clone(),
        identifier: meta.identifier.clone(),
        trust_level: meta.trust_level.clone(),
        repo: meta.repo.clone(),
        path: meta.path.clone(),
        resolved_github_id,
        url: meta.url.clone(),
        tags: meta.tags.clone(),
    }
}

fn index_entry_score(entry: &IndexEntry, query: &str) -> usize {
    let q = query.to_lowercase();
    let name = entry.name.to_lowercase();
    let id = entry.identifier.to_lowercase();
    let desc = entry.description.to_lowercase();
    if name == q {
        0
    } else if id == q {
        1
    } else if name.starts_with(&q) {
        2
    } else if id.starts_with(&q) {
        3
    } else if name.contains(&q) {
        4
    } else if id.contains(&q) {
        5
    } else if desc.contains(&q) {
        6
    } else {
        7
    }
}

fn index_entry_matches(entry: &IndexEntry, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {} {}",
        entry.name,
        entry.identifier,
        entry.description,
        entry.tags.join(" "),
        entry.source
    )
    .to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|token| haystack.contains(token))
}

/// True when the unified index has skills and can serve `filter=all` searches alone.
pub fn unified_index_available() -> bool {
    read_index_file()
        .map(|index| !index.skills.is_empty())
        .unwrap_or(false)
}

/// Search the local unified index (sync, zero network).
pub fn search_unified_index(query: &str, limit: usize) -> SearchGroup {
    let summary = HubSourceInfo {
        id: "unified-index".into(),
        label: "Unified Index".into(),
        origin: "local cache".into(),
        trust_level: "mixed".into(),
    };

    let Some(index) = read_index_file() else {
        return SearchGroup {
            source: summary,
            results: Vec::new(),
            notice: Some(
                "Unified index not built yet — run `/skills search <query>` to seed from live sources."
                    .into(),
            ),
        };
    };

    if index.skills.is_empty() {
        return SearchGroup {
            source: summary,
            results: Vec::new(),
            notice: Some("Unified index empty — searching live sources.".into()),
        };
    }

    let mut ranked: Vec<_> = index
        .skills
        .iter()
        .filter(|entry| query.trim().is_empty() || index_entry_matches(entry, query))
        .collect();
    ranked.sort_by_key(|entry| index_entry_score(entry, query));
    let results: Vec<SkillMeta> = ranked.into_iter().take(limit).map(entry_to_meta).collect();

    let notice = if index_is_fresh(&index) {
        None
    } else {
        Some(format!(
            "index age {}h — live sources may supplement stale entries",
            (chrono::Utc::now().timestamp() - index.fetched_at) / 3600
        ))
    };

    SearchGroup {
        source: summary,
        results,
        notice,
    }
}

/// Merge search report hits into the persistent unified index.
pub fn merge_search_report_into_index(report: &SearchReport) {
    let mut index = read_index_file().unwrap_or(UnifiedIndexFile {
        version: 1,
        fetched_at: chrono::Utc::now().timestamp(),
        source: "edgecrab-merge".into(),
        skills: Vec::new(),
    });

    let mut by_id: std::collections::HashMap<String, IndexEntry> = index
        .skills
        .into_iter()
        .map(|e| (e.identifier.to_lowercase(), e))
        .collect();

    for group in &report.groups {
        for meta in &group.results {
            let resolved = meta
                .repo
                .as_ref()
                .zip(meta.path.as_ref())
                .map(|(repo, path)| format!("{repo}/{path}"));
            let entry = meta_to_entry(meta, resolved);
            by_id.insert(entry.identifier.to_lowercase(), entry);
        }
    }

    index.skills = by_id.into_values().collect();
    index.fetched_at = chrono::Utc::now().timestamp();
    index.source = "edgecrab-merge".into();
    let _ = write_index_file(&index);
}

/// Fetch remote Hermes index into the local unified cache (best-effort).
pub async fn refresh_unified_index_from_remote(client: &reqwest::Client) -> Result<usize, String> {
    super::ensure_safe_url(REMOTE_INDEX_URL)?;
    let resp = client
        .get(REMOTE_INDEX_URL)
        .send()
        .await
        .map_err(|e| format!("index fetch failed: {e}"))?;

    if !resp.status().is_success() {
        if let Some(stale) = read_index_file()
            && !stale.skills.is_empty()
        {
            return Ok(stale.skills.len());
        }
        return Err(format!("index HTTP {}", resp.status()));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let skills = data
        .get("skills")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let entries: Vec<IndexEntry> = skills
        .into_iter()
        .filter_map(|item| {
            let identifier = item.get("identifier")?.as_str()?.to_string();
            Some(IndexEntry {
                name: item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&identifier)
                    .to_string(),
                description: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                source: item
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hermes-index")
                    .to_string(),
                identifier,
                trust_level: item
                    .get("trust_level")
                    .and_then(|v| v.as_str())
                    .unwrap_or("community")
                    .to_string(),
                repo: item.get("repo").and_then(|v| v.as_str()).map(String::from),
                path: item.get("path").and_then(|v| v.as_str()).map(String::from),
                resolved_github_id: item
                    .get("resolved_github_id")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                url: item
                    .get("extra")
                    .and_then(|e| e.get("detail_url"))
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("url").and_then(|v| v.as_str()))
                    .map(String::from),
                tags: item
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|t| t.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();

    let count = entries.len();
    let file = UnifiedIndexFile {
        version: 1,
        fetched_at: chrono::Utc::now().timestamp(),
        source: REMOTE_INDEX_URL.into(),
        skills: entries,
    };
    write_index_file(&file)?;
    Ok(count)
}

/// Install lookup: find index entry by identifier (exact or prefix-stripped).
pub fn find_index_entry(identifier: &str) -> Option<IndexEntry> {
    let index = read_index_file()?;
    let normalized = normalize_index_lookup_key(identifier);
    index.skills.into_iter().find(|entry| {
        normalize_index_lookup_key(&entry.identifier) == normalized
            || entry.name.eq_ignore_ascii_case(identifier)
    })
}

fn normalize_index_lookup_key(identifier: &str) -> String {
    let trimmed = identifier.trim();
    for prefix in [
        "skills-sh/",
        "skills.sh/",
        "clawhub/",
        "browse-sh/",
        "github/",
        "official/",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.to_lowercase();
        }
    }
    trimmed.to_lowercase()
}

/// Format index status for `/skills index status`.
pub fn format_index_status() -> String {
    let Some(index) = read_index_file() else {
        return "Unified index: not built.\nRun: /skills index refresh".into();
    };
    let age_hours = (chrono::Utc::now().timestamp() - index.fetched_at) / 3600;
    format!(
        "Unified index: {} skills\nSource: {}\nAge: {}h{}\nPath: {}",
        index.skills.len(),
        index.source,
        age_hours,
        if index_is_fresh(&index) {
            " (fresh)"
        } else {
            " (stale — run /skills index refresh)"
        },
        index_path().display()
    )
}

/// Seed unified index from curated GitHub tree caches (zero network).
pub fn bootstrap_index_from_local_caches() -> usize {
    let cache_dir = crate::config_ref::resolve_edgecrab_home()
        .join("skills")
        .join(".hub")
        .join("index-cache");

    #[derive(Deserialize)]
    struct LocalCache {
        #[serde(default)]
        entries: Vec<LocalEntry>,
    }
    #[derive(Deserialize)]
    struct LocalEntry {
        name: String,
        relative_path: String,
        identifier: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        tags: Vec<String>,
    }

    let repos: &[(&str, &str, &str)] = &[
        ("edgecrab", "raphaelmansuy/edgecrab", "trusted"),
        ("hermes-agent", "NousResearch/hermes-agent", "trusted"),
        ("openai", "openai/skills", "trusted"),
        ("anthropics", "anthropics/skills", "trusted"),
    ];

    let mut merged: std::collections::HashMap<String, IndexEntry> = read_index_file()
        .map(|f| {
            f.skills
                .into_iter()
                .map(|e| (e.identifier.to_lowercase(), e))
                .collect()
        })
        .unwrap_or_default();

    for (source_id, repo, trust) in repos {
        let path = cache_dir.join(format!("{source_id}.json"));
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(cache) = serde_json::from_str::<LocalCache>(&content) else {
            continue;
        };
        for entry in cache.entries {
            let github_path = format!("skills/{}", entry.relative_path);
            merged.insert(
                entry.identifier.to_lowercase(),
                IndexEntry {
                    name: entry.name,
                    description: entry.description,
                    source: (*source_id).into(),
                    identifier: entry.identifier,
                    trust_level: (*trust).into(),
                    repo: Some((*repo).into()),
                    path: Some(github_path.clone()),
                    resolved_github_id: Some(format!("{repo}/{github_path}")),
                    url: Some(format!("https://github.com/{repo}/tree/HEAD/{github_path}")),
                    tags: entry.tags,
                },
            );
        }
    }

    // Claude marketplace plugin caches (claude_marketplace_*.json)
    if let Ok(read_dir) = std::fs::read_dir(&cache_dir) {
        for entry in read_dir.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if !fname.starts_with("claude_marketplace_") || !fname.ends_with(".json") {
                continue;
            }
            let marketplace_repo = fname
                .trim_start_matches("claude_marketplace_")
                .trim_end_matches(".json")
                .replace('_', "/");
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            #[derive(Deserialize)]
            struct PluginCache {
                #[serde(default)]
                #[allow(dead_code)]
                fetched_at: i64,
                items: Vec<serde_json::Value>,
            }
            let Ok(cache) = serde_json::from_str::<PluginCache>(&content) else {
                continue;
            };
            for plugin in cache.items {
                let Some(github_id) = (|| {
                    let source_path = plugin.get("source")?.as_str()?;
                    Some(super::sources::resolve_marketplace_github_id(
                        &marketplace_repo,
                        source_path,
                    ))
                })() else {
                    continue;
                };
                let name = plugin
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&github_id)
                    .to_string();
                let description = plugin
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let identifier = format!("claude-marketplace:{github_id}");
                let (repo, path) = match super::parse_github_identifier(&github_id) {
                    Some(pair) => pair,
                    None => continue,
                };
                merged.insert(
                    identifier.to_lowercase(),
                    IndexEntry {
                        name,
                        description,
                        source: "claude-marketplace".into(),
                        identifier: identifier.clone(),
                        trust_level: super::sources::marketplace_trust_level(&github_id).into(),
                        repo: Some(repo),
                        path: Some(path.clone()),
                        resolved_github_id: Some(github_id.clone()),
                        url: Some(format!("https://github.com/{github_id}")),
                        tags: Vec::new(),
                    },
                );
            }
        }
    }

    seed_index_from_repo_trees(&mut merged);
    seed_index_from_embedded_catalog(&mut merged);

    let count = merged.len();
    if count == 0 {
        return 0;
    }

    let file = UnifiedIndexFile {
        version: 1,
        fetched_at: chrono::Utc::now().timestamp(),
        source: "local-cache-bootstrap".into(),
        skills: merged.into_values().collect(),
    };
    let _ = write_index_file(&file);
    count
}

fn seed_index_from_repo_trees(merged: &mut std::collections::HashMap<String, IndexEntry>) {
    if let Some(bundled) = crate::tools::skills_sync::bundled_skills_dir() {
        walk_skills_tree(
            &bundled,
            "edgecrab",
            "raphaelmansuy/edgecrab",
            "skills",
            "trusted",
            merged,
        );
    }
    if let Some(optional) = crate::tools::skills_sync::optional_skills_dir() {
        walk_skills_tree(
            &optional,
            "edgecrab",
            "raphaelmansuy/edgecrab",
            "optional-skills",
            "trusted",
            merged,
        );
    }
}

fn walk_skills_tree(
    root: &std::path::Path,
    source_id: &str,
    repo: &str,
    tree_prefix: &str,
    trust: &str,
    merged: &mut std::collections::HashMap<String, IndexEntry>,
) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.join("SKILL.md").is_file() {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let display_name = relative.rsplit('/').next().unwrap_or(&relative).to_string();
                let github_path = format!("{tree_prefix}/{relative}");
                let identifier = format!("{source_id}:{relative}");
                let resolved = format!("{repo}/{github_path}");
                merged.insert(
                    identifier.to_lowercase(),
                    IndexEntry {
                        name: display_name,
                        description: String::new(),
                        source: source_id.into(),
                        identifier,
                        trust_level: trust.into(),
                        repo: Some(repo.into()),
                        path: Some(github_path),
                        resolved_github_id: Some(resolved.clone()),
                        url: Some(format!("https://github.com/{resolved}")),
                        tags: Vec::new(),
                    },
                );
            } else {
                stack.push(path);
            }
        }
    }
}

fn seed_index_from_embedded_catalog(merged: &mut std::collections::HashMap<String, IndexEntry>) {
    for (skills, tree_prefix) in [
        (
            crate::tools::skills_sync::embedded_bundled_skills(),
            "skills",
        ),
        (
            crate::tools::skills_sync::embedded_optional_skills(),
            "optional-skills",
        ),
    ] {
        for skill in skills {
            let relative = skill.name;
            let display_name = relative.rsplit('/').next().unwrap_or(relative).to_string();
            let github_path = format!("{tree_prefix}/{relative}");
            let identifier = format!("edgecrab:{relative}");
            let resolved = format!("raphaelmansuy/edgecrab/{github_path}");
            merged.insert(
                identifier.to_lowercase(),
                IndexEntry {
                    name: display_name,
                    description: String::new(),
                    source: "edgecrab".into(),
                    identifier,
                    trust_level: "trusted".into(),
                    repo: Some("raphaelmansuy/edgecrab".into()),
                    path: Some(github_path),
                    resolved_github_id: Some(resolved.clone()),
                    url: Some(format!("https://github.com/{resolved}")),
                    tags: Vec::new(),
                },
            );
        }
    }
}

/// Inspect via unified index (zero network).
pub fn inspect_index_identifier(identifier: &str) -> Option<SkillMeta> {
    find_index_entry(identifier).map(|entry| index_entry_as_meta(&entry))
}

/// Install via unified index resolved GitHub path.
pub async fn try_fetch_from_index(identifier: &str) -> Option<SkillBundle> {
    let entry = find_index_entry(identifier)?;
    fetch_bundle_from_entry(&entry, identifier).await.ok()
}

pub fn index_entry_as_meta(entry: &IndexEntry) -> SkillMeta {
    entry_to_meta(entry)
}

/// Resolve an index entry to a downloadable bundle via resolved GitHub path.
pub async fn fetch_bundle_from_entry(
    entry: &IndexEntry,
    normalized_identifier: &str,
) -> Result<SkillBundle, String> {
    let client = super::hub_client()?;
    if let Some(resolved) = &entry.resolved_github_id
        && let Some((repo, path)) = super::parse_github_identifier(resolved)
    {
        let mut bundle =
            super::fetch_github_bundle(&client, &repo, &path, normalized_identifier).await?;
        bundle.source = entry.source.clone();
        bundle.identifier = entry.identifier.clone();
        return Ok(bundle);
    }
    if let (Some(repo), Some(path)) = (&entry.repo, &entry.path) {
        let mut bundle =
            super::fetch_github_bundle(&client, repo, path, normalized_identifier).await?;
        bundle.source = entry.source.clone();
        bundle.identifier = entry.identifier.clone();
        return Ok(bundle);
    }
    Err(format!(
        "Index entry '{}' has no resolved install path",
        entry.identifier
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_entry_score_prefers_exact_name() {
        let entry = IndexEntry {
            name: "ascii-diagram-fixer".into(),
            description: "fix diagrams".into(),
            source: "skills.sh".into(),
            identifier: "skills-sh/foo/bar/ascii-diagram-fixer".into(),
            trust_level: "community".into(),
            repo: None,
            path: None,
            resolved_github_id: None,
            url: None,
            tags: vec![],
        };
        assert!(index_entry_matches(&entry, "diagram fixer"));
        assert!(
            index_entry_score(&entry, "ascii-diagram-fixer") < index_entry_score(&entry, "diagram")
        );
    }

    #[test]
    fn normalize_lookup_strips_prefixes() {
        assert_eq!(normalize_index_lookup_key("clawhub/my-skill"), "my-skill");
    }
}
