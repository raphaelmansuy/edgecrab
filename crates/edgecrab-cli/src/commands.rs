//! # commands — Slash command registry and dispatch
//!
//! WHY slash commands: Power-user shortcuts for model switching,
//! session management, and configuration. Mirrors hermes-agent's
//! 40+ command set.
//!
//! Full command map (46 commands, 54+ aliases):
//!
//! ```text
//!   Navigation    /help /quit /clear /new /status /version
//!   Model         /model /cheap_model /vision_model /image_model /moa /provider /reasoning /stream
//!   Session       /session /retry /undo /stop /history /save /export /title /resume
//!   Config        /config /prompt /verbose /personality /statusbar /log
//!   Tools         /tools /toolsets /mcp /reload-mcp /plugins /hooks
//!   Memory        /memory
//!   Analysis      /cost /usage /compress /insights
//!   Appearance    /skin /paste
//!   Advanced      /queue /background /rollback
//!   Gateway       /platforms /gateway /approve /deny /sethome /update
//!   Scheduling    /cron
//!   Media         /voice
//!   Diagnostics   /doctor [/permissions on macOS]
//! ```
//!
//! Command handlers are plain fn pointers (not closures) so they can
//! be stored without lifetime issues. State-mutating commands return
//! a rich `CommandResult` variant that the App event loop handles.

use std::collections::HashMap;

use edgecrab_command_catalog::{
    SlashCommandSpec, SlashSurface, grouped_slash_commands_for_surface, slash_commands_for_surface,
};
use edgecrab_core::{DiscoveryAvailability, live_discovery_availability};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolManagerMode {
    All,
    Toolsets,
}

/// Result of executing a slash command.
#[allow(dead_code)]
#[derive(Debug)]
pub enum CommandResult {
    /// Print text to the output area
    Output(String),
    /// Clear the output area
    Clear,
    /// Exit the application
    Exit,
    /// No visible effect
    Noop,
    /// Switch the active model (app handles provider creation + agent swap)
    ModelSwitch(String),
    /// Activate the interactive model selector overlay
    ModelSelector,
    /// Activate the interactive cheap-model selector overlay.
    CheapModelSelector,
    /// Activate the interactive vision-model selector overlay.
    VisionModelSelector,
    /// Activate the interactive image-model selector overlay.
    ImageModelSelector,
    /// Activate the interactive MoA aggregator selector overlay.
    MoaAggregatorSelector,
    /// Activate the interactive MoA reference-model selector overlay.
    MoaReferenceSelector,
    /// Show the current cheap-model routing state.
    ShowCheapModel,
    /// Show the current auxiliary vision-model routing state.
    ShowVisionModel,
    /// Update the cheap-model routing state.
    SetCheapModel(String),
    /// Update the auxiliary vision-model routing state.
    SetVisionModel(String),
    /// Show the current image-generation routing state.
    ShowImageModel,
    /// Update the default image-generation routing state.
    SetImageModel(String),
    /// Show the current Mixture-of-Agents defaults.
    ShowMoaConfig,
    /// Enable or disable Mixture-of-Agents for future tool calls.
    SetMoaEnabled(bool),
    /// Update the default MoA aggregator model.
    SetMoaAggregator(String),
    /// Add a reference model to the default MoA roster.
    AddMoaReference(String),
    /// Remove a reference model from the default MoA roster.
    RemoveMoaReference(String),
    /// Reset the default MoA roster and aggregator.
    ResetMoaConfig,
    /// Start a fresh session (app clears messages + session state)
    SessionNew,
    /// Load a theme from YAML skin file and redraw
    ReloadTheme,
    /// Signal the app to cancel the current in-flight agent request
    Stop,
    /// Re-send the last user message
    Retry,
    /// Remove last user + assistant message pair from history
    Undo,
    /// Trigger manual context compression
    Compress,

    // ── Phase 8.1: stateful commands (app handles via Agent) ─────
    /// Show live session stats (token counts, model, budget)
    ShowStatus,
    /// Show cost breakdown (tokens × pricing → estimated USD)
    ShowCost,
    /// Show full token usage breakdown
    ShowUsage,
    /// Show, clear, or update the custom system prompt override
    PromptCommand(String),
    /// Open or query the configuration surface
    ShowConfig(String),
    /// Show message history summary
    ShowHistory,
    /// Cycle tool progress display
    ToggleVerbose,
    /// Set or inspect tool progress display mode
    SetToolProgress(String),
    /// Save session to a JSON file (optional path)
    SaveSession(Option<String>),
    /// Export session as Markdown (optional path)
    ExportSession(Option<String>),
    /// Set the session title
    SetTitle(String),
    /// Open a debugger/inspector for the current in-memory session.
    InspectCurrentSession,
    /// Open the session browser overlay, optionally seeded with a query.
    SessionBrowse(Option<String>),
    /// Switch to a persisted session by ID prefix
    SessionSwitch(String),
    /// Delete a persisted session by ID prefix
    SessionDelete(String),
    /// Resume a named or most-recent session
    ResumeSession(Option<String>),
    /// Rename a session (id_prefix, new_title)
    SessionRename(String, String),
    /// Prune old ended sessions (older_than_days)
    SessionPrune(u32),
    /// Queue a prompt to run after the current one completes
    QueuePrompt(String),
    /// Run a prompt in the background
    BackgroundPrompt(String),
    /// Ask an ephemeral side question using the current session context.
    SideQuestion(String),
    /// Fork the current session into a new branch and switch to it.
    BranchSession(Option<String>),
    /// Show/manage skills
    ShowSkills(String),
    /// Show/manage profiles
    ShowProfiles(String),
    /// Show/manage MCP servers and presets
    ShowMcp(String),
    /// Activate the interactive skill selector overlay
    SkillSelector,
    /// Activate the interactive profile selector overlay.
    ProfileSelector,
    /// Activate the interactive tool manager overlay.
    ToolManager(ToolManagerMode),
    /// Reset tool and toolset policy back to defaults.
    ResetToolPolicy,
    /// Set reasoning effort or think-mode visibility (low/medium/high/show/hide/status)
    SetReasoning(String),
    /// Toggle live token streaming (on/off/toggle/status)
    SetStreaming(String),
    /// Toggle the TUI status bar visibility (on/off/toggle/status)
    SetStatusBar(String),
    /// Inspect logs or configure the saved logging level.
    LogCommand(String),
    /// Inspect or configure persistent git worktree mode.
    WorktreeCommand(String),
    /// List available models for the current or specified provider
    ListModels(String),
    /// Show cron job status (args: "list" or "")
    ShowCronStatus(String),
    /// Manage plugins and open the local plugin browser overlay.
    ShowPlugins(String),
    /// Inspect, reload, and browse the local hook registry.
    ShowHooks(String),
    /// Open the interactive local plugin browser overlay.
    ShowPluginToggle {
        name: Option<String>,
        platform: Option<String>,
    },
    /// Show gateway platform availability
    ShowPlatforms,
    /// Show active personality/persona
    ShowPersonality,
    /// Switch to a named personality preset (session-level overlay)
    SwitchPersonality(String),
    /// Switch to a named skin preset (session-level)
    SwitchSkin(String),
    /// Show conversation insights from session DB for the requested day window.
    ShowInsights(u32),
    /// Paste clipboard text into the input
    PasteClipboard,
    /// Queue a local image file for the next prompt.
    AttachImagePath(String),
    /// Run one auth control-plane command.
    AuthCommand(crate::cli_args::AuthCommand),
    /// Start a login/import flow for one auth target.
    LoginTarget(String),
    /// Clear cached auth state for one or all targets.
    LogoutTarget(Option<String>),
    /// Manage dynamic gateway webhook subscriptions.
    WebhookCommand(crate::cli_args::WebhookCommand),
    /// Plan or execute a local uninstall action.
    UninstallCommand(crate::uninstall_cmd::UninstallOptions),
    /// Trigger Copilot GitHub authentication (device code flow or auto-import from VS Code)
    CopilotAuth,
    /// Manage terminal mouse capture mode (on/off/toggle/status)
    MouseMode(String),
    /// Toggle or inspect YOLO approval bypass for the current session.
    SetYolo(String),
    /// Toggle the Shadow Judge completion oracle (on/off/toggle/status).
    SetShadowJudge(String),
    /// Resolve the current approval prompt from a slash command.
    ApprovalChoice(edgecrab_core::ApprovalChoice),
    /// macOS permission diagnostics and bootstrap workflow.
    #[cfg(target_os = "macos")]
    MacosPermissions(String),
    /// Restore a file checkpoint (list if no name given, restore <name> otherwise).
    /// Wires to the `checkpoint` tool via the agent.
    RollbackCheckpoint(String),
    /// Drop all active MCP server connections and re-connect on next tool call.
    ReloadMcp,
    /// Toggle voice mode — TTS readback of agent responses (on/off/tts/status).
    VoiceMode(String),
    /// Manage MCP OAuth Bearer tokens (set/remove/list).
    McpToken(String),
    /// Manage browser CDP connection (connect/disconnect/status).
    BrowserCommand(String),
    /// Show or update gateway home-channel configuration.
    SetHomeChannel(String),
    /// Start, stop, restart, or inspect the gateway runtime.
    GatewayControl(String),
    /// Show local upgrade status and actionable update guidance.
    CheckUpdates,
    /// Show the gateway slash-command catalog, optionally paginated.
    ShowGatewayCommands(String),
}

/// A registered slash command.
#[allow(dead_code)]
pub struct Command {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub handler: fn(args: &str) -> CommandResult,
}

/// Registry of all slash commands.
///
/// Two maps are maintained to keep both name and alias lookup O(1):
/// - `commands`: canonical name → Command struct
/// - `alias_map`: alias → canonical name (every alias resolves to one name)
pub struct CommandRegistry {
    commands: HashMap<&'static str, Command>,
    /// Alias → canonical command name. Built in `register()` alongside `commands`.
    alias_map: HashMap<&'static str, &'static str>,
}

fn parse_session_archive_command(args: &str) -> CommandResult {
    match args.trim() {
        "" | "list" | "ls" | "browse" => CommandResult::SessionBrowse(None),
        s if s.starts_with("browse ") => {
            let query = s
                .strip_prefix("browse ")
                .unwrap_or_default()
                .trim()
                .to_string();
            CommandResult::SessionBrowse((!query.is_empty()).then_some(query))
        }
        s if s.starts_with("search ") => {
            let query = s
                .strip_prefix("search ")
                .unwrap_or_default()
                .trim()
                .to_string();
            if query.is_empty() {
                CommandResult::Output("Usage: /sessions search <query>".into())
            } else {
                CommandResult::SessionBrowse(Some(query))
            }
        }
        s if s.starts_with("switch ") || s.starts_with("sw ") => {
            let id = s.split_whitespace().nth(1).unwrap_or("").to_string();
            if id.is_empty() {
                CommandResult::Output("Usage: /sessions switch <id-prefix>".into())
            } else {
                CommandResult::SessionSwitch(id)
            }
        }
        s if s.starts_with("delete ") || s.starts_with("del ") || s.starts_with("rm ") => {
            let id = s.split_whitespace().nth(1).unwrap_or("").to_string();
            if id.is_empty() {
                CommandResult::Output("Usage: /sessions delete <id-prefix>".into())
            } else {
                CommandResult::SessionDelete(id)
            }
        }
        s if s.starts_with("rename ") || s.starts_with("mv ") => {
            let mut parts = s.splitn(3, ' ');
            let _cmd = parts.next();
            let id = parts.next().unwrap_or("").to_string();
            let title = parts.next().unwrap_or("").to_string();
            if id.is_empty() || title.is_empty() {
                CommandResult::Output("Usage: /sessions rename <id-prefix> <new title>".into())
            } else {
                CommandResult::SessionRename(id, title)
            }
        }
        "prune" => CommandResult::SessionPrune(90),
        s if s.starts_with("prune ") => {
            let days: u32 = s
                .split_whitespace()
                .nth(1)
                .and_then(|n| n.parse().ok())
                .unwrap_or(90);
            CommandResult::SessionPrune(days)
        }
        "new" | "reset" => CommandResult::SessionNew,
        _ => CommandResult::Output(
            "Usage: /sessions [browse|search <query>|switch <id>|delete <id>|rename <id> <title>|prune [days]|new]".into(),
        ),
    }
}

fn parse_plugins_command(args: &str) -> CommandResult {
    let trimmed = args.trim();
    if trimmed.is_empty() || matches!(trimmed, "list" | "ls") {
        return CommandResult::ShowPluginToggle {
            name: None,
            platform: None,
        };
    }
    if trimmed == "toggle" {
        return CommandResult::ShowPluginToggle {
            name: None,
            platform: None,
        };
    }
    if let Some(rest) = trimmed.strip_prefix("toggle ") {
        let mut name = None;
        let mut platform = None;
        let mut parts = rest.split_whitespace().peekable();
        while let Some(part) = parts.next() {
            if part == "--platform" {
                platform = parts.next().map(ToString::to_string);
            } else if name.is_none() {
                name = Some(part.to_string());
            }
        }
        return CommandResult::ShowPluginToggle { name, platform };
    }
    CommandResult::ShowPlugins(trimmed.to_string())
}

fn parse_current_session_command(args: &str) -> CommandResult {
    match args.trim() {
        "" | "current" | "inspect" | "debug" => CommandResult::InspectCurrentSession,
        "new" | "reset" => CommandResult::SessionNew,
        other => parse_session_archive_command(other),
    }
}

impl CommandRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
            alias_map: HashMap::new(),
        };
        registry.register_defaults();
        registry
    }

    /// Try to dispatch a slash command. Returns None if not a slash command.
    ///
    /// Lookup is O(1): canonical names are looked up directly in `commands`;
    /// aliases are resolved via `alias_map` to a canonical name, then looked
    /// up in `commands`.
    pub fn dispatch(&self, input: &str) -> Option<CommandResult> {
        let input = input.trim();
        if !input.starts_with('/') {
            return None;
        }

        let (cmd_name, args) = match input.find(' ') {
            Some(pos) => (&input[1..pos], input[pos + 1..].trim()),
            None => (&input[1..], ""),
        };

        // O(1) — try canonical name first, then resolve via alias_map.
        let canonical = if self.commands.contains_key(cmd_name) {
            cmd_name
        } else {
            self.alias_map.get(cmd_name).copied()?
        };

        self.commands.get(canonical).map(|cmd| (cmd.handler)(args))
    }

    /// List all registered commands (sorted).
    #[allow(dead_code)]
    pub fn list(&self) -> Vec<(&'static str, &'static str)> {
        let mut cmds: Vec<_> = self
            .commands
            .values()
            .map(|c| (c.name, c.description))
            .collect();
        cmds.sort_by_key(|(name, _)| *name);
        cmds
    }

    /// List all command names **and** their aliases (sorted, deduped).
    pub fn all_names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = Vec::new();
        for cmd in self.commands.values() {
            names.push(cmd.name);
            names.extend_from_slice(cmd.aliases);
        }
        names.sort();
        names.dedup();
        names
    }

    /// Map every name/alias → description (aliases share parent's description).
    pub fn all_descriptions(&self) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for cmd in self.commands.values() {
            map.insert(format!("/{}", cmd.name), cmd.description.to_string());
            for alias in cmd.aliases {
                map.insert(format!("/{alias}"), cmd.description.to_string());
            }
        }
        map
    }

    pub fn gateway_entries(&self) -> Vec<(&'static str, &'static str)> {
        let mut entries: Vec<_> = slash_commands_for_surface(SlashSurface::Gateway)
            .into_iter()
            .map(|cmd| (cmd.name, cmd.description))
            .collect();
        entries.sort_by_key(|(name, _)| *name);
        entries
    }

    fn register(&mut self, cmd: Command) {
        // Register all aliases in the alias_map BEFORE moving cmd into commands.
        // We read cmd.aliases first (while cmd is still accessible), then insert.
        for alias in cmd.aliases {
            self.alias_map.insert(alias, cmd.name);
        }
        self.commands.insert(cmd.name, cmd);
    }

    fn register_defaults(&mut self) {
        // ── Navigation ────────────────────────────────────────────────

        self.register(Command {
            name: "help",
            aliases: &["h", "?"],
            description: "Show available commands",
            handler: |_| CommandResult::Output(help_text()),
        });

        self.register(Command {
            name: "quit",
            aliases: &["exit", "q"],
            description: "Exit EdgeCrab",
            handler: |_| CommandResult::Exit,
        });

        self.register(Command {
            name: "clear",
            aliases: &["cls"],
            description: "Clear the screen and start a fresh session",
            handler: |_| CommandResult::Clear,
        });

        self.register(Command {
            name: "version",
            aliases: &["ver"],
            description: "Show version and build info",
            handler: |_| {
                CommandResult::Output(format!(
                    "EdgeCrab v{}\n\
                     Build: Rust {}\n\
                     Default model: copilot/gpt-4.1-mini\n\
                     Crates: edgecrab-core, edgecrab-cli, edgecrab-tools, \
                     edgecrab-state, edgecrab-acp, edgecrab-migrate",
                    env!("CARGO_PKG_VERSION"),
                    env!("CARGO_PKG_RUST_VERSION", "stable"),
                ))
            },
        });

        self.register(Command {
            name: "status",
            aliases: &["stat"],
            description: "Show current session status",
            handler: |_| CommandResult::ShowStatus,
        });

        // ── Model commands ────────────────────────────────────────────

        self.register(Command {
            name: "model",
            aliases: &[],
            description:
                "Show model selector or switch model (e.g. /model openrouter/openai/gpt-5.4)",
            // WHY return ModelSwitch: The handler can't access the agent directly
            // (fn pointer, not closure). The App event loop performs the actual
            // provider creation + agent.swap_model() call.
            handler: |args| {
                if args.is_empty() {
                    CommandResult::ModelSelector
                } else {
                    CommandResult::ModelSwitch(args.to_string())
                }
            },
        });

        self.register(Command {
            name: "cheap_model",
            aliases: &["cheap-model"],
            description: "Open, show, or set the smart-routing cheap model (/cheap_model, /cheap_model status, /cheap_model off)",
            handler: |args| {
                let trimmed = args.trim();
                if trimmed.is_empty()
                    || trimmed.eq_ignore_ascii_case("open")
                    || trimmed.eq_ignore_ascii_case("list")
                {
                    CommandResult::CheapModelSelector
                } else if trimmed.eq_ignore_ascii_case("status") {
                    CommandResult::ShowCheapModel
                } else {
                    CommandResult::SetCheapModel(trimmed.to_string())
                }
            },
        });

        self.register(Command {
            name: "vision_model",
            aliases: &["vision-model"],
            description: "Open, show, or set the dedicated vision backend (/vision_model, /vision_model status, /vision_model auto)",
            handler: |args| {
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    CommandResult::VisionModelSelector
                } else if trimmed.eq_ignore_ascii_case("status") {
                    CommandResult::ShowVisionModel
                } else {
                    CommandResult::SetVisionModel(trimmed.to_string())
                }
            },
        });

        self.register(Command {
            name: "image_model",
            aliases: &["image-model"],
            description: "Open, show, or set the default image-generation backend (/image_model, /image_model status, /image_model gemini/gemini-2.5-flash-image)",
            handler: |args| {
                let trimmed = args.trim();
                if trimmed.is_empty()
                    || trimmed.eq_ignore_ascii_case("open")
                    || trimmed.eq_ignore_ascii_case("list")
                {
                    CommandResult::ImageModelSelector
                } else if trimmed.eq_ignore_ascii_case("status") {
                    CommandResult::ShowImageModel
                } else {
                    CommandResult::SetImageModel(trimmed.to_string())
                }
            },
        });

        self.register(Command {
            name: "moa",
            aliases: &[],
            description: "Inspect, enable, or configure MoA defaults (/moa status, /moa on, /moa aggregator, /moa experts, /moa add, /moa remove)",
            handler: |args| {
                let trimmed = args.trim();
                if trimmed.is_empty()
                    || matches!(
                        trimmed.to_ascii_lowercase().as_str(),
                        "status" | "show" | "list"
                    )
                {
                    return CommandResult::ShowMoaConfig;
                }

                if matches!(
                    trimmed.to_ascii_lowercase().as_str(),
                    "on" | "enable" | "enabled"
                ) {
                    return CommandResult::SetMoaEnabled(true);
                }

                if matches!(
                    trimmed.to_ascii_lowercase().as_str(),
                    "off" | "disable" | "disabled"
                ) {
                    return CommandResult::SetMoaEnabled(false);
                }

                if trimmed.eq_ignore_ascii_case("reset")
                    || trimmed.eq_ignore_ascii_case("default")
                {
                    return CommandResult::ResetMoaConfig;
                }

                if trimmed.eq_ignore_ascii_case("references")
                    || trimmed.eq_ignore_ascii_case("refs")
                    || trimmed.eq_ignore_ascii_case("roster")
                    || trimmed.eq_ignore_ascii_case("experts")
                    || trimmed.eq_ignore_ascii_case("expert")
                    || trimmed.eq_ignore_ascii_case("edit")
                    || trimmed.eq_ignore_ascii_case("open")
                {
                    return CommandResult::MoaReferenceSelector;
                }

                if trimmed.eq_ignore_ascii_case("aggregator")
                    || trimmed.eq_ignore_ascii_case("agg")
                {
                    return CommandResult::MoaAggregatorSelector;
                }

                if let Some(model) = trimmed.strip_prefix("aggregator ") {
                    return CommandResult::SetMoaAggregator(model.trim().to_string());
                }
                if let Some(model) = trimmed.strip_prefix("agg ") {
                    return CommandResult::SetMoaAggregator(model.trim().to_string());
                }
                if trimmed.eq_ignore_ascii_case("add") {
                    return CommandResult::AddMoaReference(String::new());
                }
                if let Some(model) = trimmed.strip_prefix("add ") {
                    return CommandResult::AddMoaReference(model.trim().to_string());
                }
                if trimmed.eq_ignore_ascii_case("remove") || trimmed.eq_ignore_ascii_case("rm") {
                    return CommandResult::RemoveMoaReference(String::new());
                }
                if let Some(model) = trimmed.strip_prefix("remove ") {
                    return CommandResult::RemoveMoaReference(model.trim().to_string());
                }
                if let Some(model) = trimmed.strip_prefix("rm ") {
                    return CommandResult::RemoveMoaReference(model.trim().to_string());
                }

                CommandResult::Output(
                    "Usage: /moa [status|on|off|reset|aggregator [provider/model]|experts|add [provider/model]|remove [provider/model]]"
                        .into(),
                )
            },
        });

        self.register(Command {
            name: "models",
            aliases: &[],
            description: "List models (supports live discovery: /models <provider>, /models refresh [provider|all])",
            handler: |args| CommandResult::ListModels(args.to_string()),
        });

        self.register(Command {
            name: "provider",
            aliases: &["providers"],
            description: "List available LLM providers",
            handler: |_| CommandResult::Output(provider_help_text()),
        });

        // ── Session commands ──────────────────────────────────────────

        self.register(Command {
            name: "new",
            aliases: &["reset"],
            description: "Start a fresh conversation",
            handler: |_| CommandResult::SessionNew,
        });

        self.register(Command {
            name: "session",
            aliases: &[],
            description: "Inspect and debug the live current session",
            handler: parse_current_session_command,
        });

        self.register(Command {
            name: "sessions",
            aliases: &[],
            description: "Browse, search, and manage saved sessions",
            handler: parse_session_archive_command,
        });

        self.register(Command {
            name: "retry",
            aliases: &["r"],
            description: "Re-send the last user message",
            handler: |_| CommandResult::Retry,
        });

        self.register(Command {
            name: "undo",
            aliases: &["u"],
            description: "Remove the last user + assistant message pair",
            handler: |_| CommandResult::Undo,
        });

        self.register(Command {
            name: "stop",
            aliases: &["cancel", "interrupt"],
            description: "Cancel the current agent request",
            handler: |_| CommandResult::Stop,
        });

        self.register(Command {
            name: "history",
            aliases: &[],
            description: "Show session message count and turn history",
            handler: |_| CommandResult::ShowHistory,
        });

        self.register(Command {
            name: "save",
            aliases: &[],
            description: "Save conversation to a JSON file",
            handler: |args| {
                let path = if args.is_empty() {
                    None
                } else {
                    Some(args.to_string())
                };
                CommandResult::SaveSession(path)
            },
        });

        self.register(Command {
            name: "export",
            aliases: &[],
            description: "Export conversation as Markdown",
            handler: |args| {
                let path = if args.is_empty() {
                    None
                } else {
                    Some(args.to_string())
                };
                CommandResult::ExportSession(path)
            },
        });

        // ── Config commands ───────────────────────────────────────────

        self.register(Command {
            name: "config",
            aliases: &["cfg"],
            description: "Open the config center or inspect paths/settings (/config, /config show, /config paths)",
            handler: |args| CommandResult::ShowConfig(args.trim().to_string()),
        });

        self.register(Command {
            name: "prompt",
            aliases: &["sys", "system"],
            description: "Show, clear, or set the custom system prompt (/prompt, /prompt clear, /prompt <text>)",
            handler: |args| CommandResult::PromptCommand(args.trim().to_string()),
        });

        self.register(Command {
            name: "verbose",
            aliases: &["v"],
            description:
                "Cycle tool-progress mode; or set directly: /verbose [off|new|all|verbose|status]",
            handler: |args| {
                let args = args.trim();
                if args.is_empty() {
                    CommandResult::ToggleVerbose
                } else {
                    CommandResult::SetToolProgress(args.to_string())
                }
            },
        });

        self.register(Command {
            name: "personality",
            aliases: &["persona"],
            description: "Show or switch personality: /personality [name|clear]",
            handler: |args| {
                let name = args.trim();
                if name.is_empty() {
                    CommandResult::ShowPersonality
                } else {
                    CommandResult::SwitchPersonality(name.to_string())
                }
            },
        });

        // ── Tools commands ────────────────────────────────────────────

        self.register(Command {
            name: "tools",
            aliases: &[],
            description: "Browse and configure tools (/tools reset restores defaults)",
            handler: |args| match args.trim() {
                "reset" => CommandResult::ResetToolPolicy,
                _ => CommandResult::ToolManager(ToolManagerMode::All),
            },
        });

        self.register(Command {
            name: "toolsets",
            aliases: &["ts"],
            description: "Browse and configure toolsets (/toolsets reset restores defaults)",
            handler: |args| match args.trim() {
                "reset" => CommandResult::ResetToolPolicy,
                _ => CommandResult::ToolManager(ToolManagerMode::Toolsets),
            },
        });

        // ── Memory commands ───────────────────────────────────────────

        self.register(Command {
            name: "memory",
            aliases: &["mem"],
            description: "Show memory status",
            handler: |_| {
                let home =
                    std::env::var("EDGECRAB_HOME").unwrap_or_else(|_| "~/.edgecrab".to_string());
                CommandResult::Output(format!(
                    "Memory store: {home}/memories/\n\
                     Format: §-delimited entries, one topic per file\n\
                     \nUse memory_read/memory_write tools to manage entries,\n\
                     or browse files directly in {home}/memories/"
                ))
            },
        });

        // ── Analysis commands ─────────────────────────────────────────

        self.register(Command {
            name: "cost",
            aliases: &[],
            description: "Show token usage and estimated cost",
            handler: |_| CommandResult::ShowCost,
        });

        self.register(Command {
            name: "usage",
            aliases: &[],
            description: "Full token usage breakdown",
            handler: |_| CommandResult::ShowUsage,
        });

        self.register(Command {
            name: "compress",
            aliases: &["compact"],
            description: "Manually trigger context compression",
            handler: |_| CommandResult::Compress,
        });

        self.register(Command {
            name: "insights",
            aliases: &[],
            description: "Show conversation insights",
            handler: |args| {
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    return CommandResult::ShowInsights(30);
                }
                match trimmed.parse::<u32>() {
                    Ok(days) if days > 0 => CommandResult::ShowInsights(days),
                    _ => CommandResult::Output("Usage: /insights [days]".into()),
                }
            },
        });

        // ── Appearance ────────────────────────────────────────────────

        self.register(Command {
            name: "skin",
            aliases: &["theme"],
            description: "Browse, reload, or switch skins: /skin, /skin reload, /skin <name>",
            handler: |args| {
                let name = args.trim();
                if name.eq_ignore_ascii_case("reload") {
                    CommandResult::ReloadTheme
                } else {
                    CommandResult::SwitchSkin(name.to_string())
                }
            },
        });

        self.register(Command {
            name: "mouse",
            aliases: &["scroll"],
            description: "Mouse/scroll mode: on/off/toggle/status  (alias: /scroll)",
            handler: |args| CommandResult::MouseMode(args.trim().to_string()),
        });

        // ── Diagnostics ───────────────────────────────────────────────

        self.register(Command {
            name: "doctor",
            aliases: &["diag"],
            description: "Run diagnostics",
            handler: |_| {
                let api_key_status = if std::env::var("OPENAI_API_KEY").is_ok()
                    || std::env::var("ANTHROPIC_API_KEY")
                        .ok()
                        .is_some_and(|value| !value.trim().is_empty())
                    || std::env::var("ANTHROPIC_AUTH_TOKEN")
                        .ok()
                        .is_some_and(|value| !value.trim().is_empty())
                    || std::env::var("GEMINI_API_KEY").is_ok()
                    || std::env::var("GITHUB_COPILOT_TOKEN").is_ok()
                {
                    "✓ API key: detected"
                } else {
                    "⚠ API key: none detected (run `edgecrab setup` to configure)"
                };

                let home =
                    std::env::var("EDGECRAB_HOME").unwrap_or_else(|_| "~/.edgecrab".to_string());

                CommandResult::Output(format!(
                    "EdgeCrab in-session diagnostics:\n\
                     ✓ Agent: running\n\
                     ✓ SQLite state: ok\n\
                     ✓ Tool registry: loaded\n\
                     {api_key_status}\n\
                     ✓ Config dir: {home}\n\
                     Skin: {home}/skin.yaml (use /skin reload to refresh)\n\
                     \nFor full diagnostics run: edgecrab doctor"
                ))
            },
        });

        self.register(Command {
            name: "dump",
            aliases: &["debug-dump", "debug"],
            description: "Show compact setup summary for support (copy-paste friendly)",
            handler: |_| {
                let output = crate::dump_cmd::run_dump(false);
                CommandResult::Output(output)
            },
        });

        #[cfg(target_os = "macos")]
        self.register(Command {
            name: "permissions",
            aliases: &["perm"],
            description: "Inspect or bootstrap macOS terminal-host permissions",
            handler: |args| CommandResult::MacosPermissions(args.trim().to_string()),
        });

        self.register(Command {
            name: "copilot-auth",
            aliases: &["copilot-login", "gh-auth"],
            description: "Authenticate with GitHub Copilot (auto-imports VS Code token or starts device flow)",
            handler: |_| CommandResult::CopilotAuth,
        });

        self.register(Command {
            name: "auth",
            aliases: &[],
            description: "Inspect or mutate EdgeCrab-managed auth state",
            handler: |args| match crate::auth_cmd::command_from_slash_args(args) {
                Ok(command) => CommandResult::AuthCommand(command),
                Err(err) => CommandResult::Output(err),
            },
        });

        self.register(Command {
            name: "login",
            aliases: &[],
            description: "Run one auth login/import flow",
            handler: |args| match crate::auth_cmd::login_target_from_slash_args(args) {
                Ok(target) => CommandResult::LoginTarget(target),
                Err(err) => CommandResult::Output(err),
            },
        });

        self.register(Command {
            name: "logout",
            aliases: &[],
            description: "Clear one or all local auth caches",
            handler: |args| match crate::auth_cmd::logout_target_from_slash_args(args) {
                Ok(target) => CommandResult::LogoutTarget(target),
                Err(err) => CommandResult::Output(err),
            },
        });

        self.register(Command {
            name: "uninstall",
            aliases: &[],
            description: "Preview or execute EdgeCrab uninstall actions",
            handler: |args| match crate::uninstall_cmd::options_from_slash_args(args) {
                Ok(options) => CommandResult::UninstallCommand(options),
                Err(err) => CommandResult::Output(err),
            },
        });

        // ── Session (extended) ────────────────────────────────────────

        self.register(Command {
            name: "title",
            aliases: &[],
            description: "Set or show session title",
            handler: |args| {
                if args.is_empty() {
                    CommandResult::ShowStatus
                } else {
                    CommandResult::SetTitle(args.to_string())
                }
            },
        });

        self.register(Command {
            name: "resume",
            aliases: &[],
            description: "Resume a named/previous session",
            handler: |args| {
                let id = if args.is_empty() {
                    None
                } else {
                    Some(args.to_string())
                };
                CommandResult::ResumeSession(id)
            },
        });

        // ── Model (extended) ──────────────────────────────────────────

        self.register(Command {
            name: "reasoning",
            aliases: &["think"],
            description: "Manage reasoning effort and display (low/medium/high/show/hide/status)",
            handler: |args| CommandResult::SetReasoning(args.trim().to_string()),
        });

        self.register(Command {
            name: "stream",
            aliases: &["streaming"],
            description: "Toggle live token streaming (on/off/toggle/status)",
            handler: |args| CommandResult::SetStreaming(args.trim().to_string()),
        });

        // ── Config (extended) ─────────────────────────────────────────

        self.register(Command {
            name: "statusbar",
            aliases: &["sb"],
            description: "Status bar visibility: /statusbar [on|off|toggle|status]",
            handler: |args| CommandResult::SetStatusBar(args.trim().to_string()),
        });

        self.register(Command {
            name: "log",
            aliases: &["logs"],
            description: "Browse and live-follow local logs or set the saved log level: /log [open|level <error|warn|info|debug|trace>]",
            handler: |args| CommandResult::LogCommand(args.trim().to_string()),
        });

        self.register(Command {
            name: "worktree",
            aliases: &["w"],
            description: "Worktree status and default launch policy: /worktree [status|on|off|toggle]",
            handler: |args| CommandResult::WorktreeCommand(args.trim().to_string()),
        });

        // ── Tools (extended) ──────────────────────────────────────────

        self.register(Command {
            name: "reload-mcp",
            aliases: &["mcp-reload", "reload_mcp"],
            description: "Reload MCP server connections",
            handler: |_| CommandResult::ReloadMcp,
        });

        self.register(Command {
            name: "mcp",
            aliases: &[],
            description: "List, search, install, test, diagnose, or remove MCP servers (/mcp help)",
            handler: |args| CommandResult::ShowMcp(args.trim().to_string()),
        });

        self.register(Command {
            name: "mcp-token",
            aliases: &[],
            description:
                "Manage MCP OAuth Bearer tokens: set <server> <token> | remove <server> | list",
            handler: |args| CommandResult::McpToken(args.trim().to_string()),
        });

        self.register(Command {
            name: "plugins",
            aliases: &["plugin"],
            description: "Browse/manage plugins: overlay, info, status, install, enable, disable, toggle, audit, hub",
            handler: |args| parse_plugins_command(args),
        });

        self.register(Command {
            name: "hooks",
            aliases: &["hook"],
            description: "Inspect or reload local lifecycle hooks: /hooks [list|status|help|reload]",
            handler: |args| CommandResult::ShowHooks(args.trim().to_string()),
        });

        // ── Advanced ──────────────────────────────────────────────────

        self.register(Command {
            name: "queue",
            aliases: &[],
            description: "Queue a prompt to run after the current one completes",
            handler: |args| {
                if args.is_empty() {
                    CommandResult::Output(
                        "Usage: /queue <prompt>\n\
                         Queues a prompt to run after the current turn."
                            .into(),
                    )
                } else {
                    CommandResult::QueuePrompt(args.to_string())
                }
            },
        });

        self.register(Command {
            name: "btw",
            aliases: &[],
            description: "Ask an ephemeral side question using the current session context",
            handler: |args| {
                let prompt = args.trim();
                if prompt.is_empty() {
                    CommandResult::Output(
                        "Usage: /btw <question>\n\
                         Example: /btw which module owns session title handling?"
                            .into(),
                    )
                } else {
                    CommandResult::SideQuestion(prompt.to_string())
                }
            },
        });

        self.register(Command {
            name: "background",
            aliases: &["bg"],
            description: "Run a prompt in the background",
            handler: |args| {
                if args.is_empty() {
                    CommandResult::Output(
                        "Usage: /background <prompt>\n\
                         Runs the prompt without blocking the UI."
                            .into(),
                    )
                } else {
                    CommandResult::BackgroundPrompt(args.to_string())
                }
            },
        });

        self.register(Command {
            name: "branch",
            aliases: &["fork"],
            description: "Branch the current session and switch to the new copy",
            handler: |args| {
                let name = args.trim();
                CommandResult::BranchSession((!name.is_empty()).then_some(name.to_string()))
            },
        });

        self.register(Command {
            name: "rollback",
            aliases: &["checkpoint"],
            description: "Restore a file checkpoint from the current session",
            handler: |args| {
                let a = args.trim().to_string();
                CommandResult::RollbackCheckpoint(a)
            },
        });

        // ── Gateway commands ──────────────────────────────────────────

        self.register(Command {
            name: "platforms",
            aliases: &["gw"],
            description: "Show gateway platform status",
            handler: |_| CommandResult::ShowPlatforms,
        });

        self.register(Command {
            name: "gateway",
            aliases: &["gatewayctl"],
            description:
                "Show gateway status or manage the runtime: /gateway [start|stop|restart|status|diagnose]",
            handler: |args| match args.trim().to_ascii_lowercase().as_str() {
                "" => CommandResult::ShowPlatforms,
                "status" => CommandResult::GatewayControl("status".into()),
                "start" => CommandResult::GatewayControl("start".into()),
                "stop" => CommandResult::GatewayControl("stop".into()),
                "restart" => CommandResult::GatewayControl("restart".into()),
                "diagnose" | "diag" => CommandResult::GatewayControl("diagnose".into()),
                other => CommandResult::Output(format!(
                    "Unknown gateway action '{other}'. Use: /gateway [start|stop|restart|status|diagnose]"
                )),
            },
        });

        self.register(Command {
            name: "commands",
            aliases: &[],
            description: "Browse the gateway slash-command catalog",
            handler: |args| CommandResult::ShowGatewayCommands(args.trim().to_string()),
        });

        self.register(Command {
            name: "approve",
            aliases: &["yes"],
            description: "Approve the current prompt: /approve [once|session|always]",
            handler: |args| {
                let choice = match args.trim().to_ascii_lowercase().as_str() {
                    "" | "once" => edgecrab_core::ApprovalChoice::Once,
                    "session" => edgecrab_core::ApprovalChoice::Session,
                    "always" => edgecrab_core::ApprovalChoice::Always,
                    other => {
                        return CommandResult::Output(format!(
                            "Unknown approve scope '{other}'. Use: /approve [once|session|always]"
                        ));
                    }
                };
                CommandResult::ApprovalChoice(choice)
            },
        });

        self.register(Command {
            name: "deny",
            aliases: &["no"],
            description: "Deny the current approval or clarify prompt",
            handler: |_| CommandResult::ApprovalChoice(edgecrab_core::ApprovalChoice::Deny),
        });

        self.register(Command {
            name: "sethome",
            aliases: &["set-home"],
            description: "Show or set gateway home channels: /sethome [platform] <channel|clear>",
            handler: |args| CommandResult::SetHomeChannel(args.trim().to_string()),
        });

        self.register(Command {
            name: "webhook",
            aliases: &[],
            description: "Manage dynamic gateway webhook subscriptions",
            handler: |args| match crate::webhook_cmd::command_from_slash_args(args) {
                Ok(command) => CommandResult::WebhookCommand(command),
                Err(err) => CommandResult::Output(err),
            },
        });

        self.register(Command {
            name: "update",
            aliases: &[],
            description: "Check release status and show channel-aware update guidance",
            handler: |_| CommandResult::CheckUpdates,
        });

        self.register(Command {
            name: "yolo",
            aliases: &[],
            description: "Toggle YOLO mode (skip dangerous command approvals for this session)",
            handler: |args| CommandResult::SetYolo(args.trim().to_string()),
        });

        self.register(Command {
            name: "shadow-judge",
            aliases: &["sj", "shadow_judge"],
            description: "Toggle shadow judge completion oracle (on/off/toggle/status)",
            handler: |args| CommandResult::SetShadowJudge(args.trim().to_string()),
        });

        // ── Scheduling ────────────────────────────────────────────────

        self.register(Command {
            name: "cron",
            aliases: &["schedule"],
            description: "Manage scheduled/recurring tasks",
            handler: |args| CommandResult::ShowCronStatus(args.to_string()),
        });

        // ── Media ─────────────────────────────────────────────────────

        self.register(Command {
            name: "voice",
            aliases: &["tts"],
            description: "Voice tools: spoken readback, mic recording, continuous capture, and transcription",
            handler: |args| CommandResult::VoiceMode(args.trim().to_string()),
        });

        // ── Browser ──────────────────────────────────────────────────

        self.register(Command {
            name: "browser",
            aliases: &[],
            description: "Manage Chrome CDP: connect, disconnect, status, tabs, recording on|off",
            handler: |args| CommandResult::BrowserCommand(args.trim().to_string()),
        });

        // ── Skills ───────────────────────────────────────────────────

        self.register(Command {
            name: "skills",
            aliases: &["skill"],
            description: "Browse & search skills (or /skills <subcommand>)",
            handler: |args| {
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    CommandResult::SkillSelector
                } else {
                    CommandResult::ShowSkills(trimmed.to_string())
                }
            },
        });

        self.register(Command {
            name: "profile",
            aliases: &[],
            description: "Inspect or manage profiles (or /profile <subcommand>)",
            handler: |args| CommandResult::ShowProfiles(args.trim().to_string()),
        });

        self.register(Command {
            name: "profiles",
            aliases: &[],
            description: "Browse and switch profiles",
            handler: |args| {
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    CommandResult::ProfileSelector
                } else {
                    CommandResult::ShowProfiles(trimmed.to_string())
                }
            },
        });

        // ── Appearance (extended) ─────────────────────────────────────

        self.register(Command {
            name: "paste",
            aliases: &[],
            description: "Paste clipboard text and attach to next message",
            handler: |_| CommandResult::PasteClipboard,
        });

        self.register(Command {
            name: "image",
            aliases: &[],
            description: "Attach a local image file to the next prompt",
            handler: |args| {
                let path = args.trim();
                if path.is_empty() {
                    CommandResult::Output(
                        "Usage: /image <path-to-image>\nAttaches the local image to your next prompt."
                            .into(),
                    )
                } else {
                    CommandResult::AttachImagePath(path.to_string())
                }
            },
        });
    }
}

fn provider_help_text() -> String {
    fn discovery_note(provider: &str) -> String {
        match live_discovery_availability(provider) {
            DiscoveryAvailability::Supported => "live discovery".to_string(),
            DiscoveryAvailability::FeatureGated(feature) => {
                format!("live discovery via `{feature}`")
            }
            DiscoveryAvailability::Unsupported => "static catalog".to_string(),
        }
    }

    format!(
        "Available providers (set via env vars):\n\
         \n\
         copilot          — GitHub Copilot (GITHUB_COPILOT_TOKEN or VS Code IPC, {})\n\
         openai           — OpenAI (OPENAI_API_KEY, {})\n\
         anthropic        — Anthropic Claude (ANTHROPIC_API_KEY, {})\n\
         google           — Google Gemini AI Studio (GEMINI_API_KEY or GOOGLE_API_KEY, {})\n\
         vertexai         — Google VertexAI / Gemini (gcloud ADC + GOOGLE_CLOUD_PROJECT, {})\n\
                            GOOGLE_CLOUD_PROJECT is auto-detected from `gcloud config` if unset.\n\
                            Usage: vertexai/gemini-2.5-flash\n\
         bedrock          — AWS Bedrock (AWS credential chain + AWS_REGION, {})\n\
         azure            — Azure OpenAI (AZURE_OPENAI_API_KEY + endpoint, static catalog)\n\
         xai              — xAI Grok (XAI_API_KEY, static catalog)\n\
         mistral          — Mistral (MISTRAL_API_KEY, static catalog)\n\
         groq             — Groq (GROQ_API_KEY, static catalog)\n\
         cohere           — Cohere (COHERE_API_KEY, static catalog)\n\
         perplexity       — Perplexity (PERPLEXITY_API_KEY, static catalog)\n\
         deepseek         — DeepSeek (DEEPSEEK_API_KEY, static catalog)\n\
         ollama           — Ollama (local, OLLAMA_BASE_URL or OLLAMA_HOST, {})\n\
         lmstudio         — LM Studio (local, LMSTUDIO_BASE_URL or LMSTUDIO_HOST, {})\n\
         openrouter       — OpenRouter (OPENROUTER_API_KEY, {})\n\
         \nUsage: /model <provider>/<model-name>",
        discovery_note("copilot"),
        discovery_note("openai"),
        discovery_note("anthropic"),
        discovery_note("google"),
        discovery_note("vertexai"),
        discovery_note("bedrock"),
        discovery_note("ollama"),
        discovery_note("lmstudio"),
        discovery_note("openrouter"),
    )
}

fn format_help_section(commands: &[&SlashCommandSpec]) -> String {
    let width = commands
        .iter()
        .map(|cmd| cmd.label_with_aliases().len())
        .max()
        .unwrap_or(0)
        + 2;

    let mut text = String::new();
    for command in commands {
        let label = command.label_with_aliases();
        text.push_str(&format!("  {label:<width$} - {}\n", command.description));
    }
    text
}

fn help_text() -> String {
    let mut text = String::from("EdgeCrab slash commands:\n\n");
    for (category, commands) in grouped_slash_commands_for_surface(SlashSurface::Cli) {
        text.push_str(category);
        text.push_str(":\n");
        text.push_str(&format_help_section(&commands));
        if category == "Diagnostics" && cfg!(target_os = "macos") {
            text.push_str(
                "  /permissions, /perm [mode] - Inspect or bootstrap macOS permissions\n",
            );
        }
        text.push('\n');
    }

    text.push_str(
        "Keyboard shortcuts:\n\
          F2                   — Open model selector\n\
          F3                   — Open skill selector\n\
          F7                   — Open vision-model selector\n\
           PgUp / PgDn          — Scroll output up/down\n\
          Ctrl+B / Ctrl+F      — Fallback PageUp / PageDown (works when the terminal swallows page keys)\n\
           Shift+↑ / Shift+↓   — Scroll output 3 rows\n\
           Alt+↑ / Alt+↓       — Scroll output 5 rows\n\
                                 Ctrl+M               — Toggle mouse capture / selection mode\n\
           Ctrl+Home            — Jump to top of output\n\
           Ctrl+End / Ctrl+G    — Jump to bottom (live view)\n\
           Ctrl+C               — Clear input / cancel / exit\n\
           Ctrl+D               — Exit (on empty input)\n\
           Ctrl+L               — Clear screen\n\
           Ctrl+U               — Clear input\n\
           Ctrl+O               — Open compose editor + insert newline\n\
           Ctrl+J               — Open compose editor when the terminal distinguishes it from Enter\n\
           Ctrl+S               — Send from compose editor\n\
           Tab                  — Tab completion\n\
           ↑/↓                 — Command history\n\
           →                   — Accept ghost hint\n\
           Shift+Enter*         — Open compose editor + insert newline\n\
           Esc / hjkl / wbe    — Compose normal mode (basic Vim editing)\n\
         \n\
         * Shift+Enter requires terminal keyboard enhancement support; on basic terminals Enter sends and Ctrl+O is the safe compose fallback.",
    );
    text
}

pub fn gateway_commands_page(page: usize, skill_names: &[String]) -> String {
    const PAGE_SIZE: usize = 12;

    let registry = CommandRegistry::new();
    let mut entries: Vec<(String, String)> = registry
        .gateway_entries()
        .into_iter()
        .map(|(name, description)| (format!("/{name}"), description.to_string()))
        .collect();

    let mut skills: Vec<String> = skill_names
        .iter()
        .filter(|name| !name.trim().is_empty())
        .map(|name| format!("/{}", name.trim()))
        .collect();
    skills.sort();
    skills.dedup();
    entries.extend(
        skills
            .into_iter()
            .map(|name| (name, "Installed skill command".to_string())),
    );
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let total_pages = entries.len().max(1).div_ceil(PAGE_SIZE);
    let page = page.clamp(1, total_pages);
    let start = (page - 1) * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(entries.len());

    let mut text = format!("Gateway commands page {page}/{total_pages}\n");
    for (name, description) in &entries[start..end] {
        text.push_str(&format!("{name:<18} {description}\n"));
    }
    if total_pages > 1 {
        text.push_str("\nUse /commands <page> to browse more.");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_help() {
        let reg = CommandRegistry::new();
        let result = reg.dispatch("/help");
        assert!(matches!(result, Some(CommandResult::Output(_))));
    }

    #[test]
    fn dispatch_alias() {
        let reg = CommandRegistry::new();
        let result = reg.dispatch("/q");
        assert!(matches!(result, Some(CommandResult::Exit)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dispatch_permissions_subcommand() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/perm bootstrap") {
            Some(CommandResult::MacosPermissions(args)) => assert_eq!(args, "bootstrap"),
            other => panic!("expected macos permissions command, got {other:?}"),
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn permissions_command_is_not_registered() {
        let reg = CommandRegistry::new();
        assert!(reg.dispatch("/perm bootstrap").is_none());
        assert!(!reg.all_names().contains(&"perm"));
        assert!(!reg.all_names().contains(&"permissions"));
    }

    #[test]
    fn dispatch_unknown() {
        // Unrecognised slash tokens return None — process_input() is
        // responsible for distinguishing skills from true unknowns.
        let reg = CommandRegistry::new();
        let result = reg.dispatch("/bogus");
        assert!(
            result.is_none(),
            "expected None for unknown command, got {result:?}"
        );
    }

    #[test]
    fn non_slash_returns_none() {
        let reg = CommandRegistry::new();
        assert!(reg.dispatch("hello world").is_none());
    }

    #[test]
    fn list_commands_sorted() {
        let reg = CommandRegistry::new();
        let cmds = reg.list();
        assert!(
            cmds.len() >= 42,
            "expected at least 42 commands, got {}",
            cmds.len()
        );
        // Check sorted
        let names: Vec<_> = cmds.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn dispatch_model_empty_activates_selector() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/model") {
            Some(CommandResult::ModelSelector) => {} // correct
            _ => panic!("expected model selector"),
        }
    }

    #[test]
    fn dispatch_model_with_name_returns_switch() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/model openai/gpt-4o") {
            Some(CommandResult::ModelSwitch(m)) => assert_eq!(m, "openai/gpt-4o"),
            _ => panic!("expected model switch"),
        }
    }

    #[test]
    fn dispatch_vision_model_empty_opens_selector() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/vision_model"),
            Some(CommandResult::VisionModelSelector)
        ));
    }

    #[test]
    fn dispatch_cheap_model_commands() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/cheap_model"),
            Some(CommandResult::CheapModelSelector)
        ));
        assert!(matches!(
            reg.dispatch("/cheap_model status"),
            Some(CommandResult::ShowCheapModel)
        ));
        match reg.dispatch("/cheap_model copilot/gpt-4.1-mini") {
            Some(CommandResult::SetCheapModel(model)) => {
                assert_eq!(model, "copilot/gpt-4.1-mini")
            }
            _ => panic!("expected cheap model override"),
        }
    }

    #[test]
    fn dispatch_vision_model_with_name_updates_override() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/vision_model openai/gpt-4o") {
            Some(CommandResult::SetVisionModel(model)) => assert_eq!(model, "openai/gpt-4o"),
            _ => panic!("expected vision model override"),
        }
    }

    #[test]
    fn dispatch_vision_model_status_shows_status() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/vision_model status"),
            Some(CommandResult::ShowVisionModel)
        ));
    }

    #[test]
    fn provider_help_mentions_bedrock() {
        let help = provider_help_text();
        assert!(help.contains("bedrock"));
        assert!(help.contains("AWS Bedrock"));
    }

    #[test]
    fn dispatch_image_model_empty_shows_status() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/image_model"),
            Some(CommandResult::ImageModelSelector)
        ));
    }

    #[test]
    fn dispatch_image_model_list_opens_selector() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/image_model list"),
            Some(CommandResult::ImageModelSelector)
        ));
    }

    #[test]
    fn dispatch_image_model_status_shows_status() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/image_model status"),
            Some(CommandResult::ShowImageModel)
        ));
    }

    #[test]
    fn dispatch_image_model_with_name_updates_override() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/image_model gemini/gemini-2.5-flash-image") {
            Some(CommandResult::SetImageModel(model)) => {
                assert_eq!(model, "gemini/gemini-2.5-flash-image")
            }
            _ => panic!("expected image model override"),
        }
    }

    #[test]
    fn dispatch_moa_commands() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/moa"),
            Some(CommandResult::ShowMoaConfig)
        ));
        assert!(matches!(
            reg.dispatch("/moa on"),
            Some(CommandResult::SetMoaEnabled(true))
        ));
        assert!(matches!(
            reg.dispatch("/moa off"),
            Some(CommandResult::SetMoaEnabled(false))
        ));
        assert!(matches!(
            reg.dispatch("/moa aggregator"),
            Some(CommandResult::MoaAggregatorSelector)
        ));
        assert!(matches!(
            reg.dispatch("/moa references"),
            Some(CommandResult::MoaReferenceSelector)
        ));
        assert!(matches!(
            reg.dispatch("/moa experts"),
            Some(CommandResult::MoaReferenceSelector)
        ));
        match reg.dispatch("/moa aggregator anthropic/claude-opus-4.6") {
            Some(CommandResult::SetMoaAggregator(model)) => {
                assert_eq!(model, "anthropic/claude-opus-4.6")
            }
            _ => panic!("expected moa aggregator override"),
        }
        match reg.dispatch("/moa add openai/gpt-4.1") {
            Some(CommandResult::AddMoaReference(model)) => {
                assert_eq!(model, "openai/gpt-4.1")
            }
            _ => panic!("expected moa add"),
        }
        match reg.dispatch("/moa add") {
            Some(CommandResult::AddMoaReference(model)) => assert!(model.is_empty()),
            _ => panic!("expected empty moa add"),
        }
        match reg.dispatch("/moa remove openai/gpt-4.1") {
            Some(CommandResult::RemoveMoaReference(model)) => {
                assert_eq!(model, "openai/gpt-4.1")
            }
            _ => panic!("expected moa remove"),
        }
        match reg.dispatch("/moa remove") {
            Some(CommandResult::RemoveMoaReference(model)) => assert!(model.is_empty()),
            _ => panic!("expected empty moa remove"),
        }
    }

    #[test]
    fn dispatch_mcp_passes_args_through() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/mcp search github") {
            Some(CommandResult::ShowMcp(args)) => assert_eq!(args, "search github"),
            _ => panic!("expected mcp command"),
        }
    }

    #[test]
    fn dispatch_new_returns_session_new() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/new"),
            Some(CommandResult::SessionNew)
        ));
    }

    #[test]
    fn dispatch_retry_returns_retry() {
        let reg = CommandRegistry::new();
        assert!(matches!(reg.dispatch("/retry"), Some(CommandResult::Retry)));
    }

    #[test]
    fn dispatch_undo_returns_undo() {
        let reg = CommandRegistry::new();
        assert!(matches!(reg.dispatch("/undo"), Some(CommandResult::Undo)));
    }

    #[test]
    fn dispatch_stop_returns_stop() {
        let reg = CommandRegistry::new();
        assert!(matches!(reg.dispatch("/stop"), Some(CommandResult::Stop)));
    }

    #[test]
    fn dispatch_compress_returns_compress() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/compress"),
            Some(CommandResult::Compress)
        ));
    }

    #[test]
    fn dispatch_provider_lists_providers() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/provider") {
            Some(CommandResult::Output(s)) => {
                assert!(s.contains("copilot"));
                assert!(s.contains("openai"));
                assert!(s.contains("anthropic"));
            }
            _ => panic!("expected output"),
        }
    }

    #[test]
    fn dispatch_version_shows_version() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/version") {
            Some(CommandResult::Output(s)) => assert!(s.contains("EdgeCrab")),
            _ => panic!("expected output"),
        }
    }

    #[test]
    fn dispatch_toolsets_opens_tool_manager() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/toolsets") {
            Some(CommandResult::ToolManager(ToolManagerMode::Toolsets)) => {}
            other => panic!("expected ToolManager(Toolsets), got {other:?}"),
        }
    }

    #[test]
    fn dispatch_tools_reset_resets_policy() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/tools reset") {
            Some(CommandResult::ResetToolPolicy) => {}
            other => panic!("expected ResetToolPolicy, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_plugins_toggle_opens_overlay() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/plugins toggle") {
            Some(CommandResult::ShowPluginToggle {
                name: None,
                platform: None,
            }) => {}
            other => panic!("expected ShowPluginToggle, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_plugins_without_args_opens_overlay() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/plugins") {
            Some(CommandResult::ShowPluginToggle {
                name: None,
                platform: None,
            }) => {}
            other => panic!("expected ShowPluginToggle, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_plugins_list_alias_opens_overlay() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/plugins list") {
            Some(CommandResult::ShowPluginToggle {
                name: None,
                platform: None,
            }) => {}
            other => panic!("expected ShowPluginToggle, got {other:?}"),
        }
    }

    #[test]
    fn help_text_mentions_all_categories() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/help") {
            Some(CommandResult::Output(s)) => {
                assert!(s.contains("Model"));
                assert!(s.contains("Session"));
                assert!(s.contains("Config"));
                assert!(s.contains("Memory"));
                assert!(s.contains("Analysis"));
                assert!(s.contains("Advanced"));
                assert!(s.contains("Gateway"));
                assert!(s.contains("Diagnostics"));
                if cfg!(target_os = "macos") {
                    assert!(s.contains("/permissions, /perm"));
                } else {
                    assert!(!s.contains("/permissions, /perm"));
                }
            }
            _ => panic!("expected help output"),
        }
    }

    #[test]
    fn dispatch_reasoning_levels() {
        let reg = CommandRegistry::new();
        for level in &["low", "medium", "high", "show", "hide", "status"] {
            match reg.dispatch(&format!("/reasoning {level}")) {
                Some(CommandResult::SetReasoning(s)) => {
                    assert_eq!(s, *level);
                }
                _ => panic!("expected SetReasoning for /reasoning {level}"),
            }
        }
    }

    #[test]
    fn dispatch_streaming_modes() {
        let reg = CommandRegistry::new();
        for mode in &["on", "off", "toggle", "status"] {
            match reg.dispatch(&format!("/stream {mode}")) {
                Some(CommandResult::SetStreaming(s)) => assert_eq!(s, *mode),
                _ => panic!("expected SetStreaming for /stream {mode}"),
            }
            match reg.dispatch(&format!("/streaming {mode}")) {
                Some(CommandResult::SetStreaming(s)) => assert_eq!(s, *mode),
                _ => panic!("expected SetStreaming for /streaming {mode}"),
            }
        }
    }

    #[test]
    fn dispatch_title_set() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/title My Session") {
            Some(CommandResult::SetTitle(s)) => assert_eq!(s, "My Session"),
            _ => panic!("expected SetTitle"),
        }
    }

    #[test]
    fn dispatch_platforms_shows_gateway() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/platforms"),
            Some(CommandResult::ShowPlatforms)
        ));
    }

    #[test]
    fn dispatch_gateway_restart() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/gateway restart"),
            Some(CommandResult::GatewayControl(action)) if action == "restart"
        ));
    }

    #[test]
    fn dispatch_voice_toggle() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/voice status") {
            Some(CommandResult::VoiceMode(s)) => assert!(s.contains("status")),
            _ => panic!("expected VoiceMode"),
        }
    }

    #[test]
    fn dispatch_cron_list() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/cron list") {
            Some(CommandResult::ShowCronStatus(s)) => assert_eq!(s.trim(), "list"),
            other => panic!("expected ShowCronStatus, got {:?}", other.is_some()),
        }
    }

    #[test]
    fn dispatch_approve_deny() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/approve"),
            Some(CommandResult::ApprovalChoice(
                edgecrab_core::ApprovalChoice::Once
            ))
        ));
        assert!(matches!(
            reg.dispatch("/approve session"),
            Some(CommandResult::ApprovalChoice(
                edgecrab_core::ApprovalChoice::Session
            ))
        ));
        assert!(matches!(
            reg.dispatch("/deny"),
            Some(CommandResult::ApprovalChoice(
                edgecrab_core::ApprovalChoice::Deny
            ))
        ));
    }

    #[test]
    fn dispatch_config_and_statusbar_commands() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/config"),
            Some(CommandResult::ShowConfig(args)) if args.is_empty()
        ));
        assert!(matches!(
            reg.dispatch("/config paths"),
            Some(CommandResult::ShowConfig(args)) if args == "paths"
        ));
        assert!(matches!(
            reg.dispatch("/statusbar off"),
            Some(CommandResult::SetStatusBar(args)) if args == "off"
        ));
    }

    #[test]
    fn dispatch_worktree_commands() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/worktree"),
            Some(CommandResult::WorktreeCommand(args)) if args.is_empty()
        ));
        assert!(matches!(
            reg.dispatch("/worktree on"),
            Some(CommandResult::WorktreeCommand(args)) if args == "on"
        ));
        assert!(matches!(
            reg.dispatch("/w toggle"),
            Some(CommandResult::WorktreeCommand(args)) if args == "toggle"
        ));
    }

    #[test]
    fn dispatch_log_commands() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/log"),
            Some(CommandResult::LogCommand(args)) if args.is_empty()
        ));
        assert!(matches!(
            reg.dispatch("/log level debug"),
            Some(CommandResult::LogCommand(args)) if args == "level debug"
        ));
        assert!(matches!(
            reg.dispatch("/logs trace"),
            Some(CommandResult::LogCommand(args)) if args == "trace"
        ));
    }

    #[test]
    fn dispatch_skin_no_args_opens_browser_and_reload_is_explicit() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/skin"),
            Some(CommandResult::SwitchSkin(name)) if name.is_empty()
        ));
        assert!(matches!(
            reg.dispatch("/skin reload"),
            Some(CommandResult::ReloadTheme)
        ));
    }

    #[test]
    fn dispatch_queue_with_prompt() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/queue fix the bug") {
            Some(CommandResult::QueuePrompt(s)) => assert_eq!(s, "fix the bug"),
            _ => panic!("expected QueuePrompt"),
        }
    }

    #[test]
    fn dispatch_all_new_commands_exist() {
        let reg = CommandRegistry::new();
        let all_names = reg.all_names();
        for command in slash_commands_for_surface(SlashSurface::Cli) {
            assert!(
                all_names.contains(&command.name),
                "missing canonical command {}",
                command.name
            );
            for alias in command.aliases {
                assert!(all_names.contains(alias), "missing alias {alias}");
            }
        }
    }

    // ── Phase 8.1: tests for new CommandResult variants ─────────────

    #[test]
    fn dispatch_status_returns_show_status() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/status"),
            Some(CommandResult::ShowStatus)
        ));
    }

    #[test]
    fn dispatch_cost_returns_show_cost() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/cost"),
            Some(CommandResult::ShowCost)
        ));
    }

    #[test]
    fn dispatch_usage_returns_show_usage() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/usage"),
            Some(CommandResult::ShowUsage)
        ));
    }

    #[test]
    fn dispatch_insights_supports_optional_days() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/insights"),
            Some(CommandResult::ShowInsights(30))
        ));
        assert!(matches!(
            reg.dispatch("/insights 14"),
            Some(CommandResult::ShowInsights(14))
        ));
        assert!(matches!(
            reg.dispatch("/insights nope"),
            Some(CommandResult::Output(msg)) if msg == "Usage: /insights [days]"
        ));
    }

    #[test]
    fn dispatch_prompt_returns_show_prompt() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/prompt"),
            Some(CommandResult::PromptCommand(args)) if args.is_empty()
        ));
        // aliases
        assert!(matches!(
            reg.dispatch("/sys"),
            Some(CommandResult::PromptCommand(args)) if args.is_empty()
        ));
        assert!(matches!(
            reg.dispatch("/system"),
            Some(CommandResult::PromptCommand(args)) if args.is_empty()
        ));
        assert!(matches!(
            reg.dispatch("/prompt clear"),
            Some(CommandResult::PromptCommand(args)) if args == "clear"
        ));
    }

    #[test]
    fn dispatch_history_returns_show_history() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/history"),
            Some(CommandResult::ShowHistory)
        ));
    }

    #[test]
    fn dispatch_verbose_returns_toggle() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/verbose"),
            Some(CommandResult::ToggleVerbose)
        ));
        assert!(matches!(
            reg.dispatch("/v"),
            Some(CommandResult::ToggleVerbose)
        ));
        assert!(matches!(
            reg.dispatch("/verbose status"),
            Some(CommandResult::SetToolProgress(mode)) if mode == "status"
        ));
        assert!(matches!(
            reg.dispatch("/verbose all"),
            Some(CommandResult::SetToolProgress(mode)) if mode == "all"
        ));
    }

    #[test]
    fn dispatch_save_returns_save_session() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/save"),
            Some(CommandResult::SaveSession(None))
        ));
        match reg.dispatch("/save /tmp/out.json") {
            Some(CommandResult::SaveSession(Some(p))) => assert_eq!(p, "/tmp/out.json"),
            _ => panic!("expected SaveSession with path"),
        }
    }

    #[test]
    fn dispatch_export_returns_export_session() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/export"),
            Some(CommandResult::ExportSession(None))
        ));
        match reg.dispatch("/export /tmp/conv.md") {
            Some(CommandResult::ExportSession(Some(p))) => assert_eq!(p, "/tmp/conv.md"),
            _ => panic!("expected ExportSession with path"),
        }
    }

    #[test]
    fn dispatch_session_defaults_to_current_inspector() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/session"),
            Some(CommandResult::InspectCurrentSession)
        ));
        assert!(matches!(
            reg.dispatch("/session debug"),
            Some(CommandResult::InspectCurrentSession)
        ));
    }

    #[test]
    fn dispatch_sessions_list() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/sessions"),
            Some(CommandResult::SessionBrowse(None))
        ));
        assert!(matches!(
            reg.dispatch("/sessions list"),
            Some(CommandResult::SessionBrowse(None))
        ));
        assert!(matches!(
            reg.dispatch("/sessions ls"),
            Some(CommandResult::SessionBrowse(None))
        ));
    }

    #[test]
    fn dispatch_session_browse_and_search() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/sessions browse"),
            Some(CommandResult::SessionBrowse(None))
        ));
        match reg.dispatch("/sessions browse websocket jitter") {
            Some(CommandResult::SessionBrowse(Some(query))) => {
                assert_eq!(query, "websocket jitter");
            }
            _ => panic!("expected SessionBrowse with query"),
        }
        match reg.dispatch("/sessions search oauth") {
            Some(CommandResult::SessionBrowse(Some(query))) => assert_eq!(query, "oauth"),
            _ => panic!("expected SessionBrowse with query"),
        }
    }

    #[test]
    fn dispatch_session_switch() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/sessions switch abc123") {
            Some(CommandResult::SessionSwitch(id)) => assert_eq!(id, "abc123"),
            _ => panic!("expected SessionSwitch"),
        }
    }

    #[test]
    fn dispatch_session_delete() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/sessions delete abc123") {
            Some(CommandResult::SessionDelete(id)) => assert_eq!(id, "abc123"),
            _ => panic!("expected SessionDelete"),
        }
    }

    #[test]
    fn dispatch_resume_no_args() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/resume"),
            Some(CommandResult::ResumeSession(None))
        ));
    }

    #[test]
    fn dispatch_resume_with_id() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/resume abc") {
            Some(CommandResult::ResumeSession(Some(id))) => assert_eq!(id, "abc"),
            _ => panic!("expected ResumeSession"),
        }
    }

    #[test]
    fn dispatch_background_with_prompt() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/background do something") {
            Some(CommandResult::BackgroundPrompt(s)) => assert_eq!(s, "do something"),
            _ => panic!("expected BackgroundPrompt"),
        }
    }

    #[test]
    fn dispatch_background_empty_shows_usage() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/background"),
            Some(CommandResult::Output(_))
        ));
    }

    #[test]
    fn dispatch_skills_no_args_returns_skill_selector() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/skills"),
            Some(CommandResult::SkillSelector)
        ));
    }

    #[test]
    fn dispatch_skills_with_args_returns_show_skills() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/skills list") {
            Some(CommandResult::ShowSkills(args)) => assert_eq!(args, "list"),
            _ => panic!("expected ShowSkills(list)"),
        }
    }

    #[test]
    fn dispatch_skills_alias_no_args_returns_skill_selector() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/skill"),
            Some(CommandResult::SkillSelector)
        ));
    }

    #[test]
    fn dispatch_profile_no_args_returns_show_profiles() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/profile") {
            Some(CommandResult::ShowProfiles(args)) => assert!(args.is_empty()),
            _ => panic!("expected ShowProfiles"),
        }
    }

    #[test]
    fn dispatch_profiles_no_args_returns_profile_selector() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/profiles"),
            Some(CommandResult::ProfileSelector)
        ));
    }

    #[test]
    fn dispatch_profiles_with_args_returns_show_profiles() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/profiles use work") {
            Some(CommandResult::ShowProfiles(args)) => assert_eq!(args, "use work"),
            _ => panic!("expected ShowProfiles(use work)"),
        }
    }

    #[test]
    fn dispatch_mouse_mode_variants() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/mouse") {
            Some(CommandResult::MouseMode(args)) => assert!(args.is_empty()),
            _ => panic!("expected MouseMode for /mouse"),
        }
        match reg.dispatch("/mouse off") {
            Some(CommandResult::MouseMode(args)) => assert_eq!(args, "off"),
            _ => panic!("expected MouseMode for /mouse off"),
        }
        match reg.dispatch("/mouse status") {
            Some(CommandResult::MouseMode(args)) => assert_eq!(args, "status"),
            _ => panic!("expected MouseMode for /mouse status"),
        }
    }

    #[test]
    fn dispatch_auth_commands() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/auth status provider/openai") {
            Some(CommandResult::AuthCommand(crate::cli_args::AuthCommand::Status { target })) => {
                assert_eq!(target.as_deref(), Some("provider/openai"))
            }
            _ => panic!("expected AuthCommand::Status"),
        }
        match reg.dispatch("/login") {
            Some(CommandResult::LoginTarget(target)) => assert_eq!(target, "copilot"),
            _ => panic!("expected LoginTarget for bare /login"),
        }
        match reg.dispatch("/login copilot") {
            Some(CommandResult::LoginTarget(target)) => assert_eq!(target, "copilot"),
            _ => panic!("expected LoginTarget"),
        }
        match reg.dispatch("/logout provider/openai") {
            Some(CommandResult::LogoutTarget(target)) => {
                assert_eq!(target.as_deref(), Some("provider/openai"))
            }
            _ => panic!("expected LogoutTarget"),
        }
    }

    #[test]
    fn dispatch_webhook_and_uninstall_commands() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/webhook list") {
            Some(CommandResult::WebhookCommand(crate::cli_args::WebhookCommand::List)) => {}
            _ => panic!("expected WebhookCommand::List"),
        }
        match reg.dispatch("/uninstall --dry-run --purge-data") {
            Some(CommandResult::UninstallCommand(options)) => {
                assert!(options.dry_run);
                assert!(options.purge_data);
            }
            _ => panic!("expected UninstallCommand"),
        }
    }

    #[test]
    fn dispatch_hooks_command() {
        let reg = CommandRegistry::new();
        match reg.dispatch("/hooks") {
            Some(CommandResult::ShowHooks(args)) => assert!(args.is_empty()),
            _ => panic!("expected ShowHooks for bare /hooks"),
        }
        match reg.dispatch("/hooks list") {
            Some(CommandResult::ShowHooks(args)) => assert_eq!(args, "list"),
            _ => panic!("expected ShowHooks for /hooks list"),
        }
    }
}
