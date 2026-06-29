//! Turn-scoped live activity state for the activity shelf (Hermes `turnController` parity).
//!
//! Single source of truth for **what is live** this turn — separate from transcript history.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use edgecrab_core::safe_truncate;

/// Long-run gentle hint after this many seconds on one tool (Hermes charms ~8s).
pub const LONG_RUN_HINT_SECS: u64 = 8;

/// Max long-run hints per turn (across all tools).
pub const MAX_LONG_RUN_HINTS_PER_TURN: usize = 4;

/// Max charms shown for a single tool (Hermes `MAX_CHARMS_PER_TOOL`).
pub const MAX_LONG_RUN_HINTS_PER_TOOL: usize = 2;

/// Minimum gap between charms on the same tool (Hermes ~10s).
pub const LONG_RUN_HINT_INTERVAL_SECS: u64 = 10;

/// Rolling activity feed lines in the shelf (Hermes Activity panel).
pub const SHELF_ACTIVITY_FEED_MAX: usize = 4;

/// Inline bg-process tail budget for the shelf (chars, not full `/tail` panel).
pub const SHELF_BG_TAIL_CHARS: usize = 120;

/// Default long-run charm copy (Hermes `LONG_RUN_CHARMS` parity).
pub const DEFAULT_LONG_RUN_CHARMS: &[&str] = &[
    "still cooking…",
    "polishing edges…",
    "asking the void nicely…",
];

/// One-time shelf tip after the first long tool in a session.
pub const SHELF_ONBOARDING_SECS: u64 = 30;

/// Max tool rows in collapsed/summary shelf mode.
pub const SHELF_MAX_TOOL_ROWS: usize = 3;

/// Max parallel tool rows when `/details tools expanded` (Hermes shows all active tools).
pub const SHELF_MAX_TOOL_ROWS_FULL: usize = 12;

/// Compact tool summary for the status bar (single source when shelf is enabled).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnToolSummary {
    pub primary_name: String,
    pub detail: String,
    pub active_count: usize,
    pub elapsed_secs: u64,
    pub preparing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ShelfPhase {
    #[default]
    Idle,
    AwaitingFirstToken,
    Thinking,
    GeneratingTool,
    ToolExec,
    Streaming,
    WaitingForApproval,
    WaitingForClarify,
    #[allow(dead_code)]
    BgOp,
}

#[derive(Clone, Debug)]
pub struct ShelfToolRow {
    pub tool_call_id: String,
    pub name: String,
    pub args_json: String,
    pub preview: String,
    pub detail: Option<String>,
    pub started_at: Instant,
    pub last_seq: u64,
    pub finished: bool,
}

#[derive(Clone, Debug)]
pub struct ShelfBgRow {
    pub process_id: String,
    pub command_preview: String,
    pub tail: String,
    pub finished: bool,
}

#[derive(Clone, Debug)]
pub struct ShelfSubagentRow {
    pub task_index: usize,
    pub task_count: usize,
    pub goal: String,
    pub detail: Option<String>,
    /// Stable subagent id for tree grouping / replay.
    pub agent_id: String,
    /// Parent subagent id when nested.
    pub parent_id: Option<String>,
    /// Delegation depth (1 = direct child of root agent).
    pub depth: u32,
    /// Running count of tool calls in this delegate (Hermes `toolCount`).
    pub tool_count: usize,
    pub current_tool: Option<String>,
    pub started_at: Instant,
    /// Recent tool names for output-tail preview (Hermes `outputTail`).
    pub recent_tools: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityTone {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityNotice {
    pub text: String,
    pub tone: ActivityTone,
}

/// Ephemeral turn state consumed by [`crate::activity_shelf`].
#[derive(Clone, Debug)]
pub struct TurnActivityState {
    pub enabled: bool,
    pub phase: ShelfPhase,
    pub phase_started: Instant,
    pub tools: HashMap<String, ShelfToolRow>,
    pub bg_processes: HashMap<String, ShelfBgRow>,
    pub subagents: HashMap<usize, ShelfSubagentRow>,
    pub generating_tool: Option<(String, String)>,
    pub generating_preview: Option<String>,
    /// Bytes accumulated in the in-flight tool-call JSON stream.
    pub generating_args_bytes: usize,
    pub reasoning_snippet: Option<String>,
    /// Live rough estimate for thinking shelf header (Hermes `thinkingTokens`).
    pub thinking_token_est: u32,
    /// Accumulated rough estimate for tool args this turn (Hermes `toolTokenAcc`).
    pub tool_token_acc: u32,
    pub hint: Option<String>,
    /// Human-readable detail for blocking non-streaming LLM waits (LM Studio, Ollama).
    pub llm_wait_detail: Option<String>,
    /// Short status-bar label for the same wait (no mid-word truncation).
    pub llm_wait_compact: Option<String>,
    /// Short rolling notices (long-run charms, onboarding) — Hermes Activity feed.
    pub activity_feed: Vec<ActivityNotice>,
    long_run_hints_per_tool: HashMap<String, usize>,
    long_run_last_at: HashMap<String, Instant>,
    long_run_hint_count: usize,
    long_run_charms: Vec<String>,
}

impl Default for TurnActivityState {
    fn default() -> Self {
        Self::new(false)
    }
}

impl TurnActivityState {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            phase: ShelfPhase::Idle,
            phase_started: Instant::now(),
            tools: HashMap::new(),
            bg_processes: HashMap::new(),
            subagents: HashMap::new(),
            generating_tool: None,
            generating_preview: None,
            generating_args_bytes: 0,
            reasoning_snippet: None,
            thinking_token_est: 0,
            tool_token_acc: 0,
            hint: None,
            llm_wait_detail: None,
            llm_wait_compact: None,
            activity_feed: Vec::new(),
            long_run_hints_per_tool: HashMap::new(),
            long_run_last_at: HashMap::new(),
            long_run_hint_count: 0,
            long_run_charms: DEFAULT_LONG_RUN_CHARMS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }

    pub fn set_long_run_charms(&mut self, charms: Vec<String>) {
        if !charms.is_empty() {
            self.long_run_charms = charms;
        }
    }

    pub fn reset_turn(&mut self) {
        self.phase = ShelfPhase::Idle;
        self.phase_started = Instant::now();
        self.tools.clear();
        self.bg_processes.clear();
        self.subagents.clear();
        self.generating_tool = None;
        self.generating_preview = None;
        self.generating_args_bytes = 0;
        self.reasoning_snippet = None;
        self.thinking_token_est = 0;
        self.tool_token_acc = 0;
        self.hint = None;
        self.llm_wait_detail = None;
        self.llm_wait_compact = None;
        self.activity_feed.clear();
        self.long_run_hints_per_tool.clear();
        self.long_run_last_at.clear();
        self.long_run_hint_count = 0;
    }

    /// Hermes `pruneTransient()` — clear bridge phases when the model resumes.
    pub fn on_model_resuming(&mut self) {
        if matches!(
            self.phase,
            ShelfPhase::AwaitingFirstToken
        ) {
            self.set_phase(ShelfPhase::Streaming);
        }
        self.llm_wait_detail = None;
        self.llm_wait_compact = None;
    }

    /// Clear in-flight streamed tool draft UI (non-streaming retry / stall abort).
    pub fn clear_tool_generating(&mut self) {
        self.generating_tool = None;
        self.generating_preview = None;
        self.generating_args_bytes = 0;
        if matches!(self.phase, ShelfPhase::GeneratingTool) {
            self.set_phase(ShelfPhase::AwaitingFirstToken);
        }
    }

    pub fn on_llm_wait_progress(
        &mut self,
        provider: &str,
        elapsed_secs: u64,
        has_tools: bool,
        ctx: edgecrab_tools::tool_progress_tail::LlmWaitContext,
    ) {
        let detail = edgecrab_tools::tool_progress_tail::llm_wait_progress_label(
            provider, elapsed_secs, has_tools, ctx,
        );
        self.llm_wait_compact = Some(edgecrab_tools::tool_progress_tail::llm_wait_status_compact(
            provider, has_tools, ctx,
        ));
        self.llm_wait_detail = Some(detail.clone());
        self.set_phase(ShelfPhase::AwaitingFirstToken);
        let tone = if elapsed_secs >= 45 {
            ActivityTone::Warn
        } else {
            ActivityTone::Info
        };
        if self
            .activity_feed
            .last()
            .is_some_and(|last| last.text == detail)
        {
            return;
        }
        if self.activity_feed.len() >= SHELF_ACTIVITY_FEED_MAX {
            self.activity_feed.remove(0);
        }
        self.activity_feed.push(ActivityNotice {
            text: detail,
            tone,
        });
        self.hint = self.llm_wait_detail.clone();
    }

    pub fn push_activity(&mut self, text: String, tone: ActivityTone) {
        if text.trim().is_empty() {
            return;
        }
        let notice = ActivityNotice { text, tone };
        if self
            .activity_feed
            .last()
            .is_some_and(|last| last.text == notice.text)
        {
            return;
        }
        if self.activity_feed.len() >= SHELF_ACTIVITY_FEED_MAX {
            self.activity_feed.remove(0);
        }
        self.activity_feed.push(notice.clone());
        self.hint = Some(notice.text.clone());
    }

    /// Short label for status bar / ghost line during blocking LLM waits.
    pub fn llm_wait_compact_label(&self) -> Option<&str> {
        self.llm_wait_compact.as_deref()
    }

    /// Full shelf line during blocking LLM waits (activity feed).
    pub fn llm_wait_label(&self) -> Option<&str> {
        self.llm_wait_detail.as_deref().or_else(|| {
            self.activity_feed.iter().rev().find_map(|n| {
                let t = n.text.as_str();
                if t.contains("composing") || t.contains("non-streaming") || t.contains("Bedrock") {
                    Some(t)
                } else {
                    None
                }
            })
        })
    }

    pub fn set_phase(&mut self, phase: ShelfPhase) {
        if self.phase != phase {
            self.phase = phase;
            self.phase_started = Instant::now();
        }
    }

    pub fn on_generating(&mut self, tool_call_id: String, name: String, partial_args: String) {
        self.on_model_resuming();
        self.generating_tool = Some((tool_call_id, name.clone()));
        self.generating_args_bytes = partial_args.len();
        let preview = crate::tool_display::extract_streaming_tool_preview(&name, &partial_args);
        self.generating_preview = Some(crate::transcript_heights::bounded_live_render_text(
            preview.trim(),
            crate::transcript_heights::LIVE_RENDER_MAX_CHARS,
        ));
        self.set_phase(ShelfPhase::GeneratingTool);
    }

    pub fn on_tool_exec(
        &mut self,
        tool_call_id: String,
        name: String,
        args_json: String,
        preview: String,
        seq: u64,
    ) {
        self.on_model_resuming();
        self.generating_tool = None;
        self.generating_preview = None;
        self.generating_args_bytes = 0;
        let rough = crate::shelf_visual::estimate_tokens_rough(&format!("{name} {preview}"));
        self.tool_token_acc = self.tool_token_acc.saturating_add(rough);
        self.tools.insert(
            tool_call_id.clone(),
            ShelfToolRow {
                tool_call_id,
                name,
                args_json,
                preview,
                detail: None,
                started_at: Instant::now(),
                last_seq: seq,
                finished: false,
            },
        );
        self.set_phase(ShelfPhase::ToolExec);
    }

    pub fn tool_row(&self, tool_call_id: &str) -> Option<&ShelfToolRow> {
        self.tools.get(tool_call_id)
    }

    pub fn contains_tool(&self, tool_call_id: &str) -> bool {
        self.tools.contains_key(tool_call_id)
    }

    pub fn latest_active_tool(&self) -> Option<(&str, &ShelfToolRow)> {
        self.sorted_active_tools()
            .next()
            .map(|row| (row.tool_call_id.as_str(), row))
    }

    pub fn tool_elapsed_secs(&self, tool_call_id: &str) -> Option<u64> {
        self.tools
            .get(tool_call_id)
            .map(|row| row.started_at.elapsed().as_secs())
    }

    pub fn maybe_onboarding_hint(&mut self, onboarding_done: &mut bool) {
        if *onboarding_done || !self.enabled {
            return;
        }
        let threshold = Duration::from_secs(SHELF_ONBOARDING_SECS);
        let long_running = self
            .tools
            .values()
            .any(|row| !row.finished && row.started_at.elapsed() >= threshold);
        if long_running {
            self.push_activity(
                "Tip: live activity stays in the shelf — /agents for delegates, /tail for bg logs"
                    .into(),
                ActivityTone::Info,
            );
            *onboarding_done = true;
        }
    }

    pub fn on_tool_progress(&mut self, tool_call_id: &str, detail: String, seq: u64, now: Instant) {
        let started = self.tools.get(tool_call_id).map(|row| row.started_at);
        if let Some(row) = self.tools.get_mut(tool_call_id) {
            row.detail = Some(detail);
            row.last_seq = seq;
        }
        if let Some(started) = started {
            self.maybe_long_run_hint(tool_call_id, started, now);
        }
        self.set_phase(ShelfPhase::ToolExec);
    }

    pub fn on_tool_done(&mut self, tool_call_id: &str) {
        // Hermes removes from activeTools on complete — transcript owns history.
        self.tools.remove(tool_call_id);
        self.long_run_hints_per_tool.remove(tool_call_id);
        self.long_run_last_at.remove(tool_call_id);
        if self.tools.is_empty() {
            self.set_phase(ShelfPhase::AwaitingFirstToken);
        } else {
            self.set_phase(ShelfPhase::ToolExec);
        }
    }

    pub fn on_bg_tail(&mut self, process_id: String, command_preview: String, tail: String) {
        self.bg_processes
            .entry(process_id.clone())
            .and_modify(|row| {
                row.command_preview = command_preview.clone();
                row.tail = tail.clone();
            })
            .or_insert(ShelfBgRow {
                process_id,
                command_preview,
                tail,
                finished: false,
            });
    }

    pub fn on_bg_finished(&mut self, process_id: &str) {
        if let Some(row) = self.bg_processes.get_mut(process_id) {
            row.finished = true;
        }
    }

    pub fn on_reasoning(&mut self, text: &str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            self.on_model_resuming();
            self.reasoning_snippet =
                Some(crate::stream_bridge::truncate_reasoning_snippet(trimmed));
            self.thinking_token_est = self
                .thinking_token_est
                .saturating_add(crate::shelf_visual::estimate_tokens_rough(text));
            if matches!(
                self.phase,
                ShelfPhase::Idle | ShelfPhase::AwaitingFirstToken
            ) {
                self.set_phase(ShelfPhase::Thinking);
            }
        }
    }

    pub fn on_subagent_start(
        &mut self,
        task_index: usize,
        task_count: usize,
        goal: String,
        depth: u32,
        agent_id: String,
        parent_id: Option<String>,
    ) {
        self.subagents.insert(
            task_index,
            ShelfSubagentRow {
                task_index,
                task_count,
                goal: goal.clone(),
                detail: None,
                agent_id,
                parent_id,
                depth,
                tool_count: 0,
                current_tool: None,
                started_at: Instant::now(),
                recent_tools: Vec::new(),
            },
        );
        if self.enabled && task_count > 0 {
            self.push_activity(
                format!(
                    "delegating · [{}/{}] {}",
                    task_index + 1,
                    task_count,
                    safe_truncate(goal.trim(), 48)
                ),
                ActivityTone::Info,
            );
        }
    }

    pub fn on_subagent_detail(&mut self, task_index: usize, detail: String) {
        if let Some(row) = self.subagents.get_mut(&task_index) {
            row.detail = Some(detail);
        }
    }

    pub fn on_subagent_tool(&mut self, task_index: usize, name: &str, tool_label: String) {
        if let Some(row) = self.subagents.get_mut(&task_index) {
            row.tool_count = row.tool_count.saturating_add(1);
            row.current_tool = Some(tool_label.clone());
            row.detail = Some(tool_label);
            push_recent_tool_name(&mut row.recent_tools, name);
        }
    }

    pub fn subagent_tool_total(&self) -> usize {
        self.subagents.values().map(|s| s.tool_count).sum()
    }

    pub fn on_subagent_finish(&mut self, task_index: usize) {
        self.subagents.remove(&task_index);
    }

    pub fn visible(&self, is_processing: bool) -> bool {
        self.enabled && is_processing && self.has_content()
    }

    pub fn has_content(&self) -> bool {
        !matches!(self.phase, ShelfPhase::Idle)
            || !self.tools.is_empty()
            || !self.bg_processes.is_empty()
            || self.generating_tool.is_some()
            || self.hint.is_some()
            || !self.activity_feed.is_empty()
    }

    pub fn phase_line(&self) -> Option<String> {
        let elapsed = self.phase_started.elapsed().as_secs();
        match self.phase {
            ShelfPhase::Idle => None,
            ShelfPhase::AwaitingFirstToken => {
                if let Some(detail) = self.llm_wait_label() {
                    Some(format!(
                        "{} ({elapsed}s)",
                        edgecrab_core::safe_truncate(detail, 72)
                    ))
                } else {
                    Some(format!("awaiting model response ({elapsed}s)"))
                }
            }
            ShelfPhase::Thinking => self
                .reasoning_snippet
                .clone()
                .map(|s| format!("thinking · {s} ({elapsed}s)"))
                .or_else(|| Some(format!("thinking ({elapsed}s)"))),
            ShelfPhase::GeneratingTool => self.generating_caption(),
            ShelfPhase::Streaming => Some(format!("streaming ({elapsed}s)")),
            ShelfPhase::ToolExec => self.active_tool_caption(),
            ShelfPhase::WaitingForApproval => Some("waiting for approval".into()),
            ShelfPhase::WaitingForClarify => Some("waiting for clarification".into()),
            ShelfPhase::BgOp => Some("background operation…".into()),
        }
    }

    /// Single-line caption for shelf compact mode, input title, and status hints.
    pub fn live_caption(&self) -> Option<String> {
        if let Some(detail) = self.llm_wait_label() {
            let elapsed = self.phase_started.elapsed().as_secs();
            return Some(format!(
                "{} ({elapsed}s)",
                edgecrab_core::safe_truncate(detail, 72)
            ));
        }
        if let Some(text) = self.generating_caption() {
            return Some(text);
        }
        if let Some(text) = self.active_tool_caption() {
            return Some(text);
        }
        if !self.subagents.is_empty() {
            let total_tools = self.subagent_tool_total();
            if total_tools > 0 {
                return Some(format!(
                    "{} delegate(s) · {total_tools} tools",
                    self.subagents.len()
                ));
            }
            return Some(format!("{} delegate(s) active", self.subagents.len()));
        }
        self.phase_line()
    }

    /// Minimum shelf rows while processing — never hide active tool work.
    pub fn minimum_shelf_lines(&self, is_processing: bool) -> u16 {
        if !self.enabled || !is_processing {
            return 0;
        }
        if self.generating_tool.is_some()
            || self.tools.values().any(|t| !t.finished)
            || !self.subagents.is_empty()
        {
            return 1;
        }
        if self.has_content() {
            return 1;
        }
        0
    }

    fn generating_caption(&self) -> Option<String> {
        let (_, name) = self.generating_tool.as_ref()?;
        let label = name.replace('_', " ");
        let elapsed = self.phase_started.elapsed().as_secs();
        let elapsed_suffix = if elapsed > 0 {
            format!(" · {elapsed}s")
        } else {
            String::new()
        };
        let preview = self
            .generating_preview
            .as_deref()
            .filter(|p| !p.trim().is_empty());
        Some(match preview {
            Some(p) => format!("preparing {label} · {p}{elapsed_suffix}"),
            None => format!(
                "preparing {label} · {}{elapsed_suffix}",
                edgecrab_tools::tool_progress_tail::format_streaming_args_progress(
                    self.generating_args_bytes
                )
            ),
        })
    }

    fn active_tool_caption(&self) -> Option<String> {
        let active: Vec<_> = self.sorted_active_tools().collect();
        let primary = active.first()?;
        let elapsed = primary.started_at.elapsed().as_secs();
        let detail = primary
            .detail
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or(primary.preview.as_str());
        let name = primary.name.replace('_', " ");
        let elapsed_suffix = if elapsed > 0 {
            format!(" · {elapsed}s")
        } else {
            String::new()
        };
        if active.len() > 1 {
            Some(format!("{name} · {detail} +{}", active.len() - 1))
        } else {
            Some(format!("{name} · {detail}{elapsed_suffix}"))
        }
    }

    pub fn sorted_active_tools(&self) -> impl Iterator<Item = &ShelfToolRow> {
        let mut rows: Vec<_> = self.tools.values().filter(|r| !r.finished).collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.last_seq));
        rows.into_iter()
    }

    /// Status-bar summary — parallel-aware (`3 tools · terminal +2`).
    pub fn tool_summary(&self) -> Option<TurnToolSummary> {
        if matches!(self.phase, ShelfPhase::GeneratingTool) {
            let (_, name) = self.generating_tool.as_ref()?;
            return Some(TurnToolSummary {
                primary_name: name.clone(),
                detail: self
                    .generating_preview
                    .clone()
                    .unwrap_or_else(|| {
                        edgecrab_tools::tool_progress_tail::format_streaming_args_progress(
                            self.generating_args_bytes,
                        )
                    }),
                active_count: 0,
                elapsed_secs: self.phase_started.elapsed().as_secs(),
                preparing: true,
            });
        }
        let active: Vec<_> = self.sorted_active_tools().collect();
        if active.is_empty() {
            return None;
        }
        let primary = active[0];
        let detail = primary
            .detail
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or(primary.preview.as_str())
            .to_string();
        Some(TurnToolSummary {
            primary_name: primary.name.clone(),
            detail,
            active_count: active.len(),
            elapsed_secs: primary.started_at.elapsed().as_secs(),
            preparing: false,
        })
    }

    fn maybe_long_run_hint(&mut self, tool_call_id: &str, started: Instant, now: Instant) {
        if self.long_run_hint_count >= MAX_LONG_RUN_HINTS_PER_TURN {
            return;
        }
        let per_tool = self
            .long_run_hints_per_tool
            .get(tool_call_id)
            .copied()
            .unwrap_or(0);
        if per_tool >= MAX_LONG_RUN_HINTS_PER_TOOL {
            return;
        }
        if now.duration_since(started) < Duration::from_secs(LONG_RUN_HINT_SECS) {
            return;
        }
        if let Some(last) = self.long_run_last_at.get(tool_call_id)
            && now.duration_since(*last) < Duration::from_secs(LONG_RUN_HINT_INTERVAL_SECS)
        {
            return;
        }
        if let Some(row) = self.tools.get(tool_call_id) {
            let charm = self
                .long_run_charms
                .get(per_tool % self.long_run_charms.len())
                .map(String::as_str)
                .unwrap_or("still working");
            let detail = row
                .detail
                .as_deref()
                .filter(|d| !d.trim().is_empty())
                .unwrap_or(row.preview.as_str());
            let line = format!("{charm} — {} · {detail}", row.name.replace('_', " "));
            self.push_activity(line, ActivityTone::Warn);
            self.long_run_hints_per_tool
                .insert(tool_call_id.to_string(), per_tool + 1);
            self.long_run_last_at.insert(tool_call_id.to_string(), now);
            self.long_run_hint_count += 1;
        }
    }

    pub fn tick_long_run_hints(&mut self, now: Instant) {
        if matches!(self.phase, ShelfPhase::GeneratingTool)
            && let Some((id, _)) = self.generating_tool.clone()
        {
            self.maybe_generating_long_run_hint(&id, now);
        }
        let active: Vec<(String, Instant)> = self
            .tools
            .iter()
            .filter(|(_, row)| !row.finished)
            .map(|(id, row)| (id.clone(), row.started_at))
            .collect();
        for (id, started) in active {
            self.maybe_long_run_hint(&id, started, now);
            if self.long_run_hint_count >= MAX_LONG_RUN_HINTS_PER_TURN {
                break;
            }
        }
    }

    fn maybe_generating_long_run_hint(&mut self, tool_call_id: &str, now: Instant) {
        if self.long_run_hint_count >= MAX_LONG_RUN_HINTS_PER_TURN {
            return;
        }
        let per_tool = self
            .long_run_hints_per_tool
            .get(tool_call_id)
            .copied()
            .unwrap_or(0);
        if per_tool >= MAX_LONG_RUN_HINTS_PER_TOOL {
            return;
        }
        if now.duration_since(self.phase_started) < Duration::from_secs(LONG_RUN_HINT_SECS) {
            return;
        }
        if let Some(last) = self.long_run_last_at.get(tool_call_id)
            && now.duration_since(*last) < Duration::from_secs(LONG_RUN_HINT_INTERVAL_SECS)
        {
            return;
        }
        let Some((_, name)) = self.generating_tool.as_ref() else {
            return;
        };
        let charm = self
            .long_run_charms
            .get(per_tool % self.long_run_charms.len())
            .map(String::as_str)
            .unwrap_or("still drafting");
        let detail = self
            .generating_preview
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or("tool args");
        let line = format!(
            "{charm} — model drafting {} · {detail}",
            name.replace('_', " ")
        );
        self.push_activity(line, ActivityTone::Warn);
        self.long_run_hints_per_tool
            .insert(tool_call_id.to_string(), per_tool + 1);
        self.long_run_last_at
            .insert(tool_call_id.to_string(), now);
        self.long_run_hint_count += 1;
    }
}

const MAX_RECENT_SUBAGENT_TOOLS: usize = 4;

fn push_recent_tool_name(recent: &mut Vec<String>, name: &str) {
    let short = name.replace('_', " ");
    if recent.last().is_some_and(|last| last == &short) {
        return;
    }
    recent.push(short);
    if recent.len() > MAX_RECENT_SUBAGENT_TOOLS {
        recent.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_progress_updates_detail_and_hint() {
        let mut state = TurnActivityState::new(true);
        state.on_tool_exec(
            "tc1".into(),
            "terminal".into(),
            r#"{"command":"cargo build"}"#.into(),
            "cargo build".into(),
            1,
        );
        state.on_tool_progress("tc1", "Compiling…".into(), 2, Instant::now());
        let row = state.tools.get("tc1").unwrap();
        assert_eq!(row.detail.as_deref(), Some("Compiling…"));
    }

    #[test]
    fn generating_preview_streams_partial_args() {
        let mut state = TurnActivityState::new(true);
        state.on_generating(
            "tc1".into(),
            "terminal".into(),
            r#"{"command":"cargo ""#.into(),
        );
        let first = state.generating_preview.clone().unwrap_or_default();
        state.on_generating(
            "tc1".into(),
            "terminal".into(),
            r#"{"command":"cargo build --workspace"}"#.into(),
        );
        let second = state.generating_preview.clone().unwrap_or_default();
        assert!(second.contains("cargo build"));
        assert!(second.chars().count() > first.chars().count());
        assert!(second.chars().count() <= crate::transcript_heights::LIVE_RENDER_MAX_CHARS + 1);
    }

    #[test]
    fn parallel_tools_tracked_independently() {
        let mut state = TurnActivityState::new(true);
        for i in 0..3 {
            let id = format!("t{i}");
            let name = ["file_read", "file_search", "terminal"][i];
            state.on_tool_exec(
                id,
                name.into(),
                "{}".into(),
                format!("{name} preview"),
                (i + 1) as u64,
            );
        }
        assert_eq!(state.sorted_active_tools().count(), 3);
        let summary = state.tool_summary().unwrap();
        assert_eq!(summary.active_count, 3);
        assert_eq!(summary.primary_name, "terminal");
    }

    #[test]
    fn generating_clears_on_tool_exec() {
        let mut state = TurnActivityState::new(true);
        state.on_generating("tc1".into(), "terminal".into(), "{}".into());
        assert_eq!(state.phase, ShelfPhase::GeneratingTool);
        state.on_tool_exec(
            "tc1".into(),
            "terminal".into(),
            "{}".into(),
            "cargo test".into(),
            1,
        );
        assert!(state.generating_tool.is_none());
        assert_eq!(state.phase, ShelfPhase::ToolExec);
    }

    #[test]
    fn subagent_recent_tools_dedupes_consecutive() {
        let mut state = TurnActivityState::new(true);
        state.on_subagent_start(0, 1, "audit".into(), 1, "sa-0".into(), None);
        state.on_subagent_tool(0, "file_read", "file_read  a.rs".into());
        state.on_subagent_tool(0, "file_read", "file_read  b.rs".into());
        state.on_subagent_tool(0, "terminal", "terminal  test".into());
        let row = state.subagents.get(&0).unwrap();
        assert_eq!(row.recent_tools, vec!["file read", "terminal"]);
    }

    #[test]
    fn reasoning_and_tool_tokens_accumulate() {
        let mut state = TurnActivityState::new(true);
        state.on_reasoning("Let me think about this problem carefully.");
        assert!(state.thinking_token_est > 0);
        state.on_tool_exec(
            "t1".into(),
            "file_write".into(),
            r#"{"path":"demo/index.html"}"#.into(),
            "demo/index.html".into(),
            1,
        );
        assert!(state.tool_token_acc > 0);
        state.reset_turn();
        assert_eq!(state.thinking_token_est, 0);
        assert_eq!(state.tool_token_acc, 0);
    }

    #[test]
    fn live_caption_during_tool_exec() {
        let mut state = TurnActivityState::new(true);
        state.on_tool_exec(
            "t1".into(),
            "file_write".into(),
            "{}".into(),
            "demo/index.html".into(),
            1,
        );
        let caption = state.live_caption().unwrap();
        assert!(caption.contains("file write"));
        assert!(caption.contains("demo/index.html"));
    }

    #[test]
    fn phase_line_tool_exec_matches_live_caption() {
        let mut state = TurnActivityState::new(true);
        state.on_tool_exec(
            "t1".into(),
            "terminal".into(),
            "{}".into(),
            "cargo build".into(),
            1,
        );
        assert_eq!(state.phase_line(), state.live_caption());
    }

    #[test]
    fn live_caption_preparing_tool() {
        let mut state = TurnActivityState::new(true);
        state.on_generating(
            "t1".into(),
            "file_write".into(),
            r#"{"path":"demo/"}"#.into(),
        );
        let caption = state.live_caption().unwrap();
        assert!(caption.contains("preparing"));
        assert!(caption.contains("file write"));
    }

    #[test]
    fn minimum_shelf_lines_during_active_tool() {
        let mut state = TurnActivityState::new(true);
        assert_eq!(state.minimum_shelf_lines(true), 0);
        state.on_tool_exec(
            "t1".into(),
            "file_write".into(),
            "{}".into(),
            "index.html".into(),
            1,
        );
        assert_eq!(state.minimum_shelf_lines(true), 1);
        assert_eq!(state.minimum_shelf_lines(false), 0);
    }

    #[test]
    fn tool_summary_parallel_format_input() {
        let mut state = TurnActivityState::new(true);
        state.on_tool_exec(
            "t1".into(),
            "terminal".into(),
            "{}".into(),
            "cargo build".into(),
            1,
        );
        state.on_tool_exec(
            "t2".into(),
            "read_file".into(),
            r#"{"path":"src/a.rs"}"#.into(),
            "src/a.rs".into(),
            2,
        );
        let summary = state.tool_summary().unwrap();
        assert_eq!(summary.active_count, 2);
        assert_eq!(summary.primary_name, "read_file");
    }
}
