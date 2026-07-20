//! Stream presentation helpers — TurnPhase + thinking/tool card lifecycle (024 W1–W4 · 026).
//!
//! SOLID: pure state; no ratatui. `turn_activity` owns shelf buffers; this module
//! owns phase transitions, card modes, verb-group kinds, session edit ledger,
//! tool-usage counters, and finished-card snapshots for the transcript.
//!
//! Law: model stream events are facts. Presentation is a state machine of cards.

// Public presentation API: fields/modes are part of the card contract (dispatch +
// harness + future shelf paint). Keep even when a given binary path has not
// read every accessor yet.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Unified turn phase (single owner for shelf + chrome).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TurnPhase {
    #[default]
    Idle,
    Thinking,
    Tool {
        id: String,
        name: String,
    },
    Responding,
}

/// Grok-style fold mode for thinking / tool / edit cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    Collapsed,
    #[default]
    Truncated,
    Expanded,
}

/// Tool presentation kind (drives running/done chrome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCardKind {
    Execute,
    Edit,
    Read,
    Search,
    Other,
}

impl ToolCardKind {
    pub fn classify(tool_name: &str) -> Self {
        match tool_name {
            "terminal" | "execute_code" | "run_process" | "browser_snapshot"
            | "browser_navigate" | "browser_vision" => Self::Execute,
            "write_file" | "patch" | "apply_patch" => Self::Edit,
            "read_file" | "file_read" => Self::Read,
            "search_files" | "file_search" | "web_search" | "web_extract" => Self::Search,
            _ => Self::Other,
        }
    }

    /// Prefer a durable transcript card while running (even with activity shelf).
    pub fn prefers_live_transcript_card(self) -> bool {
        matches!(self, Self::Execute | Self::Edit | Self::Read | Self::Search)
    }

    pub fn is_execute(self) -> bool {
        matches!(self, Self::Execute)
    }

    pub fn is_edit(self) -> bool {
        matches!(self, Self::Edit)
    }
}

/// Verb-group category for consecutive collapsed successes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbGroupKind {
    Read,
    Search,
    Edit,
}

impl VerbGroupKind {
    pub fn from_tool(tool_name: &str) -> Option<Self> {
        match ToolCardKind::classify(tool_name) {
            ToolCardKind::Read => Some(Self::Read),
            ToolCardKind::Search => Some(Self::Search),
            ToolCardKind::Edit => Some(Self::Edit),
            _ => None,
        }
    }

    pub fn running_label(self, n: usize) -> String {
        let n = n.max(1);
        match self {
            Self::Read => {
                if n == 1 {
                    "Reading 1 file".into()
                } else {
                    format!("Reading {n} files")
                }
            }
            Self::Search => {
                if n == 1 {
                    "Searching 1 pattern".into()
                } else {
                    format!("Searching {n} patterns")
                }
            }
            Self::Edit => {
                if n == 1 {
                    "Editing 1 file".into()
                } else {
                    format!("Editing {n} files")
                }
            }
        }
    }

    pub fn done_label(self, n: usize) -> String {
        let n = n.max(1);
        match self {
            Self::Read => {
                if n == 1 {
                    "Read 1 file".into()
                } else {
                    format!("Read {n} files")
                }
            }
            Self::Search => {
                if n == 1 {
                    "Searched 1 pattern".into()
                } else {
                    format!("Searched {n} patterns")
                }
            }
            Self::Edit => {
                if n == 1 {
                    "Edited 1 file".into()
                } else {
                    format!("Edited {n} files")
                }
            }
        }
    }
}

/// Accumulator for consecutive Read / Search successes (Edit uses `EditVerbGroup`).
#[derive(Debug, Clone)]
pub struct ActionVerbGroup {
    pub kind: VerbGroupKind,
    pub items: Vec<String>,
    pub header_line_idx: Option<usize>,
}

impl ActionVerbGroup {
    pub fn new(kind: VerbGroupKind, item: String) -> Self {
        Self {
            kind,
            items: if item.is_empty() {
                Vec::new()
            } else {
                vec![item]
            },
            header_line_idx: None,
        }
    }

    pub fn push_item(&mut self, item: &str) {
        if item.is_empty() {
            return;
        }
        if !self.items.iter().any(|i| i == item) {
            self.items.push(item.to_string());
        }
    }

    pub fn count(&self) -> usize {
        self.items.len().max(1)
    }

    pub fn header_label(&self, running: bool) -> String {
        if running {
            self.kind.running_label(self.count())
        } else {
            self.kind.done_label(self.count())
        }
    }

    pub fn expandable_list(&self) -> String {
        let mut lines = vec![self.header_label(false)];
        for (i, item) in self.items.iter().enumerate() {
            lines.push(format!("  {}. {item}", i + 1));
        }
        lines.join("\n")
    }
}

/// Session/turn tool category counts (`Exec N · Read N · Edit N · Search N`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolUsageCounters {
    pub exec: u32,
    pub read: u32,
    pub edit: u32,
    pub search: u32,
    pub other: u32,
}

impl ToolUsageCounters {
    pub fn record(&mut self, kind: ToolCardKind) {
        match kind {
            ToolCardKind::Execute => self.exec = self.exec.saturating_add(1),
            ToolCardKind::Read => self.read = self.read.saturating_add(1),
            ToolCardKind::Edit => self.edit = self.edit.saturating_add(1),
            ToolCardKind::Search => self.search = self.search.saturating_add(1),
            ToolCardKind::Other => self.other = self.other.saturating_add(1),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.exec == 0 && self.read == 0 && self.edit == 0 && self.search == 0 && self.other == 0
    }

    /// Compact strip: `Exec 2 · Read 5 · Edit 3 · Search 1`
    pub fn caption(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        if self.exec > 0 {
            parts.push(format!("Exec {}", self.exec));
        }
        if self.read > 0 {
            parts.push(format!("Read {}", self.read));
        }
        if self.edit > 0 {
            parts.push(format!("Edit {}", self.edit));
        }
        if self.search > 0 {
            parts.push(format!("Search {}", self.search));
        }
        if self.other > 0 {
            parts.push(format!("Other {}", self.other));
        }
        Some(parts.join(" · "))
    }
}

/// Session-scoped edit ledger (`files N  +X −Y`).
#[derive(Debug, Clone, Default)]
pub struct SessionEditLedger {
    files: HashSet<String>,
    pub plus: usize,
    pub minus: usize,
}

impl SessionEditLedger {
    pub fn record(&mut self, path: &str, plus: usize, minus: usize) {
        if !path.is_empty() {
            self.files.insert(path.to_string());
        }
        self.plus = self.plus.saturating_add(plus);
        self.minus = self.minus.saturating_add(minus);
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.plus == 0 && self.minus == 0
    }

    /// Shelf / status strip: `files 3  +42 −7`
    pub fn caption(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let n = self.file_count().max(1);
        Some(format!("files {n}  +{} −{}", self.plus, self.minus))
    }
}

/// Finished thinking card for the transcript (Grok Truncated → Collapsed summary).
#[derive(Debug, Clone)]
pub struct FinishedThinkingCard {
    pub text: String,
    pub duration: Duration,
    pub mode: DisplayMode,
}

impl FinishedThinkingCard {
    /// One-line collapsed header.
    pub fn collapsed_label(&self) -> String {
        let secs = self.duration.as_secs_f32();
        if secs < 0.05 {
            "Thought".into()
        } else if secs < 10.0 {
            format!("Thought · {secs:.1}s")
        } else {
            format!("Thought · {:.0}s", secs)
        }
    }

    /// Truncated body: last `max_lines` non-empty lines.
    pub fn truncated_body(&self, max_lines: usize) -> String {
        last_n_lines(&self.text, max_lines)
    }
}

/// Rolling thinking session (accumulates SuperGrok / reasoning stream).
#[derive(Debug, Clone, Default)]
pub struct ThinkingSession {
    pub buffer: String,
    pub started: Option<Instant>,
    pub mode: DisplayMode,
}

impl ThinkingSession {
    pub fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.started.is_none() {
            self.started = Some(Instant::now());
            self.mode = DisplayMode::Truncated;
        }
        self.buffer.push_str(text);
    }

    pub fn is_active(&self) -> bool {
        !self.buffer.trim().is_empty()
    }

    /// Shelf tail snippet (last chars, not first).
    pub fn shelf_snippet(&self, max_chars: usize) -> Option<String> {
        let trimmed = self.buffer.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(tail_chars(trimmed, max_chars))
    }

    /// Last N lines for Truncated shelf / live transcript.
    pub fn truncated_lines(&self, max_lines: usize) -> Option<String> {
        let trimmed = self.buffer.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(last_n_lines(trimmed, max_lines))
    }

    /// Finish session → card; clears buffer. Empty → None (no fake Thought card).
    pub fn take_finished(&mut self) -> Option<FinishedThinkingCard> {
        let text = std::mem::take(&mut self.buffer);
        let started = self.started.take();
        self.mode = DisplayMode::Collapsed;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        let duration = started
            .map(|s| s.elapsed())
            .unwrap_or_else(|| Duration::from_millis(0));
        Some(FinishedThinkingCard {
            text: trimmed.to_string(),
            duration,
            mode: DisplayMode::Collapsed,
        })
    }
}

/// Live tool body buffer (progress / stdout tail).
#[derive(Debug, Clone)]
pub struct ToolBodyBuffer {
    pub tool_call_id: String,
    pub name: String,
    pub kind: ToolCardKind,
    pub body: String,
    pub started: Instant,
    pub mode: DisplayMode,
}

impl ToolBodyBuffer {
    pub fn new(tool_call_id: String, name: String) -> Self {
        let kind = ToolCardKind::classify(&name);
        let mode = match kind {
            ToolCardKind::Execute => DisplayMode::Truncated,
            ToolCardKind::Edit => DisplayMode::Truncated,
            _ => DisplayMode::Collapsed,
        };
        Self {
            tool_call_id,
            name,
            kind,
            body: String::new(),
            started: Instant::now(),
            mode,
        }
    }

    pub fn append(&mut self, chunk: &str, max_bytes: usize) {
        if chunk.is_empty() {
            return;
        }
        self.body.push_str(chunk);
        if !chunk.ends_with('\n') {
            self.body.push('\n');
        }
        if self.body.len() > max_bytes {
            let keep = max_bytes * 3 / 4;
            let start = self
                .body
                .char_indices()
                .rev()
                .find(|(i, _)| *i <= self.body.len().saturating_sub(keep))
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.body = format!("…(truncated)\n{}", &self.body[start..]);
        }
    }

    pub fn tail_lines(&self, max_lines: usize) -> String {
        last_n_lines(self.body.trim_end(), max_lines)
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

const DEFAULT_TOOL_BODY_MAX: usize = 8_192;
const DEFAULT_SHELF_THINK_CHARS: usize = 240;
const DEFAULT_THINK_TRUNC_LINES: usize = 6;
const DEFAULT_TOOL_TRUNC_LINES: usize = 3;

/// Presentation state for one agent turn (testable without App).
#[derive(Debug, Clone)]
pub struct StreamPresentation {
    pub phase: TurnPhase,
    pub thinking: ThinkingSession,
    pub tools: HashMap<String, ToolBodyBuffer>,
    pub shelf_think_chars: usize,
    pub tool_body_max: usize,
    pub think_trunc_lines: usize,
    pub tool_trunc_lines: usize,
    /// Wall-clock start of the current agent turn (for “Worked for”).
    pub turn_started: Option<Instant>,
    /// Tools completed this turn (for Worked-for optional count).
    pub tools_completed: u32,
    /// Session-scoped edit ledger (survives turn reset until explicit clear).
    pub edit_ledger: SessionEditLedger,
    /// Session-scoped tool category usage (survives turn reset until clear).
    pub tool_usage: ToolUsageCounters,
    /// Per-turn tool category usage (reset with turn).
    pub turn_tool_usage: ToolUsageCounters,
}

impl Default for StreamPresentation {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamPresentation {
    pub fn new() -> Self {
        Self {
            phase: TurnPhase::Idle,
            thinking: ThinkingSession::default(),
            tools: HashMap::new(),
            shelf_think_chars: DEFAULT_SHELF_THINK_CHARS,
            tool_body_max: DEFAULT_TOOL_BODY_MAX,
            think_trunc_lines: DEFAULT_THINK_TRUNC_LINES,
            tool_trunc_lines: DEFAULT_TOOL_TRUNC_LINES,
            turn_started: None,
            tools_completed: 0,
            edit_ledger: SessionEditLedger::default(),
            tool_usage: ToolUsageCounters::default(),
            turn_tool_usage: ToolUsageCounters::default(),
        }
    }

    fn ensure_turn_started(&mut self) {
        if self.turn_started.is_none() {
            self.turn_started = Some(Instant::now());
        }
    }

    pub fn on_reasoning(&mut self, text: &str) {
        self.ensure_turn_started();
        self.thinking.push(text);
        self.phase = TurnPhase::Thinking;
    }

    /// First answer token or tool work ends thinking.
    pub fn on_responding_or_tool(&mut self) -> Option<FinishedThinkingCard> {
        self.thinking.take_finished()
    }

    pub fn on_tool_exec(
        &mut self,
        tool_call_id: String,
        name: String,
    ) -> Option<FinishedThinkingCard> {
        self.ensure_turn_started();
        let finished = self.thinking.take_finished();
        self.tools.insert(
            tool_call_id.clone(),
            ToolBodyBuffer::new(tool_call_id.clone(), name.clone()),
        );
        self.phase = TurnPhase::Tool {
            id: tool_call_id,
            name,
        };
        finished
    }

    pub fn on_tool_progress(&mut self, tool_call_id: &str, message: &str) {
        if let Some(buf) = self.tools.get_mut(tool_call_id) {
            buf.append(message, self.tool_body_max);
        }
    }

    pub fn on_tool_done(&mut self, tool_call_id: &str) -> Option<ToolBodyBuffer> {
        let buf = self.tools.remove(tool_call_id);
        if let Some(ref b) = buf {
            self.tools_completed = self.tools_completed.saturating_add(1);
            self.tool_usage.record(b.kind);
            self.turn_tool_usage.record(b.kind);
        }
        if self.tools.is_empty() {
            // Between tools: idle until next token / tool (maps to AwaitingFirstToken).
            self.phase = TurnPhase::Idle;
        } else if let Some((id, remaining)) = self.tools.iter().next() {
            self.phase = TurnPhase::Tool {
                id: id.clone(),
                name: remaining.name.clone(),
            };
        }
        buf
    }

    pub fn on_token(&mut self) -> Option<FinishedThinkingCard> {
        self.ensure_turn_started();
        let card = self.thinking.take_finished();
        self.phase = TurnPhase::Responding;
        card
    }

    pub fn record_edit(&mut self, path: &str, plus: usize, minus: usize) {
        self.edit_ledger.record(path, plus, minus);
    }

    /// Turn footer: `Worked for 2m36s` (None if turn never started or <50ms).
    pub fn worked_for_label(&self) -> Option<String> {
        let started = self.turn_started?;
        let elapsed = started.elapsed();
        if elapsed.as_millis() < 50 {
            return None;
        }
        Some(format!("Worked for {}", format_elapsed_compact(elapsed)))
    }

    /// Reset per-turn state; keep session edit ledger + session tool usage.
    pub fn reset_turn(&mut self) {
        let ledger = std::mem::take(&mut self.edit_ledger);
        let usage = std::mem::take(&mut self.tool_usage);
        *self = Self::new();
        self.edit_ledger = ledger;
        self.tool_usage = usage;
    }

    /// Clear session ledger (e.g. `/new` session).
    pub fn clear_session_ledger(&mut self) {
        self.edit_ledger = SessionEditLedger::default();
        self.tool_usage = ToolUsageCounters::default();
    }

    /// Prefer turn strip while a turn is active; else session strip.
    pub fn tool_usage_caption(&self) -> Option<String> {
        if !self.turn_tool_usage.is_empty() {
            self.turn_tool_usage.caption()
        } else {
            self.tool_usage.caption()
        }
    }

    pub fn shelf_thinking_snippet(&self) -> Option<String> {
        self.thinking.shelf_snippet(self.shelf_think_chars)
    }

    pub fn shelf_thinking_truncated(&self) -> Option<String> {
        self.thinking.truncated_lines(self.think_trunc_lines)
    }

    /// Full accumulated thinking (sole buffer — DRY).
    pub fn thinking_full(&self) -> &str {
        self.thinking.buffer.as_str()
    }

    /// Live transcript Truncated card: header + last N lines (Grok scrollback).
    pub fn live_thinking_transcript_text(&self) -> Option<String> {
        let trunc = self.thinking.truncated_lines(self.think_trunc_lines)?;
        Some(format!("Thinking…\n{trunc}"))
    }

    /// How many body lines the shelf should paint for Truncated thinking.
    pub fn shelf_thinking_body_line_budget(&self, section_full: bool) -> usize {
        if section_full {
            self.think_trunc_lines
        } else {
            // Collapsed / summary: one peek line (plus header painted separately).
            1
        }
    }

    pub fn tool_body_tail(&self, tool_call_id: &str) -> Option<String> {
        self.tools
            .get(tool_call_id)
            .map(|b| b.tail_lines(self.tool_trunc_lines))
            .filter(|s| !s.is_empty())
    }
}

/// Map presentation phase → shelf phase label enum (no dual invention of phase).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromePhaseHint {
    Idle,
    AwaitingFirstToken,
    Thinking,
    GeneratingTool,
    ToolExec,
    Streaming,
}

impl StreamPresentation {
    /// Single mapping: TurnPhase (+ generating flag) → chrome hint for shelf/status.
    pub fn chrome_phase_hint(&self, tool_generating: bool) -> ChromePhaseHint {
        if tool_generating {
            return ChromePhaseHint::GeneratingTool;
        }
        match &self.phase {
            TurnPhase::Idle => {
                if self.turn_started.is_some() {
                    ChromePhaseHint::AwaitingFirstToken
                } else {
                    ChromePhaseHint::Idle
                }
            }
            TurnPhase::Thinking => ChromePhaseHint::Thinking,
            TurnPhase::Tool { .. } => ChromePhaseHint::ToolExec,
            TurnPhase::Responding => ChromePhaseHint::Streaming,
        }
    }

    /// Shared phase label for shelf title + status chrome (026 A5).
    pub fn phase_activity_label(&self, tool_generating: bool) -> String {
        phase_activity_label(self.chrome_phase_hint(tool_generating), self.active_tool_name())
    }

    pub fn active_tool_name(&self) -> Option<&str> {
        match &self.phase {
            TurnPhase::Tool { name, .. } => Some(name.as_str()),
            _ => None,
        }
    }

    /// True when chrome should show a turn-status row (non-idle turn).
    pub fn turn_status_visible(&self) -> bool {
        !matches!(
            self.chrome_phase_hint(false),
            ChromePhaseHint::Idle
        ) || !self.tools.is_empty()
            || self.thinking.is_active()
    }
}

/// Single source for human-readable phase sentences (shelf + turn-status + status bar).
pub fn phase_activity_label(hint: ChromePhaseHint, tool_name: Option<&str>) -> String {
    match hint {
        ChromePhaseHint::Idle => "Idle".into(),
        ChromePhaseHint::AwaitingFirstToken => "Waiting for response…".into(),
        ChromePhaseHint::Thinking => "Thinking…".into(),
        ChromePhaseHint::GeneratingTool => {
            if let Some(name) = tool_name {
                format!("Preparing {}…", name.replace('_', " "))
            } else {
                "Preparing tool…".into()
            }
        }
        ChromePhaseHint::ToolExec => {
            if let Some(name) = tool_name {
                format!("Running {}…", name.replace('_', " "))
            } else {
                "Running tool…".into()
            }
        }
        ChromePhaseHint::Streaming => "Responding…".into(),
    }
}

pub fn format_elapsed_compact(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 {
            format!("{m}m")
        } else {
            format!("{m}m{s}s")
        }
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{h}h{m}m")
    }
}

fn last_n_lines(s: &str, max_lines: usize) -> String {
    if max_lines == 0 {
        return String::new();
    }
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() <= max_lines {
        return lines.join("\n");
    }
    let start = lines.len() - max_lines;
    format!("…\n{}", lines[start..].join("\n"))
}

fn tail_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut rev: String = s.chars().rev().take(max).collect();
    rev = rev.chars().rev().collect();
    format!("…{rev}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_accumulates_and_tails_snippet() {
        let mut p = StreamPresentation::new();
        p.on_reasoning("Hello ");
        p.on_reasoning("world, this is a longer reasoning stream for SuperGrok.");
        assert!(matches!(p.phase, TurnPhase::Thinking));
        assert_eq!(p.chrome_phase_hint(false), ChromePhaseHint::Thinking);
        let snip = p.shelf_thinking_snippet().expect("snippet");
        assert!(snip.contains("SuperGrok") || snip.starts_with('…'));
        let card = p.on_token().expect("finished");
        assert!(card.text.contains("Hello"));
        assert!(card.collapsed_label().starts_with("Thought"));
        assert_eq!(card.mode, DisplayMode::Collapsed);
        assert!(!p.thinking.is_active());
        assert_eq!(p.chrome_phase_hint(false), ChromePhaseHint::Streaming);
    }

    #[test]
    fn empty_thinking_produces_no_card() {
        let mut p = StreamPresentation::new();
        assert!(p.on_token().is_none());
    }

    #[test]
    fn tool_exec_finalizes_thinking() {
        let mut p = StreamPresentation::new();
        p.on_reasoning("plan the edit");
        let card = p
            .on_tool_exec("tc1".into(), "write_file".into())
            .expect("thinking card");
        assert!(card.text.contains("plan"));
        assert!(matches!(p.phase, TurnPhase::Tool { .. }));
        assert_eq!(ToolCardKind::classify("write_file"), ToolCardKind::Edit);
        p.on_tool_progress("tc1", "line one");
        p.on_tool_progress("tc1", "line two");
        let body = p.on_tool_done("tc1").expect("body");
        assert!(body.body.contains("line one"));
        assert!(body.body.contains("line two"));
        assert_eq!(p.tools_completed, 1);
        assert_eq!(p.tool_usage.edit, 1);
        assert_eq!(
            p.tool_usage_caption().as_deref(),
            Some("Edit 1")
        );
        assert_eq!(
            p.chrome_phase_hint(false),
            ChromePhaseHint::AwaitingFirstToken
        );
        assert!(p.phase_activity_label(false).contains("Waiting"));
    }

    #[test]
    fn tool_usage_strip_aggregates_kinds() {
        let mut p = StreamPresentation::new();
        let _ = p.on_tool_exec("a".into(), "read_file".into());
        let _ = p.on_tool_done("a");
        let _ = p.on_tool_exec("b".into(), "read_file".into());
        let _ = p.on_tool_done("b");
        let _ = p.on_tool_exec("c".into(), "terminal".into());
        let _ = p.on_tool_done("c");
        assert_eq!(p.tool_usage.read, 2);
        assert_eq!(p.tool_usage.exec, 1);
        let cap = p.tool_usage_caption().expect("caption");
        assert!(cap.contains("Exec 1"));
        assert!(cap.contains("Read 2"));
    }

    #[test]
    fn tool_body_rolls_at_cap() {
        let mut buf = ToolBodyBuffer::new("t".into(), "terminal".into());
        assert_eq!(buf.tool_call_id, "t");
        assert_eq!(buf.mode, DisplayMode::Truncated);
        assert!(buf.elapsed().as_secs() < 60);
        let chunk = "x".repeat(100);
        for _ in 0..200 {
            buf.append(&chunk, 500);
        }
        assert!(buf.body.len() <= 600);
        assert!(buf.body.contains("truncated") || buf.body.len() <= 500);
        assert!(buf.kind.is_execute());
    }

    #[test]
    fn thinking_truncated_last_n_lines() {
        let mut p = StreamPresentation::new();
        p.think_trunc_lines = 2;
        p.on_reasoning("line1\nline2\nline3\nline4\n");
        let t = p.shelf_thinking_truncated().expect("trunc");
        assert!(t.contains("line4"));
        assert!(!t.contains("line1") || t.starts_with('…'));
        let live = p.live_thinking_transcript_text().expect("live");
        assert!(live.starts_with("Thinking…"));
        assert!(live.contains("line4"));
        assert!(!live.contains("line1") || live.contains('…'));
    }

    #[test]
    fn live_thinking_transcript_rolls_window() {
        let mut p = StreamPresentation::new();
        p.think_trunc_lines = 3;
        for i in 1..=10 {
            p.on_reasoning(&format!("step {i}\n"));
        }
        let live = p.live_thinking_transcript_text().expect("live");
        assert!(live.contains("step 10"));
        assert!(live.contains("step 8"));
        assert!(!live.contains("step 7"));
        assert_eq!(
            p.thinking_full()
                .lines()
                .filter(|l| l.starts_with("step"))
                .count(),
            10
        );
    }

    #[test]
    fn session_edit_ledger_and_worked_for() {
        let mut p = StreamPresentation::new();
        p.on_reasoning("x");
        p.record_edit("a.rs", 3, 1);
        p.record_edit("b.rs", 2, 0);
        assert_eq!(p.edit_ledger.file_count(), 2);
        assert_eq!(p.edit_ledger.caption().as_deref(), Some("files 2  +5 −1"));
        // Force elapsed for worked_for
        p.turn_started = Some(Instant::now() - Duration::from_secs(96));
        let label = p.worked_for_label().expect("worked");
        assert!(label.starts_with("Worked for"));
        assert!(label.contains('m') || label.contains('s'));
        p.reset_turn();
        assert!(!p.edit_ledger.is_empty(), "ledger survives turn reset");
        p.clear_session_ledger();
        assert!(p.edit_ledger.is_empty());
    }

    #[test]
    fn verb_group_labels() {
        assert_eq!(VerbGroupKind::Read.done_label(3), "Read 3 files");
        assert_eq!(
            VerbGroupKind::Search.running_label(1),
            "Searching 1 pattern"
        );
        let mut g = ActionVerbGroup::new(VerbGroupKind::Read, "a.rs".into());
        g.push_item("b.rs");
        assert_eq!(g.header_label(false), "Read 2 files");
    }

    #[test]
    fn classify_tool_kinds() {
        assert_eq!(ToolCardKind::classify("terminal"), ToolCardKind::Execute);
        assert_eq!(ToolCardKind::classify("patch"), ToolCardKind::Edit);
        assert_eq!(ToolCardKind::classify("read_file"), ToolCardKind::Read);
        assert_eq!(ToolCardKind::classify("search_files"), ToolCardKind::Search);
        assert!(ToolCardKind::Edit.prefers_live_transcript_card());
    }

    #[test]
    fn format_elapsed_compact_units() {
        assert_eq!(format_elapsed_compact(Duration::from_secs(12)), "12s");
        assert_eq!(format_elapsed_compact(Duration::from_secs(90)), "1m30s");
        assert_eq!(format_elapsed_compact(Duration::from_secs(120)), "2m");
    }
}
