//! Kanban tool visibility — Hermes `HERMES_KANBAN_TASK` parity.

use crate::registry::ToolContext;

/// Env var set by the kanban dispatcher on spawned workers (Hermes: `HERMES_KANBAN_TASK`).
pub const ENV_KANBAN_TASK: &str = "EDGECRAB_KANBAN_TASK";

/// Active kanban task id from agent context or process env.
pub fn env_task_id() -> Option<String> {
    std::env::var(ENV_KANBAN_TASK)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve explicit tool arg, agent-scoped task id, or env fallback.
pub fn resolve_task_id(ctx: &ToolContext, arg: Option<&str>) -> Option<String> {
    arg.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| ctx.kanban_task_id.clone())
        .or_else(env_task_id)
}

/// Orchestrator-only tools (`kanban_list`, `kanban_unblock`) hide from workers.
pub fn orchestrator_tool_visible(ctx: &ToolContext) -> bool {
    ctx.kanban_task_id.is_none() && env_task_id().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolContext;

    #[test]
    fn resolve_prefers_explicit_task_id() {
        unsafe {
            std::env::remove_var(ENV_KANBAN_TASK);
        }
        let ctx = ToolContext::test_context();
        assert_eq!(
            resolve_task_id(&ctx, Some("kb-abc")).as_deref(),
            Some("kb-abc")
        );
    }

    #[test]
    fn resolve_uses_ctx_task_id() {
        let mut ctx = ToolContext::test_context();
        ctx.kanban_task_id = Some("kb-ctx".into());
        assert_eq!(resolve_task_id(&ctx, None).as_deref(), Some("kb-ctx"));
    }

    #[test]
    fn orchestrator_hidden_when_worker_ctx_set() {
        let mut ctx = ToolContext::test_context();
        ctx.kanban_task_id = Some("kb-1".into());
        assert!(!orchestrator_tool_visible(&ctx));
        ctx.kanban_task_id = None;
        assert!(orchestrator_tool_visible(&ctx));
    }
}
