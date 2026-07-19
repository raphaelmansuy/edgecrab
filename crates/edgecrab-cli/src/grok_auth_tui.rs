//! In-TUI SuperGrok OAuth — clean step UI (no stderr pollution).
//!
//! Best practices (GitHub CLI / device-flow UX):
//! - One primary action per step; secondary keys in help bar only
//! - Never dump raw query-string URLs as a single unbroken line without wrap
//! - Never clear the host terminal from OAuth side-effects
//! - Mask secrets; show status chips (browser · code ready · saving)
//! - Progress rail: Open → Paste → Done

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::auth_cmd;
use crate::proxy_hub::PROXY_ACCENT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrokAuthScreen {
    /// Open x.ai and save PKCE session (~30 min).
    Start,
    /// Submit code (clipboard or terminal readline — no in-TUI text box).
    Finish,
    /// Brief success before close.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrokAuthAction {
    None,
    Close,
    /// Run silent `start_xai_oauth_login` (TUI owns browser + messaging).
    RunStart,
    /// Load clipboard into `pending_code`, then user presses Enter again.
    LoadClipboard,
    /// Submit: clipboard → pending → suspended readline (in that order).
    RunFinish,
    /// Open authorize URL in the system browser.
    OpenBrowser,
}

pub struct GrokAuthTui {
    pub active: bool,
    pub screen: GrokAuthScreen,
    pub busy: bool,
    /// When token exchange started (for elapsed UI + watchdog).
    pub busy_started: Option<std::time::Instant>,
    /// Abort in-flight finish task (Esc while exchanging).
    pub finish_abort: Option<tokio::task::AbortHandle>,
    pub error: Option<String>,
    pub authorize_url: Option<String>,
    pub pending_path: Option<PathBuf>,
    pub success_message: Option<String>,
    /// Normalized code ready to exchange (set by `p` or clipboard on Enter).
    pub pending_code: Option<String>,
    /// Browser was launched successfully after step 1.
    pub browser_opened: bool,
    pub(crate) no_browser: bool,
    /// Saved path for full authorize URL (fallback when hyperlinks fail).
    pub saved_url_path: Option<PathBuf>,
}

impl GrokAuthTui {
    pub fn new() -> Self {
        Self {
            active: false,
            screen: GrokAuthScreen::Start,
            busy: false,
            busy_started: None,
            finish_abort: None,
            error: None,
            authorize_url: None,
            pending_path: None,
            success_message: None,
            pending_code: None,
            browser_opened: false,
            no_browser: false,
            saved_url_path: None,
        }
    }

    pub fn open(&mut self, screen: GrokAuthScreen) {
        self.abort_finish();
        self.busy = false;
        self.busy_started = None;
        self.error = None;
        self.success_message = None;
        self.pending_code = None;
        self.no_browser = false;
        self.browser_opened = false;
        self.saved_url_path = None;

        if let Some((url, path)) = auth_cmd::grok_load_valid_pending()
            && matches!(screen, GrokAuthScreen::Finish | GrokAuthScreen::Start)
        {
            self.screen = GrokAuthScreen::Finish;
            self.pending_path = Some(path);
            self.authorize_url = Some(url.clone());
            self.saved_url_path = auth_cmd::persist_last_oauth_url_silent(&url);
        } else {
            self.screen = screen;
            self.authorize_url = None;
            self.pending_path = None;
        }

        self.active = true;
    }

    pub fn close(&mut self) {
        self.abort_finish();
        self.active = false;
        self.busy = false;
        self.busy_started = None;
        self.pending_code = None;
        self.browser_opened = false;
    }

    pub fn abort_finish(&mut self) {
        if let Some(h) = self.finish_abort.take() {
            h.abort();
        }
        self.busy = false;
        self.busy_started = None;
    }

    pub fn begin_finish_busy(&mut self, abort: tokio::task::AbortHandle) {
        self.busy = true;
        self.busy_started = Some(std::time::Instant::now());
        self.finish_abort = Some(abort);
        self.error = None;
    }

    pub fn set_start_result(
        &mut self,
        authorize_url: String,
        pending_path: PathBuf,
        browser_opened: bool,
    ) {
        self.abort_finish();
        self.saved_url_path = auth_cmd::persist_last_oauth_url_silent(&authorize_url);
        self.authorize_url = Some(authorize_url);
        self.pending_path = Some(pending_path);
        self.browser_opened = browser_opened;
        self.screen = GrokAuthScreen::Finish;
        self.busy = false;
        self.busy_started = None;
        self.error = None;
        self.pending_code = None;
    }

    pub fn set_finish_success(&mut self, message: String) {
        self.finish_abort = None;
        self.success_message = Some(message);
        self.screen = GrokAuthScreen::Done;
        self.busy = false;
        self.busy_started = None;
        self.error = None;
        self.pending_code = None;
    }

    pub fn set_error(&mut self, message: String) {
        self.finish_abort = None;
        self.error = Some(message);
        self.busy = false;
        self.busy_started = None;
    }

    pub fn set_pending_code(&mut self, code: String) {
        self.pending_code = Some(code);
        self.error = None;
    }

    pub fn title(&self) -> &'static str {
        match self.screen {
            GrokAuthScreen::Start => " SuperGrok sign-in ",
            GrokAuthScreen::Finish => " SuperGrok sign-in · paste code ",
            GrokAuthScreen::Done => " SuperGrok · signed in ",
        }
    }

    pub fn subtitle(&self) -> &'static str {
        "SuperGrok / X Premium+  ·  OAuth (one-time)"
    }

    /// Body lines for a given content width (wrap long URLs ourselves).
    pub fn body_lines(&self, width: u16) -> Vec<Line<'static>> {
        let w = (width as usize).saturating_sub(4).max(24);
        let mut lines = Vec::new();

        lines.extend(progress_rail(self.screen));
        lines.push(Line::from(""));

        if self.busy {
            let elapsed = self
                .busy_started
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            let msg = match self.screen {
                GrokAuthScreen::Start => format!("  Starting secure sign-in… ({elapsed}s)"),
                GrokAuthScreen::Finish => format!(
                    "  Exchanging code for tokens… ({elapsed}s)  ·  Esc cancels"
                ),
                GrokAuthScreen::Done => "  …".into(),
            };
            lines.push(Line::from(Span::styled(
                msg,
                Style::default()
                    .fg(Color::Rgb(255, 200, 120))
                    .add_modifier(Modifier::ITALIC),
            )));
            if elapsed >= 20 {
                lines.push(Line::from(Span::styled(
                    "  Still waiting on x.ai — network or invalid code. Esc to cancel and retry.",
                    Style::default().fg(Color::Rgb(255, 180, 100)),
                )));
            }
            lines.push(Line::from(""));
        }

        if let Some(ref err) = self.error {
            lines.push(Line::from(Span::styled(
                "  ⚠  Problem",
                Style::default()
                    .fg(Color::Rgb(239, 83, 80))
                    .add_modifier(Modifier::BOLD),
            )));
            for part in wrap_text(err, w.saturating_sub(4)) {
                lines.push(Line::from(Span::styled(
                    format!("     {part}"),
                    Style::default().fg(Color::Rgb(239, 83, 80)),
                )));
            }
            lines.push(Line::from(""));
        }

        match self.screen {
            GrokAuthScreen::Start => {
                lines.extend(start_body(w));
            }
            GrokAuthScreen::Finish => {
                lines.extend(finish_body(
                    w,
                    self.authorize_url.as_deref(),
                    self.saved_url_path.as_ref(),
                    self.browser_opened,
                    self.pending_code.as_deref(),
                ));
            }
            GrokAuthScreen::Done => {
                lines.extend(done_body(w, self.success_message.as_deref()));
            }
        }

        lines
    }

    pub fn help_line(&self) -> Line<'static> {
        if self.busy {
            return Line::from(Span::styled(
                " Please wait… ",
                Style::default().fg(Color::DarkGray),
            ));
        }
        let hint = match self.screen {
            GrokAuthScreen::Start => " Enter — open x.ai & continue  ·  Esc — cancel ",
            GrokAuthScreen::Finish => {
                " Enter — submit code  ·  p — clipboard  ·  o — reopen browser  ·  Esc — cancel "
            }
            GrokAuthScreen::Done => " Enter / Esc — close ",
        };
        Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray)))
    }

    /// Enter / Ctrl+J / Ctrl+M / bare CR·LF — submit primary action.
    ///
    /// WHY ignore Shift/Super on Enter: VS Code and some PTYs attach extra
    /// modifier bits; treating only "pure" Enter as submit made "Press Enter
    /// to save tokens" look dead while `[code ready]` was already set.
    pub fn is_submit_key(key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => true,
            KeyCode::Char('\r' | '\n') => true,
            KeyCode::Char('j' | 'J' | 'm' | 'M')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                true
            }
            _ => false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> GrokAuthAction {
        // Allow Esc to cancel an in-flight token exchange (otherwise busy forever).
        if self.busy {
            if key.code == KeyCode::Esc {
                return GrokAuthAction::Close;
            }
            return GrokAuthAction::None;
        }

        // Submit must win over the Ctrl/Alt filter (Ctrl+J is a submit alias).
        match self.screen {
            GrokAuthScreen::Done => {
                if Self::is_submit_key(&key) || key.code == KeyCode::Esc {
                    return GrokAuthAction::Close;
                }
                return GrokAuthAction::None;
            }
            GrokAuthScreen::Start => {
                if key.code == KeyCode::Esc {
                    return GrokAuthAction::Close;
                }
                if Self::is_submit_key(&key) {
                    return GrokAuthAction::RunStart;
                }
                if matches!(key.code, KeyCode::Char('o' | 'O')) && self.authorize_url.is_some() {
                    return GrokAuthAction::OpenBrowser;
                }
                return GrokAuthAction::None;
            }
            GrokAuthScreen::Finish => {
                if Self::is_submit_key(&key) {
                    return GrokAuthAction::RunFinish;
                }
                if key.code == KeyCode::Esc {
                    return GrokAuthAction::Close;
                }
                // Ignore Ctrl/Alt chords for non-submit keys.
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return GrokAuthAction::None;
                }
                return match key.code {
                    KeyCode::Char('p' | 'P') => GrokAuthAction::LoadClipboard,
                    KeyCode::Char('o' | 'O') => GrokAuthAction::OpenBrowser,
                    _ => GrokAuthAction::None,
                };
            }
        }
    }
}

// ── Layout helpers ───────────────────────────────────────────────────────────

fn progress_rail(screen: GrokAuthScreen) -> Vec<Line<'static>> {
    let (a, b, c) = match screen {
        GrokAuthScreen::Start => (true, false, false),
        GrokAuthScreen::Finish => (true, true, false),
        GrokAuthScreen::Done => (true, true, true),
    };
    let step = |done: bool, active: bool, label: &str| -> Span<'static> {
        let (mark, style) = if done && !active {
            (
                "✓",
                Style::default().fg(Color::Rgb(140, 220, 160)),
            )
        } else if active {
            (
                "●",
                Style::default()
                    .fg(PROXY_ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ("○", Style::default().fg(Color::DarkGray))
        };
        Span::styled(format!("{mark} {label}"), style)
    };
    let sep = Span::styled("  →  ", Style::default().fg(Color::DarkGray));
    vec![Line::from(vec![
        Span::raw("  "),
        step(a, matches!(screen, GrokAuthScreen::Start), "Open x.ai"),
        sep.clone(),
        step(
            b && !matches!(screen, GrokAuthScreen::Finish),
            matches!(screen, GrokAuthScreen::Finish),
            "Paste code",
        ),
        sep,
        step(c, matches!(screen, GrokAuthScreen::Done), "Done"),
    ])]
}

fn start_body(width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            "  What happens next",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  1. EdgeCrab opens x.ai in your browser."),
        Line::from("  2. Sign in with SuperGrok / X Premium+."),
        Line::from("  3. If x.ai shows \"Could not establish connection\", that is OK —"),
        Line::from("     copy the long authorization code on that page (not the URL)."),
        Line::from("  4. Return here and paste the code (step 2)."),
        Line::from(""),
    ];
    for part in wrap_text(
        "Credentials inject automatically after success — no restart. If you picked SuperGrok in /model, the switch resumes for you.",
        width.saturating_sub(2),
    ) {
        lines.push(Line::from(Span::styled(
            format!("  {part}"),
            Style::default().fg(Color::Rgb(140, 220, 160)),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press Enter to continue.",
        Style::default()
            .fg(PROXY_ACCENT)
            .add_modifier(Modifier::BOLD),
    )));
    lines
}

fn finish_body(
    width: usize,
    authorize_url: Option<&str>,
    saved_url_path: Option<&PathBuf>,
    browser_opened: bool,
    pending_code: Option<&str>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Status chips
    let mut chips = Vec::new();
    if browser_opened {
        chips.push(chip("browser opened", Color::Rgb(140, 220, 160)));
    } else {
        chips.push(chip("press o if browser did not open", Color::Rgb(255, 200, 120)));
    }
    if pending_code.is_some() {
        chips.push(chip("code ready", Color::Rgb(140, 220, 160)));
    }
    lines.push(Line::from({
        let mut spans = vec![Span::raw("  ")];
        for (i, c) in chips.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(c);
        }
        spans
    }));
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "  Primary action",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from("  1. Copy the authorization code from the x.ai page."));
    lines.push(Line::from(
        "  2. Press p to load clipboard  —  or  Enter to paste at a terminal prompt.",
    ));
    lines.push(Line::from("  3. Enter again to exchange the code (if already loaded)."));
    lines.push(Line::from(""));

    if let Some(code) = pending_code {
        lines.push(Line::from(Span::styled(
            format!("  Code ready: {}", auth_cmd::mask_grok_code(code)),
            Style::default()
                .fg(Color::Rgb(140, 220, 160))
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "  Press Enter to save tokens.",
            Style::default().fg(PROXY_ACCENT),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "  Sign-in URL (if you need it)",
        Style::default().fg(Color::DarkGray),
    )));
    if let Some(url) = authorize_url {
        // Host-only summary first — full query string is rarely useful to read.
        if let Some(host) = url_host_summary(url) {
            lines.push(Line::from(Span::styled(
                format!("  {host}"),
                Style::default().fg(Color::Rgb(160, 180, 200)),
            )));
        }
        for chunk in wrap_url(url, width.saturating_sub(4)) {
            lines.push(Line::from(Span::styled(
                format!("  {chunk}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  (URL not available — press o after start, or Esc and /login grok again)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(path) = saved_url_path {
        lines.push(Line::from(Span::styled(
            format!("  Saved: {}", path.display()),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Tip: paste the code only — never the full callback URL with secrets.",
        Style::default().fg(Color::Rgb(255, 200, 120)),
    )));

    lines
}

fn done_body(width: usize, success: Option<&str>) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            "  ✓  SuperGrok is ready for this session",
            Style::default()
                .fg(Color::Rgb(140, 220, 160))
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    let msg = success.unwrap_or("OAuth tokens saved to ~/.edgecrab/auth.json");
    for part in wrap_text(msg, width.saturating_sub(2)) {
        lines.push(Line::from(Span::styled(
            format!("  {part}"),
            Style::default().fg(Color::Rgb(140, 220, 160)),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("  Press Enter to close."));
    lines
}

fn chip(label: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!("[{label}]"),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn url_host_summary(url: &str) -> Option<String> {
    // https://accounts.x.ai/oauth2/authorize?... → accounts.x.ai · OAuth authorize
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split('/').next().unwrap_or(rest);
    Some(format!("{host}  ·  OAuth authorize"))
}

/// Hard-wrap a URL on safe boundaries (never dump OSC-8 or mid-control mess).
fn wrap_url(url: &str, width: usize) -> Vec<String> {
    let width = width.max(16);
    let mut out = Vec::new();
    let mut rest = url;
    while !rest.is_empty() {
        if rest.len() <= width {
            out.push(rest.to_string());
            break;
        }
        // Prefer splitting after & or = for readability.
        let window = &rest[..width];
        let split = window
            .rfind(['&', '=', '?', '/'])
            .filter(|&i| i > width / 3)
            .unwrap_or(width);
        out.push(rest[..split].to_string());
        rest = &rest[split..];
    }
    out
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(12);
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if line.is_empty() {
                line = word.to_string();
            } else if line.len() + 1 + word.len() <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_url_splits_long_query() {
        let url = "https://accounts.x.ai/oauth2/authorize?response_type=code&client_id=abc&redirect_uri=http%3A%2F%2F127.0.0.1%3A56121%2Fcallback&scope=openid";
        let lines = wrap_url(url, 40);
        assert!(lines.len() >= 2, "{lines:?}");
        assert!(lines.iter().all(|l| l.len() <= 40), "{lines:?}");
        assert_eq!(lines.concat(), url);
    }

    #[test]
    fn progress_rail_marks_finish_step() {
        let lines = progress_rail(GrokAuthScreen::Finish);
        let s = lines[0].spans.iter().map(|sp| sp.content.as_ref()).collect::<String>();
        assert!(s.contains("Paste code"), "{s}");
        assert!(s.contains("Open x.ai"), "{s}");
    }

    #[test]
    fn body_finish_shows_host_not_only_raw_blob() {
        let mut t = GrokAuthTui::new();
        t.screen = GrokAuthScreen::Finish;
        t.authorize_url = Some(
            "https://accounts.x.ai/oauth2/authorize?response_type=code&client_id=test".into(),
        );
        t.browser_opened = true;
        let body = t.body_lines(80);
        let joined: String = body
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("accounts.x.ai"), "{joined}");
        assert!(joined.contains("browser opened") || joined.contains("[browser"), "{joined}");
    }

    #[test]
    fn wrap_text_breaks_on_words() {
        let lines = wrap_text("hello world from edgecrab oauth experience", 12);
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|l| l.len() <= 12 || !l.contains(' ')));
    }

    #[test]
    fn enter_with_shift_still_submits_on_finish() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut t = GrokAuthTui::new();
        t.screen = GrokAuthScreen::Finish;
        t.pending_code = Some("test-code".into());
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(t.handle_key(key), GrokAuthAction::RunFinish);
    }

    #[test]
    fn bare_enter_submits_when_code_ready() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut t = GrokAuthTui::new();
        t.screen = GrokAuthScreen::Finish;
        t.set_pending_code("abc".into());
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(t.handle_key(key), GrokAuthAction::RunFinish);
    }

    #[test]
    fn busy_swallows_enter() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut t = GrokAuthTui::new();
        t.screen = GrokAuthScreen::Finish;
        t.busy = true;
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(t.handle_key(key), GrokAuthAction::None);
    }
}
