//! Post-install automation blueprints → suggestions (Hermes parity, never silent cron).

use super::SkillBundle;

/// If SKILL.md declares an automation / blueprint block, return a user-facing suggestion.
/// Never schedules cron silently — only proposes next steps.
pub fn post_install_suggestion(bundle: &SkillBundle) -> Option<String> {
    let content = bundle.files.get("SKILL.md")?;
    let lower = content.to_ascii_lowercase();
    let mentions_automation = lower.contains("automation:")
        || lower.contains("blueprint:")
        || lower.contains("suggested_cron")
        || lower.contains("schedule:");
    if !mentions_automation {
        return None;
    }
    Some(format!(
        "Blueprint hint: '{}' mentions automation. Review SKILL.md and propose a cron/suggestion \
         explicitly — EdgeCrab will not enable schedules silently. Try: /cron list",
        bundle.name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn no_hint_without_automation_markers() {
        let bundle = SkillBundle {
            name: "plain".into(),
            files: HashMap::from([("SKILL.md".into(), "# Hello\nJust a skill.".into())]),
            source: "test".into(),
            identifier: "test/plain".into(),
            trust_level: "community".into(),
        };
        assert!(post_install_suggestion(&bundle).is_none());
    }

    #[test]
    fn hint_when_blueprint_mentioned() {
        let bundle = SkillBundle {
            name: "auto".into(),
            files: HashMap::from([(
                "SKILL.md".into(),
                "---\nname: auto\nautomation: daily\n---\n".into(),
            )]),
            source: "test".into(),
            identifier: "test/auto".into(),
            trust_level: "community".into(),
        };
        let hint = post_install_suggestion(&bundle).expect("hint");
        assert!(hint.contains("Blueprint hint"));
        assert!(hint.contains("/cron"));
    }
}
