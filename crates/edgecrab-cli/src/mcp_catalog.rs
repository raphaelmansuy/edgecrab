use std::path::{Path, PathBuf};

use edgecrab_core::AppConfig;
use edgecrab_core::config::McpServerConfig;

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

fn render_arg(arg: &str, resolved_path: Option<&Path>) -> String {
    if arg == "{{workspace}}" {
        return resolved_path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| ".".to_string());
    }
    arg.to_string()
}

fn normalize_install_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(
            server
                .args
                .iter()
                .any(|arg| arg.contains(&dir.path().display().to_string()))
        );
        let expected = normalize_install_path(dir.path());
        assert_eq!(server.cwd.as_deref(), Some(expected.as_path()));
    }

    #[test]
    fn install_rejects_unsafe_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = AppConfig::default();
        let result = install_preset(&mut config, "filesystem", Some("../bad"), None, dir.path());
        assert!(result.is_err());
    }
}
