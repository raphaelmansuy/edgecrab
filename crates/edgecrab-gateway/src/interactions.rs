//! Pending interaction broker for gateway sessions.
//!
//! WHY broker: approvals and clarifications should not be implicit side
//! effects of the message queue. This broker keeps one explicit FIFO queue of
//! user-facing interaction requests per session so replies can be resolved
//! deterministically.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::Mutex;

use edgecrab_core::ApprovalChoice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingInteractionKind {
    Approval {
        command: String,
        full_command: String,
        reasons: Vec<String>,
    },
    Clarify {
        question: String,
        choices: Option<Vec<String>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingInteractionView {
    pub id: u64,
    pub kind: PendingInteractionKind,
}

enum PendingResponder {
    Approval(tokio::sync::oneshot::Sender<ApprovalChoice>),
    Clarify(tokio::sync::oneshot::Sender<String>),
}

struct PendingInteraction {
    id: u64,
    kind: PendingInteractionKind,
    responder: PendingResponder,
}

pub struct InteractionBroker {
    queues: Mutex<HashMap<String, VecDeque<PendingInteraction>>>,
    next_id: AtomicU64,
}

impl InteractionBroker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            queues: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        })
    }

    pub async fn enqueue_approval(
        &self,
        session_key: &str,
        command: String,
        full_command: String,
        reasons: Vec<String>,
        response_tx: tokio::sync::oneshot::Sender<ApprovalChoice>,
    ) -> PendingInteractionView {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let kind = PendingInteractionKind::Approval {
            command,
            full_command,
            reasons,
        };
        let view = PendingInteractionView {
            id,
            kind: kind.clone(),
        };

        let interaction = PendingInteraction {
            id,
            kind,
            responder: PendingResponder::Approval(response_tx),
        };

        let mut queues = self.queues.lock().await;
        queues
            .entry(session_key.to_string())
            .or_default()
            .push_back(interaction);
        view
    }

    pub async fn enqueue_clarify(
        &self,
        session_key: &str,
        question: String,
        choices: Option<Vec<String>>,
        response_tx: tokio::sync::oneshot::Sender<String>,
    ) -> PendingInteractionView {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let kind = PendingInteractionKind::Clarify { question, choices };
        let view = PendingInteractionView {
            id,
            kind: kind.clone(),
        };

        let interaction = PendingInteraction {
            id,
            kind,
            responder: PendingResponder::Clarify(response_tx),
        };

        let mut queues = self.queues.lock().await;
        queues
            .entry(session_key.to_string())
            .or_default()
            .push_back(interaction);
        view
    }

    pub async fn peek(&self, session_key: &str) -> Option<PendingInteractionView> {
        let queues = self.queues.lock().await;
        queues.get(session_key).and_then(|queue| {
            queue.front().map(|item| PendingInteractionView {
                id: item.id,
                kind: item.kind.clone(),
            })
        })
    }

    pub async fn pending_count(&self, session_key: &str) -> usize {
        let queues = self.queues.lock().await;
        queues.get(session_key).map_or(0, VecDeque::len)
    }

    pub async fn resolve_oldest_approval(
        &self,
        session_key: &str,
        choice: ApprovalChoice,
    ) -> usize {
        let target = {
            let mut queues = self.queues.lock().await;
            let Some(queue) = queues.get_mut(session_key) else {
                return 0;
            };

            let is_approval = queue
                .front()
                .map(|item| matches!(item.kind, PendingInteractionKind::Approval { .. }))
                .unwrap_or(false);
            if !is_approval {
                return 0;
            }

            let item = queue.pop_front();
            if queue.is_empty() {
                queues.remove(session_key);
            }
            item
        };

        match target {
            Some(PendingInteraction {
                responder: PendingResponder::Approval(tx),
                ..
            }) => {
                let _ = tx.send(choice);
                1
            }
            _ => 0,
        }
    }

    pub async fn resolve_oldest_clarify(&self, session_key: &str, answer: String) -> bool {
        let target = {
            let mut queues = self.queues.lock().await;
            let Some(queue) = queues.get_mut(session_key) else {
                return false;
            };

            let is_clarify = queue
                .front()
                .map(|item| matches!(item.kind, PendingInteractionKind::Clarify { .. }))
                .unwrap_or(false);
            if !is_clarify {
                return false;
            }

            let item = queue.pop_front();
            if queue.is_empty() {
                queues.remove(session_key);
            }
            item
        };

        match target {
            Some(PendingInteraction {
                responder: PendingResponder::Clarify(tx),
                ..
            }) => {
                let _ = tx.send(answer);
                true
            }
            _ => false,
        }
    }

    pub async fn cancel_session(&self, session_key: &str) -> usize {
        let pending = {
            let mut queues = self.queues.lock().await;
            queues.remove(session_key).unwrap_or_default()
        };

        let mut count = 0usize;
        for item in pending {
            count += 1;
            match item.responder {
                PendingResponder::Approval(tx) => {
                    let _ = tx.send(ApprovalChoice::Deny);
                }
                PendingResponder::Clarify(tx) => {
                    let _ = tx.send(String::new());
                }
            }
        }
        count
    }
}

impl Default for InteractionBroker {
    fn default() -> Self {
        Self {
            queues: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approval_queue_is_fifo() {
        let broker = InteractionBroker::new();
        let (tx1, rx1) = tokio::sync::oneshot::channel();
        let (tx2, rx2) = tokio::sync::oneshot::channel();

        broker
            .enqueue_approval(
                "telegram:chat",
                "rm -rf /tmp/a".into(),
                "rm -rf /tmp/a".into(),
                vec!["destructive-file-ops".into()],
                tx1,
            )
            .await;
        broker
            .enqueue_approval(
                "telegram:chat",
                "rm -rf /tmp/b".into(),
                "rm -rf /tmp/b".into(),
                vec!["destructive-file-ops".into()],
                tx2,
            )
            .await;

        assert_eq!(
            broker
                .resolve_oldest_approval("telegram:chat", ApprovalChoice::Once)
                .await,
            1
        );
        assert_eq!(rx1.await.expect("first resolution"), ApprovalChoice::Once);

        let pending = broker.peek("telegram:chat").await.expect("second pending");
        match pending.kind {
            PendingInteractionKind::Approval { full_command, .. } => {
                assert!(full_command.ends_with("/tmp/b"));
            }
            _ => panic!("expected approval"),
        }
        assert_eq!(
            broker
                .resolve_oldest_approval("telegram:chat", ApprovalChoice::Deny)
                .await,
            1
        );
        assert_eq!(rx2.await.expect("second resolution"), ApprovalChoice::Deny);
    }

    #[tokio::test]
    async fn cancel_session_denies_approvals_and_clears_clarify() {
        let broker = InteractionBroker::new();
        let (approval_tx, approval_rx) = tokio::sync::oneshot::channel();
        let (clarify_tx, clarify_rx) = tokio::sync::oneshot::channel();

        broker
            .enqueue_approval(
                "discord:chat",
                "rm -rf /tmp/demo".into(),
                "rm -rf /tmp/demo".into(),
                vec![],
                approval_tx,
            )
            .await;
        broker
            .enqueue_clarify(
                "discord:chat",
                "Which folder?".into(),
                Some(vec!["Work".into(), "Personal".into()]),
                clarify_tx,
            )
            .await;

        assert_eq!(broker.cancel_session("discord:chat").await, 2);
        assert_eq!(
            approval_rx.await.expect("approval cancellation"),
            ApprovalChoice::Deny
        );
        assert_eq!(clarify_rx.await.expect("clarify cancellation"), "");
        assert!(broker.peek("discord:chat").await.is_none());
    }
}
