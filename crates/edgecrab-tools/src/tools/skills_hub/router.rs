//! SkillSourceRouter — SOLID dispatcher for registry search/fetch (019 WR).
//!
//! Thin adapters wrap existing `sources.rs` / `mod.rs` functions so new
//! registries implement [`SkillSource`] instead of forking façade code.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};

use super::normalize::normalize_identifier;
use super::source_trait::SkillSource;
use super::{
    HubSourceInfo, SearchGroup, SkillBundle, SkillMeta, hub_client, is_provider_filter, sources,
};

type GroupFuture = Pin<Box<dyn Future<Output = SearchGroup> + Send>>;

/// Classify a normalized identifier into a `source_id`.
pub fn classify_source_id(normalized: &str) -> &'static str {
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with("npm:") {
        return "npm";
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        if lower.contains("/.well-known/skills") || lower.contains("well-known:") {
            return "well-known";
        }
        return "url";
    }
    if lower.starts_with("well-known:") {
        return "well-known";
    }
    if lower.starts_with("clawhub:") || lower.starts_with('@') {
        return "clawhub";
    }
    if lower.starts_with("skills.sh:") || lower.starts_with("skills-sh:") {
        return "skills-sh";
    }
    if lower.starts_with("browse-sh:") || lower.starts_with("browse.sh:") {
        return "browse-sh";
    }
    if lower.starts_with("claude-marketplace:") || lower.starts_with("claude:") {
        return "claude-marketplace";
    }
    if lower.starts_with("lobehub:") {
        return "lobehub";
    }
    if lower.starts_with("agentskills:") || lower.starts_with("agentskills.io:") {
        return "agentskills.io";
    }
    if lower.starts_with("official/") {
        return "official";
    }
    if Path::new(normalized).exists() {
        return "local";
    }
    if lower.contains('/') && !lower.contains(' ') {
        return "github";
    }
    "hermes-index"
}

/// Global router with one adapter per catalogued `source_id`.
pub struct SkillSourceRouter {
    sources: Vec<Arc<dyn SkillSource>>,
}

impl SkillSourceRouter {
    pub fn new() -> Self {
        Self {
            sources: default_adapters(),
        }
    }

    pub fn global() -> &'static SkillSourceRouter {
        use std::sync::OnceLock;
        static ROUTER: OnceLock<SkillSourceRouter> = OnceLock::new();
        ROUTER.get_or_init(SkillSourceRouter::new)
    }

    pub fn source_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.sources.iter().map(|s| s.source_id())
    }

    pub fn get(&self, source_id: &str) -> Option<&dyn SkillSource> {
        self.sources
            .iter()
            .find(|s| s.source_id().eq_ignore_ascii_case(source_id))
            .map(|s| s.as_ref())
    }

    pub fn classify(&self, identifier: &str) -> &'static str {
        classify_source_id(&normalize_identifier(identifier))
    }

    /// Fetch via the adapter matching the identifier's source_id.
    ///
    /// `optional_dir` is applied for official/local resolution (trait fetch has no cwd).
    pub async fn fetch(
        &self,
        identifier: &str,
        optional_dir: Option<&Path>,
    ) -> Result<SkillBundle, String> {
        let normalized = normalize_identifier(identifier);
        let source_id = classify_source_id(&normalized);
        match source_id {
            "local" => {
                return super::local_bundle::build_local_skill_bundle(Path::new(&normalized), None);
            }
            "official" => {
                return super::load_official_skill_bundle(&normalized, optional_dir);
            }
            "hermes-index" => {
                if let Some(bundle) = super::index::try_fetch_from_index(&normalized).await {
                    return Ok(bundle);
                }
                return super::fetch_github_resolved(&normalized, optional_dir).await;
            }
            "github" => {
                return super::fetch_github_resolved(&normalized, optional_dir).await;
            }
            _ => {}
        }
        if let Some(src) = self.get(source_id) {
            return src.fetch(&normalized).await;
        }
        Err(format!(
            "Skill source '{identifier}' not found (unknown source_id '{source_id}')"
        ))
    }

    pub async fn search(&self, source_id: &str, query: &str, limit: usize) -> Vec<SkillMeta> {
        match self.get(source_id) {
            Some(src) => src.search(query, limit).await,
            None => Vec::new(),
        }
    }

    /// Parallel live search for `search_hub` — curated + registry adapters + taps.
    ///
    /// Does not include unified-index (façade owns index bootstrap / short-circuit).
    pub async fn search_groups(
        &self,
        query: &str,
        filter: &str,
        limit: usize,
        configured_hub_url: Option<&str>,
    ) -> Vec<SearchGroup> {
        self.search_groups_progressive(query, filter, limit, configured_hub_url, &mut |_| {})
            .await
    }

    /// Progressive live search — invokes `on_partial` as each source group completes.
    ///
    /// True cross-source fan-out: curated + registry + taps share one
    /// `FuturesUnordered` pool (no phase barrier awaiting all curated before
    /// registries/taps start).
    pub async fn search_groups_progressive(
        &self,
        query: &str,
        filter: &str,
        limit: usize,
        configured_hub_url: Option<&str>,
        on_partial: &mut (dyn FnMut(SearchGroup) + Send),
    ) -> Vec<SearchGroup> {
        let mut groups = Vec::new();

        let client = match hub_client() {
            Ok(c) => c,
            Err(error) => {
                let g = SearchGroup {
                    source: HubSourceInfo {
                        id: "hub".into(),
                        label: "Skills Hub".into(),
                        origin: "local".into(),
                        trust_level: "n/a".into(),
                    },
                    results: Vec::new(),
                    notice: Some(error),
                };
                on_partial(g.clone());
                return vec![g];
            }
        };

        let mut pending: FuturesUnordered<GroupFuture> = FuturesUnordered::new();

        // Curated GitHub trees + skills.sh.
        if filter == "all"
            || filter == "github"
            || filter == "curated"
            || filter == "skills.sh"
            || filter == "skills-sh"
            || filter == "registry"
            || is_provider_filter(filter)
            || curated_search_entries().any(|s| source_id_matches_filter(s.id, filter))
        {
            for source in curated_search_entries().filter(|s| super::source_matches_filter(s, filter))
            {
                let client = client.clone();
                let q = query.to_string();
                let source = *source;
                pending.push(Box::pin(async move {
                    super::search_source(&client, &source, &q, limit).await
                }));
            }
        }

        // Registry adapters.
        if filter == "all" || sources::registry_filter_includes_any(filter) {
            let registry_filter = if filter == "all" || filter == "registry" {
                "all"
            } else {
                filter
            };
            let reg_limit = limit.clamp(1, 200);
            for source in sources::REGISTRY_SOURCES
                .iter()
                .filter(|source| sources::registry_source_included(source, registry_filter))
            {
                let client = client.clone();
                let q = query.to_string();
                let source = *source;
                pending.push(Box::pin(async move {
                    sources::search_one_registry(&client, &source, &q, reg_limit).await
                }));
            }
        }

        // Custom taps — one future each (stream as they complete).
        if filter == "all" || filter == "tap" || filter == "taps" || is_provider_filter(filter) {
            for tap in super::read_taps()
                .into_iter()
                .filter(|tap| !super::tap_mirrors_curated_catalog(tap))
            {
                let client = client.clone();
                let q = query.to_string();
                pending.push(Box::pin(async move {
                    super::search_custom_tap(&client, &tap, &q, limit).await
                }));
            }
        }

        while let Some(group) = pending.next().await {
            let is_registry = sources::REGISTRY_SOURCES
                .iter()
                .any(|s| s.id.eq_ignore_ascii_case(&group.source.id));
            if is_registry {
                if filter != "all"
                    && filter != "registry"
                    && !registry_id_matches_filter(&group.source.id, filter)
                {
                    continue;
                }
                if group.results.is_empty() && group.notice.is_none() {
                    continue;
                }
            }
            on_partial(group.clone());
            groups.push(group);
        }

        // Well-known URL queries stay sequential (rare; after fan-out).
        if (filter == "all" || filter == "well-known")
            && (query.starts_with("https://") || query.starts_with("http://"))
        {
            let g = super::search_well_known_source(&client, query, limit).await;
            on_partial(g.clone());
            groups.push(g);
        }

        if let Some(url) = configured_hub_url.map(str::trim).filter(|u| !u.is_empty())
            && (filter == "all" || filter == "well-known" || filter == "hub")
        {
            let g = super::search_well_known_source(&client, url, limit).await;
            on_partial(g.clone());
            groups.push(g);
        }

        groups
    }
}

fn curated_search_entries() -> impl Iterator<Item = &'static super::HubCatalogEntry> {
    super::catalog::curated_search_entries()
}

fn source_id_matches_filter(source_id: &str, filter: &str) -> bool {
    filter.eq_ignore_ascii_case(source_id)
}

fn registry_id_matches_filter(source_id: &str, filter: &str) -> bool {
    let f = filter.trim().to_ascii_lowercase();
    let id = source_id.to_ascii_lowercase();
    f == id
        || (f == "skills-sh" && id == "skills-sh")
        || (f == "skills.sh" && (id == "skills-sh" || id == "skills.sh"))
        || (f == "agentskills" && id == "agentskills.io")
        || (f == "registry")
}

impl Default for SkillSourceRouter {
    fn default() -> Self {
        Self::new()
    }
}

fn default_adapters() -> Vec<Arc<dyn SkillSource>> {
    vec![
        Arc::new(OfficialSource),
        Arc::new(HermesIndexSource),
        Arc::new(SkillsShSource),
        Arc::new(WellKnownSource),
        Arc::new(UrlSource),
        Arc::new(GitHubSource),
        Arc::new(ClawHubSource),
        Arc::new(ClaudeMarketplaceSource),
        Arc::new(LobeHubSource),
        Arc::new(BrowseShSource),
        Arc::new(AgentskillsSource),
        Arc::new(NpmSource),
        Arc::new(LocalSource),
    ]
}

struct OfficialSource;
struct HermesIndexSource;
struct SkillsShSource;
struct WellKnownSource;
struct UrlSource;
struct GitHubSource;
struct ClawHubSource;
struct ClaudeMarketplaceSource;
struct LobeHubSource;
struct BrowseShSource;
struct AgentskillsSource;
struct NpmSource;
struct LocalSource;

#[async_trait]
impl SkillSource for OfficialSource {
    fn source_id(&self) -> &'static str {
        "official"
    }
    async fn search(&self, _query: &str, _limit: usize) -> Vec<SkillMeta> {
        Vec::new()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        super::load_official_skill_bundle(identifier, None)
    }
    fn trust_level_for(&self, _: &str) -> &'static str {
        "official"
    }
}

#[async_trait]
impl SkillSource for HermesIndexSource {
    fn source_id(&self) -> &'static str {
        "hermes-index"
    }
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta> {
        let group = super::index::search_unified_index(query, limit);
        group.results
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        if let Some(bundle) = super::index::try_fetch_from_index(identifier).await {
            return Ok(bundle);
        }
        super::fetch_github_resolved(identifier, None).await
    }
}

#[async_trait]
impl SkillSource for SkillsShSource {
    fn source_id(&self) -> &'static str {
        "skills-sh"
    }
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta> {
        let groups = sources::search_registry_sources(query, "skills-sh", limit).await;
        groups
            .into_iter()
            .flat_map(|g| g.results)
            .take(limit)
            .collect()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        sources::fetch_registry_bundle(identifier).await
    }
}

#[async_trait]
impl SkillSource for WellKnownSource {
    fn source_id(&self) -> &'static str {
        "well-known"
    }
    async fn search(&self, _query: &str, _limit: usize) -> Vec<SkillMeta> {
        Vec::new()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        sources::fetch_registry_bundle(identifier).await
    }
}

#[async_trait]
impl SkillSource for UrlSource {
    fn source_id(&self) -> &'static str {
        "url"
    }
    async fn search(&self, _query: &str, _limit: usize) -> Vec<SkillMeta> {
        Vec::new()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        let client = super::hub_client()?;
        sources::fetch_url_skill_bundle(&client, identifier).await
    }
}

#[async_trait]
impl SkillSource for GitHubSource {
    fn source_id(&self) -> &'static str {
        "github"
    }
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta> {
        let groups = super::search_curated_groups(query, "github", limit).await;
        groups
            .into_iter()
            .flat_map(|g| g.results)
            .take(limit)
            .collect()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        super::fetch_github_resolved(identifier, None).await
    }
}

#[async_trait]
impl SkillSource for ClawHubSource {
    fn source_id(&self) -> &'static str {
        "clawhub"
    }
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta> {
        let groups = sources::search_registry_sources(query, "clawhub", limit).await;
        groups
            .into_iter()
            .flat_map(|g| g.results)
            .take(limit)
            .collect()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        sources::fetch_registry_bundle(identifier).await
    }
}

#[async_trait]
impl SkillSource for ClaudeMarketplaceSource {
    fn source_id(&self) -> &'static str {
        "claude-marketplace"
    }
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta> {
        let groups = sources::search_registry_sources(query, "claude-marketplace", limit).await;
        groups
            .into_iter()
            .flat_map(|g| g.results)
            .take(limit)
            .collect()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        sources::fetch_registry_bundle(identifier).await
    }
}

#[async_trait]
impl SkillSource for LobeHubSource {
    fn source_id(&self) -> &'static str {
        "lobehub"
    }
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta> {
        let groups = sources::search_registry_sources(query, "lobehub", limit).await;
        groups
            .into_iter()
            .flat_map(|g| g.results)
            .take(limit)
            .collect()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        sources::fetch_registry_bundle(identifier).await
    }
}

#[async_trait]
impl SkillSource for BrowseShSource {
    fn source_id(&self) -> &'static str {
        "browse-sh"
    }
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta> {
        let groups = sources::search_registry_sources(query, "browse-sh", limit).await;
        groups
            .into_iter()
            .flat_map(|g| g.results)
            .take(limit)
            .collect()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        sources::fetch_registry_bundle(identifier).await
    }
}

#[async_trait]
impl SkillSource for AgentskillsSource {
    fn source_id(&self) -> &'static str {
        "agentskills.io"
    }
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta> {
        let groups = sources::search_registry_sources(query, "agentskills", limit).await;
        groups
            .into_iter()
            .flat_map(|g| g.results)
            .take(limit)
            .collect()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        sources::fetch_registry_bundle(identifier).await
    }
}

#[async_trait]
impl SkillSource for NpmSource {
    fn source_id(&self) -> &'static str {
        "npm"
    }
    async fn search(&self, _query: &str, _limit: usize) -> Vec<SkillMeta> {
        Vec::new()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        let bundles = super::npm_pack::fetch_npm_skill_bundles(identifier)?;
        bundles
            .into_iter()
            .next()
            .ok_or_else(|| format!("npm package '{identifier}' produced no skill bundles"))
    }
}

#[async_trait]
impl SkillSource for LocalSource {
    fn source_id(&self) -> &'static str {
        "local"
    }
    async fn search(&self, _query: &str, _limit: usize) -> Vec<SkillMeta> {
        Vec::new()
    }
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String> {
        super::local_bundle::build_local_skill_bundle(Path::new(identifier), None)
    }
    fn trust_level_for(&self, _: &str) -> &'static str {
        "community"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::source_trait::ALL_SOURCE_IDS;
    use std::time::Duration;

    #[test]
    fn classify_peer_and_registry_ids() {
        assert_eq!(classify_source_id("npm:foo"), "npm");
        assert_eq!(classify_source_id("clawhub:bar"), "clawhub");
        assert_eq!(classify_source_id("skills.sh:a/b/c"), "skills-sh");
        assert_eq!(classify_source_id("well-known:https://x/y"), "well-known");
        assert_eq!(classify_source_id("official/cat/name"), "official");
        assert_eq!(classify_source_id("owner/repo/path"), "github");
    }

    #[test]
    fn router_registers_all_catalog_ids() {
        let router = SkillSourceRouter::new();
        let ids: Vec<_> = router.source_ids().collect();
        for id in ALL_SOURCE_IDS {
            assert!(
                ids.iter().any(|x| x == id),
                "router missing adapter for {id}"
            );
        }
    }

    /// Cross-source pool ordering: a fast sibling completes before a slow one
    /// (the barrier that made registries wait on curated).
    #[tokio::test]
    async fn cross_source_pool_emits_fast_before_slow() {
        let mut pending: FuturesUnordered<GroupFuture> = FuturesUnordered::new();
        pending.push(Box::pin(async {
            tokio::time::sleep(Duration::from_millis(80)).await;
            SearchGroup {
                source: HubSourceInfo {
                    id: "slow-curated".into(),
                    label: "Slow".into(),
                    origin: "test".into(),
                    trust_level: "n/a".into(),
                },
                results: Vec::new(),
                notice: Some("slow".into()),
            }
        }));
        pending.push(Box::pin(async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            SearchGroup {
                source: HubSourceInfo {
                    id: "fast-registry".into(),
                    label: "Fast".into(),
                    origin: "test".into(),
                    trust_level: "n/a".into(),
                },
                results: Vec::new(),
                notice: Some("fast".into()),
            }
        }));
        let first = pending.next().await.expect("first completion");
        assert_eq!(
            first.source.id, "fast-registry",
            "registry-shaped sibling must be able to finish before slow curated"
        );
    }
}
