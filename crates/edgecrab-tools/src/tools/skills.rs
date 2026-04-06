//! # skills — Discover, view, and manage agent skills
//!
//! WHY skills: Extensible domain knowledge modules that inject
//! context into the system prompt. Skills live in `~/.edgecrab/skills/`
//! as directories with SKILL.md files.
//!
//! Skills support YAML frontmatter for metadata:
//! ```yaml
//! ---
//! name: My Skill
//! description: What this skill does
//! category: coding
//! platforms: [cli, telegram]
//! read_files: [extra.md, examples.md]
//! ---
//! ```

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

// ─── Path expansion helpers ────────────────────────────────────

/// Expand ~ and ${VAR} in a path string.
fn expand_path_with_env(path_str: &str) -> PathBuf {
    let mut result = path_str.to_string();

    // Expand ~ to home directory
    if result.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            result = result.replacen("~", home.to_string_lossy().as_ref(), 1);
        }
    }

    // Expand ${VAR} and $VAR environment variables
    // Simple regex-free approach: find ${...} and $VAR patterns
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let value = std::env::var(var_name).unwrap_or_default();
            result = format!(
                "{}{}{}",
                &result[..start],
                value,
                &result[start + end + 1..]
            );
        } else {
            break;
        }
    }

    std::path::PathBuf::from(result)
}

/// Resolve a list of skill directories, expanding paths and filtering non-existent ones.
fn resolve_skill_directories(
    base_dir: &std::path::Path,
    external_dirs: &[String],
) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![base_dir.to_path_buf()];

    for dir_str in external_dirs {
        let expanded = expand_path_with_env(dir_str);
        if expanded.is_dir() {
            dirs.push(expanded);
        }
        // Silently skip non-existent paths (mirrors hermes behavior)
    }

    dirs
}

/// Find a skill directory by name, searching recursively through category
/// subdirectories. Mirrors the hermes-agent pattern:
///   1. Try direct flat path: `<base>/skills/<name>/SKILL.md`
///   2. Recursively search for a directory whose leaf name matches `name`
///      and contains a `SKILL.md`.
///
/// Returns the first matching skill directory, or None.
fn find_skill_dir(skills_base: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    // 1. Direct flat lookup
    let direct = skills_base.join(name);
    if direct.join("SKILL.md").is_file() {
        return Some(direct);
    }

    // 2. Recursive search by leaf directory name
    let mut stack = vec![skills_base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(leaf) = path.file_name().and_then(|n| n.to_str()) {
                    if leaf == name && path.join("SKILL.md").is_file() {
                        return Some(path);
                    }
                }
                stack.push(path);
            }
        }
    }

    None
}

/// Find a skill directory across multiple skill root directories (local + external).
fn find_skill_dir_in_roots(roots: &[std::path::PathBuf], name: &str) -> Option<std::path::PathBuf> {
    for root in roots {
        if let Some(found) = find_skill_dir(root, name) {
            return Some(found);
        }
    }
    None
}

/// Collect available skill names from all root directories (up to `limit`).
/// Used for not-found hints so the model can self-correct.
fn collect_available_skill_names(roots: &[std::path::PathBuf], limit: usize) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let leaf = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if leaf.starts_with('.') {
                    continue;
                }
                if path.join("SKILL.md").is_file() {
                    if seen.insert(leaf.clone()) {
                        names.push(leaf);
                        if names.len() >= limit {
                            return names;
                        }
                    }
                } else {
                    stack.push(path);
                }
            }
        }
    }
    names.sort();
    names
}

// ─── Frontmatter parsing ───────────────────────────────────────

/// Required environment variable spec for a skill.
#[derive(Debug, Clone, Default)]
struct EnvVarSpec {
    name: String,
    prompt: Option<String>,
    help: Option<String>,
    required_for: String, // "full functionality" | "operation"
}

/// Conditional activation rules for a skill.
#[derive(Debug, Clone, Default)]
struct ConditionalActivation {
    fallback_for_toolsets: Vec<String>, // Show only when these toolsets are unavailable
    requires_toolsets: Vec<String>,     // Show only when these toolsets are available
    fallback_for_tools: Vec<String>,    // Show only when these tools are unavailable
    requires_tools: Vec<String>,        // Show only when these tools are available
}

/// Metadata extracted from YAML frontmatter in SKILL.md files.
#[derive(Debug, Clone, Default)]
struct SkillMeta {
    name: Option<String>,
    description: Option<String>,
    category: Option<String>,
    version: Option<String>,
    license: Option<String>,
    platforms: Vec<String>,  // Restrict to [macos, linux, windows]
    read_files: Vec<String>, // Progressive disclosure: linked files to load
    required_environment_variables: Vec<EnvVarSpec>, // Secure setup on load
    conditional_activation: ConditionalActivation, // Fallback/requires rules
}

/// Parse YAML frontmatter from a skill file into SkillMeta.
///
/// WHY manual parsing: Adding a full YAML parser dependency (serde_yaml)
/// just for frontmatter extraction is overkill. We only need a handful
/// of simple key-value pairs. This parser handles the common cases.
///
/// Supports:
/// - Simple scalars: name, description, category, version, license
/// - Lists: platforms, read_files, fallback_for_toolsets, requires_toolsets, etc.
/// - Nested objects: required_environment_variables (list of {name, prompt, help, required_for})
fn parse_skill_frontmatter(content: &str) -> SkillMeta {
    let mut meta = SkillMeta::default();
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return meta;
    }
    let after_first = &trimmed[3..];
    let end_pos = match after_first.find("\n---") {
        Some(p) => p,
        None => return meta,
    };
    let frontmatter = &after_first[..end_pos];

    let mut current_list_key: Option<&str> = None;
    let mut current_list_context: Vec<String> = Vec::new();
    let mut in_env_var_list = false;
    let mut current_env_var: EnvVarSpec = EnvVarSpec::default();

    for line in frontmatter.lines() {
        let trimmed_line = line.trim();

        // Empty lines
        if trimmed_line.is_empty() {
            continue;
        }

        // Handle environment variables list items ({name: ..., prompt: ...})
        if in_env_var_list {
            if trimmed_line.starts_with("- name:") {
                // Start of a new env var spec
                if !current_env_var.name.is_empty() {
                    meta.required_environment_variables
                        .push(current_env_var.clone());
                }
                current_env_var = EnvVarSpec::default();
                let val = trimmed_line.trim_start_matches("- name:").trim();
                current_env_var.name = val.trim_matches('"').trim_matches('\'').to_string();
                continue;
            } else if trimmed_line.starts_with("prompt:") && !current_env_var.name.is_empty() {
                let val = trimmed_line.trim_start_matches("prompt:").trim();
                current_env_var.prompt = Some(val.trim_matches('"').trim_matches('\'').to_string());
                continue;
            } else if trimmed_line.starts_with("help:") && !current_env_var.name.is_empty() {
                let val = trimmed_line.trim_start_matches("help:").trim();
                current_env_var.help = Some(val.trim_matches('"').trim_matches('\'').to_string());
                continue;
            } else if trimmed_line.starts_with("required_for:") && !current_env_var.name.is_empty()
            {
                let val = trimmed_line.trim_start_matches("required_for:").trim();
                current_env_var.required_for = val.trim_matches('"').trim_matches('\'').to_string();
                continue;
            } else if !trimmed_line.starts_with('-') && trimmed_line.contains(':') {
                // Some other field → end of env var list
                in_env_var_list = false;
                if !current_env_var.name.is_empty() {
                    meta.required_environment_variables
                        .push(current_env_var.clone());
                    current_env_var = EnvVarSpec::default();
                }
            } else {
                continue;
            }
        }

        // Handle YAML list continuation items (  - value)
        if let Some(stripped) = trimmed_line.strip_prefix("- ") {
            if current_list_key.is_some() {
                let item = stripped.trim().to_string();
                if !item.is_empty() {
                    current_list_context.push(item);
                }
            }
            continue;
        }

        // Reset list context when we hit a non-list key-value line
        if trimmed_line.contains(':')
            && !trimmed_line.starts_with('-')
            && !trimmed_line.starts_with('#')
        {
            // Flush accumulated list to metadata
            match current_list_key {
                Some("platforms") => meta.platforms = current_list_context.clone(),
                Some("read_files") => meta.read_files = current_list_context.clone(),
                Some("fallback_for_toolsets") => {
                    meta.conditional_activation.fallback_for_toolsets = current_list_context.clone()
                }
                Some("requires_toolsets") => {
                    meta.conditional_activation.requires_toolsets = current_list_context.clone()
                }
                Some("fallback_for_tools") => {
                    meta.conditional_activation.fallback_for_tools = current_list_context.clone()
                }
                Some("requires_tools") => {
                    meta.conditional_activation.requires_tools = current_list_context.clone()
                }
                _ => {}
            }
            current_list_key = None;
            current_list_context.clear();
        }

        if let Some((key, val)) = trimmed_line.split_once(':') {
            let key = key.trim();
            let val = val.trim().trim_matches('"').trim_matches('\'');
            match key {
                "name" => meta.name = Some(val.to_string()),
                "description" => {
                    if !val.is_empty() {
                        meta.description = Some(val.to_string());
                    }
                }
                "category" => meta.category = Some(val.to_string()),
                "version" => {
                    if !val.is_empty() {
                        meta.version = Some(val.to_string());
                    }
                }
                "license" => {
                    if !val.is_empty() {
                        meta.license = Some(val.to_string());
                    }
                }
                "platforms" => {
                    if val.is_empty() {
                        // Multi-line YAML list follows
                        current_list_key = Some("platforms");
                        current_list_context.clear();
                    } else {
                        // Inline format: [cli, telegram]
                        let stripped = val.trim_start_matches('[').trim_end_matches(']');
                        meta.platforms = stripped
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                "read_files" => {
                    if val.is_empty() {
                        current_list_key = Some("read_files");
                        current_list_context.clear();
                    } else {
                        let stripped = val.trim_start_matches('[').trim_end_matches(']');
                        meta.read_files = stripped
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                "fallback_for_toolsets" => {
                    if val.is_empty() {
                        current_list_key = Some("fallback_for_toolsets");
                        current_list_context.clear();
                    } else {
                        let stripped = val.trim_start_matches('[').trim_end_matches(']');
                        meta.conditional_activation.fallback_for_toolsets = stripped
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                "requires_toolsets" => {
                    if val.is_empty() {
                        current_list_key = Some("requires_toolsets");
                        current_list_context.clear();
                    } else {
                        let stripped = val.trim_start_matches('[').trim_end_matches(']');
                        meta.conditional_activation.requires_toolsets = stripped
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                "fallback_for_tools" => {
                    if val.is_empty() {
                        current_list_key = Some("fallback_for_tools");
                        current_list_context.clear();
                    } else {
                        let stripped = val.trim_start_matches('[').trim_end_matches(']');
                        meta.conditional_activation.fallback_for_tools = stripped
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                "requires_tools" => {
                    if val.is_empty() {
                        current_list_key = Some("requires_tools");
                        current_list_context.clear();
                    } else {
                        let stripped = val.trim_start_matches('[').trim_end_matches(']');
                        meta.conditional_activation.requires_tools = stripped
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                "required_environment_variables" => {
                    if val.is_empty() {
                        in_env_var_list = true;
                    }
                }
                _ => {}
            }
        }
    }

    // Flush any accumulated context
    match current_list_key {
        Some("platforms") => meta.platforms = current_list_context,
        Some("read_files") => meta.read_files = current_list_context,
        Some("fallback_for_toolsets") => {
            meta.conditional_activation.fallback_for_toolsets = current_list_context
        }
        Some("requires_toolsets") => {
            meta.conditional_activation.requires_toolsets = current_list_context
        }
        Some("fallback_for_tools") => {
            meta.conditional_activation.fallback_for_tools = current_list_context
        }
        Some("requires_tools") => meta.conditional_activation.requires_tools = current_list_context,
        _ => {}
    }

    if !current_env_var.name.is_empty() {
        meta.required_environment_variables.push(current_env_var);
    }

    meta
}

/// Strip YAML frontmatter from content, returning just the body.
fn strip_frontmatter(content: &str) -> &str {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content;
    }
    let after_first = &trimmed[3..];
    if let Some(end_pos) = after_first.find("\n---") {
        let remainder = &after_first[end_pos + 4..];
        remainder.trim_start_matches('\n').trim_start_matches('\r')
    } else {
        content
    }
}

/// Determine the current platform string for filtering.
/// Maps Platform enum to ["darwin", "linux", "windows"] as per agentskills.io spec.
fn get_current_platform_filter() -> String {
    #[cfg(target_os = "macos")]
    return "darwin".to_string();
    #[cfg(target_os = "linux")]
    return "linux".to_string();
    #[cfg(target_os = "windows")]
    return "windows".to_string();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return "unknown".to_string();
}

/// Check if a skill should be visible on the current platform.
/// If platforms list is empty, the skill is available on all platforms.
fn should_show_on_platform(platforms: &[String]) -> bool {
    if platforms.is_empty() {
        return true; // No restrictions
    }
    let current = get_current_platform_filter();
    platforms.iter().any(|p| p.to_lowercase() == current)
}

/// Check conditional activation rules given available toolsets and tools.
/// Returns true if the skill should be shown.
///
/// Rules:
/// - If any fallback_for_toolsets are in the available set, HIDE the skill
/// - If fallback_for_toolsets is not empty but none match, show it
/// - If requires_toolsets is not empty, SHOW only if at least one matches  
/// - Same logic for fallback_for_tools and requires_tools
fn should_show_by_condition(
    meta: &ConditionalActivation,
    available_toolsets: &[String],
    available_tools: &[String],
) -> bool {
    // fallback_for_toolsets: hide if ANY active toolset matches
    if !meta.fallback_for_toolsets.is_empty()
        && meta
            .fallback_for_toolsets
            .iter()
            .any(|t| available_toolsets.iter().any(|a| a.eq_ignore_ascii_case(t)))
    {
        return false; // Primary tool is available, hide fallback
    }

    // requires_toolsets: show only if AT LEAST ONE required toolset is available
    if !meta.requires_toolsets.is_empty()
        && !meta
            .requires_toolsets
            .iter()
            .any(|t| available_toolsets.iter().any(|a| a.eq_ignore_ascii_case(t)))
    {
        return false; // Required toolset not available
    }

    // fallback_for_tools: hide if ANY active tool matches
    if !meta.fallback_for_tools.is_empty()
        && meta
            .fallback_for_tools
            .iter()
            .any(|t| available_tools.iter().any(|a| a.eq_ignore_ascii_case(t)))
    {
        return false;
    }

    // requires_tools: show only if AT LEAST ONE required tool is available
    if !meta.requires_tools.is_empty()
        && !meta
            .requires_tools
            .iter()
            .any(|t| available_tools.iter().any(|a| a.eq_ignore_ascii_case(t)))
    {
        return false;
    }

    true // All conditions pass, show the skill
}

// ─── skills_list ───────────────────────────────────────────────

pub struct SkillsListTool;

#[async_trait]
impl ToolHandler for SkillsListTool {
    fn name(&self) -> &'static str {
        "skills_list"
    }

    fn toolset(&self) -> &'static str {
        "skills"
    }

    fn emoji(&self) -> &'static str {
        "📚"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skills_list".into(),
            description: "List all available skills with descriptions and categories.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Filter by category (optional)"
                    }
                }
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let filter_category = args
            .get("category")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase());

        let skills_dir = ctx.config.edgecrab_home.join("skills");
        if !skills_dir.is_dir() {
            return Ok(
                "No skills directory found. Create ~/.edgecrab/skills/<name>/SKILL.md".into(),
            );
        }

        // Resolve all skill directories (local + external from config)
        let all_dirs = resolve_skill_directories(&skills_dir, &ctx.config.external_skill_dirs);

        // Build disabled skills set for filtering
        let disabled_set: std::collections::HashSet<String> = ctx
            .config
            .disabled_skills
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        let mut skills: Vec<(String, SkillMeta)> = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        for scan_dir in &all_dirs {
            if !scan_dir.is_dir() {
                continue;
            }
            // Walk recursively to support nested category dirs
            // (e.g. skills/mlops/training/axolotl/SKILL.md)
            let mut stack = vec![scan_dir.to_path_buf()];
            while let Some(current_dir) = stack.pop() {
                let mut entries = match tokio::fs::read_dir(&current_dir).await {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                while let Some(entry) = entries
                    .next_entry()
                    .await
                    .map_err(|e| ToolError::Other(e.to_string()))?
                {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    // Skip hidden/hub directories
                    let dir_name_str = path.file_name().unwrap_or_default().to_string_lossy();
                    if dir_name_str.starts_with('.') {
                        continue;
                    }
                    let skill_md = path.join("SKILL.md");
                    if !skill_md.is_file() {
                        // Not a skill dir — recurse into it as a category dir
                        stack.push(path);
                        continue;
                    }
                    let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n.to_string(),
                        None => continue,
                    };

                    // Deduplicate: first directory wins (local > external)
                    if !seen_names.insert(dir_name.clone()) {
                        continue;
                    }

                    // Parse frontmatter for metadata
                    let meta = match tokio::fs::read_to_string(&skill_md).await {
                        Ok(content) => parse_skill_frontmatter(&content),
                        Err(_) => SkillMeta::default(),
                    };

                    // Check disabled skills list
                    let skill_name_lower = meta.name.as_deref().unwrap_or(&dir_name).to_lowercase();
                    if disabled_set.contains(&skill_name_lower)
                        || disabled_set.contains(&dir_name.to_lowercase())
                    {
                        continue;
                    }

                    // Filter by platform compatibility (always check current OS)
                    if !should_show_on_platform(&meta.platforms) {
                        continue;
                    }

                    // Filter by category if specified
                    if let Some(ref cat) = filter_category {
                        if let Some(ref skill_cat) = meta.category {
                            if skill_cat.to_lowercase() != *cat {
                                continue;
                            }
                        } else {
                            continue; // No category on skill, skip when filtering
                        }
                    }

                    // Filter by conditional activation (use available toolsets/tools from context if present)
                    if !should_show_by_condition(
                        &meta.conditional_activation,
                        &ctx.config.parent_active_toolsets,
                        &[], // TODO: get available_tools from context once we plumb it through
                    ) {
                        continue;
                    }

                    skills.push((dir_name, meta));
                }
            }
        }

        if skills.is_empty() {
            return Ok("No skills found. Create a skill by adding <name>/SKILL.md in the skills directory.".into());
        }

        skills.sort_by(|a, b| a.0.cmp(&b.0));

        // Group by category for display
        let mut output = format!("Found {} skills:\n\n", skills.len());
        let mut current_category: Option<String> = None;
        for (name, meta) in &skills {
            let cat = meta.category.as_deref().unwrap_or("uncategorized");
            if current_category.as_deref() != Some(cat) {
                output.push_str(&format!("### {}\n", cat));
                current_category = Some(cat.to_string());
            }
            let display_name = meta.name.as_deref().unwrap_or(name.as_str());
            if let Some(ref desc) = meta.description {
                output.push_str(&format!("- **{}**: {}\n", display_name, desc));
            } else {
                output.push_str(&format!("- **{}**\n", display_name));
            }
        }
        Ok(output)
    }
}

inventory::submit!(&SkillsListTool as &dyn ToolHandler);

// ─── skills_categories ─────────────────────────────────────────

/// List available skill categories with skill counts.
/// Progressive disclosure tier 0 — lets the agent discover what
/// categories exist before drilling into individual skills.
pub struct SkillsCategoriesList;

#[async_trait]
impl ToolHandler for SkillsCategoriesList {
    fn name(&self) -> &'static str {
        "skills_categories"
    }

    fn toolset(&self) -> &'static str {
        "skills"
    }

    fn emoji(&self) -> &'static str {
        "📂"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skills_categories".into(),
            description: "List available skill categories with skill counts. Use to discover what categories exist before drilling into skills_list with a category filter.".into(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let skills_dir = ctx.config.edgecrab_home.join("skills");
        if !skills_dir.is_dir() {
            return Ok("No skills directory found.".into());
        }

        let mut categories: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();

        // Recursive scan: find all SKILL.md files and extract categories
        let mut stack = vec![(skills_dir.clone(), Vec::<String>::new())];
        while let Some((dir, path_parts)) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if name.starts_with('.') {
                    continue;
                }
                let skill_md = path.join("SKILL.md");
                if skill_md.is_file() {
                    // This is a skill — check platform compatibility
                    if let Ok(content) = std::fs::read_to_string(&skill_md) {
                        let m = parse_skill_frontmatter(&content);
                        if !m.platforms.is_empty() && !should_show_on_platform(&m.platforms) {
                            continue;
                        }
                    }
                    let category = if path_parts.is_empty() {
                        "uncategorized".to_string()
                    } else {
                        path_parts.join("/")
                    };
                    *categories.entry(category).or_insert(0) += 1;
                } else {
                    // Not a skill — recurse
                    let mut child_parts = path_parts.clone();
                    child_parts.push(name);
                    stack.push((path, child_parts));
                }
            }
        }

        if categories.is_empty() {
            return Ok("No skill categories found.".into());
        }

        let mut output = format!("## Skill Categories ({} total)\n\n", categories.len());
        for (cat, count) in &categories {
            let plural = if *count == 1 { "skill" } else { "skills" };
            output.push_str(&format!("- **{cat}** ({count} {plural})\n"));
        }
        output.push_str(
            "\nUse `skills_list` with a category filter to see skills in a specific category.",
        );
        Ok(output)
    }
}

inventory::submit!(&SkillsCategoriesList as &dyn ToolHandler);

// ─── skill_view ────────────────────────────────────────────────

pub struct SkillViewTool;

#[derive(Deserialize)]
struct ViewArgs {
    name: String,
    /// Optional path to a linked/supporting file within the skill dir
    /// (e.g. "references/api.md", "templates/output.md").
    /// If omitted, loads the main SKILL.md with all read_files.
    #[serde(default)]
    file_path: Option<String>,
}

#[async_trait]
impl ToolHandler for SkillViewTool {
    fn name(&self) -> &'static str {
        "skill_view"
    }

    fn toolset(&self) -> &'static str {
        "skills"
    }

    fn emoji(&self) -> &'static str {
        "🔍"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skill_view".into(),
            description: "View a skill's content with linked files (progressive disclosure). Use file_path to load a specific supporting file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the skill to view"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to a linked file within the skill (e.g. 'references/api.md', 'templates/output.md'). Omit to load the main SKILL.md."
                    }
                },
                "required": ["name"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: ViewArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "skill_view".into(),
            message: e.to_string(),
        })?;

        // Sanitize skill name to prevent path traversal
        if args.name.contains("..") {
            return Err(ToolError::PermissionDenied(
                "Invalid skill name — must not contain '..'".into(),
            ));
        }

        let skills_base = ctx.config.edgecrab_home.join("skills");
        let external_dirs = ctx.config.external_skill_dirs.clone();
        let roots = resolve_skill_directories(&skills_base, &external_dirs);
        let skill_dir = match find_skill_dir_in_roots(&roots, &args.name) {
            Some(d) => d,
            None => {
                let available = collect_available_skill_names(&roots, 20);
                let hint = if available.is_empty() {
                    "No skills are installed. Use skills_list to check.".to_string()
                } else {
                    format!(
                        "Available skills: {}. Use skills_list for the full list.",
                        available.join(", ")
                    )
                };
                return Err(ToolError::NotFound(format!(
                    "Skill '{}' not found. {}",
                    args.name, hint
                )));
            }
        };

        // If file_path is provided, return just that file (tier 3 progressive disclosure)
        if let Some(ref fp) = args.file_path {
            if fp.contains("..") {
                return Err(ToolError::PermissionDenied(
                    "Path traversal ('..') is not allowed in file_path".into(),
                ));
            }
            let target = skill_dir.join(fp);
            // Ensure the resolved target stays within the skill dir
            let canonical_skill = skill_dir
                .canonicalize()
                .unwrap_or_else(|_| skill_dir.clone());
            let canonical_target = target.canonicalize().unwrap_or_else(|_| target.clone());
            if !canonical_target.starts_with(&canonical_skill) {
                return Err(ToolError::PermissionDenied(
                    "file_path must resolve within the skill directory".into(),
                ));
            }
            if !target.is_file() {
                return Err(ToolError::NotFound(format!(
                    "File '{}' not found in skill '{}'",
                    fp, args.name
                )));
            }
            let content = tokio::fs::read_to_string(&target)
                .await
                .map_err(|e| ToolError::Other(format!("Cannot read file: {}", e)))?;
            return Ok(format!("## Skill: {} / {}\n\n{}", args.name, fp, content));
        }

        let skill_path = skill_dir.join("SKILL.md");

        if !skill_path.is_file() {
            return Err(ToolError::NotFound(format!(
                "Skill '{}' not found — no SKILL.md at expected path",
                args.name
            )));
        }

        let content = tokio::fs::read_to_string(&skill_path)
            .await
            .map_err(|e| ToolError::Other(format!("Cannot read skill: {}", e)))?;

        // Parse frontmatter for linked files
        let meta = parse_skill_frontmatter(&content);
        let body = strip_frontmatter(&content);

        let mut output = format!("## Skill: {}\n\n{}", args.name, body);

        // Register required_environment_variables declared by this skill as
        // passthrough so they survive the `safe_env()` blocklist filter.
        // Mirrors hermes-agent's `register_env_passthrough(available_env_names)`
        // call in skills_tool.py.
        if !meta.required_environment_variables.is_empty() {
            let names: Vec<&str> = meta
                .required_environment_variables
                .iter()
                .map(|spec| spec.name.as_str())
                .collect();
            crate::tools::backends::local::register_env_passthrough(names);
        }

        // Phase 2: Show environment variable guidance if any are missing
        let missing_vars = check_missing_env_vars(&meta);
        if !missing_vars.is_empty() {
            output.push_str(&format_env_var_guidance(&missing_vars));
        }

        // Progressive disclosure: load linked files from read_files frontmatter
        for linked_file in &meta.read_files {
            // Sanitize linked filename
            if linked_file.contains("..")
                || linked_file.contains('/') && linked_file.starts_with('/')
            {
                continue; // Skip absolute paths or traversal attempts
            }
            let linked_path = skill_dir.join(linked_file);
            if linked_path.is_file() {
                match tokio::fs::read_to_string(&linked_path).await {
                    Ok(linked_content) => {
                        output.push_str(&format!(
                            "\n\n--- {} ---\n{}",
                            linked_file,
                            linked_content.trim()
                        ));
                    }
                    Err(_) => {
                        output.push_str(&format!(
                            "\n\n--- {} ---\n(could not read file)",
                            linked_file
                        ));
                    }
                }
            }
        }

        // Discover supporting files in standard subdirectories
        // (references/, templates/, scripts/, assets/) — like hermes-agent
        let mut supporting_files: Vec<String> = Vec::new();
        for subdir in &["references", "templates", "scripts", "assets"] {
            let sub_path = skill_dir.join(subdir);
            if sub_path.is_dir() {
                if let Ok(mut entries) = tokio::fs::read_dir(&sub_path).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let p = entry.path();
                        if p.is_file() {
                            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                                supporting_files.push(format!("{}/{}", subdir, name));
                            }
                        }
                    }
                }
            }
        }

        if !supporting_files.is_empty() {
            supporting_files.sort();
            output.push_str("\n\n### Supporting Files\n\n");
            output
                .push_str("Load any of these with `skill_view` using the `file_path` parameter:\n");
            for sf in &supporting_files {
                output.push_str(&format!("- `{}`\n", sf));
            }
        }

        Ok(output)
    }
}

inventory::submit!(&SkillViewTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx_in(dir: &std::path::Path) -> ToolContext {
        let mut ctx = ToolContext::test_context();
        ctx.config.edgecrab_home = dir.to_path_buf();
        ctx
    }

    #[tokio::test]
    async fn skills_list_empty() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());
        let result = SkillsListTool.execute(json!({}), &ctx).await.expect("list");
        assert!(result.contains("No skills directory"));
    }

    #[tokio::test]
    async fn skills_list_finds_skills() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("my_skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(skill_dir.join("SKILL.md"), "# My Skill").expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillsListTool.execute(json!({}), &ctx).await.expect("list");
        assert!(result.contains("my_skill"));
        assert!(result.contains("1 skills"));
    }

    #[tokio::test]
    async fn skill_view_found() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("test_skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(skill_dir.join("SKILL.md"), "# Test\nDescription here").expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillViewTool
            .execute(json!({"name": "test_skill"}), &ctx)
            .await
            .expect("view");
        assert!(result.contains("Description here"));
    }

    #[tokio::test]
    async fn skill_view_strips_frontmatter() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("fm_skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        let content = "---\ndescription: A test skill\ncategory: testing\n---\n# Body\nHello world";
        std::fs::write(skill_dir.join("SKILL.md"), content).expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillViewTool
            .execute(json!({"name": "fm_skill"}), &ctx)
            .await
            .expect("view");
        assert!(result.contains("# Body"));
        assert!(result.contains("Hello world"));
        // Frontmatter YAML should be stripped
        assert!(!result.contains("description: A test skill"));
    }

    #[tokio::test]
    async fn skill_view_loads_linked_files() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("linked_skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        let content = "---\nread_files:\n  - extra.md\n  - data.txt\n---\n# Main";
        std::fs::write(skill_dir.join("SKILL.md"), content).expect("write");
        std::fs::write(skill_dir.join("extra.md"), "Extra content here").expect("write");
        std::fs::write(skill_dir.join("data.txt"), "Data payload").expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillViewTool
            .execute(json!({"name": "linked_skill"}), &ctx)
            .await
            .expect("view");
        assert!(result.contains("# Main"));
        assert!(result.contains("Extra content here"));
        assert!(result.contains("Data payload"));
        assert!(result.contains("--- extra.md ---"));
        assert!(result.contains("--- data.txt ---"));
    }

    #[tokio::test]
    async fn skill_view_traversal_blocked() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());
        let result = SkillViewTool
            .execute(json!({"name": "../../../etc"}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn skill_view_not_found_lists_available() {
        let dir = TempDir::new().expect("tmpdir");
        // Create some skills so we can check hints
        let s1 = dir.path().join("skills").join("alpha-skill");
        let s2 = dir.path().join("skills").join("beta-skill");
        std::fs::create_dir_all(&s1).expect("mkdir");
        std::fs::create_dir_all(&s2).expect("mkdir");
        std::fs::write(s1.join("SKILL.md"), "# Alpha").expect("write");
        std::fs::write(s2.join("SKILL.md"), "# Beta").expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillViewTool
            .execute(json!({"name": "nonexistent"}), &ctx)
            .await;
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("nonexistent"),
            "should mention the requested name"
        );
        assert!(
            msg.contains("alpha-skill") || msg.contains("beta-skill"),
            "should list available skills, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn skill_view_finds_nested_category_skill() {
        let dir = TempDir::new().expect("tmpdir");
        // Create a skill inside a category subdirectory (e.g. media/gif-search/)
        let skill_dir = dir.path().join("skills").join("media").join("gif-search");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(skill_dir.join("SKILL.md"), "# GIF Search\nSearch for GIFs").expect("write");

        let ctx = ctx_in(dir.path());
        // skill_view should find it by leaf name "gif-search"
        let result = SkillViewTool
            .execute(json!({"name": "gif-search"}), &ctx)
            .await
            .expect("view nested skill");
        assert!(result.contains("Search for GIFs"));
    }

    #[tokio::test]
    async fn skill_view_finds_deeply_nested_skill() {
        let dir = TempDir::new().expect("tmpdir");
        // Create a skill nested two levels deep (e.g. mlops/training/axolotl/)
        let skill_dir = dir
            .path()
            .join("skills")
            .join("mlops")
            .join("training")
            .join("axolotl");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(skill_dir.join("SKILL.md"), "# Axolotl\nFine-tuning tool").expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillViewTool
            .execute(json!({"name": "axolotl"}), &ctx)
            .await
            .expect("view deeply nested skill");
        assert!(result.contains("Fine-tuning tool"));
    }

    #[test]
    fn find_skill_dir_flat() {
        let dir = TempDir::new().expect("tmpdir");
        let skills = dir.path().join("skills");
        let skill = skills.join("my-skill");
        std::fs::create_dir_all(&skill).expect("mkdir");
        std::fs::write(skill.join("SKILL.md"), "# Test").expect("write");

        assert_eq!(find_skill_dir(&skills, "my-skill"), Some(skill));
    }

    #[test]
    fn find_skill_dir_nested() {
        let dir = TempDir::new().expect("tmpdir");
        let skills = dir.path().join("skills");
        let skill = skills.join("media").join("gif-search");
        std::fs::create_dir_all(&skill).expect("mkdir");
        std::fs::write(skill.join("SKILL.md"), "# GIF").expect("write");

        assert_eq!(find_skill_dir(&skills, "gif-search"), Some(skill));
    }

    #[test]
    fn find_skill_dir_prefers_direct() {
        let dir = TempDir::new().expect("tmpdir");
        let skills = dir.path().join("skills");
        // Both flat and nested exist — direct/flat should win
        let flat = skills.join("my-skill");
        let nested = skills.join("category").join("my-skill");
        std::fs::create_dir_all(&flat).expect("mkdir");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::write(flat.join("SKILL.md"), "# Flat").expect("write");
        std::fs::write(nested.join("SKILL.md"), "# Nested").expect("write");

        assert_eq!(find_skill_dir(&skills, "my-skill"), Some(flat));
    }

    #[test]
    fn find_skill_dir_not_found() {
        let dir = TempDir::new().expect("tmpdir");
        let skills = dir.path().join("skills");
        std::fs::create_dir_all(&skills).expect("mkdir");

        assert_eq!(find_skill_dir(&skills, "nonexistent"), None);
    }

    #[tokio::test]
    async fn skill_manage_edit_nested_skill() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("dev").join("my-skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(skill_dir.join("SKILL.md"), "# Old content").expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillManageTool
            .execute(
                json!({"action": "edit", "name": "my-skill", "content": "# New content"}),
                &ctx,
            )
            .await
            .expect("edit nested skill");
        assert!(result.contains("my-skill"));
        assert!(result.contains("edit"));
        // Verify the file was updated
        let updated = std::fs::read_to_string(skill_dir.join("SKILL.md")).expect("read");
        assert_eq!(updated, "# New content");
    }

    #[tokio::test]
    async fn skills_categories_lists_categories() {
        let dir = TempDir::new().expect("tmpdir");
        // Create skills in two categories
        let s1 = dir.path().join("skills").join("media").join("gif-search");
        std::fs::create_dir_all(&s1).expect("mkdir");
        std::fs::write(s1.join("SKILL.md"), "# GIF Search").expect("write");

        let s2 = dir.path().join("skills").join("media").join("video-edit");
        std::fs::create_dir_all(&s2).expect("mkdir");
        std::fs::write(s2.join("SKILL.md"), "# Video Edit").expect("write");

        let s3 = dir.path().join("skills").join("research").join("arxiv");
        std::fs::create_dir_all(&s3).expect("mkdir");
        std::fs::write(s3.join("SKILL.md"), "# ArXiv").expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillsCategoriesList
            .execute(json!({}), &ctx)
            .await
            .expect("categories");
        assert!(result.contains("media"), "missing media category");
        assert!(result.contains("research"), "missing research category");
        assert!(result.contains("2 skills"), "media should have 2 skills");
        assert!(result.contains("1 skill"), "research should have 1 skill");
    }
}

// ─── skill_manage ──────────────────────────────────────────────

/// Manage skills: create, edit content, or delete a skill from the
/// `~/.edgecrab/skills/` directory without leaving the conversation.
///
/// WHY: Mirrors hermes `skill_commands.py` which lets users create and
/// iterate on skill prompts inline. Writing files outside edgecrab_home
/// is blocked by path-traversal checks.
pub struct SkillManageTool;

#[derive(Deserialize)]
struct ManageArgs {
    /// One of: "create", "edit", "delete", "patch", "write_file", "remove_file"
    action: String,
    /// Skill name (directory name under skills/)
    name: String,
    /// New content for the SKILL.md file (required for create / edit)
    #[serde(default)]
    content: Option<String>,
    /// String to find in SKILL.md (required for patch)
    #[serde(default)]
    old_string: Option<String>,
    /// Replacement string (required for patch)
    #[serde(default)]
    new_string: Option<String>,
    /// Replace all occurrences (for patch, default false = exactly 1)
    #[serde(default)]
    replace_all: bool,
    /// Optional subdirectory category for create (e.g. "mlops/training")
    #[serde(default)]
    category: Option<String>,
    /// Path within the skill dir (required for write_file / remove_file).
    /// Must be under references/, templates/, scripts/, or assets/.
    #[serde(default)]
    file_path: Option<String>,
    /// Content for write_file action
    #[serde(default)]
    file_content: Option<String>,
}

#[async_trait]
impl ToolHandler for SkillManageTool {
    fn name(&self) -> &'static str {
        "skill_manage"
    }

    fn toolset(&self) -> &'static str {
        "skills"
    }

    fn emoji(&self) -> &'static str {
        "✏️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skill_manage".into(),
            description: "Create, edit, patch, delete a skill, or write/remove supporting files in ~/.edgecrab/skills/. \
                           Use action='create' or 'edit' with content to write SKILL.md. \
                           Use action='patch' with old_string and new_string for targeted replacement. \
                           Use action='delete' to remove the skill directory. \
                           Use action='write_file' with file_path and file_content to add/overwrite a supporting file. \
                           Use action='remove_file' with file_path to remove a supporting file."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "edit", "patch", "delete", "write_file", "remove_file"],
                        "description": "Operation to perform"
                    },
                    "name": {
                        "type": "string",
                        "description": "Skill name (alphanumeric, hyphens, underscores only)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Markdown content for SKILL.md (required for create/edit)"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "Exact string to find in SKILL.md (required for patch)"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "Replacement string (required for patch)"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace all occurrences (patch only, default: false = exactly 1)"
                    },
                    "category": {
                        "type": "string",
                        "description": "Category subdirectory for create (e.g. 'mlops/training')"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Relative path within the skill dir for write_file/remove_file (must be under references/, templates/, scripts/, or assets/)"
                    },
                    "file_content": {
                        "type": "string",
                        "description": "Content for write_file action"
                    }
                },
                "required": ["action", "name"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: ManageArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "skill_manage".into(),
                message: e.to_string(),
            })?;

        // Validate name: only safe characters, no path traversal
        let valid_name = args
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
        if !valid_name || args.name.is_empty() || args.name.contains("..") {
            return Err(ToolError::PermissionDenied(
                "Skill name must contain only alphanumeric characters, hyphens, and underscores"
                    .into(),
            ));
        }

        let skills_base = ctx.config.edgecrab_home.join("skills");

        // For create: use explicit category path or flat.
        // For all other actions: search recursively to find the existing skill.
        let skill_dir = if args.action == "create" {
            if let Some(ref cat) = args.category {
                // Validate category: no traversal, alphanumeric + hyphens + slashes
                if cat.contains("..") || cat.starts_with('/') {
                    return Err(ToolError::PermissionDenied(
                        "Category must not contain '..' or start with '/'".into(),
                    ));
                }
                skills_base.join(cat).join(&args.name)
            } else {
                skills_base.join(&args.name)
            }
        } else {
            // Try recursive search first, fall back to flat path
            find_skill_dir(&skills_base, &args.name).unwrap_or_else(|| skills_base.join(&args.name))
        };

        match args.action.as_str() {
            "create" | "edit" => {
                let content = args
                    .content
                    .as_deref()
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: "content is required for create/edit".into(),
                    })?;

                tokio::fs::create_dir_all(&skill_dir)
                    .await
                    .map_err(|e| ToolError::Other(format!("Cannot create skill dir: {e}")))?;

                let skill_path = skill_dir.join("SKILL.md");
                tokio::fs::write(&skill_path, content)
                    .await
                    .map_err(|e| ToolError::Other(format!("Cannot write SKILL.md: {e}")))?;

                // Invalidate the skills prompt cache so the updated skill is
                // picked up by the next system-prompt rebuild.
                if let Some(ref f) = ctx.on_skills_changed {
                    f();
                }

                Ok(format!(
                    "Skill '{}' {}d at {}",
                    args.name,
                    args.action,
                    skill_path.display()
                ))
            }
            "delete" => {
                if !skill_dir.exists() {
                    return Err(ToolError::NotFound(format!(
                        "Skill '{}' does not exist",
                        args.name
                    )));
                }
                tokio::fs::remove_dir_all(&skill_dir)
                    .await
                    .map_err(|e| ToolError::Other(format!("Cannot delete skill: {e}")))?;

                // Invalidate the skills prompt cache.
                if let Some(ref f) = ctx.on_skills_changed {
                    f();
                }

                Ok(format!("Skill '{}' deleted.", args.name))
            }
            "patch" => {
                let old = args
                    .old_string
                    .as_deref()
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: "old_string is required for patch".into(),
                    })?;
                let new = args
                    .new_string
                    .as_deref()
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: "new_string is required for patch".into(),
                    })?;

                let skill_path = skill_dir.join("SKILL.md");
                if !skill_path.is_file() {
                    return Err(ToolError::NotFound(format!(
                        "Skill '{}' does not exist",
                        args.name
                    )));
                }

                let current = tokio::fs::read_to_string(&skill_path)
                    .await
                    .map_err(|e| ToolError::Other(format!("Cannot read SKILL.md: {e}")))?;

                // Require exactly one match unless replace_all is set
                let count = current.matches(old).count();
                if count == 0 {
                    return Err(ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: format!(
                            "old_string not found in skill '{}'. \
                             Verify the exact text including whitespace.",
                            args.name
                        ),
                    });
                }
                if count > 1 && !args.replace_all {
                    return Err(ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: format!(
                            "old_string matches {} times in skill '{}'. \
                             Use replace_all=true to replace all, or provide more context.",
                            count, args.name
                        ),
                    });
                }

                let updated = if args.replace_all {
                    current.replace(old, new)
                } else {
                    current.replacen(old, new, 1)
                };
                tokio::fs::write(&skill_path, &updated)
                    .await
                    .map_err(|e| ToolError::Other(format!("Cannot write SKILL.md: {e}")))?;

                // Invalidate the skills prompt cache.
                if let Some(ref f) = ctx.on_skills_changed {
                    f();
                }

                Ok(format!(
                    "Skill '{}' patched: replaced {} occurrence(s).",
                    args.name,
                    if args.replace_all { count } else { 1 }
                ))
            }
            "write_file" => {
                // Write a supporting file (references/, templates/, scripts/, assets/)
                let fp = args
                    .file_path
                    .as_deref()
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: "file_path is required for write_file".into(),
                    })?;
                let fc = args
                    .file_content
                    .as_deref()
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: "file_content is required for write_file".into(),
                    })?;

                // Validate file_path: must be under an allowed subdirectory
                if fp.contains("..") {
                    return Err(ToolError::PermissionDenied(
                        "Path traversal ('..') is not allowed".into(),
                    ));
                }
                let allowed_subdirs = ["references", "templates", "scripts", "assets"];
                let first_component = fp.split('/').next().unwrap_or("");
                if !allowed_subdirs.contains(&first_component) {
                    return Err(ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: format!(
                            "file_path must be under one of: {}. Got: '{}'",
                            allowed_subdirs.join(", "),
                            fp
                        ),
                    });
                }

                if !skill_dir.exists() {
                    return Err(ToolError::NotFound(format!(
                        "Skill '{}' does not exist. Create it first.",
                        args.name
                    )));
                }

                let target = skill_dir.join(fp);
                if let Some(parent) = target.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|e| ToolError::Other(format!("Cannot create dir: {e}")))?;
                }
                tokio::fs::write(&target, fc)
                    .await
                    .map_err(|e| ToolError::Other(format!("Cannot write file: {e}")))?;

                if let Some(ref f) = ctx.on_skills_changed {
                    f();
                }

                Ok(format!("Wrote '{}' in skill '{}'.", fp, args.name))
            }
            "remove_file" => {
                // Remove a supporting file
                let fp = args
                    .file_path
                    .as_deref()
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: "file_path is required for remove_file".into(),
                    })?;

                if fp.contains("..") {
                    return Err(ToolError::PermissionDenied(
                        "Path traversal ('..') is not allowed".into(),
                    ));
                }
                let allowed_subdirs = ["references", "templates", "scripts", "assets"];
                let first_component = fp.split('/').next().unwrap_or("");
                if !allowed_subdirs.contains(&first_component) {
                    return Err(ToolError::InvalidArgs {
                        tool: "skill_manage".into(),
                        message: format!(
                            "file_path must be under one of: {}. Got: '{}'",
                            allowed_subdirs.join(", "),
                            fp
                        ),
                    });
                }

                let target = skill_dir.join(fp);
                if !target.is_file() {
                    return Err(ToolError::NotFound(format!(
                        "File '{}' not found in skill '{}'",
                        fp, args.name
                    )));
                }

                tokio::fs::remove_file(&target)
                    .await
                    .map_err(|e| ToolError::Other(format!("Cannot remove file: {e}")))?;

                if let Some(ref f) = ctx.on_skills_changed {
                    f();
                }

                Ok(format!("Removed '{}' from skill '{}'.", fp, args.name))
            }
            other => Err(ToolError::InvalidArgs {
                tool: "skill_manage".into(),
                message: format!(
                    "Unknown action '{}'. Use: create, edit, patch, delete, write_file, remove_file",
                    other
                ),
            }),
        }
    }
}

inventory::submit!(&SkillManageTool as &dyn ToolHandler);

#[cfg(test)]
mod skill_manage_tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx_in(dir: &std::path::Path) -> ToolContext {
        let mut ctx = ToolContext::test_context();
        ctx.config.edgecrab_home = dir.to_path_buf();
        ctx
    }

    #[tokio::test]
    async fn create_skill_roundtrip() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = SkillManageTool
            .execute(
                json!({"action": "create", "name": "my_skill", "content": "# My Skill\ndescription"}),
                &ctx,
            )
            .await
            .expect("create");
        assert!(result.contains("my_skill"));
        assert!(result.contains("created"));

        // Verify we can view it
        let view = SkillViewTool
            .execute(json!({"name": "my_skill"}), &ctx)
            .await
            .expect("view");
        assert!(view.contains("# My Skill"));
    }

    #[tokio::test]
    async fn edit_skill_updates_content() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        SkillManageTool
            .execute(
                json!({"action": "create", "name": "skill1", "content": "old"}),
                &ctx,
            )
            .await
            .expect("create");

        SkillManageTool
            .execute(
                json!({"action": "edit", "name": "skill1", "content": "new content"}),
                &ctx,
            )
            .await
            .expect("edit");

        let view = SkillViewTool
            .execute(json!({"name": "skill1"}), &ctx)
            .await
            .expect("view");
        assert!(view.contains("new content"));
        assert!(!view.contains("old"));
    }

    #[tokio::test]
    async fn delete_skill_removes_dir() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        SkillManageTool
            .execute(
                json!({"action": "create", "name": "deleteme", "content": "bye"}),
                &ctx,
            )
            .await
            .expect("create");

        let result = SkillManageTool
            .execute(json!({"action": "delete", "name": "deleteme"}), &ctx)
            .await
            .expect("delete");
        assert!(result.contains("deleted"));

        // List should not show it
        let list = SkillsListTool.execute(json!({}), &ctx).await.expect("list");
        assert!(!list.contains("deleteme"));
    }

    #[tokio::test]
    async fn traversal_blocked() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());
        let result = SkillManageTool
            .execute(
                json!({"action": "create", "name": "../../../evil", "content": "x"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_nonexistent_returns_error() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());
        let result = SkillManageTool
            .execute(json!({"action": "delete", "name": "ghost"}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn patch_skill_replaces_unique_occurrence() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        SkillManageTool
            .execute(
                json!({"action": "create", "name": "patchable", "content": "Hello old world"}),
                &ctx,
            )
            .await
            .expect("create");

        let result = SkillManageTool
            .execute(
                json!({"action": "patch", "name": "patchable", "old_string": "old", "new_string": "new"}),
                &ctx,
            )
            .await
            .expect("patch");
        assert!(result.contains("patched"));

        let view = SkillViewTool
            .execute(json!({"name": "patchable"}), &ctx)
            .await
            .expect("view");
        assert!(
            view.contains("new world"),
            "expected 'new world' in: {view}"
        );
        assert!(
            !view.contains("old world"),
            "unexpected 'old world' in: {view}"
        );
    }

    #[tokio::test]
    async fn patch_skill_no_match_returns_error() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        SkillManageTool
            .execute(
                json!({"action": "create", "name": "nomatch", "content": "content here"}),
                &ctx,
            )
            .await
            .expect("create");

        let result = SkillManageTool
            .execute(
                json!({"action": "patch", "name": "nomatch", "old_string": "not_present", "new_string": "anything"}),
                &ctx,
            )
            .await;
        assert!(result.is_err(), "patch with no match should error");
    }

    #[tokio::test]
    async fn patch_skill_multiple_matches_returns_error() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        SkillManageTool
            .execute(
                json!({"action": "create", "name": "multimatch", "content": "dup dup here"}),
                &ctx,
            )
            .await
            .expect("create");

        let result = SkillManageTool
            .execute(
                json!({"action": "patch", "name": "multimatch", "old_string": "dup", "new_string": "REPLACED"}),
                &ctx,
            )
            .await;
        assert!(result.is_err(), "patch with multiple matches should error");
    }

    #[tokio::test]
    async fn patch_skill_not_found_returns_error() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());
        let result = SkillManageTool
            .execute(
                json!({"action": "patch", "name": "ghost", "old_string": "x", "new_string": "y"}),
                &ctx,
            )
            .await;
        assert!(result.is_err(), "patch on nonexistent skill should error");
    }

    /// Verify that `on_skills_changed` is invoked for create, edit, patch,
    /// and delete actions so that consumers (e.g. prompt builder cache)
    /// can react to skill mutations without polling.
    #[tokio::test]
    async fn on_skills_changed_callback_invoked() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let dir = TempDir::new().expect("tmpdir");
        let counter = Arc::new(AtomicU32::new(0));

        let mut ctx = ctx_in(dir.path());
        let c = Arc::clone(&counter);
        ctx.on_skills_changed = Some(Arc::new(move || {
            c.fetch_add(1, Ordering::Relaxed);
        }));

        // create → counter = 1
        SkillManageTool
            .execute(
                json!({"action": "create", "name": "cb_skill", "content": "hello world"}),
                &ctx,
            )
            .await
            .expect("create");
        assert_eq!(
            counter.load(Ordering::Relaxed),
            1,
            "create should fire callback"
        );

        // edit → counter = 2
        SkillManageTool
            .execute(
                json!({"action": "edit", "name": "cb_skill", "content": "hello earth"}),
                &ctx,
            )
            .await
            .expect("edit");
        assert_eq!(
            counter.load(Ordering::Relaxed),
            2,
            "edit should fire callback"
        );

        // patch → counter = 3
        SkillManageTool
            .execute(
                json!({"action": "patch", "name": "cb_skill", "old_string": "hello earth", "new_string": "hi there"}),
                &ctx,
            )
            .await
            .expect("patch");
        assert_eq!(
            counter.load(Ordering::Relaxed),
            3,
            "patch should fire callback"
        );

        // delete → counter = 4
        SkillManageTool
            .execute(json!({"action": "delete", "name": "cb_skill"}), &ctx)
            .await
            .expect("delete");
        assert_eq!(
            counter.load(Ordering::Relaxed),
            4,
            "delete should fire callback"
        );
    }

    #[tokio::test]
    async fn write_file_and_remove_file() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        // Create skill first
        SkillManageTool
            .execute(
                json!({"action": "create", "name": "wf_skill", "content": "# Test"}),
                &ctx,
            )
            .await
            .expect("create");

        // Write a supporting file
        let result = SkillManageTool
            .execute(
                json!({"action": "write_file", "name": "wf_skill", "file_path": "references/api.md", "file_content": "API docs"}),
                &ctx,
            )
            .await
            .expect("write_file");
        assert!(result.contains("Wrote"));

        // Verify file exists
        let ref_file = dir.path().join("skills/wf_skill/references/api.md");
        assert!(ref_file.is_file());
        assert_eq!(std::fs::read_to_string(&ref_file).unwrap(), "API docs");

        // View the supporting file via skill_view file_path
        let view_result = SkillViewTool
            .execute(
                json!({"name": "wf_skill", "file_path": "references/api.md"}),
                &ctx,
            )
            .await
            .expect("view file_path");
        assert!(view_result.contains("API docs"));

        // Remove the file
        let rm_result = SkillManageTool
            .execute(
                json!({"action": "remove_file", "name": "wf_skill", "file_path": "references/api.md"}),
                &ctx,
            )
            .await
            .expect("remove_file");
        assert!(rm_result.contains("Removed"));
        assert!(!ref_file.is_file());
    }

    #[tokio::test]
    async fn write_file_invalid_subdir_blocked() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        SkillManageTool
            .execute(
                json!({"action": "create", "name": "bad_wf", "content": "# Test"}),
                &ctx,
            )
            .await
            .expect("create");

        let result = SkillManageTool
            .execute(
                json!({"action": "write_file", "name": "bad_wf", "file_path": "evil/hack.sh", "file_content": "bad"}),
                &ctx,
            )
            .await;
        assert!(
            result.is_err(),
            "write_file to invalid subdir should be blocked"
        );
    }

    #[tokio::test]
    async fn patch_replace_all() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        SkillManageTool
            .execute(
                json!({"action": "create", "name": "ra_skill", "content": "foo bar foo baz foo"}),
                &ctx,
            )
            .await
            .expect("create");

        let result = SkillManageTool
            .execute(
                json!({"action": "patch", "name": "ra_skill", "old_string": "foo", "new_string": "qux", "replace_all": true}),
                &ctx,
            )
            .await
            .expect("patch replace_all");
        assert!(result.contains("3 occurrence(s)"));

        let view = SkillViewTool
            .execute(json!({"name": "ra_skill"}), &ctx)
            .await
            .expect("view");
        assert!(!view.contains("foo"));
        assert!(view.contains("qux"));
    }

    #[tokio::test]
    async fn create_with_category() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = SkillManageTool
            .execute(
                json!({"action": "create", "name": "axolotl", "content": "# Axolotl", "category": "mlops/training"}),
                &ctx,
            )
            .await
            .expect("create with category");
        assert!(result.contains("axolotl"));

        // Verify file exists under the category path
        let skill_file = dir.path().join("skills/mlops/training/axolotl/SKILL.md");
        assert!(skill_file.is_file());
    }

    #[tokio::test]
    async fn skills_list_finds_nested_category_skills() {
        let dir = TempDir::new().expect("tmpdir");
        // Create a nested category skill manually
        let nested = dir.path().join("skills/mlops/training/trl");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::write(
            nested.join("SKILL.md"),
            "---\nname: trl\ndescription: Fine-tuning\n---\n# TRL",
        )
        .expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillsListTool.execute(json!({}), &ctx).await.expect("list");
        assert!(
            result.contains("trl"),
            "nested category skill should appear in list: {result}"
        );
    }

    #[tokio::test]
    async fn skills_list_respects_disabled() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("disabled_skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: disabled_skill\n---\n# Test",
        )
        .expect("write");

        let mut ctx = ctx_in(dir.path());
        ctx.config.disabled_skills = vec!["disabled_skill".to_string()];

        let result = SkillsListTool.execute(json!({}), &ctx).await.expect("list");
        assert!(
            !result.contains("disabled_skill"),
            "disabled skill should be hidden: {result}"
        );
    }

    #[tokio::test]
    async fn skill_view_lists_supporting_files() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("rich_skill");
        let refs = skill_dir.join("references");
        std::fs::create_dir_all(&refs).expect("mkdir");
        std::fs::write(skill_dir.join("SKILL.md"), "# Rich Skill").expect("write");
        std::fs::write(refs.join("api.md"), "API reference").expect("write");

        let ctx = ctx_in(dir.path());
        let result = SkillViewTool
            .execute(json!({"name": "rich_skill"}), &ctx)
            .await
            .expect("view");
        assert!(
            result.contains("Supporting Files"),
            "should list supporting files: {result}"
        );
        assert!(
            result.contains("references/api.md"),
            "should list the file: {result}"
        );
    }
}

// ────────────────────────────────────────────────────────────────
// PHASE 2: Environment Variables, Trust Levels, Registry Integration
// ────────────────────────────────────────────────────────────────

/// Check which required environment variables are missing for a skill.
fn check_missing_env_vars(meta: &SkillMeta) -> Vec<&EnvVarSpec> {
    meta.required_environment_variables
        .iter()
        .filter(|spec| std::env::var(&spec.name).is_err())
        .collect()
}

/// Format environment variable guidance for skill_view.
fn format_env_var_guidance(missing_vars: &[&EnvVarSpec]) -> String {
    if missing_vars.is_empty() {
        return String::new();
    }

    let mut output = String::from("\n### ⚠️ Required Environment Variables\n\n");
    output.push_str(
        "The following environment variables are **NOT SET** but may be needed for this skill:\n\n",
    );

    for spec in missing_vars {
        output.push_str(&format!("- **`{}`**", spec.name));
        if let Some(prompt) = &spec.prompt {
            output.push_str(&format!(" — {}", prompt));
        }
        output.push('\n');

        if let Some(help) = &spec.help {
            output.push_str(&format!("  - Help: _{}_\n", help));
        }

        if spec.required_for != "optional" {
            output.push_str(&format!("  - Required for: {}\n", spec.required_for));
        }

        output.push_str(&format!("  - Set with: `export {}=<value>`\n", spec.name));
    }

    output.push_str("\n_To skip these warnings, set the variables or use skill_setup_env._\n");
    output
}

// ─── Trust Levels ──────────────────────────────────────────────

/// Trust level for a skill (builtin > official > trusted > community).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    Builtin,
    Official,
    Trusted,
    Community,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustLevel::Builtin => write!(f, "builtin"),
            TrustLevel::Official => write!(f, "official"),
            TrustLevel::Trusted => write!(f, "trusted"),
            TrustLevel::Community => write!(f, "community"),
        }
    }
}

impl TrustLevel {
    /// Verdict from security scan required before install.
    pub fn min_verdict_for_install(&self) -> &'static str {
        match self {
            TrustLevel::Builtin => "any",              // No scanning required
            TrustLevel::Official => "safe_or_caution", // Official can have minor issues
            TrustLevel::Trusted => "safe_or_caution",  // Trusted sources similar
            TrustLevel::Community => "safe",           // Community must be clean
        }
    }
}

impl std::str::FromStr for TrustLevel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "builtin" => TrustLevel::Builtin,
            "official" => TrustLevel::Official,
            "trusted" => TrustLevel::Trusted,
            _ => TrustLevel::Community,
        })
    }
}

// ─── Skills Registry (Phase 2) ──────────────────────────────────

/// Metadata for a skill in a remote registry (skills.sh, well-known, GitHub).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public infrastructure used by hub tools and future integrations
pub struct RegistrySkillMeta {
    pub name: String,
    pub description: String,
    pub source: String,     // "skills.sh", "well-known", "github-tap"
    pub identifier: String, // source-specific ID
    pub trust_level: TrustLevel,
    pub repo: Option<String>,
    pub url: Option<String>, // For well-known endpoints
}

/// Fetch skills from skills.sh registry API.
/// API: GET https://skills.sh/api/search?q=<query>&limit=<limit>
pub async fn search_skills_sh_registry(
    query: &str,
    limit: usize,
) -> Result<Vec<RegistrySkillMeta>, String> {
    let encoded_query: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let search_url = format!(
        "https://skills.sh/api/search?q={}&limit={}",
        encoded_query, limit
    );

    // Phase 2: Use reqwest to query the actual API
    let resp = reqwest::get(&search_url)
        .await
        .map_err(|e| format!("skills.sh search failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("skills.sh returned HTTP {}", resp.status()));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse skills.sh response: {e}"))?;

    let skills = data
        .get("skills")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();

    let results = skills
        .iter()
        .filter_map(|item| {
            Some(RegistrySkillMeta {
                name: item.get("name")?.as_str()?.to_string(),
                description: item
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                source: "skills.sh".into(),
                identifier: item
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string(),
                trust_level: TrustLevel::Community,
                repo: item
                    .get("source")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string()),
                url: Some(format!(
                    "https://skills.sh/{}",
                    item.get("id").and_then(|i| i.as_str()).unwrap_or("")
                )),
            })
        })
        .take(limit)
        .collect();

    Ok(results)
}

/// Fetch skills from well-known endpoints discovery protocol.
/// Spec: https://agentskills.io/well-known-skills/
pub async fn discover_well_known_skills(base_url: &str) -> Result<Vec<RegistrySkillMeta>, String> {
    let well_known_url = format!(
        "{}/.well-known/skills/index.json",
        base_url.trim_end_matches('/')
    );

    let resp = reqwest::get(&well_known_url)
        .await
        .map_err(|e| format!("Well-known skills discovery failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Well-known endpoint returned HTTP {}",
            resp.status()
        ));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse well-known response: {e}"))?;

    let skills = data
        .get("skills")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();

    let results = skills
        .iter()
        .filter_map(|item| {
            let name = item.get("name").and_then(|n| n.as_str())?.to_string();
            Some(RegistrySkillMeta {
                name: name.clone(),
                description: item
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                source: "well-known".into(),
                identifier: format!(
                    "well-known:{}/.well-known/skills/{}",
                    base_url.trim_end_matches('/'),
                    name
                ),
                trust_level: TrustLevel::Community,
                repo: None,
                url: Some(format!(
                    "{}/.well-known/skills/{}",
                    base_url.trim_end_matches('/'),
                    name
                )),
            })
        })
        .collect();

    Ok(results)
}

// ─── skills_hub Tool (Phase 2) ──────────────────────────────────

pub struct SkillsHubTool;

#[derive(Deserialize)]
struct HubArgs {
    action: String, // "search", "browse", "inspect", "install", "update", "uninstall"
    query: Option<String>,
    source: Option<String>, // "skills.sh", "well-known", "github-tap"
    #[serde(default)]
    force: bool, // Override trust policy
}

#[async_trait]
impl ToolHandler for SkillsHubTool {
    fn name(&self) -> &'static str {
        "skills_hub"
    }

    fn toolset(&self) -> &'static str {
        "skills"
    }

    fn emoji(&self) -> &'static str {
        "🌐"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skills_hub".into(),
            description: "Search, browse, and install skills from remote registries (skills.sh, well-known endpoints, GitHub taps).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["search", "browse", "inspect", "install", "update", "uninstall"],
                        "description": "Hub action: search, browse, inspect, install, update, or uninstall"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query or skill identifier for inspect/install"
                    },
                    "source": {
                        "type": "string",
                        "enum": ["skills.sh", "well-known", "github-tap", "all"],
                        "description": "Registry source (default: all)"
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Override install policy for community/untrusted skills (requires explicit confirmation)"
                    }
                },
                "required": ["action"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: HubArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "skills_hub".into(),
            message: e.to_string(),
        })?;

        match args.action.as_str() {
            "search" => {
                let query = args.query.ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skills_hub".into(),
                    message: "search requires 'query' parameter".into(),
                })?;

                let mut output = format!("🔍 Searching registries for: '{}'\n\n", query);

                let source_filter = args.source.as_deref().unwrap_or("all");

                // Search skills.sh registry
                if source_filter == "all" || source_filter == "skills.sh" {
                    match search_skills_sh_registry(&query, 10).await {
                        Ok(results) => {
                            output.push_str("### skills.sh Results\n\n");
                            if results.is_empty() {
                                output.push_str("- No results\n");
                            }
                            for meta in results {
                                output.push_str(&format!(
                                    "- **{}** — {} [trust: {}]\n",
                                    meta.name, meta.description, meta.trust_level
                                ));
                            }
                            output.push('\n');
                        }
                        Err(msg) => output.push_str(&format!("ℹ️  skills.sh: {}\n\n", msg)),
                    }
                }

                if source_filter == "all" || source_filter == "github-tap" {
                    let taps = super::skills_hub::read_taps();
                    if !taps.is_empty() {
                        output.push_str("### GitHub Taps\n\n");
                        for tap in taps {
                            output.push_str(&format!("- **{}** ({})\n", tap.name, tap.url));
                        }
                        output.push('\n');
                    }
                }

                if (source_filter == "all" || source_filter == "well-known")
                    && (query.starts_with("http://") || query.starts_with("https://"))
                {
                    match discover_well_known_skills(&query).await {
                        Ok(results) => {
                            output.push_str("### Well-known Endpoint Results\n\n");
                            if results.is_empty() {
                                output.push_str("- No results\n");
                            }
                            for meta in results {
                                output.push_str(&format!(
                                    "- **{}** — {} [trust: {}]\n",
                                    meta.name, meta.description, meta.trust_level
                                ));
                            }
                            output.push('\n');
                        }
                        Err(msg) => output.push_str(&format!("ℹ️  well-known: {}\n\n", msg)),
                    }
                }

                output.push_str("\n💡 **Pro tip**: Use `skills_hub inspect <identifier>` to view details before installing.\n");

                Ok(output)
            }

            "browse" => {
                let mut output = String::from("🌐 Skills Hub — Sources\n\n");
                output.push_str("Featured registry: https://skills.sh\n\n");
                output.push_str("- skills.sh (public registry)\n");
                output.push_str("- well-known endpoints (.well-known/skills/index.json)\n");
                output.push_str("- GitHub taps (custom sources)\n");
                output.push_str("- optional skills (bundled local skills)\n\n");

                let taps = super::skills_hub::read_taps();
                if !taps.is_empty() {
                    output.push_str("Configured taps:\n");
                    for tap in taps {
                        output.push_str(&format!("- {} -> {}\n", tap.name, tap.url));
                    }
                    output.push('\n');
                }

                let installed = super::skills_hub::read_lock();
                if !installed.is_empty() {
                    output.push_str("Installed hub skills:\n");
                    for (name, entry) in installed {
                        output.push_str(&format!("- {} ({})\n", name, entry.source));
                    }
                }

                output.push_str("\nTry: `skills_hub search coding`\n");
                Ok(output)
            }

            "inspect" => {
                let identifier = args.query.ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skills_hub".into(),
                    message: "inspect requires 'query' (skill identifier)".into(),
                })?;

                let output = format!(
                    "📋 Skill Details for: {}\n\nUse `skills_hub install {}` to install, or `skills_hub search {}` to find matching sources.",
                    identifier, identifier, identifier
                );
                Ok(output)
            }

            "install" => {
                let identifier = args.query.ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skills_hub".into(),
                    message: "install requires 'query' (skill identifier)".into(),
                })?;

                let skills_dir = ctx.config.edgecrab_home.join("skills");
                let optional_dir = super::skills_sync::optional_skills_dir();

                // ── official/ prefix → install from optional-skills dir ──
                if identifier.starts_with("official/") {
                    let bundle = super::skills_hub::load_official_skill_bundle(
                        &identifier,
                        optional_dir.as_deref(),
                    )
                    .map_err(ToolError::Other);
                    return bundle.and_then(|bundle| {
                        let skill_name = bundle.name.clone();
                        super::skills_hub::install_skill(&bundle, &skills_dir, args.force)
                            .map(|m| format!("{}\n\nActivate with skill_view {}", m, skill_name))
                            .map_err(ToolError::Other)
                    });
                }

                // ── owner/repo/path → GitHub install ──
                if identifier.contains('/') {
                    return super::skills_hub::install_github_skill(
                        &identifier,
                        &skills_dir,
                        args.force,
                    )
                    .await
                    .map(|m| {
                        let skill_name = identifier.split('/').next_back().unwrap_or("skill");
                        format!("{}\n\nActivate with skill_view {}", m, skill_name)
                    })
                    .map_err(ToolError::Other);
                }

                // ── bare name → search optional-skills ──
                let optional_search_root = optional_dir
                    .clone()
                    .unwrap_or_else(|| ctx.config.edgecrab_home.join("optional-skills"));
                let candidates =
                    super::skills_hub::search_optional_skills(&optional_search_root, &identifier);
                if let Some(meta) = candidates.first() {
                    let bundle = super::skills_hub::load_official_skill_bundle(
                        &meta.identifier,
                        optional_dir.as_deref(),
                    )
                    .map_err(ToolError::Other)?;
                    let skill_name = bundle.name.clone();
                    return super::skills_hub::install_skill(&bundle, &skills_dir, args.force)
                        .map(|m| format!("{}\n\nActivate with skill_view {}", m, skill_name))
                        .map_err(ToolError::Other);
                }

                Err(ToolError::NotFound(format!(
                    "Skill '{}' not found in optional skills. Use official/<category>/<skill> or owner/repo/path for GitHub install.",
                    identifier
                )))
            }

            "update" => {
                let lock = super::skills_hub::read_lock();
                if lock.is_empty() {
                    return Ok("No hub-installed skills found.".into());
                }

                let mut output = String::from("Installed hub skills:\n\n");
                for (name, entry) in lock {
                    output.push_str(&format!(
                        "- {} | source={} | id={} | installed={}\n",
                        name, entry.source, entry.identifier, entry.installed_at
                    ));
                }
                output.push_str("\nUse skills_hub install <identifier> force=true to reinstall.");
                Ok(output)
            }

            "uninstall" => {
                let identifier = args.query.ok_or_else(|| ToolError::InvalidArgs {
                    tool: "skills_hub".into(),
                    message: "uninstall requires 'query' (skill name)".into(),
                })?;
                let skills_dir = ctx.config.edgecrab_home.join("skills");
                super::skills_hub::uninstall_skill(&identifier, &skills_dir)
                    .map_err(ToolError::Other)
            }

            other => Err(ToolError::InvalidArgs {
                tool: "skills_hub".into(),
                message: format!(
                    "Unknown action '{}'. Use: search, browse, inspect, install, update, uninstall",
                    other
                ),
            }),
        }
    }
}

inventory::submit!(&SkillsHubTool as &dyn ToolHandler);

#[cfg(test)]
mod skills_hub_tests {
    use super::*;

    #[tokio::test]
    async fn hub_search_returns_registry_info() {
        let result = SkillsHubTool
            .execute(
                json!({"action": "search", "query": "web"}),
                &ToolContext::test_context(),
            )
            .await;
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("skills.sh"));
    }

    #[tokio::test]
    async fn hub_browse_shows_featured() {
        let result = SkillsHubTool
            .execute(json!({"action": "browse"}), &ToolContext::test_context())
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("https://skills.sh"));
    }

    #[tokio::test]
    async fn hub_install_requires_identifier() {
        let result = SkillsHubTool
            .execute(json!({"action": "install"}), &ToolContext::test_context())
            .await;
        assert!(result.is_err());
    }
}
