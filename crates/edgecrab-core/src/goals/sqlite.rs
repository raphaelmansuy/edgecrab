//! SQLite-backed persistent goal store (per session_id).

use std::sync::Arc;

use edgecrab_state::{SessionDb, StoredGoalState, StoredSubGoal};
use edgecrab_types::{AgentError, GoalContract, parse_goal_with_contract};

use super::{GoalState, GoalStatus, GoalStore, SubGoal};

fn map_state(stored: StoredGoalState) -> GoalState {
    GoalState {
        goal_text: stored.goal_text,
        subgoals: stored
            .subgoals
            .into_iter()
            .map(|s| SubGoal {
                id: s.id,
                text: s.text,
                done: s.done,
            })
            .collect(),
        status: GoalStatus::parse(&stored.status),
        turns_used: stored.turns_used,
        max_turns: stored.max_turns,
        paused_reason: stored.paused_reason,
        last_verdict: stored.last_verdict,
        last_reason: stored.last_reason,
        consecutive_parse_failures: stored.consecutive_parse_failures,
        contract: GoalContract::from_json(stored.contract_json.as_deref()),
    }
}

fn map_subgoal(sub: StoredSubGoal) -> SubGoal {
    SubGoal {
        id: sub.id,
        text: sub.text,
        done: sub.done,
    }
}

pub struct SqliteGoalStore {
    db: Arc<SessionDb>,
}

impl SqliteGoalStore {
    pub fn new(db: Arc<SessionDb>) -> Self {
        Self { db }
    }
}

impl GoalStore for SqliteGoalStore {
    fn active(&self, session_id: &str) -> Result<GoalState, AgentError> {
        Ok(map_state(self.db.goals_active(session_id)?))
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
        self.db
            .goals_set_with_contract(session_id, text, max_turns, contract.to_json().as_deref())
    }

    fn clear(&self, session_id: &str) -> Result<(), AgentError> {
        self.db.goals_clear(session_id)
    }

    fn push_subgoal(&self, session_id: &str, text: &str) -> Result<(), AgentError> {
        match self.db.goals_push_subgoal(session_id, text) {
            Ok(()) => Ok(()),
            Err(AgentError::Database(msg)) if msg.contains("top-level goal") => {
                Err(AgentError::Config(msg))
            }
            Err(err) => Err(err),
        }
    }

    fn complete_subgoal(&self, session_id: &str) -> Result<Option<SubGoal>, AgentError> {
        Ok(self.db.goals_complete_subgoal(session_id)?.map(map_subgoal))
    }

    fn clear_subgoals(&self, session_id: &str) -> Result<u32, AgentError> {
        match self.db.goals_clear_subgoals(session_id) {
            Ok(count) => Ok(count),
            Err(AgentError::Database(msg)) if msg.contains("top-level goal") => {
                Err(AgentError::Config(msg))
            }
            Err(err) => Err(err),
        }
    }

    fn remove_subgoal(&self, session_id: &str, index_1based: usize) -> Result<String, AgentError> {
        match self.db.goals_remove_subgoal(session_id, index_1based) {
            Ok(text) => Ok(text),
            Err(AgentError::Database(msg))
                if msg.contains("top-level goal") || msg.contains("index out of range") =>
            {
                Err(AgentError::Config(msg))
            }
            Err(err) => Err(err),
        }
    }

    fn pause(&self, session_id: &str, reason: &str) -> Result<(), AgentError> {
        self.db.goals_pause(session_id, reason)
    }

    fn resume(&self, session_id: &str, reset_budget: bool) -> Result<(), AgentError> {
        self.db.goals_resume(session_id, reset_budget)
    }

    fn save_loop_state(&self, session_id: &str, state: &GoalState) -> Result<(), AgentError> {
        let stored = StoredGoalState {
            goal_text: state.goal_text.clone(),
            subgoals: state
                .subgoals
                .iter()
                .map(|s| StoredSubGoal {
                    id: s.id,
                    text: s.text.clone(),
                    done: s.done,
                })
                .collect(),
            status: state.status.as_str().to_string(),
            turns_used: state.turns_used,
            max_turns: state.max_turns,
            paused_reason: state.paused_reason.clone(),
            last_verdict: state.last_verdict.clone(),
            last_reason: state.last_reason.clone(),
            consecutive_parse_failures: state.consecutive_parse_failures,
            contract_json: state.contract.to_json(),
        };
        self.db.goals_save_loop_state(session_id, &stored)
    }
}
