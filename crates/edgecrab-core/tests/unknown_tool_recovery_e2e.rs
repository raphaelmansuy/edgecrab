//! E2E: unknown-tool → tool_search recovery + PartialAbort integrity (no live LLM).
//!
//! First principles (July 2026 progressive disclosure):
//! - Registry is truth; invent names get CallToolFirst(tool_search)
//! - BM25 + CORE anchors recommend real tools (not lexicographic browser_* slice)
//! - Tool-call closure + typed InvalidToolBudget when search is ignored
//! - Operator notice is not double-wrapped

use std::sync::Arc;

use edgecrab_core::turn_completion::{count_unanswered_tool_calls, format_operator_notice};
use edgecrab_core::{AgentBuilder, ConversationResult};
use edgecrab_tools::recovery_catalog;
use edgecrab_tools::{
    ToolRegistry, UNKNOWN_TOOL_SAMPLE_LIMIT, unknown_tool_error_response,
    unknown_tool_recovery_sample, unknown_tool_search_query,
};
use edgecrab_types::{CompletionDecision, ExitReason, RecoveryAction};
use edgequake_llm::providers::MockAgentProvider;
use edgequake_llm::{FunctionCall, LLMProvider, ToolCall};

fn fake_stock_quote_call(id: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "quick_stock_quote".into(),
            arguments: r#"{"symbol":"MSFT"}"#.into(),
        },
        thought_signature: None,
    }
}

#[test]
fn invent_recovery_mandates_tool_search_dictionary() {
    let registry = ToolRegistry::new();
    let query = unknown_tool_search_query("quick_stock_quote");
    assert_eq!(query, "quick stock quote");

    let sample = unknown_tool_recovery_sample(&registry, &query, UNKNOWN_TOOL_SAMPLE_LIMIT);
    assert!(
        sample.iter().any(|n| n == "web_search"),
        "registry candidates must include web_search, got {sample:?}"
    );

    let body = unknown_tool_error_response(&registry, "quick_stock_quote");
    assert!(
        body.contains("tool_search"),
        "NotFound must point at tool_search, got: {}",
        edgecrab_core::safe_truncate(&body, 500)
    );
    assert!(body.contains("web_search"));
    assert!(body.contains("Do not retry the invalid name"));

    let err = recovery_catalog::unknown_tool("quick_stock_quote", None, &query, &sample);
    let recovery = err
        .to_llm_payload()
        .recovery_feedback
        .expect("recovery_feedback");
    assert_eq!(recovery.suggestions[0].action, RecoveryAction::CallToolFirst);
    let params = serde_json::to_string(&recovery.suggestions[0].parameters).expect("json");
    assert!(params.contains("tool_search"));
    assert!(params.contains("query"));
}

#[tokio::test]
async fn three_strike_unknown_tool_aborts_closed_and_typed() {
    let provider = MockAgentProvider::new();
    for i in 1..=3 {
        provider.add_tool_response_sync("", vec![fake_stock_quote_call(&format!("c{i}"))]);
    }

    let provider: Arc<dyn LLMProvider> = Arc::new(provider);
    let registry = Arc::new(ToolRegistry::new());
    let agent = AgentBuilder::new("mock-agent")
        .provider(provider)
        .tools(registry)
        .skip_context_files(true)
        .skip_memory(true)
        .max_iterations(12)
        .streaming(false)
        .build()
        .expect("build agent");

    let result: ConversationResult = agent
        .run_conversation("What is the MSFT price today?", None, None)
        .await
        .expect("run_conversation");

    assert_eq!(
        count_unanswered_tool_calls(&result.messages),
        0,
        "tool-call closure invariant must hold after PartialAbort"
    );

    assert_eq!(result.run_outcome.state, CompletionDecision::Failed);
    assert_eq!(
        result.run_outcome.exit_reason,
        ExitReason::InvalidToolBudget
    );
    assert!(
        result.final_response.contains("quick_stock_quote"),
        "abort reason in final_response: {}",
        result.final_response
    );

    let tool_blobs: Vec<String> = result
        .messages
        .iter()
        .filter(|m| m.role == edgecrab_types::Role::Tool)
        .map(|m| m.text_content())
        .collect();
    assert!(
        tool_blobs.iter().any(|b| b.contains("tool_search")),
        "unknown-tool results must mandate tool_search; blobs={tool_blobs:?}"
    );
    assert!(
        tool_blobs.iter().any(|b| b.contains("web_search")),
        "unknown-tool results must cite registry candidates; blobs={tool_blobs:?}"
    );

    let notice = format_operator_notice(&result.run_outcome);
    assert_eq!(
        notice.matches('❌').count(),
        1,
        "operator notice must not double-wrap:\n{notice}"
    );
    assert!(
        notice.contains("invalid tool call retry budget exhausted"),
        "typed InvalidToolBudget headline expected:\n{notice}"
    );
    assert!(
        !notice.contains("ended unexpectedly"),
        "generic Failed headline must not override typed exit:\n{notice}"
    );
}

#[tokio::test]
async fn invent_then_tool_search_avoids_invalid_tool_budget() {
    // Happy path (no network): invent → CallToolFirst(tool_search) → final text.
    let provider = MockAgentProvider::new();
    provider.add_tool_response_sync("", vec![fake_stock_quote_call("c1")]);
    provider.add_tool_response_sync(
        "",
        vec![ToolCall {
            id: "c2".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "tool_search".into(),
                arguments: r#"{"query":"stock price MSFT","limit":5}"#.into(),
            },
            thought_signature: None,
        }],
    );
    provider.add_response_sync(
        "I will use web_search next; tool_search returned registered candidates.",
    );

    let provider: Arc<dyn LLMProvider> = Arc::new(provider);
    let registry = Arc::new(ToolRegistry::new());
    let agent = AgentBuilder::new("mock-agent")
        .provider(provider)
        .tools(registry)
        .skip_context_files(true)
        .skip_memory(true)
        .max_iterations(12)
        .streaming(false)
        .build()
        .expect("build agent");

    let result = agent
        .run_conversation("What is the MSFT price today?", None, None)
        .await
        .expect("run_conversation");

    assert_eq!(
        count_unanswered_tool_calls(&result.messages),
        0,
        "history must stay closed"
    );
    assert_ne!(
        result.run_outcome.exit_reason,
        ExitReason::InvalidToolBudget,
        "recovering via tool_search must not abort"
    );
    let names: Vec<String> = result
        .messages
        .iter()
        .filter(|m| m.role == edgecrab_types::Role::Tool)
        .filter_map(|m| m.name.clone())
        .collect();
    assert!(
        names.iter().any(|n| n == "tool_search"),
        "expected tool_search in history, names={names:?}"
    );
    assert!(
        names.iter().any(|n| n == "quick_stock_quote"),
        "invent strike must leave a closed tool result, names={names:?}"
    );
    // tool_search result should surface real registry names.
    let search_blob = result
        .messages
        .iter()
        .find(|m| m.name.as_deref() == Some("tool_search"))
        .map(|m| m.text_content())
        .unwrap_or_default();
    assert!(
        search_blob.contains("web_search") || search_blob.contains("tool"),
        "tool_search should return catalog hits, got: {}",
        edgecrab_core::safe_truncate(&search_blob, 300)
    );
}
