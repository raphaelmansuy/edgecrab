use std::path::{Path, PathBuf};

use edgecrab_core::AppConfig;
use edgecrab_core::config::McpServerConfig;

pub struct McpPreset {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
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
        id: "filesystem",
        display_name: "Filesystem",
        description: "Official MCP filesystem server scoped to one allowed root path.",
        command: "npx",
        args: &[
            "-y",
            "@modelcontextprotocol/server-filesystem",
            "{{workspace}}",
        ],
        tags: &["files", "local", "official"],
        required_env: &[],
        notes: "Install with --path to scope access to a specific directory. Defaults to the current working directory.",
    },
    McpPreset {
        id: "github",
        display_name: "GitHub",
        description: "Official MCP GitHub server for issues, pull requests, and repository operations.",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-github"],
        tags: &["github", "git", "official"],
        required_env: &["GITHUB_TOKEN"],
        notes: "Requires a GitHub token in the shell environment or ~/.edgecrab/.env.",
    },
    McpPreset {
        id: "memory",
        display_name: "Memory",
        description: "Official MCP memory server for lightweight persistent facts and notes.",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-memory"],
        tags: &["memory", "notes", "official"],
        required_env: &[],
        notes: "Requires Node.js and npx on PATH.",
    },
    McpPreset {
        id: "pdf",
        display_name: "PDF",
        description: "Official MCP PDF server for inspecting and extracting content from PDF files.",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-pdf"],
        tags: &["pdf", "documents", "official"],
        required_env: &[],
        notes: "Requires Node.js and npx on PATH.",
    },
    McpPreset {
        id: "sequential-thinking",
        display_name: "Sequential Thinking",
        description: "Official MCP reasoning helper for explicit step-by-step decomposition and planning.",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-sequential-thinking"],
        tags: &["reasoning", "planning", "official"],
        required_env: &[],
        notes: "Useful when you want an external MCP server to expose explicit reasoning utilities.",
    },
    McpPreset {
        id: "postgres",
        display_name: "Postgres",
        description: "Official MCP PostgreSQL server for querying and exploring Postgres databases.",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-postgres"],
        tags: &["database", "postgres", "sql", "official"],
        required_env: &["DATABASE_URL"],
        notes: "Requires Node.js and a DATABASE_URL connection string in the environment.",
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
    let q = query.to_lowercase();
    PRESETS
        .iter()
        .filter(|preset| {
            preset.id.to_lowercase().contains(&q)
                || preset.display_name.to_lowercase().contains(&q)
                || preset.description.to_lowercase().contains(&q)
                || preset
                    .tags
                    .iter()
                    .any(|tag| tag.to_lowercase().contains(&q))
        })
        .collect()
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
