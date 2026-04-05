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
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use edgecrab_core::AppConfig;
use edgecrab_state::SessionDb;

// ─── Directories ──────────────────────────────────────────────────────────

fn default_edgecrab_home() -> PathBuf {
    std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".edgecrab")
        })
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

/// Home directory for a specific named profile.
fn profile_home(name: &str) -> PathBuf {
    profiles_root().join(name)
}

/// Path to the file that records the currently active profile name.
fn active_profile_file() -> PathBuf {
    edgecrab_home().join(".active_profile")
}

fn effective_home_for(name: &str) -> PathBuf {
    if name == "default" {
        edgecrab_home()
    } else {
        profile_home(name)
    }
}

pub fn activate_profile(name: Option<&str>) -> Result<String> {
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
        let path = active_profile_file();
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

    /// Persist the active profile name.
    fn set_active(&self, name: &str) -> Result<()> {
        let path = active_profile_file();
        fs::write(&path, format!("{name}\n"))
            .with_context(|| format!("Failed to write {}", path.display()))
    }

    // ── list ──────────────────────────────────────────────────────────

    /// Print all known profiles. The active one is marked with `*`.
    pub fn list(&self) -> Result<()> {
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
        println!("Active profile set to: {name}");
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

        if clone || clone_all {
            let active = self.active();
            let source_name = clone_from.unwrap_or(active.as_str());
            let source = effective_home_for(source_name);

            // Files always copied (--clone and --clone-all)
            let config_files = ["config.yaml", ".env", "SOUL.md"];
            for f in &config_files {
                let src = source.join(f);
                if src.exists() {
                    fs::copy(&src, dest.join(f)).with_context(|| format!("Copying {f}"))?;
                }
            }

            if clone_all {
                copy_profile_contents(&source, &dest)?;
            }
        }

        self.write_alias(name, name)?;
        println!("Profile '{}' created at {}", name, dest.display());
        Ok(())
    }

    // ── delete ────────────────────────────────────────────────────────

    /// Delete a profile after optional confirmation.
    pub fn delete(&self, name: &str, yes: bool) -> Result<()> {
        if name == "default" {
            bail!("Cannot delete the 'default' profile.");
        }
        if name == self.active() {
            bail!(
                "Cannot delete the currently active profile '{}'. \
                 Switch to another profile first: edgecrab profile use default",
                name
            );
        }

        let home = profile_home(name);
        if !home.exists() {
            bail!("Profile '{}' does not exist.", name);
        }

        if !yes {
            print!(
                "Delete profile '{}' at {}? This cannot be undone. [y/N] ",
                name,
                home.display()
            );
            use std::io::{Write, stdin, stdout};
            stdout().flush().ok();
            let mut answer = String::new();
            stdin().read_line(&mut answer).ok();
            if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
                println!("Aborted.");
                return Ok(());
            }
        }

        fs::remove_dir_all(&home)
            .with_context(|| format!("Failed to remove {}", home.display()))?;

        // Best-effort alias removal
        let alias_path = alias_bin_path(name);
        let _ = fs::remove_file(alias_path);

        println!("Profile '{}' deleted.", name);
        Ok(())
    }

    // ── show ──────────────────────────────────────────────────────────

    /// Display profile details.
    pub fn show(&self, name: &str) -> Result<()> {
        let home = effective_home_for_existing(name)?;
        let config_path = home.join("config.yaml");
        let config = if config_path.is_file() {
            AppConfig::load_from(&config_path).unwrap_or_default()
        } else {
            AppConfig::default()
        };
        let skills_count = count_entries(&home.join("skills"));
        let plugins_count = count_entries(&home.join("plugins"));
        let hooks_count = count_entries(&home.join("hooks"));
        let sessions_count = count_sessions(&home.join("state.db"));
        let toolsets = config
            .tools
            .enabled_toolsets
            .clone()
            .unwrap_or_else(|| vec!["all".into()]);
        let enabled_mcp = config
            .mcp_servers
            .values()
            .filter(|server| server.enabled)
            .count();
        let honcho_state = if config.honcho.enabled {
            if config.honcho.cloud_sync {
                "enabled (cloud sync)"
            } else {
                "enabled (local-first)"
            }
        } else {
            "disabled"
        };
        let soul_state = if home.join("SOUL.md").exists() {
            "present"
        } else {
            "missing (seeded on first run)"
        };

        let disk_mb = dir_size_mb(&home);

        println!("Profile:    {name}");
        println!("Home:       {}", home.display());
        println!("Model:      {}", config.model.default_model);
        println!("Toolsets:   {}", toolsets.join(", "));
        println!("MCP/ACP:    {enabled_mcp} server(s) configured");
        println!("Honcho:     {honcho_state}");
        println!("SOUL:       {soul_state}");
        println!("Skills:     {skills_count} installed");
        println!("Plugins:    {plugins_count} installed");
        println!("Hooks:      {hooks_count} loaded");
        println!("Sessions:   {sessions_count} persisted");
        println!("State DB:   {}", home.join("state.db").display());
        println!("Disk:       {disk_mb} MB");
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

        self.write_alias(name, script_name)?;
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

        // Update active profile if it pointed to old_name
        if self.active() == old_name {
            self.set_active(new_name)?;
        }

        // Move the alias
        let old_alias = alias_bin_path(old_name);
        if old_alias.exists() {
            let _ = fs::remove_file(&old_alias);
        }
        self.write_alias(new_name, new_name)?;

        println!("Profile '{old_name}' renamed to '{new_name}'.");
        Ok(())
    }

    // ── export ────────────────────────────────────────────────────────

    /// Export a profile to a `.tar.gz` archive.
    pub fn export(&self, name: &str, output: Option<&str>) -> Result<()> {
        let home = if name == "default" {
            edgecrab_home()
        } else {
            let h = profile_home(name);
            if !h.exists() {
                bail!("Profile '{}' does not exist.", name);
            }
            h
        };

        let out_path = output
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("{name}.tar.gz")));

        create_tar_gz(&home, &out_path).with_context(|| format!("Exporting profile '{name}'"))?;

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

        // Infer profile name from archive filename if not provided
        let profile_name = name
            .map(|s| s.to_string())
            .unwrap_or_else(|| infer_name_from_archive(&archive_path));

        validate_name(&profile_name)?;

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

        self.write_alias(&profile_name, &profile_name)?;
        println!("Profile '{}' imported to {}", profile_name, dest.display());
        Ok(())
    }

    // ── helpers ───────────────────────────────────────────────────────

    /// Write a shell wrapper script to `~/.local/bin/<alias_name>`.
    fn write_alias(&self, profile_name: &str, alias_name: &str) -> Result<()> {
        let bin_dir = alias_bin_dir();
        if let Err(e) = fs::create_dir_all(&bin_dir) {
            // Non-fatal: some systems don't have ~/.local/bin
            tracing::debug!(error = %e, "Could not create alias bin dir");
            return Ok(());
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
        Ok(())
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

    local subcommands="setup doctor migrate acp version whatsapp status sessions config tools mcp plugins cron gateway skills profile completion"
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

/// Validate a profile name: alphanumeric + hyphens + underscores.
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Profile name cannot be empty.");
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        bail!(
            "Invalid profile name '{}'. Use alphanumeric characters, hyphens, or underscores.",
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

fn count_sessions(state_db_path: &Path) -> usize {
    SessionDb::open(state_db_path)
        .ok()
        .and_then(|db| db.list_sessions(10_000).ok().map(|sessions| sessions.len()))
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

fn seed_profile_home(dest: &Path) -> Result<()> {
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
        if matches!(name_str, ".active_profile" | "profiles") {
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

/// Create a tar.gz archive of `src_dir` at `out_path`.
fn create_tar_gz(src_dir: &Path, out_path: &Path) -> Result<()> {
    use std::process::Command;
    let status = Command::new("tar")
        .args([
            "czf",
            out_path.to_str().unwrap_or("profile.tar.gz"),
            "-C",
            src_dir.parent().unwrap_or(src_dir).to_str().unwrap_or("."),
            src_dir.file_name().and_then(|f| f.to_str()).unwrap_or("."),
        ])
        .status()
        .with_context(|| "Running tar — make sure tar is installed")?;

    if !status.success() {
        bail!("tar exited with status {}", status);
    }
    Ok(())
}

/// Extract a tar.gz archive into `dest_dir`.
fn extract_tar_gz(archive: &Path, dest_dir: &Path) -> Result<()> {
    use std::process::Command;
    let status = Command::new("tar")
        .args([
            "xzf",
            archive.to_str().unwrap_or("profile.tar.gz"),
            "-C",
            dest_dir.to_str().unwrap_or("."),
            "--strip-components=1",
        ])
        .status()
        .with_context(|| "Running tar — make sure tar is installed")?;

    if !status.success() {
        bail!("tar exited with status {}", status);
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
}
