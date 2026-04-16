//! # delegate_task — Sub-agent task delegation
//!
//! WHY delegation: Complex tasks benefit from decomposition. The main agent
//! can spawn a sub-agent with a focused task, separate tool subset, and
//! independent session, then merge the result back.
//!
//! ```text
//!   delegate_task(goal, context, toolsets, tasks)
//!       │
//!       ├── depth check (MAX_DEPTH=2)
//!       ├── build child Agent via AgentBuilder
//!       │   ├── filtered toolsets (strip blocked: delegation, clarify, memory)
//!       │   ├── focused system prompt
//!       │   └── inherited provider + tool_registry
//!       ├── child.run_conversation(goal) — full execute_loop with tools
//!       └── return structured result to parent
//! ```
//!
//! Supports single-task and batch (parallel) modes.
//! Batch mode runs up to MAX_CONCURRENT_CHILDREN tasks via tokio::spawn.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{Content, Message, Role, ToolError, ToolSchema};

use crate::registry::{DelegationEvent, SubAgentRunRequest, ToolContext, ToolHandler};

/// Maximum delegation depth: parent(0) → child(1) → grandchild rejected(2)
const MAX_DEPTH: u32 = 2;

/// Maximum concurrent child agents in batch mode
const MAX_CONCURRENT_CHILDREN: usize = 3;

/// Default iteration budget per child agent
const DEFAULT_CHILD_MAX_ITERATIONS: u32 = 50;

/// Toolsets that children must never receive
const BLOCKED_TOOLSETS: &[&str] = &[
    "delegation",     // no recursive delegation
    "clarify",        // no user interaction
    "memory",         // no writes to shared MEMORY.md
    "code_execution", // children should reason instead of scripting
    "messaging",      // no cross-platform side effects
];

pub struct DelegateTaskToolReal;

#[derive(Deserialize)]
struct DelegateTaskArgs {
    /// Single-task mode: clear task description
    #[serde(default)]
    goal: Option<String>,
    /// Legacy alias for goal
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    context: Option<String>,
    /// Toolsets to enable for the child (filtered against blocked list)
    #[serde(default)]
    toolsets: Option<Vec<String>>,
    /// Batch mode: array of {goal, context, toolsets} objects (max 3)
    #[serde(default)]
    tasks: Option<Vec<BatchTask>>,
    /// Iteration budget for each child agent
    #[serde(default)]
    max_iterations: Option<u32>,
}

#[derive(Deserialize, Clone)]
struct BatchTask {
    goal: String,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    toolsets: Option<Vec<String>>,
}

/// Build a focused system prompt for a child agent.
fn build_child_system_prompt(goal: &str, context: Option<&str>) -> String {
    let mut parts = vec![
        "You are a focused subagent working on a specific delegated task.".to_string(),
        String::new(),
        format!("YOUR TASK:\n{}", goal),
    ];
    if let Some(ctx) = context {
        if !ctx.trim().is_empty() {
            parts.push(format!("\nCONTEXT:\n{}", ctx));
        }
    }
    parts.push(
        "\nComplete this task using the tools available to you. \
         When finished, provide a clear, concise summary of:\n\
         - What you did\n\
         - What you found or accomplished\n\
         - Any files you created or modified\n\
         - Any issues encountered\n\n\
         Be thorough but concise — your response is returned to the \
         parent agent as a summary."
            .to_string(),
    );
    parts.join("\n")
}

/// Filter toolsets: remove blocked ones, intersect with parent's enabled set.
fn filter_child_toolsets(requested: Option<&[String]>, parent_enabled: &[String]) -> Vec<String> {
    let base = if let Some(req) = requested {
        // Intersect requested with parent's enabled (child can't gain tools parent lacks)
        if parent_enabled.is_empty() {
            req.to_vec()
        } else {
            req.iter()
                .filter(|t| parent_enabled.iter().any(|p| p == *t))
                .cloned()
                .collect()
        }
    } else if !parent_enabled.is_empty() {
        parent_enabled.to_vec()
    } else {
        // Default toolsets when nothing is configured
        vec!["terminal".into(), "file".into(), "web".into()]
    };

    // Strip blocked toolsets
    base.into_iter()
        .filter(|t| !BLOCKED_TOOLSETS.contains(&t.as_str()))
        .collect()
}

fn build_tool_trace(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut tool_trace = Vec::new();
    let mut trace_by_id: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for msg in messages {
        match msg.role {
            Role::Assistant => {
                if let Some(tool_calls) = &msg.tool_calls {
                    for tc in tool_calls {
                        let idx = tool_trace.len();
                        tool_trace.push(json!({
                            "tool": tc.function.name,
                            "args_bytes": tc.function.arguments.len(),
                        }));
                        trace_by_id.insert(tc.id.clone(), idx);
                    }
                }
            }
            Role::Tool => {
                let content = match &msg.content {
                    Some(Content::Text(text)) => text.clone(),
                    Some(Content::Parts(parts)) => parts
                        .iter()
                        .filter_map(|part| match part {
                            edgecrab_types::ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    None => String::new(),
                };
                let status = if content
                    .get(..80)
                    .unwrap_or(content.as_str())
                    .to_ascii_lowercase()
                    .contains("error")
                {
                    "error"
                } else {
                    "ok"
                };
                let result_meta = json!({
                    "result_bytes": content.len(),
                    "status": status,
                });

                if let Some(tool_call_id) = &msg.tool_call_id {
                    if let Some(idx) = trace_by_id.get(tool_call_id).copied() {
                        if let Some(entry) = tool_trace.get_mut(idx) {
                            if let Some(obj) = entry.as_object_mut() {
                                obj.extend(result_meta.as_object().cloned().unwrap_or_default());
                            }
                        }
                        continue;
                    }
                }

                if let Some(entry) = tool_trace.last_mut() {
                    if let Some(obj) = entry.as_object_mut() {
                        obj.extend(result_meta.as_object().cloned().unwrap_or_default());
                    }
                }
            }
            _ => {}
        }
    }

    tool_trace
}

struct ChildTaskRequest {
    task_index: usize,
    task_count: usize,
    goal: String,
    context: Option<String>,
    toolsets: Option<Vec<String>>,
    max_iterations: u32,
    model_override: Option<String>,
}

/// Run a single child task using the SubAgentRunner.
async fn run_child_task(parent_ctx: &ToolContext, request: ChildTaskRequest) -> serde_json::Value {
    let ChildTaskRequest {
        task_index,
        task_count,
        goal,
        context,
        toolsets,
        max_iterations,
        model_override,
    } = request;
    let start = std::time::Instant::now();

    // We need a sub_agent_runner to spawn a real sub-agent
    let Some(ref runner) = parent_ctx.sub_agent_runner else {
        return json!({
            "task_index": task_index,
            "status": "skipped",
            "summary": format!(
                "Sub-task delegated (no sub-agent runner available).\n\
                 Task: {}\nStatus: Please proceed with the task inline.",
                goal
            ),
            "duration_seconds": 0,
        });
    };

    let system_prompt = build_child_system_prompt(&goal, context.as_deref());
    if let Some(tx) = &parent_ctx.delegation_event_tx {
        let _ = tx.send(DelegationEvent::TaskStarted {
            task_index,
            task_count,
            goal: goal.clone(),
        });
    }

    // Child toolsets: use requested ones or fall back to defaults
    // Parent active toolset info is populated by edgecrab-core in ToolContext.
    let child_toolsets = filter_child_toolsets(
        toolsets.as_deref(),
        &parent_ctx.config.parent_active_toolsets,
    );

    match runner
        .run_task(SubAgentRunRequest {
            goal: goal.clone(),
            system_prompt,
            enabled_toolsets: child_toolsets,
            max_iterations,
            model_override,
            parent_cancel: parent_ctx.cancel.clone(),
            progress_tx: parent_ctx.delegation_event_tx.clone(),
            task_index,
            task_count,
        })
        .await
    {
        Ok(result) => {
            let duration = start.elapsed().as_secs_f64();
            let summary = result.summary.trim().to_string();
            let status = if result.interrupted {
                "interrupted"
            } else if !summary.is_empty() {
                "completed"
            } else {
                "failed"
            };
            let duration_ms = start.elapsed().as_millis() as u64;
            if let Some(tx) = &parent_ctx.delegation_event_tx {
                let _ = tx.send(DelegationEvent::TaskFinished {
                    task_index,
                    task_count,
                    status: status.to_string(),
                    duration_ms,
                    summary: summary.clone(),
                    api_calls: result.api_calls,
                    model: result.model.clone(),
                });
            }

            let exit_reason = if result.interrupted {
                "interrupted"
            } else if result.budget_exhausted {
                "max_iterations"
            } else if !summary.is_empty() {
                "completed"
            } else {
                "failed"
            };
            let tool_trace = build_tool_trace(&result.messages);
            json!({
                "task_index": task_index,
                "status": status,
                "exit_reason": exit_reason,
                "summary": if summary.is_empty() {
                    format!("Sub-agent completed but returned empty response.\nTask: {}", goal)
                } else {
                    summary
                },
                "api_calls": result.api_calls,
                "model": result.model,
                "duration_seconds": (duration * 100.0).round() / 100.0,
                "tokens": {
                    "input": result.input_tokens,
                    "output": result.output_tokens,
                    "cache_read": result.cache_read_tokens,
                    "cache_write": result.cache_write_tokens,
                    "reasoning": result.reasoning_tokens,
                    "prompt": result.input_tokens
                        + result.cache_read_tokens
                        + result.cache_write_tokens,
                    "total": result.input_tokens
                        + result.cache_read_tokens
                        + result.cache_write_tokens
                        + result.output_tokens
                        + result.reasoning_tokens,
                },
                "tool_trace": tool_trace,
            })
        }
        Err(e) => {
            if let Some(tx) = &parent_ctx.delegation_event_tx {
                let _ = tx.send(DelegationEvent::TaskFinished {
                    task_index,
                    task_count,
                    status: "error".to_string(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    summary: String::new(),
                    api_calls: 0,
                    model: None,
                });
            }
            json!({
                "task_index": task_index,
                "status": "error",
                "error": format!("Sub-agent execution failed: {}", e),
                "duration_seconds": start.elapsed().as_secs_f64(),
            })
        }
    }
}

#[async_trait]
impl ToolHandler for DelegateTaskToolReal {
    fn name(&self) -> &'static str {
        "delegate_task"
    }

    fn toolset(&self) -> &'static str {
        "delegation"
    }

    fn emoji(&self) -> &'static str {
        "🔀"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "delegate_task".into(),
            description: "Spawn one or more focused sub-agents to handle delegated tasks. \
                           Supports single-task (goal) or batch (tasks array, max 3 parallel). \
                           Each child gets its own execute_loop with tools and independent context. \
                           Pass all relevant paths, errors, and constraints via `context` because \
                           child agents do not inherit your full conversation. Use this for \
                           reasoning-heavy or parallel subtasks, not for single direct tool calls."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "goal": {
                        "type": "string",
                        "description": "Clear task description for single-task mode"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional additional context or constraints"
                    },
                    "toolsets": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Toolsets to enable for the sub-agent (default: terminal, file, web)"
                    },
                    "tasks": {
                        "type": "array",
                        "maxItems": MAX_CONCURRENT_CHILDREN,
                        "items": {
                            "type": "object",
                            "properties": {
                                "goal": { "type": "string" },
                                "context": { "type": "string" },
                                "toolsets": { "type": "array", "items": { "type": "string" } }
                            },
                            "required": ["goal"]
                        },
                        "description": "Batch mode: array of tasks to run in parallel (max 3)"
                    },
                    "max_iterations": {
                        "type": "integer",
                        "description": "Iteration budget per sub-agent (default: 50)"
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
        let args: DelegateTaskArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "delegate_task".into(),
                message: format!("Invalid delegate_task args: {e}"),
            })?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        if !ctx.config.delegation_enabled {
            return Err(ToolError::Other(
                "Delegation is disabled in configuration (delegation.enabled=false).".into(),
            ));
        }

        // Depth limit
        if ctx.delegate_depth >= MAX_DEPTH {
            return Err(ToolError::Other(format!(
                "Delegation depth limit reached ({}). Subagents cannot spawn further subagents.",
                MAX_DEPTH
            )));
        }

        let config_default_max_iter = if ctx.config.delegation_max_iterations > 0 {
            ctx.config.delegation_max_iterations
        } else {
            DEFAULT_CHILD_MAX_ITERATIONS
        };
        let max_iter = args.max_iterations.unwrap_or(config_default_max_iter);

        let configured_max_subagents = if ctx.config.delegation_max_subagents > 0 {
            ctx.config.delegation_max_subagents as usize
        } else {
            MAX_CONCURRENT_CHILDREN
        };
        let max_subagents = configured_max_subagents.min(MAX_CONCURRENT_CHILDREN);

        let model_override = ctx
            .config
            .delegation_model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|m| {
                if m.contains('/') {
                    m.to_string()
                } else if let Some(provider) = ctx
                    .config
                    .delegation_provider
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                {
                    format!("{provider}/{m}")
                } else {
                    m.to_string()
                }
            });

        // Normalize to task list: batch mode or single-task mode
        let task_list: Vec<BatchTask> = if let Some(tasks) = args.tasks {
            if tasks.is_empty() {
                return Err(ToolError::InvalidArgs {
                    tool: "delegate_task".into(),
                    message: "tasks array is empty".into(),
                });
            }
            let tasks: Vec<BatchTask> = tasks.into_iter().take(max_subagents).collect();
            for (idx, task) in tasks.iter().enumerate() {
                if task.goal.trim().is_empty() {
                    return Err(ToolError::InvalidArgs {
                        tool: "delegate_task".into(),
                        message: format!("Task {idx} is missing a non-empty 'goal'."),
                    });
                }
            }
            tasks
        } else {
            // Single task: use goal (or legacy "task" field)
            let goal = args.goal.or(args.task).unwrap_or_default();
            if goal.trim().is_empty() {
                return Err(ToolError::InvalidArgs {
                    tool: "delegate_task".into(),
                    message: "Provide either 'goal' (single task) or 'tasks' (batch).".into(),
                });
            }
            vec![BatchTask {
                goal,
                context: args.context.clone(),
                toolsets: args.toolsets.clone(),
            }]
        };

        let overall_start = std::time::Instant::now();
        let task_count = task_list.len();

        if task_list.len() == 1 {
            // Single task — run directly (no spawn overhead)
            let t = &task_list[0];
            let result = run_child_task(
                ctx,
                ChildTaskRequest {
                    task_index: 0,
                    task_count,
                    goal: t.goal.clone(),
                    context: t.context.clone(),
                    toolsets: t.toolsets.clone(),
                    max_iterations: max_iter,
                    model_override: model_override.clone(),
                },
            )
            .await;

            let total_duration = (overall_start.elapsed().as_secs_f64() * 100.0).round() / 100.0;

            Ok(serde_json::to_string_pretty(&json!({
                "results": [result],
                "total_duration_seconds": total_duration,
            }))
            .unwrap_or_default())
        } else {
            // Batch mode — run in parallel via tokio::spawn
            let mut handles = Vec::new();
            for (i, t) in task_list.into_iter().enumerate() {
                let runner = ctx.sub_agent_runner.clone();
                let platform = ctx.platform;
                let cancel = ctx.cancel.clone();

                // Build a minimal ctx for each child (only sub_agent_runner is needed)
                let child_ctx = ToolContext {
                    task_id: uuid::Uuid::new_v4().to_string(),
                    cwd: ctx.cwd.clone(),
                    session_id: format!("subagent-{}", i),
                    user_task: None,
                    cancel,
                    config: ctx.config.clone(),
                    state_db: ctx.state_db.clone(),
                    platform,
                    process_table: ctx.process_table.clone(),
                    provider: None,
                    tool_registry: None,
                    delegate_depth: ctx.delegate_depth + 1,
                    sub_agent_runner: runner,
                    delegation_event_tx: ctx.delegation_event_tx.clone(),
                    clarify_tx: None, // sub-agents don't propagate interactive clarify
                    approval_tx: None, // sub-agents don't propagate approvals
                    on_skills_changed: None, // sub-agents don't invalidate parent cache
                    gateway_sender: None,
                    origin_chat: None, // sub-agents don't inherit origin
                    session_key: ctx.session_key.clone(),
                    todo_store: None,
                    current_tool_call_id: None,
                    current_tool_name: None,
                    injected_messages: None,
                    tool_progress_tx: None,
                    watch_notification_tx: None,
                };

                let goal = t.goal.clone();
                let context = t.context.clone();
                let toolsets = t.toolsets.clone();
                let model_override = model_override.clone();

                handles.push(tokio::spawn(async move {
                    run_child_task(
                        &child_ctx,
                        ChildTaskRequest {
                            task_index: i,
                            task_count,
                            goal,
                            context,
                            toolsets,
                            max_iterations: max_iter,
                            model_override,
                        },
                    )
                    .await
                }));
            }

            let mut results = Vec::new();
            for handle in handles {
                match handle.await {
                    Ok(result) => results.push(result),
                    Err(e) => results.push(json!({
                        "status": "error",
                        "error": format!("Task join error: {}", e),
                    })),
                }
            }

            // Sort by task_index
            results.sort_by_key(|r| r.get("task_index").and_then(|v| v.as_u64()).unwrap_or(0));

            let total_duration = (overall_start.elapsed().as_secs_f64() * 100.0).round() / 100.0;

            Ok(serde_json::to_string_pretty(&json!({
                "results": results,
                "total_duration_seconds": total_duration,
            }))
            .unwrap_or_default())
        }
    }
}

inventory::submit!(&DelegateTaskToolReal as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use crate::registry::{SubAgentResult, SubAgentRunRequest, SubAgentRunner};

    #[derive(Default, Clone)]
    struct RecordedCall {
        enabled_toolsets: Vec<String>,
        max_iterations: u32,
        model_override: Option<String>,
    }

    struct RecordingRunner {
        calls: Arc<Mutex<Vec<RecordedCall>>>,
    }

    #[async_trait]
    impl SubAgentRunner for RecordingRunner {
        async fn run_task(&self, request: SubAgentRunRequest) -> Result<SubAgentResult, String> {
            self.calls
                .lock()
                .expect("recording mutex poisoned")
                .push(RecordedCall {
                    enabled_toolsets: request.enabled_toolsets,
                    max_iterations: request.max_iterations,
                    model_override: request.model_override,
                });
            Ok(SubAgentResult {
                summary: "ok".into(),
                api_calls: 1,
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                model: Some("mock/model".into()),
                interrupted: false,
                budget_exhausted: false,
                messages: Vec::new(),
            })
        }
    }

    #[test]
    fn tool_metadata() {
        let tool = DelegateTaskToolReal;
        assert_eq!(tool.name(), "delegate_task");
        assert_eq!(tool.toolset(), "delegation");
        assert!(tool.is_available());
    }

    #[tokio::test]
    async fn delegate_without_provider_returns_fallback() {
        let tool = DelegateTaskToolReal;
        let ctx = ToolContext::test_context(); // sub_agent_runner: None
        let result = tool
            .execute(
                json!({ "goal": "Find all TODO comments in the codebase" }),
                &ctx,
            )
            .await
            .expect("should succeed");
        assert!(result.contains("no sub-agent runner"));
        assert!(result.contains("TODO comments"));
    }

    #[tokio::test]
    async fn delegate_depth_limit_rejects() {
        let tool = DelegateTaskToolReal;
        let mut ctx = ToolContext::test_context();
        ctx.delegate_depth = MAX_DEPTH;
        let result = tool.execute(json!({ "goal": "some task" }), &ctx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("depth limit"));
    }

    #[tokio::test]
    async fn delegate_empty_goal_rejected() {
        let tool = DelegateTaskToolReal;
        let ctx = ToolContext::test_context();
        let result = tool.execute(json!({ "goal": "" }), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delegate_legacy_task_field_works() {
        let tool = DelegateTaskToolReal;
        let ctx = ToolContext::test_context();
        let result = tool
            .execute(json!({ "task": "legacy task field" }), &ctx)
            .await
            .expect("should succeed");
        assert!(result.contains("legacy task field"));
    }

    #[test]
    fn filter_blocked_toolsets() {
        let parent: Vec<String> = vec![
            "terminal".into(),
            "file".into(),
            "delegation".into(),
            "memory".into(),
            "code_execution".into(),
            "messaging".into(),
            "web".into(),
        ];
        let filtered = filter_child_toolsets(None, &parent);
        assert!(filtered.contains(&"terminal".to_string()));
        assert!(filtered.contains(&"file".to_string()));
        assert!(filtered.contains(&"web".to_string()));
        assert!(!filtered.contains(&"delegation".to_string()));
        assert!(!filtered.contains(&"memory".to_string()));
        assert!(!filtered.contains(&"code_execution".to_string()));
        assert!(!filtered.contains(&"messaging".to_string()));
    }

    #[test]
    fn child_system_prompt_includes_goal() {
        let prompt = build_child_system_prompt("Find all tests", Some("Focus on Rust files"));
        assert!(prompt.contains("Find all tests"));
        assert!(prompt.contains("Focus on Rust files"));
        assert!(prompt.contains("focused subagent"));
    }

    #[tokio::test]
    async fn delegate_respects_parent_toolsets_and_blocked_child_toolsets() {
        let tool = DelegateTaskToolReal;
        let calls = Arc::new(Mutex::new(Vec::<RecordedCall>::new()));
        let mut ctx = ToolContext::test_context();
        ctx.config.parent_active_toolsets = vec![
            "terminal".into(),
            "file".into(),
            "web".into(),
            "code_execution".into(),
            "messaging".into(),
        ];
        ctx.sub_agent_runner = Some(Arc::new(RecordingRunner {
            calls: calls.clone(),
        }));

        let _ = tool
            .execute(
                json!({
                    "goal": "run task",
                    "toolsets": ["terminal", "code_execution", "messaging", "web", "unknown"]
                }),
                &ctx,
            )
            .await
            .expect("delegate_task should succeed");

        let calls = calls.lock().expect("recording mutex poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].enabled_toolsets, vec!["terminal", "web"]);
    }

    #[tokio::test]
    async fn delegate_uses_config_defaults_for_iterations_and_model_override() {
        let tool = DelegateTaskToolReal;
        let calls = Arc::new(Mutex::new(Vec::<RecordedCall>::new()));
        let mut ctx = ToolContext::test_context();
        ctx.config.delegation_max_iterations = 17;
        ctx.config.delegation_model = Some("gpt-4.1-mini".into());
        ctx.config.delegation_provider = Some("copilot".into());
        ctx.sub_agent_runner = Some(Arc::new(RecordingRunner {
            calls: calls.clone(),
        }));

        let _ = tool
            .execute(json!({ "goal": "config defaults" }), &ctx)
            .await
            .expect("delegate_task should succeed");

        let calls = calls.lock().expect("recording mutex poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].max_iterations, 17);
        assert_eq!(
            calls[0].model_override.as_deref(),
            Some("copilot/gpt-4.1-mini")
        );
    }

    #[tokio::test]
    async fn delegate_respects_configured_max_subagents() {
        let tool = DelegateTaskToolReal;
        let calls = Arc::new(Mutex::new(Vec::<RecordedCall>::new()));
        let mut ctx = ToolContext::test_context();
        ctx.config.delegation_max_subagents = 1;
        ctx.sub_agent_runner = Some(Arc::new(RecordingRunner {
            calls: calls.clone(),
        }));

        let result = tool
            .execute(
                json!({
                    "tasks": [
                        { "goal": "task 1" },
                        { "goal": "task 2" }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("delegate_task should succeed");

        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        let results = parsed
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results array");
        assert_eq!(results.len(), 1);

        let calls = calls.lock().expect("recording mutex poisoned");
        assert_eq!(calls.len(), 1);
    }

    #[tokio::test]
    async fn delegate_rejects_when_disabled_in_config() {
        let tool = DelegateTaskToolReal;
        let mut ctx = ToolContext::test_context();
        ctx.config.delegation_enabled = false;

        let err = tool
            .execute(json!({ "goal": "should fail" }), &ctx)
            .await
            .expect_err("delegation should be disabled");

        assert!(err.to_string().contains("disabled"));
    }

    #[tokio::test]
    async fn delegate_includes_observability_fields() {
        #[derive(Clone)]
        struct ObservabilityRunner;

        #[async_trait]
        impl SubAgentRunner for ObservabilityRunner {
            async fn run_task(
                &self,
                _request: SubAgentRunRequest,
            ) -> Result<SubAgentResult, String> {
                Ok(SubAgentResult {
                    summary: "done".into(),
                    api_calls: 2,
                    input_tokens: 42,
                    output_tokens: 7,
                    cache_read_tokens: 11,
                    cache_write_tokens: 3,
                    reasoning_tokens: 5,
                    model: Some("test/model".into()),
                    interrupted: false,
                    budget_exhausted: false,
                    messages: vec![
                        Message::assistant_with_tool_calls(
                            "",
                            vec![edgecrab_types::ToolCall {
                                id: "tc_1".into(),
                                r#type: "function".into(),
                                function: edgecrab_types::FunctionCall {
                                    name: "terminal".into(),
                                    arguments: r#"{"command":"pwd"}"#.into(),
                                },
                                thought_signature: None,
                            }],
                        ),
                        Message::tool_result("tc_1", "terminal", "ok"),
                    ],
                })
            }
        }

        let tool = DelegateTaskToolReal;
        let mut ctx = ToolContext::test_context();
        ctx.sub_agent_runner = Some(Arc::new(ObservabilityRunner));

        let result = tool
            .execute(json!({ "goal": "collect metadata" }), &ctx)
            .await
            .expect("delegate_task should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        let entry = &parsed["results"][0];

        assert_eq!(entry["model"], "test/model");
        assert_eq!(entry["exit_reason"], "completed");
        assert_eq!(entry["tokens"]["input"], 42);
        assert_eq!(entry["tokens"]["output"], 7);
        assert_eq!(entry["tokens"]["cache_read"], 11);
        assert_eq!(entry["tokens"]["cache_write"], 3);
        assert_eq!(entry["tokens"]["reasoning"], 5);
        assert_eq!(entry["tokens"]["prompt"], 56);
        assert_eq!(entry["tokens"]["total"], 68);
        assert_eq!(entry["tool_trace"][0]["tool"], "terminal");
        assert_eq!(entry["tool_trace"][0]["status"], "ok");
    }

    #[tokio::test]
    async fn delegate_rejects_empty_goal_in_batch_task() {
        let tool = DelegateTaskToolReal;
        let ctx = ToolContext::test_context();
        let err = tool
            .execute(
                json!({
                    "tasks": [
                        { "goal": "valid" },
                        { "goal": "   " }
                    ]
                }),
                &ctx,
            )
            .await
            .expect_err("empty batch goal should fail");

        assert!(err.to_string().contains("Task 1"));
    }

    #[tokio::test]
    async fn delegate_emits_progress_events() {
        #[derive(Clone)]
        struct ProgressRunner;

        #[async_trait]
        impl SubAgentRunner for ProgressRunner {
            async fn run_task(
                &self,
                request: SubAgentRunRequest,
            ) -> Result<SubAgentResult, String> {
                if let Some(tx) = request.progress_tx {
                    let _ = tx.send(DelegationEvent::ToolCalled {
                        task_index: request.task_index,
                        task_count: request.task_count,
                        tool_name: "terminal".into(),
                        args_json: r#"{"command":"pwd"}"#.into(),
                    });
                }
                Ok(SubAgentResult {
                    summary: "done".into(),
                    api_calls: 1,
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    reasoning_tokens: 0,
                    model: Some("mock/model".into()),
                    interrupted: false,
                    budget_exhausted: false,
                    messages: Vec::new(),
                })
            }
        }

        let tool = DelegateTaskToolReal;
        let mut ctx = ToolContext::test_context();
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<DelegationEvent>();
        ctx.delegation_event_tx = Some(progress_tx);
        ctx.sub_agent_runner = Some(Arc::new(ProgressRunner));

        tool.execute(json!({ "goal": "inspect repo" }), &ctx)
            .await
            .expect("delegate_task should succeed");

        let first = progress_rx.recv().await.expect("start event");
        let second = progress_rx.recv().await.expect("tool event");
        let third = progress_rx.recv().await.expect("finish event");

        match first {
            DelegationEvent::TaskStarted {
                task_index,
                task_count,
                goal,
            } => {
                assert_eq!(task_index, 0);
                assert_eq!(task_count, 1);
                assert_eq!(goal, "inspect repo");
            }
            other => panic!("unexpected first event: {other:?}"),
        }

        match second {
            DelegationEvent::ToolCalled {
                task_index,
                task_count,
                tool_name,
                ..
            } => {
                assert_eq!(task_index, 0);
                assert_eq!(task_count, 1);
                assert_eq!(tool_name, "terminal");
            }
            other => panic!("unexpected second event: {other:?}"),
        }

        match third {
            DelegationEvent::TaskFinished {
                task_index,
                task_count,
                status,
                ..
            } => {
                assert_eq!(task_index, 0);
                assert_eq!(task_count, 1);
                assert_eq!(status, "completed");
            }
            other => panic!("unexpected third event: {other:?}"),
        }
    }
}
