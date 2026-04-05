//! # plugins -- Plugin system with TOML manifests and hook points
//!
//! WHY plugins: Power users need extensibility beyond built-in toolsets.
//! Plugins let users add custom tools, pre/post-processing hooks, and
//! startup/shutdown actions without forking the codebase.
//!
//! ```text
//!   Discovery order:
//!     1. ~/.edgecrab/plugins/       (user plugins)
//!     2. .edgecrab/plugins/         (project-local plugins)
//!     3. /usr/share/edgecrab/plugins/ (system plugins)
//!
//!   Plugin layout:
//!     my-plugin/
//!       plugin.toml          <- manifest (name, version, hooks, tools)
//!       tools/               <- tool definitions (YAML/JSON)
//!       scripts/             <- hook scripts (shell/python/etc.)
//! ```
//!
//! Hooks are fired at six well-defined points matching hermes-agent:
//!   PreMessage, PostMessage, PreTool, PostTool, OnStartup, OnShutdown

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---- Types ----------------------------------------------------------------

/// Where a plugin was discovered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginSource {
    /// User-level: ~/.edgecrab/plugins/
    User,
    /// Project-level: .edgecrab/plugins/
    Project,
    /// System-level: /usr/share/edgecrab/plugins/
    System,
}

impl std::fmt::Display for PluginSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
            Self::System => write!(f, "system"),
        }
    }
}

/// Hook points where plugins can inject behaviour.
///
/// WHY six hooks: Mirrors hermes-agent's plugin lifecycle exactly.
/// Pre/Post pairs for messages and tools, plus startup/shutdown for
/// one-time init/cleanup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginHook {
    /// Before a user message is sent to the LLM
    PreMessage,
    /// After the LLM response is received
    PostMessage,
    /// Before a tool call is executed
    PreTool,
    /// After a tool call completes
    PostTool,
    /// When the agent starts up
    OnStartup,
    /// When the agent shuts down
    OnShutdown,
}

impl std::fmt::Display for PluginHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreMessage => write!(f, "pre_message"),
            Self::PostMessage => write!(f, "post_message"),
            Self::PreTool => write!(f, "pre_tool"),
            Self::PostTool => write!(f, "post_tool"),
            Self::OnStartup => write!(f, "on_startup"),
            Self::OnShutdown => write!(f, "on_shutdown"),
        }
    }
}

/// A tool definition contributed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTool {
    /// Tool name (must be unique across all plugins)
    pub name: String,
    /// Human-readable description for the LLM
    pub description: String,
    /// Command to execute (shell command or script path)
    pub command: String,
    /// Expected input schema (JSON Schema as string, optional)
    #[serde(default)]
    pub input_schema: Option<String>,
}

/// A hook handler definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookHandler {
    /// Which hook this handler binds to
    pub hook: PluginHook,
    /// Command or script to run
    pub command: String,
    /// Whether failure of this hook should abort the pipeline
    #[serde(default)]
    pub required: bool,
}

/// TOML manifest for a plugin (`plugin.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub hooks: Vec<HookHandler>,
    #[serde(default)]
    pub tools: Vec<PluginTool>,
}

fn default_version() -> String {
    "0.1.0".into()
}

/// A fully resolved plugin (manifest + source metadata).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Plugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub source: PluginSource,
    pub path: PathBuf,
    pub hooks: Vec<HookHandler>,
    pub tools: Vec<PluginTool>,
    pub enabled: bool,
}

/// Context passed to hook handlers at execution time.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct HookContext {
    /// The hook being executed
    pub hook_name: String,
    /// Message content (for message hooks)
    pub message: Option<String>,
    /// Tool name (for tool hooks)
    pub tool_name: Option<String>,
    /// Tool arguments as JSON (for tool hooks)
    pub tool_args: Option<String>,
    /// Arbitrary key-value metadata
    pub metadata: HashMap<String, String>,
}

// ---- PluginManager --------------------------------------------------------

/// Discovers, loads, and manages plugins across all source directories.
///
/// WHY a manager struct: Centralizes discovery logic and provides a
/// single point for hook dispatch. The alternative (global functions)
/// would scatter state across the codebase.
pub struct PluginManager {
    plugins: Vec<Plugin>,
}

impl PluginManager {
    /// Create an empty manager (no plugins discovered yet).
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Discover plugins from all standard directories.
    ///
    /// Scans in priority order: user > project > system.
    /// Later discoveries do not overwrite earlier ones with the same name.
    pub fn discover() -> Vec<Plugin> {
        let mut plugins = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        let dirs = Self::plugin_dirs();
        for (dir, source) in &dirs {
            if !dir.exists() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let manifest_path = path.join("plugin.toml");
                    if !manifest_path.exists() {
                        continue;
                    }
                    match Self::load_manifest(&manifest_path) {
                        Ok(manifest) => {
                            if seen_names.contains(&manifest.name) {
                                tracing::debug!(
                                    "skipping duplicate plugin '{}' from {}",
                                    manifest.name,
                                    dir.display()
                                );
                                continue;
                            }
                            seen_names.insert(manifest.name.clone());
                            plugins.push(Plugin {
                                name: manifest.name.clone(),
                                version: manifest.version.clone(),
                                description: manifest.description.clone(),
                                source: source.clone(),
                                path: path.clone(),
                                hooks: manifest.hooks,
                                tools: manifest.tools,
                                enabled: true,
                            });
                        }
                        Err(e) => {
                            tracing::warn!(
                                "failed to load plugin manifest {}: {e}",
                                manifest_path.display()
                            );
                        }
                    }
                }
            }
        }

        plugins
    }

    /// Discover and populate the manager's plugin list.
    pub fn discover_all(&mut self) {
        self.plugins = Self::discover();
    }

    /// Load a single plugin by name from the discovered set.
    #[allow(dead_code)]
    pub fn load(&self, name: &str) -> anyhow::Result<&Plugin> {
        self.plugins
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| anyhow::anyhow!("plugin not found: '{name}'"))
    }

    /// Get all discovered plugins.
    pub fn plugins(&self) -> &[Plugin] {
        &self.plugins
    }

    /// Get names of all discovered plugins.
    #[allow(dead_code)]
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.name.clone()).collect()
    }

    /// Execute all handlers for a given hook point.
    ///
    /// WHY synchronous: Hook scripts are typically fast (env setup,
    /// logging, validation). Async execution can be added later for
    /// long-running hooks via tokio::process::Command.
    #[allow(dead_code)]
    pub fn execute_hook(&self, hook: PluginHook, ctx: &HookContext) -> anyhow::Result<()> {
        for plugin in &self.plugins {
            if !plugin.enabled {
                continue;
            }
            for handler in &plugin.hooks {
                if handler.hook != hook {
                    continue;
                }

                tracing::debug!("executing hook {} for plugin '{}'", hook, plugin.name);

                let result = Self::run_hook_command(&handler.command, &plugin.path, ctx);

                if let Err(e) = &result {
                    if handler.required {
                        return Err(anyhow::anyhow!(
                            "required hook '{}' in plugin '{}' failed: {e}",
                            hook,
                            plugin.name
                        ));
                    }
                    tracing::warn!(
                        "optional hook '{}' in plugin '{}' failed: {e}",
                        hook,
                        plugin.name
                    );
                }
            }
        }
        Ok(())
    }

    /// Get all tools contributed by all enabled plugins.
    #[allow(dead_code)]
    pub fn all_tools(&self) -> Vec<&PluginTool> {
        self.plugins
            .iter()
            .filter(|p| p.enabled)
            .flat_map(|p| &p.tools)
            .collect()
    }

    // ---- Internal helpers -------------------------------------------------

    /// Standard plugin directory search paths.
    fn plugin_dirs() -> Vec<(PathBuf, PluginSource)> {
        let mut dirs = vec![(
            edgecrab_core::edgecrab_home().join("plugins"),
            PluginSource::User,
        )];

        // Project-local plugins
        dirs.push((
            PathBuf::from(".edgecrab").join("plugins"),
            PluginSource::Project,
        ));

        // System plugins (Unix)
        #[cfg(unix)]
        dirs.push((
            PathBuf::from("/usr/share/edgecrab/plugins"),
            PluginSource::System,
        ));

        // System plugins (Windows)
        #[cfg(windows)]
        if let Ok(program_data) = std::env::var("ProgramData") {
            dirs.push((
                PathBuf::from(program_data).join("edgecrab").join("plugins"),
                PluginSource::System,
            ));
        }

        dirs
    }

    /// Parse a plugin.toml manifest.
    fn load_manifest(path: &Path) -> anyhow::Result<PluginManifest> {
        let content = std::fs::read_to_string(path)?;
        let manifest: PluginManifest = toml_parse(&content)?;
        Ok(manifest)
    }

    /// Run a hook command in the plugin's directory.
    ///
    /// Environment variables are set for the hook context:
    ///   EDGECRAB_HOOK   = hook name
    ///   EDGECRAB_MSG    = message content (if present)
    ///   EDGECRAB_TOOL   = tool name (if present)
    #[allow(dead_code)]
    fn run_hook_command(
        command: &str,
        working_dir: &Path,
        ctx: &HookContext,
    ) -> anyhow::Result<()> {
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", command]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", command]);
            c
        };

        cmd.current_dir(working_dir);
        cmd.env("EDGECRAB_HOOK", &ctx.hook_name);
        if let Some(msg) = &ctx.message {
            cmd.env("EDGECRAB_MSG", msg);
        }
        if let Some(tool) = &ctx.tool_name {
            cmd.env("EDGECRAB_TOOL", tool);
        }
        if let Some(args) = &ctx.tool_args {
            cmd.env("EDGECRAB_TOOL_ARGS", args);
        }

        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "hook command failed (exit {}): {}",
                output.status,
                stderr.trim()
            );
        }
        Ok(())
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Minimal TOML parser --------------------------------------------------

/// Parse TOML content into a PluginManifest.
///
/// WHY a wrapper: We avoid pulling in the `toml` crate dependency by
/// using serde_json as an intermediate step with a hand-rolled TOML-lite
/// parser. For production, replace with `toml::from_str`.
fn toml_parse(content: &str) -> anyhow::Result<PluginManifest> {
    // Simple TOML parser for plugin manifests.
    // Handles flat key=value pairs and [[array]] tables.
    let mut name = String::new();
    let mut version = String::from("0.1.0");
    let mut description = String::new();
    let mut author = String::new();
    let mut hooks = Vec::new();
    let mut tools = Vec::new();
    let mut current_section = String::new();
    let mut current_map: HashMap<String, String> = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Section headers
        if line.starts_with("[[") && line.ends_with("]]") {
            // Flush previous section
            flush_section(&current_section, &current_map, &mut hooks, &mut tools);
            current_section = line[2..line.len() - 2].trim().to_string();
            current_map.clear();
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            flush_section(&current_section, &current_map, &mut hooks, &mut tools);
            current_section = line[1..line.len() - 1].trim().to_string();
            current_map.clear();
            continue;
        }

        // Key = value
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim().to_string();
            let val = val.trim().trim_matches('"').to_string();

            match current_section.as_str() {
                "" | "plugin" => match key.as_str() {
                    "name" => name = val,
                    "version" => version = val,
                    "description" => description = val,
                    "author" => author = val,
                    _ => {}
                },
                _ => {
                    current_map.insert(key, val);
                }
            }
        }
    }

    // Flush last section
    flush_section(&current_section, &current_map, &mut hooks, &mut tools);

    if name.is_empty() {
        anyhow::bail!("plugin manifest missing 'name' field");
    }

    Ok(PluginManifest {
        name,
        version,
        description,
        author,
        hooks,
        tools,
    })
}

fn flush_section(
    section: &str,
    map: &HashMap<String, String>,
    hooks: &mut Vec<HookHandler>,
    tools: &mut Vec<PluginTool>,
) {
    if map.is_empty() {
        return;
    }
    match section {
        "hooks" | "hook" => {
            if let (Some(hook_str), Some(command)) = (map.get("hook"), map.get("command")) {
                let hook = match hook_str.as_str() {
                    "pre_message" => PluginHook::PreMessage,
                    "post_message" => PluginHook::PostMessage,
                    "pre_tool" => PluginHook::PreTool,
                    "post_tool" => PluginHook::PostTool,
                    "on_startup" => PluginHook::OnStartup,
                    "on_shutdown" => PluginHook::OnShutdown,
                    _ => return,
                };
                hooks.push(HookHandler {
                    hook,
                    command: command.clone(),
                    required: map.get("required").map(|v| v == "true").unwrap_or(false),
                });
            }
        }
        "tools" | "tool" => {
            if let (Some(name), Some(command)) = (map.get("name"), map.get("command")) {
                tools.push(PluginTool {
                    name: name.clone(),
                    description: map.get("description").cloned().unwrap_or_default(),
                    command: command.clone(),
                    input_schema: map.get("input_schema").cloned(),
                });
            }
        }
        _ => {}
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn plugin_manager_new_is_empty() {
        let pm = PluginManager::new();
        assert!(pm.plugins().is_empty());
    }

    #[test]
    fn discover_returns_vec() {
        // Should not panic even if no plugin dirs exist
        let plugins = PluginManager::discover();
        // Result may be empty in test env -- that's fine
        let _ = plugins;
    }

    #[test]
    fn parse_minimal_manifest() {
        let toml = r#"
name = "test-plugin"
version = "1.0.0"
description = "A test plugin"
author = "test"
"#;
        let manifest = toml_parse(toml).expect("parse");
        assert_eq!(manifest.name, "test-plugin");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, "A test plugin");
    }

    #[test]
    fn parse_manifest_with_hooks_and_tools() {
        let toml = r#"
name = "my-plugin"
version = "0.2.0"

[[hooks]]
hook = "pre_message"
command = "echo pre"
required = "true"

[[hooks]]
hook = "on_startup"
command = "echo startup"

[[tools]]
name = "my_tool"
description = "Does something"
command = "python tool.py"
"#;
        let manifest = toml_parse(toml).expect("parse");
        assert_eq!(manifest.name, "my-plugin");
        assert_eq!(manifest.hooks.len(), 2);
        assert_eq!(manifest.hooks[0].hook, PluginHook::PreMessage);
        assert!(manifest.hooks[0].required);
        assert_eq!(manifest.hooks[1].hook, PluginHook::OnStartup);
        assert!(!manifest.hooks[1].required);
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "my_tool");
    }

    #[test]
    fn parse_manifest_missing_name_errors() {
        let toml = r#"
version = "1.0.0"
"#;
        assert!(toml_parse(toml).is_err());
    }

    #[test]
    fn discover_from_temp_dir() {
        let tmp = TempDir::new().expect("tmp");
        let plugin_dir = tmp.path().join("test-plugin");
        std::fs::create_dir_all(&plugin_dir).expect("mkdir");

        let manifest = r#"
name = "test-plugin"
version = "0.1.0"
description = "A temporary test plugin"
"#;
        std::fs::write(plugin_dir.join("plugin.toml"), manifest).expect("write");

        // Direct manifest loading test
        let m = PluginManager::load_manifest(&plugin_dir.join("plugin.toml")).expect("load");
        assert_eq!(m.name, "test-plugin");
    }

    #[test]
    fn plugin_source_display() {
        assert_eq!(PluginSource::User.to_string(), "user");
        assert_eq!(PluginSource::Project.to_string(), "project");
        assert_eq!(PluginSource::System.to_string(), "system");
    }

    #[test]
    fn hook_display() {
        assert_eq!(PluginHook::PreMessage.to_string(), "pre_message");
        assert_eq!(PluginHook::PostMessage.to_string(), "post_message");
        assert_eq!(PluginHook::PreTool.to_string(), "pre_tool");
        assert_eq!(PluginHook::PostTool.to_string(), "post_tool");
        assert_eq!(PluginHook::OnStartup.to_string(), "on_startup");
        assert_eq!(PluginHook::OnShutdown.to_string(), "on_shutdown");
    }

    #[test]
    fn all_tools_empty_on_new() {
        let pm = PluginManager::new();
        assert!(pm.all_tools().is_empty());
    }

    #[test]
    fn execute_hook_noop_when_empty() {
        let pm = PluginManager::new();
        let ctx = HookContext::default();
        // Should succeed -- no plugins, no hooks to run
        pm.execute_hook(PluginHook::OnStartup, &ctx)
            .expect("noop hook");
    }

    #[test]
    fn hook_context_defaults() {
        let ctx = HookContext::default();
        assert!(ctx.hook_name.is_empty());
        assert!(ctx.message.is_none());
        assert!(ctx.tool_name.is_none());
        assert!(ctx.tool_args.is_none());
        assert!(ctx.metadata.is_empty());
    }

    #[test]
    fn plugin_names_empty() {
        let pm = PluginManager::new();
        assert!(pm.plugin_names().is_empty());
    }

    #[test]
    fn load_unknown_plugin_errors() {
        let pm = PluginManager::new();
        assert!(pm.load("nonexistent_plugin").is_err());
    }
}
