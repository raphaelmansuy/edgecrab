use crate::types::SkillReadinessStatus;

pub const REMOTE_ENV_BACKENDS: &[&str] = &["docker", "singularity", "modal", "ssh", "daytona"];

pub fn resolve_skill_readiness(required_env: &[String]) -> SkillReadinessStatus {
    let missing: Vec<String> = required_env
        .iter()
        .filter(|name| std::env::var(name.as_str()).is_err())
        .cloned()
        .collect();
    if missing.is_empty() {
        return SkillReadinessStatus::Available;
    }
    let remote_backend = std::env::var("HERMES_ENVIRONMENT")
        .ok()
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| {
            REMOTE_ENV_BACKENDS
                .iter()
                .any(|candidate| candidate == value)
        })
        .or_else(|| {
            std::env::var("EDGECRAB_ENVIRONMENT")
                .ok()
                .map(|value| value.to_ascii_lowercase())
                .filter(|value| {
                    REMOTE_ENV_BACKENDS
                        .iter()
                        .any(|candidate| candidate == value)
                })
        });
    if let Some(backend) = remote_backend {
        return SkillReadinessStatus::Unsupported {
            reason: format!("missing required environment variables in remote backend {backend}"),
        };
    }
    SkillReadinessStatus::SetupNeeded { missing }
}
