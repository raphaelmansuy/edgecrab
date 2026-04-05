//! # PromptBuilder — assembles the system prompt from context sources
//!
//! WHY a builder: The system prompt is assembled from ~12 sources
//! (identity, platform hints, time, SOUL.md, AGENTS.md, memory,
//! skills, etc.). Each source is optional and discovered at runtime.
//! The builder collects them in priority order and joins them into
//! a single cohesive prompt.
//!
//! ```text
//!   PromptBuilder::new(platform)
//!       .identity(override?)
//!       .discover_context_files(cwd)
//!       .add_memory_sections(...)
//!       .build()  →  String
//! ```

use std::borrow::Cow;
use std::path::Path;
use std::sync::Mutex;

use edgecrab_types::Platform;

// ─── Skills cache ─────────────────────────────────────────────────────
//
// WHY: Scanning ~/.edgecrab/skills/ on every session start is redundant
// when files rarely change. A module-level in-memory cache with a 60-second
// TTL avoids repeated disk I/O while still picking up newly installed skills.
// This mirrors hermes-agent's two-layer (LRU + disk snapshot) approach,
// simplified to a single Mutex-protected entry since the cache key is
// always the same home directory.

struct SkillsCacheEntry {
    /// The cached summary string (or None if skills dir is absent).
    summary: Option<String>,
    /// Disabled skills used when this entry was generated.
    disabled_at_build: Vec<String>,
    /// Wall-clock time when the cache was populated.
    built_at: std::time::Instant,
}

// Key = canonical edgecrab_home path
type SkillsCacheMap = std::collections::HashMap<std::path::PathBuf, SkillsCacheEntry>;

static SKILLS_CACHE: Mutex<Option<SkillsCacheMap>> = Mutex::new(None);

/// TTL for the in-process skills cache.
const SKILLS_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// Invalidate the in-process skills cache.
///
/// Call this after installing or removing a skill so the next
/// `load_skill_summary` call rescans the disk immediately.
pub fn invalidate_skills_cache() {
    if let Ok(mut guard) = SKILLS_CACHE.lock() {
        *guard = None;
    }
}

// ─── Constants ────────────────────────────────────────────────────────

const DEFAULT_IDENTITY: &str = "\
You are EdgeCrab, an intelligent AI agent built with Rust for speed and safety. \
You are helpful, knowledgeable, and direct. You assist users with a wide range of \
tasks including answering questions, writing and debugging code, code review, \
architecture design, analysing information, creative work, and executing actions \
via your tools. You communicate clearly, admit uncertainty when appropriate, and \
prioritise being genuinely useful over being verbose unless otherwise directed. \
Be targeted and efficient in your exploration and investigations.";

const CLI_HINT: &str = "\
You are a CLI AI Agent. Use markdown formatting with code blocks where helpful. \
ANSI colors are supported.";

const TELEGRAM_HINT: &str = "\
You are on a text messaging communication platform, Telegram. \
Please do not use markdown as it does not render. \
You can send media files natively: to deliver a file to the user, \
include MEDIA:/absolute/path/to/file in your response. Images \
(.png, .jpg, .webp) appear as photos, audio (.ogg) sends as voice \
bubbles, and videos (.mp4) play inline. You can also include image \
URLs in markdown format ![alt](url) and they will be sent as native photos.";

const DISCORD_HINT: &str = "\
You are in a Discord server or group chat communicating with your user. \
You can send media files natively: include MEDIA:/absolute/path/to/file \
in your response. Images (.png, .jpg, .webp) are sent as photo \
attachments, audio as file attachments. You can also include image URLs \
in markdown format ![alt](url) and they will be sent as attachments.";

const WHATSAPP_HINT: &str = "\
You are on a text messaging communication platform, WhatsApp. \
Please do not use markdown as it does not render. \
You can send media files natively: to deliver a file to the user, \
include MEDIA:/absolute/path/to/file in your response. The file \
will be sent as a native WhatsApp attachment — images (.jpg, .png, \
.webp) appear as photos, videos (.mp4, .mov) play inline, and other \
files arrive as downloadable documents. You can also include image \
URLs in markdown format ![alt](url) and they will be sent as photos.";

const SLACK_HINT: &str = "\
You are in a Slack workspace communicating with your user. \
You can send media files natively: include MEDIA:/absolute/path/to/file \
in your response. Images (.png, .jpg, .webp) are uploaded as photo \
attachments, audio as file attachments. You can also include image URLs \
in markdown format ![alt](url) and they will be uploaded as attachments.";

const FEISHU_HINT: &str = "\
You are in a Feishu or Lark workspace communicating with your user. \
Prefer plain text over heavy markdown. Keep formatting simple and clear, \
and avoid tables unless the content really requires them.";

const WECOM_HINT: &str = "\
You are in a WeCom workspace communicating with your user. \
Keep responses concise and readable in chat. Prefer plain text with light \
structure over complex markdown or deeply nested formatting.";

const SIGNAL_HINT: &str = "\
You are on a text messaging communication platform, Signal. \
Please do not use markdown as it does not render. \
You can send media files natively: to deliver a file to the user, \
include MEDIA:/absolute/path/to/file in your response. Images \
(.png, .jpg, .webp) appear as photos, audio as attachments, and other \
files arrive as downloadable documents. You can also include image \
URLs in markdown format ![alt](url) and they will be sent as photos.";

const EMAIL_HINT: &str = "\
You are communicating via email. Write clear, well-structured responses \
suitable for email. Use plain text formatting (no markdown). \
Keep responses concise but complete. You can send file attachments — \
include MEDIA:/absolute/path/to/file in your response. The subject line \
is preserved for threading. Do not include greetings or sign-offs unless \
contextually appropriate.";

const SMS_HINT: &str = "\
You are communicating via SMS. Keep responses concise and use plain text \
only — no markdown, no formatting. SMS messages are limited to ~1600 \
characters, so be brief and direct.";

const WEBHOOK_HINT: &str = "\
You are running via Webhook integration. Return structured JSON-friendly \
responses. Keep responses concise and machine-parseable when possible.";

const API_HINT: &str = "\
You are running via API. Return well-structured responses suitable for \
programmatic consumption. Use markdown for formatting.";

const CRON_HINT: &str = "\
You are running as a scheduled cron job. There is no user present — you \
cannot ask questions, request clarification, or wait for follow-up. Execute \
the task fully and autonomously, making reasonable decisions where needed. \
Your final response is automatically delivered to the job's configured \
destination — put the primary content directly in your response.";

const MEMORY_GUIDANCE: &str = "\
You have persistent memory across sessions. Save durable facts using the memory_write \
tool: user preferences, environment details, tool quirks, and stable conventions. \
Memory is injected into every session, so keep it compact and focused on facts that \
will still matter later. Prioritise what reduces future user steering — the most \
valuable memory is one that prevents the user from having to correct or remind you \
again. User preferences and recurring corrections matter more than procedural task \
details. Do NOT save task progress, session outcomes, completed-work logs, or \
temporary TODO state to memory; use session_search to recall those from past \
transcripts. If you've discovered a new non-trivial workflow, save it as a skill \
with skill_manage.";

const SESSION_SEARCH_GUIDANCE: &str = "\
When the user references something from a past conversation or you suspect relevant \
cross-session context exists, use session_search to recall it before asking them to \
repeat themselves.";

const SCHEDULING_GUIDANCE: &str = "\
Use manage_cron_jobs for ALL cron job operations — never edit ~/.edgecrab/cron/jobs.json \
directly via terminal.\n\
\n\
Action→intent mapping:\n\
  create — user wants to schedule a new task: 'every morning', 'remind me daily',\n\
           'check every 2 hours', 'run this each weekday at 9am'.\n\
  list   — user wants to see scheduled jobs: 'show my cron jobs', 'what's scheduled',\n\
           'list automations', 'what jobs are running'.\n\
  pause  — user wants to stop/suppress a job: 'pause the daily briefing',\n\
           'suppress all cron jobs', 'disable the weather check', 'stop the reminder'.\n\
  resume — user wants to re-enable a paused job: 'restart', 'resume', 're-enable'.\n\
  remove — user wants to delete permanently: 'delete', 'remove', 'cancel the job'.\n\
  status — user wants a summary count / next-run time: 'cron status', 'when does it run'.\n\
  update — user wants to change schedule/prompt/delivery of an existing job.\n\
\n\
Workflow for 'suppress all cron jobs':\n\
  1. manage_cron_jobs(action='list')  ← get all job_ids\n\
  2. manage_cron_jobs(action='pause', job_id='<id>')  ← pause each one\n\
\n\
The cron prompt must be fully self-contained — include all specifics (URLs, \
credentials, servers, what to check) since the job runs in a fresh session with \
no access to chat history.\n\
\n\
Delivery — map the user's words to the deliver= parameter:\n\
  'send me on Telegram' / 'notify via Telegram'    → deliver='telegram'\n\
  'send to Discord' / 'post in Discord'            → deliver='discord'\n\
  'notify me on Slack'                             → deliver='slack'\n\
  'send via WhatsApp'                              → deliver='whatsapp'\n\
  'email me the results'                           → deliver='email'\n\
  'send me on Signal'                              → deliver='signal'\n\
  'notify me here' / 'reply in this chat'          → deliver='origin'\n\
  'keep local' / no delivery preference mentioned  → deliver='local'\n\
  'telegram chat -100123456'                        → deliver='telegram:-100123456'\n\
Default: deliver='local' on CLI unless the user specifies a platform. \
For delivery back to the user in this chat, use deliver='origin'.";

const MESSAGE_DELIVERY_GUIDANCE: &str = "\
Use send_message only when the user explicitly wants content delivered to a different \
platform, contact, channel, or thread than the current reply path.\n\
\n\
Rules:\n\
  - Normal reply in the current chat unless the user asks to send elsewhere.\n\
  - If the user gives a clear imperative to send and provides the destination and content, send it directly instead of asking for redundant confirmation.\n\
  - If the user asks to send to Telegram, WhatsApp, Discord, Slack, Signal, email, SMS, or another target, use send_message.\n\
  - If the user asks for a draft, suggested wording, or a message to review, do NOT send it.\n\
  - If the user names only a platform, use that platform's home channel.\n\
  - If the user names a specific channel/person and the target is ambiguous, call send_message(action='list') first.\n\
  - Do not claim you cannot send messages when send_message is available.";

const SKILLS_GUIDANCE: &str = "\
After completing a complex task (5+ tool calls), fixing a tricky error, or discovering \
a non-trivial workflow, save the approach as a skill with skill_manage so you can reuse \
it next time. When using a skill and finding it outdated, incomplete, or wrong, patch it \
immediately with skill_manage(action='edit') — don't wait to be asked. Skills that \
aren't maintained become liabilities.";

/// Injected when `vision_analyze` is in the tool list.
///
/// WHY: Two distinct failure modes motivate this block:
///
/// 1. Tool-selection ambiguity (small LOCAL models — qwen3, llama3):
///    Both `browser_vision` and `vision_analyze` contain "vision". Without
///    explicit guidance the model picks `browser_vision` first because it
///    appears earlier in the tool list — even when the user attached a local
///    file. Rule: file path → vision_analyze.
///
/// 2. Double-call (all models, observed with qwen3.5:latest):
///    After `vision_analyze` returns a result the model sometimes continues
///    making tool calls and also calls `browser_vision` as a "confirmation".
///    This wastes 200-300 s and produces duplicate output. Rule: call
///    vision_analyze EXACTLY ONCE per image, then respond.
///
/// 3. execute_code path (large FRONTIER models — GPT-4.1, Sonnet, etc.):
///    These models prefer to wrap multi-step work in execute_code scripts.
///    `vision_analyze` is now exposed as an RPC stub in the sandbox, so
///    `from edgecrab_tools import vision_analyze` works correctly. The rule
///    here is the same: do NOT also call browser_vision afterwards.
const VISION_GUIDANCE: &str = "\
## Image Analysis — Tool Selection Rules

You have TWO vision tools. Use EXACTLY ONE per attached image:

- **vision_analyze** — analyzes a local image FILE or an HTTP(S) image URL.
  Use this when:
  * The user pastes or attaches an image (clipboard paste gives a file path such
    as ~/.edgecrab/images/clipboard_*.png).
  * The user provides any local file path ending in .png, .jpg, .jpeg, .gif,
    .webp, .bmp, .tiff, .avif, or .ico.
  * The user provides an https:// image URL.
  * The prompt contains an *** ATTACHED IMAGES block.
  * Inside execute_code scripts: `from edgecrab_tools import vision_analyze`.

- **browser_vision** — captures a LIVE SCREENSHOT of the current browser page.
  Use this ONLY when you need to visually inspect a web page that is currently
  open in the browser. It does NOT accept file paths. It cannot analyze local
  files or clipboard images.

Decision rule (apply literally, no exceptions):
  file path or *** ATTACHED IMAGES block present  →  vision_analyze (once)
  inspecting the live browser page                →  browser_vision (once)

CRITICAL — ONE CALL RULE:
  After vision_analyze returns a result, respond to the user immediately.
  Do NOT call browser_vision as a second step, confirmation, or fallback.
  Do NOT call browser_vision after vision_analyze for the same request.
  Calling both tools for one image is always wrong.

NEVER call browser_vision when a local image file path is given.";

/// Maximum characters for a context file before truncation kicks in.
const CONTEXT_FILE_MAX_CHARS: usize = 20_000;

/// Head fraction of the truncated content (70% head, 30% tail).
const TRUNCATION_HEAD_RATIO: f64 = 0.70;

// ─── Prompt Injection Scanning ────────────────────────────────────────

/// Severity level for a detected injection threat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreatSeverity {
    Low,
    Medium,
    High,
}

/// A single detected injection threat in user-supplied text.
#[derive(Debug, Clone, PartialEq)]
pub struct InjectionThreat {
    pub pattern_name: String,
    pub severity: ThreatSeverity,
}

/// Invisible unicode codepoints used in prompt injection attacks.
const INVISIBLE_CHARS: &[char] = &[
    '\u{200B}', // zero-width space
    '\u{200C}', // zero-width non-joiner
    '\u{200D}', // zero-width joiner
    '\u{2060}', // word joiner
    '\u{FEFF}', // BOM / zero-width no-break space
    '\u{2028}', // line separator
    '\u{2029}', // paragraph separator
];

/// Homoglyph characters that look like ASCII but are different unicode codepoints.
/// Common Cyrillic/Greek lookalikes for Latin letters.
const HOMOGLYPH_RANGES: &[(char, char)] = &[
    ('\u{0400}', '\u{04FF}'), // Cyrillic
    ('\u{0370}', '\u{03FF}'), // Greek
    ('\u{FF01}', '\u{FF5E}'), // Fullwidth ASCII variants
];

/// Scan text for prompt injection patterns.
///
/// Returns a list of detected threats. An empty Vec means no threats found.
/// This is a heuristic scanner — not a guarantee against all attacks.
pub fn scan_for_injection(text: &str) -> Vec<InjectionThreat> {
    let mut threats = Vec::new();
    let lower = text.to_lowercase();

    // Text-based threat patterns: (substring, pattern_name, severity)
    let text_patterns: &[(&str, &str, ThreatSeverity)] = &[
        ("ignore previous", "ignore_previous", ThreatSeverity::High),
        (
            "ignore all instructions",
            "ignore_all_instructions",
            ThreatSeverity::High,
        ),
        ("disregard", "disregard", ThreatSeverity::Medium),
        ("override system", "override_system", ThreatSeverity::High),
        ("you are now", "you_are_now", ThreatSeverity::High),
        (
            "forget everything",
            "forget_everything",
            ThreatSeverity::High,
        ),
        (
            "new instructions:",
            "new_instructions",
            ThreatSeverity::High,
        ),
        (
            "system prompt:",
            "system_prompt_leak",
            ThreatSeverity::Medium,
        ),
    ];

    for &(pattern, name, severity) in text_patterns {
        if lower.contains(pattern) {
            threats.push(InjectionThreat {
                pattern_name: name.to_string(),
                severity,
            });
        }
    }

    // Check for invisible unicode characters
    if text.chars().any(|c| INVISIBLE_CHARS.contains(&c)) {
        threats.push(InjectionThreat {
            pattern_name: "invisible_unicode".to_string(),
            severity: ThreatSeverity::High,
        });
    }

    // Check for homoglyph characters
    let has_homoglyphs = text.chars().any(|c| {
        HOMOGLYPH_RANGES
            .iter()
            .any(|&(start, end)| c >= start && c <= end)
    });
    if has_homoglyphs {
        threats.push(InjectionThreat {
            pattern_name: "homoglyph_characters".to_string(),
            severity: ThreatSeverity::Medium,
        });
    }

    threats
}

// ─── YAML Frontmatter Stripping ──────────────────────────────────────

/// Strip YAML frontmatter (content between leading `---` markers) from text.
///
/// YAML frontmatter is metadata at the top of markdown files:
/// ```text
/// ---
/// title: My Doc
/// tags: [a, b]
/// ---
/// Actual content starts here.
/// ```
pub fn strip_yaml_frontmatter(text: &str) -> &str {
    let trimmed = text.trim_start();
    if !trimmed.starts_with("---") {
        return text;
    }
    // Find the closing `---` after the opening one
    let after_first = &trimmed[3..];
    if let Some(end_pos) = after_first.find("\n---") {
        // Skip past the closing `---` and the newline after it
        let remainder = &after_first[end_pos + 4..];
        remainder.trim_start_matches('\n').trim_start_matches('\r')
    } else {
        // No closing `---` found — return original text
        text
    }
}

// ─── Context File Truncation ─────────────────────────────────────────

/// Truncate a context file using head/tail strategy.
///
/// If `text` exceeds `CONTEXT_FILE_MAX_CHARS`, keep 70% from the head
/// and 30% from the tail with a marker in between.
pub fn truncate_context_file(text: &str) -> Cow<'_, str> {
    if text.len() <= CONTEXT_FILE_MAX_CHARS {
        return Cow::Borrowed(text);
    }

    let head_len = (CONTEXT_FILE_MAX_CHARS as f64 * TRUNCATION_HEAD_RATIO) as usize;
    let tail_len = CONTEXT_FILE_MAX_CHARS - head_len;

    let head = crate::safe_truncate(text, head_len);
    let tail_start = crate::safe_char_start(text, text.len() - tail_len);
    let tail = &text[tail_start..];

    let omitted = text.len() - CONTEXT_FILE_MAX_CHARS;
    Cow::Owned(format!(
        "{head}\n\n... [{omitted} characters omitted] ...\n\n{tail}"
    ))
}

// ─── PromptBuilder ────────────────────────────────────────────────────

pub struct PromptBuilder {
    platform: Platform,
    skip_context_files: bool,
    /// Optional list of tool names available in this session.
    /// When `None`, all guidance is injected (backward compat / tests).
    /// When `Some`, each guidance snippet is only injected when its gate tool is present.
    available_tools: Option<Vec<String>>,
}

impl PromptBuilder {
    pub fn new(platform: Platform) -> Self {
        Self {
            platform,
            skip_context_files: false,
            available_tools: None,
        }
    }

    pub fn skip_context_files(mut self, skip: bool) -> Self {
        self.skip_context_files = skip;
        self
    }

    /// Gate behavioral guidance on tool availability.
    ///
    /// WHY: MEMORY_GUIDANCE is only useful when `memory_write` is available.
    /// SESSION_SEARCH_GUIDANCE only matters when `session_search` is present.
    /// SKILLS_GUIDANCE only fires when `skill_manage` is loaded. Injecting all
    /// guidance unconditionally wastes tokens on configurations without those tools.
    /// Mirrors hermes-agent's tool-gated guidance injection in `_build_system_prompt`.
    pub fn available_tools(mut self, tools: Vec<String>) -> Self {
        self.available_tools = Some(tools);
        self
    }

    /// Returns true when the list is absent (all tools assumed) or when `tool` is present.
    fn has_tool(&self, tool: &str) -> bool {
        match &self.available_tools {
            None => true,
            Some(tools) => tools.iter().any(|t| t == tool),
        }
    }

    /// Build the full system prompt.
    ///
    /// `override_identity` replaces the default identity paragraph.
    /// `cwd` is the working directory for context file discovery.
    /// `memory_sections` are pre-formatted memory strings to inject.
    /// `skill_prompt` is the active skills system prompt.
    pub fn build(
        &self,
        override_identity: Option<&str>,
        cwd: Option<&Path>,
        memory_sections: &[String],
        skill_prompt: Option<&str>,
    ) -> String {
        let mut sections: Vec<Cow<'_, str>> = Vec::with_capacity(12);

        // 1. Identity
        sections.push(Cow::Borrowed(override_identity.unwrap_or(DEFAULT_IDENTITY)));

        // 2. Platform hints
        if let Some(hint) = platform_hint(&self.platform) {
            sections.push(Cow::Borrowed(hint));
        }

        // 3. Date/time stamp
        sections.push(Cow::Owned(format!(
            "Current date/time: {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z")
        )));

        // 4-6. Context files (SOUL.md, AGENTS.md, .cursorrules, etc.)
        if !self.skip_context_files {
            if let Some(dir) = cwd {
                let context_files = discover_context_files(dir);
                if !context_files.is_empty() {
                    let mut context_parts: Vec<String> = Vec::new();
                    for (name, content) in context_files {
                        // Scan for prompt injection before injecting into the system prompt.
                        // WHY: Context files like AGENTS.md and SOUL.md are sourced from the
                        // project workspace and could be tampered with to inject malicious
                        // instructions. Scan and block any suspicious content.
                        let threats = scan_for_injection(&content);
                        let critical: Vec<_> = threats
                            .iter()
                            .filter(|t| matches!(t.severity, ThreatSeverity::High))
                            .collect();
                        if !critical.is_empty() {
                            let kinds: Vec<&str> =
                                critical.iter().map(|t| t.pattern_name.as_str()).collect();
                            tracing::warn!(
                                file = %name,
                                threats = ?kinds,
                                "Prompt injection detected in context file — blocking injection"
                            );
                            context_parts.push(format!(
                                "[BLOCKED: {name} contained potential prompt injection ({kinds}). Content skipped for security.]",
                                kinds = kinds.join(", ")
                            ));
                            continue;
                        }
                        // Strip YAML frontmatter and truncate
                        let stripped = strip_yaml_frontmatter(&content);
                        let truncated = truncate_context_file(stripped);
                        context_parts.push(format!("## {name}\n\n{}", truncated.trim()));
                    }
                    if !context_parts.is_empty() {
                        // Wrap in a "Project Context" header matching hermes-agent's format.
                        // WHY: This header instructs the agent that these files should be
                        // followed, not just read. Without the header the agent may treat them
                        // as passive reference material rather than binding guidelines.
                        let block = format!(
                            "# Project Context\n\nThe following project context files have been loaded and should be followed:\n\n{}",
                            context_parts.join("\n\n")
                        );
                        sections.push(Cow::Owned(block));
                    }
                }
            }
        }

        // 7. Memory guidance + sections — only when memory tool is available
        if !memory_sections.is_empty() {
            if self.has_tool("memory_write") {
                sections.push(Cow::Borrowed(MEMORY_GUIDANCE));
            }
            for s in memory_sections {
                sections.push(Cow::Borrowed(s.as_str()));
            }
        }

        // 8. Session search guidance — only when session_search tool is present
        if self.has_tool("session_search") {
            sections.push(Cow::Borrowed(SESSION_SEARCH_GUIDANCE));
        }

        // 9. Skills guidance — only when skill_manage tool is present
        if self.has_tool("skill_manage") {
            sections.push(Cow::Borrowed(SKILLS_GUIDANCE));
        }

        // 10. Scheduling guidance — only for interactive sessions (not cron) and
        //     when manage_cron_jobs is available.
        if self.platform != Platform::Cron && self.has_tool("manage_cron_jobs") {
            sections.push(Cow::Borrowed(SCHEDULING_GUIDANCE));
        }

        // 11. Cross-platform delivery guidance — only when send_message is present.
        if self.has_tool("send_message") {
            sections.push(Cow::Borrowed(MESSAGE_DELIVERY_GUIDANCE));
        }

        // 12. Vision tool disambiguation — only when vision_analyze is present.
        // WHY: Smaller local models (qwen3, llama3) reliably pick browser_vision over
        // vision_analyze when local image files are attached, because browser_vision
        // appears earlier in the tool list. Schema descriptions alone are insufficient;
        // the system prompt is the authoritative source of tool-selection rules.
        if self.has_tool("vision_analyze") {
            sections.push(Cow::Borrowed(VISION_GUIDANCE));
        }

        // 13. Skills prompt (available skill descriptions)
        if let Some(sp) = skill_prompt {
            if !sp.is_empty() {
                sections.push(Cow::Borrowed(sp));
            }
        }

        sections.join("\n\n")
    }
}

// ─── Platform hints ───────────────────────────────────────────────────

fn platform_hint(platform: &Platform) -> Option<&'static str> {
    match platform {
        Platform::Cli => Some(CLI_HINT),
        Platform::Telegram => Some(TELEGRAM_HINT),
        Platform::Discord => Some(DISCORD_HINT),
        Platform::Whatsapp => Some(WHATSAPP_HINT),
        Platform::Slack => Some(SLACK_HINT),
        Platform::Feishu => Some(FEISHU_HINT),
        Platform::Wecom => Some(WECOM_HINT),
        Platform::Signal => Some(SIGNAL_HINT),
        Platform::Email => Some(EMAIL_HINT),
        Platform::Sms => Some(SMS_HINT),
        Platform::Webhook => Some(WEBHOOK_HINT),
        Platform::Api => Some(API_HINT),
        Platform::Cron => Some(CRON_HINT),
        _ => None,
    }
}

// ─── Context file discovery ───────────────────────────────────────────

/// Discover context files by walking from `cwd` upward.
///
/// Discover project context files with hermes-compatible priority-first-match.
///
/// Priority order (only ONE project context type is loaded — first match wins):
///
/// 1. `.hermes.md` / `HERMES.md` — walks upward to git root (highest priority)
/// 2. `AGENTS.md`                — hierarchical walk: CWD + all subdirectories
///    (skips hidden dirs, node_modules, __pycache__,
///    venv, .venv, target)
/// 3. `CLAUDE.md`                — CWD only
/// 4. `.cursorrules`             — CWD only
///    `.cursor/rules/*.mdc`      — CWD only (loaded together with .cursorrules)
///
/// SOUL.md is NOT loaded here — it is the agent's identity slot and is loaded
/// globally via `load_global_soul()`. Including it in context files would
/// duplicate it in the prompt.
///
/// This mirrors hermes-agent's `build_context_files_prompt()` behaviour.
fn discover_context_files(cwd: &Path) -> Vec<(String, String)> {
    // Priority 1: .hermes.md / HERMES.md — walk to git root
    if let Some(item) = walk_to_git_root_for_file(
        cwd,
        &[".hermes.md", "HERMES.md", ".edgecrab.md", "EDGECRAB.md"],
    ) {
        return vec![item];
    }

    // Priority 2: AGENTS.md — hierarchical walk (CWD + all subdirectories)
    let agents_files = collect_agents_md_files(cwd);
    if !agents_files.is_empty() {
        return agents_files;
    }

    // Priority 3: CLAUDE.md — CWD only
    if let Some(item) = load_cwd_file(cwd, "CLAUDE.md") {
        return vec![item];
    }

    // Priority 4: .cursorrules + .cursor/rules/*.mdc — CWD only
    let mut cursor_items = Vec::new();
    if let Some(item) = load_cwd_file(cwd, ".cursorrules") {
        cursor_items.push(item);
    }
    cursor_items.extend(load_cursor_mdc_rules(cwd));
    if !cursor_items.is_empty() {
        return cursor_items;
    }

    Vec::new()
}

/// Load a single file from exactly `cwd/name` (no upward walk).
fn load_cwd_file(cwd: &Path, name: &str) -> Option<(String, String)> {
    let path = cwd.join(name);
    let content = std::fs::read_to_string(&path).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    Some((name.to_string(), content))
}

/// Walk upward from `cwd` toward the git root looking for any of `candidates`.
///
/// Stops walking when it finds a `.git` directory (git root reached) or the
/// filesystem root. Returns the first match in priority order.
fn walk_to_git_root_for_file(start: &Path, candidates: &[&str]) -> Option<(String, String)> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        for name in candidates {
            let path = d.join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if !content.trim().is_empty() {
                    return Some((name.to_string(), content));
                }
            }
        }
        // Stop at git root (don't continue past it)
        if d.join(".git").exists() {
            break;
        }
        dir = d.parent();
    }
    None
}

/// Collect all AGENTS.md files by walking CWD and all subdirectories.
///
/// WHY hierarchical: A monorepo may have AGENTS.md files at multiple levels
/// (root, crates/, packages/, etc.) that each provide relevant context.
/// All found files are concatenated so the agent gets the full picture.
///
/// Skipped directories: hidden (`.`-prefixed), node_modules, __pycache__,
/// venv, .venv, target — matching hermes-agent's exclusion list.
fn collect_agents_md_files(cwd: &Path) -> Vec<(String, String)> {
    let mut files: Vec<(std::path::PathBuf, String)> = Vec::new(); // (abs_path, content)
    collect_agents_md_recursive(cwd, cwd, &mut files);

    if files.is_empty() {
        return Vec::new();
    }

    // Sort by path depth then lexicographic so parent AGENTS.md comes first.
    files.sort_by_key(|(p, _)| (p.components().count(), p.to_path_buf()));

    files
        .into_iter()
        .map(|(abs_path, content)| {
            // Display name: relative to cwd if possible
            let rel = abs_path.strip_prefix(cwd).unwrap_or(&abs_path);
            let display = rel.to_string_lossy().into_owned();
            let display = if display.is_empty() {
                "AGENTS.md".to_string()
            } else {
                display
            };
            (display, content)
        })
        .collect()
}

/// Recursive helper: walk `dir`, collecting AGENTS.md files.
fn collect_agents_md_recursive(
    dir: &Path,
    _cwd: &Path,
    files: &mut Vec<(std::path::PathBuf, String)>,
) {
    // Load AGENTS.md in this directory
    let agents_path = dir.join("AGENTS.md");
    if let Ok(content) = std::fs::read_to_string(&agents_path) {
        if !content.trim().is_empty() {
            files.push((agents_path, content));
        }
    }

    // Recurse into subdirectories (skip hidden/system dirs)
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        // Skip hidden dirs, package manager caches, build artifacts
        if name.starts_with('.') {
            continue;
        }
        if matches!(
            name,
            "node_modules" | "__pycache__" | "venv" | ".venv" | "target"
        ) {
            continue;
        }
        collect_agents_md_recursive(&path, _cwd, files);
    }
}

/// Load `.cursor/rules/*.mdc` files from `cwd`.
fn load_cursor_mdc_rules(cwd: &Path) -> Vec<(String, String)> {
    let cursor_rules_dir = cwd.join(".cursor").join("rules");
    if !cursor_rules_dir.is_dir() {
        return Vec::new();
    }

    let entries = match std::fs::read_dir(&cursor_rules_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut mdc_files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "mdc"))
        .collect();
    mdc_files.sort_by_key(|e| e.file_name());

    let mut result = Vec::new();
    for entry in mdc_files {
        let path = entry.path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if !content.trim().is_empty() {
                let display_name = format!(".cursor/rules/{}", entry.file_name().to_string_lossy());
                result.push((display_name, content));
            }
        }
    }
    result
}

// ─── Memory loading (frozen snapshot) ────────────────────────────────

/// Maximum characters for memory content in the system prompt.
const MEMORY_MAX_CHARS: usize = 2200;
/// Maximum characters for user profile content in the system prompt.
const USER_MAX_CHARS: usize = 1375;

/// Load memory sections from `~/.edgecrab/memories/` for system prompt injection.
///
/// WHY frozen snapshot: The system prompt gets a snapshot of memory at session
/// start. Mid-session `memory_write` calls update disk but NOT the cached
/// system prompt. This preserves prompt cache efficiency (Anthropic charges
/// for cache misses when the system prompt changes).
///
/// Returns a Vec of formatted memory sections ready for PromptBuilder.
/// Load a single memory markdown file and format it as a section string.
///
/// Extracted to remove the identical read → trim → truncate → format pattern
/// that existed for both MEMORY.md and USER.md inside `load_memory_sections`.
fn load_memory_file(
    mem_dir: &std::path::Path,
    filename: &str,
    title: &str,
    max_chars: usize,
) -> Option<String> {
    let content = std::fs::read_to_string(mem_dir.join(filename)).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let truncated = crate::safe_truncate(trimmed, max_chars);
    let pct = (trimmed.len() * 100) / max_chars;
    let sep = "═".repeat(46);
    Some(format!(
        "{sep}\n{title} [{pct}% — {}/{max_chars} chars]\n{sep}\n{truncated}",
        trimmed.len()
    ))
}

/// Default SOUL.md content — seeded into `~/.edgecrab/SOUL.md` on first run
/// if the file does not yet exist. Mirrors hermes-agent's auto-seeding behavior.
/// Users can freely edit or replace this file; EdgeCrab never overwrites it.
const DEFAULT_SOUL_MD: &str = "\
You are EdgeCrab — a precise, efficient, and trustworthy AI agent built with Rust. \
You care about correctness, clear explanations, and getting things done. \
You prefer direct answers over verbosity, but never sacrifice clarity for brevity. \
When you're uncertain, you say so. When you make an error, you acknowledge it and fix it. \
You treat users as capable adults and provide the depth they need.";

/// Load the global SOUL.md from `~/.edgecrab/SOUL.md`.
///
/// WHY global vs project: The global SOUL.md defines the agent's baseline
/// identity across all projects. It is seeded automatically on first run so
/// users always have a customisable persona file. Project-level SOUL.md files
/// (in the CWD tree) are loaded separately as context file sections, allowing
/// per-project persona tuning on top of the global baseline.
///
/// Matches hermes-agent's approach: SOUL.md at HERMES_HOME is slot #1 identity.
pub fn load_global_soul(edgecrab_home: &Path) -> Option<String> {
    // Auto-seed if missing
    seed_global_soul(edgecrab_home);

    let soul_path = edgecrab_home.join("SOUL.md");
    let content = std::fs::read_to_string(&soul_path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Security scan — same as context files
    let threats = scan_for_injection(trimmed);
    let critical: Vec<_> = threats
        .iter()
        .filter(|t| matches!(t.severity, ThreatSeverity::High))
        .collect();
    if !critical.is_empty() {
        let kinds: Vec<&str> = critical.iter().map(|t| t.pattern_name.as_str()).collect();
        tracing::warn!(
            threats = ?kinds,
            "Prompt injection detected in SOUL.md — using default identity"
        );
        return None;
    }

    // Truncate at CONTEXT_FILE_MAX_CHARS
    let truncated = truncate_context_file(trimmed);
    Some(truncated.into_owned())
}

/// Seed the global SOUL.md file if it does not already exist.
///
/// WHY: New users should have a SOUL.md file they can edit immediately.
/// We never overwrite an existing file — even an empty one.
/// This mirrors hermes-agent's auto-seed-if-missing behavior.
fn seed_global_soul(edgecrab_home: &Path) {
    let soul_path = edgecrab_home.join("SOUL.md");
    if soul_path.exists() {
        return;
    }
    // Ensure the directory exists before writing
    if let Err(e) = std::fs::create_dir_all(edgecrab_home) {
        tracing::warn!("Cannot create edgecrab home for SOUL.md seeding: {e}");
        return;
    }
    if let Err(e) = std::fs::write(&soul_path, DEFAULT_SOUL_MD) {
        tracing::warn!("Cannot seed SOUL.md: {e}");
    } else {
        tracing::info!("Seeded default SOUL.md at {}", soul_path.display());
    }
}

pub fn load_memory_sections(edgecrab_home: &Path) -> Vec<String> {
    let mut sections = Vec::new();
    let mem_dir = edgecrab_home.join("memories");

    if let Some(s) = load_memory_file(
        &mem_dir,
        "MEMORY.md",
        "MEMORY (your personal notes)",
        MEMORY_MAX_CHARS,
    ) {
        sections.push(s);
    }
    if let Some(s) = load_memory_file(&mem_dir, "USER.md", "USER PROFILE", USER_MAX_CHARS) {
        sections.push(s);
    }

    // Load Honcho user model — persistent cross-session observations.
    // WHY here: The user model is loaded alongside MEMORY.md / USER.md at
    // session start so the agent immediately knows the user's preferences,
    // projects, and communication style without needing to ask.
    if let Some(honcho_section) = edgecrab_tools::tools::honcho::load_honcho_user_context() {
        sections.push(honcho_section);
    }

    sections
}

// ─── Skill summary for system prompt ─────────────────────────────────

/// Scan `~/.edgecrab/skills/` and produce a summary of available skills
/// for injection into the system prompt.
///
/// WHY progressive disclosure: We only include skill names and short
/// descriptions (from YAML frontmatter) in the system prompt — not the
/// full skill content. The agent can use `skill_view` to load full
/// details on demand. This keeps prompt size manageable.
/// Load the full content of preloaded skills from `~/.edgecrab/skills/<name>/SKILL.md`.
///
/// Returns a formatted string containing each skill's full markdown content,
/// prefixed with a header. Returns an empty string when the list is empty or
/// no skill files are found.
pub fn load_preloaded_skills(edgecrab_home: &Path, skill_names: &[String]) -> String {
    if skill_names.is_empty() {
        return String::new();
    }
    let skills_dir = edgecrab_home.join("skills");
    let mut parts: Vec<String> = Vec::new();

    for name in skill_names {
        // Try both flat and optional-skills locations:
        // ~/.edgecrab/skills/<name>/SKILL.md
        // ~/.edgecrab/optional-skills/<name>/SKILL.md
        let candidates = [
            skills_dir.join(name).join("SKILL.md"),
            edgecrab_home
                .join("optional-skills")
                .join(name)
                .join("SKILL.md"),
        ];
        let mut loaded = false;
        for path in &candidates {
            if let Ok(content) = std::fs::read_to_string(path) {
                let stripped = strip_yaml_frontmatter(content.trim()).trim().to_string();
                if !stripped.is_empty() {
                    parts.push(format!("## Skill: {name}\n\n{stripped}"));
                    loaded = true;
                    break;
                }
            }
        }
        if !loaded {
            tracing::debug!("preloaded skill '{name}' not found in skills directories");
        }
    }

    if parts.is_empty() {
        return String::new();
    }
    format!(
        "# Preloaded Skills\n\nThe following skills are preloaded and active:\n\n{}",
        parts.join("\n\n---\n\n")
    )
}

///
/// Supports both flat (`skills/my-skill/SKILL.md`) and nested category
/// layouts (`skills/category/my-skill/SKILL.md`) matching hermes-agent.
///
/// # Arguments
/// * `edgecrab_home` — the `~/.edgecrab/` directory path
/// * `disabled_skills` — skill names to suppress globally (from config.skills.disabled)
/// * `available_tools` — optional list of available tool names for conditional activation
/// * `available_toolsets` — optional list of available toolset names for conditional activation
pub fn load_skill_summary(
    edgecrab_home: &Path,
    disabled_skills: &[String],
    available_tools: Option<&[String]>,
    available_toolsets: Option<&[String]>,
) -> Option<String> {
    // ── In-process cache hit (60-second TTL) ───────────────────────────
    // Only cache when there are no conditional filters (available_tools /
    // available_toolsets) because those are per-call and can vary.
    let can_cache = available_tools.is_none() && available_toolsets.is_none();
    let cache_key = edgecrab_home.to_path_buf();
    if can_cache {
        if let Ok(guard) = SKILLS_CACHE.lock() {
            if let Some(ref map) = *guard {
                if let Some(entry) = map.get(&cache_key) {
                    if entry.built_at.elapsed() < SKILLS_CACHE_TTL
                        && entry.disabled_at_build == disabled_skills
                    {
                        tracing::trace!("skills cache hit");
                        return entry.summary.clone();
                    }
                }
            }
        }
    }

    let result = load_skill_summary_inner(
        edgecrab_home,
        disabled_skills,
        available_tools,
        available_toolsets,
    );

    if can_cache {
        if let Ok(mut guard) = SKILLS_CACHE.lock() {
            let map = guard.get_or_insert_with(std::collections::HashMap::new);
            map.insert(
                cache_key,
                SkillsCacheEntry {
                    summary: result.clone(),
                    disabled_at_build: disabled_skills.to_vec(),
                    built_at: std::time::Instant::now(),
                },
            );
        }
    }
    result
}

/// Inner (uncached) implementation of the skill scanning logic.
fn load_skill_summary_inner(
    edgecrab_home: &Path,
    disabled_skills: &[String],
    available_tools: Option<&[String]>,
    available_toolsets: Option<&[String]>,
) -> Option<String> {
    let skills_dir = edgecrab_home.join("skills");
    if !skills_dir.is_dir() {
        return None;
    }

    // Detect the current OS platform for platform filtering
    let current_platform = std::env::consts::OS; // "macos" | "linux" | "windows"

    // Build a set of disabled skill names for fast lookup
    let disabled_set: std::collections::HashSet<&str> =
        disabled_skills.iter().map(|s| s.as_str()).collect();

    // Collect (category, name, description) tuples
    let mut skills: Vec<(Option<String>, String, String)> = Vec::new();

    // Recursive helper: scan a directory for skill folders (dirs containing SKILL.md)
    fn scan_dir(
        dir: &Path,
        category: Option<&str>,
        current_platform: &str,
        disabled_set: &std::collections::HashSet<&str>,
        available_tools: Option<&[String]>,
        available_toolsets: Option<&[String]>,
        skills: &mut Vec<(Option<String>, String, String)>,
    ) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            // Skip hidden/system dirs
            if name.starts_with('.') {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                // This directory is a skill
                let content = std::fs::read_to_string(&skill_md).unwrap_or_default();

                // Check platform compatibility from frontmatter
                if !skill_matches_platform(&content, current_platform) {
                    continue;
                }

                // Read frontmatter name (if present) for the disabled check
                let frontmatter_name = extract_frontmatter_name(&content);
                let display_name = frontmatter_name.as_deref().unwrap_or(&name);

                // Skip disabled skills
                if disabled_set.contains(name.as_str()) || disabled_set.contains(display_name) {
                    continue;
                }

                // Conditional activation: apply only when tool names are known
                if available_tools.is_some() || available_toolsets.is_some() {
                    let conditions = extract_skill_conditions(&content);
                    if !skill_should_show(&conditions, available_tools, available_toolsets) {
                        continue;
                    }
                }

                // Try SKILL.md frontmatter description first, fallback to DESCRIPTION.md
                let description = extract_skill_description(&content)
                    .or_else(|| {
                        // DESCRIPTION.md fallback: check in same skill dir
                        let desc_md = path.join("DESCRIPTION.md");
                        std::fs::read_to_string(&desc_md).ok().and_then(|d| {
                            extract_skill_description(&d).or_else(|| {
                                // If no frontmatter description, take the first non-empty,
                                // non-heading line from DESCRIPTION.md body
                                d.lines()
                                    .map(|l| l.trim())
                                    .find(|l| !l.is_empty() && !l.starts_with('#'))
                                    .map(|l| {
                                        if l.len() > 200 {
                                            format!("{}…", crate::safe_truncate(l, 197))
                                        } else {
                                            l.to_string()
                                        }
                                    })
                            })
                        })
                    })
                    .unwrap_or_default();

                skills.push((category.map(|s| s.to_string()), name, description));
            } else {
                // Not a skill — might be a category directory; recurse deeper.
                // Build nested category path (e.g. "mlops" → "mlops/training").
                let nested_cat = match category {
                    Some(cat) => format!("{cat}/{name}"),
                    None => name.clone(),
                };
                scan_dir(
                    &path,
                    Some(&nested_cat),
                    current_platform,
                    disabled_set,
                    available_tools,
                    available_toolsets,
                    skills,
                );
            }
        }
    }

    scan_dir(
        &skills_dir,
        None,
        current_platform,
        &disabled_set,
        available_tools,
        available_toolsets,
        &mut skills,
    );

    if skills.is_empty() {
        return None;
    }

    skills.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut output = String::from(
        "## Skills (mandatory)\n\
         Before replying, scan the skills below. If one clearly matches your task, \
         load it with skill_view(name) and follow its instructions. \
         If a skill has issues, fix it with skill_manage(action='edit') — don't wait to be asked.\n\
         After difficult/iterative tasks, save the approach as a skill. \
         If a skill you loaded was missing steps, had wrong commands, or needed \
         pitfalls you discovered, update it before finishing.\n\n\
         <available_skills>\n",
    );
    let mut current_category: Option<&str> = None;
    for (cat, name, desc) in &skills {
        // Emit category header when it changes
        let cat_str = cat.as_deref();
        if cat_str != current_category {
            if let Some(c) = cat_str {
                output.push_str(&format!("### {c}\n"));
            }
            current_category = cat_str;
        }
        if desc.is_empty() {
            output.push_str(&format!("- **{name}**\n"));
        } else {
            output.push_str(&format!("- **{name}**: {desc}\n"));
        }
    }
    output.push_str(
        "</available_skills>\n\nIf none match, proceed normally without loading a skill.",
    );
    Some(output)
}

/// Check if a skill is compatible with the current OS platform.
///
/// Reads the `platforms:` YAML frontmatter field (list of strings).
/// Recognized platform names: `macos`, `linux`, `windows`.
/// If the field is absent or empty, the skill is always included.
fn skill_matches_platform(skill_md_content: &str, current_os: &str) -> bool {
    let trimmed = skill_md_content.trim_start();
    if !trimmed.starts_with("---") {
        return true; // no frontmatter → always include
    }
    let after_first = &trimmed[3..];
    let end_pos = match after_first.find("\n---") {
        Some(p) => p,
        None => return true,
    };
    let frontmatter = &after_first[..end_pos];

    // Look for `platforms:` line — simple YAML list parsing
    // Accepts both inline: `platforms: [macos, linux]` and block list:
    //   platforms:
    //     - macos
    let mut platforms: Vec<String> = Vec::new();
    let mut in_platforms_block = false;

    for line in frontmatter.lines() {
        let trimmed_line = line.trim();

        if let Some(rest) = trimmed_line.strip_prefix("platforms:") {
            in_platforms_block = true;
            // Inline list: platforms: [macos, linux]
            let rest = rest.trim().trim_start_matches('[').trim_end_matches(']');
            if !rest.is_empty() {
                for item in rest.split(',') {
                    let v = item
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_lowercase();
                    if !v.is_empty() {
                        platforms.push(v);
                    }
                }
                in_platforms_block = false; // inline list is complete
            }
        } else if in_platforms_block {
            if let Some(item) = trimmed_line.strip_prefix("- ") {
                let v = item
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_lowercase();
                if !v.is_empty() {
                    platforms.push(v);
                }
            } else if !trimmed_line.is_empty() && !trimmed_line.starts_with('#') {
                in_platforms_block = false; // end of block list
            }
        }
    }

    if platforms.is_empty() {
        return true; // no restriction
    }

    // std::env::consts::OS values already use "macos", "linux", "windows"
    let canonical_os = current_os;

    platforms.iter().any(|p| p == canonical_os)
}

/// Extract the description field from YAML frontmatter in a skill file.
///
/// Looks for a `description:` line between `---` markers.
pub fn extract_skill_description(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_first = &trimmed[3..];
    let end_pos = after_first.find("\n---")?;
    let frontmatter = &after_first[..end_pos];

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("description:") {
            let desc = rest.trim().trim_matches('"').trim_matches('\'');
            if !desc.is_empty() {
                // Truncate long descriptions
                return Some(if desc.len() > 200 {
                    format!("{}…", crate::safe_truncate(desc, 197))
                } else {
                    desc.to_string()
                });
            }
        }
    }
    None
}

/// Extract the `name:` field from YAML frontmatter in a skill file.
pub fn extract_frontmatter_name(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_first = &trimmed[3..];
    let end_pos = after_first.find("\n---")?;
    let frontmatter = &after_first[..end_pos];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name:") {
            let n = rest.trim().trim_matches('"').trim_matches('\'');
            if !n.is_empty() {
                return Some(n.to_string());
            }
        }
    }
    None
}

/// Conditional activation fields from a skill's YAML frontmatter.
///
/// These mirror hermes-agent's skill condition system:
/// - `requires_tools`: skill only shows when ALL listed tool names are available
/// - `requires_toolsets`: skill only shows when ALL listed toolsets are available
/// - `fallback_for_tools`: skill is hidden when ANY of these tools are available (it's a fallback)
/// - `fallback_for_toolsets`: skill is hidden when ANY of these toolsets are available
#[derive(Debug, Default)]
pub struct SkillConditions {
    pub requires_tools: Vec<String>,
    pub requires_toolsets: Vec<String>,
    pub fallback_for_tools: Vec<String>,
    pub fallback_for_toolsets: Vec<String>,
}

/// Extract conditional activation fields from YAML frontmatter.
fn extract_skill_conditions(content: &str) -> SkillConditions {
    let mut cond = SkillConditions::default();
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return cond;
    }
    let after_first = &trimmed[3..];
    let end_pos = match after_first.find("\n---") {
        Some(p) => p,
        None => return cond,
    };
    let frontmatter = &after_first[..end_pos];

    // Helper: parse a list from a YAML key (inline or block)
    fn parse_yaml_list(frontmatter: &str, key: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut in_block = false;
        for line in frontmatter.lines() {
            let tl = line.trim();
            if let Some(rest) = tl.strip_prefix(key) {
                in_block = true;
                let rest = rest.trim().trim_start_matches('[').trim_end_matches(']');
                if !rest.is_empty() {
                    // inline list
                    for item in rest.split(',') {
                        let v = item.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !v.is_empty() {
                            result.push(v);
                        }
                    }
                    in_block = false;
                }
            } else if in_block {
                if let Some(item) = tl.strip_prefix("- ") {
                    let v = item.trim().trim_matches('"').trim_matches('\'').to_string();
                    if !v.is_empty() {
                        result.push(v);
                    }
                } else if !tl.is_empty() && !tl.starts_with('#') {
                    in_block = false;
                }
            }
        }
        result
    }

    cond.requires_tools = parse_yaml_list(frontmatter, "requires_tools:");
    cond.requires_toolsets = parse_yaml_list(frontmatter, "requires_toolsets:");
    cond.fallback_for_tools = parse_yaml_list(frontmatter, "fallback_for_tools:");
    cond.fallback_for_toolsets = parse_yaml_list(frontmatter, "fallback_for_toolsets:");
    cond
}

/// Determine whether a skill should be shown given its conditions and available tools.
///
/// Returns `false` (hide) when:
/// - A `fallback_for_tools` entry IS in the available tools (primary tool is present)
/// - A `fallback_for_toolsets` entry IS in the available toolsets
/// - A `requires_tools` entry is NOT in the available tools
/// - A `requires_toolsets` entry is NOT in the available toolsets
///
/// Returns `true` (show) in all other cases (including when tool info is unavailable).
fn skill_should_show(
    conditions: &SkillConditions,
    available_tools: Option<&[String]>,
    available_toolsets: Option<&[String]>,
) -> bool {
    let at = available_tools.unwrap_or(&[]);
    let ats = available_toolsets.unwrap_or(&[]);

    // fallback_for: hide when the primary tool/toolset IS available
    for t in &conditions.fallback_for_tools {
        if at.iter().any(|a| a == t) {
            return false;
        }
    }
    for ts in &conditions.fallback_for_toolsets {
        if ats.iter().any(|a| a == ts) {
            return false;
        }
    }

    // requires: hide when a required tool/toolset is NOT available
    for t in &conditions.requires_tools {
        if !at.is_empty() && !at.iter().any(|a| a == t) {
            return false;
        }
    }
    for ts in &conditions.requires_toolsets {
        if !ats.is_empty() && !ats.iter().any(|a| a == ts) {
            return false;
        }
    }

    true
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prompt_contains_identity() {
        let builder = PromptBuilder::new(Platform::Cli);
        let prompt = builder.build(None, None, &[], None);
        assert!(prompt.contains("EdgeCrab"));
        assert!(prompt.contains("Current date/time"));
    }

    #[test]
    fn override_identity() {
        let builder = PromptBuilder::new(Platform::Cli);
        let prompt = builder.build(Some("You are TestBot."), None, &[], None);
        assert!(prompt.contains("You are TestBot."));
        assert!(!prompt.contains("EdgeCrab"));
    }

    #[test]
    fn platform_hint_injected() {
        let builder = PromptBuilder::new(Platform::Telegram);
        let prompt = builder.build(None, None, &[], None);
        // Telegram hint tells agent not to use markdown and to use MEDIA:// protocol
        assert!(
            prompt.contains("MEDIA:"),
            "Telegram hint should mention MEDIA:// protocol"
        );
        assert!(
            prompt.contains("Telegram"),
            "Telegram hint should mention platform name"
        );
    }

    #[test]
    fn cli_hint_injected() {
        let builder = PromptBuilder::new(Platform::Cli);
        let prompt = builder.build(None, None, &[], None);
        assert!(prompt.contains("ANSI colors"));
    }

    #[test]
    fn memory_sections_included() {
        let builder = PromptBuilder::new(Platform::Cli);
        let mem = vec!["USER.md content here".into()];
        let prompt = builder.build(None, None, &mem, None);
        assert!(prompt.contains("persistent memory"));
        assert!(prompt.contains("USER.md content here"));
    }

    #[test]
    fn skill_prompt_included() {
        let builder = PromptBuilder::new(Platform::Cli);
        let prompt = builder.build(None, None, &[], Some("Use skill X for Y."));
        assert!(prompt.contains("Use skill X for Y."));
    }

    #[test]
    fn empty_skill_prompt_excluded() {
        let builder = PromptBuilder::new(Platform::Cli);
        let prompt = builder.build(None, None, &[], Some(""));
        // Empty string should not contribute blank lines
        let prompt_without_datetime = prompt
            .lines()
            .filter(|l| !l.starts_with("Current date/time"))
            .collect::<Vec<_>>()
            .join("\n");
        // Should not have triple newlines from empty section
        assert!(!prompt_without_datetime.contains("\n\n\n\n"));
    }

    #[test]
    fn skip_context_files_flag() {
        let builder = PromptBuilder::new(Platform::Cli).skip_context_files(true);
        let tmp = std::env::temp_dir();
        let prompt = builder.build(None, Some(&tmp), &[], None);
        // Should not contain any "---" file sections
        assert!(!prompt.contains("--- SOUL.md ---"));
    }

    #[test]
    fn context_file_discovery_with_temp_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("AGENTS.md"), "# My agents\ntest content").expect("write");

        let builder = PromptBuilder::new(Platform::Cli);
        let prompt = builder.build(None, Some(tmp.path()), &[], None);
        assert!(prompt.contains("AGENTS.md"));
        assert!(prompt.contains("test content"));
    }

    #[test]
    fn soul_file_not_in_context_files() {
        // SOUL.md is identity-only (loaded globally via load_global_soul).
        // It must NOT appear in discover_context_files — hermes does not load
        // SOUL.md from the CWD into context files.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("SOUL.md"), "soul content").expect("write");
        std::fs::write(tmp.path().join(".SOUL.md"), "dot soul content").expect("write");

        let files = discover_context_files(tmp.path());
        // Neither SOUL.md variant should appear in context files
        let soul = files
            .iter()
            .find(|(name, _)| name.to_lowercase().contains("soul"));
        assert!(
            soul.is_none(),
            "SOUL.md should not appear in context files: {soul:?}"
        );
    }

    #[test]
    fn hermes_md_wins_over_agents_md() {
        // .hermes.md has higher priority than AGENTS.md
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join(".hermes.md"), "hermes instructions").expect("write");
        std::fs::write(tmp.path().join("AGENTS.md"), "agents instructions").expect("write");

        let files = discover_context_files(tmp.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].1.contains("hermes instructions"));
        assert!(!files[0].1.contains("agents instructions"));
    }

    #[test]
    fn agents_md_hierarchical_walk() {
        // AGENTS.md files from subdirectories are collected
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("AGENTS.md"), "root agents").expect("write");
        let subdir = tmp.path().join("subdir");
        std::fs::create_dir(&subdir).expect("mkdir");
        std::fs::write(subdir.join("AGENTS.md"), "sub agents").expect("write");

        let files = discover_context_files(tmp.path());
        assert_eq!(files.len(), 2, "should find both AGENTS.md files");
        let combined: String = files
            .iter()
            .map(|(_, c)| c.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(combined.contains("root agents"));
        assert!(combined.contains("sub agents"));
    }

    #[test]
    fn agents_md_skips_hidden_dirs() {
        // Hidden subdirectories should not be walked for AGENTS.md
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("AGENTS.md"), "root agents").expect("write");
        let hidden = tmp.path().join(".hidden");
        std::fs::create_dir(&hidden).expect("mkdir");
        std::fs::write(hidden.join("AGENTS.md"), "hidden agents").expect("write");

        let files = discover_context_files(tmp.path());
        let combined: String = files
            .iter()
            .map(|(_, c)| c.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !combined.contains("hidden agents"),
            "content from hidden dirs should be skipped"
        );
    }

    #[test]
    fn load_skill_summary_flat_skills() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skill_dir = tmp.path().join("skills").join("my-skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\ndescription: Does something useful\n---\n# My Skill",
        )
        .expect("write");

        let summary = load_skill_summary(tmp.path(), &[], None, None).expect("Some");
        assert!(summary.contains("my-skill"), "skill name missing");
        assert!(
            summary.contains("Does something useful"),
            "description missing"
        );
        // Flat skill should NOT produce a category header
        assert!(
            !summary.contains("###"),
            "unexpected category header for flat skill"
        );
    }

    #[test]
    fn load_skill_summary_nested_categories() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // skills/coding/formatter/SKILL.md
        let formatter = tmp.path().join("skills").join("coding").join("formatter");
        std::fs::create_dir_all(&formatter).expect("mkdir");
        std::fs::write(
            formatter.join("SKILL.md"),
            "---\ndescription: Formats code\n---\n# Formatter",
        )
        .expect("write formatter");

        // skills/writing/proofreader/SKILL.md
        let proofreader = tmp
            .path()
            .join("skills")
            .join("writing")
            .join("proofreader");
        std::fs::create_dir_all(&proofreader).expect("mkdir");
        std::fs::write(
            proofreader.join("SKILL.md"),
            "---\ndescription: Checks grammar\n---\n# Proofreader",
        )
        .expect("write proofreader");

        let summary = load_skill_summary(tmp.path(), &[], None, None).expect("Some");

        // Category headers should appear
        assert!(
            summary.contains("### coding"),
            "missing coding header in:\n{summary}"
        );
        assert!(
            summary.contains("### writing"),
            "missing writing header in:\n{summary}"
        );

        // Skill names should appear
        assert!(
            summary.contains("formatter"),
            "missing formatter in:\n{summary}"
        );
        assert!(
            summary.contains("proofreader"),
            "missing proofreader in:\n{summary}"
        );

        // Descriptions should appear
        assert!(summary.contains("Formats code"), "missing formatter desc");
        assert!(
            summary.contains("Checks grammar"),
            "missing proofreader desc"
        );

        // coding should come before writing (alphabetical sort)
        let coding_pos = summary.find("coding").expect("coding pos");
        let writing_pos = summary.find("writing").expect("writing pos");
        assert!(
            coding_pos < writing_pos,
            "coding should sort before writing"
        );
    }

    #[test]
    fn load_skill_summary_deeply_nested() {
        // Test skills nested more than one level (e.g. mlops/training/axolotl)
        let tmp = tempfile::tempdir().expect("tempdir");
        let axolotl = tmp
            .path()
            .join("skills")
            .join("mlops")
            .join("training")
            .join("axolotl");
        std::fs::create_dir_all(&axolotl).expect("mkdir");
        std::fs::write(
            axolotl.join("SKILL.md"),
            "---\ndescription: Fine-tuning tool\n---\n# Axolotl",
        )
        .expect("write");

        let summary = load_skill_summary(tmp.path(), &[], None, None).expect("Some");
        assert!(
            summary.contains("axolotl"),
            "missing axolotl in:\n{summary}"
        );
        assert!(
            summary.contains("Fine-tuning tool"),
            "missing description in:\n{summary}"
        );
        // Should show nested category path
        assert!(
            summary.contains("mlops/training"),
            "missing nested category in:\n{summary}"
        );
    }

    #[test]
    fn load_skill_summary_returns_none_when_no_skills_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // No skills/ subdirectory exists
        assert!(load_skill_summary(tmp.path(), &[], None, None).is_none());
    }

    #[test]
    fn load_skill_summary_description_md_fallback() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skill_dir = tmp.path().join("skills").join("no-desc-skill");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        // SKILL.md has no description in frontmatter
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: no-desc-skill\n---\n# No Desc Skill",
        )
        .expect("write SKILL.md");
        // DESCRIPTION.md has a description
        std::fs::write(
            skill_dir.join("DESCRIPTION.md"),
            "---\ndescription: Fallback description from DESCRIPTION.md\n---\n",
        )
        .expect("write DESCRIPTION.md");

        let summary = load_skill_summary(tmp.path(), &[], None, None).expect("Some");
        assert!(summary.contains("no-desc-skill"), "skill name missing");
        assert!(
            summary.contains("Fallback description from DESCRIPTION.md"),
            "DESCRIPTION.md fallback not used:\n{summary}"
        );
    }

    #[test]
    fn skill_matches_platform_no_restriction_returns_true() {
        // No platforms field → always include
        assert!(skill_matches_platform(
            "---\nname: my-skill\n---\n",
            "macos"
        ));
        assert!(skill_matches_platform(
            "---\nname: my-skill\n---\n",
            "linux"
        ));
        assert!(skill_matches_platform("no frontmatter at all", "windows"));
    }

    #[test]
    fn skill_matches_platform_inline_list() {
        let content = "---\nplatforms: [macos, linux]\n---\n";
        assert!(skill_matches_platform(content, "macos"));
        assert!(skill_matches_platform(content, "linux"));
        assert!(!skill_matches_platform(content, "windows"));
    }

    #[test]
    fn skill_matches_platform_block_list() {
        let content = "---\nplatforms:\n  - linux\n  - windows\n---\n";
        assert!(skill_matches_platform(content, "linux"));
        assert!(skill_matches_platform(content, "windows"));
        assert!(!skill_matches_platform(content, "macos"));
    }

    #[test]
    fn load_skill_summary_platform_filtering() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // A skill for all platforms
        let skill_all = tmp.path().join("skills").join("all-platforms");
        std::fs::create_dir_all(&skill_all).expect("mkdir");
        std::fs::write(
            skill_all.join("SKILL.md"),
            "---\ndescription: Works everywhere\n---\n",
        )
        .expect("write");

        // A skill that only works on an OS that doesn't match any test host
        // (we test macos/linux/windows exclusion by using a fictitious OS)
        let skill_restricted = tmp.path().join("skills").join("macos-only");
        std::fs::create_dir_all(&skill_restricted).expect("mkdir");
        std::fs::write(
            skill_restricted.join("SKILL.md"),
            "---\nplatforms: [no-such-os]\ndescription: Never shown\n---\n",
        )
        .expect("write");

        let summary = load_skill_summary(tmp.path(), &[], None, None).expect("Some");
        assert!(
            summary.contains("all-platforms"),
            "all-platform skill should appear"
        );
        assert!(
            !summary.contains("macos-only"),
            "platform-restricted skill should not appear"
        );
        assert!(
            !summary.contains("Never shown"),
            "restricted skill desc should not appear"
        );
    }

    // ── Disabled-skill filtering tests ────────────────────────────────────

    #[test]
    fn load_skill_summary_disabled_skill_is_hidden() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // Create two skills
        for skill in ["active-skill", "disabled-skill"] {
            let dir = tmp.path().join("skills").join(skill);
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\ndescription: Description of {skill}\n---\n"),
            )
            .expect("write");
        }

        let disabled = vec!["disabled-skill".to_string()];
        let summary = load_skill_summary(tmp.path(), &disabled, None, None).expect("Some");

        assert!(
            summary.contains("active-skill"),
            "active skill should appear"
        );
        assert!(
            !summary.contains("disabled-skill"),
            "disabled skill should be hidden"
        );
        assert!(
            !summary.contains("Description of disabled-skill"),
            "disabled skill desc should be hidden"
        );
    }

    #[test]
    fn load_skill_summary_multiple_disabled_skills() {
        let tmp = tempfile::tempdir().expect("tempdir");

        for skill in ["skill-a", "skill-b", "skill-c"] {
            let dir = tmp.path().join("skills").join(skill);
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\ndescription: Desc of {skill}\n---\n"),
            )
            .expect("write");
        }

        let disabled = vec!["skill-a".to_string(), "skill-c".to_string()];
        let summary = load_skill_summary(tmp.path(), &disabled, None, None).expect("Some");

        assert!(summary.contains("skill-b"), "skill-b should appear");
        assert!(!summary.contains("skill-a"), "skill-a should be hidden");
        assert!(!summary.contains("skill-c"), "skill-c should be hidden");
    }

    #[test]
    fn load_skill_summary_empty_disabled_shows_all() {
        let tmp = tempfile::tempdir().expect("tempdir");

        for skill in ["skill-x", "skill-y"] {
            let dir = tmp.path().join("skills").join(skill);
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\ndescription: Desc of {skill}\n---\n"),
            )
            .expect("write");
        }

        // No disabled skills → all should appear
        let summary = load_skill_summary(tmp.path(), &[], None, None).expect("Some");
        assert!(summary.contains("skill-x"), "skill-x should appear");
        assert!(summary.contains("skill-y"), "skill-y should appear");
    }

    // ── Skills cache tests ─────────────────────────────────────────────

    #[test]
    fn skills_cache_different_homes_are_independent() {
        let tmp1 = tempfile::tempdir().expect("tempdir1");
        let tmp2 = tempfile::tempdir().expect("tempdir2");

        // Skill only in tmp1
        let dir1 = tmp1.path().join("skills").join("home1-skill");
        std::fs::create_dir_all(&dir1).expect("mkdir");
        std::fs::write(dir1.join("SKILL.md"), "---\ndescription: Home1 only\n---\n")
            .expect("write");

        // Skill only in tmp2
        let dir2 = tmp2.path().join("skills").join("home2-skill");
        std::fs::create_dir_all(&dir2).expect("mkdir");
        std::fs::write(dir2.join("SKILL.md"), "---\ndescription: Home2 only\n---\n")
            .expect("write");

        let summary1 = load_skill_summary(tmp1.path(), &[], None, None).expect("Some from tmp1");
        let summary2 = load_skill_summary(tmp2.path(), &[], None, None).expect("Some from tmp2");

        assert!(
            summary1.contains("home1-skill"),
            "tmp1 should see home1-skill"
        );
        assert!(
            !summary1.contains("home2-skill"),
            "tmp1 should not see home2-skill"
        );

        assert!(
            summary2.contains("home2-skill"),
            "tmp2 should see home2-skill"
        );
        assert!(
            !summary2.contains("home1-skill"),
            "tmp2 should not see home1-skill"
        );
    }

    #[test]
    fn scheduling_guidance_present_for_cli() {
        // Non-cron platforms should include SCHEDULING_GUIDANCE to prompt the LLM
        // to use manage_cron_jobs when users express scheduling intent.
        let builder = PromptBuilder::new(Platform::Cli).skip_context_files(true);
        let prompt = builder.build(None, None, &[], None);
        assert!(
            prompt.contains("manage_cron_jobs"),
            "CLI system prompt must include scheduling guidance with manage_cron_jobs reference"
        );
        assert!(
            prompt.contains("every morning") || prompt.contains("schedule"),
            "scheduling guidance should mention natural scheduling examples"
        );
    }

    #[test]
    fn scheduling_guidance_absent_for_cron_platform() {
        // Cron sessions are headless scheduled runs — the scheduling guidance is
        // irrelevant and must not appear (the cron recursion guard blocks the tool anyway).
        let builder = PromptBuilder::new(Platform::Cron).skip_context_files(true);
        let prompt = builder.build(None, None, &[], None);
        // CRON_HINT should be present (the cron platform hint)
        assert!(
            prompt.contains("scheduled cron job"),
            "cron system prompt must include CRON_HINT"
        );
        // SCHEDULING_GUIDANCE must NOT appear in cron sessions
        assert!(
            !prompt.contains("manage_cron_jobs(action='create')"),
            "cron system prompt must NOT include scheduling guidance"
        );
    }

    #[test]
    fn message_delivery_guidance_present_when_send_message_available() {
        let builder = PromptBuilder::new(Platform::Whatsapp)
            .skip_context_files(true)
            .available_tools(vec!["send_message".to_string()]);
        let prompt = builder.build(None, None, &[], None);
        assert!(
            prompt
                .contains("Use send_message only when the user explicitly wants content delivered"),
            "message delivery guidance must explain when cross-platform delivery should happen"
        );
        assert!(
            prompt.contains("Do not claim you cannot send messages when send_message is available"),
            "message delivery guidance must explicitly block false inability claims"
        );
        assert!(
            prompt.contains("redundant confirmation"),
            "message delivery guidance must discourage unnecessary send confirmations"
        );
    }

    #[test]
    fn message_delivery_guidance_absent_when_send_message_unavailable() {
        let builder = PromptBuilder::new(Platform::Cli)
            .skip_context_files(true)
            .available_tools(vec!["read_file".to_string()]);
        let prompt = builder.build(None, None, &[], None);
        assert!(
            !prompt
                .contains("Use send_message only when the user explicitly wants content delivered"),
            "message delivery guidance must not appear when send_message is unavailable"
        );
    }

    #[test]
    fn vision_guidance_present_when_vision_analyze_available() {
        // When vision_analyze is in the tool list, the system prompt must include
        // the vision tool decision rules so the model selects the right tool.
        let builder = PromptBuilder::new(Platform::Cli)
            .skip_context_files(true)
            .available_tools(vec![
                "vision_analyze".to_string(),
                "browser_vision".to_string(),
            ]);
        let prompt = builder.build(None, None, &[], None);
        assert!(
            prompt.contains("NEVER call browser_vision"),
            "vision guidance must include unambiguous rule against browser_vision for local files"
        );
        assert!(
            prompt.contains("vision_analyze"),
            "vision guidance must name vision_analyze as the correct tool"
        );
        assert!(
            prompt.contains("ATTACHED IMAGES"),
            "vision guidance must reference the ATTACHED IMAGES block marker"
        );
    }

    #[test]
    fn vision_guidance_absent_when_vision_analyze_not_available() {
        // Without vision_analyze in the tool list, the vision guidance block
        // must not be injected — it would be confusing and waste tokens.
        let builder = PromptBuilder::new(Platform::Cli)
            .skip_context_files(true)
            .available_tools(vec!["read_file".to_string(), "write_file".to_string()]);
        let prompt = builder.build(None, None, &[], None);
        assert!(
            !prompt.contains("NEVER call browser_vision"),
            "vision guidance must NOT appear when vision_analyze is not in tool list"
        );
    }
}
