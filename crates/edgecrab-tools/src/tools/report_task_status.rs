//! # report_task_status — structured milestone/status signaling
//!
//! WHY this tool: It gives the model a safe way to tell the harness what it
//! believes just happened (still working, blocked, or completed) without letting
//! the model terminate the run on its own.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ReportedTaskStatus, TaskStatusKind, ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

pub struct ReportTaskStatusTool;

#[derive(Debug, Deserialize)]
struct Args {
    status: TaskStatusKind,
    summary: String,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(default)]
    remaining_steps: Vec<String>,
}

#[async_trait]
impl ToolHandler for ReportTaskStatusTool {
    fn name(&self) -> &'static str {
        "report_task_status"
    }

    fn toolset(&self) -> &'static str {
        "meta"
    }

    fn emoji(&self) -> &'static str {
        "🏁"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "report_task_status".into(),
            description: "Report your current task status to the harness using a structured signal. Use this after important milestones or when blocked. Allowed statuses: in_progress, blocked, completed. Calling this tool does NOT end the run by itself; continue until the request is truly satisfied or explicitly blocked.".into(),
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["in_progress", "blocked", "completed"],
                        "description": "Your current task status."
                    },
                    "summary": {
                        "type": "string",
                        "description": "Short explanation of what was accomplished or what is blocked."
                    },
                    "evidence": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Concrete evidence such as test/build results, file changes, or tool outcomes."
                    },
                    "remaining_steps": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "What work still remains, if any."
                    }
                },
                "required": ["status", "summary", "evidence", "remaining_steps"]
            }),
            strict: Some(true),
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: self.name().into(),
            message: e.to_string(),
        })?;

        let summary = args.summary.trim();
        if summary.is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: self.name().into(),
                message: "summary must not be empty".into(),
            });
        }

        let report = ReportedTaskStatus {
            status: args.status,
            summary: summary.to_string(),
            evidence: args
                .evidence
                .into_iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect(),
            remaining_steps: args
                .remaining_steps
                .into_iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect(),
        };

        serde_json::to_string(&report).map_err(|e| ToolError::Other(e.to_string()))
    }
}

inventory::submit!(&ReportTaskStatusTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn report_task_status_serializes_completed_report() {
        let ctx = ToolContext::test_context();
        let result = ReportTaskStatusTool
            .execute(
                json!({
                    "status": "completed",
                    "summary": "Finished the implementation",
                    "evidence": ["cargo test passed"],
                    "remaining_steps": []
                }),
                &ctx,
            )
            .await
            .expect("ok");

        let parsed: ReportedTaskStatus = serde_json::from_str(&result).expect("json");
        assert_eq!(parsed.status, TaskStatusKind::Completed);
        assert_eq!(parsed.evidence, vec!["cargo test passed"]);
    }

    #[tokio::test]
    async fn report_task_status_rejects_empty_summary() {
        let ctx = ToolContext::test_context();
        let err = ReportTaskStatusTool
            .execute(
                json!({
                    "status": "blocked",
                    "summary": "   "
                }),
                &ctx,
            )
            .await
            .expect_err("empty summary should fail");

        assert!(err.to_string().contains("summary"));
    }

    #[test]
    fn report_task_status_schema_is_strict() {
        let schema = ReportTaskStatusTool.schema();
        assert_eq!(schema.strict, Some(true));
        assert_eq!(schema.parameters["type"], "object");
        assert_eq!(schema.parameters["additionalProperties"], false);
        assert_eq!(
            schema.parameters["required"],
            json!(["status", "summary", "evidence", "remaining_steps"])
        );
        assert_eq!(
            schema.parameters["properties"]["status"]["enum"],
            json!(["in_progress", "blocked", "completed"])
        );
    }
}
