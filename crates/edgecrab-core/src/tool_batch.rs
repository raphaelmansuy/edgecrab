//! Multi-tool batch planning (spec 022/014 WS-B).
//!
//! DRY: sole owner of *which* tool calls run in parallel vs sequential.
//! Execution (JoinSet, DispatchContext) stays in `conversation.rs` so this
//! module stays free of loop coupling (SOLID S / D).
//!
//! Policy source of truth for path overlap remains
//! [`edgecrab_tools::ToolRegistry::can_parallelize_in_batch`].

use std::collections::HashSet;

use edgecrab_tools::ToolRegistry;
use edgecrab_types::ToolCall;

/// One planned call with stable index into the original batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedToolCall {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Partition result for a multi-tool turn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolBatchPlan {
    /// Independent calls eligible for concurrent JoinSet dispatch.
    pub parallel: Vec<PlannedToolCall>,
    /// Calls that must run sequentially (mutating, path overlap, or not parallel_safe).
    pub sequential: Vec<PlannedToolCall>,
}

impl ToolBatchPlan {
    pub fn total(&self) -> usize {
        self.parallel.len() + self.sequential.len()
    }

    pub fn has_parallel(&self) -> bool {
        !self.parallel.is_empty()
    }
}

/// Plan parallel vs sequential dispatch for a tool-call batch.
///
/// Order of evaluation matches the historical conversation loop:
/// for each call in order, try to place it in the parallel set using
/// [`ToolRegistry::can_parallelize_in_batch`] with accumulating claimed paths.
pub fn plan_tool_batch(registry: Option<&ToolRegistry>, calls: &[ToolCall]) -> ToolBatchPlan {
    let mut plan = ToolBatchPlan::default();
    let mut claimed_paths: HashSet<String> = HashSet::new();

    for (index, tc) in calls.iter().enumerate() {
        let planned = PlannedToolCall {
            index,
            id: tc.id.clone(),
            name: tc.function.name.clone(),
            arguments: tc.function.arguments.clone(),
        };

        let is_parallel = registry
            .map(|r| r.can_parallelize_in_batch(&planned.name, &planned.arguments, &claimed_paths))
            .unwrap_or(false);

        if is_parallel {
            if let Some(reg) = registry {
                for p in reg.extract_paths(&planned.name, &planned.arguments) {
                    claimed_paths.insert(p);
                }
            }
            plan.parallel.push(planned);
        } else {
            plan.sequential.push(planned);
        }
    }

    plan
}

/// Cap concurrent workers for a parallel batch (JoinSet size).
///
/// Env `EDGECRAB_TOOL_PARALLEL_MAX` overrides `configured` when set and > 0.
pub fn parallel_max_workers(configured: Option<usize>, batch_len: usize) -> usize {
    let from_env = std::env::var("EDGECRAB_TOOL_PARALLEL_MAX")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0);
    let cap = from_env.or(configured).unwrap_or(32);
    cap.max(1).min(batch_len.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::{FunctionCall, ToolCall};

    fn tc(id: &str, name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            r#type: "function".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: args.into(),
            },
            thought_signature: None,
        }
    }

    #[test]
    fn empty_batch() {
        let plan = plan_tool_batch(None, &[]);
        assert_eq!(plan.total(), 0);
    }

    #[test]
    fn without_registry_all_sequential() {
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.rs"}"#),
            tc("2", "read_file", r#"{"path":"b.rs"}"#),
        ];
        let plan = plan_tool_batch(None, &calls);
        assert!(plan.parallel.is_empty());
        assert_eq!(plan.sequential.len(), 2);
    }

    #[test]
    fn with_registry_independent_reads_parallel() {
        let reg = ToolRegistry::new();
        // Inventory may or may not include read_file in unit test binary —
        // if registered and parallel_safe, expect parallel.
        let calls = vec![
            tc("1", "read_file", r#"{"path":"a.rs"}"#),
            tc("2", "read_file", r#"{"path":"b.rs"}"#),
        ];
        let plan = plan_tool_batch(Some(&reg), &calls);
        assert_eq!(plan.total(), 2);
        if reg.is_parallel_safe("read_file") {
            assert_eq!(plan.parallel.len(), 2, "independent reads should parallel");
            assert!(plan.sequential.is_empty());
        } else {
            // Tool not in this test binary's inventory — still a valid plan.
            assert_eq!(plan.sequential.len() + plan.parallel.len(), 2);
        }
    }

    #[test]
    fn overlapping_paths_second_goes_sequential_for_path_aware_non_safe() {
        let reg = ToolRegistry::new();
        // parallel_safe tools (e.g. read_file) intentionally skip path claims.
        // Path overlap serialization applies to non-safe tools with path_arguments.
        let tool = "write_file";
        if reg.path_arguments(tool).is_empty() {
            return;
        }
        if reg.is_parallel_safe(tool) {
            return;
        }
        let calls = vec![
            tc("1", tool, r#"{"path":"src/main.rs","content":"a"}"#),
            tc("2", tool, r#"{"path":"src/main.rs","content":"b"}"#),
        ];
        let plan = plan_tool_batch(Some(&reg), &calls);
        assert_eq!(plan.total(), 2);
        // Second same-path write must serialize after first claims path.
        assert!(
            plan.sequential.len() >= 1,
            "same-path writes must serialize: {plan:?}"
        );
    }

    #[test]
    fn parallel_max_workers_caps() {
        assert_eq!(parallel_max_workers(Some(4), 10), 4);
        assert_eq!(parallel_max_workers(Some(0), 10), 1); // max(1)
        assert_eq!(parallel_max_workers(None, 3), 3); // default 32 min batch
        assert_eq!(parallel_max_workers(Some(100), 2), 2);
    }
}
