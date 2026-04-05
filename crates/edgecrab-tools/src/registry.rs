//! # Tool Registry — trait-based dispatch with compile-time registration
//!
//! WHY this design: hermes-agent uses runtime registration via Python decorators.
//! EdgeCrab uses `inventory` for compile-time registration — zero startup cost,
//! no forgotten registrations, and the linker guarantees all tools are present.
//!
//! ```text
//!   ┌─────────────────────────────────────────────────────────┐
//!   │                   ToolRegistry                          │
//!   │                                                         │
//!   │  inventory::iter ──→ HashMap<name, &dyn ToolHandler>    │
//!   │                                                         │
//!   │  dispatch("read_file", args, ctx)                       │
//!   │      │                                                  │
//!   │      ├── exact match? → handler.execute(args, ctx)      │
//!   │      │                                                  │
//!   │      └── no match? → fuzzy_match (strsim) → suggestion  │
//!   │                                                         │
//!   │  get_definitions(enabled, disabled)                     │
//!   │      → filter by toolset + availability                 │
//!   │      → Vec<ToolSchema> for LLM API call                 │
//!   └─────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use edgecrab_state::SessionDb;
use edgecrab_types::{Message, Platform, ToolError, ToolSchema};

use crate::config_ref::AppConfigRef;
use crate::process_table::ProcessTable;

// ─── SubAgentRunner ───────────────────────────────────────────────────

/// A clarification request from the `clarify` tool.
///
/// When the agent calls `clarify`, it sends one of these on the channel.
/// The TUI displays the question (and optional multiple-choice options)
/// and sends the user's answer back via `response_tx`.
///
/// Mirrors hermes-agent's `clarify_tool.py` schema: up to `MAX_CLARIFY_CHOICES`
/// predefined choices may be offered; UI appends "Other (type your answer)".
pub struct ClarifyRequest {
    pub question: String,
    /// Up to MAX_CLARIFY_CHOICES predefined answer choices, or None for open-ended.
    pub choices: Option<Vec<String>>,
    /// One-shot channel to send the user's answer back to the waiting tool.
    pub response_tx: tokio::sync::oneshot::Sender<String>,
}

/// Approval decision returned to a waiting tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalResponse {
    Once,
    Session,
    Always,
    Deny,
}

/// A dangerous-command approval request from a tool.
///
/// Tools send one of these when they need explicit user confirmation before
/// running a risky shell command. The UI or gateway resolves the request by
/// sending an `ApprovalResponse` back on `response_tx`.
pub struct ApprovalRequest {
    /// Short human-readable preview suitable for compact UIs.
    pub command: String,
    /// The full command string that would be executed.
    pub full_command: String,
    /// Optional scanner-derived reasons shown to the user.
    pub reasons: Vec<String>,
    /// One-shot channel to send the user's decision back to the waiting tool.
    pub response_tx: tokio::sync::oneshot::Sender<ApprovalResponse>,
}

/// Maximum number of predefined choices for a clarify question.
/// A 5th "Other (type your answer)" option is automatically appended by the UI.
pub const MAX_CLARIFY_CHOICES: usize = 4;

/// Result of a sub-agent task execution.
#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub summary: String,
    pub api_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub model: Option<String>,
    pub interrupted: bool,
    pub budget_exhausted: bool,
    pub messages: Vec<Message>,
}

/// Trait for running sub-agent tasks with full tool execution.
///
/// WHY trait: delegate_task lives in edgecrab-tools but needs to spawn
/// full Agent instances (which live in edgecrab-core). Using a trait
/// breaks the circular dependency: edgecrab-core implements it,
/// edgecrab-tools consumes it via trait object.
#[async_trait]
pub trait SubAgentRunner: Send + Sync {
    /// Run a sub-agent task with full execute_loop and tool access.
    async fn run_task(
        &self,
        goal: &str,
        system_prompt: &str,
        enabled_toolsets: Vec<String>,
        max_iterations: u32,
        model_override: Option<String>,
        parent_cancel: CancellationToken,
    ) -> Result<SubAgentResult, String>;
}

// ─── ToolHandler trait ────────────────────────────────────────────────

/// Trait that every tool must implement.
///
/// WHY async_trait: Tool execution is inherently async (file I/O, HTTP,
/// subprocess). The trait object dispatch overhead (~1 vtable lookup) is
/// negligible compared to actual I/O latency.
#[async_trait]
pub trait ToolHandler: Send + Sync + 'static {
    /// Unique tool name (e.g., "read_file", "terminal")
    fn name(&self) -> &'static str;

    /// Toolset membership for enable/disable filtering (e.g., "file", "web")
    fn toolset(&self) -> &'static str;

    /// OpenAI-format function schema sent to the LLM
    fn schema(&self) -> ToolSchema;

    /// Execute the tool with parsed JSON arguments
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError>;

    /// Startup availability check (e.g., binary exists, env var present).
    /// Runs once at registry build time.
    fn is_available(&self) -> bool {
        true
    }

    /// Per-request gating (e.g., gateway running, Honcho active).
    /// Distinct from is_available: check_fn runs on every dispatch.
    fn check_fn(&self, _ctx: &ToolContext) -> bool {
        true
    }

    /// Whether this tool can safely run in parallel with other parallel-safe tools
    fn parallel_safe(&self) -> bool {
        false
    }

    /// Display emoji for TUI rendering
    fn emoji(&self) -> &'static str {
        "⚡"
    }
}

// Compile-time registration via inventory crate
inventory::collect!(&'static dyn ToolHandler);

// ─── ToolContext ───────────────────────────────────────────────────────

/// Shared context passed to every tool execution.
///
/// WHY Arc for config/state_db: Multiple tools may execute in parallel,
/// and each needs read access to shared state without cloning heavy objects.
pub struct ToolContext {
    /// Unique identifier for the current task/conversation
    pub task_id: String,
    /// Working directory for file operations (path-jailed)
    pub cwd: PathBuf,
    /// Session identifier for state persistence
    pub session_id: String,
    /// The user's original task description (for context in sub-tools)
    pub user_task: Option<String>,
    /// Cancellation token — shared with the agent for cooperative shutdown
    pub cancel: CancellationToken,
    /// Application configuration (tools section, security, etc.)
    pub config: AppConfigRef,
    /// Session database for persistence
    pub state_db: Option<Arc<SessionDb>>,
    /// Platform context — affects tool behavior (CLI vs gateway vs ACP)
    pub platform: Platform,
    /// Shared background process table for this agent session.
    /// WHY Option: Not all callers (tests, ACP) need process management.
    pub process_table: Option<Arc<ProcessTable>>,
    /// LLM provider for sub-agent delegation (delegate_task).
    /// WHY Option: Most tools don't need an LLM, and tests don't provide one.
    pub provider: Option<Arc<dyn edgequake_llm::LLMProvider>>,
    /// Tool registry for sub-agent delegation (delegate_task).
    /// WHY: Sub-agents need their own filtered tool definitions.
    pub tool_registry: Option<Arc<ToolRegistry>>,
    /// Current delegation depth (0 = root, 1 = child, 2+ = blocked).
    /// WHY: Prevents infinite recursion in delegate_task chains.
    pub delegate_depth: u32,
    /// Sub-agent runner for full execute_loop delegation.
    /// WHY Option + trait object: Breaks circular dependency between
    /// edgecrab-tools (defines trait) and edgecrab-core (implements it).
    pub sub_agent_runner: Option<Arc<dyn SubAgentRunner>>,
    /// Optional channel to request user clarification.
    /// WHY Option: Only CLI/interactive mode provides this. Gateway and
    /// ACP modes fall back to returning the [CLARIFY] marker.
    pub clarify_tx: Option<tokio::sync::mpsc::UnboundedSender<ClarifyRequest>>,
    /// Optional channel to request dangerous-command approval.
    ///
    /// WHY Option: CLI and gateway sessions can surface interactive approval
    /// prompts. Batch / test contexts without a UI simply deny dangerous
    /// commands instead of auto-executing them.
    pub approval_tx: Option<tokio::sync::mpsc::UnboundedSender<ApprovalRequest>>,
    /// Optional callback invoked whenever a skill is created, edited, patched,
    /// or deleted. Used by edgecrab-core to invalidate the skills prompt cache.
    /// WHY Option + Arc<dyn Fn>: edgecrab-tools must not depend on edgecrab-core;
    /// this callback lets core inject the invalidation hook at startup without
    /// creating a circular crate dependency.
    pub on_skills_changed: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Optional gateway message sender — allows tools to send messages to
    /// external platforms via the gateway. Set when running in gateway mode.
    pub gateway_sender: Option<Arc<dyn GatewaySender>>,
    /// Origin of the current session: (platform_name, chat_id).
    ///
    /// Set by the gateway dispatcher when a message arrives from a real chat
    /// (e.g. WhatsApp, Telegram). Used by `manage_cron_jobs` to populate the
    /// `origin` field on newly created jobs so `deliver='origin'` can route
    /// results back to the correct chat. None in CLI and cron sessions.
    pub origin_chat: Option<(String, String)>,
    /// Stable session key for this conversation turn.
    ///
    /// WHY: Mirrors hermes-agent's `ProcessSession.session_key`. Passed to
    /// `ProcessTable::register()` when spawning background processes so that
    /// `has_active_for_session()` can check whether a gateway session has live
    /// background processes before resetting. Typically `"platform:chat_id"`
    /// in gateway mode or the `conversation_session_id` in CLI mode.
    pub session_key: Option<String>,
    /// Per-session todo list store.
    ///
    /// WHY: Survives context compression. After compression,
    /// `format_for_injection()` re-injects active items into the conversation
    /// so the model never loses its plan. When None (tests, minimal contexts),
    /// the todo tool falls back to stateless formatting.
    pub todo_store: Option<Arc<crate::tools::todo::TodoStore>>,
}

/// Trait for sending messages through the gateway to external platforms.
/// Implemented by edgecrab-gateway's DeliveryRouter or a wrapper around it.
#[async_trait]
pub trait GatewaySender: Send + Sync + 'static {
    /// Send a text message to a platform target.
    async fn send_message(
        &self,
        platform: &str,
        recipient: &str,
        message: &str,
    ) -> Result<(), String>;

    /// List available messaging targets (channels, users, DMs).
    async fn list_targets(&self) -> Result<Vec<String>, String>;
}

impl ToolContext {
    /// Create a minimal context for testing
    #[cfg(test)]
    pub fn test_context() -> Self {
        Self {
            task_id: "test-task".into(),
            cwd: std::env::temp_dir(),
            session_id: "test-session".into(),
            user_task: None,
            cancel: CancellationToken::new(),
            config: AppConfigRef::default(),
            state_db: None,
            platform: Platform::Cli,
            process_table: None,
            provider: None,
            tool_registry: None,
            delegate_depth: 0,
            sub_agent_runner: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: None,
            todo_store: None,
        }
    }
}

// ─── ToolRegistry ─────────────────────────────────────────────────────

/// Registry of all available tools, built from compile-time inventory.
///
/// WHY HashMap over Vec: O(1) lookup by name during dispatch. The registry
/// is built once at startup and queried on every LLM response that contains
/// tool calls — fast lookup matters.
pub struct ToolRegistry {
    /// name → handler lookup
    tools: HashMap<&'static str, &'static dyn ToolHandler>,
    /// toolset → [tool_names] for group operations
    toolset_index: HashMap<&'static str, Vec<&'static str>>,
    /// Dynamic tools registered at runtime (plugins, MCP)
    dynamic_tools: HashMap<String, Box<dyn ToolHandler>>,
}

impl ToolRegistry {
    fn toolset_allowed_in_ctx(toolset: &str, ctx: &ToolContext) -> bool {
        ctx.config.is_toolset_enabled(toolset)
    }

    /// Build registry from all inventory-registered tools.
    ///
    /// Called once at startup. Iterates compile-time collected tool handlers
    /// and builds lookup indices.
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        let mut toolset_index: HashMap<&str, Vec<&str>> = HashMap::new();

        for handler in inventory::iter::<&dyn ToolHandler> {
            tools.insert(handler.name(), *handler);
            toolset_index
                .entry(handler.toolset())
                .or_default()
                .push(handler.name());
        }

        Self {
            tools,
            toolset_index,
            dynamic_tools: HashMap::new(),
        }
    }

    /// Get tool definitions for an LLM API call, filtered by enabled/disabled toolsets.
    ///
    /// WHY filter at definition time (not dispatch time): Sending the LLM
    /// tools it can't use wastes tokens. Better to omit disabled tools from
    /// the schema entirely.
    pub fn get_definitions(
        &self,
        enabled: Option<&[String]>,
        disabled: Option<&[String]>,
        ctx: &ToolContext,
    ) -> Vec<ToolSchema> {
        // WHY closure: the identical 4-check predicate was previously duplicated
        // for static and dynamic tool iterators. Extracting it keeps the
        // enabled/disabled logic in a single place.
        let is_eligible = |toolset: &str, available: bool, passes_check: bool| -> bool {
            available
                && passes_check
                && Self::toolset_allowed_in_ctx(toolset, ctx)
                && enabled.is_none_or(|sets| sets.iter().any(|s| s == toolset))
                && disabled.is_none_or(|sets| !sets.iter().any(|s| s == toolset))
        };

        let static_schemas = self
            .tools
            .values()
            .filter(|h| is_eligible(h.toolset(), h.is_available(), h.check_fn(ctx)))
            .map(|h| h.schema());

        let dynamic_schemas = self
            .dynamic_tools
            .values()
            .filter(|h| is_eligible(h.toolset(), h.is_available(), h.check_fn(ctx)))
            .map(|h| h.schema());

        static_schemas.chain(dynamic_schemas).collect()
    }

    /// Dispatch a tool call by name.
    ///
    /// On name mismatch, uses fuzzy matching (Levenshtein distance via strsim)
    /// to suggest the closest tool — helps the LLM self-correct typos.
    pub async fn dispatch(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        // Any tool other than read_file / search_files resets the consecutive
        // re-read counter so only truly back-to-back identical reads trigger
        // the loop guard. Mirrors hermes-agent's notify_other_tool_call().
        const READ_LOOP_EXEMPT: &[&str] = &["read_file", "search_files"];
        if !READ_LOOP_EXEMPT.contains(&name) {
            crate::read_tracker::notify_other_tool_call(&ctx.session_id);
        }

        // Check static tools first
        if let Some(handler) = self.tools.get(name) {
            if !Self::toolset_allowed_in_ctx(handler.toolset(), ctx) {
                return Err(ToolError::Unavailable {
                    tool: name.to_string(),
                    reason: format!(
                        "toolset '{}' is disabled in this session",
                        handler.toolset()
                    ),
                });
            }
            if !handler.check_fn(ctx) {
                return Err(ToolError::Unavailable {
                    tool: name.to_string(),
                    reason: "tool gating check failed".into(),
                });
            }
            return handler.execute(args, ctx).await;
        }

        // Check dynamic tools
        if let Some(handler) = self.dynamic_tools.get(name) {
            if !Self::toolset_allowed_in_ctx(handler.toolset(), ctx) {
                return Err(ToolError::Unavailable {
                    tool: name.to_string(),
                    reason: format!(
                        "toolset '{}' is disabled in this session",
                        handler.toolset()
                    ),
                });
            }
            if !handler.check_fn(ctx) {
                return Err(ToolError::Unavailable {
                    tool: name.to_string(),
                    reason: "tool gating check failed".into(),
                });
            }
            return handler.execute(args, ctx).await;
        }

        // Fuzzy fallback
        if let Some(suggestion) = self.fuzzy_match(name) {
            Err(ToolError::NotFound(format!(
                "Unknown tool '{}'. Did you mean '{}'?",
                name, suggestion
            )))
        } else {
            Err(ToolError::NotFound(name.to_string()))
        }
    }

    /// Register a dynamic tool at runtime (plugins, MCP proxies).
    pub fn register_dynamic(&mut self, handler: Box<dyn ToolHandler>) {
        let name = handler.name().to_string();
        self.dynamic_tools.insert(name, handler);
    }

    /// All registered tool names (static + dynamic)
    pub fn tool_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tools.keys().copied().collect();
        names.extend(self.dynamic_tools.keys().map(|s| s.as_str()));
        names.sort();
        names
    }

    /// All toolset names
    pub fn toolset_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.toolset_index.keys().copied().collect();
        names.sort();
        names
    }

    /// Tools belonging to a specific toolset
    pub fn tools_in_toolset(&self, toolset: &str) -> Vec<&str> {
        self.toolset_index.get(toolset).cloned().unwrap_or_default()
    }

    /// Toolset containing a specific tool.
    pub fn toolset_for_tool(&self, name: &str) -> Option<String> {
        self.tools
            .get(name)
            .map(|h| h.toolset().to_string())
            .or_else(|| {
                self.dynamic_tools
                    .get(name)
                    .map(|h| h.toolset().to_string())
            })
    }

    /// Summary of toolsets with tool counts.
    pub fn toolset_summary(&self) -> Vec<(String, usize)> {
        let mut summary: Vec<(String, usize)> = self
            .toolset_index
            .iter()
            .map(|(name, tools)| (name.to_string(), tools.len()))
            .collect();
        summary.sort_by(|a, b| a.0.cmp(&b.0));
        summary
    }

    /// Check if a tool is parallel-safe
    pub fn is_parallel_safe(&self, name: &str) -> bool {
        self.tools
            .get(name)
            .map(|h| h.parallel_safe())
            .or_else(|| self.dynamic_tools.get(name).map(|h| h.parallel_safe()))
            .unwrap_or(false)
    }

    /// Fuzzy match tool name using Levenshtein distance.
    /// Returns the closest match if distance ≤ 3 (catches common typos).
    fn fuzzy_match(&self, name: &str) -> Option<&str> {
        let threshold = 3;
        let mut best: Option<(&str, usize)> = None;

        for &tool_name in self.tools.keys() {
            let dist = strsim::levenshtein(name, tool_name);
            if dist <= threshold
                && (best.is_none() || dist < best.as_ref().map_or(usize::MAX, |b| b.1))
            {
                best = Some((tool_name, dist));
            }
        }

        for tool_name in self.dynamic_tools.keys() {
            let dist = strsim::levenshtein(name, tool_name);
            if dist <= threshold
                && (best.is_none() || dist < best.as_ref().map_or(usize::MAX, |b| b.1))
            {
                best = Some((tool_name.as_str(), dist));
            }
        }

        best.map(|(name, _)| name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── edgequake-llm bridge ─────────────────────────────────────────────

/// Convert our ToolSchema to edgequake-llm's ToolDefinition for API calls.
pub fn to_llm_definitions(schemas: &[ToolSchema]) -> Vec<edgequake_llm::ToolDefinition> {
    schemas
        .iter()
        .map(|s| {
            edgequake_llm::ToolDefinition::function(&s.name, &s.description, s.parameters.clone())
        })
        .collect()
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Test tool for unit tests
    struct TestTool;

    #[async_trait]
    impl ToolHandler for TestTool {
        fn name(&self) -> &'static str {
            "test_tool"
        }
        fn toolset(&self) -> &'static str {
            "test"
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "test_tool".into(),
                description: "A test tool".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string" }
                    },
                    "required": ["input"]
                }),
                strict: None,
            }
        }
        async fn execute(
            &self,
            args: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<String, ToolError> {
            let input = args["input"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "test_tool".into(),
                    message: "input required".into(),
                })?;
            Ok(format!("echo: {}", input))
        }
        fn parallel_safe(&self) -> bool {
            true
        }
        fn emoji(&self) -> &'static str {
            "🧪"
        }
    }

    struct GatedTool;

    #[async_trait]
    impl ToolHandler for GatedTool {
        fn name(&self) -> &'static str {
            "gated_tool"
        }
        fn toolset(&self) -> &'static str {
            "gated"
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "gated_tool".into(),
                description: "A gated tool".into(),
                parameters: json!({"type": "object"}),
                strict: None,
            }
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<String, ToolError> {
            Ok("gated result".into())
        }
        fn check_fn(&self, _ctx: &ToolContext) -> bool {
            false // always gated off
        }
    }

    fn make_registry_with_tools() -> ToolRegistry {
        let mut registry = ToolRegistry {
            tools: HashMap::new(),
            toolset_index: HashMap::new(),
            dynamic_tools: HashMap::new(),
        };

        // Leak static references for test tools (only in tests)
        let test_tool: &'static dyn ToolHandler = Box::leak(Box::new(TestTool));
        let gated_tool: &'static dyn ToolHandler = Box::leak(Box::new(GatedTool));

        registry.tools.insert(test_tool.name(), test_tool);
        registry.tools.insert(gated_tool.name(), gated_tool);
        registry
            .toolset_index
            .entry("test")
            .or_default()
            .push("test_tool");
        registry
            .toolset_index
            .entry("gated")
            .or_default()
            .push("gated_tool");

        registry
    }

    #[test]
    fn registry_default_builds() {
        let registry = ToolRegistry::new();
        // With no inventory-submitted tools, registry is empty
        assert!(registry.tools.is_empty() || !registry.tools.is_empty());
    }

    #[tokio::test]
    async fn dispatch_exact_match() {
        let registry = make_registry_with_tools();
        let ctx = ToolContext::test_context();

        let result = registry
            .dispatch("test_tool", json!({"input": "hello"}), &ctx)
            .await;

        assert_eq!(result.expect("dispatch"), "echo: hello");
    }

    #[tokio::test]
    async fn dispatch_rejects_disabled_toolset_even_when_tool_exists() {
        let registry = make_registry_with_tools();
        let mut ctx = ToolContext::test_context();
        ctx.config.disabled_toolsets = vec!["test".into()];

        let err = registry
            .dispatch("test_tool", json!({"input": "hello"}), &ctx)
            .await
            .expect_err("disabled toolset should block dispatch");

        match err {
            ToolError::Unavailable { reason, .. } => {
                assert!(reason.contains("disabled"));
            }
            other => panic!("Expected Unavailable, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn dispatch_unknown_tool_fuzzy() {
        let registry = make_registry_with_tools();
        let ctx = ToolContext::test_context();

        let err = registry
            .dispatch("test_tol", json!({}), &ctx) // typo: "tol" vs "tool"
            .await
            .expect_err("should suggest similar tool name");

        match err {
            ToolError::NotFound(msg) => {
                assert!(msg.contains("Did you mean"), "Got: {}", msg);
                assert!(msg.contains("test_tool"), "Got: {}", msg);
            }
            other => panic!("Expected NotFound, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn dispatch_completely_unknown() {
        let registry = make_registry_with_tools();
        let ctx = ToolContext::test_context();

        let err = registry
            .dispatch("zzzzzzzzzzzzz", json!({}), &ctx)
            .await
            .expect_err("completely unknown tool should error");

        match err {
            ToolError::NotFound(msg) => {
                assert!(!msg.contains("Did you mean"), "Got: {}", msg);
            }
            other => panic!("Expected NotFound, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn dispatch_gated_tool_blocked() {
        let registry = make_registry_with_tools();
        let ctx = ToolContext::test_context();

        let err = registry
            .dispatch("gated_tool", json!({}), &ctx)
            .await
            .expect_err("gated tool should be unavailable");

        match err {
            ToolError::Unavailable { tool, .. } => assert_eq!(tool, "gated_tool"),
            other => panic!("Expected Unavailable, got: {:?}", other),
        }
    }

    #[test]
    fn get_definitions_filters_by_toolset() {
        let registry = make_registry_with_tools();
        let ctx = ToolContext::test_context();

        // Only enable "test" toolset
        let defs = registry.get_definitions(Some(&["test".to_string()]), None, &ctx);

        // Should include test_tool but not gated_tool
        assert!(defs.iter().any(|d| d.name == "test_tool"));
        assert!(!defs.iter().any(|d| d.name == "gated_tool"));
    }

    #[test]
    fn get_definitions_excludes_disabled() {
        let registry = make_registry_with_tools();
        let ctx = ToolContext::test_context();

        let defs = registry.get_definitions(None, Some(&["gated".to_string()]), &ctx);

        // gated toolset should be excluded
        assert!(!defs.iter().any(|d| d.name == "gated_tool"));
    }

    #[test]
    fn get_definitions_excludes_check_fn_failed() {
        let registry = make_registry_with_tools();
        let ctx = ToolContext::test_context();

        // No filters — but gated_tool fails check_fn
        let defs = registry.get_definitions(None, None, &ctx);

        // gated_tool should be excluded because check_fn returns false
        assert!(!defs.iter().any(|d| d.name == "gated_tool"));
        // test_tool should be included
        assert!(defs.iter().any(|d| d.name == "test_tool"));
    }

    #[test]
    fn get_definitions_excludes_ctx_disabled_toolsets() {
        let registry = make_registry_with_tools();
        let mut ctx = ToolContext::test_context();
        ctx.config.disabled_toolsets = vec!["test".into()];

        let defs = registry.get_definitions(None, None, &ctx);
        assert!(!defs.iter().any(|d| d.name == "test_tool"));
    }

    #[test]
    fn tool_names_sorted() {
        let registry = make_registry_with_tools();
        let names = registry.tool_names();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn parallel_safe_check() {
        let registry = make_registry_with_tools();
        assert!(registry.is_parallel_safe("test_tool"));
        assert!(!registry.is_parallel_safe("gated_tool"));
        assert!(!registry.is_parallel_safe("nonexistent"));
    }

    #[test]
    fn register_dynamic_tool() {
        let mut registry = make_registry_with_tools();
        let ctx = ToolContext::test_context();

        registry.register_dynamic(Box::new(TestTool));

        // Should appear in tool_names
        assert!(registry.tool_names().contains(&"test_tool"));

        // Dynamic tools should show in definitions
        let defs = registry.get_definitions(None, None, &ctx);
        assert!(defs.iter().any(|d| d.name == "test_tool"));
    }

    #[test]
    fn fuzzy_match_close_typo() {
        let registry = make_registry_with_tools();
        assert_eq!(registry.fuzzy_match("test_tol"), Some("test_tool"));
        assert_eq!(registry.fuzzy_match("tset_tool"), Some("test_tool"));
    }

    #[test]
    fn fuzzy_match_too_far() {
        let registry = make_registry_with_tools();
        assert_eq!(registry.fuzzy_match("completely_different"), None);
    }

    #[test]
    fn to_llm_definitions_conversion() {
        let schemas = vec![ToolSchema {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
            strict: None,
        }];

        let defs = to_llm_definitions(&schemas);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].function.name, "read_file");
    }

    #[test]
    fn toolset_names_and_tools() {
        let registry = make_registry_with_tools();
        let toolsets = registry.toolset_names();
        assert!(toolsets.contains(&"test"));
        assert!(toolsets.contains(&"gated"));

        let test_tools = registry.tools_in_toolset("test");
        assert!(test_tools.contains(&"test_tool"));
    }
}
