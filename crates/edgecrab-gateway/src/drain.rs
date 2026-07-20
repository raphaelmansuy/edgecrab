//! Graceful gateway drain coordination.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

#[derive(Debug)]
struct Inner {
    accepting: AtomicBool,
    in_flight: AtomicUsize,
    changed: Notify,
}

/// Stops admission and waits for tracked work without polling sleeps.
#[derive(Clone, Debug)]
pub struct DrainController {
    inner: Arc<Inner>,
}

impl Default for DrainController {
    fn default() -> Self {
        Self::new()
    }
}

impl DrainController {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                accepting: AtomicBool::new(true),
                in_flight: AtomicUsize::new(0),
                changed: Notify::new(),
            }),
        }
    }

    pub fn is_accepting(&self) -> bool {
        self.inner.accepting.load(Ordering::Acquire)
    }

    /// Admit one unit of work. The post-increment recheck closes the race with
    /// `begin_drain`: work either gets a permit or is rolled back before return.
    pub fn try_start(&self) -> Option<DrainPermit> {
        if !self.is_accepting() {
            return None;
        }
        self.inner.in_flight.fetch_add(1, Ordering::AcqRel);
        if !self.is_accepting() {
            self.finish_one();
            return None;
        }
        Some(DrainPermit {
            controller: self.clone(),
        })
    }

    pub fn begin_drain(&self) {
        self.inner.accepting.store(false, Ordering::Release);
        self.inner.changed.notify_waiters();
    }

    pub fn in_flight(&self) -> usize {
        self.inner.in_flight.load(Ordering::Acquire)
    }

    pub async fn wait_for_idle(&self, timeout: Duration) -> bool {
        let wait = async {
            loop {
                let notified = self.inner.changed.notified();
                if self.in_flight() == 0 {
                    return;
                }
                notified.await;
            }
        };
        tokio::time::timeout(timeout, wait).await.is_ok()
    }

    fn finish_one(&self) {
        let previous = self.inner.in_flight.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "drain in-flight counter underflow");
        if previous == 1 {
            self.inner.changed.notify_waiters();
        }
    }
}

/// RAII token ensuring panics and early returns still release in-flight work.
pub struct DrainPermit {
    controller: DrainController,
}

impl Drop for DrainPermit {
    fn drop(&mut self) {
        self.controller.finish_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn drain_rejects_new_work_and_waits_for_release() {
        let drain = DrainController::new();
        let permit = drain.try_start().expect("admitted");
        drain.begin_drain();
        assert!(drain.try_start().is_none());

        let waiter = {
            let drain = drain.clone();
            tokio::spawn(async move { drain.wait_for_idle(Duration::from_secs(1)).await })
        };
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        drop(permit);
        assert!(waiter.await.expect("join"));
    }

    #[tokio::test]
    async fn drain_timeout_is_deterministic() {
        let drain = DrainController::new();
        let _permit = drain.try_start().expect("admitted");
        drain.begin_drain();
        assert!(!drain.wait_for_idle(Duration::ZERO).await);
    }
}
