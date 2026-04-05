//! # ProcessTable — shared background process tracking
//!
//! WHY DashMap: Multiple tool invocations may read/write the process
//! table concurrently (e.g., a `list_processes` call while a build
//! is running). DashMap provides lock-free concurrent reads and
//! fine-grained shard locking for writes.
//!
//! ```text
//!   Agent
//!     └── Arc<ProcessTable>
//!           └── ToolContext (each invocation gets a clone of the Arc)
//!                 └── ProcessTool.execute() → start/list/kill
//! ```
//!
//! Process lifecycle:
//! ```text
//!   spawn → Pending → Running → Exited / Killed
//!              │
//!              └── background tokio task drains stdout/stderr to Ring buffer
//!              └── PID stored for OS-level SIGKILL
//! ```
//!
//! ## Parity with hermes-agent
//!
//! | Feature                         | hermes-agent        | edgecrab (this)           |
//! |---------------------------------|---------------------|---------------------------|
//! | Ring buffer output               | 200KB char-based    | 500 line-based VecDeque   |
//! | OS SIGKILL on kill()             | os.killpg (SIGTERM) | kill(-pgid) + kill(pid)   |
//! | Whole process group kill         | os.killpg(-pgid, …) | kill(-pgid, SIGKILL)      |
//! | MAX_PROCESSES LRU pruning        | 64, LRU             | 64, oldest-first          |
//! | FINISHED_TTL GC                  | 30 min              | 30 min, auto task         |
//! | Shell noise filtering            | ✓                   | ✓ (in drain_reader)       |
//! | get_process_output (tail)        | ✓ process(poll)     | ✓ tail param              |
//! | get_process_output (pagination)  | ✓ process(log)      | ✓ offset+limit params     |
//! | wait_for_process tool            | ✓ process(wait)     | ✓                         |
//! | kill_all (session reset)         | ✓ kill_all()        | ✓ kill_all(session_key)   |
//! | ring buffer O(1) insertion       | char-based 200KB    | VecDeque O(1)             |
//! | safe env for background procs    | _sanitize_env()     | safe_env()                |
//! | process group isolation          | os.setsid           | .process_group(0)         |

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::{Mutex, mpsc};

/// Maximum number of tracked processes (LRU pruning beyond this).
/// Mirrors hermes-agent's `MAX_PROCESSES = 64`.
pub const MAX_PROCESSES: usize = 64;

/// Keep finished processes for 30 minutes before GC.
/// Mirrors hermes-agent's `FINISHED_TTL_SECONDS = 1800`.
pub const FINISHED_TTL: Duration = Duration::from_secs(1800);

// ─── ProcessRecord ────────────────────────────────────────────────────

/// A captured snapshot of a background process.
#[derive(Debug, Clone)]
pub struct ProcessRecord {
    /// User-facing stable ID (e.g., "proc-1", incremented monotonically)
    pub process_id: String,
    /// The command that was launched
    pub command: String,
    /// Current status
    pub status: ProcessStatus,
    /// Wall-clock start time
    pub started_at: std::time::SystemTime,
    /// Working directory at the time of launch
    pub cwd: String,
    /// Captured output lines (ring buffer, most recent `RING_CAPACITY` lines)
    ///
    /// WHY VecDeque: allows O(1) `pop_front()` eviction when the buffer is full,
    /// vs the O(n) `Vec::remove(0)` that shifts all elements on each insert.
    pub output_lines: VecDeque<String>,
    /// Exit code (set when status transitions to Exited)
    pub exit_code: Option<i32>,
    /// OS process ID — set immediately after spawn.
    ///
    /// WHY: Stored so `kill()` can send `SIGKILL` to the actual OS process.
    /// Without this, `kill_process` only mutates the in-memory record but
    /// leaves the OS process running — diverging from hermes-agent behaviour.
    pub pid: Option<u32>,
    /// Gateway session key that spawned this process.
    ///
    /// WHY: Mirrors hermes-agent `ProcessSession.session_key` — used by
    /// `has_active_for_session()` to let the gateway check whether any of
    /// a user's background processes are still running before resetting the
    /// session state.
    pub session_key: String,
    /// Channel for sending data to the process's stdin.
    ///
    /// WHY: Mirrors hermes-agent `write_stdin()` — interactive tools (REPL,
    /// Claude Code, Codex) need input injected after spawn.  A channel sender
    /// decouples the tool call from the actual write, which happens in a
    /// background task holding the ChildStdin handle.
    pub stdin_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

/// Process lifecycle states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Exited,
    Killed,
}

impl ProcessStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Killed => "killed",
        }
    }
}

// Maximum output lines retained per process (prevents unbounded memory growth)
const RING_CAPACITY: usize = 500;

// ─── ProcessTable ─────────────────────────────────────────────────────

/// Shared table of background processes for the current agent session.
///
/// WHY `Arc<Mutex<ProcessRecord>>`: The outer DashMap is lock-free for
/// insertions/removals. The inner Mutex allows the output-draining
/// background task to append lines while the tool reads them.
pub struct ProcessTable {
    records: DashMap<String, Arc<Mutex<ProcessRecord>>>,
    controls: DashMap<String, ProcessControl>,
    next_id: std::sync::atomic::AtomicU32,
}

enum ProcessControl {
    Remote(RemoteProcessControl),
}

struct RemoteProcessControl {
    kill_tx: mpsc::UnboundedSender<()>,
}

impl ProcessTable {
    pub fn new() -> Self {
        Self {
            records: DashMap::new(),
            controls: DashMap::new(),
            next_id: std::sync::atomic::AtomicU32::new(1),
        }
    }

    /// Allocate the next monotonic process ID string.
    fn allocate_id(&self) -> String {
        let n = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("proc-{n}")
    }

    /// Prune the oldest finished processes when the table exceeds MAX_PROCESSES.
    ///
    /// WHY: Prevents unbounded memory growth in long-running gateway sessions.
    /// Strategy: remove oldest-started finished processes first, then oldest
    /// running processes if still over limit (same as hermes-agent LRU pruning).
    pub async fn prune_if_full(&self) {
        if self.records.len() < MAX_PROCESSES {
            return;
        }

        // Collect (process_id, started_at, is_running) snapshots
        let mut entries: Vec<(String, std::time::SystemTime, bool)> = Vec::new();
        for entry in self.records.iter() {
            let rec = entry.value().lock().await;
            let is_running = rec.status == ProcessStatus::Running;
            entries.push((entry.key().clone(), rec.started_at, is_running));
        }

        // Sort: finished first (removing them is safest), then by age (oldest first)
        entries.sort_by(|a, b| {
            let a_fin = !a.2; // finished = true first
            let b_fin = !b.2;
            b_fin.cmp(&a_fin).then_with(|| a.1.cmp(&b.1))
        });

        let to_remove = entries.len().saturating_sub(MAX_PROCESSES - 1);
        for (id, _, _) in entries.into_iter().take(to_remove) {
            self.records.remove(&id);
            self.controls.remove(&id);
        }
    }

    /// Register a new running process. Returns the assigned process_id.
    ///
    /// `session_key` identifies the gateway session that launched this process
    /// (e.g. `"telegram:12345"`).  Pass an empty string for CLI/test contexts.
    /// Used by `has_active_for_session()` for gateway reset protection —
    /// mirrors hermes-agent's `ProcessSession.session_key`.
    pub fn register(
        &self,
        command: impl Into<String>,
        cwd: impl Into<String>,
        session_key: impl Into<String>,
    ) -> String {
        let id = self.allocate_id();
        let record = ProcessRecord {
            process_id: id.clone(),
            command: command.into(),
            status: ProcessStatus::Running,
            started_at: std::time::SystemTime::now(),
            cwd: cwd.into(),
            output_lines: VecDeque::new(),
            exit_code: None,
            pid: None,
            session_key: session_key.into(),
            stdin_tx: None,
        };
        self.records
            .insert(id.clone(), Arc::new(Mutex::new(record)));
        id
    }

    /// Set the stdin channel sender for an already-registered process.
    ///
    /// Call immediately after the stdin writer task is spawned.  The sender
    /// is used by `write_stdin` tool to inject data into the process's stdin.
    pub async fn set_stdin_tx(
        &self,
        process_id: &str,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        if let Some(entry) = self.records.get(process_id) {
            let mut rec = entry.value().lock().await;
            rec.stdin_tx = Some(tx);
        }
    }

    /// Register remote-process kill control for a non-local backend task.
    pub fn set_remote_kill(&self, process_id: &str, tx: mpsc::UnboundedSender<()>) {
        self.controls.insert(
            process_id.to_string(),
            ProcessControl::Remote(RemoteProcessControl { kill_tx: tx }),
        );
    }

    /// Retrieve the stdin sender for a running process.
    ///
    /// Returns `None` if the process doesn't exist or stdin is unavailable.
    pub async fn get_stdin_tx(
        &self,
        process_id: &str,
    ) -> Option<tokio::sync::mpsc::UnboundedSender<String>> {
        if let Some(entry) = self.records.get(process_id) {
            let rec = entry.value().lock().await;
            rec.stdin_tx.clone()
        } else {
            None
        }
    }

    /// Check whether any processes for the given session key are still running.
    ///
    /// WHY: Mirrors hermes-agent's `process_registry.has_active_for_session(key)`.
    /// The gateway calls this before resetting a session to avoid killing
    /// background processes that the user started (e.g. a build or test run).
    pub async fn has_active_for_session(&self, session_key: &str) -> bool {
        for entry in self.records.iter() {
            let rec = entry.value().lock().await;
            if rec.session_key == session_key && rec.status == ProcessStatus::Running {
                return true;
            }
        }
        false
    }

    /// Set the OS process ID for an already-registered process.
    ///
    /// Call immediately after the OS child is spawned.  The PID is used
    /// by `kill()` to send `SIGKILL` to the actual OS process.
    pub async fn set_pid(&self, process_id: &str, pid: u32) {
        if let Some(entry) = self.records.get(process_id) {
            let mut rec = entry.value().lock().await;
            rec.pid = Some(pid);
        }
    }

    /// Append output lines to a process (called by background drain task).
    pub async fn append_output(&self, process_id: &str, lines: Vec<String>) {
        if let Some(entry) = self.records.get(process_id) {
            let mut rec: tokio::sync::MutexGuard<'_, ProcessRecord> = entry.value().lock().await;
            for line in lines {
                if rec.output_lines.len() >= RING_CAPACITY {
                    rec.output_lines.pop_front(); // O(1) eviction
                }
                rec.output_lines.push_back(line);
            }
        }
    }

    /// Replace the buffered output with a fresh snapshot.
    ///
    /// Remote backends are polled by reading a log file from inside the
    /// sandbox. Each poll returns a current snapshot, so replacing avoids
    /// duplicating lines on every poll iteration.
    pub async fn replace_output(&self, process_id: &str, lines: Vec<String>) {
        if let Some(entry) = self.records.get(process_id) {
            let mut rec = entry.value().lock().await;
            rec.output_lines.clear();
            for line in lines {
                if rec.output_lines.len() >= RING_CAPACITY {
                    rec.output_lines.pop_front();
                }
                rec.output_lines.push_back(line);
            }
        }
    }

    /// Mark a process as exited with an exit code.
    pub async fn mark_exited(&self, process_id: &str, exit_code: i32) {
        if let Some(entry) = self.records.get(process_id) {
            let mut rec: tokio::sync::MutexGuard<'_, ProcessRecord> = entry.value().lock().await;
            rec.status = ProcessStatus::Exited;
            rec.exit_code = Some(exit_code);
        }
        self.controls.remove(process_id);
    }

    /// Mark a process as killed.
    pub async fn mark_killed(&self, process_id: &str) {
        if let Some(entry) = self.records.get(process_id) {
            let mut rec: tokio::sync::MutexGuard<'_, ProcessRecord> = entry.value().lock().await;
            rec.status = ProcessStatus::Killed;
        }
        self.controls.remove(process_id);
    }

    /// List all process records (cloned snapshots, sorted by start time desc).
    pub async fn list_all(&self) -> Vec<ProcessRecord> {
        let mut records: Vec<ProcessRecord> = Vec::new();
        for entry in self.records.iter() {
            let rec: tokio::sync::MutexGuard<'_, ProcessRecord> = entry.value().lock().await;
            records.push(rec.clone());
        }
        records.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        records
    }

    /// Retrieve output lines for a specific process.
    pub async fn get_output(&self, process_id: &str) -> Option<Vec<String>> {
        if let Some(entry) = self.records.get(process_id) {
            let rec: tokio::sync::MutexGuard<'_, ProcessRecord> = entry.value().lock().await;
            Some(rec.output_lines.iter().cloned().collect())
        } else {
            None
        }
    }

    /// Retrieve the last `tail` output lines as a single string.
    ///
    /// Used by `get_process_output` tool to return buffered output
    /// to the agent, mirroring hermes-agent's `process(action="poll")`.
    pub async fn get_output_tail(
        &self,
        process_id: &str,
        tail: usize,
    ) -> Option<(String, ProcessStatus, Option<i32>)> {
        if let Some(entry) = self.records.get(process_id) {
            let rec = entry.value().lock().await;
            let skip = rec.output_lines.len().saturating_sub(tail);
            let text = rec
                .output_lines
                .iter()
                .skip(skip)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            Some((text, rec.status.clone(), rec.exit_code))
        } else {
            None
        }
    }

    /// Retrieve a paginated slice of output lines with offset and limit.
    ///
    /// When `offset == 0`, returns the **last** `limit` lines (most recent),
    /// which is the default for `get_process_output` and mirrors hermes-agent's
    /// `process(action="log", offset=0)` behaviour.
    ///
    /// When `offset > 0`, skips the first `offset` lines from the oldest end
    /// of the ring buffer and returns up to `limit` lines.
    ///
    /// Returns `None` if the process doesn't exist.
    /// Returns `(text, total_lines, status, exit_code)`.
    pub async fn get_output_page(
        &self,
        process_id: &str,
        offset: usize,
        limit: usize,
    ) -> Option<(String, usize, ProcessStatus, Option<i32>)> {
        if let Some(entry) = self.records.get(process_id) {
            let rec = entry.value().lock().await;
            let total = rec.output_lines.len();
            let selected: Vec<&str> = if offset == 0 {
                // Default: last `limit` lines (tail behaviour)
                let skip = total.saturating_sub(limit);
                rec.output_lines
                    .iter()
                    .skip(skip)
                    .map(|s| s.as_str())
                    .collect()
            } else {
                rec.output_lines
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .map(|s| s.as_str())
                    .collect()
            };
            let text = selected.join("\n");
            Some((text, total, rec.status.clone(), rec.exit_code))
        } else {
            None
        }
    }

    /// Attempt to kill a process. Returns true if the process existed.
    ///
    /// WHY real kill: Without sending `SIGKILL` to the OS process, only the
    /// in-memory record is updated and the process keeps running.  This method
    /// sends `SIGKILL` via `libc::kill()` on Unix when a PID is available,
    /// mirroring hermes-agent's `os.killpg(-pgid, signal.SIGKILL)` behaviour.
    pub async fn kill(&self, process_id: &str) -> bool {
        if let Some(entry) = self.records.get(process_id) {
            let pid = {
                let rec = entry.value().lock().await;
                rec.pid
            };

            // Send SIGKILL to the process group so sh -c's children are also
            // killed. run_process spawns with .process_group(0), so pgid == pid.
            // Sending kill(-pgid, SIGKILL) kills every process in the group,
            // matching hermes-agent's os.killpg(-pgid, signal.SIGKILL) pattern.
            //
            // We also send a direct kill(pid, SIGKILL) as belt-and-suspenders
            // in case the process already moved its children to another group.
            #[cfg(unix)]
            if let Some(pid) = pid {
                unsafe {
                    // Kill the whole process group first
                    libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
                    // Belt-and-suspenders: direct kill in case pgid differs
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }

            if let Some(control) = self.controls.get(process_id) {
                match control.value() {
                    ProcessControl::Remote(remote) => {
                        let _ = remote.kill_tx.send(());
                    }
                }
            }

            self.mark_killed(process_id).await;
            true
        } else {
            false
        }
    }

    /// Kill all running processes, optionally filtered by session key.
    ///
    /// Returns the count of processes killed.
    ///
    /// WHY: Mirrors hermes-agent's `process_registry.kill_all(task_id=None)`.
    /// The gateway calls this on session reset to reap orphaned background
    /// processes before resetting state — prevents resource leaks when a user
    /// starts a new conversation.
    ///
    /// When `session_key` is `Some`, only processes for that key are killed.
    /// When `None`, all running processes are killed.
    pub async fn kill_all(&self, session_key: Option<&str>) -> usize {
        // Collect IDs first to avoid holding DashMap shards across async points.
        let ids_to_kill: Vec<String> = {
            let mut ids = Vec::new();
            for entry in self.records.iter() {
                let rec = entry.value().lock().await;
                let matches = match session_key {
                    Some(key) => rec.session_key == key,
                    None => true,
                };
                if matches && rec.status == ProcessStatus::Running {
                    ids.push(entry.key().clone());
                }
            }
            ids
        };

        let mut killed = 0;
        for id in ids_to_kill {
            if self.kill(&id).await {
                killed += 1;
            }
        }
        killed
    }

    /// Remove processes that exited/were killed more than `age` ago.
    /// Call periodically to prevent unbounded table growth.  
    pub async fn gc(&self, age: Duration) {
        let now = Instant::now();
        let cutoff = std::time::SystemTime::now()
            .checked_sub(age)
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

        let mut to_remove = Vec::new();
        for entry in self.records.iter() {
            let rec: tokio::sync::MutexGuard<'_, ProcessRecord> = entry.value().lock().await;
            if rec.status != ProcessStatus::Running && rec.started_at < cutoff {
                to_remove.push(entry.key().clone());
            }
            drop(rec); // release lock before modifying DashMap
        }
        let _ = now; // silence unused warning

        for id in to_remove {
            self.records.remove(&id);
            self.controls.remove(&id);
        }
    }

    /// Spawn the background GC task.
    ///
    /// WHY: Without this, finished processes accumulate forever.  The task
    /// mirrors hermes-agent's `FINISHED_TTL_SECONDS = 1800` cleanup logic.
    /// Call once per process-table lifetime (typically from `Agent::new()`).
    pub fn spawn_gc_task(self: &Arc<Self>, cancel: tokio_util::sync::CancellationToken) {
        let table = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // every 5 min
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        table.gc(FINISHED_TTL).await;
                    }
                    _ = cancel.cancelled() => break,
                }
            }
        });
    }

    /// Number of currently registered processes.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

impl Default for ProcessTable {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_list() {
        let table = ProcessTable::new();
        let id = table.register("cargo build", "/tmp", "");
        let records = table.list_all().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].process_id, id);
        assert_eq!(records[0].status, ProcessStatus::Running);
    }

    #[tokio::test]
    async fn append_output_ring_buffer() {
        let table = ProcessTable::new();
        let id = table.register("echo loop", "/tmp", "");
        // Overfill the ring buffer
        let mut lines = Vec::new();
        for i in 0..=RING_CAPACITY + 10 {
            lines.push(format!("line {i}"));
        }
        table.append_output(&id, lines).await;
        let output = table.get_output(&id).await.expect("should exist");
        assert!(output.len() <= RING_CAPACITY, "ring buffer exceeded");
    }

    #[tokio::test]
    async fn kill_marks_record() {
        let table = ProcessTable::new();
        let id = table.register("long_running", "/tmp", "");
        let killed = table.kill(&id).await;
        assert!(killed, "kill should return true for existing id");
        let records = table.list_all().await;
        assert_eq!(records[0].status, ProcessStatus::Killed);
    }

    #[tokio::test]
    async fn kill_unknown_returns_false() {
        let table = ProcessTable::new();
        let killed = table.kill("proc-9999").await;
        assert!(!killed);
    }

    #[tokio::test]
    async fn gc_removes_old_entries() {
        let table = ProcessTable::new();
        let id = table.register("old", "/tmp", "");
        table.mark_exited(&id, 0).await;
        // GC with zero age should clean everything
        table.gc(Duration::from_secs(0)).await;
        assert!(table.is_empty(), "GC should have removed old entries");
    }

    #[tokio::test]
    async fn monotonic_ids() {
        let table = ProcessTable::new();
        let id1 = table.register("cmd1", "/tmp", "");
        let id2 = table.register("cmd2", "/tmp", "");
        assert_ne!(id1, id2);
        assert!(id2 > id1, "ids should be monotonically increasing");
    }

    #[tokio::test]
    async fn has_active_for_session_returns_true_for_running() {
        let table = ProcessTable::new();
        table.register("server", "/tmp", "telegram:42");
        assert!(table.has_active_for_session("telegram:42").await);
        assert!(!table.has_active_for_session("telegram:99").await);
    }

    #[tokio::test]
    async fn has_active_for_session_false_after_exit() {
        let table = ProcessTable::new();
        let id = table.register("build", "/tmp", "cli:1");
        table.mark_exited(&id, 0).await;
        assert!(!table.has_active_for_session("cli:1").await);
    }

    // ─── kill_all tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn kill_all_no_filter_kills_all_running() {
        let table = ProcessTable::new();
        let id1 = table.register("proc1", "/tmp", "session:A");
        let id2 = table.register("proc2", "/tmp", "session:B");
        let id3 = table.register("proc3", "/tmp", "session:A");
        // Mark id3 as already exited — should not count
        table.mark_exited(&id3, 0).await;

        let killed = table.kill_all(None).await;
        assert_eq!(killed, 2, "should kill both running processes");

        let recs = table.list_all().await;
        for rec in &recs {
            if rec.process_id == id1 || rec.process_id == id2 {
                assert_eq!(rec.status, ProcessStatus::Killed);
            }
        }
    }

    #[tokio::test]
    async fn kill_all_with_session_key_kills_only_matching() {
        let table = ProcessTable::new();
        let id1 = table.register("serverA", "/tmp", "session:A");
        let id2 = table.register("serverB", "/tmp", "session:B");

        let killed = table.kill_all(Some("session:A")).await;
        assert_eq!(killed, 1, "only session:A process killed");

        // id1 should be killed, id2 still running
        let recs = table.list_all().await;
        for rec in &recs {
            if rec.process_id == id1 {
                assert_eq!(rec.status, ProcessStatus::Killed);
            } else if rec.process_id == id2 {
                assert_eq!(rec.status, ProcessStatus::Running);
            }
        }
    }

    #[tokio::test]
    async fn kill_all_returns_zero_when_none_running() {
        let table = ProcessTable::new();
        let id = table.register("done", "/tmp", "s:1");
        table.mark_exited(&id, 0).await;

        let killed = table.kill_all(None).await;
        assert_eq!(killed, 0);
    }

    // ─── get_output_page tests ───────────────────────────────────────

    #[tokio::test]
    async fn get_output_page_default_returns_tail() {
        let table = ProcessTable::new();
        let id = table.register("cmd", "/tmp", "");
        let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
        table.append_output(&id, lines).await;

        // offset=0, limit=5 → last 5 lines
        let (text, total, _status, _ec) = table
            .get_output_page(&id, 0, 5)
            .await
            .expect("should exist");
        assert_eq!(total, 20);
        let result_lines: Vec<&str> = text.lines().collect();
        assert_eq!(result_lines.len(), 5);
        assert_eq!(result_lines[0], "line 15");
        assert_eq!(result_lines[4], "line 19");
    }

    #[tokio::test]
    async fn get_output_page_with_offset_skips_lines() {
        let table = ProcessTable::new();
        let id = table.register("cmd", "/tmp", "");
        let lines: Vec<String> = (0..10).map(|i| format!("line {i}")).collect();
        table.append_output(&id, lines).await;

        // offset=3, limit=4 → lines 3,4,5,6
        let (text, total, _status, _ec) = table
            .get_output_page(&id, 3, 4)
            .await
            .expect("should exist");
        assert_eq!(total, 10);
        let result_lines: Vec<&str> = text.lines().collect();
        assert_eq!(result_lines.len(), 4);
        assert_eq!(result_lines[0], "line 3");
        assert_eq!(result_lines[3], "line 6");
    }

    #[tokio::test]
    async fn get_output_page_not_found_returns_none() {
        let table = ProcessTable::new();
        assert!(table.get_output_page("proc-999", 0, 10).await.is_none());
    }
}
