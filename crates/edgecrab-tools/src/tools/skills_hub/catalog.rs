//! Single source of truth for curated GitHub sources, default taps, and provider filters.
//!
//! DRY: DEFAULT_TAPS, CURATED_SOURCES, provider_filter_repos, and TUI filter cycle
//! all derive from [`HUB_CATALOG`].

/// Shared registry/GitHub cache TTL (15 minutes). Re-exported for façade cache checks.
pub const REGISTRY_CACHE_TTL_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy)]
pub struct TapSeed {
    pub name: &'static str,
    pub repo: &'static str,
    pub root: &'static str,
    pub trust_level: &'static str,
}

impl TapSeed {
    pub fn github_url(&self) -> String {
        if self.root.is_empty() {
            format!("https://github.com/{}", self.repo)
        } else {
            format!("https://github.com/{}/{}", self.repo, self.root)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CatalogKind {
    GitHubRepo {
        repo: &'static str,
        root: &'static str,
    },
    SkillsSh,
    /// Provider-filter only (no live curated tree search).
    ProviderOnly {
        repos: &'static [&'static str],
    },
}

#[derive(Debug, Clone, Copy)]
pub struct HubCatalogEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub origin: &'static str,
    pub trust_level: &'static str,
    pub kind: CatalogKind,
    /// When set, seed into `taps.json` on hub init.
    pub tap_name: Option<&'static str>,
    /// Hermes GITHUB_TAP_PROVIDERS-style filter keys (e.g. `openai`, `hf`).
    pub provider_keys: &'static [&'static str],
    /// Include in parallel curated GitHub / skills.sh search.
    pub curated_search: bool,
}

/// Canonical hub catalog — taps, curated search, and provider filters derive from this.
pub const HUB_CATALOG: &[HubCatalogEntry] = &[
    HubCatalogEntry {
        id: "edgecrab",
        label: "EdgeCrab",
        origin: "https://github.com/raphaelmansuy/edgecrab",
        trust_level: "trusted",
        kind: CatalogKind::GitHubRepo {
            repo: "raphaelmansuy/edgecrab",
            root: "skills",
        },
        tap_name: None,
        provider_keys: &[],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "hermes-agent",
        label: "Hermes Agent",
        origin: "https://hermes-agent.nousresearch.com/",
        trust_level: "trusted",
        kind: CatalogKind::GitHubRepo {
            repo: "NousResearch/hermes-agent",
            root: "skills",
        },
        tap_name: None,
        provider_keys: &[],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "openai",
        label: "OpenAI Skills (Codex curated)",
        origin: "https://github.com/openai/skills",
        trust_level: "trusted",
        kind: CatalogKind::GitHubRepo {
            repo: "openai/skills",
            root: "skills/.curated",
        },
        tap_name: Some("openai-skills-curated"),
        provider_keys: &["openai"],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "openai-system",
        label: "OpenAI Skills (Codex system)",
        origin: "https://github.com/openai/skills",
        trust_level: "trusted",
        kind: CatalogKind::GitHubRepo {
            repo: "openai/skills",
            root: "skills/.system",
        },
        tap_name: Some("openai-skills-system"),
        provider_keys: &["openai"],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "anthropics",
        label: "Anthropic Skills",
        origin: "https://github.com/anthropics/skills",
        trust_level: "trusted",
        kind: CatalogKind::GitHubRepo {
            repo: "anthropics/skills",
            root: "skills",
        },
        tap_name: Some("anthropics-skills"),
        provider_keys: &["anthropic", "anthropics"],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "huggingface",
        label: "Hugging Face Skills",
        origin: "https://github.com/huggingface/skills",
        trust_level: "trusted",
        kind: CatalogKind::GitHubRepo {
            repo: "huggingface/skills",
            root: "skills",
        },
        tap_name: Some("huggingface-skills"),
        provider_keys: &["huggingface", "hf"],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "nvidia",
        label: "NVIDIA Skills",
        origin: "https://github.com/NVIDIA/skills",
        trust_level: "trusted",
        kind: CatalogKind::GitHubRepo {
            repo: "NVIDIA/skills",
            root: "skills",
        },
        tap_name: Some("nvidia-skills"),
        provider_keys: &["nvidia"],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "skills.sh",
        label: "skills.sh",
        origin: "https://skills.sh",
        trust_level: "community",
        kind: CatalogKind::SkillsSh,
        tap_name: None,
        provider_keys: &[],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "gstack",
        label: "gstack",
        origin: "https://github.com/garrytan/gstack",
        trust_level: "community",
        kind: CatalogKind::GitHubRepo {
            repo: "garrytan/gstack",
            root: "",
        },
        tap_name: Some("gstack"),
        provider_keys: &["gstack"],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "voltagent",
        label: "VoltAgent Awesome Skills",
        origin: "https://github.com/voltagent/awesome-agent-skills",
        trust_level: "community",
        // Awesome-list: browse harvests README GitHub links (no SKILL.md tree).
        kind: CatalogKind::GitHubRepo {
            repo: "voltagent/awesome-agent-skills",
            root: "",
        },
        tap_name: Some("voltagent-awesome"),
        provider_keys: &["voltagent"],
        curated_search: true,
    },
    HubCatalogEntry {
        id: "minimax",
        label: "MiniMax CLI",
        origin: "https://github.com/minimax-ai/cli",
        trust_level: "community",
        kind: CatalogKind::GitHubRepo {
            repo: "minimax-ai/cli",
            root: "skill",
        },
        tap_name: Some("minimax-cli"),
        provider_keys: &["minimax"],
        curated_search: true,
    },
];

/// Curated sources used for live parallel search (GitHub trees + skills.sh).
pub fn curated_search_entries() -> impl Iterator<Item = &'static HubCatalogEntry> {
    HUB_CATALOG.iter().filter(|e| e.curated_search)
}

/// Default tap seeds derived from catalog entries with `tap_name`.
pub fn default_tap_seeds() -> Vec<TapSeed> {
    HUB_CATALOG
        .iter()
        .filter_map(|e| {
            let name = e.tap_name?;
            let (repo, root) = match e.kind {
                CatalogKind::GitHubRepo { repo, root } => (repo, root),
                _ => return None,
            };
            Some(TapSeed {
                name,
                repo,
                root,
                trust_level: e.trust_level,
            })
        })
        .collect()
}

/// Repos matched by a provider filter key (e.g. `openai` → `openai/skills`).
pub fn provider_filter_repos(filter: &str) -> Option<Vec<&'static str>> {
    let key = filter.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    let mut repos: Vec<&'static str> = Vec::new();
    for entry in HUB_CATALOG {
        if !entry
            .provider_keys
            .iter()
            .any(|k| k.eq_ignore_ascii_case(&key))
        {
            continue;
        }
        match entry.kind {
            CatalogKind::GitHubRepo { repo, .. } => {
                if !repos.iter().any(|r| r.eq_ignore_ascii_case(repo)) {
                    repos.push(repo);
                }
                // nvidia filter historically accepts lowercase path form too
                if key == "nvidia" && !repos.contains(&"nvidia/skills") {
                    repos.push("nvidia/skills");
                }
            }
            CatalogKind::ProviderOnly { repos: r } => {
                for repo in r {
                    if !repos.iter().any(|x| x.eq_ignore_ascii_case(repo)) {
                        repos.push(repo);
                    }
                }
            }
            CatalogKind::SkillsSh => {}
        }
    }
    if repos.is_empty() { None } else { Some(repos) }
}

pub fn is_provider_filter(filter: &str) -> bool {
    provider_filter_repos(filter).is_some()
}

/// Primary provider keys for TUI cycle (stable order, de-duplicated).
pub fn marketplace_provider_filter_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = vec!["all"];
    // Prefer the first (canonical) key per entry that contributes a provider filter.
    for entry in HUB_CATALOG {
        if let Some(primary) = entry.provider_keys.first()
            && !keys.iter().any(|k| k.eq_ignore_ascii_case(primary))
        {
            keys.push(primary);
        }
    }
    // Registry-only filters (not GitHub provider taps)
    for extra in ["clawhub", "skills-sh"] {
        if !keys.iter().any(|k| k.eq_ignore_ascii_case(extra)) {
            keys.push(extra);
        }
    }
    keys
}

/// Static slice for TUI/CLI that need `&[&str]` — rebuilt once via OnceLock.
pub fn marketplace_provider_filters() -> &'static [&'static str] {
    use std::sync::OnceLock;
    static FILTERS: OnceLock<Vec<&'static str>> = OnceLock::new();
    FILTERS
        .get_or_init(marketplace_provider_filter_keys)
        .as_slice()
}

/// Marketplace source class — drives uniform browse/search limit policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketplaceSourceClass {
    /// `all` / unified catalog fan-out.
    All,
    /// GitHub curated trees (openai, anthropic, nvidia, …).
    GitHubProvider,
    /// External registries with large catalogs (skills.sh, ClawHub).
    Registry,
}

/// Classify a marketplace filter key for browse/search policy.
pub fn marketplace_source_class(filter: &str) -> MarketplaceSourceClass {
    let f = filter.trim().to_ascii_lowercase();
    match f.as_str() {
        "all" | "" => MarketplaceSourceClass::All,
        "skills-sh" | "skills.sh" | "skillssh" | "skills_sh" | "clawhub" => {
            MarketplaceSourceClass::Registry
        }
        _ => MarketplaceSourceClass::GitHubProvider,
    }
}

/// Memory-safety ceiling for empty-query browse fetch (sitemap-scale).
/// Display uses TUI virtual scroll / CLI `--page` — not this as a UX row clamp.
pub const MARKETPLACE_BROWSE_FETCH_MAX: usize = 10_000;
/// First-page / extend page size for on-demand TUI browse (viewport-scale).
pub const MARKETPLACE_BROWSE_PAGE_SIZE: usize = 80;
/// Typed-query search cap (ranked results, not full catalog).
pub const MARKETPLACE_SEARCH_MAX: usize = 50;

/// Resolve the effective per-source limit for marketplace browse or search.
///
/// Empty query = browse: honor `requested` up to [`MARKETPLACE_BROWSE_FETCH_MAX`]
/// (no GitHub-100 / registry-200 class clamps). Typed query = search, capped at
/// [`MARKETPLACE_SEARCH_MAX`]. `filter` is reserved for source-class metadata.
pub fn marketplace_result_limit(filter: &str, requested: usize, browse: bool) -> usize {
    let _ = marketplace_source_class(filter); // keep class SoT warm for callers/tests
    if !browse {
        return requested.clamp(1, MARKETPLACE_SEARCH_MAX);
    }
    requested.clamp(1, MARKETPLACE_BROWSE_FETCH_MAX)
}

/// Index bootstrap cache tuples: (source_id, repo, trust).
pub fn index_bootstrap_repos() -> Vec<(&'static str, &'static str, &'static str)> {
    HUB_CATALOG
        .iter()
        .filter(|e| e.curated_search)
        .filter_map(|e| match e.kind {
            CatalogKind::GitHubRepo { repo, .. } => Some((e.id, repo, e.trust_level)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_filter_includes_openai_skills() {
        let repos = provider_filter_repos("openai").expect("openai");
        assert!(
            repos
                .iter()
                .any(|r| r.eq_ignore_ascii_case("openai/skills"))
        );
    }

    #[test]
    fn clawhub_is_marketplace_filter_not_provider_tap() {
        assert!(!is_provider_filter("clawhub"));
        assert!(marketplace_provider_filters().contains(&"clawhub"));
        assert!(marketplace_provider_filters().contains(&"skills-sh"));
    }

    #[test]
    fn default_taps_include_hf_nvidia_gstack() {
        let seeds = default_tap_seeds();
        assert!(seeds.iter().any(|s| s.repo == "huggingface/skills"));
        assert!(seeds.iter().any(|s| s.repo == "NVIDIA/skills"));
        assert!(seeds.iter().any(|s| s.name == "gstack"));
    }

    #[test]
    fn curated_search_includes_gstack_and_openai() {
        assert!(curated_search_entries().any(|e| e.id == "gstack"));
        assert!(curated_search_entries().any(|e| e.id == "openai"));
        assert!(curated_search_entries().any(|e| e.id == "minimax"));
    }

    #[test]
    fn marketplace_result_limit_browse_has_no_github_100_clamp() {
        for filter in marketplace_provider_filters() {
            let browse = marketplace_result_limit(filter, 500, true);
            let search = marketplace_result_limit(filter, 100, false);
            assert!(
                (1..=MARKETPLACE_SEARCH_MAX).contains(&search),
                "filter `{filter}` search limit {search}"
            );
            assert_eq!(
                browse, 500,
                "filter `{filter}` browse must honor requested>100 (got {browse})"
            );
            let capped = marketplace_result_limit(filter, 50_000, true);
            assert_eq!(capped, MARKETPLACE_BROWSE_FETCH_MAX);
        }
        assert_eq!(
            marketplace_source_class("skills-sh"),
            MarketplaceSourceClass::Registry
        );
        assert_eq!(
            marketplace_source_class("clawhub"),
            MarketplaceSourceClass::Registry
        );
        assert_eq!(
            marketplace_result_limit("voltagent", 847, true),
            847,
            "VoltAgent browse must not clamp at 100"
        );
        assert_eq!(
            marketplace_source_class("openai"),
            MarketplaceSourceClass::GitHubProvider
        );
    }
}
