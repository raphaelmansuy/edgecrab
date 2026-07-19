//! Evidence latch graph — session-scoped, deterministic (019 + 022 Heal deadlock fix).
//!
//! Single owner for: Artifact → Preview(content-qualified) → Perceive latches,
//! Heal phase (exclusive re-serve), thrash fingerprints, create/verify budgets.
//!
//! **Law (July 2026):** every phase has a non-empty allowed action set until
//! LatchedDone or Escalated. Never instruct a blocked command.
//!
//! SOLID: pure state machine — no CDP/network I/O. Conversation records facts;
//! assessor and pre_dispatch read latches.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use edgecrab_tools::ContentClass;
use sha2::{Digest, Sha256};

/// Tool-phase for budget accounting and allowed-action policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvidencePhase {
    #[default]
    Create,
    /// Artifact present; waiting for content-qualified preview + perceive.
    Verify,
    /// Exclusive one-shot re-serve after content-failed latch (022 deadlock break).
    Heal,
    /// Visual (or media) evidence complete — stop thrash.
    LatchedDone,
    /// Closed action set — hard stop; no reopen.
    Escalated,
}

/// Latched preview server (content-qualified only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewLatch {
    pub dir: PathBuf,
    pub port: u16,
    pub url: String,
    pub process_id: Option<String>,
    pub bind_ready: bool,
    pub http_status: Option<u16>,
    /// Hash of tracked artifact paths when latched (dirty if mutations change set).
    pub artifact_fingerprint: u64,
    /// True only after ContentClass::Ok perception or explicit content probe.
    pub content_qualified: bool,
}

/// Latched perception evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerceiveLatch {
    pub url: String,
    pub content_class: ContentClass,
    pub evidence_tool: String,
}

/// Config knobs (mirrors harness config; pure defaults for tests).
#[derive(Debug, Clone, Copy)]
pub struct EvidenceLatchConfig {
    pub enabled: bool,
    pub verify_tool_budget: u32,
    pub thrash_fingerprint_limit: u32,
    /// Max exclusive Heal re-serves per session dirty cycle (022).
    pub heal_budget: u32,
    /// Browser tools allowed after Perceive Ok (default 0 — hard stop thrash).
    pub post_perceive_browser_budget: u32,
}

impl Default for EvidenceLatchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            verify_tool_budget: 12,
            thrash_fingerprint_limit: 3,
            heal_budget: 1,
            post_perceive_browser_budget: 0,
        }
    }
}

/// Copy-friendly snapshot for completion assess (no lifetimes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EvidenceAssessSnapshot {
    pub visual_complete: bool,
    pub media_complete: bool,
    pub escalated: bool,
    pub verify_budget_exhausted: bool,
    pub in_heal: bool,
    pub latched_done: bool,
}

/// Session evidence state machine.
#[derive(Debug, Clone)]
pub struct EvidenceState {
    pub config: EvidenceLatchConfig,
    pub phase: EvidencePhase,
    pub artifact: bool,
    /// Content-qualified preview only (blocks re-serve when set).
    pub preview: Option<PreviewLatch>,
    /// Bind candidate from terminal (does **not** block re-serve until qualified).
    pub preview_candidate: Option<PreviewLatch>,
    pub perceive: Option<PerceiveLatch>,
    pub oracle_ok: bool,
    pub thrash_blocked: bool,
    thrash_counts: HashMap<String, u32>,
    pub create_tools: u32,
    pub verify_tools: u32,
    /// Remaining exclusive Heal serves (starts at config.heal_budget).
    pub heal_remaining: u32,
    /// Browser tools used after LatchedDone (should stay 0).
    pub post_perceive_tools: u32,
    artifact_paths: Vec<String>,
    pub last_advisory_class: Option<String>,
    /// Last heal target port/dir for allowed-action messages.
    pub last_heal_port: Option<u16>,
    pub last_heal_dir: Option<PathBuf>,
}

impl Default for EvidenceState {
    fn default() -> Self {
        Self::new(EvidenceLatchConfig::default())
    }
}

impl EvidenceState {
    pub fn new(config: EvidenceLatchConfig) -> Self {
        let heal_remaining = config.heal_budget;
        Self {
            config,
            phase: EvidencePhase::Create,
            artifact: false,
            preview: None,
            preview_candidate: None,
            perceive: None,
            oracle_ok: false,
            thrash_blocked: false,
            thrash_counts: HashMap::new(),
            create_tools: 0,
            verify_tools: 0,
            heal_remaining,
            post_perceive_tools: 0,
            artifact_paths: Vec::new(),
            last_advisory_class: None,
            last_heal_port: None,
            last_heal_dir: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn assess_snapshot(&self) -> EvidenceAssessSnapshot {
        if !self.config.enabled {
            return EvidenceAssessSnapshot::default();
        }
        EvidenceAssessSnapshot {
            visual_complete: self.visual_evidence_complete(),
            media_complete: self.media_evidence_complete(),
            escalated: self.is_escalated(),
            verify_budget_exhausted: self.verify_budget_exhausted(),
            in_heal: matches!(self.phase, EvidencePhase::Heal),
            latched_done: matches!(self.phase, EvidencePhase::LatchedDone),
        }
    }

    /// Hard stop: escalated or verify budget exhausted after artifact.
    pub fn should_hard_stop(&self) -> bool {
        self.config.enabled && (self.is_escalated() || self.verify_budget_exhausted())
    }

    pub fn is_escalated(&self) -> bool {
        self.config.enabled && matches!(self.phase, EvidencePhase::Escalated)
    }

    /// Record a successful file mutation path.
    pub fn note_artifact_path(&mut self, path: &str) {
        if !self.config.enabled {
            return;
        }
        let path = path.replace('\\', "/");
        if !self.artifact_paths.iter().any(|p| p == &path) {
            self.artifact_paths.push(path);
        }
        self.artifact = !self.artifact_paths.is_empty();
        self.dirty_if_preview_paths_changed();
    }

    /// Mark verify phase (e.g. after artifact exists and task is visual).
    pub fn enter_verify_phase(&mut self) {
        if self.artifact && !matches!(self.phase, EvidencePhase::Escalated | EvidencePhase::LatchedDone)
        {
            self.phase = EvidencePhase::Verify;
        }
    }

    /// Count a tool toward create or verify budget.
    pub fn count_tool(&mut self, tool_name: &str, is_mutation: bool) {
        if !self.config.enabled {
            return;
        }
        if is_mutation || matches!(self.phase, EvidencePhase::Create) && !self.artifact {
            self.create_tools = self.create_tools.saturating_add(1);
        } else if matches!(
            self.phase,
            EvidencePhase::Verify | EvidencePhase::Heal | EvidencePhase::Escalated
        ) || self.artifact
        {
            if is_verify_tool(tool_name)
                || self.preview.is_some()
                || self.preview_candidate.is_some()
            {
                self.verify_tools = self.verify_tools.saturating_add(1);
                if matches!(self.phase, EvidencePhase::Create) {
                    self.phase = EvidencePhase::Verify;
                }
            }
        }
        if matches!(self.phase, EvidencePhase::LatchedDone) && is_browser_tool(tool_name) {
            self.post_perceive_tools = self.post_perceive_tools.saturating_add(1);
        }
    }

    pub fn verify_budget_exhausted(&self) -> bool {
        self.config.enabled
            && self.artifact
            && self.verify_tools >= self.config.verify_tool_budget
    }

    /// Bind candidate only — does **not** content-qualify or block re-serve (022 A1).
    pub fn note_preview_candidate(
        &mut self,
        dir: PathBuf,
        port: u16,
        url: String,
        process_id: Option<String>,
        http_status: Option<u16>,
    ) {
        if !self.config.enabled {
            return;
        }
        let fp = fingerprint_paths(&self.artifact_paths);
        let latch = PreviewLatch {
            dir: dir.clone(),
            port,
            url: url.clone(),
            process_id,
            bind_ready: true,
            http_status,
            artifact_fingerprint: fp,
            content_qualified: false,
        };
        self.preview_candidate = Some(latch);
        self.last_heal_port = Some(port);
        self.last_heal_dir = Some(dir);
        if matches!(self.phase, EvidencePhase::Heal) {
            self.consume_heal_serve();
        } else if matches!(self.phase, EvidencePhase::Create) {
            self.phase = EvidencePhase::Verify;
        }
    }

    /// Content-qualified preview latch (only after Ok perception or verified probe).
    pub fn latch_preview(
        &mut self,
        dir: PathBuf,
        port: u16,
        url: String,
        process_id: Option<String>,
        http_status: Option<u16>,
    ) {
        self.latch_preview_qualified(dir, port, url, process_id, http_status, true);
    }

    /// Latch with explicit content qualification flag.
    ///
    /// `content_qualified=false` only records a candidate (no re-serve block).
    pub fn latch_preview_qualified(
        &mut self,
        dir: PathBuf,
        port: u16,
        url: String,
        process_id: Option<String>,
        http_status: Option<u16>,
        content_qualified: bool,
    ) {
        if !self.config.enabled {
            return;
        }
        if !content_qualified {
            self.note_preview_candidate(dir, port, url, process_id, http_status);
            return;
        }
        let fp = fingerprint_paths(&self.artifact_paths);
        self.preview = Some(PreviewLatch {
            dir: dir.clone(),
            port,
            url,
            process_id,
            bind_ready: true,
            http_status,
            artifact_fingerprint: fp,
            content_qualified: true,
        });
        self.preview_candidate = None;
        self.last_heal_port = Some(port);
        self.last_heal_dir = Some(dir);
        if !matches!(
            self.phase,
            EvidencePhase::LatchedDone | EvidencePhase::Escalated
        ) {
            self.phase = EvidencePhase::Verify;
        }
    }

    /// Whether a new preview serve is forbidden.
    ///
    /// Heal phase with remaining budget: **never** blocked (022 A3).
    /// Only **content-qualified** preview blocks re-serve.
    pub fn blocks_preview_serve(&self, _port: u16, _dir: &Path) -> bool {
        if !self.config.enabled {
            return false;
        }
        if matches!(self.phase, EvidencePhase::Heal) && self.heal_remaining > 0 {
            return false;
        }
        if matches!(self.phase, EvidencePhase::Escalated) {
            return true;
        }
        // Content-qualified latch only.
        self.preview
            .as_ref()
            .is_some_and(|p| p.content_qualified)
    }

    /// Block browser tools when thrash Escalated or post LatchedDone budget exhausted.
    pub fn blocks_browser_tools(&self) -> bool {
        if !self.config.enabled {
            return false;
        }
        if matches!(self.phase, EvidencePhase::Heal) {
            // Allow navigate/snapshot after heal serve (phase still Heal until consume).
            return false;
        }
        if matches!(self.phase, EvidencePhase::LatchedDone) {
            return self.post_perceive_tools >= self.config.post_perceive_browser_budget;
        }
        if matches!(self.phase, EvidencePhase::Escalated) {
            return true;
        }
        self.thrash_blocked
    }

    /// Force-allow terminal command in Heal for exclusive recipe only.
    ///
    /// Returns `Some(true)` allow, `Some(false)` block, `None` no opinion.
    pub fn terminal_heal_policy(&self, command: &str) -> Option<bool> {
        if !self.config.enabled {
            return None;
        }
        if matches!(self.phase, EvidencePhase::Escalated | EvidencePhase::LatchedDone) {
            if edgecrab_tools::dev_server::is_preview_server_command(command) {
                return Some(false);
            }
            return None;
        }
        if !matches!(self.phase, EvidencePhase::Heal) || self.heal_remaining == 0 {
            return None;
        }
        if edgecrab_tools::dev_server::is_preview_server_command(command) {
            return Some(true);
        }
        if self.is_heal_port_aux_command(command) {
            return Some(true);
        }
        // Exclusive Heal: block other shell while healing.
        Some(false)
    }

    fn is_heal_port_aux_command(&self, command: &str) -> bool {
        let port = self
            .last_heal_port
            .or_else(|| self.preview_candidate.as_ref().map(|p| p.port))
            .or_else(|| self.preview.as_ref().map(|p| p.port));
        let Some(port) = port else {
            return false;
        };
        let p = port.to_string();
        let lower = command.to_ascii_lowercase();
        (lower.contains("lsof") || lower.contains("kill") || lower.contains("fuser"))
            && (command.contains(&p) || lower.contains(&format!(":{port}")))
    }

    /// Record perception; only snapshot/vision Ok latches Perceive (not bare navigate).
    pub fn note_perceive(
        &mut self,
        tool_name: &str,
        url: &str,
        content_class: ContentClass,
    ) {
        if !self.config.enabled {
            return;
        }
        let can_latch_perceive = matches!(
            tool_name,
            "browser_snapshot"
                | "browser_vision"
                | "vision"
                | "vision_analyze"
                | "analyze_image"
                | "capture_screenshot"
        );

        if content_class.is_evidence() && can_latch_perceive {
            self.ensure_preview_from_url(url);
            self.perceive = Some(PerceiveLatch {
                url: url.to_string(),
                content_class,
                evidence_tool: tool_name.to_string(),
            });
            self.thrash_counts.clear();
            self.thrash_blocked = false;
            if self.artifact && self.preview.is_some() {
                self.phase = EvidencePhase::LatchedDone;
            } else if !matches!(self.phase, EvidencePhase::Escalated) {
                self.phase = EvidencePhase::Verify;
            }
            return;
        }

        // Navigate transport Ok alone never latches perceive.
        if content_class.is_evidence() && tool_name == "browser_navigate" {
            return;
        }

        // Content failure.
        self.perceive = None;
        if self.should_enter_heal_on_content_fail(content_class) {
            self.enter_heal();
            return;
        }
        self.note_thrash_fingerprint(tool_name, content_class, url);
    }

    fn should_enter_heal_on_content_fail(&self, content_class: ContentClass) -> bool {
        if self.heal_remaining == 0 {
            return false;
        }
        if matches!(self.phase, EvidencePhase::Heal | EvidencePhase::Escalated | EvidencePhase::LatchedDone)
        {
            return false;
        }
        // Only content failures that indicate wrong/broken preview.
        matches!(
            content_class,
            ContentClass::HttpErrorPage
                | ContentClass::EmptyDocument
                | ContentClass::AppFail
                | ContentClass::ChromeError
        ) && (self.preview.is_some()
            || self.preview_candidate.is_some()
            || self.last_heal_port.is_some())
    }

    /// Enter exclusive Heal phase: clear false latch, allow one re-serve.
    pub fn enter_heal(&mut self) {
        if !self.config.enabled || self.heal_remaining == 0 {
            self.escalate("heal_exhausted");
            return;
        }
        if let Some(p) = self.preview.as_ref().or(self.preview_candidate.as_ref()) {
            self.last_heal_port = Some(p.port);
            self.last_heal_dir = Some(p.dir.clone());
        }
        self.preview = None;
        self.preview_candidate = None;
        self.perceive = None;
        self.thrash_blocked = false;
        self.thrash_counts.clear();
        self.phase = EvidencePhase::Heal;
    }

    /// After a Heal-allowed serve executes.
    pub fn consume_heal_serve(&mut self) {
        if self.heal_remaining > 0 {
            self.heal_remaining = self.heal_remaining.saturating_sub(1);
        }
        self.phase = EvidencePhase::Verify;
        self.thrash_blocked = false;
        self.thrash_counts.clear();
    }

    pub fn escalate(&mut self, _reason: &str) {
        self.phase = EvidencePhase::Escalated;
        self.thrash_blocked = true;
    }

    /// Exact thrash fingerprint (019 FP5).
    pub fn note_thrash_fingerprint(
        &mut self,
        tool_name: &str,
        content_class: ContentClass,
        url_or_shape: &str,
    ) {
        if !self.config.enabled {
            return;
        }
        if matches!(self.phase, EvidencePhase::LatchedDone) {
            return;
        }
        let fp = format!(
            "{}|{:?}|{}",
            tool_name,
            content_class,
            normalize_url_shape(url_or_shape)
        );
        let count = self.thrash_counts.entry(fp).or_insert(0);
        *count = count.saturating_add(1);
        if *count >= self.config.thrash_fingerprint_limit {
            if self.heal_remaining > 0 && !matches!(self.phase, EvidencePhase::Heal) {
                self.enter_heal();
            } else {
                self.escalate("thrash_limit");
            }
        }
    }

    pub fn note_transport_thrash(&mut self, tool_name: &str, error_code: &str) {
        self.note_thrash_fingerprint(tool_name, ContentClass::TransportFail, error_code);
    }

    /// Inject once per advisory class (supersede — no spam). Returns false if already injected.
    pub fn may_inject_advisory(&mut self, class: &str) -> bool {
        if !self.config.enabled {
            return true;
        }
        if self.last_advisory_class.as_deref() == Some(class) {
            return false;
        }
        self.last_advisory_class = Some(class.to_string());
        true
    }

    pub fn note_media_artifact_ok(&mut self, path: &str, size_bytes: u64) {
        if !self.config.enabled {
            return;
        }
        if size_bytes > 0 {
            self.note_artifact_path(path);
            self.oracle_ok = true;
            self.perceive = Some(PerceiveLatch {
                url: format!("file:{path}"),
                content_class: ContentClass::Ok,
                evidence_tool: "media_artifact".into(),
            });
            self.phase = EvidencePhase::LatchedDone;
        }
    }

    pub fn set_oracle_ok(&mut self, ok: bool) {
        self.oracle_ok = ok;
    }

    /// Visual_ux: artifact + content-qualified preview + perceive(Ok).
    pub fn visual_evidence_complete(&self) -> bool {
        self.artifact
            && self
                .preview
                .as_ref()
                .is_some_and(|p| p.content_qualified)
            && self
                .perceive
                .as_ref()
                .is_some_and(|p| p.content_class.is_evidence())
    }

    pub fn media_evidence_complete(&self) -> bool {
        self.artifact
            && self
                .perceive
                .as_ref()
                .is_some_and(|p| p.evidence_tool == "media_artifact")
    }

    /// Operator/model message listing **only currently allowed** actions (022 A4).
    pub fn allowed_action_message(&self) -> String {
        if !self.config.enabled {
            return String::new();
        }
        match self.phase {
            EvidencePhase::Heal => {
                let port = self
                    .last_heal_port
                    .or_else(|| self.preview_candidate.as_ref().map(|p| p.port))
                    .unwrap_or(8000);
                let dir = self
                    .last_heal_dir
                    .as_ref()
                    .or_else(|| self.preview_candidate.as_ref().map(|p| &p.dir))
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<demo-dir>".into());
                format!(
                    "[harness] HEAL phase — only allowed action: terminal \
                     `python3 -m http.server {port} --directory {dir}` \
                     (optional: lsof/kill on :{port}), then browser_navigate \
                     http://127.0.0.1:{port}/ and browser_snapshot. \
                     Do not retry other ports or shell debug."
                )
            }
            EvidencePhase::Escalated => {
                "[harness] ESCALATED — verification hard-stop. Summarize deliverables and \
                 why preview failed. Do not call browser_*, http.server, or shell thrash."
                    .into()
            }
            EvidencePhase::LatchedDone => {
                "[harness] DONE latch — perception evidence is complete. \
                 Reply to the user; do not call browser tools or re-serve."
                    .into()
            }
            EvidencePhase::Verify if self.verify_budget_exhausted() => {
                "[harness] VERIFY BUDGET EXHAUSTED — stop tool thrash; summarize status."
                    .into()
            }
            EvidencePhase::Verify | EvidencePhase::Create => {
                if let Some(p) = self.preview.as_ref().filter(|p| p.content_qualified) {
                    format!(
                        "[harness] Navigate only {} then browser_snapshot/browser_vision.",
                        p.url
                    )
                } else if let Some(c) = &self.preview_candidate {
                    format!(
                        "[harness] Preview candidate on port {} — browser_navigate {} then \
                         browser_snapshot. If Error response, heal with one re-serve of the demo dir.",
                        c.port, c.url
                    )
                } else {
                    "[harness] Start one preview: `python3 -m http.server PORT --directory <demo-dir>`, \
                     then browser_navigate + browser_snapshot."
                        .into()
                }
            }
        }
    }

    fn ensure_preview_from_url(&mut self, url: &str) {
        if self
            .preview
            .as_ref()
            .is_some_and(|p| p.content_qualified)
        {
            return;
        }
        let port = parse_loopback_port(url).unwrap_or(8000);
        let dir = self
            .preview_candidate
            .as_ref()
            .map(|c| c.dir.clone())
            .or_else(|| self.last_heal_dir.clone())
            .unwrap_or_else(|| PathBuf::from("."));
        let process_id = self
            .preview_candidate
            .as_ref()
            .and_then(|c| c.process_id.clone());
        self.latch_preview_qualified(
            dir,
            port,
            url.to_string(),
            process_id,
            Some(200),
            true,
        );
    }

    fn dirty_if_preview_paths_changed(&mut self) {
        let Some(preview) = &self.preview else {
            return;
        };
        let fp = fingerprint_paths(&self.artifact_paths);
        if fp != preview.artifact_fingerprint {
            self.perceive = None;
            if let Some(p) = &mut self.preview {
                p.artifact_fingerprint = fp;
                p.content_qualified = false;
            }
            // Mutation after latch → need re-perceive; allow re-serve if needed.
            if matches!(self.phase, EvidencePhase::LatchedDone) {
                self.phase = EvidencePhase::Verify;
            }
        }
    }

    pub fn clear_preview(&mut self) {
        self.preview = None;
        self.preview_candidate = None;
        self.perceive = None;
    }
}

fn is_verify_tool(name: &str) -> bool {
    matches!(
        name,
        "browser_navigate"
            | "browser_snapshot"
            | "browser_vision"
            | "browser_click"
            | "browser_get_images"
            | "browser_console"
            | "vision"
            | "vision_analyze"
            | "analyze_image"
            | "capture_screenshot"
            | "tool_search"
    )
}

fn is_browser_tool(name: &str) -> bool {
    name.starts_with("browser_")
        || matches!(
            name,
            "vision" | "vision_analyze" | "analyze_image" | "capture_screenshot"
        )
}

fn fingerprint_paths(paths: &[String]) -> u64 {
    let mut sorted: Vec<&str> = paths.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let joined = sorted.join("\n");
    let digest = Sha256::digest(joined.as_bytes());
    u64::from_le_bytes(digest[0..8].try_into().unwrap_or([0u8; 8]))
}

fn normalize_url_shape(url: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    base.trim_end_matches('/').to_string()
}

fn parse_loopback_port(url: &str) -> Option<u16> {
    // http://127.0.0.1:8000/ or http://localhost:8765
    let after = url.split("://").nth(1)?;
    let hostport = after.split('/').next()?;
    let port_s = hostport.split(':').nth(1)?;
    port_s.parse().ok()
}

/// Auto-goal texts for visual_ux (019 C5).
pub fn visual_ux_auto_subgoals() -> &'static [&'static str] {
    &[
        "Write demo artifacts to the target directory",
        "Latch preview server (single port) for the demo directory",
        "Capture browser perception evidence (snapshot or vision) on latched URL",
        "Stop when evidence latches are complete",
    ]
}

/// Auto-goal texts for media_render.
pub fn media_render_auto_subgoals() -> &'static [&'static str] {
    &[
        "Produce render output file (video/media artifact)",
        "Confirm output file exists with non-zero size",
        "Stop when media evidence is complete",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nf_u5_content_qualified_preview_blocks_serves() {
        let mut st = EvidenceState::default();
        st.note_artifact_path("demos/pinguin/index.html");
        // Candidate only — does not block.
        st.note_preview_candidate(
            PathBuf::from("demos/pinguin"),
            8000,
            "http://127.0.0.1:8000/".into(),
            None,
            Some(200),
        );
        assert!(!st.blocks_preview_serve(8000, Path::new("demos/pinguin")));
        st.latch_preview(
            PathBuf::from("demos/pinguin"),
            8000,
            "http://127.0.0.1:8000/".into(),
            None,
            Some(200),
        );
        assert!(st.blocks_preview_serve(8010, Path::new("demos/pinguin")));
        assert!(st.blocks_preview_serve(8000, Path::new("demos/pinguin")));
    }

    #[test]
    fn nf_u6_mutation_dirties_perceive() {
        let mut st = EvidenceState::default();
        st.note_artifact_path("a.html");
        st.latch_preview(
            PathBuf::from("."),
            8000,
            "http://127.0.0.1:8000/".into(),
            None,
            None,
        );
        st.note_perceive("browser_snapshot", "http://127.0.0.1:8000/", ContentClass::Ok);
        assert!(st.perceive.is_some());
        st.note_artifact_path("b.js");
        assert!(st.perceive.is_none(), "new artifact path dirties perceive");
    }

    #[test]
    fn nf_u7_thrash_enters_heal_then_escalates() {
        let mut st = EvidenceState::new(EvidenceLatchConfig {
            enabled: true,
            verify_tool_budget: 12,
            thrash_fingerprint_limit: 3,
            heal_budget: 1,
            post_perceive_browser_budget: 0,
        });
        st.note_preview_candidate(
            PathBuf::from("demos/chess"),
            8000,
            "http://127.0.0.1:8000/".into(),
            None,
            None,
        );
        // First content fail → Heal immediately
        st.note_perceive(
            "browser_navigate",
            "http://127.0.0.1:8000/",
            ContentClass::HttpErrorPage,
        );
        assert_eq!(st.phase, EvidencePhase::Heal);
        assert!(!st.blocks_preview_serve(8000, Path::new("demos/chess")));
        // Consume heal
        st.note_preview_candidate(
            PathBuf::from("demos/chess"),
            8000,
            "http://127.0.0.1:8000/".into(),
            None,
            None,
        );
        assert_eq!(st.heal_remaining, 0);
        // Fail again → thrash to escalated
        for _ in 0..3 {
            st.note_thrash_fingerprint(
                "browser_navigate",
                ContentClass::HttpErrorPage,
                "http://127.0.0.1:8000/",
            );
        }
        assert!(st.is_escalated());
        assert!(st.blocks_browser_tools());
    }

    #[test]
    fn nf_u8_advisory_supersede_records_class() {
        let mut st = EvidenceState::default();
        assert!(st.may_inject_advisory("preview_recovery"));
        assert_eq!(st.last_advisory_class.as_deref(), Some("preview_recovery"));
        assert!(
            !st.may_inject_advisory("preview_recovery"),
            "same class must not re-inject"
        );
        assert!(st.may_inject_advisory("heal_phase"));
    }

    #[test]
    fn nf_u9_media_artifact_completes_without_browser() {
        let mut st = EvidenceState::default();
        st.note_media_artifact_ok("/tmp/out.mp4", 1024);
        assert!(st.media_evidence_complete());
        assert!(!st.visual_evidence_complete());
    }

    #[test]
    fn nf_u10_verify_budget_exhaust() {
        let mut st = EvidenceState::new(EvidenceLatchConfig {
            enabled: true,
            verify_tool_budget: 3,
            thrash_fingerprint_limit: 3,
            heal_budget: 1,
            post_perceive_browser_budget: 0,
        });
        st.note_artifact_path("index.html");
        st.enter_verify_phase();
        for _ in 0..3 {
            st.count_tool("browser_navigate", false);
        }
        assert!(st.verify_budget_exhausted());
        assert!(st.should_hard_stop());
    }

    #[test]
    fn success_snapshot_latches_done() {
        let mut st = EvidenceState::default();
        st.note_artifact_path("index.html");
        st.note_preview_candidate(
            PathBuf::from("demos/chess"),
            8000,
            "http://127.0.0.1:8000/".into(),
            None,
            None,
        );
        st.note_perceive(
            "browser_snapshot",
            "http://127.0.0.1:8000/",
            ContentClass::Ok,
        );
        assert!(st.visual_evidence_complete());
        assert_eq!(st.phase, EvidencePhase::LatchedDone);
        assert!(st.blocks_browser_tools());
    }

    #[test]
    fn navigate_ok_alone_does_not_latch_perceive() {
        let mut st = EvidenceState::default();
        st.note_artifact_path("index.html");
        st.note_perceive(
            "browser_navigate",
            "http://127.0.0.1:8000/",
            ContentClass::Ok,
        );
        assert!(st.perceive.is_none());
        assert!(!st.visual_evidence_complete());
    }

    #[test]
    fn heal_allows_exact_server_command() {
        let mut st = EvidenceState::default();
        st.last_heal_port = Some(8000);
        st.last_heal_dir = Some(PathBuf::from("demos/chess"));
        st.phase = EvidencePhase::Heal;
        st.heal_remaining = 1;
        assert_eq!(
            st.terminal_heal_policy(
                "python3 -m http.server 8000 --directory demos/chess"
            ),
            Some(true)
        );
        assert_eq!(st.terminal_heal_policy("ls -la"), Some(false));
        assert_eq!(
            st.terminal_heal_policy("lsof -i :8000"),
            Some(true)
        );
    }

    #[test]
    fn allowed_action_message_heal_mentions_only_heal() {
        let mut st = EvidenceState::default();
        st.phase = EvidencePhase::Heal;
        st.last_heal_port = Some(8000);
        st.last_heal_dir = Some(PathBuf::from("demos/chess"));
        let msg = st.allowed_action_message();
        assert!(msg.contains("HEAL"));
        assert!(msg.contains("http.server"));
        assert!(msg.contains("8000"));
    }
}
