//! Kanban orchestration settings — Hermes `/orchestration` API parity.

use serde::Deserialize;
use serde_json::{json, Value};

use edgecrab_types::AgentError;

use crate::config::AppConfig;
use crate::kanban_profiles::{
    active_profile_name, install_root, profile_exists, resolve_default_assignee,
    resolve_orchestrator_profile,
};

/// Partial update body for `PUT /api/kanban/orchestration`.
#[derive(Debug, Deserialize, Default)]
pub struct OrchestrationSettingsPatch {
    pub orchestrator_profile: Option<String>,
    pub default_assignee: Option<String>,
    pub auto_decompose: Option<bool>,
    pub auto_promote_children: Option<bool>,
    pub max_in_progress_per_profile: Option<u32>,
}

fn explicit_or_empty(opt: &Option<String>) -> String {
    opt.as_deref().map(str::trim).unwrap_or("").to_string()
}

/// `GET /api/kanban/orchestration` payload.
pub fn get_orchestration_settings(cfg: &AppConfig) -> Value {
    let root = install_root();
    let active = active_profile_name(&root);
    let explicit_orch = explicit_or_empty(&cfg.kanban.orchestrator_profile);
    let explicit_default = explicit_or_empty(&cfg.kanban.default_assignee);
    json!({
        "orchestrator_profile": explicit_orch,
        "default_assignee": explicit_default,
        "auto_decompose": cfg.kanban.auto_decompose,
        "auto_promote_children": cfg.kanban.auto_promote_children,
        "max_workers": cfg.kanban.max_workers,
        "max_in_progress_per_profile": cfg.kanban.max_in_progress_per_profile,
        "resolved_orchestrator_profile": resolve_orchestrator_profile(&cfg.kanban, &root),
        "resolved_default_assignee": resolve_default_assignee(&cfg.kanban, &root),
        "active_profile": active,
    })
}

fn validate_profile_name(install_root: &std::path::Path, name: &str) -> Result<(), AgentError> {
    if name.is_empty() {
        return Ok(());
    }
    if profile_exists(install_root, name) {
        Ok(())
    } else {
        Err(AgentError::Validation(format!(
            "profile '{name}' does not exist"
        )))
    }
}

/// Apply patch and persist to install-root `config.yaml`.
pub fn patch_orchestration_settings(
    patch: OrchestrationSettingsPatch,
) -> Result<Value, AgentError> {
    let root = install_root();
    let mut cfg = AppConfig::load()?;

    if let Some(raw) = patch.orchestrator_profile {
        let name = raw.trim().to_string();
        validate_profile_name(&root, &name)?;
        cfg.kanban.orchestrator_profile = if name.is_empty() {
            None
        } else {
            Some(name)
        };
    }

    if let Some(raw) = patch.default_assignee {
        let name = raw.trim().to_string();
        validate_profile_name(&root, &name)?;
        cfg.kanban.default_assignee = if name.is_empty() {
            None
        } else {
            Some(name)
        };
    }

    if let Some(v) = patch.auto_decompose {
        cfg.kanban.auto_decompose = v;
    }
    if let Some(v) = patch.auto_promote_children {
        cfg.kanban.auto_promote_children = v;
    }
    if let Some(v) = patch.max_in_progress_per_profile {
        cfg.kanban.max_in_progress_per_profile = if v == 0 { None } else { Some(v) };
    }

    cfg.save()?;
    Ok(get_orchestration_settings(&cfg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn get_settings_includes_resolved_profiles() {
        let dir = TempDir::new().expect("tmpdir");
        let root = dir.path();
        fs::create_dir_all(root.join("profiles/work")).expect("mkdir");
        let cfg = AppConfig {
            kanban: crate::config::KanbanConfig {
                enabled: true,
                auto_decompose: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let body = get_orchestration_settings(&cfg);
        assert_eq!(body["auto_decompose"], true);
        assert!(body["resolved_orchestrator_profile"].is_string());
    }

    #[test]
    fn patch_rejects_unknown_profile() {
        let patch = OrchestrationSettingsPatch {
            default_assignee: Some("no-such-profile".into()),
            ..Default::default()
        };
        assert!(patch_orchestration_settings(patch).is_err());
    }
}
