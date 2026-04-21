use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::Value as JsonValue;

use crate::common::{
    ENTRY_DELIMITER, MergeStats, backup_existing, copy_dir_recursive, copy_path, ensure_dir,
    ensure_parent, extract_markdown_entries, load_yaml_file, merge_entries, parse_env_file,
    parse_existing_memory_entries, relative_label, save_env_file, save_yaml_file,
};
use crate::report::{MigrationItem, MigrationReport, MigrationStatus};

const DEFAULT_MEMORY_CHAR_LIMIT: usize = 2200;
const DEFAULT_USER_CHAR_LIMIT: usize = 1375;
const SKILL_CATEGORY_DIRNAME: &str = "openclaw-imports";
const SKILL_CATEGORY_DESCRIPTION: &str = "Skills migrated from an OpenClaw workspace.\n";
const SUPPORTED_SECRET_TARGETS: &[&str] = &[
    "TELEGRAM_BOT_TOKEN",
    "OPENROUTER_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "ELEVENLABS_API_KEY",
    "VOICE_TOOLS_OPENAI_KEY",
];
const OPENCLAW_DIR_NAMES: &[&str] = &[".openclaw", ".clawdbot", ".moldbot"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenClawPreset {
    UserData,
    Full,
}

impl OpenClawPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserData => "user-data",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillConflictMode {
    Skip,
    Overwrite,
    Rename,
}

impl SkillConflictMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Overwrite => "overwrite",
            Self::Rename => "rename",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenClawMigrationOptions {
    pub execute: bool,
    pub overwrite: bool,
    pub migrate_secrets: bool,
    pub preset: OpenClawPreset,
    pub workspace_target: Option<PathBuf>,
    pub skill_conflict_mode: SkillConflictMode,
}

impl Default for OpenClawMigrationOptions {
    fn default() -> Self {
        Self {
            execute: true,
            overwrite: false,
            migrate_secrets: false,
            preset: OpenClawPreset::UserData,
            workspace_target: None,
            skill_conflict_mode: SkillConflictMode::Skip,
        }
    }
}

pub struct OpenClawMigrator {
    source_root: PathBuf,
    target_root: PathBuf,
    options: OpenClawMigrationOptions,
    output_root: PathBuf,
}

impl OpenClawMigrator {
    pub fn new(
        source_root: PathBuf,
        target_root: PathBuf,
        options: OpenClawMigrationOptions,
    ) -> Self {
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let output_root = target_root
            .join("migration")
            .join("openclaw")
            .join(timestamp);
        Self {
            source_root,
            target_root,
            options,
            output_root,
        }
    }

    pub fn migrate_all(&self) -> anyhow::Result<MigrationReport> {
        let mut report = MigrationReport::new("openclaw → EdgeCrab");

        self.ensure_target_root()?;

        let config = self.load_openclaw_config();
        report.add(self.migrate_soul());
        report.add(self.migrate_workspace_agents());
        report.add(self.migrate_memory_file(
            self.source_candidate(&["workspace/MEMORY.md", "workspace.default/MEMORY.md"]),
            self.target_root.join("memories").join("MEMORY.md"),
            DEFAULT_MEMORY_CHAR_LIMIT,
            "memory",
        ));
        report.add(self.migrate_memory_file(
            self.source_candidate(&["workspace/USER.md", "workspace.default/USER.md"]),
            self.target_root.join("memories").join("USER.md"),
            DEFAULT_USER_CHAR_LIMIT,
            "user-profile",
        ));
        report.add(self.migrate_tts_assets());
        report.add(self.migrate_daily_memory());
        report.add(self.migrate_skills());
        report.add(self.migrate_shared_skills());
        report.add(self.migrate_command_allowlist());
        report.add(self.migrate_messaging_settings(&config));
        report.add(self.migrate_discord_settings(&config));
        report.add(self.migrate_slack_settings(&config));
        report.add(self.migrate_whatsapp_settings(&config));
        report.add(self.migrate_signal_settings(&config));
        report.add(self.handle_secret_settings(&config));
        report.add(self.handle_provider_keys(&config));
        report.add(self.migrate_model_config(&config));
        report.add(self.migrate_tts_config(&config));
        report.add(self.migrate_mcp_servers(&config));
        report.add(self.migrate_agent_config(&config));
        report.add(self.migrate_tools_config(&config));
        report.add(self.archive_config_section(
            "gateway-config",
            config.get("gateway"),
            "gateway-config.json",
            "gateway configuration archived for manual recreation",
        ));
        report.add(self.archive_config_section(
            "session-config",
            config.get("session"),
            "session-config.json",
            "session configuration archived for manual recreation",
        ));
        report.add(self.archive_config_section(
            "browser-config",
            config.get("browser"),
            "browser-config.json",
            "browser configuration archived because EdgeCrab does not import OpenClaw browser transport settings directly",
        ));
        report.add(self.archive_config_section(
            "approvals-config",
            config.get("approvals"),
            "approvals-config.json",
            "approval rules archived for manual review",
        ));
        report.add(self.archive_config_section(
            "skills-config",
            config.get("skills"),
            "skills-config.json",
            "skills registry configuration archived for manual review",
        ));
        report.add(self.archive_config_section(
            "memory-backend",
            config.get("memory"),
            "memory-backend.json",
            "memory backend configuration archived for manual review",
        ));
        report.add(self.archive_config_section(
            "ui-identity",
            config.get("ui"),
            "ui-identity.json",
            "UI and identity settings archived for manual review",
        ));
        report.add(self.archive_logging_config(&config));
        report.add(self.archive_openclaw_docs());

        Ok(report)
    }

    pub fn default_source_home() -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        OPENCLAW_DIR_NAMES
            .iter()
            .map(|name| home.join(name))
            .find(|candidate| candidate.is_dir())
    }

    pub fn known_source_homes() -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        OPENCLAW_DIR_NAMES
            .iter()
            .map(|name| home.join(name))
            .filter(|candidate| candidate.is_dir())
            .collect()
    }

    fn ensure_target_root(&self) -> anyhow::Result<()> {
        ensure_dir(&self.target_root)?;
        ensure_dir(&self.target_root.join("memories"))?;
        ensure_dir(&self.target_root.join("skills"))?;
        Ok(())
    }

    fn source_candidate(&self, relative_paths: &[&str]) -> Option<PathBuf> {
        relative_paths
            .iter()
            .map(|relative| self.source_root.join(relative))
            .find(|candidate| candidate.exists())
    }

    fn load_openclaw_config(&self) -> JsonValue {
        for name in ["openclaw.json", "clawdbot.json", "moldbot.json"] {
            let path = self.source_root.join(name);
            if !path.exists() {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<JsonValue>(&raw) else {
                continue;
            };
            if value.is_object() {
                return value;
            }
        }
        JsonValue::Object(Default::default())
    }

    fn load_openclaw_env(&self) -> BTreeMap<String, String> {
        parse_env_file(&self.source_root.join(".env"))
    }

    fn archive_dir(&self) -> PathBuf {
        self.output_root.join("archive")
    }

    fn backup_root(&self) -> PathBuf {
        self.output_root.join("backups")
    }

    fn maybe_backup(&self, path: &Path) -> anyhow::Result<Option<PathBuf>> {
        if !self.options.execute || !self.options.overwrite {
            return Ok(None);
        }
        backup_existing(path, &self.backup_root())
    }

    fn maybe_write_skill_category_description(&self) -> anyhow::Result<()> {
        let description_path = self
            .target_root
            .join("skills")
            .join(SKILL_CATEGORY_DIRNAME)
            .join("DESCRIPTION.md");
        if description_path.exists() || !self.options.execute {
            return Ok(());
        }
        ensure_parent(&description_path)?;
        std::fs::write(description_path, SKILL_CATEGORY_DESCRIPTION)?;
        Ok(())
    }

    fn migrate_soul(&self) -> MigrationItem {
        let Some(source) =
            self.source_candidate(&["workspace/SOUL.md", "workspace.default/SOUL.md"])
        else {
            return MigrationItem::skipped("soul", "no OpenClaw SOUL.md found");
        };
        self.copy_file_item(&source, &self.target_root.join("SOUL.md"), "soul")
    }

    fn migrate_workspace_agents(&self) -> MigrationItem {
        let Some(source) =
            self.source_candidate(&["workspace/AGENTS.md", "workspace.default/AGENTS.md"])
        else {
            return MigrationItem::skipped("workspace-agents", "no workspace AGENTS.md found");
        };
        let Some(workspace_target) = self.options.workspace_target.as_ref() else {
            return MigrationItem::skipped(
                "workspace-agents",
                "no workspace target was provided; use --workspace-target to import AGENTS.md",
            );
        };
        self.copy_file_item(
            &source,
            &workspace_target.join("AGENTS.md"),
            "workspace-agents",
        )
    }

    fn migrate_memory_file(
        &self,
        source: Option<PathBuf>,
        destination: PathBuf,
        limit: usize,
        kind: &str,
    ) -> MigrationItem {
        let Some(source) = source else {
            return MigrationItem::skipped(kind, "source file not found");
        };
        let Ok(raw) = std::fs::read_to_string(&source) else {
            return MigrationItem::failed(kind, "failed to read source file");
        };
        let incoming = extract_markdown_entries(&raw);
        if incoming.is_empty() {
            return MigrationItem::skipped(kind, "no importable entries found");
        }

        let existing = parse_existing_memory_entries(&destination);
        let (merged, stats, overflowed) = merge_entries(&existing, &incoming, limit);
        if stats.added == 0 && overflowed.is_empty() {
            return MigrationItem::skipped(kind, "all entries already present");
        }

        if self.options.execute {
            if let Err(error) = self.maybe_backup(&destination) {
                return MigrationItem::failed(kind, &format!("backup failed: {error}"));
            }
            if let Err(error) = ensure_parent(&destination) {
                return MigrationItem::failed(kind, &format!("create parent failed: {error}"));
            }
            let payload = if merged.is_empty() {
                String::new()
            } else {
                format!("{}\n", merged.join(ENTRY_DELIMITER))
            };
            if let Err(error) = std::fs::write(&destination, payload) {
                return MigrationItem::failed(kind, &format!("write failed: {error}"));
            }
            if let Err(error) = self.write_overflow_entries(kind, &overflowed) {
                return MigrationItem::failed(kind, &format!("overflow export failed: {error}"));
            }
        }

        let detail = describe_memory_merge(&destination, stats, &overflowed, self.options.execute);
        MigrationItem::new(kind, MigrationStatus::Success, &detail)
    }

    fn migrate_tts_assets(&self) -> MigrationItem {
        let Some(source_root) = self.source_candidate(&["workspace/tts"]) else {
            return MigrationItem::skipped("tts-assets", "no workspace tts/ directory found");
        };
        let destination_root = self.target_root.join("tts");
        match self.copy_tree_non_destructive(&source_root, &destination_root, "tts-assets") {
            Ok(item) => item,
            Err(error) => MigrationItem::failed("tts-assets", &error.to_string()),
        }
    }

    fn migrate_daily_memory(&self) -> MigrationItem {
        let Some(source_root) = self.source_candidate(&["workspace/memory"]) else {
            return MigrationItem::skipped("daily-memory", "no workspace/memory directory found");
        };
        let Ok(entries) = collect_daily_memory_entries(&source_root) else {
            return MigrationItem::failed("daily-memory", "failed to read daily memory files");
        };
        if entries.is_empty() {
            return MigrationItem::skipped("daily-memory", "no importable entries found");
        }

        let destination = self.target_root.join("memories").join("MEMORY.md");
        let existing = parse_existing_memory_entries(&destination);
        let (merged, stats, overflowed) =
            merge_entries(&existing, &entries, DEFAULT_MEMORY_CHAR_LIMIT);
        if stats.added == 0 && overflowed.is_empty() {
            return MigrationItem::skipped("daily-memory", "all entries already present");
        }

        if self.options.execute {
            if let Err(error) = self.maybe_backup(&destination) {
                return MigrationItem::failed("daily-memory", &format!("backup failed: {error}"));
            }
            if let Err(error) = ensure_parent(&destination) {
                return MigrationItem::failed(
                    "daily-memory",
                    &format!("create parent failed: {error}"),
                );
            }
            let payload = if merged.is_empty() {
                String::new()
            } else {
                format!("{}\n", merged.join(ENTRY_DELIMITER))
            };
            if let Err(error) = std::fs::write(&destination, payload) {
                return MigrationItem::failed("daily-memory", &format!("write failed: {error}"));
            }
            if let Err(error) = self.write_overflow_entries("daily-memory", &overflowed) {
                return MigrationItem::failed(
                    "daily-memory",
                    &format!("overflow export failed: {error}"),
                );
            }
        }

        let detail = describe_memory_merge(&destination, stats, &overflowed, self.options.execute);
        MigrationItem::new("daily-memory", MigrationStatus::Success, &detail)
    }

    fn migrate_skills(&self) -> MigrationItem {
        let Some(source_root) = self.source_candidate(&["workspace/skills"]) else {
            return MigrationItem::skipped("skills", "no OpenClaw workspace skills found");
        };
        match self.import_skill_directory(&source_root, "skills") {
            Ok(item) => item,
            Err(error) => MigrationItem::failed("skills", &error.to_string()),
        }
    }

    fn migrate_shared_skills(&self) -> MigrationItem {
        let mut imported = 0usize;
        let mut conflicts = 0usize;
        let mut renamed = 0usize;
        let mut found_any = false;

        let candidates = [
            self.source_root.join("skills"),
            self.source_root
                .join("workspace")
                .join(".agents")
                .join("skills"),
            self.source_root
                .join("workspace.default")
                .join(".agents")
                .join("skills"),
            dirs::home_dir()
                .map(|home| home.join(".agents").join("skills"))
                .unwrap_or_else(|| PathBuf::from(".agents/skills")),
        ];

        for source_root in candidates {
            if !source_root.exists() {
                continue;
            }
            found_any = true;
            let summary = match self.import_skill_directory(&source_root, "shared-skills") {
                Ok(item) => item,
                Err(error) => return MigrationItem::failed("shared-skills", &error.to_string()),
            };
            imported += count_token(&summary.detail, "imported");
            conflicts += count_token(&summary.detail, "conflict");
            renamed += count_token(&summary.detail, "renamed");
        }

        if !found_any {
            return MigrationItem::skipped(
                "shared-skills",
                "no shared OpenClaw skills directories found",
            );
        }

        let detail = if imported == 0 && conflicts > 0 {
            format!(
                "all shared skills conflicted with existing destinations ({conflicts} conflict(s))"
            )
        } else {
            format!(
                "imported {imported} shared skill(s); {conflicts} conflict(s); {renamed} renamed import(s)"
            )
        };
        let status = if imported == 0 && conflicts > 0 {
            MigrationStatus::Skipped
        } else {
            MigrationStatus::Success
        };
        MigrationItem::new("shared-skills", status, &detail)
    }

    fn import_skill_directory(
        &self,
        source_root: &Path,
        kind: &str,
    ) -> anyhow::Result<MigrationItem> {
        let Ok(entries) = std::fs::read_dir(source_root) else {
            return Ok(MigrationItem::skipped(
                kind,
                "skill source directory is unreadable",
            ));
        };

        let skill_dirs = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir() && path.join("SKILL.md").exists())
            .collect::<Vec<_>>();
        if skill_dirs.is_empty() {
            return Ok(MigrationItem::skipped(
                kind,
                "no skills with SKILL.md found",
            ));
        }

        let destination_root = self.target_root.join("skills").join(SKILL_CATEGORY_DIRNAME);
        let mut imported = 0usize;
        let mut conflicts = 0usize;
        let mut renamed = 0usize;

        for skill_dir in skill_dirs {
            let Some(name) = skill_dir.file_name().and_then(|value| value.to_str()) else {
                conflicts += 1;
                continue;
            };
            let destination = destination_root.join(name);
            let final_destination = if destination.exists() {
                match self.options.skill_conflict_mode {
                    SkillConflictMode::Skip => {
                        conflicts += 1;
                        continue;
                    }
                    SkillConflictMode::Overwrite => destination.clone(),
                    SkillConflictMode::Rename => {
                        renamed += 1;
                        self.resolve_skill_destination(&destination)
                    }
                }
            } else {
                destination.clone()
            };

            if self.options.execute {
                if final_destination == destination {
                    let _ = self.maybe_backup(&destination)?;
                    if destination.exists() {
                        std::fs::remove_dir_all(&destination)?;
                    }
                }
                copy_dir_recursive(&skill_dir, &final_destination)?;
            }
            imported += 1;
        }

        self.maybe_write_skill_category_description()?;

        let detail = format!(
            "imported {imported} skill(s); {conflicts} conflict(s); {renamed} renamed import(s)"
        );
        let status = if imported == 0 && conflicts > 0 {
            MigrationStatus::Skipped
        } else {
            MigrationStatus::Success
        };
        Ok(MigrationItem::new(kind, status, &detail))
    }

    fn migrate_command_allowlist(&self) -> MigrationItem {
        let source = self.source_root.join("exec-approvals.json");
        if !source.exists() {
            return MigrationItem::skipped("command-allowlist", "no exec-approvals.json found");
        }

        let Ok(raw) = std::fs::read_to_string(&source) else {
            return MigrationItem::failed("command-allowlist", "failed to read exec approvals");
        };
        let Ok(json) = serde_json::from_str::<JsonValue>(&raw) else {
            return MigrationItem::failed("command-allowlist", "invalid exec approvals JSON");
        };

        let mut patterns = BTreeSet::new();
        if let Some(agents) = json.get("agents").and_then(JsonValue::as_object) {
            for agent_data in agents.values() {
                let Some(allowlist) = agent_data.get("allowlist").and_then(JsonValue::as_array)
                else {
                    continue;
                };
                for entry in allowlist {
                    if let Some(pattern) = entry.get("pattern").and_then(JsonValue::as_str)
                        && !pattern.trim().is_empty()
                    {
                        patterns.insert(pattern.trim().to_string());
                    }
                }
            }
        }
        if patterns.is_empty() {
            return MigrationItem::skipped("command-allowlist", "no allowlist patterns found");
        }

        let destination = self.target_root.join("command_allowlist.json");
        let current = std::fs::read_to_string(&destination)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default();
        let current_set = current.into_iter().collect::<BTreeSet<_>>();
        let merged = current_set.union(&patterns).cloned().collect::<Vec<_>>();
        let added = patterns
            .difference(&current_set)
            .cloned()
            .collect::<Vec<_>>();
        if added.is_empty() {
            return MigrationItem::skipped("command-allowlist", "all patterns already present");
        }

        if self.options.execute {
            if let Err(error) = self.maybe_backup(&destination) {
                return MigrationItem::failed(
                    "command-allowlist",
                    &format!("backup failed: {error}"),
                );
            }
            if let Err(error) = ensure_parent(&destination) {
                return MigrationItem::failed(
                    "command-allowlist",
                    &format!("create parent failed: {error}"),
                );
            }
            let Ok(payload) = serde_json::to_string_pretty(&merged) else {
                return MigrationItem::failed(
                    "command-allowlist",
                    "serialize merged allowlist failed",
                );
            };
            if let Err(error) = std::fs::write(&destination, format!("{payload}\n")) {
                return MigrationItem::failed(
                    "command-allowlist",
                    &format!("write failed: {error}"),
                );
            }
        }

        MigrationItem::new(
            "command-allowlist",
            MigrationStatus::Success,
            &format!(
                "merged {} new pattern(s) into {}",
                added.len(),
                destination.display()
            ),
        )
    }

    fn migrate_messaging_settings(&self, config: &JsonValue) -> MigrationItem {
        let mut additions = BTreeMap::new();

        if let Some(workspace) = config
            .pointer("/agents/defaults/workspace")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            additions.insert("MESSAGING_CWD".to_string(), workspace.to_string());
        }

        let allowlist_path = self
            .source_root
            .join("credentials")
            .join("telegram-default-allowFrom.json");
        if allowlist_path.exists()
            && let Ok(raw) = std::fs::read_to_string(&allowlist_path)
            && let Ok(value) = serde_json::from_str::<JsonValue>(&raw)
            && let Some(users) = csv_from_json_array(value.get("allowFrom"))
        {
            additions.insert("TELEGRAM_ALLOWED_USERS".to_string(), users);
        }

        self.merge_env_values(
            "messaging-settings",
            self.source_root.join("openclaw.json"),
            additions,
        )
    }

    fn handle_secret_settings(&self, config: &JsonValue) -> MigrationItem {
        if !self.options.migrate_secrets {
            return MigrationItem::skipped(
                "secret-settings",
                "secret migration disabled; re-run with --migrate-secrets to import allowlisted secrets",
            );
        }

        let mut additions = BTreeMap::new();
        if let Some(token) = config
            .pointer("/channels/telegram/botToken")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            additions.insert("TELEGRAM_BOT_TOKEN".to_string(), token.to_string());
        }
        if additions.is_empty() {
            return MigrationItem::skipped(
                "secret-settings",
                &format!(
                    "no allowlisted secrets found ({})",
                    SUPPORTED_SECRET_TARGETS.join(", ")
                ),
            );
        }

        self.merge_env_values(
            "secret-settings",
            self.source_root.join("openclaw.json"),
            additions,
        )
    }

    fn migrate_discord_settings(&self, config: &JsonValue) -> MigrationItem {
        self.merge_channel_env(
            "discord-settings",
            config
                .get("channels")
                .and_then(|channels| channels.get("discord")),
            &[
                ("token", "DISCORD_BOT_TOKEN"),
                ("allowFrom", "DISCORD_ALLOWED_USERS"),
            ],
        )
    }

    fn migrate_slack_settings(&self, config: &JsonValue) -> MigrationItem {
        self.merge_channel_env(
            "slack-settings",
            config
                .get("channels")
                .and_then(|channels| channels.get("slack")),
            &[
                ("botToken", "SLACK_BOT_TOKEN"),
                ("appToken", "SLACK_APP_TOKEN"),
                ("allowFrom", "SLACK_ALLOWED_USERS"),
            ],
        )
    }

    fn migrate_whatsapp_settings(&self, config: &JsonValue) -> MigrationItem {
        self.merge_channel_env(
            "whatsapp-settings",
            config
                .get("channels")
                .and_then(|channels| channels.get("whatsapp")),
            &[("allowFrom", "WHATSAPP_ALLOWED_USERS")],
        )
    }

    fn migrate_signal_settings(&self, config: &JsonValue) -> MigrationItem {
        self.merge_channel_env(
            "signal-settings",
            config
                .get("channels")
                .and_then(|channels| channels.get("signal")),
            &[
                ("account", "SIGNAL_ACCOUNT"),
                ("httpUrl", "SIGNAL_HTTP_URL"),
                ("allowFrom", "SIGNAL_ALLOWED_USERS"),
            ],
        )
    }

    fn handle_provider_keys(&self, config: &JsonValue) -> MigrationItem {
        if !self.options.migrate_secrets {
            return MigrationItem::skipped(
                "provider-keys",
                "secret migration disabled; re-run with --migrate-secrets to import provider API keys",
            );
        }

        let openclaw_env = self.load_openclaw_env();
        let mut additions = BTreeMap::new();

        if let Some(providers) = config
            .pointer("/models/providers")
            .and_then(JsonValue::as_object)
        {
            for (provider_name, provider_cfg) in providers {
                let Some(provider_cfg) = provider_cfg.as_object() else {
                    continue;
                };
                let api_key = resolve_secret_input(provider_cfg.get("apiKey"), &openclaw_env)
                    .or_else(|| resolve_secret_input(provider_cfg.get("api_key"), &openclaw_env));
                let Some(api_key) = api_key else {
                    continue;
                };

                let env_key = provider_env_key(
                    provider_name,
                    provider_cfg.get("baseUrl"),
                    provider_cfg.get("api"),
                );
                if let Some(env_key) = env_key {
                    additions.insert(env_key.to_string(), api_key);
                }
            }
        }

        if let Some(elevenlabs_key) = config
            .pointer("/messages/tts/elevenlabs/apiKey")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            additions.insert("ELEVENLABS_API_KEY".to_string(), elevenlabs_key.to_string());
        }
        if let Some(openai_tts_key) = config
            .pointer("/messages/tts/openai/apiKey")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            additions.insert(
                "VOICE_TOOLS_OPENAI_KEY".to_string(),
                openai_tts_key.to_string(),
            );
        }

        for key in [
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "ELEVENLABS_API_KEY",
            "TELEGRAM_BOT_TOKEN",
            "DEEPSEEK_API_KEY",
            "GEMINI_API_KEY",
            "ZAI_API_KEY",
            "MINIMAX_API_KEY",
        ] {
            if let Some(value) = openclaw_env
                .get(key)
                .filter(|value| !value.trim().is_empty())
            {
                additions
                    .entry(key.to_string())
                    .or_insert_with(|| value.clone());
            }
        }

        if additions.is_empty() {
            return MigrationItem::skipped("provider-keys", "no provider API keys found");
        }

        self.merge_env_values(
            "provider-keys",
            self.source_root.join("openclaw.json"),
            additions,
        )
    }

    fn migrate_model_config(&self, config: &JsonValue) -> MigrationItem {
        let model_value = config.pointer("/agents/defaults/model");
        let model: Option<String> = match model_value {
            Some(JsonValue::String(value)) if !value.trim().is_empty() => {
                Some(value.trim().to_string())
            }
            Some(JsonValue::Object(map)) => map
                .get("primary")
                .and_then(JsonValue::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            _ => None,
        };
        let Some(model) = model else {
            return MigrationItem::skipped("model-config", "no default model found");
        };

        let destination = self.target_root.join("config.yaml");
        let mut yaml = match load_yaml_file(&destination) {
            Ok(value) => value,
            Err(error) => {
                return MigrationItem::failed(
                    "model-config",
                    &format!("load config failed: {error}"),
                );
            }
        };
        let root = ensure_mapping(&mut yaml);
        let model_map = ensure_child_mapping(root, "model");
        let current = model_map
            .get(serde_yml::Value::String("default".into()))
            .and_then(serde_yml::Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        if current == model {
            return MigrationItem::skipped(
                "model-config",
                "model already matches OpenClaw default",
            );
        }
        if !current.is_empty() && !self.options.overwrite {
            return MigrationItem::skipped(
                "model-config",
                "target config already has a different model; re-run with --overwrite to replace it",
            );
        }

        model_map.insert(
            serde_yml::Value::String("default".into()),
            serde_yml::Value::String(model.clone()),
        );
        if self.options.execute {
            if let Err(error) = self.maybe_backup(&destination) {
                return MigrationItem::failed("model-config", &format!("backup failed: {error}"));
            }
            if let Err(error) = save_yaml_file(&destination, &yaml) {
                return MigrationItem::failed(
                    "model-config",
                    &format!("save config failed: {error}"),
                );
            }
        }

        MigrationItem::new(
            "model-config",
            MigrationStatus::Success,
            &format!("set model.default to {model}"),
        )
    }

    fn migrate_tts_config(&self, config: &JsonValue) -> MigrationItem {
        let tts = config
            .pointer("/messages/tts")
            .and_then(JsonValue::as_object);
        let Some(tts) = tts else {
            return MigrationItem::skipped("tts-config", "no TTS configuration found");
        };

        let provider = tts
            .get("provider")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default();

        let providers = tts.get("providers").and_then(JsonValue::as_object);
        let talk = config.get("talk").and_then(JsonValue::as_object);
        let talk_providers = talk
            .and_then(|value| value.get("providers"))
            .and_then(JsonValue::as_object);

        let elevenlabs = providers
            .and_then(|map| map.get("elevenlabs"))
            .or_else(|| talk_providers.and_then(|map| map.get("elevenlabs")))
            .or_else(|| tts.get("elevenlabs"))
            .and_then(JsonValue::as_object);
        let openai_tts = providers
            .and_then(|map| map.get("openai"))
            .or_else(|| talk_providers.and_then(|map| map.get("openai")))
            .or_else(|| tts.get("openai"))
            .and_then(JsonValue::as_object);
        let edge_tts = providers
            .and_then(|map| map.get("edge"))
            .or_else(|| tts.get("edge"))
            .and_then(JsonValue::as_object);

        let destination = self.target_root.join("config.yaml");
        let mut yaml = match load_yaml_file(&destination) {
            Ok(value) => value,
            Err(error) => {
                return MigrationItem::failed(
                    "tts-config",
                    &format!("load config failed: {error}"),
                );
            }
        };
        let root = ensure_mapping(&mut yaml);
        let tts_map = ensure_child_mapping(root, "tts");
        let mut changed = false;

        if !provider.is_empty() {
            let mapped_provider = if provider == "edge" {
                "edge-tts"
            } else {
                provider
            };
            changed |= set_yaml_string(tts_map, "provider", mapped_provider.to_string());
        }
        if let Some(settings) = elevenlabs {
            changed |= set_yaml_string_opt(
                tts_map,
                "elevenlabs_voice_id",
                json_str(settings.get("voiceId")),
            );
            changed |= set_yaml_string_opt(
                tts_map,
                "elevenlabs_model_id",
                json_str(settings.get("modelId")),
            );
        }
        if let Some(settings) = openai_tts {
            changed |= set_yaml_string_opt(
                tts_map,
                "model",
                json_str(settings.get("model")).or_else(|| json_str(settings.get("modelId"))),
            );
            changed |= set_yaml_string_opt(tts_map, "voice", json_str(settings.get("voice")));
        }
        if let Some(settings) = edge_tts {
            changed |= set_yaml_string_opt(tts_map, "voice", json_str(settings.get("voice")));
        }
        if !changed {
            return MigrationItem::skipped("tts-config", "no compatible TTS settings found");
        }

        if self.options.execute {
            if let Err(error) = self.maybe_backup(&destination) {
                return MigrationItem::failed("tts-config", &format!("backup failed: {error}"));
            }
            if let Err(error) = save_yaml_file(&destination, &yaml) {
                return MigrationItem::failed(
                    "tts-config",
                    &format!("save config failed: {error}"),
                );
            }
        }

        MigrationItem::success("tts-config")
    }

    fn migrate_mcp_servers(&self, config: &JsonValue) -> MigrationItem {
        let Some(servers) = config
            .pointer("/mcp/servers")
            .and_then(JsonValue::as_object)
        else {
            return MigrationItem::skipped("mcp-servers", "no MCP servers found");
        };

        let destination = self.target_root.join("config.yaml");
        let mut yaml = match load_yaml_file(&destination) {
            Ok(value) => value,
            Err(error) => {
                return MigrationItem::failed(
                    "mcp-servers",
                    &format!("load config failed: {error}"),
                );
            }
        };
        let root = ensure_mapping(&mut yaml);
        let mcp_map = ensure_child_mapping(root, "mcp_servers");
        let mut added = 0usize;
        let mut conflicts = 0usize;

        for (name, server) in servers {
            let Some(server) = server.as_object() else {
                continue;
            };
            let key = serde_yml::Value::String(name.clone());
            if mcp_map.contains_key(&key) && !self.options.overwrite {
                conflicts += 1;
                continue;
            }

            let mut server_yaml = serde_yml::Mapping::new();
            copy_json_string(server, "command", &mut server_yaml, "command");
            copy_json_string_list(server, "args", &mut server_yaml, "args");
            copy_json_object_strings(server, "env", &mut server_yaml, "env");
            copy_json_string(server, "cwd", &mut server_yaml, "cwd");
            copy_json_string(server, "url", &mut server_yaml, "url");
            copy_json_object_strings(server, "headers", &mut server_yaml, "headers");
            copy_json_bool(server, "enabled", &mut server_yaml, "enabled");
            copy_json_u64(server, "timeout", &mut server_yaml, "timeout");
            copy_json_u64(
                server,
                "connectTimeout",
                &mut server_yaml,
                "connect_timeout",
            );

            if let Some(tools) = server.get("tools").and_then(JsonValue::as_object) {
                let mut tools_yaml = serde_yml::Mapping::new();
                copy_json_string_list(tools, "include", &mut tools_yaml, "include");
                copy_json_string_list(tools, "exclude", &mut tools_yaml, "exclude");
                if !tools_yaml.is_empty() {
                    server_yaml.insert(
                        serde_yml::Value::String("tools".into()),
                        serde_yml::Value::Mapping(tools_yaml),
                    );
                }
            }

            mcp_map.insert(key, serde_yml::Value::Mapping(server_yaml));
            added += 1;
        }

        if added == 0 && conflicts > 0 {
            return MigrationItem::skipped(
                "mcp-servers",
                "all MCP servers conflicted with existing config",
            );
        }
        if added == 0 {
            return MigrationItem::skipped("mcp-servers", "no MCP servers were importable");
        }

        if self.options.execute {
            if let Err(error) = self.maybe_backup(&destination) {
                return MigrationItem::failed("mcp-servers", &format!("backup failed: {error}"));
            }
            if let Err(error) = save_yaml_file(&destination, &yaml) {
                return MigrationItem::failed(
                    "mcp-servers",
                    &format!("save config failed: {error}"),
                );
            }
        }

        MigrationItem::new(
            "mcp-servers",
            MigrationStatus::Success,
            &format!("imported {added} MCP server(s); {conflicts} conflict(s)"),
        )
    }

    fn migrate_agent_config(&self, config: &JsonValue) -> MigrationItem {
        let defaults = config
            .pointer("/agents/defaults")
            .and_then(JsonValue::as_object);
        let Some(defaults) = defaults else {
            return MigrationItem::skipped("agent-config", "no agent defaults found");
        };

        let destination = self.target_root.join("config.yaml");
        let mut yaml = match load_yaml_file(&destination) {
            Ok(value) => value,
            Err(error) => {
                return MigrationItem::failed(
                    "agent-config",
                    &format!("load config failed: {error}"),
                );
            }
        };
        let root = ensure_mapping(&mut yaml);
        let mut changed = false;

        if let Some(timezone) = defaults
            .get("userTimezone")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            changed |= set_yaml_root_string(root, "timezone", timezone.to_string());
        }
        if let Some(thinking) = defaults
            .get("thinkingDefault")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let reasoning_effort = match thinking {
                "always" | "high" => "high",
                "auto" | "medium" => "medium",
                "off" | "low" | "none" => "low",
                _ => "",
            };
            if !reasoning_effort.is_empty() {
                changed |=
                    set_yaml_root_string(root, "reasoning_effort", reasoning_effort.to_string());
            }
        }

        if !changed {
            return MigrationItem::skipped(
                "agent-config",
                "no directly mappable agent settings found",
            );
        }

        if self.options.execute {
            if let Err(error) = self.maybe_backup(&destination) {
                return MigrationItem::failed("agent-config", &format!("backup failed: {error}"));
            }
            if let Err(error) = save_yaml_file(&destination, &yaml) {
                return MigrationItem::failed(
                    "agent-config",
                    &format!("save config failed: {error}"),
                );
            }
        }

        MigrationItem::success("agent-config")
    }

    fn migrate_tools_config(&self, config: &JsonValue) -> MigrationItem {
        let Some(timeout) = config
            .pointer("/tools/exec/timeoutSec")
            .and_then(JsonValue::as_u64)
            .or_else(|| {
                config
                    .pointer("/tools/exec/timeout")
                    .and_then(JsonValue::as_u64)
            })
        else {
            return MigrationItem::skipped(
                "tools-config",
                "no directly mappable tools settings found",
            );
        };

        let destination = self.target_root.join("config.yaml");
        let mut yaml = match load_yaml_file(&destination) {
            Ok(value) => value,
            Err(error) => {
                return MigrationItem::failed(
                    "tools-config",
                    &format!("load config failed: {error}"),
                );
            }
        };
        let root = ensure_mapping(&mut yaml);
        let terminal = ensure_child_mapping(root, "terminal");
        let changed = set_yaml_u64(terminal, "timeout", timeout);
        if !changed {
            return MigrationItem::skipped("tools-config", "terminal timeout already matches");
        }

        if self.options.execute {
            if let Err(error) = self.maybe_backup(&destination) {
                return MigrationItem::failed("tools-config", &format!("backup failed: {error}"));
            }
            if let Err(error) = save_yaml_file(&destination, &yaml) {
                return MigrationItem::failed(
                    "tools-config",
                    &format!("save config failed: {error}"),
                );
            }
        }

        MigrationItem::new(
            "tools-config",
            MigrationStatus::Success,
            &format!("set terminal.timeout to {timeout}"),
        )
    }

    fn archive_config_section(
        &self,
        kind: &str,
        value: Option<&JsonValue>,
        file_name: &str,
        reason: &str,
    ) -> MigrationItem {
        let Some(value) = value else {
            return MigrationItem::skipped(kind, "no matching OpenClaw configuration found");
        };
        if value.is_null() || value == &JsonValue::Object(Default::default()) {
            return MigrationItem::skipped(kind, "no matching OpenClaw configuration found");
        }

        let destination = self.archive_dir().join(file_name);
        if self.options.execute {
            if let Err(error) = ensure_parent(&destination) {
                return MigrationItem::failed(kind, &format!("create archive dir failed: {error}"));
            }
            let Ok(payload) = serde_json::to_string_pretty(value) else {
                return MigrationItem::failed(kind, "serialize archive payload failed");
            };
            if let Err(error) = std::fs::write(&destination, format!("{payload}\n")) {
                return MigrationItem::failed(kind, &format!("write archive failed: {error}"));
            }
        }

        MigrationItem::new(
            kind,
            MigrationStatus::Success,
            &format!("{reason}; archived to {}", destination.display()),
        )
    }

    fn archive_logging_config(&self, config: &JsonValue) -> MigrationItem {
        let mut payload = serde_json::Map::new();
        if let Some(logging) = config.get("logging").filter(|value| !value.is_null()) {
            payload.insert("logging".into(), logging.clone());
        }
        if let Some(diagnostics) = config.get("diagnostics").filter(|value| !value.is_null()) {
            payload.insert("diagnostics".into(), diagnostics.clone());
        }
        if payload.is_empty() {
            return MigrationItem::skipped(
                "logging-config",
                "no logging or diagnostics configuration found",
            );
        }

        let value = JsonValue::Object(payload);
        self.archive_config_section(
            "logging-config",
            Some(&value),
            "logging-diagnostics.json",
            "logging and diagnostics configuration archived for manual review",
        )
    }

    fn archive_openclaw_docs(&self) -> MigrationItem {
        let candidates = [
            "workspace/IDENTITY.md",
            "workspace.default/IDENTITY.md",
            "workspace/TOOLS.md",
            "workspace.default/TOOLS.md",
            "workspace/HEARTBEAT.md",
            "workspace.default/HEARTBEAT.md",
            "workspace/BOOTSTRAP.md",
            "workspace.default/BOOTSTRAP.md",
        ];

        let existing = candidates
            .iter()
            .map(|relative| self.source_root.join(relative))
            .filter(|path| path.exists())
            .collect::<Vec<_>>();
        if existing.is_empty() {
            return MigrationItem::skipped(
                "archive",
                "no supplemental OpenClaw docs found to archive",
            );
        }

        let mut archived = 0usize;
        for source in existing {
            let destination = self
                .archive_dir()
                .join(relative_label(&source, &self.source_root));
            if self.options.execute
                && let Err(error) = copy_path(&source, &destination)
            {
                return MigrationItem::failed("archive", &format!("archive copy failed: {error}"));
            }
            archived += 1;
        }

        MigrationItem::new(
            "archive",
            MigrationStatus::Success,
            &format!("archived {archived} supplemental OpenClaw document(s)"),
        )
    }

    fn merge_channel_env(
        &self,
        kind: &str,
        channel: Option<&JsonValue>,
        mappings: &[(&str, &str)],
    ) -> MigrationItem {
        let Some(channel) = channel.and_then(JsonValue::as_object) else {
            return MigrationItem::skipped(kind, "no channel settings found");
        };

        let mut additions = BTreeMap::new();
        for (json_key, env_key) in mappings {
            if *json_key == "allowFrom" {
                if let Some(value) = csv_from_json_array(channel.get(*json_key)) {
                    additions.insert((*env_key).to_string(), value);
                }
                continue;
            }
            if let Some(value) = channel
                .get(*json_key)
                .and_then(JsonValue::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                additions.insert((*env_key).to_string(), value.to_string());
            }
        }

        self.merge_env_values(kind, self.source_root.join("openclaw.json"), additions)
    }

    fn merge_env_values(
        &self,
        kind: &str,
        _source: PathBuf,
        additions: BTreeMap<String, String>,
    ) -> MigrationItem {
        if additions.is_empty() {
            return MigrationItem::skipped(kind, "no compatible values found");
        }

        let destination = self.target_root.join(".env");
        let mut env_data = parse_env_file(&destination);
        let mut added = Vec::new();
        let mut conflicts = Vec::new();

        for (key, value) in additions {
            match env_data.get(&key) {
                Some(current) if current == &value => {}
                Some(_) if !self.options.overwrite => conflicts.push(key),
                _ => {
                    env_data.insert(key.clone(), value);
                    added.push(key);
                }
            }
        }

        if added.is_empty() && conflicts.is_empty() {
            return MigrationItem::skipped(kind, "all values already present");
        }
        if added.is_empty() && !conflicts.is_empty() {
            return MigrationItem::skipped(
                kind,
                &format!("conflict on existing keys: {}", conflicts.join(", ")),
            );
        }

        if self.options.execute {
            if let Err(error) = self.maybe_backup(&destination) {
                return MigrationItem::failed(kind, &format!("backup failed: {error}"));
            }
            if let Err(error) = save_env_file(&destination, &env_data) {
                return MigrationItem::failed(kind, &format!("save env failed: {error}"));
            }
        }

        let mut detail = format!("merged {} env key(s)", added.len());
        if !conflicts.is_empty() {
            detail.push_str(&format!("; skipped conflicts: {}", conflicts.join(", ")));
        }
        MigrationItem::new(kind, MigrationStatus::Success, &detail)
    }

    fn copy_file_item(&self, source: &Path, destination: &Path, kind: &str) -> MigrationItem {
        if destination.exists() && !self.options.overwrite {
            return MigrationItem::skipped(
                kind,
                "destination already exists; re-run with --overwrite to replace it",
            );
        }
        if self.options.execute {
            if let Err(error) = self.maybe_backup(destination) {
                return MigrationItem::failed(kind, &format!("backup failed: {error}"));
            }
            if let Err(error) = copy_path(source, destination) {
                return MigrationItem::failed(kind, &format!("copy failed: {error}"));
            }
        }
        MigrationItem::new(
            kind,
            MigrationStatus::Success,
            &format!("copied {} to {}", source.display(), destination.display()),
        )
    }

    fn copy_tree_non_destructive(
        &self,
        source_root: &Path,
        destination_root: &Path,
        kind: &str,
    ) -> anyhow::Result<MigrationItem> {
        let files = collect_files_recursive(source_root)?;
        if files.is_empty() {
            return Ok(MigrationItem::skipped(kind, "no files found"));
        }

        let mut copied = 0usize;
        let mut conflicts = 0usize;
        let mut unchanged = 0usize;
        for source in files {
            let relative = match source.strip_prefix(source_root) {
                Ok(relative) => relative.to_path_buf(),
                Err(_) => continue,
            };
            let destination = destination_root.join(relative);
            if destination.exists() {
                let same = std::fs::read(&source).ok() == std::fs::read(&destination).ok();
                if same {
                    unchanged += 1;
                    continue;
                }
                if !self.options.overwrite {
                    conflicts += 1;
                    continue;
                }
            }
            if self.options.execute {
                let _ = self.maybe_backup(&destination)?;
                copy_path(&source, &destination)?;
            }
            copied += 1;
        }

        let status = if copied == 0 {
            MigrationStatus::Skipped
        } else {
            MigrationStatus::Success
        };
        Ok(MigrationItem::new(
            kind,
            status,
            &format!("copied {copied} file(s); {unchanged} unchanged; {conflicts} conflict(s)"),
        ))
    }

    fn resolve_skill_destination(&self, base: &Path) -> PathBuf {
        let parent = base.parent().unwrap_or(base);
        let stem = base
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("skill");

        let mut idx = 0usize;
        loop {
            let candidate_name = if idx == 0 {
                format!("{stem}-imported")
            } else {
                format!("{stem}-imported-{}", idx + 1)
            };
            let candidate = parent.join(candidate_name);
            if !candidate.exists() {
                return candidate;
            }
            idx += 1;
        }
    }

    fn write_overflow_entries(&self, kind: &str, overflowed: &[String]) -> anyhow::Result<()> {
        if overflowed.is_empty() || !self.options.execute {
            return Ok(());
        }
        let destination = self
            .output_root
            .join("overflow")
            .join(format!("{kind}.txt"));
        ensure_parent(&destination)?;
        std::fs::write(destination, overflowed.join("\n"))?;
        Ok(())
    }
}

fn ensure_mapping(value: &mut serde_yml::Value) -> &mut serde_yml::Mapping {
    if !matches!(value, serde_yml::Value::Mapping(_)) {
        *value = serde_yml::Value::Mapping(serde_yml::Mapping::new());
    }
    match value {
        serde_yml::Value::Mapping(map) => map,
        _ => unreachable!(),
    }
}

fn ensure_child_mapping<'a>(
    parent: &'a mut serde_yml::Mapping,
    key: &str,
) -> &'a mut serde_yml::Mapping {
    let entry = parent
        .entry(serde_yml::Value::String(key.to_string()))
        .or_insert_with(|| serde_yml::Value::Mapping(serde_yml::Mapping::new()));
    ensure_mapping(entry)
}

fn set_yaml_string(map: &mut serde_yml::Mapping, key: &str, value: String) -> bool {
    let key_value = serde_yml::Value::String(key.to_string());
    let next = serde_yml::Value::String(value);
    if map.get(&key_value) == Some(&next) {
        return false;
    }
    map.insert(key_value, next);
    true
}

fn set_yaml_root_string(map: &mut serde_yml::Mapping, key: &str, value: String) -> bool {
    set_yaml_string(map, key, value)
}

fn set_yaml_string_opt(map: &mut serde_yml::Mapping, key: &str, value: Option<String>) -> bool {
    value
        .map(|value| set_yaml_string(map, key, value))
        .unwrap_or(false)
}

fn set_yaml_u64(map: &mut serde_yml::Mapping, key: &str, value: u64) -> bool {
    let key_value = serde_yml::Value::String(key.to_string());
    let next = serde_yml::Value::Number(serde_yml::Number::from(value));
    if map.get(&key_value) == Some(&next) {
        return false;
    }
    map.insert(key_value, next);
    true
}

fn copy_json_string(
    source: &serde_json::Map<String, JsonValue>,
    source_key: &str,
    destination: &mut serde_yml::Mapping,
    destination_key: &str,
) {
    if let Some(value) = source
        .get(source_key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        destination.insert(
            serde_yml::Value::String(destination_key.to_string()),
            serde_yml::Value::String(value.to_string()),
        );
    }
}

fn copy_json_bool(
    source: &serde_json::Map<String, JsonValue>,
    source_key: &str,
    destination: &mut serde_yml::Mapping,
    destination_key: &str,
) {
    if let Some(value) = source.get(source_key).and_then(JsonValue::as_bool) {
        destination.insert(
            serde_yml::Value::String(destination_key.to_string()),
            serde_yml::Value::Bool(value),
        );
    }
}

fn copy_json_u64(
    source: &serde_json::Map<String, JsonValue>,
    source_key: &str,
    destination: &mut serde_yml::Mapping,
    destination_key: &str,
) {
    if let Some(value) = source.get(source_key).and_then(JsonValue::as_u64) {
        destination.insert(
            serde_yml::Value::String(destination_key.to_string()),
            serde_yml::Value::Number(serde_yml::Number::from(value)),
        );
    }
}

fn copy_json_string_list(
    source: &serde_json::Map<String, JsonValue>,
    source_key: &str,
    destination: &mut serde_yml::Mapping,
    destination_key: &str,
) {
    let Some(values) = source.get(source_key).and_then(JsonValue::as_array) else {
        return;
    };
    let items = values
        .iter()
        .filter_map(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| serde_yml::Value::String(value.to_string()))
        .collect::<Vec<_>>();
    if items.is_empty() {
        return;
    }
    destination.insert(
        serde_yml::Value::String(destination_key.to_string()),
        serde_yml::Value::Sequence(items),
    );
}

fn copy_json_object_strings(
    source: &serde_json::Map<String, JsonValue>,
    source_key: &str,
    destination: &mut serde_yml::Mapping,
    destination_key: &str,
) {
    let Some(values) = source.get(source_key).and_then(JsonValue::as_object) else {
        return;
    };
    let mut map = serde_yml::Mapping::new();
    for (key, value) in values {
        if let Some(value) = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert(
                serde_yml::Value::String(key.clone()),
                serde_yml::Value::String(value.to_string()),
            );
        }
    }
    if map.is_empty() {
        return;
    }
    destination.insert(
        serde_yml::Value::String(destination_key.to_string()),
        serde_yml::Value::Mapping(map),
    );
}

fn json_str(value: Option<&JsonValue>) -> Option<String> {
    value
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn csv_from_json_array(value: Option<&JsonValue>) -> Option<String> {
    let values = value
        .and_then(JsonValue::as_array)?
        .iter()
        .map(|item| match item {
            JsonValue::String(value) => value.trim().to_string(),
            JsonValue::Number(value) => value.to_string(),
            _ => String::new(),
        })
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.join(","))
    }
}

fn provider_env_key(
    provider_name: &str,
    base_url: Option<&JsonValue>,
    api_type: Option<&JsonValue>,
) -> Option<&'static str> {
    if let Some(base_url) = base_url.and_then(JsonValue::as_str) {
        let base_url = base_url.to_ascii_lowercase();
        if base_url.contains("openrouter") {
            return Some("OPENROUTER_API_KEY");
        }
        if base_url.contains("openai.com") {
            return Some("OPENAI_API_KEY");
        }
        if base_url.contains("anthropic") {
            return Some("ANTHROPIC_API_KEY");
        }
    }
    if matches!(
        api_type.and_then(JsonValue::as_str),
        Some("anthropic-messages")
    ) {
        return Some("ANTHROPIC_API_KEY");
    }

    let provider = provider_name.to_ascii_lowercase();
    if provider == "openrouter" {
        return Some("OPENROUTER_API_KEY");
    }
    if provider.contains("openai") {
        return Some("OPENAI_API_KEY");
    }
    if provider.contains("anthropic") {
        return Some("ANTHROPIC_API_KEY");
    }
    None
}

fn resolve_secret_input(
    value: Option<&JsonValue>,
    env: &BTreeMap<String, String>,
) -> Option<String> {
    match value? {
        JsonValue::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some(var) = trimmed
                .strip_prefix("${")
                .and_then(|value| value.strip_suffix('}'))
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return env.get(var).cloned();
            }
            Some(trimmed.to_string())
        }
        JsonValue::Object(map) => map
            .get("env")
            .and_then(JsonValue::as_str)
            .and_then(|name| env.get(name.trim()))
            .cloned(),
        _ => None,
    }
}

fn describe_memory_merge(
    destination: &Path,
    stats: MergeStats,
    overflowed: &[String],
    execute: bool,
) -> String {
    let action = if execute { "merged" } else { "would merge" };
    let mut detail = format!(
        "{action} {} new entrie(s) into {} ({} duplicate(s))",
        stats.added,
        destination.display(),
        stats.duplicates
    );
    if !overflowed.is_empty() {
        detail.push_str(&format!("; {} entrie(s) overflowed", overflowed.len()));
    }
    detail
}

fn collect_daily_memory_entries(source_root: &Path) -> anyhow::Result<Vec<String>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(source_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        entries.extend(extract_markdown_entries(&raw));
    }
    Ok(entries)
}

fn collect_files_recursive(source_root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![source_root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }

    Ok(files)
}

fn count_token(detail: &str, token: &str) -> usize {
    detail
        .split(';')
        .find(|part| part.contains(token))
        .and_then(|part| {
            part.split_whitespace()
                .find_map(|chunk| chunk.parse::<usize>().ok())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_options() -> OpenClawMigrationOptions {
        OpenClawMigrationOptions {
            execute: true,
            overwrite: false,
            migrate_secrets: false,
            preset: OpenClawPreset::UserData,
            workspace_target: None,
            skill_conflict_mode: SkillConflictMode::Skip,
        }
    }

    #[test]
    fn migrator_copies_skill_and_merges_allowlist() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join(".openclaw");
        let target = temp.path().join(".edgecrab");
        std::fs::create_dir_all(source.join("workspace/skills/demo-skill")).expect("create skill");
        std::fs::write(
            source.join("workspace/skills/demo-skill/SKILL.md"),
            "---\nname: demo-skill\ndescription: demo\n---\n\nbody\n",
        )
        .expect("write skill");
        std::fs::write(
            source.join("exec-approvals.json"),
            serde_json::json!({
                "agents": {
                    "*": {
                        "allowlist": [
                            {"pattern": "/usr/bin/*"},
                            {"pattern": "/home/test/**"}
                        ]
                    }
                }
            })
            .to_string(),
        )
        .expect("write allowlist");

        let migrator = OpenClawMigrator::new(source, target.clone(), base_options());
        let report = migrator.migrate_all().expect("migrate");

        assert!(
            target
                .join("skills")
                .join(SKILL_CATEGORY_DIRNAME)
                .join("demo-skill")
                .join("SKILL.md")
                .exists()
        );
        let allowlist =
            std::fs::read_to_string(target.join("command_allowlist.json")).expect("allowlist");
        assert!(allowlist.contains("/home/test/**"));
        assert!(report.success_count() >= 2);
    }

    #[test]
    fn migrator_imports_supported_secrets_and_messaging_settings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join(".openclaw");
        let target = temp.path().join(".edgecrab");
        std::fs::create_dir_all(source.join("credentials")).expect("credentials");
        std::fs::write(
            source.join("openclaw.json"),
            serde_json::json!({
                "agents": {"defaults": {"workspace": "/tmp/openclaw-workspace"}},
                "channels": {"telegram": {"botToken": "123:abc"}}
            })
            .to_string(),
        )
        .expect("write config");
        std::fs::write(
            source.join("credentials/telegram-default-allowFrom.json"),
            serde_json::json!({"allowFrom": ["111", "222"]}).to_string(),
        )
        .expect("write allowfrom");

        let mut options = base_options();
        options.migrate_secrets = true;
        let migrator = OpenClawMigrator::new(source, target.clone(), options);
        migrator.migrate_all().expect("migrate");

        let env_text = std::fs::read_to_string(target.join(".env")).expect("env");
        assert!(env_text.contains("MESSAGING_CWD=/tmp/openclaw-workspace"));
        assert!(env_text.contains("TELEGRAM_ALLOWED_USERS=111,222"));
        assert!(env_text.contains("TELEGRAM_BOT_TOKEN=123:abc"));
    }

    #[test]
    fn migrator_sets_model_and_tts_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join(".openclaw");
        let target = temp.path().join(".edgecrab");
        std::fs::create_dir_all(&source).expect("source");
        std::fs::create_dir_all(&target).expect("target");
        std::fs::write(
            source.join("openclaw.json"),
            serde_json::json!({
                "agents": {"defaults": {"model": {"primary": "openai/gpt-4o"}}},
                "messages": {
                    "tts": {
                        "provider": "elevenlabs",
                        "elevenlabs": {
                            "voiceId": "voice-123",
                            "modelId": "eleven_turbo_v2"
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect("write config");

        let migrator = OpenClawMigrator::new(source, target.clone(), base_options());
        migrator.migrate_all().expect("migrate");

        let config_text = std::fs::read_to_string(target.join("config.yaml")).expect("config");
        assert!(config_text.contains("openai/gpt-4o"));
        assert!(config_text.contains("voice-123"));
        assert!(config_text.contains("eleven_turbo_v2"));
    }

    #[test]
    fn migrator_imports_mcp_servers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join(".openclaw");
        let target = temp.path().join(".edgecrab");
        std::fs::create_dir_all(&source).expect("source");
        std::fs::create_dir_all(&target).expect("target");
        std::fs::write(
            source.join("openclaw.json"),
            serde_json::json!({
                "mcp": {
                    "servers": {
                        "demo": {
                            "command": "npx",
                            "args": ["-y", "@demo/mcp"],
                            "env": {"NODE_ENV": "production"},
                            "enabled": true,
                            "tools": {"include": ["search"]}
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect("write config");

        let migrator = OpenClawMigrator::new(source, target.clone(), base_options());
        migrator.migrate_all().expect("migrate");

        let config_text = std::fs::read_to_string(target.join("config.yaml")).expect("config");
        assert!(config_text.contains("mcp_servers"));
        assert!(config_text.contains("demo"));
        assert!(config_text.contains("@demo/mcp"));
    }
}
