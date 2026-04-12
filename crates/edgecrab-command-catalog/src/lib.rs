#![deny(clippy::unwrap_used)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashSurface {
    Cli,
    Gateway,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub category: &'static str,
    pub args_hint: &'static str,
    pub cli: bool,
    pub gateway: bool,
}

impl SlashCommandSpec {
    pub const fn available_on(&self, surface: SlashSurface) -> bool {
        match surface {
            SlashSurface::Cli => self.cli,
            SlashSurface::Gateway => self.gateway,
        }
    }

    pub fn slash_names(&self) -> impl Iterator<Item = String> + '_ {
        std::iter::once(format!("/{}", self.name))
            .chain(self.aliases.iter().map(|alias| format!("/{alias}")))
    }

    pub fn primary_label(&self) -> String {
        if self.args_hint.is_empty() {
            format!("/{}", self.name)
        } else {
            format!("/{} {}", self.name, self.args_hint)
        }
    }

    pub fn aliases_label(&self) -> String {
        if self.aliases.is_empty() {
            String::new()
        } else {
            self.aliases
                .iter()
                .map(|alias| format!("/{alias}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    pub fn label_with_aliases(&self) -> String {
        let mut label = self.primary_label();
        let aliases = self.aliases_label();
        if !aliases.is_empty() {
            label.push_str(", ");
            label.push_str(&aliases);
        }
        label
    }
}

pub const CLI_HELP_CATEGORY_ORDER: &[&str] = &[
    "Navigation",
    "Model",
    "Session",
    "Config",
    "Tools",
    "Memory & Skills",
    "Analysis",
    "Advanced",
    "Gateway",
    "Scheduling & Media",
    "Appearance",
    "Diagnostics",
];

pub const BUILTIN_SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        name: "help",
        aliases: &["h", "?"],
        description: "Show available commands",
        category: "Navigation",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "quit",
        aliases: &["exit", "q"],
        description: "Exit EdgeCrab",
        category: "Navigation",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "clear",
        aliases: &["cls"],
        description: "Clear the output area",
        category: "Navigation",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "version",
        aliases: &["ver"],
        description: "Show version and build info",
        category: "Navigation",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "status",
        aliases: &["stat"],
        description: "Show current session status",
        category: "Navigation",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "model",
        aliases: &[],
        description: "Show or switch the active model",
        category: "Model",
        args_hint: "[name]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "cheap_model",
        aliases: &["cheap-model"],
        description: "Open, show, or set the cheap routing model",
        category: "Model",
        args_hint: "[spec]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "vision_model",
        aliases: &["vision-model"],
        description: "Open, show, or set the vision model",
        category: "Model",
        args_hint: "[spec]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "image_model",
        aliases: &["image-model"],
        description: "Open, show, or set the image generation model",
        category: "Model",
        args_hint: "[spec]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "moa",
        aliases: &[],
        description: "Configure Mixture-of-Agents defaults",
        category: "Model",
        args_hint: "[subcommand]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "models",
        aliases: &[],
        description: "List available models",
        category: "Model",
        args_hint: "[provider]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "provider",
        aliases: &["providers"],
        description: "Show provider availability",
        category: "Model",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "reasoning",
        aliases: &["think"],
        description: "Set reasoning effort or display mode",
        category: "Model",
        args_hint: "[mode]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "stream",
        aliases: &["streaming"],
        description: "Toggle live token streaming",
        category: "Model",
        args_hint: "[mode]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "new",
        aliases: &["reset"],
        description: "Start a fresh conversation",
        category: "Session",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "session",
        aliases: &[],
        description: "Inspect or switch the live session",
        category: "Session",
        args_hint: "[id]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "sessions",
        aliases: &[],
        description: "Browse and manage saved sessions",
        category: "Session",
        args_hint: "[browse]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "retry",
        aliases: &["r"],
        description: "Retry the last user message",
        category: "Session",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "undo",
        aliases: &["u"],
        description: "Undo the last user and assistant turn",
        category: "Session",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "stop",
        aliases: &["cancel", "interrupt"],
        description: "Stop the current agent response",
        category: "Session",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "btw",
        aliases: &[],
        description: "Ask an ephemeral side question",
        category: "Session",
        args_hint: "<question>",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "history",
        aliases: &[],
        description: "Show session history summary",
        category: "Session",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "save",
        aliases: &[],
        description: "Save the conversation to JSON",
        category: "Session",
        args_hint: "[path]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "export",
        aliases: &[],
        description: "Export the conversation as Markdown",
        category: "Session",
        args_hint: "[path]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "title",
        aliases: &[],
        description: "Show or set the session title",
        category: "Session",
        args_hint: "[name]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "resume",
        aliases: &[],
        description: "Resume a persisted session",
        category: "Session",
        args_hint: "[id]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "branch",
        aliases: &["fork"],
        description: "Fork the current session into a new branch",
        category: "Session",
        args_hint: "[name]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "config",
        aliases: &["cfg"],
        description: "Open config center or inspect config",
        category: "Config",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "prompt",
        aliases: &["sys", "system"],
        description: "Show the assembled system prompt",
        category: "Config",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "verbose",
        aliases: &["v"],
        description: "Cycle tool progress display",
        category: "Config",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "personality",
        aliases: &["persona"],
        description: "Show or switch the active personality",
        category: "Config",
        args_hint: "[name]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "statusbar",
        aliases: &["sb"],
        description: "Show or set status bar visibility",
        category: "Config",
        args_hint: "[mode]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "log",
        aliases: &["logs"],
        description: "Browse local logs or set the saved log level",
        category: "Config",
        args_hint: "[open|level <level>]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "worktree",
        aliases: &["w"],
        description: "Inspect or configure the saved worktree launch policy",
        category: "Config",
        args_hint: "[status|on|off|toggle]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "yolo",
        aliases: &[],
        description: "Toggle dangerous-command approval bypass",
        category: "Config",
        args_hint: "[mode]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "tools",
        aliases: &[],
        description: "Browse and configure tools",
        category: "Tools",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "toolsets",
        aliases: &["ts"],
        description: "Browse and configure toolsets",
        category: "Tools",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "mcp",
        aliases: &[],
        description: "Search, install, test, or remove MCP servers",
        category: "Tools",
        args_hint: "[args]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "reload-mcp",
        aliases: &["mcp-reload", "reload_mcp"],
        description: "Reload MCP server connections",
        category: "Tools",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "mcp-token",
        aliases: &[],
        description: "Manage MCP bearer tokens",
        category: "Tools",
        args_hint: "[args]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "plugins",
        aliases: &["plugin"],
        description: "Browse installed plugins",
        category: "Tools",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "memory",
        aliases: &["mem"],
        description: "Show memory status",
        category: "Memory & Skills",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "profile",
        aliases: &[],
        description: "Show or manage the active profile",
        category: "Memory & Skills",
        args_hint: "[args]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "profiles",
        aliases: &[],
        description: "Browse and switch profiles",
        category: "Memory & Skills",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "skills",
        aliases: &["skill"],
        description: "List and manage skills",
        category: "Memory & Skills",
        args_hint: "[args]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "cost",
        aliases: &[],
        description: "Show token usage and estimated cost",
        category: "Analysis",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "usage",
        aliases: &[],
        description: "Show usage and session stats",
        category: "Analysis",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "compress",
        aliases: &["compact"],
        description: "Manually compress the live context",
        category: "Analysis",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "insights",
        aliases: &[],
        description: "Show session and historical analytics",
        category: "Analysis",
        args_hint: "[days]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "queue",
        aliases: &[],
        description: "Queue a prompt for later execution",
        category: "Advanced",
        args_hint: "<prompt>",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "background",
        aliases: &["bg"],
        description: "Run a prompt in a background session",
        category: "Advanced",
        args_hint: "<prompt>",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "rollback",
        aliases: &[],
        description: "List or restore checkpoints",
        category: "Advanced",
        args_hint: "[name]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "platforms",
        aliases: &["gw"],
        description: "Show gateway platform status",
        category: "Gateway",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "gateway",
        aliases: &["gatewayctl"],
        description: "Show gateway status or control the runtime",
        category: "Gateway",
        args_hint: "[action]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "commands",
        aliases: &[],
        description: "Browse gateway slash commands and skills",
        category: "Gateway",
        args_hint: "[page]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "approve",
        aliases: &["yes"],
        description: "Approve the oldest pending action",
        category: "Gateway",
        args_hint: "[scope]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "deny",
        aliases: &["no"],
        description: "Deny the oldest pending action",
        category: "Gateway",
        args_hint: "",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "sethome",
        aliases: &["set-home"],
        description: "Show or set gateway home channels",
        category: "Gateway",
        args_hint: "[args]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "webhook",
        aliases: &[],
        description: "Manage dynamic webhook subscriptions",
        category: "Gateway",
        args_hint: "[subcommand]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "update",
        aliases: &[],
        description: "Check release status and update guidance",
        category: "Gateway",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "cron",
        aliases: &["schedule"],
        description: "Manage scheduled tasks",
        category: "Scheduling & Media",
        args_hint: "[subcommand]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "voice",
        aliases: &["tts"],
        description: "Control voice mode and spoken replies",
        category: "Scheduling & Media",
        args_hint: "[subcommand]",
        cli: true,
        gateway: true,
    },
    SlashCommandSpec {
        name: "browser",
        aliases: &[],
        description: "Manage the browser CDP connection",
        category: "Scheduling & Media",
        args_hint: "[sub]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "skin",
        aliases: &["theme"],
        description: "Reload, browse, or switch skins",
        category: "Appearance",
        args_hint: "[name]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "mouse",
        aliases: &["scroll"],
        description: "Toggle mouse capture mode",
        category: "Appearance",
        args_hint: "[mode]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "paste",
        aliases: &[],
        description: "Paste clipboard text or image",
        category: "Appearance",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "image",
        aliases: &[],
        description: "Attach a local image file to the next prompt",
        category: "Appearance",
        args_hint: "<path>",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "doctor",
        aliases: &["diag"],
        description: "Run diagnostics",
        category: "Diagnostics",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "copilot-auth",
        aliases: &["copilot-login", "gh-auth"],
        description: "Run GitHub Copilot authentication",
        category: "Diagnostics",
        args_hint: "",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "auth",
        aliases: &[],
        description: "Inspect or mutate auth state",
        category: "Diagnostics",
        args_hint: "[subcommand]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "login",
        aliases: &[],
        description: "Run one auth login/import flow",
        category: "Diagnostics",
        args_hint: "<target>",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "logout",
        aliases: &[],
        description: "Clear one or all local auth caches",
        category: "Diagnostics",
        args_hint: "[target]",
        cli: true,
        gateway: false,
    },
    SlashCommandSpec {
        name: "uninstall",
        aliases: &[],
        description: "Preview or execute local uninstall actions",
        category: "Diagnostics",
        args_hint: "[flags]",
        cli: true,
        gateway: false,
    },
];

pub fn grouped_slash_commands_for_surface(
    surface: SlashSurface,
) -> Vec<(&'static str, Vec<&'static SlashCommandSpec>)> {
    let mut grouped = Vec::new();
    for category in CLI_HELP_CATEGORY_ORDER {
        let commands = BUILTIN_SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.category == *category && cmd.available_on(surface))
            .collect::<Vec<_>>();
        if !commands.is_empty() {
            grouped.push((*category, commands));
        }
    }
    grouped
}

pub fn slash_commands_for_surface(surface: SlashSurface) -> Vec<&'static SlashCommandSpec> {
    BUILTIN_SLASH_COMMANDS
        .iter()
        .filter(|cmd| cmd.available_on(surface))
        .collect()
}

pub fn resolve_slash_command(name: &str) -> Option<&'static SlashCommandSpec> {
    let needle = name.trim().trim_start_matches('/');
    BUILTIN_SLASH_COMMANDS.iter().find(|cmd| {
        cmd.name.eq_ignore_ascii_case(needle)
            || cmd
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(needle))
    })
}

#[cfg(test)]
mod tests {
    use super::{SlashSurface, resolve_slash_command, slash_commands_for_surface};

    #[test]
    fn hermes_reference_commands_are_present_with_expected_aliases_and_hints() {
        // Source of law: hermes_cli/commands.py in the upstream Hermes repo.
        for name in [
            "new",
            "clear",
            "history",
            "save",
            "retry",
            "undo",
            "title",
            "branch",
            "compress",
            "rollback",
            "stop",
            "approve",
            "deny",
            "background",
            "btw",
            "queue",
            "status",
            "profile",
            "sethome",
            "resume",
            "config",
            "model",
            "provider",
            "prompt",
            "personality",
            "statusbar",
            "worktree",
            "verbose",
            "yolo",
            "reasoning",
            "skin",
            "voice",
            "tools",
            "toolsets",
            "skills",
            "cron",
            "reload-mcp",
            "browser",
            "plugins",
            "commands",
            "help",
            "usage",
            "insights",
            "platforms",
            "paste",
            "update",
            "quit",
        ] {
            let _ = resolve_slash_command(name).unwrap_or_else(|| panic!("missing /{name}"));
        }

        for (name, alias) in [
            ("new", "reset"),
            ("branch", "fork"),
            ("background", "bg"),
            ("prompt", "sys"),
            ("prompt", "system"),
            ("statusbar", "sb"),
            ("worktree", "w"),
            ("reasoning", "think"),
            ("reload-mcp", "reload_mcp"),
            ("quit", "exit"),
            ("quit", "q"),
        ] {
            let alias_spec =
                resolve_slash_command(alias).unwrap_or_else(|| panic!("missing alias /{alias}"));
            assert_eq!(
                alias_spec.name, name,
                "alias /{alias} does not resolve to /{name}"
            );
        }

        for (name, args_hint) in [
            ("branch", "[name]"),
            ("background", "<prompt>"),
            ("btw", "<question>"),
            ("queue", "<prompt>"),
            ("insights", "[days]"),
        ] {
            let spec = resolve_slash_command(name).unwrap_or_else(|| panic!("missing /{name}"));
            assert_eq!(spec.args_hint, args_hint, "args hint drifted for /{name}");
        }
    }

    #[test]
    fn hermes_reference_commands_are_reachable_on_cli_surface() {
        let cli_names = slash_commands_for_surface(SlashSurface::Cli)
            .into_iter()
            .map(|spec| spec.name)
            .collect::<std::collections::HashSet<_>>();

        for name in [
            "new",
            "clear",
            "history",
            "retry",
            "undo",
            "btw",
            "profile",
            "config",
            "model",
            "provider",
            "prompt",
            "personality",
            "statusbar",
            "worktree",
            "verbose",
            "yolo",
            "reasoning",
            "voice",
            "tools",
            "toolsets",
            "skills",
            "cron",
            "reload-mcp",
            "browser",
            "plugins",
            "help",
            "usage",
            "insights",
            "platforms",
            "paste",
            "quit",
        ] {
            assert!(
                cli_names.contains(name),
                "/{name} missing from CLI slash surface"
            );
        }
    }
}
