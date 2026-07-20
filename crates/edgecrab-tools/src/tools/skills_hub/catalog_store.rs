//! Per-source marketplace catalog SoT for TUI page-on-demand extend.
//!
//! First paint still uses progressive `search_hub` (page size 80). Scroll extend
//! calls [`catalog_page`] against local caches; [`ensure_catalog`] fills those
//! caches out-of-band when incomplete.

use super::catalog::{CatalogKind, HUB_CATALOG, MARKETPLACE_BROWSE_FETCH_MAX};
use super::{HubSourceInfo, SearchGroup, SkillMeta, hub_client_catalog};

/// Per-source catalog store used by TUI virtual-scroll extend.
pub trait SkillCatalogStore: Send + Sync {
    fn source_id(&self) -> &str;
    fn total(&self) -> Option<usize>;
    fn complete(&self) -> bool;
    /// Sync SoT slice (preferred path for TUI extend).
    fn page(&self, offset: usize, limit: usize) -> Vec<SkillMeta>;
}

/// Filter-keyed store wrapping [`catalog_page`] / [`catalog_total`] / [`catalog_complete`].
#[derive(Debug, Clone)]
pub struct FilterCatalogStore {
    filter: String,
}

impl FilterCatalogStore {
    pub fn for_filter(filter: &str) -> Self {
        Self {
            filter: normalize_filter(filter),
        }
    }

    pub fn page_groups(&self, offset: usize, limit: usize) -> CatalogPage {
        catalog_page(&self.filter, offset, limit)
    }

    pub fn total(&self) -> Option<usize> {
        catalog_total(&self.filter)
    }

    pub fn complete(&self) -> bool {
        catalog_complete(&self.filter)
    }

    pub async fn ensure(&self) -> Result<(), String> {
        ensure_catalog(&self.filter).await
    }
}

impl SkillCatalogStore for FilterCatalogStore {
    fn source_id(&self) -> &str {
        &self.filter
    }

    fn total(&self) -> Option<usize> {
        catalog_total(&self.filter)
    }

    fn complete(&self) -> bool {
        catalog_complete(&self.filter)
    }

    fn page(&self, offset: usize, limit: usize) -> Vec<SkillMeta> {
        catalog_page(&self.filter, offset, limit)
            .groups
            .into_iter()
            .flat_map(|g| g.results)
            .collect()
    }
}

/// Sync page from the filter's authoritative catalog cache.
#[derive(Debug, Clone, Default)]
pub struct CatalogPage {
    pub groups: Vec<SearchGroup>,
    pub total: Option<usize>,
    pub complete: bool,
}

impl CatalogPage {
    /// Row count in this page (pre-dedup SoT slice size).
    pub fn row_count(&self) -> usize {
        self.groups.iter().map(|g| g.results.len()).sum()
    }
}

/// Page the SoT for a marketplace filter (`all`, `openai`, `skills-sh`, …).
pub fn catalog_page(filter: &str, offset: usize, limit: usize) -> CatalogPage {
    let filter = normalize_filter(filter);
    match filter.as_str() {
        "all" | "" => page_all(offset, limit),
        "index" | "unified-index" => page_index(offset, limit),
        "skills-sh" | "skills.sh" | "skillssh" | "skills_sh" => page_skills_sh(offset, limit),
        "clawhub" => page_clawhub(offset, limit),
        "voltagent" => page_voltagent(offset, limit),
        other if super::is_provider_filter(other) => page_github_provider(other, offset, limit),
        other => {
            if let Some(entry) = HUB_CATALOG
                .iter()
                .find(|e| e.id.eq_ignore_ascii_case(other))
            {
                page_github_source_ids(&[entry.id], offset, limit)
            } else {
                CatalogPage::default()
            }
        }
    }
}

pub fn catalog_total(filter: &str) -> Option<usize> {
    let filter = normalize_filter(filter);
    match filter.as_str() {
        "all" | "" => {
            let idx = super::unified_index_len().unwrap_or(0);
            let sh = super::skills_sh_sitemap_cache_len().unwrap_or(0);
            // Footer uses max: All merges by identifier so sum over-counts.
            let t = idx.max(sh);
            (t > 0).then_some(t)
        }
        "index" | "unified-index" => super::unified_index_len(),
        "skills-sh" | "skills.sh" | "skillssh" | "skills_sh" => {
            super::skills_sh_sitemap_cache_len()
        }
        "clawhub" => super::sources::clawhub_listing_cache_len(),
        "voltagent" => super::sources::registry_meta_cache_len("voltagent_awesome"),
        other if super::is_provider_filter(other) => github_provider_total(other),
        other => github_source_total(other),
    }
}

pub fn catalog_complete(filter: &str) -> bool {
    let filter = normalize_filter(filter);
    match filter.as_str() {
        "all" | "" => {
            // Index ready; skills.sh may still be filling (ensure keeps All growing).
            super::unified_index_len().is_some_and(|n| n > 0)
                && (super::skills_sh_sitemap_catalog_complete()
                    || super::skills_sh_sitemap_cache_len().is_none())
        }
        "index" | "unified-index" => super::unified_index_len().is_some_and(|n| n > 0),
        "skills-sh" | "skills.sh" | "skillssh" | "skills_sh" => {
            super::skills_sh_sitemap_catalog_complete()
        }
        "clawhub" => super::sources::clawhub_listing_catalog_complete(),
        "voltagent" => super::sources::registry_meta_cache_complete("voltagent_awesome"),
        other if super::is_provider_filter(other) => github_provider_complete(other),
        other => github_source_complete(other),
    }
}

/// Fill/refresh the SoT for a filter (no short first-paint timeout).
pub async fn ensure_catalog(filter: &str) -> Result<(), String> {
    let filter = normalize_filter(filter);
    match filter.as_str() {
        "all" | "" => {
            super::index::bootstrap_index_from_local_caches();
            let _ = super::ensure_skills_sh_sitemap_catalog().await;
            Ok(())
        }
        "index" | "unified-index" => {
            super::index::bootstrap_index_from_local_caches();
            Ok(())
        }
        "skills-sh" | "skills.sh" | "skillssh" | "skills_sh" => {
            super::ensure_skills_sh_sitemap_catalog().await
        }
        "clawhub" => super::sources::ensure_clawhub_listing_catalog().await,
        "voltagent" => ensure_voltagent_catalog().await,
        other if super::is_provider_filter(other) => ensure_github_provider(other).await,
        other => ensure_github_source_id(other).await,
    }
}

fn normalize_filter(filter: &str) -> String {
    let f = filter.trim().to_ascii_lowercase();
    if f.is_empty() { "all".into() } else { f }
}

fn page_all(offset: usize, limit: usize) -> CatalogPage {
    let mut groups = Vec::new();
    let idx = super::browse_unified_index_slice("", offset, limit.max(1));
    if !idx.results.is_empty() || idx.notice.is_some() {
        groups.push(idx);
    }
    if let Some(g) = super::browse_skills_sh_cache_slice(offset, limit.max(1)) {
        groups.push(g);
    }
    CatalogPage {
        groups,
        total: catalog_total("all"),
        complete: catalog_complete("all"),
    }
}

fn page_index(offset: usize, limit: usize) -> CatalogPage {
    let g = super::browse_unified_index_slice("", offset, limit.max(1));
    let total = super::unified_index_len();
    CatalogPage {
        groups: if g.results.is_empty() && g.notice.is_none() {
            Vec::new()
        } else {
            vec![g]
        },
        total,
        complete: total.is_some_and(|n| n > 0),
    }
}

fn page_skills_sh(offset: usize, limit: usize) -> CatalogPage {
    CatalogPage {
        groups: super::browse_skills_sh_cache_slice(offset, limit.max(1))
            .into_iter()
            .collect(),
        total: super::skills_sh_sitemap_cache_len(),
        complete: super::skills_sh_sitemap_catalog_complete(),
    }
}

fn page_clawhub(offset: usize, limit: usize) -> CatalogPage {
    CatalogPage {
        groups: super::sources::browse_clawhub_cache_slice(offset, limit.max(1))
            .into_iter()
            .collect(),
        total: super::sources::clawhub_listing_cache_len(),
        complete: super::sources::clawhub_listing_catalog_complete(),
    }
}

fn page_voltagent(offset: usize, limit: usize) -> CatalogPage {
    CatalogPage {
        groups: super::sources::browse_registry_meta_cache_slice(
            "voltagent_awesome",
            "voltagent",
            "VoltAgent",
            "https://github.com/voltagent/awesome-agent-skills",
            "community",
            offset,
            limit.max(1),
        )
        .into_iter()
        .collect(),
        total: super::sources::registry_meta_cache_len("voltagent_awesome"),
        complete: super::sources::registry_meta_cache_complete("voltagent_awesome"),
    }
}

fn github_source_ids_for_provider(filter: &str) -> Vec<&'static str> {
    HUB_CATALOG
        .iter()
        .filter(|e| {
            matches!(e.kind, CatalogKind::GitHubRepo { .. })
                && e.provider_keys
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(filter))
        })
        .map(|e| e.id)
        .collect()
}

fn page_github_provider(filter: &str, offset: usize, limit: usize) -> CatalogPage {
    let ids = github_source_ids_for_provider(filter);
    if ids.is_empty() {
        return CatalogPage::default();
    }
    page_github_source_ids(&ids, offset, limit)
}

fn page_github_source_ids(ids: &[&str], offset: usize, limit: usize) -> CatalogPage {
    let mut all: Vec<(HubSourceInfo, SkillMeta)> = Vec::new();
    let mut any_cache = false;
    let mut all_complete = !ids.is_empty();
    for id in ids {
        let Some(source) = HUB_CATALOG.iter().find(|e| e.id.eq_ignore_ascii_case(id)) else {
            all_complete = false;
            continue;
        };
        if let Some((metas, _)) = super::github_cache_entries_slice(source.id, 0, usize::MAX) {
            any_cache = true;
            all_complete = all_complete && super::github_cache_complete(source.id);
            let info = HubSourceInfo {
                id: source.id.into(),
                label: source.label.into(),
                origin: source.origin.into(),
                trust_level: source.trust_level.into(),
            };
            for meta in metas {
                all.push((info.clone(), meta));
            }
        } else {
            all_complete = false;
        }
    }
    // Dedup across multi-root providers (e.g. openai curated + system) by identifier.
    all.sort_by(|a, b| a.1.identifier.cmp(&b.1.identifier));
    all.dedup_by(|a, b| a.1.identifier == b.1.identifier);
    let unique_total = all.len();
    let page: Vec<(HubSourceInfo, SkillMeta)> =
        all.into_iter().skip(offset).take(limit.max(1)).collect();
    let mut groups: Vec<SearchGroup> = Vec::new();
    for (info, meta) in page {
        if let Some(g) = groups.iter_mut().find(|g| g.source.id == info.id) {
            g.results.push(meta);
        } else {
            groups.push(SearchGroup {
                source: info,
                results: vec![meta],
                notice: None,
            });
        }
    }
    CatalogPage {
        groups,
        total: any_cache.then_some(unique_total),
        complete: all_complete && unique_total > 0,
    }
}

fn github_provider_total(filter: &str) -> Option<usize> {
    let page = page_github_provider(filter, 0, usize::MAX);
    page.total
}

fn github_provider_complete(filter: &str) -> bool {
    let ids = github_source_ids_for_provider(filter);
    !ids.is_empty() && ids.iter().all(|id| super::github_cache_complete(id))
}

fn github_source_total(id: &str) -> Option<usize> {
    super::github_cache_len(id)
}

fn github_source_complete(id: &str) -> bool {
    super::github_cache_complete(id)
}

async fn ensure_github_provider(filter: &str) -> Result<(), String> {
    let ids = github_source_ids_for_provider(filter);
    if ids.is_empty() {
        return Ok(());
    }
    let client = hub_client_catalog()?;
    for id in ids {
        let Some(source) = HUB_CATALOG.iter().find(|e| e.id.eq_ignore_ascii_case(id)) else {
            continue;
        };
        if super::github_cache_complete(source.id) {
            continue;
        }
        match super::refresh_github_cache(&client, source).await {
            Ok(cache) => super::write_source_cache(source.id, &cache),
            Err(e) => {
                tracing::warn!(source = source.id, error = %e, "github catalog ensure failed");
            }
        }
    }
    Ok(())
}

async fn ensure_github_source_id(id: &str) -> Result<(), String> {
    let Some(source) = HUB_CATALOG.iter().find(|e| e.id.eq_ignore_ascii_case(id)) else {
        return Ok(());
    };
    if super::github_cache_complete(source.id) {
        return Ok(());
    }
    let client = hub_client_catalog()?;
    let cache = super::refresh_github_cache(&client, source).await?;
    super::write_source_cache(source.id, &cache);
    Ok(())
}

async fn ensure_voltagent_catalog() -> Result<(), String> {
    if super::sources::registry_meta_cache_complete("voltagent_awesome") {
        return Ok(());
    }
    let client = hub_client_catalog()?;
    let Some(source) = HUB_CATALOG.iter().find(|e| e.id == "voltagent") else {
        return Ok(());
    };
    let _ = super::search_source(&client, source, "", MARKETPLACE_BROWSE_FETCH_MAX).await;
    if super::sources::registry_meta_cache_len("voltagent_awesome").is_some_and(|n| n > 0) {
        super::sources::mark_registry_meta_cache_complete("voltagent_awesome");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome as TestHome;

    #[test]
    fn catalog_page_github_slices_second_page() {
        let _home = TestHome::new();
        let mut entries = Vec::new();
        for i in 0..200 {
            entries.push(super::super::CachedSkillEntry {
                name: format!("skill-{i:03}"),
                relative_path: format!("skills/skill-{i:03}"),
                identifier: format!("openai/skills/skill-{i:03}"),
                description: format!("d{i}"),
                tags: vec![],
            });
        }
        super::super::write_source_cache(
            "openai",
            &super::super::SourceCache {
                fetched_at: chrono::Utc::now().timestamp(),
                entries,
            },
        );
        let page = catalog_page("openai", 80, 80);
        assert_eq!(page.total, Some(200));
        assert_eq!(page.row_count(), 80);
        let slice =
            super::super::browse_github_cache_slice("openai", 80, 80).expect("direct source slice");
        assert_eq!(slice.results.len(), 80);
        assert!(super::super::github_cache_complete("openai"));
        assert!(
            page.groups.iter().flat_map(|g| g.results.iter()).any(|m| m
                .identifier
                .contains("skill-080")
                || m.identifier.contains("skill-081")
                || m.identifier.contains("skill-100")),
            "page 2 should include mid-range skills"
        );
    }

    #[test]
    fn catalog_page_dedups_multi_root_provider() {
        let _home = TestHome::new();
        let shared = |i: usize| super::super::CachedSkillEntry {
            name: format!("shared-{i}"),
            relative_path: format!("shared-{i}"),
            identifier: format!("dup-{i}"),
            description: String::new(),
            tags: vec![],
        };
        super::super::write_source_cache(
            "openai",
            &super::super::SourceCache {
                fetched_at: chrono::Utc::now().timestamp(),
                entries: (0..50)
                    .map(shared)
                    .chain((50..100).map(|i| super::super::CachedSkillEntry {
                        name: format!("a-{i}"),
                        relative_path: format!("a-{i}"),
                        identifier: format!("openai-only-{i}"),
                        description: String::new(),
                        tags: vec![],
                    }))
                    .collect(),
            },
        );
        super::super::write_source_cache(
            "openai-system",
            &super::super::SourceCache {
                fetched_at: chrono::Utc::now().timestamp(),
                entries: (0..50)
                    .map(shared)
                    .chain((100..150).map(|i| super::super::CachedSkillEntry {
                        name: format!("b-{i}"),
                        relative_path: format!("b-{i}"),
                        identifier: format!("system-only-{i}"),
                        description: String::new(),
                        tags: vec![],
                    }))
                    .collect(),
            },
        );
        let total = catalog_total("openai").expect("total");
        // 50 shared + 50 openai-only + 50 system-only = 150 unique
        assert_eq!(total, 150);
        let page = catalog_page("openai", 0, 200);
        assert_eq!(page.row_count(), 150);
    }
}
