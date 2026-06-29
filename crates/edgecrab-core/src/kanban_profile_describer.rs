//! Profile auto-describer — Hermes `profile_describer.py` subset.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use edgequake_llm::LLMProvider;
use serde::Deserialize;
use serde_json::Value;

use crate::auxiliary_model::resolve_side_task_provider_and_model;
use crate::config::AppConfig;
use crate::kanban_profiles::{
    install_root, load_config_for_profile, normalize_profile_name, profile_effective_home,
    profile_exists, write_profile_meta,
};

const MAX_SKILLS_FOR_PROMPT: usize = 60;

const SYSTEM_PROMPT: &str = r#"You are a profile-describer for the EdgeCrab kanban board.

A user runs multiple "profiles" — distinct agent identities, each with their own skills,
model, and configuration. The kanban orchestrator routes work to whichever profile best
fits each task. Every profile needs a short, concrete description of what it's good at.

Produce a single JSON object:

  { "description": "<1-2 sentence description, plain prose, no preamble>" }

Rules:
  - Lead with the profile's strongest capability (orchestrator routing signal).
  - Stay concrete; <= 280 characters.
  - Never invent capabilities the skills don't suggest.
  - No code fences — JSON only.
"#;

#[derive(Debug, Clone)]
pub struct DescribeProfileOutcome {
    pub profile_name: String,
    pub ok: bool,
    pub reason: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LlmResponse {
    #[serde(default)]
    description: Option<String>,
}

fn collect_skill_names(profile_dir: &Path) -> Vec<String> {
    let skills_dir = profile_dir.join("skills");
    if !skills_dir.is_dir() {
        return Vec::new();
    }
    let mut names = Vec::new();
    for entry in walkdir_light(&skills_dir) {
        if entry.file_name().is_some_and(|n| n == "SKILL.md")
            && let Ok(rel) = entry.strip_prefix(&skills_dir)
        {
            let parts: Vec<_> = rel
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .filter(|p| *p != "SKILL.md")
                .collect();
            if parts.is_empty() {
                continue;
            }
            let label = if parts.len() == 1 {
                parts[0].to_string()
            } else {
                format!("{}/{}", parts[0], parts[parts.len() - 1])
            };
            names.push(label);
        }
    }
    names.sort();
    if names.len() <= MAX_SKILLS_FOR_PROMPT {
        return names;
    }
    let step = names.len() as f64 / MAX_SKILLS_FOR_PROMPT as f64;
    (0..MAX_SKILLS_FOR_PROMPT)
        .map(|i| names[(i as f64 * step) as usize].clone())
        .collect()
}

fn walkdir_light(root: &Path) -> Vec<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

fn extract_json_object(raw: &str) -> Option<Value> {
    let stripped = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let start = stripped.find('{')?;
    let end = stripped.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&stripped[start..=end]).ok()
}

fn read_profile_meta(profile_dir: &Path) -> (String, bool) {
    let path = profile_dir.join("profile.yaml");
    if !path.is_file() {
        return (String::new(), false);
    }
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return (String::new(), false);
    };
    let Ok(val) = serde_yml::from_str::<serde_yml::Value>(&raw) else {
        return (String::new(), false);
    };
    let desc = val
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let auto = val
        .get("description_auto")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    (desc, auto)
}

/// Auto-generate profile description via auxiliary LLM.
pub async fn describe_profile(
    profile_name: &str,
    overwrite: bool,
    provider: Arc<dyn LLMProvider>,
    main_model: &str,
    cfg: &AppConfig,
) -> DescribeProfileOutcome {
    let root = install_root();
    let canon = normalize_profile_name(profile_name);
    if !profile_exists(&root, &canon) {
        return DescribeProfileOutcome {
            profile_name: canon,
            ok: false,
            reason: "profile not found".into(),
            description: None,
        };
    }

    let profile_dir = profile_effective_home(&root, &canon);
    let (existing_desc, existing_auto) = read_profile_meta(&profile_dir);
    if !existing_desc.is_empty() && !existing_auto && !overwrite {
        return DescribeProfileOutcome {
            profile_name: canon,
            ok: false,
            reason: "profile already has a user-authored description (use overwrite=true)"
                .into(),
            description: None,
        };
    }

    let skill_names = collect_skill_names(&profile_dir);
    let skill_list = if skill_names.is_empty() {
        "  (no skills installed)".to_string()
    } else {
        skill_names
            .iter()
            .map(|n| format!("  - {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let model = load_config_for_profile(&root, &canon)
        .map(|c| c.model.default_model)
        .unwrap_or_default();

    let describer_cfg = &cfg.auxiliary.profile_describer;
    let (side_provider, _side_model) = resolve_side_task_provider_and_model(
        describer_cfg.model.as_deref(),
        cfg.auxiliary.model.as_deref(),
        provider,
        main_model,
        "profile describer",
    );

    let user = format!(
        "Profile name: {canon}\nDefault model: {model}\nInstalled skill count: {}\nNotable skills:\n{skill_list}",
        skill_names.len()
    );
    let messages = vec![
        edgequake_llm::ChatMessage::system(SYSTEM_PROMPT),
        edgequake_llm::ChatMessage::user(&user),
    ];
    let options = edgequake_llm::CompletionOptions {
        max_tokens: Some(describer_cfg.max_tokens as usize),
        temperature: Some(0.3),
        ..Default::default()
    };

    let raw = match side_provider
        .chat_with_tools(&messages, &[], None, Some(&options))
        .await
    {
        Ok(r) => r.content,
        Err(e) => {
            return DescribeProfileOutcome {
                profile_name: canon,
                ok: false,
                reason: format!("LLM error: {e}"),
                description: None,
            };
        }
    };

    let description = if let Some(val) = extract_json_object(&raw) {
        match serde_json::from_value::<LlmResponse>(val) {
            Ok(parsed) => parsed
                .description
                .filter(|d| !d.trim().is_empty())
                .map(|d| d.trim().chars().take(280).collect::<String>()),
            Err(_) => None,
        }
    } else {
        let text = raw.trim().lines().next().unwrap_or("").trim();
        if text.is_empty() {
            None
        } else {
            Some(text.chars().take(280).collect())
        }
    };

    let Some(description) = description else {
        return DescribeProfileOutcome {
            profile_name: canon,
            ok: false,
            reason: "LLM returned empty description".into(),
            description: None,
        };
    };

    if write_profile_meta(&root, &canon, &description, true).is_err() {
        return DescribeProfileOutcome {
            profile_name: canon,
            ok: false,
            reason: "failed to write profile.yaml".into(),
            description: None,
        };
    }

    DescribeProfileOutcome {
        profile_name: canon,
        ok: true,
        reason: "described".into(),
        description: Some(description),
    }
}

pub fn describe_outcome_json(outcome: &DescribeProfileOutcome) -> Value {
    serde_json::json!({
        "ok": outcome.ok,
        "profile": outcome.profile_name,
        "reason": outcome.reason,
        "description": outcome.description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn collect_skills_from_tree() {
        let dir = TempDir::new().expect("tmpdir");
        let skills = dir.path().join("skills/devops/deploy/SKILL.md");
        std::fs::create_dir_all(skills.parent().unwrap()).expect("mkdir");
        std::fs::write(&skills, "# deploy").expect("write");
        let names = collect_skill_names(dir.path());
        assert!(names.iter().any(|n| n.contains("deploy")));
    }
}
