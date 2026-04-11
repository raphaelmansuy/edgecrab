//! # profile — Profile management for EdgeCrab
//!
//! Mirrors `hermes profile` — each profile is an isolated home directory
//! under `~/.edgecrab/profiles/<name>/` containing its own:
//!
//! * `config.yaml` — per-profile model/toolset/platform settings
//! * `.env` — per-profile API keys
//! * `SOUL.md` — per-profile system prompt
//! * `memories/` — per-profile long-term memory
//! * `skills/` — per-profile skill library
//! * `state.db` — per-profile SQLite session store
//!
//! The **active** profile is persisted in `~/.edgecrab/.active_profile`.
//! Shell aliases live at `~/.local/bin/<profile_name>` and are thin
//! wrappers that forward all arguments to `edgecrab -p <name>`.
//!
//! ## SOLID compliance
//!
//! * `ProfileManager` owns all profile I/O — commands are thin callers.
//! * Every operation is `pub fn` so commands simply delegate.
//! * No business logic in `main.rs` — complex decisions live here.

use std::fs;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::bundled_profiles::{BundledProfileSyncReport, sync_bundled_profiles};
use anyhow::{Context, Result, bail};
use edgecrab_core::AppConfig;
use edgecrab_state::SessionDb;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use tar::{Archive, Builder, EntryType};

#[cfg(test)]
thread_local! {
    static TEST_EDGECRAB_HOME_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

const PROFILE_CLONE_CONFIG_FILES: &[&str] = &["config.yaml", ".env", "SOUL.md"];
const PROFILE_CLONE_SUBDIR_FILES: &[&str] = &["memories/MEMORY.md", "memories/USER.md"];
const PROFILE_RUNTIME_STRIP: &[&str] = &["gateway.pid", "gateway_state.json", "processes.json"];
const PROFILE_DEFAULT_EXPORT_EXCLUDE_ROOT: &[&str] = &[
    "profiles",
    ".env",
    "auth.json",
    "mcp-tokens",
    "gateway.pid",
    "gateway_state.json",
    "processes.json",
    "logs",
    "cache",
    "images",
    "sandboxes",
];
const RESERVED_ALIAS_NAMES: &[&str] = &["edgecrab", "default", "test", "tmp", "root", "sudo"];
const EDGECRAB_SUBCOMMANDS: &[&str] = &[
    "auth",
    "login",
    "logout",
    "setup",
    "doctor",
    "migrate",
    "acp",
    "version",
    "whatsapp",
    "status",
    "sessions",
    "session",
    "config",
    "tools",
    "mcp",
    "plugins",
    "cron",
    "gateway",
    "webhook",
    "skills",
    "profile",
    "completion",
    "uninstall",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSummary {
    pub name: String,
    pub home: PathBuf,
    pub is_default: bool,
    pub is_active: bool,
    pub model: String,
    pub has_env: bool,
    pub soul_present: bool,
    pub gateway_running: bool,
    pub skill_count: usize,
    pub plugin_count: usize,
    pub hook_count: usize,
    pub session_count: usize,
    pub enabled_mcp_servers: usize,
    pub honcho_enabled: bool,
    pub honcho_cloud_sync: bool,
    pub alias_path: Option<PathBuf>,
    pub disk_mb: u64,
}

impl ProfileSummary {
    pub fn state_label(&self) -> &'static str {
        if self.is_active {
            "active"
        } else if self.gateway_running {
            "running"
        } else {
            "ready"
        }
    }

    pub fn kind_label(&self) -> &'static str {
        if self.is_default { "default" } else { "named" }
    }

    pub fn honcho_label(&self) -> &'static str {
        if !self.honcho_enabled {
            "disabled"
        } else if self.honcho_cloud_sync {
            "enabled (cloud sync)"
        } else {
            "enabled (local-first)"
        }
    }

    pub fn list_detail(&self) -> String {
        format!(
            "{} | skills={} | sessions={} | env={} | gw={}",
            self.model,
            self.skill_count,
            self.session_count,
            if self.has_env { "yes" } else { "no" },
            if self.gateway_running { "on" } else { "off" }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileCreateReport {
    pub name: String,
    pub home: PathBuf,
    pub clone_source: Option<String>,
    pub clone_all: bool,
    pub alias_path: Option<PathBuf>,
    pub alias_warning: Option<String>,
}

impl ProfileCreateReport {
    pub fn render(&self) -> String {
        let mut lines = vec![format!(
            "Profile '{}' created at {}",
            self.name,
            self.home.display()
        )];

        if let Some(source) = &self.clone_source {
            let clone_mode = if self.clone_all {
                "Full copy"
            } else {
                "Cloned config, .env, SOUL.md, and core memory"
            };
            lines.push(format!("{clone_mode} from {source}."));
        } else {
            lines.push("Starter files created: config.yaml, SOUL.md, memories/USER.md, memories/MEMORY.md.".into());
        }

        match (&self.alias_path, &self.alias_warning) {
            (Some(path), _) => lines.push(format!("Alias ready: {}", path.display())),
            (None, Some(warning)) => lines.push(format!("Alias skipped: {warning}")),
            (None, None) => {}
        }

        lines.push(String::new());
        lines.push("Next steps:".into());
        lines.push(format!("  edgecrab -p {} config edit", self.name));
        lines.push(format!("  edgecrab -p {} \"your first prompt\"", self.name));
        lines.push(format!("  edgecrab profile use {}", self.name));
        lines.push(format!(
            "  edit {}/.env for profile-specific secrets",
            self.home.display()
        ));
        lines.push(format!(
            "  edit {}/SOUL.md for profile identity",
            self.home.display()
        ));

        lines.join("\n")
    }
}

// ─── Directories ──────────────────────────────────────────────────────────

fn default_edgecrab_home() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = TEST_EDGECRAB_HOME_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return path;
    }

    std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".edgecrab")
        })
}

#[cfg(test)]
pub fn set_test_edgecrab_home_override(path: Option<&Path>) {
    TEST_EDGECRAB_HOME_OVERRIDE.with(|slot| {
        *slot.borrow_mut() = path.map(Path::to_path_buf);
    });
}

fn normalize_root_home(path: PathBuf) -> PathBuf {
    let Some(parent) = path.parent() else {
        return path;
    };

    let in_profiles_dir = parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "profiles");

    if in_profiles_dir {
        parent
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.clone())
    } else {
        path
    }
}

/// Root directory for EdgeCrab user data.
pub fn edgecrab_home() -> PathBuf {
    normalize_root_home(default_edgecrab_home())
}

/// Directory that holds all named profiles.
fn profiles_root() -> PathBuf {
    edgecrab_home().join("profiles")
}

fn profiles_root_at(root_home: &Path) -> PathBuf {
    root_home.join("profiles")
}

/// Home directory for a specific named profile.
fn profile_home(name: &str) -> PathBuf {
    profiles_root().join(name)
}

fn profile_home_at(root_home: &Path, name: &str) -> PathBuf {
    profiles_root_at(root_home).join(name)
}

/// Path to the file that records the currently active profile name.
fn active_profile_file() -> PathBuf {
    edgecrab_home().join(".active_profile")
}

fn active_profile_file_at(root_home: &Path) -> PathBuf {
    root_home.join(".active_profile")
}

fn effective_home_for(name: &str) -> PathBuf {
    if name == "default" {
        edgecrab_home()
    } else {
        profile_home(name)
    }
}

fn read_active_profile_at(root_home: &Path) -> String {
    let path = active_profile_file_at(root_home);
    if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "default".into())
    } else {
        "default".into()
    }
}

pub fn activate_profile(name: Option<&str>) -> Result<String> {
    ensure_bundled_profiles_seeded()?;
    let manager = ProfileManager::new();
    let profile_name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| manager.active());

    if profile_name != "default" {
        let home = profile_home(&profile_name);
        if !home.exists() {
            bail!(
                "Profile '{}' does not exist. Create it with: edgecrab profile create {}",
                profile_name,
                profile_name
            );
        }
    }

    let effective_home = effective_home_for(&profile_name);
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("EDGECRAB_HOME", &effective_home);
    }

    Ok(profile_name)
}

pub fn ensure_bundled_profiles_seeded() -> Result<BundledProfileSyncReport> {
    sync_bundled_profiles(&edgecrab_home(), seed_profile_home)
}

// ─── ProfileManager ───────────────────────────────────────────────────────

/// Owns all profile management I/O operations.
///
/// Callers (CLI command handlers in `main.rs`) create an instance and call
/// the appropriate method — no profile logic leaks into `main.rs`.
pub struct ProfileManager;

impl ProfileManager {
    pub fn new() -> Self {
        Self
    }

    // ── active profile ────────────────────────────────────────────────

    /// Return the currently active profile name (defaults to "default").
    pub fn active(&self) -> String {
        read_active_profile_at(&edgecrab_home())
    }

    /// Persist the active profile name.
    fn set_active(&self, name: &str) -> Result<()> {
        let path = active_profile_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        if name == "default" {
            if path.exists() {
                fs::remove_file(&path)
                    .with_context(|| format!("Failed to remove {}", path.display()))?;
            }
            return Ok(());
        }

        let tmp_path = path.with_extension("tmp");
        fs::write(&tmp_path, format!("{name}\n"))
            .with_context(|| format!("Failed to write {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &path)
            .with_context(|| format!("Failed to move {} into place", path.display()))
    }

    // ── list ──────────────────────────────────────────────────────────

    /// Collect profile summaries for the default profile and all named profiles.
    pub fn summaries(&self) -> Result<Vec<ProfileSummary>> {
        let active = self.active();
        let mut names = vec!["default".to_string()];
        let root = profiles_root();
        if root.is_dir() {
            let mut entries: Vec<String> = fs::read_dir(&root)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| name != "default")
                .collect();
            entries.sort();
            names.extend(entries);
        }

        names
            .into_iter()
            .map(|name| self.summary_with_active(&name, &active))
            .collect()
    }

    /// Gather a single profile summary.
    pub fn summary(&self, name: &str) -> Result<ProfileSummary> {
        self.summary_with_active(name, &self.active())
    }

    pub fn render_list_table(&self) -> Result<String> {
        let summaries = self.summaries()?;
        let mut lines = vec![format!("Profiles ({})", summaries.len()), String::new()];
        for summary in summaries {
            let marker = if summary.is_active { "*" } else { " " };
            lines.push(format!(
                "{marker} {:<14} {:<7} {:<7} {}",
                summary.name,
                summary.kind_label(),
                summary.state_label(),
                summary.list_detail()
            ));
        }
        lines.push(String::new());
        lines.push(
            "Commands: /profile show <name>  /profile use <name>  /profile create <name>".into(),
        );
        Ok(lines.join("\n"))
    }

    pub fn render_active_status(&self) -> String {
        let active = self.active();
        format!(
            "Profile: {}\nHome:    {}",
            active,
            effective_home_for(&active).display()
        )
    }

    pub fn render_show(&self, name: &str) -> Result<String> {
        let summary = self.summary(name)?;
        let alias = summary
            .alias_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".into());
        let soul = if summary.soul_present {
            "present"
        } else {
            "missing (seeded on first run)"
        };

        Ok(format!(
            "Profile:    {}\nHome:       {}\nKind:       {}\nState:      {}\nModel:      {}\nMCP/ACP:    {} server(s) enabled\nHoncho:     {}\nSOUL:       {}\nEnv:        {}\nSkills:     {}\nPlugins:    {}\nHooks:      {}\nSessions:   {}\nAlias:      {}\nState DB:   {}\nDisk:       {} MB",
            summary.name,
            summary.home.display(),
            summary.kind_label(),
            summary.state_label(),
            summary.model,
            summary.enabled_mcp_servers,
            summary.honcho_label(),
            soul,
            if summary.has_env {
                "present"
            } else {
                "missing"
            },
            summary.skill_count,
            summary.plugin_count,
            summary.hook_count,
            summary.session_count,
            alias,
            summary.home.join("state.db").display(),
            summary.disk_mb
        ))
    }

    pub fn render_config(&self, name: &str) -> Result<String> {
        self.render_text_file(name, "config.yaml", "Config")
    }

    pub fn render_soul(&self, name: &str) -> Result<String> {
        self.render_text_file(name, "SOUL.md", "SOUL")
    }

    pub fn render_memory(&self, name: &str) -> Result<String> {
        let home = effective_home_for_existing(name)?;
        let sections = [
            ("USER", home.join("memories").join("USER.md")),
            ("MEMORY", home.join("memories").join("MEMORY.md")),
        ];

        let mut lines = vec![format!("Profile Memory: {name}")];
        let mut found_any = false;
        for (label, path) in sections {
            lines.push(String::new());
            lines.push(format!("[{label}] {}", path.display()));
            match fs::read_to_string(&path) {
                Ok(content) => {
                    found_any = true;
                    lines.push(content.trim_end().to_string());
                }
                Err(_) => lines.push("(missing)".into()),
            }
        }

        if !found_any {
            lines.push(String::new());
            lines.push("No memory files found for this profile.".into());
        }

        Ok(lines.join("\n"))
    }

    pub fn render_tools_report(&self, name: &str) -> Result<String> {
        let home = effective_home_for_existing(name)?;
        let config = read_profile_config(&home);
        let enabled_toolsets = config
            .tools
            .enabled_toolsets
            .clone()
            .unwrap_or_else(|| vec!["all".into()]);
        let disabled_toolsets = config
            .tools
            .disabled_toolsets
            .clone()
            .unwrap_or_else(|| vec!["none".into()]);
        let enabled_tools = config
            .tools
            .enabled_tools
            .clone()
            .unwrap_or_else(|| vec!["all".into()]);
        let disabled_tools = config
            .tools
            .disabled_tools
            .clone()
            .unwrap_or_else(|| vec!["none".into()]);

        Ok(format!(
            "Profile Tools: {name}\nHome:             {}\nModel:            {}\nReasoning:        {}\nEnabled toolsets: {}\nDisabled toolsets:{}\nEnabled tools:    {}\nDisabled tools:   {}\nParallel exec:    {}\nMax workers:      {}\nLSP:              {}\nPlugins:          {}\nMCP servers:      {} enabled",
            home.display(),
            config.model.default_model,
            config.reasoning_effort.unwrap_or_else(|| "default".into()),
            enabled_toolsets.join(", "),
            if disabled_toolsets.is_empty() {
                " none".into()
            } else {
                format!(" {}", disabled_toolsets.join(", "))
            },
            enabled_tools.join(", "),
            disabled_tools.join(", "),
            if config.tools.parallel_execution {
                "on"
            } else {
                "off"
            },
            config.tools.max_parallel_workers,
            if config.lsp.enabled {
                "enabled"
            } else {
                "disabled"
            },
            if config.plugins.enabled {
                "enabled"
            } else {
                "disabled"
            },
            config
                .mcp_servers
                .values()
                .filter(|server| server.enabled)
                .count()
        ))
    }

    fn summary_with_active(&self, name: &str, active: &str) -> Result<ProfileSummary> {
        let home = effective_home_for_existing(name)?;
        let config = read_profile_config(&home);

        Ok(ProfileSummary {
            name: name.to_string(),
            home: home.clone(),
            is_default: name == "default",
            is_active: name == active,
            model: config.model.default_model.clone(),
            has_env: home.join(".env").exists(),
            soul_present: home.join("SOUL.md").exists(),
            gateway_running: read_gateway_pid(&home).is_some_and(process_exists),
            skill_count: count_skills(&home.join("skills")),
            plugin_count: count_entries(&home.join("plugins")),
            hook_count: count_entries(&home.join("hooks")),
            session_count: count_sessions(&home.join("state.db")),
            enabled_mcp_servers: config
                .mcp_servers
                .values()
                .filter(|server| server.enabled)
                .count(),
            honcho_enabled: config.honcho.enabled,
            honcho_cloud_sync: config.honcho.cloud_sync,
            alias_path: summary_alias_path(name),
            disk_mb: dir_size_mb(&home),
        })
    }

    fn render_text_file(&self, name: &str, relative_path: &str, title: &str) -> Result<String> {
        let home = effective_home_for_existing(name)?;
        let path = home.join(relative_path);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Profile '{name}' has no readable {}", path.display()))?;
        Ok(format!(
            "{title}: {name}\nPath: {}\n\n{}",
            path.display(),
            content.trim_end()
        ))
    }

    /// Print all known profiles. The active one is marked with `*`.
    pub fn list(&self) -> Result<()> {
        if std::io::stdout().is_terminal() {
            println!("{}", self.render_list_table()?);
            return Ok(());
        }

        let active = self.active();
        let root = profiles_root();

        // Always include "default".
        let mut names: Vec<String> = vec!["default".into()];

        if root.exists() {
            let mut entries: Vec<String> = fs::read_dir(&root)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n != "default")
                .collect();
            entries.sort();
            names.extend(entries);
        }

        for name in &names {
            let marker = if name == &active { "* " } else { "  " };
            println!("{marker}{name}");
        }
        Ok(())
    }

    // ── use ───────────────────────────────────────────────────────────

    /// Set `name` as the active profile.
    pub fn use_profile(&self, name: &str) -> Result<()> {
        self.set_active_profile(name)?;
        println!("Active profile set to: {name}");
        Ok(())
    }

    pub fn set_active_profile(&self, name: &str) -> Result<()> {
        if name != "default" {
            let home = profile_home(name);
            if !home.exists() {
                bail!(
                    "Profile '{}' does not exist. Create it with: edgecrab profile create {}",
                    name,
                    name
                );
            }
        }
        self.set_active(name)?;
        Ok(())
    }

    // ── create ────────────────────────────────────────────────────────

    /// Create a new profile, optionally cloning from an existing one.
    pub fn create(
        &self,
        name: &str,
        clone: bool,
        clone_all: bool,
        clone_from: Option<&str>,
    ) -> Result<()> {
        let report = self.create_report(name, clone, clone_all, clone_from)?;
        println!("{}", report.render());
        Ok(())
    }

    pub fn create_report(
        &self,
        name: &str,
        clone: bool,
        clone_all: bool,
        clone_from: Option<&str>,
    ) -> Result<ProfileCreateReport> {
        validate_name(name)?;

        if name == "default" {
            bail!("'default' is the built-in profile and cannot be created explicitly.");
        }

        let dest = profile_home(name);
        if dest.exists() {
            bail!("Profile '{}' already exists.", name);
        }
        fs::create_dir_all(&dest).with_context(|| format!("Cannot create {}", dest.display()))?;
        seed_profile_home(&dest)?;

        let mut source_name = None;
        if clone || clone_all {
            let active = self.active();
            let source = clone_from.unwrap_or(active.as_str());
            let source_home = effective_home_for(source);
            source_name = Some(source.to_string());

            // Files always copied (--clone and --clone-all)
            for f in PROFILE_CLONE_CONFIG_FILES {
                let src = source_home.join(f);
                if src.exists() {
                    fs::copy(&src, dest.join(f)).with_context(|| format!("Copying {f}"))?;
                }
            }

            for rel in PROFILE_CLONE_SUBDIR_FILES {
                let src = source_home.join(rel);
                if !src.exists() {
                    continue;
                }
                let dst = dest.join(rel);
                if let Some(parent) = dst.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("Creating {}", parent.display()))?;
                }
                fs::copy(&src, &dst).with_context(|| format!("Copying {}", src.display()))?;
            }

            if clone_all {
                copy_profile_contents(&source_home, &dest)?;
            }
        }

        let alias_warning = self.write_alias(name, name)?;
        let alias_path = alias_warning.is_none().then(|| alias_bin_path(name));

        Ok(ProfileCreateReport {
            name: name.to_string(),
            home: dest,
            clone_source: source_name,
            clone_all,
            alias_path,
            alias_warning,
        })
    }

    // ── delete ────────────────────────────────────────────────────────

    /// Delete a profile after optional confirmation.
    pub fn delete(&self, name: &str, yes: bool) -> Result<()> {
        let root_home = edgecrab_home();
        if name == "default" {
            bail!("Cannot delete the 'default' profile.");
        }
        if name == read_active_profile_at(&root_home) {
            bail!(
                "Cannot delete the currently active profile '{}'. \
                 Switch to another profile first: edgecrab profile use default",
                name
            );
        }

        let home = profile_home_at(&root_home, name);
        if !home.exists() {
            bail!("Profile '{}' does not exist.", name);
        }

        if !yes {
            print!(
                "Delete profile '{}' at {}? This cannot be undone. [y/N] ",
                name,
                home.display()
            );
            std::io::stdout().flush().ok();
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer).ok();
            if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
                println!("Aborted.");
                return Ok(());
            }
        }

        if let Some(pid) = read_gateway_pid(&home) {
            stop_gateway_process(pid);
        }

        // Deleting the directory that contains the current working directory can
        // fail on macOS. Step back to the profile root first and retry briefly
        // in case another concurrent test moved the process cwd into the
        // profile between attempts.
        let mut last_error = None;
        for _ in 0..5 {
            if let Ok(cwd) = std::env::current_dir()
                && cwd.starts_with(&home)
            {
                let _ = std::env::set_current_dir(&root_home);
            }

            match fs::remove_dir_all(&home) {
                Ok(()) => {
                    last_error = None;
                    break;
                }
                Err(err) if !home.exists() => {
                    last_error = None;
                    break;
                }
                Err(err) => {
                    last_error = Some(err);
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        }

        if let Some(err) = last_error {
            Err(err).with_context(|| format!("Failed to remove {}", home.display()))?;
        }

        // Best-effort alias removal
        let alias_path = alias_bin_path(name);
        let _ = fs::remove_file(alias_path);

        println!("Profile '{}' deleted.", name);
        Ok(())
    }

    // ── show ──────────────────────────────────────────────────────────

    /// Display profile details.
    pub fn show(&self, name: Option<&str>) -> Result<()> {
        if let Some(name) = name {
            println!("{}", self.render_show(name)?);
        } else {
            println!("{}", self.render_active_status());
        }
        Ok(())
    }

    // ── alias ─────────────────────────────────────────────────────────

    /// Regenerate or remove the shell alias wrapper script.
    pub fn alias(&self, name: &str, remove: bool, alias_name: Option<&str>) -> Result<()> {
        if name != "default" {
            let home = profile_home(name);
            if !home.exists() {
                bail!("Profile '{}' does not exist.", name);
            }
        }

        let script_name = alias_name.unwrap_or(name);
        let path = alias_bin_path(script_name);

        if remove {
            if path.exists() {
                fs::remove_file(&path)
                    .with_context(|| format!("Cannot remove {}", path.display()))?;
                println!("Removed alias: {}", path.display());
            } else {
                println!("Alias not found: {}", path.display());
            }
            return Ok(());
        }

        if let Some(warning) = self.write_alias(name, script_name)? {
            println!("Alias skipped: {warning}");
            return Ok(());
        }
        println!("Alias written: {}", path.display());
        Ok(())
    }

    // ── rename ────────────────────────────────────────────────────────

    /// Rename a profile, moving its directory and updating the alias.
    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<()> {
        if old_name == "default" {
            bail!("Cannot rename the 'default' profile.");
        }
        validate_name(new_name)?;

        let src = profile_home(old_name);
        if !src.exists() {
            bail!("Profile '{}' does not exist.", old_name);
        }
        let dst = profile_home(new_name);
        if dst.exists() {
            bail!("Profile '{}' already exists.", new_name);
        }

        fs::rename(&src, &dst)
            .with_context(|| format!("Renaming {} → {}", src.display(), dst.display()))?;

        if let Some(pid) = read_gateway_pid(&dst) {
            stop_gateway_process(pid);
        }

        // Update active profile if it pointed to old_name
        if self.active() == old_name {
            self.set_active(new_name)?;
        }

        // Move the alias
        let old_alias = alias_bin_path(old_name);
        if old_alias.exists() {
            let _ = fs::remove_file(&old_alias);
        }
        if let Some(warning) = self.write_alias(new_name, new_name)? {
            println!("Profile '{old_name}' renamed to '{new_name}'.");
            println!("Alias skipped: {warning}");
            return Ok(());
        }

        println!("Profile '{old_name}' renamed to '{new_name}'.");
        Ok(())
    }

    // ── export ────────────────────────────────────────────────────────

    /// Export a profile to a `.tar.gz` archive.
    pub fn export(&self, name: &str, output: Option<&str>) -> Result<()> {
        let home = effective_home_for_existing(name)?;
        let out_path = output
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("{name}.tar.gz")));

        create_tar_gz(name, &home, &out_path)
            .with_context(|| format!("Exporting profile '{name}'"))?;

        println!("Profile '{}' exported to {}", name, out_path.display());
        Ok(())
    }

    // ── import ────────────────────────────────────────────────────────

    /// Import a profile from a `.tar.gz` archive.
    pub fn import(&self, archive: &str, name: Option<&str>) -> Result<()> {
        let archive_path = PathBuf::from(archive);
        if !archive_path.exists() {
            bail!("Archive not found: {}", archive_path.display());
        }

        let top_level = archive_root_name(&archive_path)?;
        let profile_name = name
            .map(|s| s.to_string())
            .or(top_level)
            .unwrap_or_else(|| infer_name_from_archive(&archive_path));

        validate_name(&profile_name)?;
        if profile_name == "default" {
            bail!(
                "Cannot import as 'default'. Use --name to import into a named profile directory."
            );
        }

        let dest = profile_home(&profile_name);
        if dest.exists() {
            bail!(
                "Profile '{}' already exists. Delete it first or pass --name to choose a different name.",
                profile_name
            );
        }
        fs::create_dir_all(&dest)?;

        extract_tar_gz(&archive_path, &dest)
            .with_context(|| format!("Importing archive '{archive}'"))?;

        if let Some(warning) = self.write_alias(&profile_name, &profile_name)? {
            println!("Profile '{}' imported to {}", profile_name, dest.display());
            println!("Alias skipped: {warning}");
            return Ok(());
        }
        println!("Profile '{}' imported to {}", profile_name, dest.display());
        Ok(())
    }

    // ── helpers ───────────────────────────────────────────────────────

    /// Write a shell wrapper script to `~/.local/bin/<alias_name>`.
    fn write_alias(&self, profile_name: &str, alias_name: &str) -> Result<Option<String>> {
        if let Some(collision) = check_alias_collision(alias_name) {
            return Ok(Some(collision));
        }

        let bin_dir = alias_bin_dir();
        if let Err(e) = fs::create_dir_all(&bin_dir) {
            // Non-fatal: some systems don't have ~/.local/bin
            tracing::debug!(error = %e, "Could not create alias bin dir");
            return Ok(Some(format!("cannot create {} ({e})", bin_dir.display())));
        }

        let script = format!("#!/bin/sh\nexec edgecrab --profile '{profile_name}' \"$@\"\n");
        let path = bin_dir.join(alias_name);
        fs::write(&path, &script).with_context(|| format!("Writing alias {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms)?;
        }
        Ok(None)
    }
}

// ─── Shell completion ─────────────────────────────────────────────────────

/// Print shell completion script for the requested shell.
pub fn print_completion(shell: &str) -> Result<()> {
    match shell {
        "bash" => {
            print!("{}", BASH_COMPLETION);
            Ok(())
        }
        "zsh" => {
            print!("{}", ZSH_COMPLETION);
            Ok(())
        }
        other => bail!("Unsupported shell: '{}'. Supported: bash, zsh", other),
    }
}

const BASH_COMPLETION: &str = r#"# EdgeCrab bash completion
_edgecrab_completions() {
    local cur prev words cword
    _init_completion || return

    local subcommands="setup doctor migrate claw acp version whatsapp status sessions config tools mcp plugins cron gateway skills profile completion"
    local profile_cmds="list use create delete show alias rename export import"

    case "${prev}" in
        edgecrab)
            COMPREPLY=($(compgen -W "${subcommands}" -- "${cur}"))
            return ;;
        profile)
            COMPREPLY=($(compgen -W "${profile_cmds}" -- "${cur}"))
            return ;;
        use|delete|show|alias|rename|export)
            # complete profile names
            local profiles
            profiles=$(edgecrab profile list 2>/dev/null | sed 's/^[* ] //')
            COMPREPLY=($(compgen -W "${profiles}" -- "${cur}"))
            return ;;
        -p|--profile)
            local profiles
            profiles=$(edgecrab profile list 2>/dev/null | sed 's/^[* ] //')
            COMPREPLY=($(compgen -W "${profiles}" -- "${cur}"))
            return ;;
        completion)
            COMPREPLY=($(compgen -W "bash zsh" -- "${cur}"))
            return ;;
    esac
}
complete -F _edgecrab_completions edgecrab
"#;

const ZSH_COMPLETION: &str = r#"#compdef edgecrab
# EdgeCrab zsh completion
_edgecrab() {
    local -a subcommands profile_cmds
    subcommands=(
        'setup:Interactive first-run wizard'
        'doctor:Run diagnostics'
        'migrate:Migrate from hermes-agent'
        'claw:Migrate from OpenClaw'
        'acp:ACP stdio server for editor integration'
        'version:Show version info'
        'whatsapp:Configure WhatsApp bridge'
        'status:Show runtime status'
        'sessions:Session management'
        'config:Inspect or modify config'
        'tools:Inspect tools and toolsets'
        'mcp:Manage MCP servers'
        'plugins:Manage plugins'
        'cron:Manage scheduled tasks'
        'gateway:Run messaging gateway'
        'skills:Manage skills'
        'profile:Manage profiles'
        'completion:Generate shell completions'
    )
    profile_cmds=(
        'list:List all profiles'
        'use:Set the active profile'
        'create:Create a new profile'
        'delete:Delete a profile'
        'show:Show profile details'
        'alias:Manage shell aliases'
        'rename:Rename a profile'
        'export:Export profile to tar.gz'
        'import:Import profile from tar.gz'
    )

    _arguments \
        '(-p --profile)'{-p,--profile}'[Profile to use]:profile:_edgecrab_profiles' \
        '1: :->subcommand' \
        '*: :->args'

    case $state in
        subcommand) _describe 'subcommand' subcommands ;;
        args)
            case $words[2] in
                profile)
                    case $words[3] in
                        use|delete|show|rename|export)
                            _edgecrab_profiles ;;
                        *)
                            _describe 'profile subcommand' profile_cmds ;;
                    esac ;;
                completion)
                    _values 'shell' 'bash' 'zsh' ;;
            esac ;;
    esac
}

_edgecrab_profiles() {
    local -a profiles
    profiles=(${(f)"$(edgecrab profile list 2>/dev/null | sed 's/^[* ] //')"})
    _describe 'profile' profiles
}

_edgecrab
"#;

// ─── Private helpers ──────────────────────────────────────────────────────

/// Validate a profile name: lowercase ASCII, digits, hyphens, underscores.
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Profile name cannot be empty.");
    }
    if name.len() > 64 {
        bail!("Profile name '{name}' is too long. Use at most 64 characters.");
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        bail!("Profile name cannot be empty.");
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        bail!(
            "Invalid profile name '{}'. Start with a lowercase letter or digit.",
            name
        );
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_') {
        bail!(
            "Invalid profile name '{}'. Use lowercase letters, digits, hyphens, or underscores.",
            name
        );
    }
    Ok(())
}

/// Path to the `~/.local/bin/<name>` alias wrapper.
fn alias_bin_path(name: &str) -> PathBuf {
    alias_bin_dir().join(name)
}

fn alias_bin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".local")
        .join("bin")
}

fn check_alias_collision(name: &str) -> Option<String> {
    if RESERVED_ALIAS_NAMES.contains(&name) {
        return Some(format!("'{name}' is reserved"));
    }
    if EDGECRAB_SUBCOMMANDS.contains(&name) {
        return Some(format!("'{name}' conflicts with an edgecrab subcommand"));
    }
    if let Ok(existing) = which::which(name) {
        let expected_wrapper = alias_bin_path(name);
        if existing == expected_wrapper
            && existing.is_file()
            && fs::read_to_string(&existing)
                .ok()
                .is_some_and(|text| text.contains("edgecrab --profile"))
        {
            return None;
        }
        return Some(format!(
            "'{name}' conflicts with an existing command ({})",
            existing.display()
        ));
    }
    None
}

/// Approximate directory size in MB (best-effort).
fn dir_size_mb(path: &Path) -> u64 {
    walk_bytes(path) / 1_000_000
}

fn walk_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = fs::read_dir(path) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_file() {
                total += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            } else if p.is_dir() {
                total += walk_bytes(&p);
            }
        }
    }
    total
}

fn count_entries(path: &Path) -> usize {
    path.read_dir()
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .is_none_or(|name| !name.starts_with('.'))
                })
                .count()
        })
        .unwrap_or(0)
}

fn count_skills(path: &Path) -> usize {
    let mut total = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(current) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let entry_path = entry.path();
            if entry_path.is_dir() {
                if entry_path.join("SKILL.md").is_file() {
                    total += 1;
                } else {
                    stack.push(entry_path);
                }
            } else if entry_path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            {
                total += 1;
            }
        }
    }
    total
}

fn count_sessions(state_db_path: &Path) -> usize {
    SessionDb::open(state_db_path)
        .ok()
        .and_then(|db| {
            db.list_sessions(100_000)
                .ok()
                .map(|sessions| sessions.len())
        })
        .unwrap_or(0)
}

fn effective_home_for_existing(name: &str) -> Result<PathBuf> {
    if name == "default" {
        Ok(edgecrab_home())
    } else {
        let home = profile_home(name);
        if !home.exists() {
            bail!("Profile '{}' does not exist.", name);
        }
        Ok(home)
    }
}

fn read_profile_config(home: &Path) -> AppConfig {
    let config_path = home.join("config.yaml");
    if config_path.is_file() {
        AppConfig::load_from(&config_path).unwrap_or_default()
    } else {
        AppConfig::default()
    }
}

fn read_gateway_pid(home: &Path) -> Option<u32> {
    fs::read_to_string(home.join("gateway.pid"))
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(pid) {
            return unsafe { libc::kill(pid, 0) == 0 };
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
    false
}

fn stop_gateway_process(pid: u32) {
    #[cfg(unix)]
    {
        if let Ok(raw_pid) = i32::try_from(pid) {
            unsafe {
                libc::kill(raw_pid, libc::SIGTERM);
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if !process_exists(pid) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            unsafe {
                libc::kill(raw_pid, libc::SIGKILL);
            }
        }
    }
}

fn summary_alias_path(name: &str) -> Option<PathBuf> {
    let path = alias_bin_path(name);
    path.exists().then_some(path)
}

pub(crate) fn seed_profile_home(dest: &Path) -> Result<()> {
    for dir in [
        "memories",
        "skills",
        "skins",
        "plugins",
        "mcp",
        "cache",
        "cron",
        "sessions",
        "logs",
        "sandboxes",
        "hooks",
        "checkpoints",
        "images",
        "mcp-tokens",
        "honcho",
        "pairing",
    ] {
        fs::create_dir_all(dest.join(dir))
            .with_context(|| format!("Creating {}", dest.join(dir).display()))?;
    }

    let config_path = dest.join("config.yaml");
    if !config_path.exists() {
        AppConfig::default()
            .save_to(&config_path)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("Writing {}", config_path.display()))?;
    }

    Ok(())
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)?.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to).with_context(|| format!("Copying {}", from.display()))?;
        }
    }
    Ok(())
}

fn copy_profile_contents(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src)?.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if matches!(name_str, ".active_profile" | "profiles")
            || PROFILE_RUNTIME_STRIP.contains(&name_str)
        {
            continue;
        }

        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to).with_context(|| format!("Copying {}", from.display()))?;
        }
    }
    Ok(())
}

/// Infer a profile name from an archive path (strip `.tar.gz` suffix).
fn infer_name_from_archive(path: &Path) -> String {
    let stem = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("profile");
    stem.trim_end_matches(".tar.gz")
        .trim_end_matches(".tgz")
        .to_string()
}

fn archive_root_name(archive: &Path) -> Result<Option<String>> {
    let file = fs::File::open(archive)
        .with_context(|| format!("Opening archive {}", archive.display()))?;
    let decoder = GzDecoder::new(file);
    let mut tar = Archive::new(decoder);
    let mut top_level: Option<String> = None;

    for entry in tar.entries()? {
        let entry = entry?;
        let path = entry.path()?;
        let Some(first) = path.components().next() else {
            continue;
        };
        let name = first.as_os_str().to_string_lossy().into_owned();
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        match &top_level {
            None => top_level = Some(name),
            Some(existing) if existing == &name => {}
            Some(_) => bail!(
                "Archive '{}' contains multiple top-level directories; import requires a single profile root.",
                archive.display()
            ),
        }
    }

    Ok(top_level)
}

fn should_skip_export_path(profile_name: &str, relative: &Path, is_dir: bool) -> bool {
    let first = relative
        .components()
        .next()
        .map(|component| component.as_os_str().to_string_lossy().into_owned());
    let Some(first) = first else {
        return false;
    };

    if matches!(first.as_str(), ".env" | "auth.json" | "mcp-tokens") {
        return true;
    }
    if profile_name == "default" && PROFILE_DEFAULT_EXPORT_EXCLUDE_ROOT.contains(&first.as_str()) {
        return true;
    }
    if !is_dir && relative.file_name().is_some_and(|name| name == ".DS_Store") {
        return true;
    }
    false
}

fn append_dir_entries(
    builder: &mut Builder<GzEncoder<fs::File>>,
    profile_name: &str,
    base: &Path,
    current: &Path,
) -> Result<()> {
    let rel = current
        .strip_prefix(base)
        .with_context(|| format!("computing export path for {}", current.display()))?;

    if !rel.as_os_str().is_empty() {
        if should_skip_export_path(profile_name, rel, true) {
            return Ok(());
        }
        let archive_path = Path::new(profile_name).join(rel);
        builder.append_dir(&archive_path, current)?;
    }

    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path
            .strip_prefix(base)
            .with_context(|| format!("computing export path for {}", path.display()))?;
        if path.is_dir() {
            if should_skip_export_path(profile_name, rel, true) {
                continue;
            }
            append_dir_entries(builder, profile_name, base, &path)?;
        } else {
            if should_skip_export_path(profile_name, rel, false) {
                continue;
            }
            let archive_path = Path::new(profile_name).join(rel);
            builder
                .append_path_with_name(&path, &archive_path)
                .with_context(|| format!("Adding {}", path.display()))?;
        }
    }

    Ok(())
}

/// Create a tar.gz archive of `src_dir` at `out_path`.
fn create_tar_gz(profile_name: &str, src_dir: &Path, out_path: &Path) -> Result<()> {
    let file = fs::File::create(out_path)
        .with_context(|| format!("Creating archive {}", out_path.display()))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);
    append_dir_entries(&mut builder, profile_name, src_dir, src_dir)?;
    builder.finish()?;
    Ok(())
}

fn archive_entry_target(dest_dir: &Path, entry_path: &Path, root_name: &str) -> Result<PathBuf> {
    let mut components = entry_path.components();
    let Some(first) = components.next() else {
        bail!("Archive contains an empty entry path.");
    };
    let first_name = first.as_os_str().to_string_lossy();
    if first_name != root_name {
        bail!(
            "Archive root '{}' does not match expected '{}'.",
            first_name,
            root_name
        );
    }

    let mut rel = PathBuf::new();
    for component in components {
        match component {
            std::path::Component::Normal(part) => rel.push(part),
            _ => bail!("Archive contains an unsafe path: {}", entry_path.display()),
        }
    }
    Ok(dest_dir.join(rel))
}

/// Extract a tar.gz archive into `dest_dir`.
fn extract_tar_gz(archive: &Path, dest_dir: &Path) -> Result<()> {
    let root_name = archive_root_name(archive)?
        .ok_or_else(|| anyhow::anyhow!("Archive '{}' is empty.", archive.display()))?;
    let file = fs::File::open(archive)
        .with_context(|| format!("Opening archive {}", archive.display()))?;
    let decoder = GzDecoder::new(file);
    let mut tar = Archive::new(decoder);

    for entry in tar.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?.into_owned();
        let target = archive_entry_target(dest_dir, &entry_path, &root_name)?;

        match entry.header().entry_type() {
            EntryType::Directory => {
                fs::create_dir_all(&target)
                    .with_context(|| format!("Creating {}", target.display()))?;
            }
            EntryType::Regular => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("Creating {}", parent.display()))?;
                }
                let mut output = fs::File::create(&target)
                    .with_context(|| format!("Creating {}", target.display()))?;
                let mut buffer = Vec::new();
                entry.read_to_end(&mut buffer)?;
                output.write_all(&buffer)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = entry.header().mode().unwrap_or(0o644);
                    fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777))?;
                }
            }
            other => bail!(
                "Archive member '{}' has unsupported type {:?}.",
                entry_path.display(),
                other
            ),
        }
    }

    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_ok() {
        assert!(validate_name("work").is_ok());
        assert!(validate_name("my-bot").is_ok());
        assert!(validate_name("dev_42").is_ok());
    }

    #[test]
    fn validate_name_empty() {
        assert!(validate_name("").is_err());
    }

    #[test]
    fn validate_name_spaces() {
        assert!(validate_name("my bot").is_err());
    }

    #[test]
    fn validate_name_slash() {
        assert!(validate_name("a/b").is_err());
    }

    #[test]
    fn infer_name_from_tar_gz() {
        assert_eq!(
            infer_name_from_archive(Path::new("work-2026-04-01.tar.gz")),
            "work-2026-04-01"
        );
        assert_eq!(infer_name_from_archive(Path::new("backup.tgz")), "backup");
    }

    #[test]
    fn normalize_root_home_strips_profile_leaf() {
        let root = normalize_root_home(PathBuf::from("/tmp/.edgecrab/profiles/work"));
        assert_eq!(root, PathBuf::from("/tmp/.edgecrab"));
    }

    #[test]
    fn normalize_root_home_keeps_plain_home() {
        let root = normalize_root_home(PathBuf::from("/tmp/.edgecrab"));
        assert_eq!(root, PathBuf::from("/tmp/.edgecrab"));
    }

    #[test]
    fn create_report_render_for_starter_profile_includes_next_steps() {
        let report = ProfileCreateReport {
            name: "work".into(),
            home: PathBuf::from("/tmp/.edgecrab/profiles/work"),
            clone_source: None,
            clone_all: false,
            alias_path: Some(PathBuf::from("/tmp/.local/bin/work")),
            alias_warning: None,
        };

        let rendered = report.render();
        assert!(rendered.contains("Profile 'work' created at /tmp/.edgecrab/profiles/work"));
        assert!(rendered.contains("Starter files created"));
        assert!(rendered.contains("edgecrab -p work config edit"));
        assert!(rendered.contains("Alias ready: /tmp/.local/bin/work"));
    }

    #[test]
    fn create_report_render_for_clone_all_mentions_full_copy() {
        let report = ProfileCreateReport {
            name: "audit".into(),
            home: PathBuf::from("/tmp/.edgecrab/profiles/audit"),
            clone_source: Some("work".into()),
            clone_all: true,
            alias_path: None,
            alias_warning: Some("collision".into()),
        };

        let rendered = report.render();
        assert!(rendered.contains("Full copy from work."));
        assert!(rendered.contains("Alias skipped: collision"));
    }
}
