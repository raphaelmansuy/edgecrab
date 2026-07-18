//! Hermes-parity Ralph loop tests — mirrors `tests/hermes_cli/test_goals.py`.

use std::sync::Arc;

use async_trait::async_trait;
use edgecrab_core::{
    AgentBuilder, GoalContinuationDecision, GoalJudgeConfig, GoalStatus, GoalStore, GoalsConfig,
    InMemoryGoalStore, drain_goal_continuations_from_queue, evaluate_goal_after_turn,
    goal_judge::parse_judge_response, is_goal_continuation_text,
    prompt_queue_has_real_user_message,
};
use edgecrab_tools::registry::ToolRegistry;
use edgecrab_types::Platform;
use edgequake_llm::Result as LlmResult;
use edgequake_llm::traits::StreamChunk;
use edgequake_llm::{
    ChatMessage, CompletionOptions, LLMProvider, LLMResponse, ToolChoice, ToolDefinition,
};
use futures::StreamExt;

struct VerdictProvider {
    content: String,
}

#[async_trait]
impl LLMProvider for VerdictProvider {
    fn name(&self) -> &str {
        "verdict-mock"
    }

    fn model(&self) -> &str {
        "verdict-mock"
    }

    fn max_context_length(&self) -> usize {
        8192
    }

    async fn complete(&self, prompt: &str) -> LlmResult<LLMResponse> {
        Ok(LLMResponse::new(prompt, self.model()))
    }

    async fn complete_with_options(
        &self,
        prompt: &str,
        _options: &CompletionOptions,
    ) -> LlmResult<LLMResponse> {
        self.complete(prompt).await
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        options: Option<&CompletionOptions>,
    ) -> LlmResult<LLMResponse> {
        self.chat_with_tools(messages, &[], None, options).await
    }

    async fn chat_with_tools(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
        _tool_choice: Option<ToolChoice>,
        _options: Option<&CompletionOptions>,
    ) -> LlmResult<LLMResponse> {
        Ok(LLMResponse::new(&self.content, self.model()))
    }

    async fn chat_with_tools_stream(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
        _tool_choice: Option<ToolChoice>,
        _options: Option<&CompletionOptions>,
    ) -> LlmResult<futures::stream::BoxStream<'static, LlmResult<StreamChunk>>> {
        Ok(futures::stream::iter(Vec::<LlmResult<StreamChunk>>::new()).boxed())
    }
}

async fn eval_with_verdict(
    store: Arc<InMemoryGoalStore>,
    session_id: &str,
    max_turns: u32,
    verdict_json: &str,
    response: &str,
) -> GoalContinuationDecision {
    store
        .set_goal(session_id, "test goal", max_turns)
        .expect("set goal");
    let provider = Arc::new(VerdictProvider {
        content: verdict_json.into(),
    });
    evaluate_goal_after_turn(
        store,
        session_id,
        response,
        &[],
        false,
        &GoalsConfig::default(),
        &GoalJudgeConfig::default(),
        None,
        provider,
        "mock/test",
    )
    .await
    .expect("evaluate")
}

#[tokio::test]
async fn evaluate_after_turn_done() {
    let store = Arc::new(InMemoryGoalStore::new());
    let decision = eval_with_verdict(
        store.clone(),
        "eval-done",
        20,
        r#"{"done": true, "reason": "shipped"}"#,
        "I shipped the feature.",
    )
    .await;
    assert_eq!(decision.verdict, "done");
    assert!(!decision.should_continue);
    assert!(decision.continuation_prompt.is_none());
    assert_eq!(
        store.active("eval-done").expect("active").status,
        GoalStatus::Done
    );
    assert_eq!(store.active("eval-done").expect("active").turns_used, 1);
}

#[tokio::test]
async fn evaluate_after_turn_continue_under_budget() {
    let store = Arc::new(InMemoryGoalStore::new());
    let decision = eval_with_verdict(
        store.clone(),
        "eval-cont",
        5,
        r#"{"done": false, "reason": "more work"}"#,
        "made some progress",
    )
    .await;
    assert_eq!(decision.verdict, "continue");
    assert!(decision.should_continue);
    assert!(decision.continuation_prompt.is_some());
    assert!(decision.continuation_prompt.unwrap().contains("test goal"));
    assert_eq!(
        store.active("eval-cont").expect("active").status,
        GoalStatus::Active
    );
}

#[tokio::test]
async fn evaluate_after_turn_budget_exhausted() {
    let store = Arc::new(InMemoryGoalStore::new());
    store.set_goal("eval-budget", "hard goal", 2).expect("set");
    let provider = Arc::new(VerdictProvider {
        content: r#"{"done": false, "reason": "not yet"}"#.into(),
    });
    let cfg = GoalsConfig::default();
    let judge = GoalJudgeConfig::default();

    let d1 = evaluate_goal_after_turn(
        store.clone(),
        "eval-budget",
        "step 1",
        &[],
        false,
        &cfg,
        &judge,
        None,
        provider.clone(),
        "mock/test",
    )
    .await
    .expect("turn 1");
    assert!(d1.should_continue);
    assert_eq!(store.active("eval-budget").expect("s").turns_used, 1);

    let d2 = evaluate_goal_after_turn(
        store.clone(),
        "eval-budget",
        "step 2",
        &[],
        false,
        &cfg,
        &judge,
        None,
        provider,
        "mock/test",
    )
    .await
    .expect("turn 2");
    assert!(!d2.should_continue);
    let state = store.active("eval-budget").expect("s");
    assert_eq!(state.status, GoalStatus::Paused);
    assert_eq!(state.turns_used, 2);
    assert!(
        state
            .paused_reason
            .as_deref()
            .is_some_and(|r| r.contains("budget"))
    );
}

#[tokio::test]
async fn evaluate_after_turn_inactive_when_paused() {
    let store = Arc::new(InMemoryGoalStore::new());
    store.set_goal("eval-inact", "a goal", 20).expect("set");
    store.pause("eval-inact", "user").expect("pause");
    let provider = Arc::new(VerdictProvider {
        content: r#"{"done": true, "reason": "nope"}"#.into(),
    });
    let decision = evaluate_goal_after_turn(
        store,
        "eval-inact",
        "anything",
        &[],
        false,
        &GoalsConfig::default(),
        &GoalJudgeConfig::default(),
        None,
        provider,
        "mock/test",
    )
    .await
    .expect("eval");
    assert_eq!(decision.verdict, "inactive");
    assert!(!decision.should_continue);
}

#[tokio::test]
async fn auto_pause_after_three_parse_failures() {
    let store = Arc::new(InMemoryGoalStore::new());
    store.set_goal("parse-fail", "do a thing", 20).expect("set");
    let provider = Arc::new(VerdictProvider {
        content: "this is not json at all".into(),
    });
    let cfg = GoalsConfig::default();
    let judge = GoalJudgeConfig::default();

    for turn in 1..=2 {
        let d = evaluate_goal_after_turn(
            store.clone(),
            "parse-fail",
            &format!("step {turn}"),
            &[],
            false,
            &cfg,
            &judge,
            None,
            provider.clone(),
            "mock/test",
        )
        .await
        .expect("eval");
        assert!(d.should_continue, "turn {turn} should continue");
        assert_eq!(
            store
                .active("parse-fail")
                .expect("s")
                .consecutive_parse_failures,
            turn
        );
    }

    let d3 = evaluate_goal_after_turn(
        store.clone(),
        "parse-fail",
        "step 3",
        &[],
        false,
        &cfg,
        &judge,
        None,
        provider,
        "mock/test",
    )
    .await
    .expect("eval");
    assert!(!d3.should_continue);
    assert_eq!(
        store.active("parse-fail").expect("s").status,
        GoalStatus::Paused
    );
    assert!(d3.message.contains("goal_judge"));
}

#[tokio::test]
async fn agent_goal_commands_match_hermes_surface() {
    let store = Arc::new(InMemoryGoalStore::new());
    let agent = AgentBuilder::new("mock/test")
        .provider(Arc::new(VerdictProvider {
            content: String::new(),
        }))
        .tools(Arc::new(ToolRegistry::new()))
        .platform(Platform::Cli)
        .goal_store(store)
        .build()
        .expect("agent");

    agent.goal_set("Build API").await.expect("goal");
    let shown = agent.goal_show().await.expect("show");
    assert!(shown.contains("Build API"));
    agent.subgoal_push("write tests").await.expect("push");
    agent.subgoal_remove(1).await.expect("remove");
    agent.subgoal_clear().await.expect("clear");
    agent.goal_pause().await.expect("pause");
    let status = agent.goal_status().await.expect("status");
    assert!(status.contains("paused"));
    agent.goal_resume().await.expect("resume");
    // draft fail-open (mock returns empty → free-form set)
    let drafted = agent.goal_draft("Ship feature").await.expect("draft");
    assert!(drafted.contains("Goal set") || drafted.contains("Draft"));
    agent.goal_clear().await.expect("clear");
}

#[test]
fn parse_judge_string_done_values() {
    for s in ["true", "yes", "done", "1"] {
        let v = parse_judge_response(&format!(r#"{{"done": "{s}", "reason": "r"}}"#));
        assert!(v.done, "expected done for {s}");
    }
    for s in ["false", "no", "not yet"] {
        let v = parse_judge_response(&format!(r#"{{"done": "{s}", "reason": "r"}}"#));
        assert!(!v.done, "expected continue for {s}");
    }
}

#[test]
fn parse_malformed_json_is_parse_failure() {
    let v = parse_judge_response("this is not json at all");
    assert!(!v.done);
    assert!(v.parse_failed);
}

#[test]
fn parse_judge_wait_flag() {
    let v = parse_judge_response(r#"{"done": false, "reason": "bg server", "wait": true}"#);
    assert!(!v.done);
    assert!(v.wait);
}

#[tokio::test]
async fn evaluate_after_turn_wait_parks_without_continuation() {
    let store = Arc::new(InMemoryGoalStore::new());
    store
        .set_goal("eval-wait", "wait for server", 20)
        .expect("set");
    let provider = Arc::new(VerdictProvider {
        content: r#"{"done": false, "reason": "process still running", "wait": true}"#.into(),
    });
    let decision = evaluate_goal_after_turn(
        store.clone(),
        "eval-wait",
        "started server",
        &[],
        false,
        &GoalsConfig::default(),
        &GoalJudgeConfig::default(),
        None,
        provider,
        "mock/test",
    )
    .await
    .expect("evaluate");
    assert_eq!(decision.verdict, "wait");
    assert!(!decision.should_continue);
    assert!(decision.continuation_prompt.is_none());
    assert_eq!(
        store.active("eval-wait").expect("s").status,
        GoalStatus::Active
    );
}

#[test]
fn queue_peek_matches_hermes_slash_aware_preemption() {
    let cont = edgecrab_core::next_continuation_prompt(&edgecrab_core::GoalState {
        goal_text: Some("Ship".into()),
        status: GoalStatus::Active,
        ..Default::default()
    })
    .expect("continuation");
    assert!(is_goal_continuation_text(&cont));

    assert!(!prompt_queue_has_real_user_message(&[
        "/subgoal add tests".into()
    ]));
    assert!(prompt_queue_has_real_user_message(&[
        "/subgoal add tests".into(),
        "fix bug".into()
    ]));

    let mut queue = vec![cont, "user follow-up".into()];
    assert_eq!(drain_goal_continuations_from_queue(&mut queue), 1);
    assert_eq!(queue, vec!["user follow-up".to_string()]);
}

#[tokio::test]
async fn goal_is_active_tracks_pause_and_clear() {
    let store = Arc::new(InMemoryGoalStore::new());
    let agent = AgentBuilder::new("mock/test")
        .provider(Arc::new(VerdictProvider {
            content: String::new(),
        }))
        .tools(Arc::new(ToolRegistry::new()))
        .platform(Platform::Cli)
        .goal_store(store)
        .build()
        .expect("agent");

    assert!(!agent.goal_is_active().await.expect("active"));
    agent.goal_set("Ship feature").await.expect("set");
    assert!(agent.goal_is_active().await.expect("active"));
    agent.goal_pause().await.expect("pause");
    assert!(!agent.goal_is_active().await.expect("active"));
    agent.goal_clear().await.expect("clear");
    assert!(!agent.goal_is_active().await.expect("active"));
}
#[tokio::test]
async fn goal_resume_with_kickoff_returns_continuation() {
    let store = Arc::new(InMemoryGoalStore::new());
    let agent = AgentBuilder::new("mock/test")
        .provider(Arc::new(VerdictProvider {
            content: String::new(),
        }))
        .tools(Arc::new(ToolRegistry::new()))
        .platform(Platform::Cli)
        .goal_store(store)
        .build()
        .expect("agent");

    agent.goal_set("Build API").await.expect("set");
    agent.goal_pause().await.expect("pause");
    let (msg, cont) = agent.goal_resume_with_kickoff().await.expect("resume");
    assert!(msg.contains("resumed"));
    let prompt = cont.expect("continuation prompt");
    assert!(is_goal_continuation_text(&prompt));
    assert!(prompt.contains("Build API"));
}

#[tokio::test]
async fn goal_status_includes_subgoals_when_present() {
    let store = Arc::new(InMemoryGoalStore::new());
    let agent = AgentBuilder::new("mock/test")
        .provider(Arc::new(VerdictProvider {
            content: String::new(),
        }))
        .tools(Arc::new(ToolRegistry::new()))
        .platform(Platform::Cli)
        .goal_store(store)
        .build()
        .expect("agent");

    agent.goal_set("Ship feature").await.expect("set");
    agent.subgoal_push("write tests").await.expect("push");
    let status = agent.goal_status().await.expect("status");
    assert!(status.contains("write tests"));
    assert!(status.contains("[ ]"));
}

#[tokio::test]
async fn contract_vetoes_judge_done_without_tool_evidence() {
    let store = Arc::new(InMemoryGoalStore::new());
    let contract = edgecrab_types::GoalContract {
        verification: "cargo test".into(),
        ..Default::default()
    };
    store
        .set_goal_with_contract("contract-veto", "Ship feature", 20, &contract)
        .expect("set");
    let provider = Arc::new(VerdictProvider {
        content: r#"{"done": true, "reason": "looks done"}"#.into(),
    });
    let decision = evaluate_goal_after_turn(
        store.clone(),
        "contract-veto",
        "I ran cargo test and everything passed.",
        &[],
        false,
        &GoalsConfig::default(),
        &GoalJudgeConfig::default(),
        None,
        provider,
        "mock/test",
    )
    .await
    .expect("evaluate");
    assert_ne!(decision.verdict, "done");
    assert!(decision.should_continue);
    assert_eq!(
        store.active("contract-veto").expect("s").status,
        GoalStatus::Active
    );
}

#[tokio::test]
async fn contract_allows_done_with_successful_terminal_evidence() {
    let store = Arc::new(InMemoryGoalStore::new());
    let contract = edgecrab_types::GoalContract {
        verification: "cargo test".into(),
        ..Default::default()
    };
    store
        .set_goal_with_contract("contract-ok", "Ship feature", 20, &contract)
        .expect("set");
    let provider = Arc::new(VerdictProvider {
        content: r#"{"done": true, "reason": "tests green"}"#.into(),
    });
    let messages = vec![edgecrab_types::Message::tool_result(
        "t1",
        "terminal",
        "[terminal_result status=success backend=local cwd=/tmp exit_code=0]\n\
         cargo test\nok",
    )];
    let decision = evaluate_goal_after_turn(
        store.clone(),
        "contract-ok",
        "Tests passed.",
        &messages,
        false,
        &GoalsConfig::default(),
        &GoalJudgeConfig::default(),
        None,
        provider,
        "mock/test",
    )
    .await
    .expect("evaluate");
    assert_eq!(decision.verdict, "done");
    assert!(!decision.should_continue);
    assert_eq!(
        store.active("contract-ok").expect("s").status,
        GoalStatus::Done
    );
}

#[tokio::test]
async fn continuation_prompt_includes_contract_reminder() {
    let store = Arc::new(InMemoryGoalStore::new());
    let contract = edgecrab_types::GoalContract {
        verification: "cargo test -p edgecrab-core".into(),
        ..Default::default()
    };
    store
        .set_goal_with_contract("contract-cont", "Refactor module", 10, &contract)
        .expect("set");
    let provider = Arc::new(VerdictProvider {
        content: r#"{"done": false, "reason": "more work"}"#.into(),
    });
    let decision = evaluate_goal_after_turn(
        store,
        "contract-cont",
        "progress",
        &[],
        false,
        &GoalsConfig::default(),
        &GoalJudgeConfig::default(),
        None,
        provider,
        "mock/test",
    )
    .await
    .expect("evaluate");
    let prompt = decision.continuation_prompt.expect("continuation");
    assert!(prompt.contains("Must produce tool evidence"));
    assert!(prompt.contains("cargo test -p edgecrab-core"));
}
