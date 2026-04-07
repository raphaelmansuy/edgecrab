//! # cli_args — clap argument parsing with subcommand support
//!
//! WHY clap derive with subcommands: Mirrors hermes-agent's rich CLI
//! surface (`hermes setup`, `hermes doctor`, `hermes migrate`, `hermes acp`)
//! while keeping argument types safe and auto-documented via `--help`.
//!
//! ```text
//! edgecrab [OPTIONS] [PROMPT]      ← interactive TUI (default)
//! edgecrab setup                   ← first-run setup wizard
//! edgecrab doctor                  ← diagnostics check
//! edgecrab migrate [--dry-run]     ← hermes → edgecrab migration
//! edgecrab acp                     ← ACP stdio server for editors
//! edgecrab version                 ← detailed version info
//! edgecrab whatsapp                ← pair and configure WhatsApp bridge
//! ```

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// EdgeCrab — AI-native terminal agent
///
/// Spiritual successor to hermes-agent. Written in Rust for performance,
/// safety, and native platform integration.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "edgecrab",
    version,
    about = "AI-native terminal agent — spiritual successor to hermes-agent",
    long_about = None,
)]
pub struct CliArgs {
    /// Subcommand to run (default: interactive TUI)
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Initial prompt to send (non-interactive if provided with --quiet)
    #[arg(trailing_var_arg = true, global = false)]
    pub prompt: Vec<String>,

    /// Model to use, e.g. copilot/gpt-4.1-mini (overrides config)
    #[arg(short, long, global = true)]
    pub model: Option<String>,

    /// Active toolsets (comma-separated or alias: coding, all)
    #[arg(long, value_delimiter = ',', global = true)]
    pub toolset: Option<Vec<String>>,

    /// Resume a specific session by ID
    #[arg(short, long, global = true)]
    pub session: Option<String>,

    /// Continue the most recent CLI session (optionally by title)
    ///
    /// With no argument, resumes the last CLI session.
    /// With a title argument, resumes the named session (latest in lineage).
    #[arg(short = 'C', long = "continue", global = true)]
    pub continue_session: Option<Option<String>>,

    /// Resume a session by ID or title (alias for --session with resolution)
    #[arg(short = 'r', long, global = true)]
    pub resume: Option<String>,

    /// Quiet/headless mode — print result and exit (no TUI)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Config file path (default: ~/.edgecrab/config.yaml)
    #[arg(short, long, global = true)]
    pub config: Option<String>,

    /// Enable debug logging
    #[arg(long, global = true)]
    pub debug: bool,

    /// Skip ASCII banner display
    #[arg(long, global = true)]
    pub no_banner: bool,

    /// Run in an isolated git worktree for parallel/experimental sessions.
    ///
    /// Creates a temporary branch and worktree under `.worktrees/` inside the
    /// current repo, then runs EdgeCrab from that isolated directory.
    /// Equivalent to `hermes -w`.
    ///
    /// For parallel agents, open multiple terminals and run `edgecrab -w` in
    /// each — every invocation gets its own worktree and branch automatically.
    #[arg(short = 'w', long = "worktree", global = false)]
    pub worktree: bool,

    /// Preload one or more skills at session start.
    ///
    /// Skills are loaded into the system prompt before the first turn.
    /// May be repeated or comma-separated: `-s skill1 -s skill2` or `-s skill1,skill2`.
    /// Equivalent to `hermes -s skill1,skill2`.
    #[arg(short = 'S', long = "skill", action = clap::ArgAction::Append, value_delimiter = ',', global = false)]
    pub skills: Vec<String>,

    /// Run under a specific profile without changing the active default.
    ///
    /// Overrides the sticky default profile for the duration of this command.
    /// Equivalent to `hermes -p <name>` / `hermes --profile <name>`.
    ///
    /// Examples:
    ///   edgecrab -p work chat -q "check the server status"
    ///   edgecrab --profile dev gateway start
    #[arg(short = 'p', long = "profile", global = true)]
    pub profile: Option<String>,
}

/// CLI subcommands — each maps to a hermes-agent equivalent.
#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Manage named profiles (isolated config + memory + skills + sessions)
    ///
    /// Equivalent to `hermes profile`. Each profile has its own home directory
    /// under `~/.edgecrab/profiles/<name>/`.
    ///
    /// Examples:
    ///   edgecrab profile list
    ///   edgecrab profile create work --clone
    ///   edgecrab profile use work
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },

    /// Generate shell completion scripts (bash or zsh)
    ///
    /// Examples:
    ///   edgecrab completion bash >> ~/.bashrc
    ///   edgecrab completion zsh  >> ~/.zshrc
    Completion {
        /// Shell to generate completions for: bash or zsh
        shell: String,
    },

    /// Interactive setup wizard (API keys, model, config)
    ///
    /// Re-run to reconfigure. Supports section-specific setup:
    ///   edgecrab setup           — full interactive wizard
    ///   edgecrab setup model     — model & provider only
    ///   edgecrab setup tools     — toolsets configuration
    ///   edgecrab setup gateway   — messaging platforms
    ///   edgecrab setup agent     — agent settings
    ///   edgecrab setup --force   — overwrite existing config from scratch
    ///
    /// Equivalent to `hermes setup`.
    Setup {
        /// Specific section to configure (model, tools, gateway, agent)
        section: Option<String>,

        /// Force fresh setup, overwriting existing config
        #[arg(long)]
        force: bool,
    },

    /// Check configuration, API keys, and provider connectivity
    ///
    /// Equivalent to `hermes doctor`. Prints a colored status report.
    Doctor,

    /// Migrate from hermes-agent (~/.hermes/) to EdgeCrab (~/.edgecrab/)
    ///
    /// Copies config, memories, skills, and .env. Safe to re-run.
    Migrate {
        /// Preview what would be migrated without writing any files
        #[arg(long)]
        dry_run: bool,
    },

    /// Start ACP stdio server or generate editor onboarding files
    ///
    /// `edgecrab acp` starts the stdio JSON-RPC server.
    /// `edgecrab acp init` generates a workspace-local ACP registry and
    /// VS Code settings so the agent shows up without manual JSON editing.
    /// Equivalent to `hermes acp`, with additional setup automation.
    Acp {
        #[command(subcommand)]
        command: Option<AcpCommand>,
    },

    /// Show detailed version and provider information
    Version,

    /// Pair and configure the WhatsApp bridge
    ///
    /// Equivalent to `hermes whatsapp`.
    Whatsapp,

    /// Show a high-level runtime status summary
    Status,

    /// Persisted session management
    Sessions {
        #[command(subcommand)]
        command: SessionCommand,
    },

    /// Inspect or modify config.yaml
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Inspect registered tools and toolsets
    Tools {
        #[command(subcommand)]
        command: ToolsCommand,
    },

    /// Manage configured MCP servers
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },

    /// Inspect installed plugins
    Plugins {
        #[command(subcommand)]
        command: PluginsCommand,
    },

    /// Manage scheduled prompts
    Cron {
        #[command(subcommand)]
        command: CronCommand,
    },

    #[command(
        about = "Run or manage the messaging gateway",
        long_about = "Run or manage the messaging gateway\n\nCommon workflow:\n  edgecrab gateway configure\n  edgecrab gateway start\n  edgecrab gateway status"
    )]
    Gateway {
        #[command(subcommand)]
        command: GatewayCommand,
    },

    /// Manage agent skills (~/.edgecrab/skills/)
    ///
    /// Equivalent to `hermes skills`. Lists, views, and installs skills.
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum SessionCommand {
    /// List recent sessions
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Filter by source platform (cli, telegram, discord, whatsapp, slack)
        #[arg(long)]
        source: Option<String>,
    },
    /// Browse recent sessions or search their contents
    Browse {
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Export a persisted session (Markdown or JSONL)
    Export {
        id: String,
        #[arg(short, long)]
        output: Option<String>,
        /// Export format: markdown (default) or jsonl
        #[arg(long, default_value = "markdown")]
        format: String,
    },
    /// Delete a persisted session
    Delete { id: String },
    /// Rename a session (set or change its title)
    Rename {
        /// Session ID (or prefix)
        id: String,
        /// New title for the session
        #[arg(trailing_var_arg = true)]
        title: Vec<String>,
    },
    /// Prune old ended sessions
    Prune {
        /// Delete sessions older than N days (default: 90)
        #[arg(long, default_value_t = 90)]
        older_than: u32,
        /// Only prune sessions from this source/platform
        #[arg(long)]
        source: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Show high-level session statistics
    Stats,
}

#[derive(Subcommand, Debug, Clone)]
pub enum AcpCommand {
    /// Generate workspace-local ACP registry and VS Code settings.json
    Init {
        /// Workspace to configure (defaults to the current directory)
        #[arg(long)]
        workspace: Option<PathBuf>,

        /// Replace an invalid or non-object settings.json if present
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigCommand {
    /// Print the active config as YAML
    Show,
    /// Open config.yaml in $EDITOR
    Edit,
    /// Set a supported config key
    Set { key: String, value: String },
    /// Print the active config path
    Path,
    /// Print the .env file path
    EnvPath,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ToolsCommand {
    /// List registered tools and toolsets
    List,
    /// Enable a toolset in config
    Enable { name: String },
    /// Disable a toolset in config
    Disable { name: String },
}

#[derive(Subcommand, Debug, Clone)]
pub enum McpCommand {
    /// List configured MCP servers
    List,
    /// Refresh the cached official MCP catalog from upstream
    Refresh,
    /// Search the curated MCP preset catalog
    Search {
        /// Optional search query; omit to list all cached official entries
        query: Option<String>,
    },
    /// Show details for a controlled preset or official catalog entry
    View {
        /// Preset name from `edgecrab mcp search`
        preset: String,
    },
    /// Install a controlled MCP preset into config.yaml
    Install {
        /// Preset name from `edgecrab mcp search`
        preset: String,
        /// Override the configured server name
        #[arg(long)]
        name: Option<String>,
        /// Override the workspace/path argument for path-scoped presets
        #[arg(long)]
        path: Option<String>,
    },
    /// Probe a configured MCP server by connecting and listing tools
    Test {
        /// Configured MCP server name; omit to test all configured servers
        name: Option<String>,
    },
    /// Diagnose configured MCP servers with static checks and live probe output
    Doctor {
        /// Configured MCP server name; omit to diagnose all configured servers
        name: Option<String>,
    },
    /// Explain the active MCP authentication/OAuth path for one configured server
    Auth {
        /// Configured MCP server name
        name: String,
    },
    /// Perform an interactive OAuth login for one configured HTTP MCP server
    Login {
        /// Configured MCP server name
        name: String,
    },
    /// Add or update an MCP server
    Add {
        name: String,
        command: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Remove an MCP server
    #[command(visible_aliases = ["uninstall", "rm"])]
    Remove { name: String },
}

#[derive(Subcommand, Debug, Clone)]
pub enum PluginsCommand {
    /// List discovered plugins
    List,
    /// Install a plugin from a git repository
    Install {
        repo: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Update an installed plugin
    Update { name: String },
    /// Remove an installed plugin
    Remove { name: String },
}

#[derive(Subcommand, Debug, Clone)]
pub enum CronCommand {
    /// List scheduled jobs
    List {
        #[arg(long)]
        all: bool,
    },
    /// Show cron scheduler status
    Status,
    /// Run due jobs once and exit
    Tick,
    /// Create a scheduled job
    Create {
        schedule: String,
        #[arg(required = true)]
        prompt: Vec<String>,
        #[arg(long)]
        name: Option<String>,
        /// Skill(s) to load for this job (may be repeated)
        #[arg(long = "skill", action = clap::ArgAction::Append, value_name = "SKILL")]
        skills: Vec<String>,
        /// Run job at most N times, then remove it
        #[arg(long)]
        repeat: Option<u32>,
        /// Delivery target: local | origin | <platform>:<chat_id> (default: local)
        #[arg(long)]
        deliver: Option<String>,
    },
    /// Update an existing scheduled job
    Edit {
        id: String,
        #[arg(long)]
        schedule: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        name: Option<String>,
        /// Replace all skills with this set (may be repeated; takes precedence over --add/remove/clear-skill)
        #[arg(long = "skill", action = clap::ArgAction::Append, value_name = "SKILL")]
        skills: Vec<String>,
        /// Add a skill to this job
        #[arg(long = "add-skill", action = clap::ArgAction::Append, value_name = "SKILL")]
        add_skills: Vec<String>,
        /// Remove a skill from this job
        #[arg(long = "remove-skill", action = clap::ArgAction::Append, value_name = "SKILL")]
        remove_skills: Vec<String>,
        /// Remove all skills from this job
        #[arg(long)]
        clear_skills: bool,
        /// Delivery target: local | origin | <platform>:<chat_id>
        #[arg(long)]
        deliver: Option<String>,
    },
    /// Pause a scheduled job
    Pause { id: String },
    /// Resume a scheduled job
    Resume { id: String },
    /// Run a scheduled job immediately
    Run { id: String },
    /// Remove a scheduled job
    Remove { id: String },
}

#[derive(Subcommand, Debug, Clone)]
pub enum GatewayCommand {
    /// Start the gateway in the foreground or background
    ///
    /// Examples:
    ///   edgecrab gateway start
    ///   edgecrab gateway start --foreground
    Start {
        #[arg(long)]
        foreground: bool,
    },
    /// Stop the background gateway process
    ///
    /// Example:
    ///   edgecrab gateway stop
    Stop,
    /// Restart the gateway safely (stop if running, then start)
    ///
    /// Example:
    ///   edgecrab gateway restart
    Restart,
    /// Show runtime status, diagnostics, and actionable next steps
    ///
    /// Example:
    ///   edgecrab gateway status
    Status,
    /// Interactive gateway configuration wizard (all platforms)
    ///
    /// Examples:
    ///   edgecrab gateway configure
    ///   edgecrab gateway configure signal
    Configure {
        /// Configure a specific platform only (telegram, discord, slack, signal, whatsapp, webhook, email, sms, matrix, mattermost, dingtalk, homeassistant)
        platform: Option<String>,
    },
}

/// Profile management subcommands — mirrors `hermes profile <sub>`.
#[derive(Subcommand, Debug, Clone)]
pub enum ProfileCommand {
    /// List all profiles (active profile marked with *)
    List,

    /// Set the active (default) profile
    ///
    /// Example:
    ///   edgecrab profile use work
    ///   edgecrab profile use default
    Use {
        /// Profile name to activate (use "default" to return to the base profile)
        name: String,
    },

    /// Create a new profile
    ///
    /// Examples:
    ///   edgecrab profile create mybot
    ///   edgecrab profile create work --clone
    ///   edgecrab profile create backup --clone-all
    ///   edgecrab profile create work2 --clone --clone-from work
    Create {
        /// Name for the new profile (alphanumeric, hyphens, underscores)
        name: String,
        /// Copy config.yaml, .env, and SOUL.md from the current profile
        #[arg(long)]
        clone: bool,
        /// Copy everything (config, memories, skills, sessions, state) from the current profile
        #[arg(long = "clone-all")]
        clone_all: bool,
        /// Clone from a specific profile instead of the current one
        #[arg(long = "clone-from")]
        clone_from: Option<String>,
    },

    /// Delete a profile and its shell alias
    ///
    /// WARNING: Permanently deletes the profile's entire directory including
    /// all config, memories, sessions, and skills. Cannot delete the active profile.
    Delete {
        /// Profile to delete
        name: String,
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Show details about a profile (home dir, model, platforms, disk usage)
    Show {
        /// Profile to inspect
        name: String,
    },

    /// Regenerate or remove the shell alias script for a profile
    ///
    /// Examples:
    ///   edgecrab profile alias work
    ///   edgecrab profile alias work --name mywork
    ///   edgecrab profile alias work --remove
    Alias {
        /// Profile to create/update alias for
        name: String,
        /// Remove the wrapper script instead of creating it
        #[arg(long)]
        remove: bool,
        /// Custom alias name (default: profile name)
        #[arg(long)]
        name_override: Option<String>,
    },

    /// Rename a profile (updates directory and shell alias)
    ///
    /// Example:
    ///   edgecrab profile rename mybot assistant
    Rename {
        /// Current profile name
        old_name: String,
        /// New profile name
        new_name: String,
    },

    /// Export a profile as a compressed tar.gz archive
    ///
    /// Examples:
    ///   edgecrab profile export work
    ///   edgecrab profile export work -o ./work-2026-04-01.tar.gz
    Export {
        /// Profile to export
        name: String,
        /// Output file path (default: <name>.tar.gz)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Import a profile from a tar.gz archive
    ///
    /// Examples:
    ///   edgecrab profile import ./work-2026-04-01.tar.gz
    ///   edgecrab profile import ./work-2026-04-01.tar.gz --name work-restored
    Import {
        /// Path to the tar.gz archive to import
        archive: String,
        /// Name for the imported profile (default: inferred from archive)
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum SkillsCommand {
    /// List all installed skills
    List,
    /// View the content of a skill's SKILL.md
    View {
        /// Skill name to view
        name: String,
    },
    /// Search skills by name (substring match)
    Search {
        /// Search query
        query: String,
    },
    /// Install a skill from a git repository or local path
    Install {
        /// Git repository URL or local path containing a SKILL.md
        source: String,
        /// Override the skill name (default: last segment of source)
        #[arg(long)]
        name: Option<String>,
    },
    /// Update one or all hub-installed remote skills to the latest version
    Update {
        /// Skill name to update; omit to update all hub-installed skills
        name: Option<String>,
    },
    /// Remove an installed skill
    Remove {
        /// Skill name to remove
        name: String,
    },
}

impl CliArgs {
    /// Combine trailing prompt words into a single string.
    pub fn prompt_text(&self) -> Option<String> {
        if self.prompt.is_empty() {
            None
        } else {
            Some(self.prompt.join(" "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let args = CliArgs::parse_from(["edgecrab"]);
        assert!(args.prompt.is_empty());
        assert!(args.model.is_none());
        assert!(!args.quiet);
        assert!(args.command.is_none());
    }

    #[test]
    fn parse_with_model_and_prompt() {
        let args = CliArgs::parse_from([
            "edgecrab",
            "--model",
            "copilot/gpt-4.1-mini",
            "hello",
            "world",
        ]);
        assert_eq!(args.model.as_deref(), Some("copilot/gpt-4.1-mini"));
        assert_eq!(args.prompt_text().as_deref(), Some("hello world"));
    }

    #[test]
    fn parse_quiet_mode() {
        let args = CliArgs::parse_from(["edgecrab", "-q", "explain", "this", "code"]);
        assert!(args.quiet);
        assert_eq!(args.prompt_text().as_deref(), Some("explain this code"));
    }

    #[test]
    fn parse_setup_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "setup"]);
        assert!(matches!(args.command, Some(Command::Setup { .. })));
    }

    #[test]
    fn parse_setup_force() {
        let args = CliArgs::parse_from(["edgecrab", "setup", "--force"]);
        assert!(matches!(
            args.command,
            Some(Command::Setup { force: true, .. })
        ));
    }

    #[test]
    fn parse_setup_section() {
        let args = CliArgs::parse_from(["edgecrab", "setup", "model"]);
        match args.command {
            Some(Command::Setup {
                section: Some(s), ..
            }) => assert_eq!(s, "model"),
            _ => panic!("expected Setup with section=model"),
        }
    }

    #[test]
    fn parse_doctor_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "doctor"]);
        assert!(matches!(args.command, Some(Command::Doctor)));
    }

    #[test]
    fn parse_migrate_dry_run() {
        let args = CliArgs::parse_from(["edgecrab", "migrate", "--dry-run"]);
        assert!(matches!(
            args.command,
            Some(Command::Migrate { dry_run: true })
        ));
    }

    #[test]
    fn parse_migrate_live() {
        let args = CliArgs::parse_from(["edgecrab", "migrate"]);
        assert!(matches!(
            args.command,
            Some(Command::Migrate { dry_run: false })
        ));
    }

    #[test]
    fn parse_acp_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "acp"]);
        assert!(matches!(args.command, Some(Command::Acp { command: None })));
    }

    #[test]
    fn parse_acp_init_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "acp", "init", "--force"]);
        assert!(matches!(
            args.command,
            Some(Command::Acp {
                command: Some(AcpCommand::Init { force: true, .. })
            })
        ));
    }

    #[test]
    fn parse_version_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "version"]);
        assert!(matches!(args.command, Some(Command::Version)));
    }

    #[test]
    fn parse_sessions_list_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "sessions", "list", "--limit", "5"]);
        assert!(matches!(
            args.command,
            Some(Command::Sessions {
                command: SessionCommand::List {
                    limit: 5,
                    source: None
                }
            })
        ));
    }

    #[test]
    fn parse_sessions_browse_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "sessions", "browse", "--query", "rust"]);
        assert!(matches!(
            args.command,
            Some(Command::Sessions {
                command: SessionCommand::Browse { .. }
            })
        ));
    }

    #[test]
    fn parse_config_set_subcommand() {
        let args = CliArgs::parse_from([
            "edgecrab",
            "config",
            "set",
            "model.default",
            "openai/gpt-4o",
        ]);
        assert!(matches!(
            args.command,
            Some(Command::Config {
                command: ConfigCommand::Set { .. }
            })
        ));
    }

    #[test]
    fn parse_tools_disable_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "tools", "disable", "browser"]);
        assert!(matches!(
            args.command,
            Some(Command::Tools {
                command: ToolsCommand::Disable { .. }
            })
        ));
    }

    #[test]
    fn parse_mcp_add_subcommand() {
        let args = CliArgs::parse_from([
            "edgecrab",
            "mcp",
            "add",
            "github",
            "npx",
            "-y",
            "@modelcontextprotocol/server-github",
        ]);
        assert!(matches!(
            args.command,
            Some(Command::Mcp {
                command: McpCommand::Add { .. }
            })
        ));
    }

    #[test]
    fn parse_mcp_search_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "mcp", "search", "github"]);
        assert!(matches!(
            args.command,
            Some(Command::Mcp {
                command: McpCommand::Search { .. }
            })
        ));
    }

    #[test]
    fn parse_mcp_install_subcommand() {
        let args =
            CliArgs::parse_from(["edgecrab", "mcp", "install", "filesystem", "--path", "/tmp"]);
        assert!(matches!(
            args.command,
            Some(Command::Mcp {
                command: McpCommand::Install { .. }
            })
        ));
    }

    #[test]
    fn parse_mcp_doctor_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "mcp", "doctor", "github"]);
        assert!(matches!(
            args.command,
            Some(Command::Mcp {
                command: McpCommand::Doctor { .. }
            })
        ));
    }

    #[test]
    fn parse_mcp_auth_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "mcp", "auth", "github"]);
        assert!(matches!(
            args.command,
            Some(Command::Mcp {
                command: McpCommand::Auth { .. }
            })
        ));
    }

    #[test]
    fn parse_mcp_login_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "mcp", "login", "github"]);
        assert!(matches!(
            args.command,
            Some(Command::Mcp {
                command: McpCommand::Login { .. }
            })
        ));
    }

    #[test]
    fn parse_plugins_list_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "plugins", "list"]);
        assert!(matches!(
            args.command,
            Some(Command::Plugins {
                command: PluginsCommand::List
            })
        ));
    }

    #[test]
    fn parse_gateway_start_foreground() {
        let args = CliArgs::parse_from(["edgecrab", "gateway", "start", "--foreground"]);
        assert!(matches!(
            args.command,
            Some(Command::Gateway {
                command: GatewayCommand::Start { foreground: true }
            })
        ));
    }

    #[test]
    fn parse_whatsapp_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "whatsapp"]);
        assert!(matches!(args.command, Some(Command::Whatsapp)));
    }

    #[test]
    fn parse_status_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "status"]);
        assert!(matches!(args.command, Some(Command::Status)));
    }

    #[test]
    fn parse_cron_create_subcommand() {
        let args = CliArgs::parse_from([
            "edgecrab",
            "cron",
            "create",
            "0 * * * *",
            "summarize",
            "the",
            "repo",
        ]);
        assert!(matches!(
            args.command,
            Some(Command::Cron {
                command: CronCommand::Create { .. }
            })
        ));
    }

    #[test]
    fn parse_skills_list_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "skills", "list"]);
        assert!(matches!(
            args.command,
            Some(Command::Skills {
                command: SkillsCommand::List
            })
        ));
    }

    #[test]
    fn parse_skills_view_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "skills", "view", "my_skill"]);
        assert!(matches!(
            args.command,
            Some(Command::Skills {
                command: SkillsCommand::View { .. }
            })
        ));
    }

    #[test]
    fn parse_skills_search_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "skills", "search", "rust"]);
        assert!(matches!(
            args.command,
            Some(Command::Skills {
                command: SkillsCommand::Search { .. }
            })
        ));
    }

    #[test]
    fn parse_skills_install_subcommand() {
        let args = CliArgs::parse_from([
            "edgecrab",
            "skills",
            "install",
            "https://github.com/example/my-skill",
        ]);
        assert!(matches!(
            args.command,
            Some(Command::Skills {
                command: SkillsCommand::Install { .. }
            })
        ));
    }

    #[test]
    fn parse_skills_remove_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "skills", "remove", "old_skill"]);
        assert!(matches!(
            args.command,
            Some(Command::Skills {
                command: SkillsCommand::Remove { .. }
            })
        ));
    }

    #[test]
    fn parse_skills_update_subcommand() {
        let args = CliArgs::parse_from(["edgecrab", "skills", "update", "remote_skill"]);
        assert!(matches!(
            args.command,
            Some(Command::Skills {
                command: SkillsCommand::Update { .. }
            })
        ));
    }
}
