use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use edgecrab_core::AppConfig;
use edgecrab_core::config::McpServerConfig;
use serde::{Deserialize, Serialize};

pub struct McpPreset {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub package_name: &'static str,
    pub source_url: &'static str,
    pub homepage: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub tags: &'static [&'static str],
    pub required_env: &'static [&'static str],
    pub notes: &'static str,
}

pub struct InstalledPreset {
    pub name: String,
    pub missing_env: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficialCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub source_url: String,
    pub homepage: String,
    pub tags: Vec<String>,
    pub installable_preset_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedOfficialCatalog {
    fetched_at_epoch_secs: u64,
    entries: Vec<OfficialCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpSearchSourceInfo {
    pub id: String,
    pub label: String,
    pub origin: String,
    pub trust_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpInstallPlan {
    Preset {
        preset_id: String,
    },
    Http {
        url: String,
        transport: String,
        required_headers: Vec<String>,
    },
    Stdio {
        command: String,
        args: Vec<String>,
        required_env: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpSearchEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub origin: String,
    pub homepage: Option<String>,
    pub tags: Vec<String>,
    pub transport: Option<String>,
    pub install: Option<McpInstallPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpSearchGroup {
    pub source: McpSearchSourceInfo,
    #[serde(default)]
    pub results: Vec<McpSearchEntry>,
    #[serde(default)]
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpSearchReport {
    #[serde(default)]
    pub groups: Vec<McpSearchGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledSearchEntry {
    pub name: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OfficialCatalogSection {
    None,
    Reference,
    Archived,
    Integration,
}

impl OfficialCatalogSection {
    fn tags(self) -> &'static [&'static str] {
        match self {
            Self::None => &[],
            Self::Reference => &["official", "reference", "upstream"],
            Self::Archived => &["official", "archived", "upstream"],
            Self::Integration => &["official", "integration"],
        }
    }
}

const OFFICIAL_MCP_README_URL: &str =
    "https://raw.githubusercontent.com/modelcontextprotocol/servers/main/README.md";
const OFFICIAL_CATALOG_MAX_AGE_SECS: u64 = 24 * 60 * 60;
const OFFICIAL_MCP_REPO_BASE_URL: &str =
    "https://github.com/modelcontextprotocol/servers/tree/main/";
const MCP_REGISTRY_BASE_URL: &str = "https://registry.modelcontextprotocol.io";

const PRESETS: &[McpPreset] = &[
    McpPreset {
        id: "everything",
        display_name: "Everything",
        description: "Official MCP reference server that exercises prompts, resources, tools, and protocol edge cases.",
        package_name: "@modelcontextprotocol/server-everything",
        source_url: "https://github.com/modelcontextprotocol/servers/tree/main/src/everything",
        homepage: "https://github.com/modelcontextprotocol/servers/blob/main/src/everything/README.md",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-everything"],
        tags: &["official", "reference", "test", "protocol"],
        required_env: &[],
        notes: "Best used as a compatibility and debugging server, not a production data source.",
    },
    McpPreset {
        id: "fetch",
        display_name: "Fetch",
        description: "Official MCP reference server for fetching web content and converting it to markdown.",
        package_name: "mcp-server-fetch",
        source_url: "https://github.com/modelcontextprotocol/servers/tree/main/src/fetch",
        homepage: "https://github.com/modelcontextprotocol/servers/blob/main/src/fetch/README.md",
        command: "uvx",
        args: &["mcp-server-fetch"],
        tags: &["official", "reference", "web", "fetch"],
        required_env: &[],
        notes: "Official upstream recommends uvx. This server can reach local/internal URLs, so apply it carefully.",
    },
    McpPreset {
        id: "filesystem",
        display_name: "Filesystem",
        description: "Official MCP filesystem server scoped to one allowed root path.",
        package_name: "@modelcontextprotocol/server-filesystem",
        source_url: "https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem",
        homepage: "https://modelcontextprotocol.io",
        command: "npx",
        args: &[
            "-y",
            "@modelcontextprotocol/server-filesystem",
            "{{workspace}}",
        ],
        tags: &["official", "reference", "files", "local"],
        required_env: &[],
        notes: "Install with --path to scope access to a specific directory. Defaults to the current working directory.",
    },
    McpPreset {
        id: "git",
        display_name: "Git",
        description: "Official MCP reference server for reading, diffing, and manipulating Git repositories.",
        package_name: "mcp-server-git",
        source_url: "https://github.com/modelcontextprotocol/servers/tree/main/src/git",
        homepage: "https://github.com/modelcontextprotocol/servers/blob/main/src/git/README.md",
        command: "uvx",
        args: &["mcp-server-git", "--repository", "{{workspace}}"],
        tags: &["official", "reference", "git", "repository"],
        required_env: &[],
        notes: "Official upstream recommends uvx. Defaults to the current working directory as the repository root.",
    },
    McpPreset {
        id: "github",
        display_name: "GitHub",
        description: "Official MCP GitHub server for issues, pull requests, and repository operations.",
        package_name: "@modelcontextprotocol/server-github",
        source_url: "https://github.com/github/github-mcp-server",
        homepage: "https://modelcontextprotocol.io",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-github"],
        tags: &["official", "integration", "github", "git"],
        required_env: &["GITHUB_PERSONAL_ACCESS_TOKEN", "GITHUB_TOKEN"],
        notes: "Official GitHub server. Prefer GITHUB_PERSONAL_ACCESS_TOKEN for upstream compatibility; GITHUB_TOKEN also works in many local setups.",
    },
    McpPreset {
        id: "memory",
        display_name: "Memory",
        description: "Official MCP memory server for lightweight persistent facts and notes.",
        package_name: "@modelcontextprotocol/server-memory",
        source_url: "https://github.com/modelcontextprotocol/servers/tree/main/src/memory",
        homepage: "https://modelcontextprotocol.io",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-memory"],
        tags: &["official", "reference", "memory", "notes"],
        required_env: &[],
        notes: "Requires Node.js and npx on PATH.",
    },
    McpPreset {
        id: "pdf",
        display_name: "PDF",
        description: "Official MCP PDF server for inspecting and extracting content from PDF files.",
        package_name: "@modelcontextprotocol/server-pdf",
        source_url: "https://github.com/modelcontextprotocol/ext-apps/tree/main/examples/pdf-server",
        homepage: "https://github.com/modelcontextprotocol/ext-apps#readme",
        command: "npx",
        args: &[
            "-y",
            "--silent",
            "--registry=https://registry.npmjs.org/",
            "@modelcontextprotocol/server-pdf",
            "--stdio",
        ],
        tags: &["official", "archived", "pdf", "documents"],
        required_env: &[],
        notes: "Uses the upstream stdio launch flags from the official package README. Without --stdio the server starts HTTP mode and breaks stdio MCP clients.",
    },
    McpPreset {
        id: "sequential-thinking",
        display_name: "Sequential Thinking",
        description: "Official MCP reasoning helper for explicit step-by-step decomposition and planning.",
        package_name: "@modelcontextprotocol/server-sequential-thinking",
        source_url: "https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking",
        homepage: "https://modelcontextprotocol.io",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-sequential-thinking"],
        tags: &["official", "reference", "reasoning", "planning"],
        required_env: &[],
        notes: "Useful when you want an external MCP server to expose explicit reasoning utilities.",
    },
    McpPreset {
        id: "postgres",
        display_name: "Postgres",
        description: "Official MCP PostgreSQL server for querying and exploring Postgres databases.",
        package_name: "@modelcontextprotocol/server-postgres",
        source_url: "https://github.com/modelcontextprotocol/servers/tree/main/src/postgres",
        homepage: "https://modelcontextprotocol.io",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-postgres"],
        tags: &["official", "archived", "database", "postgres", "sql"],
        required_env: &["DATABASE_URL"],
        notes: "Requires Node.js and a DATABASE_URL connection string in the environment.",
    },
    McpPreset {
        id: "time",
        display_name: "Time",
        description: "Official MCP reference server for current time and timezone conversion utilities.",
        package_name: "mcp-server-time",
        source_url: "https://github.com/modelcontextprotocol/servers/tree/main/src/time",
        homepage: "https://github.com/modelcontextprotocol/servers/blob/main/src/time/README.md",
        command: "uvx",
        args: &["mcp-server-time"],
        tags: &["official", "reference", "time", "timezone"],
        required_env: &[],
        notes: "Official upstream recommends uvx. It auto-detects the system timezone unless you later add --local-timezone.",
    },
];

#[cfg(test)]
pub fn preset_catalog() -> &'static [McpPreset] {
    PRESETS
}

pub fn find_preset(id: &str) -> Option<&'static McpPreset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

#[cfg(test)]
pub fn search_presets(query: Option<&str>) -> Vec<&'static McpPreset> {
    let Some(query) = query.map(|q| q.trim()).filter(|q| !q.is_empty()) else {
        return PRESETS.iter().collect();
    };
    let terms: Vec<String> = normalize_query(query)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if terms.is_empty() {
        return PRESETS.iter().collect();
    }
    PRESETS
        .iter()
        .filter(|preset| {
            let haystack = format!(
                "{} {} {} {} {} {} {} {} {} {}",
                preset.id,
                preset.display_name,
                preset.description,
                preset.package_name,
                preset.source_url,
                preset.homepage,
                preset.command,
                preset.notes,
                preset.args.join(" "),
                preset.tags.join(" ")
            );
            let normalized = normalize_query(&haystack);
            terms.iter().all(|term| normalized.contains(term))
        })
        .collect()
}

fn normalize_query(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
}

fn slugify_name(value: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn section_suffix(section: OfficialCatalogSection) -> Option<&'static str> {
    match section {
        OfficialCatalogSection::None | OfficialCatalogSection::Reference => None,
        OfficialCatalogSection::Archived => Some("archived"),
        OfficialCatalogSection::Integration => Some("official"),
    }
}

fn preset_aliases_for_name(name: &str) -> Option<&'static str> {
    match slugify_name(name).as_str() {
        "everything" => Some("everything"),
        "fetch" => Some("fetch"),
        "filesystem" => Some("filesystem"),
        "git" => Some("git"),
        "github" => Some("github"),
        "memory" => Some("memory"),
        "pdf" => Some("pdf"),
        "postgresql" | "postgres" => Some("postgres"),
        "sequential-thinking" => Some("sequential-thinking"),
        "time" => Some("time"),
        _ => None,
    }
}

fn installable_preset_id_for_entry(name: &str, source_url: &str) -> Option<String> {
    if let Some(id) = preset_aliases_for_name(name) {
        return Some(id.to_string());
    }

    PRESETS
        .iter()
        .find(|preset| {
            source_url.eq_ignore_ascii_case(preset.source_url)
                || source_url.eq_ignore_ascii_case(preset.homepage)
        })
        .map(|preset| preset.id.to_string())
}

fn resolve_entry_id(
    section: OfficialCatalogSection,
    display_name: &str,
    installable_preset_id: Option<&str>,
) -> String {
    let base = installable_preset_id
        .map(str::to_string)
        .unwrap_or_else(|| slugify_name(display_name));
    match section_suffix(section) {
        Some(suffix) if section == OfficialCatalogSection::Archived => format!("{base}-{suffix}"),
        _ => base,
    }
}

fn resolve_source_url(raw_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }

    let relative = trimmed.trim_start_matches("./").trim_start_matches('/');
    format!("{OFFICIAL_MCP_REPO_BASE_URL}{relative}")
}

fn parse_markdown_entry_line(
    line: &str,
    section: OfficialCatalogSection,
) -> Option<OfficialCatalogEntry> {
    let trimmed = line.trim();
    if !trimmed.starts_with("- ") {
        return None;
    }

    let bold_start = trimmed.find("**[")?;
    let name_start = bold_start + 3;
    let name_end = trimmed[name_start..].find("](")? + name_start;
    let url_start = name_end + 2;
    let url_end = trimmed[url_start..].find(')')? + url_start;
    let link_suffix = &trimmed[url_end + 1..];
    let description = link_suffix
        .trim()
        .trim_start_matches('*')
        .trim()
        .trim_start_matches('-')
        .trim_start_matches('–')
        .trim_start_matches('—')
        .trim()
        .to_string();

    let display_name = trimmed[name_start..name_end].trim().to_string();
    let source_url = resolve_source_url(&trimmed[url_start..url_end]);
    let installable_preset_id = installable_preset_id_for_entry(&display_name, &source_url);
    let id = resolve_entry_id(section, &display_name, installable_preset_id.as_deref());

    Some(OfficialCatalogEntry {
        id,
        display_name,
        description,
        source_url: source_url.clone(),
        homepage: source_url,
        tags: section
            .tags()
            .iter()
            .map(|tag| (*tag).to_string())
            .collect(),
        installable_preset_id,
    })
}

fn official_snapshot_entries() -> Vec<OfficialCatalogEntry> {
    PRESETS
        .iter()
        .map(|preset| OfficialCatalogEntry {
            id: preset.id.to_string(),
            display_name: preset.display_name.to_string(),
            description: preset.description.to_string(),
            source_url: preset.source_url.to_string(),
            homepage: preset.homepage.to_string(),
            tags: preset.tags.iter().map(|tag| (*tag).to_string()).collect(),
            installable_preset_id: Some(preset.id.to_string()),
        })
        .collect()
}

fn dedupe_entries(entries: Vec<OfficialCatalogEntry>) -> Vec<OfficialCatalogEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for entry in entries {
        if seen.insert(entry.id.clone()) {
            deduped.push(entry);
        }
    }
    deduped
}

fn parse_official_catalog_markdown(markdown: &str) -> Vec<OfficialCatalogEntry> {
    let mut section = OfficialCatalogSection::None;
    let mut entries = Vec::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        section = match trimmed {
            "## 🌟 Reference Servers" => OfficialCatalogSection::Reference,
            "### Archived" => OfficialCatalogSection::Archived,
            "### 🎖️ Official Integrations" => OfficialCatalogSection::Integration,
            heading if heading.starts_with("## ") => OfficialCatalogSection::None,
            heading if heading.starts_with("### ") && heading != "### Archived" => {
                if section == OfficialCatalogSection::Integration {
                    OfficialCatalogSection::None
                } else {
                    section
                }
            }
            _ => section,
        };

        if section == OfficialCatalogSection::None {
            continue;
        }

        if let Some(entry) = parse_markdown_entry_line(trimmed, section) {
            entries.push(entry);
        }
    }

    dedupe_entries(entries)
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

fn official_catalog_cache_path() -> PathBuf {
    edgecrab_core::edgecrab_home()
        .join("cache")
        .join("mcp_official_catalog.json")
}

fn read_cached_official_catalog() -> Option<CachedOfficialCatalog> {
    let path = official_catalog_cache_path();
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: CachedOfficialCatalog = serde_json::from_str(&content).ok()?;
    if parsed.entries.is_empty() {
        return None;
    }
    Some(parsed)
}

fn write_cached_official_catalog(entries: &[OfficialCatalogEntry]) -> anyhow::Result<()> {
    let path = official_catalog_cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let payload = CachedOfficialCatalog {
        fetched_at_epoch_secs: current_epoch_secs(),
        entries: entries.to_vec(),
    };
    std::fs::write(path, serde_json::to_vec_pretty(&payload)?)?;
    Ok(())
}

fn official_catalog_needs_refresh(cache: Option<&CachedOfficialCatalog>) -> bool {
    let Some(cache) = cache else {
        return true;
    };
    current_epoch_secs().saturating_sub(cache.fetched_at_epoch_secs) > OFFICIAL_CATALOG_MAX_AGE_SECS
}

fn search_entries(
    entries: Vec<OfficialCatalogEntry>,
    query: Option<&str>,
) -> Vec<OfficialCatalogEntry> {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return entries;
    };

    let terms: Vec<String> = normalize_query(query)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if terms.is_empty() {
        return entries;
    }

    entries
        .into_iter()
        .filter(|entry| {
            let haystack = format!(
                "{} {} {} {} {} {} {}",
                entry.id,
                entry.display_name,
                entry.description,
                entry.source_url,
                entry.homepage,
                entry.tags.join(" "),
                entry.installable_preset_id.as_deref().unwrap_or_default()
            );
            let normalized = normalize_query(&haystack);
            terms.iter().all(|term| normalized.contains(term))
        })
        .collect()
}

pub fn load_official_catalog_cached() -> Vec<OfficialCatalogEntry> {
    read_cached_official_catalog()
        .map(|cache| cache.entries)
        .filter(|entries| !entries.is_empty())
        .unwrap_or_else(official_snapshot_entries)
}

#[derive(Debug, Deserialize)]
struct RegistryResponse {
    #[serde(default)]
    servers: Vec<RegistryServerEnvelope>,
}

#[derive(Debug, Deserialize)]
struct RegistryServerEnvelope {
    server: RegistryServer,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryServer {
    name: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    website_url: Option<String>,
    #[serde(default)]
    repository: Option<RegistryRepository>,
    #[serde(default)]
    remotes: Vec<RegistryRemote>,
    #[serde(default)]
    packages: Vec<RegistryPackage>,
}

#[derive(Debug, Deserialize)]
struct RegistryRepository {
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegistryRemote {
    #[serde(rename = "type")]
    transport_type: String,
    url: String,
    #[serde(default)]
    headers: Vec<RegistryHeaderRequirement>,
}

#[derive(Debug, Deserialize)]
struct RegistryHeaderRequirement {
    name: String,
    #[serde(default)]
    is_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryPackage {
    registry_type: String,
    identifier: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    transport: Option<RegistryPackageTransport>,
    #[serde(default)]
    environment_variables: Vec<RegistryEnvironmentVariable>,
}

#[derive(Debug, Deserialize)]
struct RegistryPackageTransport {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct RegistryEnvironmentVariable {
    name: String,
    #[serde(default)]
    is_required: bool,
}

fn official_search_source(id: &str, label: &str, origin: &str) -> McpSearchSourceInfo {
    McpSearchSourceInfo {
        id: id.to_string(),
        label: label.to_string(),
        origin: origin.to_string(),
        trust_level: "official".into(),
    }
}

fn entry_matches_source(entry: &OfficialCatalogEntry, tag: &str) -> bool {
    entry.tags.iter().any(|entry_tag| entry_tag == tag)
}

fn official_entry_to_search_entry(entry: OfficialCatalogEntry) -> McpSearchEntry {
    let install = entry
        .installable_preset_id
        .clone()
        .map(|preset_id| McpInstallPlan::Preset { preset_id });
    McpSearchEntry {
        id: entry.id,
        name: entry.display_name,
        description: entry.description,
        source: "official-catalog".into(),
        origin: entry.source_url.clone(),
        homepage: Some(entry.homepage),
        tags: entry.tags,
        transport: None,
        install,
    }
}

fn registry_origin(server: &RegistryServer) -> String {
    server
        .website_url
        .clone()
        .or_else(|| server.repository.as_ref().and_then(|repo| repo.url.clone()))
        .unwrap_or_else(|| format!("{MCP_REGISTRY_BASE_URL}/?q={}", server.name))
}

fn registry_package_install_plan(package: &RegistryPackage) -> Option<McpInstallPlan> {
    if package.transport.as_ref().map(|t| t.kind.as_str()) != Some("stdio") {
        return None;
    }

    let required_env = package
        .environment_variables
        .iter()
        .filter(|env| env.is_required)
        .map(|env| env.name.clone())
        .collect::<Vec<_>>();

    match package.registry_type.as_str() {
        "npm" => {
            let identifier = if let Some(version) = &package.version {
                format!("{}@{version}", package.identifier)
            } else {
                package.identifier.clone()
            };
            Some(McpInstallPlan::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), identifier],
                required_env,
            })
        }
        "pypi" => {
            let identifier = if let Some(version) = &package.version {
                format!("{}=={version}", package.identifier)
            } else {
                package.identifier.clone()
            };
            Some(McpInstallPlan::Stdio {
                command: "uvx".into(),
                args: vec![identifier],
                required_env,
            })
        }
        _ => None,
    }
}

fn registry_remote_install_plan(remote: &RegistryRemote) -> Option<McpInstallPlan> {
    if remote.transport_type != "streamable-http" {
        return None;
    }

    Some(McpInstallPlan::Http {
        url: remote.url.clone(),
        transport: remote.transport_type.clone(),
        required_headers: remote
            .headers
            .iter()
            .filter(|header| header.is_required)
            .map(|header| header.name.clone())
            .collect(),
    })
}

fn registry_server_to_search_entry(server: RegistryServer) -> McpSearchEntry {
    let origin = registry_origin(&server);
    let install = server
        .remotes
        .iter()
        .find_map(registry_remote_install_plan)
        .or_else(|| {
            server
                .packages
                .iter()
                .find_map(registry_package_install_plan)
        });
    let transport = server
        .remotes
        .first()
        .map(|remote| remote.transport_type.clone())
        .or_else(|| {
            server.packages.first().and_then(|pkg| {
                pkg.transport
                    .as_ref()
                    .map(|transport| transport.kind.clone())
            })
        });
    let mut tags = vec!["official".into(), "registry".into()];
    if let Some(transport) = &transport {
        tags.push(transport.clone());
    }
    if install.is_none() {
        tags.push("view-only".into());
    }
    McpSearchEntry {
        id: server.name.clone(),
        name: server.title.clone().unwrap_or_else(|| server.name.clone()),
        description: server
            .description
            .unwrap_or_else(|| "No description available.".into()),
        source: "mcp-registry".into(),
        origin,
        homepage: server.website_url.clone(),
        tags,
        transport,
        install,
    }
}

async fn search_registry(query: Option<&str>, limit: usize) -> anyhow::Result<Vec<McpSearchEntry>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("edgecrab/mcp-registry-search")
        .build()?;
    let mut request = client
        .get(format!("{MCP_REGISTRY_BASE_URL}/v0.1/servers"))
        .query(&[
            ("limit", limit.to_string()),
            ("version", "latest".to_string()),
        ]);
    if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
        request = request.query(&[("search", query.to_string())]);
    }
    let response = request.send().await?.error_for_status()?;
    let payload: RegistryResponse = response.json().await?;
    Ok(payload
        .servers
        .into_iter()
        .map(|entry| registry_server_to_search_entry(entry.server))
        .collect())
}

pub async fn search_mcp_sources(query: Option<&str>, per_source_limit: usize) -> McpSearchReport {
    let (official_entries, registry_results) = tokio::join!(
        load_official_catalog(true),
        search_registry(query, per_source_limit)
    );

    let filtered_official = search_entries(official_entries, query);
    let mut groups = Vec::new();

    let reference_results = filtered_official
        .iter()
        .filter(|entry| entry_matches_source(entry, "reference"))
        .cloned()
        .map(official_entry_to_search_entry)
        .take(per_source_limit)
        .collect::<Vec<_>>();
    groups.push(McpSearchGroup {
        source: official_search_source("mcp-reference", "MCP Reference", OFFICIAL_MCP_README_URL),
        results: reference_results,
        notice: None,
    });

    let integration_results = filtered_official
        .iter()
        .filter(|entry| entry_matches_source(entry, "integration"))
        .cloned()
        .map(official_entry_to_search_entry)
        .take(per_source_limit)
        .collect::<Vec<_>>();
    groups.push(McpSearchGroup {
        source: official_search_source(
            "mcp-integrations",
            "Official Apps",
            OFFICIAL_MCP_README_URL,
        ),
        results: integration_results,
        notice: None,
    });

    let archived_results = filtered_official
        .iter()
        .filter(|entry| entry_matches_source(entry, "archived"))
        .cloned()
        .map(official_entry_to_search_entry)
        .take(per_source_limit)
        .collect::<Vec<_>>();
    groups.push(McpSearchGroup {
        source: official_search_source(
            "mcp-archived",
            "Archived",
            OFFICIAL_MCP_README_URL,
        ),
        results: archived_results,
        notice: Some(
            "Archived upstream entries are shown for discovery but should be treated as compatibility-only.".into(),
        ),
    });

    match registry_results {
        Ok(results) => groups.push(McpSearchGroup {
            source: official_search_source(
                "mcp-registry",
                "MCP Registry",
                MCP_REGISTRY_BASE_URL,
            ),
            results,
            notice: Some(
                "Registry results are live official listings. EdgeCrab only auto-installs streamable HTTP, npm stdio, and PyPI stdio entries.".into(),
            ),
        }),
        Err(err) => groups.push(McpSearchGroup {
            source: official_search_source(
                "mcp-registry",
                "MCP Registry",
                MCP_REGISTRY_BASE_URL,
            ),
            results: Vec::new(),
            notice: Some(format!("Registry search failed: {err}")),
        }),
    }

    McpSearchReport { groups }
}

pub fn render_search_report(query: Option<&str>, report: &McpSearchReport) -> String {
    let mut lines = vec![match query.map(str::trim).filter(|q| !q.is_empty()) {
        Some(query) => format!("Official MCP search for '{query}'"),
        None => "Official MCP search".into(),
    }];

    for group in &report.groups {
        lines.push(String::new());
        lines.push(format!(
            "{} [{}]",
            group.source.label, group.source.trust_level
        ));
        if group.results.is_empty() {
            lines.push("  (no matches)".into());
        } else {
            for entry in &group.results {
                let install = match &entry.install {
                    Some(McpInstallPlan::Preset { preset_id }) => format!("install={preset_id}"),
                    Some(McpInstallPlan::Http { transport, .. }) => {
                        format!("install=http:{transport}")
                    }
                    Some(McpInstallPlan::Stdio { command, .. }) => {
                        format!("install=stdio:{command}")
                    }
                    None => "install=view-only".into(),
                };
                lines.push(format!(
                    "  {} — {} [{}] {}",
                    entry.id,
                    entry.description,
                    entry.tags.join(", "),
                    install,
                ));
            }
        }
        if let Some(notice) = &group.notice {
            lines.push(format!("  note: {notice}"));
        }
    }

    lines.join("\n")
}

pub async fn load_official_catalog(prefer_refresh: bool) -> Vec<OfficialCatalogEntry> {
    let cache = read_cached_official_catalog();
    if !prefer_refresh {
        if let Some(cache) = cache.as_ref() {
            return cache.entries.clone();
        }
    }

    if prefer_refresh || official_catalog_needs_refresh(cache.as_ref()) {
        if let Ok(entries) = refresh_official_catalog().await {
            return entries;
        }
    }

    cache
        .map(|cached| cached.entries)
        .filter(|entries| !entries.is_empty())
        .unwrap_or_else(official_snapshot_entries)
}

#[cfg(test)]
pub fn search_official_catalog(query: Option<&str>) -> Vec<OfficialCatalogEntry> {
    search_entries(load_official_catalog_cached(), query)
}

pub async fn find_official_catalog_entry_with_refresh(id: &str) -> Option<OfficialCatalogEntry> {
    load_official_catalog(true)
        .await
        .into_iter()
        .find(|entry| entry.id == id)
}

pub async fn refresh_official_catalog() -> anyhow::Result<Vec<OfficialCatalogEntry>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("edgecrab/official-mcp-catalog")
        .build()?;
    let response = client.get(OFFICIAL_MCP_README_URL).send().await?;
    let response = response.error_for_status()?;
    let markdown = response.text().await?;
    let entries = parse_official_catalog_markdown(&markdown);
    if entries.is_empty() {
        anyhow::bail!("official MCP catalog parsing returned zero entries");
    }
    write_cached_official_catalog(&entries)?;
    Ok(entries)
}

pub fn render_official_catalog_entry(entry: &OfficialCatalogEntry) -> String {
    let preset = entry.installable_preset_id.as_deref().and_then(find_preset);
    let mut lines = vec![
        format!("Catalog: {}", entry.id),
        format!("Name:    {}", entry.display_name),
        format!("Why:     {}", entry.description),
        format!("Source:  {}", entry.source_url),
        format!("Docs:    {}", entry.homepage),
        format!("Tags:    {}", entry.tags.join(", ")),
    ];
    match (&entry.installable_preset_id, preset) {
        (Some(installable), Some(preset)) => {
            lines.push(format!("Install: /mcp install {installable}"));
            lines.push(format!("Preset:  {}", preset.display_name));
            lines.push(format!("Pkg:     {}", preset.package_name));
            lines.push(format!(
                "Cmd:     {} {}",
                preset.command,
                preset.args.join(" ")
            ));
            if !preset.required_env.is_empty() {
                lines.push(format!("Env:     {}", preset.required_env.join(", ")));
            }
            lines.push(format!("Notes:   {}", preset.notes));
        }
        (Some(installable), None) => {
            lines.push(format!("Install: /mcp install {installable}"));
            lines.push(
                "Preset:  installable catalog entry, but no bundled preset metadata was found"
                    .into(),
            );
        }
        (None, _) => {
            lines.push("Install: not available as a controlled preset yet".into());
        }
    }
    lines.join("\n")
}

pub fn render_preset_detail(preset: &McpPreset) -> String {
    let mut lines = vec![
        format!("Preset: {}", preset.id),
        format!("Name:   {}", preset.display_name),
        format!("Why:    {}", preset.description),
        format!("Pkg:    {}", preset.package_name),
        format!("Source: {}", preset.source_url),
        format!("Docs:   {}", preset.homepage),
        format!("Cmd:    {} {}", preset.command, preset.args.join(" ")),
        format!("Tags:   {}", preset.tags.join(", ")),
    ];
    if !preset.required_env.is_empty() {
        lines.push(format!("Env:    {}", preset.required_env.join(", ")));
    }
    lines.push(format!("Notes:  {}", preset.notes));
    lines.join("\n")
}

#[cfg(test)]
mod render_tests {
    use super::*;

    #[test]
    fn render_official_catalog_entry_includes_install_plan_metadata() {
        let rendered = render_official_catalog_entry(&OfficialCatalogEntry {
            id: "github".into(),
            display_name: "GitHub".into(),
            description: "Official GitHub server.".into(),
            source_url: "https://github.com/github/github-mcp-server".into(),
            homepage: "https://modelcontextprotocol.io".into(),
            tags: vec!["official".into(), "integration".into()],
            installable_preset_id: Some("github".into()),
        });

        assert!(rendered.contains("Install: /mcp install github"));
        assert!(rendered.contains("Pkg:     @modelcontextprotocol/server-github"));
        assert!(rendered.contains("Cmd:     npx -y @modelcontextprotocol/server-github"));
        assert!(rendered.contains("Env:     GITHUB_PERSONAL_ACCESS_TOKEN, GITHUB_TOKEN"));
    }

    #[test]
    fn render_preset_detail_includes_notes_and_command() {
        let preset = find_preset("time").expect("time preset");
        let rendered = render_preset_detail(preset);

        assert!(rendered.contains("Preset: time"));
        assert!(rendered.contains("Cmd:    uvx mcp-server-time"));
        assert!(rendered.contains("Notes:"));
    }
}

#[cfg(test)]
mod detail_render_tests {
    use super::*;

    #[test]
    fn render_official_catalog_entry_without_install_plan_mentions_catalog_only_state() {
        let rendered = render_official_catalog_entry(&OfficialCatalogEntry {
            id: "custom-http".into(),
            display_name: "Custom HTTP".into(),
            description: "HTTP-only catalog entry.".into(),
            source_url: "https://example.com/custom-http".into(),
            homepage: "https://example.com/docs".into(),
            tags: vec!["community".into()],
            installable_preset_id: None,
        });

        assert!(rendered.contains("Install: not available as a controlled preset yet"));
    }
}

pub fn install_preset(
    config: &mut AppConfig,
    preset_id: &str,
    name_override: Option<&str>,
    path_override: Option<&Path>,
    cwd: &Path,
) -> anyhow::Result<InstalledPreset> {
    let preset = find_preset(preset_id)
        .ok_or_else(|| anyhow::anyhow!("unknown MCP preset '{}'", preset_id))?;
    let name = name_override.unwrap_or(preset.id).trim().to_string();

    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        anyhow::bail!("unsafe MCP server name '{}'", name);
    }

    let resolved_path = path_override
        .map(PathBuf::from)
        .or_else(|| {
            if preset.args.iter().any(|arg| arg.contains("{{workspace}}")) {
                Some(cwd.to_path_buf())
            } else {
                None
            }
        })
        .map(|path| normalize_install_path(&path));

    let args = preset
        .args
        .iter()
        .map(|arg| render_arg(arg, resolved_path.as_deref()))
        .collect();

    config.mcp_servers.insert(
        name.clone(),
        McpServerConfig {
            command: preset.command.to_string(),
            args,
            cwd: resolved_path,
            enabled: true,
            ..Default::default()
        },
    );

    let missing_env = preset
        .required_env
        .iter()
        .filter(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.is_empty())
                .is_none()
        })
        .map(|name| (*name).to_string())
        .collect();

    Ok(InstalledPreset { name, missing_env })
}

fn preferred_search_entry_name(entry: &McpSearchEntry) -> String {
    let candidate = if let Some(McpInstallPlan::Preset { preset_id }) = &entry.install {
        preset_id.clone()
    } else if !entry.id.trim().is_empty() {
        entry
            .id
            .split('/')
            .next_back()
            .map(slugify_name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| slugify_name(&entry.name))
    } else {
        slugify_name(&entry.name)
    };
    if candidate.is_empty() {
        "mcp-server".into()
    } else {
        candidate
    }
}

fn allocate_server_name(config: &AppConfig, preferred: &str) -> String {
    if !config.mcp_servers.contains_key(preferred) {
        return preferred.to_string();
    }

    for idx in 2..1000 {
        let candidate = format!("{preferred}-{idx}");
        if !config.mcp_servers.contains_key(&candidate) {
            return candidate;
        }
    }

    format!("{preferred}-{}", current_epoch_secs())
}

pub fn install_search_entry(
    config: &mut AppConfig,
    entry: &McpSearchEntry,
    cwd: &Path,
) -> anyhow::Result<InstalledSearchEntry> {
    match &entry.install {
        Some(McpInstallPlan::Preset { preset_id }) => {
            let installed = install_preset(config, preset_id, None, None, cwd)?;
            Ok(InstalledSearchEntry {
                name: installed.name,
                warnings: installed.missing_env,
            })
        }
        Some(McpInstallPlan::Http {
            url,
            required_headers,
            ..
        }) => {
            let preferred = preferred_search_entry_name(entry);
            let name = allocate_server_name(config, &preferred);
            config.mcp_servers.insert(
                name.clone(),
                McpServerConfig {
                    url: Some(url.clone()),
                    enabled: true,
                    ..Default::default()
                },
            );
            let warnings = if required_headers.is_empty() {
                Vec::new()
            } else {
                vec![format!(
                    "Required auth/config headers still need manual setup: {}",
                    required_headers.join(", ")
                )]
            };
            Ok(InstalledSearchEntry { name, warnings })
        }
        Some(McpInstallPlan::Stdio {
            command,
            args,
            required_env,
        }) => {
            let preferred = preferred_search_entry_name(entry);
            let name = allocate_server_name(config, &preferred);
            config.mcp_servers.insert(
                name.clone(),
                McpServerConfig {
                    command: command.clone(),
                    args: args.clone(),
                    cwd: Some(normalize_install_path(cwd)),
                    enabled: true,
                    ..Default::default()
                },
            );
            Ok(InstalledSearchEntry {
                name,
                warnings: required_env.clone(),
            })
        }
        None => anyhow::bail!(
            "'{}' is view-only and cannot be installed automatically",
            entry.id
        ),
    }
}

fn render_arg(arg: &str, resolved_path: Option<&Path>) -> String {
    if arg == "{{workspace}}" {
        return resolved_path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| ".".to_string());
    }
    arg.to_string()
}

fn normalize_install_path(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Windows: canonicalize() returns \\?\-prefixed extended-length paths.
    // Strip the prefix so stored cwds and rendered args are plain paths that
    // external commands and test assertions can compare without special-casing.
    #[cfg(windows)]
    if let Some(stripped) = canonical.to_string_lossy().strip_prefix(r"\\?\") {
        return PathBuf::from(stripped.to_string());
    }
    canonical
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    struct TempEdgecrabHome {
        _guard: MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
    }

    impl TempEdgecrabHome {
        fn new() -> Self {
            let guard = crate::gateway_catalog::TEST_ENV_LOCK
                .lock()
                .expect("env lock");
            let dir = tempfile::tempdir().expect("tempdir");
            unsafe {
                std::env::set_var("EDGECRAB_HOME", dir.path());
            }
            Self {
                _guard: guard,
                _dir: dir,
            }
        }
    }

    impl Drop for TempEdgecrabHome {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var("EDGECRAB_HOME");
            }
        }
    }

    #[test]
    fn search_without_query_returns_all_presets() {
        assert_eq!(search_presets(None).len(), preset_catalog().len());
    }

    #[test]
    fn search_matches_tags_and_description() {
        let results = search_presets(Some("git"));
        assert!(results.iter().any(|preset| preset.id == "github"));
    }

    #[test]
    fn search_matches_package_name_and_source() {
        assert!(
            search_presets(Some("server pdf"))
                .iter()
                .any(|preset| preset.id == "pdf")
        );
        assert!(
            search_presets(Some("ext apps"))
                .iter()
                .any(|preset| preset.id == "pdf")
        );
    }

    #[test]
    fn search_matches_reference_servers_from_official_snapshot() {
        assert!(
            search_presets(Some("mcp-server-fetch"))
                .iter()
                .any(|preset| preset.id == "fetch")
        );
        assert!(
            search_presets(Some("timezone"))
                .iter()
                .any(|preset| preset.id == "time")
        );
        assert!(
            search_presets(Some("repository"))
                .iter()
                .any(|preset| preset.id == "git")
        );
        assert!(
            search_presets(Some("everything protocol"))
                .iter()
                .any(|preset| preset.id == "everything")
        );
    }

    #[test]
    fn install_filesystem_uses_cwd_by_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = AppConfig::default();

        let installed =
            install_preset(&mut config, "filesystem", None, None, dir.path()).expect("install");

        assert_eq!(installed.name, "filesystem");
        let server = config
            .mcp_servers
            .get("filesystem")
            .expect("filesystem preset");
        // Use normalize_install_path for comparison so that the match is against
        // the same canonical (\\?\ -stripped on Windows) representation that
        // render_arg stores in the args list.
        let expected = normalize_install_path(dir.path());
        let expected_str = expected.display().to_string();
        assert!(
            server.args.iter().any(|arg| arg.contains(&expected_str)),
            "no arg contains expected path {expected_str}; got: {:?}",
            server.args
        );
        assert_eq!(server.cwd.as_deref(), Some(expected.as_path()));
    }

    #[test]
    fn install_rejects_unsafe_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = AppConfig::default();
        let result = install_preset(&mut config, "filesystem", Some("../bad"), None, dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn parse_official_catalog_extracts_reference_archived_and_integrations() {
        let markdown = r#"
## 🌟 Reference Servers

- **[Everything](src/everything)** - Reference / test server with prompts, resources, and tools.

### Archived

- **[PostgreSQL](https://github.com/modelcontextprotocol/servers-archived/tree/main/src/postgres)** - Read-only database access with schema inspection.

## 🤝 Third-Party Servers

### 🎖️ Official Integrations

- <img height="12" width="12" src="https://github.githubassets.com/assets/GitHub-Mark-ea2971cee799.png" alt="GitHub Logo" /> **[GitHub](https://github.com/github/github-mcp-server)** - GitHub's official MCP Server.
"#;

        let entries = parse_official_catalog_markdown(markdown);
        assert!(entries.iter().any(|entry| entry.id == "everything"));
        assert!(entries.iter().any(|entry| entry.id == "postgres-archived"));
        assert!(entries.iter().any(|entry| entry.id == "github"));

        let everything = entries
            .iter()
            .find(|entry| entry.id == "everything")
            .expect("everything entry");
        assert_eq!(
            everything.source_url,
            "https://github.com/modelcontextprotocol/servers/tree/main/src/everything"
        );
    }

    #[test]
    #[serial_test::serial(edgecrab_home_env)]
    fn cached_catalog_falls_back_to_snapshot() {
        let _home = TempEdgecrabHome::new();
        let entries = load_official_catalog_cached();
        assert!(entries.iter().any(|entry| entry.id == "git"));
    }

    #[test]
    #[serial_test::serial(edgecrab_home_env)]
    fn search_official_catalog_matches_official_entry_metadata() {
        let _home = TempEdgecrabHome::new();
        write_cached_official_catalog(&[OfficialCatalogEntry {
            id: "brave-search-archived".into(),
            display_name: "Brave Search".into(),
            description: "Web and local search using Brave's Search API.".into(),
            source_url: "https://github.com/modelcontextprotocol/servers-archived/tree/main/src/brave-search".into(),
            homepage: "https://github.com/brave/brave-search-mcp-server".into(),
            tags: vec!["official".into(), "archived".into()],
            installable_preset_id: None,
        }])
        .expect("write cache");

        let results = search_official_catalog(Some("brave search archived"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "brave-search-archived");
    }

    #[test]
    fn registry_server_http_entries_become_installable_http_results() {
        let entry = registry_server_to_search_entry(RegistryServer {
            name: "acme/http".into(),
            title: Some("Acme HTTP".into()),
            description: Some("Remote HTTP MCP".into()),
            website_url: Some("https://acme.test/mcp".into()),
            repository: None,
            remotes: vec![RegistryRemote {
                transport_type: "streamable-http".into(),
                url: "https://acme.test/mcp".into(),
                headers: vec![RegistryHeaderRequirement {
                    name: "Authorization".into(),
                    is_required: true,
                }],
            }],
            packages: Vec::new(),
        });

        assert_eq!(entry.id, "acme/http");
        assert_eq!(entry.transport.as_deref(), Some("streamable-http"));
        assert!(matches!(entry.install, Some(McpInstallPlan::Http { .. })));
    }

    #[test]
    fn install_search_entry_allocates_unique_name_for_registry_http_entry() {
        let mut config = AppConfig::default();
        config.mcp_servers.insert(
            "docs-mcp".into(),
            McpServerConfig {
                command: "npx".into(),
                ..Default::default()
            },
        );

        let installed = install_search_entry(
            &mut config,
            &McpSearchEntry {
                id: "ac.tandem/docs-mcp".into(),
                name: "Docs MCP".into(),
                description: "Docs".into(),
                source: "mcp-registry".into(),
                origin: "https://tandem.ac/mcp".into(),
                homepage: None,
                tags: vec!["official".into(), "registry".into()],
                transport: Some("streamable-http".into()),
                install: Some(McpInstallPlan::Http {
                    url: "https://tandem.ac/mcp".into(),
                    transport: "streamable-http".into(),
                    required_headers: vec!["Authorization".into()],
                }),
            },
            Path::new("."),
        )
        .expect("install");

        assert_eq!(installed.name, "docs-mcp-2");
        assert!(
            config
                .mcp_servers
                .get("docs-mcp-2")
                .and_then(|server| server.url.as_deref())
                .is_some_and(|url| url == "https://tandem.ac/mcp")
        );
    }
}
