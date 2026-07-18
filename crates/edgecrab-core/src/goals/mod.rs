//! Persistent session goals — Ralph loop intent re-injection.

mod loop_manager;
mod sqlite;

use std::sync::{Arc, Mutex};

use edgecrab_types::{AgentError, GoalContract, parse_goal_with_contract};

pub use loop_manager::{
    GoalContinuationDecision, GoalStatusChip, compact_status_chip,
    drain_goal_continuations_from_queue, evaluate_goal_after_turn, goal_flash_from_decision,
    is_goal_continuation_text, looks_like_slash_command, next_continuation_prompt,
    prompt_queue_has_real_user_message, render_subgoals_list, status_line,
};
pub use sqlite::SqliteGoalStore;

/// Lifecycle status for the Ralph loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalStatus {
    #[default]
    Cleared,
    Active,
    Paused,
    Done,
}

impl GoalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cleared => "cleared",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Done => "done",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "done" => Self::Done,
            _ => Self::Cleared,
        }
    }
}

/// One sub-step under the active top-level goal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubGoal {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

/// Active goal state for a session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalState {
    pub goal_text: Option<String>,
    pub subgoals: Vec<SubGoal>,
    pub status: GoalStatus,
    pub turns_used: u32,
    pub max_turns: u32,
    pub paused_reason: Option<String>,
    pub last_verdict: Option<String>,
    pub last_reason: Option<String>,
    pub consecutive_parse_failures: u32,
    /// Evidence-based completion contract (empty = free-form judge).
    #[serde(default)]
    pub contract: GoalContract,
}

impl Default for GoalState {
    fn default() -> Self {
        Self {
            goal_text: None,
            subgoals: Vec::new(),
            status: GoalStatus::Cleared,
            turns_used: 0,
            max_turns: 20,
            paused_reason: None,
            last_verdict: None,
            last_reason: None,
            consecutive_parse_failures: 0,
            contract: GoalContract::default(),
        }
    }
}

impl GoalState {
    pub fn is_empty(&self) -> bool {
        self.goal_text.as_ref().is_none_or(|t| t.trim().is_empty())
    }

    pub fn has_goal(&self) -> bool {
        !self.is_empty() && matches!(self.status, GoalStatus::Active | GoalStatus::Paused)
    }

    pub fn is_active(&self) -> bool {
        !self.is_empty() && self.status == GoalStatus::Active
    }
}

/// Storage backend for persistent goals.
pub trait GoalStore: Send + Sync {
    fn active(&self, session_id: &str) -> Result<GoalState, AgentError>;
    fn set_goal(&self, session_id: &str, text: &str, max_turns: u32) -> Result<(), AgentError>;
    /// Set goal with an explicit contract (preferred over parsing alone).
    fn set_goal_with_contract(
        &self,
        session_id: &str,
        text: &str,
        max_turns: u32,
        contract: &GoalContract,
    ) -> Result<(), AgentError>;
    fn clear(&self, session_id: &str) -> Result<(), AgentError>;
    fn push_subgoal(&self, session_id: &str, text: &str) -> Result<(), AgentError>;
    fn complete_subgoal(&self, session_id: &str) -> Result<Option<SubGoal>, AgentError>;
    fn clear_subgoals(&self, session_id: &str) -> Result<u32, AgentError>;
    fn remove_subgoal(&self, session_id: &str, index_1based: usize) -> Result<String, AgentError>;
    fn pause(&self, session_id: &str, reason: &str) -> Result<(), AgentError>;
    fn resume(&self, session_id: &str, reset_budget: bool) -> Result<(), AgentError>;
    fn save_loop_state(&self, session_id: &str, state: &GoalState) -> Result<(), AgentError>;
}

/// Render the per-turn injection block shown to the model.
pub fn render_goal_block(state: &GoalState) -> String {
    if !state.is_active() {
        return String::new();
    }

    let Some(goal) = state
        .goal_text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return String::new();
    };

    let mut lines = vec![
        "[GOAL CONTEXT — auto-injected each turn]".to_string(),
        format!("Active goal: {goal}"),
    ];

    if !state.subgoals.is_empty() {
        lines.push("Subgoals:".to_string());
        for (idx, sub) in state.subgoals.iter().enumerate() {
            let marker = if sub.done { "[x]" } else { "[ ]" };
            lines.push(format!("  {}. {marker} {}", idx + 1, sub.text));
        }
    }

    if !state.contract.is_empty() {
        lines.push("Completion contract:".to_string());
        if !state.contract.outcome.trim().is_empty() {
            lines.push(format!("  outcome: {}", state.contract.outcome.trim()));
        }
        if !state.contract.verification.trim().is_empty() {
            lines.push(format!(
                "  verification: {}",
                state.contract.verification.trim()
            ));
        }
        if !state.contract.constraints.trim().is_empty() {
            lines.push(format!(
                "  constraints: {}",
                state.contract.constraints.trim()
            ));
        }
        if !state.contract.boundaries.trim().is_empty() {
            lines.push(format!(
                "  boundaries: {}",
                state.contract.boundaries.trim()
            ));
        }
        if !state.contract.stop_when.trim().is_empty() {
            lines.push(format!("  stop_when: {}", state.contract.stop_when.trim()));
        }
        lines.push(
            "Done only with concrete evidence for verification (command/file/test/browser) — not vibes."
                .into(),
        );
    }

    lines.join("\n")
}

/// In-memory fallback when no SQLite state DB is configured (tests / minimal runs).
pub struct InMemoryGoalStore {
    sessions: Mutex<std::collections::HashMap<String, GoalState>>,
}

impl Default for InMemoryGoalStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryGoalStore {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn with_session<F, T>(&self, session_id: &str, f: F) -> Result<T, AgentError>
    where
        F: FnOnce(&mut GoalState) -> Result<T, AgentError>,
    {
        if session_id.trim().is_empty() {
            return Err(AgentError::Config(
                "session_id is required for goal operations".into(),
            ));
        }
        let mut guard = self
            .sessions
            .lock()
            .map_err(|_| AgentError::Database("goal store mutex poisoned".into()))?;
        f(guard.entry(session_id.to_string()).or_default())
    }

    fn require_goal(session_id: &str, state: &GoalState) -> Result<(), AgentError> {
        if !state.has_goal() {
            return Err(AgentError::Config(
                "set a top-level goal with /goal before managing subgoals".into(),
            ));
        }
        let _ = session_id;
        Ok(())
    }
}

impl GoalStore for InMemoryGoalStore {
    fn active(&self, session_id: &str) -> Result<GoalState, AgentError> {
        if session_id.trim().is_empty() {
            return Ok(GoalState::default());
        }
        let guard = self
            .sessions
            .lock()
            .map_err(|_| AgentError::Database("goal store mutex poisoned".into()))?;
        Ok(guard.get(session_id).cloned().unwrap_or_default())
    }

    fn set_goal(&self, session_id: &str, text: &str, max_turns: u32) -> Result<(), AgentError> {
        let (goal_text, contract) = parse_goal_with_contract(text);
        self.set_goal_with_contract(session_id, &goal_text, max_turns, &contract)
    }

    fn set_goal_with_contract(
        &self,
        session_id: &str,
        text: &str,
        max_turns: u32,
        contract: &GoalContract,
    ) -> Result<(), AgentError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(AgentError::Config("goal text must not be empty".into()));
        }
        self.with_session(session_id, |state| {
            state.goal_text = Some(trimmed.to_string());
            state.subgoals.clear();
            state.status = GoalStatus::Active;
            state.turns_used = 0;
            state.max_turns = max_turns.max(1);
            state.paused_reason = None;
            state.last_verdict = None;
            state.last_reason = None;
            state.consecutive_parse_failures = 0;
            state.contract = contract.clone();
            Ok(())
        })
    }

    fn clear(&self, session_id: &str) -> Result<(), AgentError> {
        if session_id.trim().is_empty() {
            return Ok(());
        }
        let mut guard = self
            .sessions
            .lock()
            .map_err(|_| AgentError::Database("goal store mutex poisoned".into()))?;
        guard.remove(session_id);
        Ok(())
    }

    fn push_subgoal(&self, session_id: &str, text: &str) -> Result<(), AgentError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(AgentError::Config("subgoal text must not be empty".into()));
        }
        self.with_session(session_id, |state| {
            if state.goal_text.as_ref().is_none_or(|g| g.trim().is_empty()) {
                return Err(AgentError::Config(
                    "set a top-level goal with /goal before adding subgoals".into(),
                ));
            }
            let next_id = state
                .subgoals
                .iter()
                .map(|s| s.id)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            state.subgoals.push(SubGoal {
                id: next_id,
                text: trimmed.to_string(),
                done: false,
            });
            Ok(())
        })
    }

    fn complete_subgoal(&self, session_id: &str) -> Result<Option<SubGoal>, AgentError> {
        self.with_session(session_id, |state| {
            if let Some(sub) = state.subgoals.iter_mut().rev().find(|s| !s.done) {
                sub.done = true;
                return Ok(Some(sub.clone()));
            }
            Ok(None)
        })
    }

    fn clear_subgoals(&self, session_id: &str) -> Result<u32, AgentError> {
        self.with_session(session_id, |state| {
            Self::require_goal(session_id, state)?;
            let count = state.subgoals.len() as u32;
            state.subgoals.clear();
            Ok(count)
        })
    }

    fn remove_subgoal(&self, session_id: &str, index_1based: usize) -> Result<String, AgentError> {
        self.with_session(session_id, |state| {
            Self::require_goal(session_id, state)?;
            if index_1based == 0 || index_1based > state.subgoals.len() {
                return Err(AgentError::Config(format!(
                    "index out of range (1..{})",
                    state.subgoals.len()
                )));
            }
            let removed = state.subgoals.remove(index_1based - 1);
            Ok(removed.text)
        })
    }

    fn pause(&self, session_id: &str, reason: &str) -> Result<(), AgentError> {
        self.with_session(session_id, |state| {
            if state.is_empty() {
                return Ok(());
            }
            state.status = GoalStatus::Paused;
            state.paused_reason = Some(reason.to_string());
            Ok(())
        })
    }

    fn resume(&self, session_id: &str, reset_budget: bool) -> Result<(), AgentError> {
        self.with_session(session_id, |state| {
            if state.is_empty() {
                return Ok(());
            }
            state.status = GoalStatus::Active;
            state.paused_reason = None;
            if reset_budget {
                state.turns_used = 0;
            }
            Ok(())
        })
    }

    fn save_loop_state(&self, session_id: &str, state: &GoalState) -> Result<(), AgentError> {
        self.with_session(session_id, |existing| {
            *existing = state.clone();
            Ok(())
        })
    }
}

/// Build the default goal store for an agent (SQLite when a state DB exists).
pub fn goal_store_for_db(db: Option<Arc<edgecrab_state::SessionDb>>) -> Arc<dyn GoalStore> {
    match db {
        Some(db) => Arc::new(SqliteGoalStore::new(db)),
        None => Arc::new(InMemoryGoalStore::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> InMemoryGoalStore {
        InMemoryGoalStore::new()
    }

    #[test]
    fn empty_store_renders_nothing() {
        let state = GoalState::default();
        assert!(render_goal_block(&state).is_empty());
        assert!(store().active("s1").expect("active").is_empty());
    }

    #[test]
    fn set_goal_and_show() {
        let s = store();
        s.set_goal("sess", "Refactor payment service", 20)
            .expect("set");
        let state = s.active("sess").expect("active");
        assert_eq!(state.goal_text.as_deref(), Some("Refactor payment service"));
        assert_eq!(state.status, GoalStatus::Active);
        let block = render_goal_block(&state);
        assert!(block.contains("Active goal: Refactor payment service"));
        assert!(block.contains("[GOAL CONTEXT"));
    }

    #[test]
    fn paused_goal_not_injected() {
        let s = store();
        s.set_goal("sess", "Ship feature", 20).expect("set");
        s.pause("sess", "user-paused").expect("pause");
        let state = s.active("sess").expect("active");
        assert!(render_goal_block(&state).is_empty());
    }

    #[test]
    fn push_subgoal_ordering() {
        let s = store();
        s.set_goal("sess", "Ship feature", 20).expect("set");
        s.push_subgoal("sess", "step 1").expect("push");
        s.push_subgoal("sess", "step 2").expect("push");
        let state = s.active("sess").expect("active");
        assert_eq!(state.subgoals.len(), 2);
        assert_eq!(state.subgoals[0].text, "step 1");
        assert_eq!(state.subgoals[1].text, "step 2");
    }

    #[test]
    fn complete_subgoal_marks_most_recent_undone() {
        let s = store();
        s.set_goal("sess", "Ship feature", 20).expect("set");
        s.push_subgoal("sess", "step 1").expect("push");
        s.push_subgoal("sess", "step 2").expect("push");
        let done = s.complete_subgoal("sess").expect("done");
        assert_eq!(done.as_ref().map(|d| d.text.as_str()), Some("step 2"));
        let state = s.active("sess").expect("active");
        assert!(!state.subgoals[0].done);
        assert!(state.subgoals[1].done);
    }

    #[test]
    fn remove_subgoal_by_index() {
        let s = store();
        s.set_goal("sess", "Ship feature", 20).expect("set");
        s.push_subgoal("sess", "first").expect("push");
        s.push_subgoal("sess", "second").expect("push");
        let removed = s.remove_subgoal("sess", 2).expect("remove");
        assert_eq!(removed, "second");
        assert_eq!(s.active("sess").expect("active").subgoals.len(), 1);
    }

    #[test]
    fn clear_subgoals_keeps_goal() {
        let s = store();
        s.set_goal("sess", "Ship feature", 20).expect("set");
        s.push_subgoal("sess", "a").expect("push");
        s.push_subgoal("sess", "b").expect("push");
        let cleared = s.clear_subgoals("sess").expect("clear");
        assert_eq!(cleared, 2);
        let state = s.active("sess").expect("active");
        assert!(state.subgoals.is_empty());
        assert_eq!(state.goal_text.as_deref(), Some("Ship feature"));
    }

    #[test]
    fn two_session_isolation() {
        let s = store();
        s.set_goal("a", "Goal A", 20).expect("set a");
        s.set_goal("b", "Goal B", 20).expect("set b");
        assert_eq!(
            s.active("a").expect("a").goal_text.as_deref(),
            Some("Goal A")
        );
    }

    #[test]
    fn clear_empties_session_only() {
        let s = store();
        s.set_goal("a", "Goal A", 20).expect("set a");
        s.set_goal("b", "Goal B", 20).expect("set b");
        s.clear("a").expect("clear");
        assert!(s.active("a").expect("a").is_empty());
        assert_eq!(
            s.active("b").expect("b").goal_text.as_deref(),
            Some("Goal B")
        );
    }

    #[test]
    fn set_goal_replaces_subgoals() {
        let s = store();
        s.set_goal("sess", "First", 20).expect("set");
        s.push_subgoal("sess", "sub").expect("push");
        s.set_goal("sess", "Second", 20).expect("replace");
        let state = s.active("sess").expect("active");
        assert_eq!(state.goal_text.as_deref(), Some("Second"));
        assert!(state.subgoals.is_empty());
    }

    #[test]
    fn pause_and_resume() {
        let s = store();
        s.set_goal("sess", "Loop", 20).expect("set");
        s.pause("sess", "testing").expect("pause");
        assert_eq!(s.active("sess").expect("active").status, GoalStatus::Paused);
        s.resume("sess", true).expect("resume");
        let state = s.active("sess").expect("active");
        assert_eq!(state.status, GoalStatus::Active);
        assert_eq!(state.turns_used, 0);
    }

    #[test]
    fn set_goal_parses_inline_contract() {
        let s = store();
        s.set_goal(
            "sess",
            "Ship auth\nverification: cargo test --workspace\nconstraints: keep API",
            20,
        )
        .expect("set");
        let state = s.active("sess").expect("active");
        assert_eq!(state.goal_text.as_deref(), Some("Ship auth"));
        assert_eq!(state.contract.verification, "cargo test --workspace");
        assert!(render_goal_block(&state).contains("verification:"));
    }
}
