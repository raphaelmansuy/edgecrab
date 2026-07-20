//! `/mcp add` — interactive URL wizard with OAuth discovery (RFC 9728).
//!
//! UX mirrors SuperGrok OAuth overlay:
//! - Progress rail (URL → Discover → Confirm → Done)
//! - Bracketed paste + clipboard (`p` when field empty)
//! - One primary Enter action per step; secrets never dumped into yaml

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edgecrab_tools::mcp_auth::DiscoveredMcpOauth;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub const MCP_ADD_ACCENT: Color = Color::Rgb(100, 180, 220);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpAddScreen {
    Url,
    Name,
    Discovering,
    Confirm,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpAddAction {
    None,
    Redraw,
    Close,
    /// Read system clipboard into the active field.
    LoadClipboard,
    /// Begin async OAuth discovery for `url`.
    StartDiscover,
    /// Persist discovered OAuth config.
    Save,
    /// Persist then run `mcp login` for the saved name.
    SaveAndLogin,
    /// Run login for already-saved server.
    Login,
}

#[derive(Debug, Clone)]
pub enum DiscoverySlot {
    Idle,
    Pending,
    Ready(Box<Result<DiscoveredMcpOauth, String>>),
}

/// Draft state for URL-only MCP add.
pub struct McpAddTui {
    pub active: bool,
    pub screen: McpAddScreen,
    pub url: String,
    pub name: String,
    pub allow_loopback: bool,
    pub toast: Option<String>,
    pub error: Option<String>,
    pub discovered: Option<DiscoveredMcpOauth>,
    pub saved_name: Option<String>,
    pub needs_login: bool,
    /// True after a successful paste/clipboard load on the URL step.
    pub url_from_paste: bool,
    pub discovery: Arc<Mutex<DiscoverySlot>>,
    config_path: PathBuf,
}

impl McpAddTui {
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            active: false,
            screen: McpAddScreen::Url,
            url: String::new(),
            name: String::new(),
            allow_loopback: false,
            toast: None,
            error: None,
            discovered: None,
            saved_name: None,
            needs_login: false,
            url_from_paste: false,
            discovery: Arc::new(Mutex::new(DiscoverySlot::Idle)),
            config_path,
        }
    }

    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }

    pub fn open(&mut self) {
        self.screen = McpAddScreen::Url;
        self.url.clear();
        self.name.clear();
        self.allow_loopback = false;
        self.toast = None;
        self.error = None;
        self.discovered = None;
        self.saved_name = None;
        self.needs_login = false;
        self.url_from_paste = false;
        if let Ok(mut slot) = self.discovery.lock() {
            *slot = DiscoverySlot::Idle;
        }
        self.active = true;
    }

    pub fn close(&mut self) {
        self.active = false;
        self.screen = McpAddScreen::Url;
        if let Ok(mut slot) = self.discovery.lock() {
            *slot = DiscoverySlot::Idle;
        }
    }

    pub fn discovery_slot(&self) -> Arc<Mutex<DiscoverySlot>> {
        Arc::clone(&self.discovery)
    }

    /// Accept bracketed-paste or clipboard text into the active editable field.
    pub fn apply_paste(&mut self, text: &str) {
        let cleaned = normalize_pasted_url(text);
        if cleaned.is_empty() {
            self.toast = Some("Clipboard/paste was empty.".into());
            return;
        }
        match self.screen {
            McpAddScreen::Url => {
                self.url = cleaned;
                self.url_from_paste = true;
                self.toast = Some("URL loaded — press Enter to continue.".into());
            }
            McpAddScreen::Name => {
                // Name paste: first token / slug only
                self.name =
                    edgecrab_tools::mcp_auth::suggest_server_name(Some(&cleaned), &self.url);
                self.toast = Some("Name loaded — press Enter to discover.".into());
            }
            _ => {
                self.toast = Some("Paste is only used on the URL / name steps.".into());
            }
        }
    }

    /// Poll background discovery while on Discovering screen.
    pub fn poll_discovery(&mut self) -> bool {
        if self.screen != McpAddScreen::Discovering {
            return false;
        }
        let ready = {
            let Ok(slot) = self.discovery.lock() else {
                return false;
            };
            matches!(&*slot, DiscoverySlot::Ready(_))
        };
        if !ready {
            return false;
        }
        let result = {
            let Ok(mut slot) = self.discovery.lock() else {
                return false;
            };
            match std::mem::replace(&mut *slot, DiscoverySlot::Idle) {
                DiscoverySlot::Ready(r) => *r,
                other => {
                    *slot = other;
                    return false;
                }
            }
        };
        match result {
            Ok(discovered) => {
                if self.name.trim().is_empty() {
                    self.name = edgecrab_tools::mcp_auth::suggest_server_name(
                        discovered.resource_name.as_deref(),
                        &self.url,
                    );
                }
                self.discovered = Some(discovered);
                self.screen = McpAddScreen::Confirm;
                self.toast = Some("Discovery complete — review and save.".into());
            }
            Err(err) => {
                self.error = Some(err);
                self.screen = McpAddScreen::Error;
            }
        }
        true
    }

    /// Enter / Ctrl+J / Ctrl+M — same submit aliases as SuperGrok overlay.
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

    pub fn handle_key(&mut self, key: KeyEvent) -> McpAddAction {
        // Submit aliases must win over the Ctrl filter (Ctrl+J).
        if Self::is_submit_key(&key) {
            return match self.screen {
                McpAddScreen::Url => self.submit_url(),
                McpAddScreen::Name => self.submit_name(),
                McpAddScreen::Confirm => McpAddAction::Save,
                McpAddScreen::Done => McpAddAction::Close,
                McpAddScreen::Error => {
                    self.error = None;
                    self.screen = McpAddScreen::Url;
                    McpAddAction::Redraw
                }
                McpAddScreen::Discovering => McpAddAction::None,
            };
        }

        if key.code == KeyCode::Esc {
            return match self.screen {
                McpAddScreen::Url | McpAddScreen::Done => McpAddAction::Close,
                McpAddScreen::Name => {
                    self.screen = McpAddScreen::Url;
                    McpAddAction::Redraw
                }
                McpAddScreen::Discovering => {
                    if let Ok(mut slot) = self.discovery.lock() {
                        *slot = DiscoverySlot::Idle;
                    }
                    self.screen = McpAddScreen::Name;
                    self.toast = Some("Discovery cancelled.".into());
                    McpAddAction::Redraw
                }
                McpAddScreen::Confirm => {
                    self.screen = McpAddScreen::Name;
                    McpAddAction::Redraw
                }
                McpAddScreen::Error => McpAddAction::Close,
            };
        }

        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return McpAddAction::None;
        }

        match self.screen {
            McpAddScreen::Url => self.handle_url_key(key),
            McpAddScreen::Name => self.handle_name_key(key),
            McpAddScreen::Discovering => McpAddAction::None,
            McpAddScreen::Confirm => self.handle_confirm_key(key),
            McpAddScreen::Done => self.handle_done_key(key),
            McpAddScreen::Error => self.handle_error_key(key),
        }
    }

    fn submit_url(&mut self) -> McpAddAction {
        let url = self.url.trim().to_string();
        if url.is_empty() {
            // Empty + Enter → load clipboard (SuperGrok-style primary action).
            return McpAddAction::LoadClipboard;
        }
        if let Err(err) = crate::mcp_register::validate_mcp_http_url(&url, self.allow_loopback) {
            self.toast = Some(err);
            return McpAddAction::Redraw;
        }
        self.url = url;
        if self.name.trim().is_empty() {
            self.name = edgecrab_tools::mcp_auth::suggest_server_name(None, &self.url);
        }
        self.screen = McpAddScreen::Name;
        self.toast = Some("Confirm the config name, then Enter to discover OAuth.".into());
        McpAddAction::Redraw
    }

    fn submit_name(&mut self) -> McpAddAction {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            self.toast = Some("Server name is required.".into());
            return McpAddAction::Redraw;
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            self.toast = Some("Invalid name (use letters, digits, '-', '_', '.')".into());
            return McpAddAction::Redraw;
        }
        self.name = name;
        self.screen = McpAddScreen::Discovering;
        self.toast = Some("Discovering OAuth…".into());
        if let Ok(mut slot) = self.discovery.lock() {
            *slot = DiscoverySlot::Pending;
        }
        McpAddAction::StartDiscover
    }

    fn handle_url_key(&mut self, key: KeyEvent) -> McpAddAction {
        match key.code {
            KeyCode::Tab => {
                self.allow_loopback = !self.allow_loopback;
                McpAddAction::Redraw
            }
            // `p` loads clipboard only when the field is empty (avoids eating 'p' in URLs).
            KeyCode::Char('p' | 'P') if self.url.is_empty() => McpAddAction::LoadClipboard,
            KeyCode::Backspace => {
                self.url.pop();
                self.url_from_paste = false;
                McpAddAction::Redraw
            }
            KeyCode::Char(c) => {
                self.url.push(c);
                self.url_from_paste = false;
                McpAddAction::Redraw
            }
            _ => McpAddAction::None,
        }
    }

    fn handle_name_key(&mut self, key: KeyEvent) -> McpAddAction {
        match key.code {
            KeyCode::Char('p' | 'P') if self.name.is_empty() => McpAddAction::LoadClipboard,
            KeyCode::Backspace => {
                self.name.pop();
                McpAddAction::Redraw
            }
            KeyCode::Char(c) => {
                self.name.push(c);
                McpAddAction::Redraw
            }
            _ => McpAddAction::None,
        }
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) -> McpAddAction {
        match key.code {
            KeyCode::Char('s' | 'S') => McpAddAction::Save,
            KeyCode::Char('l' | 'L') => McpAddAction::SaveAndLogin,
            _ => McpAddAction::None,
        }
    }

    fn handle_done_key(&mut self, key: KeyEvent) -> McpAddAction {
        match key.code {
            KeyCode::Char('l' | 'L') if self.needs_login => McpAddAction::Login,
            _ => McpAddAction::None,
        }
    }

    fn handle_error_key(&mut self, key: KeyEvent) -> McpAddAction {
        match key.code {
            KeyCode::Char('r' | 'R') => {
                self.error = None;
                self.screen = McpAddScreen::Url;
                McpAddAction::Redraw
            }
            _ => McpAddAction::None,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.screen {
            McpAddScreen::Url => " Add MCP server ",
            McpAddScreen::Name => " Add MCP · name ",
            McpAddScreen::Discovering => " Add MCP · discovering ",
            McpAddScreen::Confirm => " Add MCP · confirm OAuth ",
            McpAddScreen::Done => " Add MCP · saved ",
            McpAddScreen::Error => " Add MCP · error ",
        }
    }

    pub fn subtitle(&self) -> &'static str {
        "HTTP MCP  ·  OAuth discovered automatically"
    }

    pub fn help_line(&self) -> Line<'static> {
        let text = match self.screen {
            McpAddScreen::Url => {
                " Enter continue  ·  paste / p clipboard  ·  Tab loopback  ·  Esc cancel "
            }
            McpAddScreen::Name => " Enter discover  ·  Esc back ",
            McpAddScreen::Discovering => " Esc cancel ",
            McpAddScreen::Confirm => " Enter/s save  ·  l save+login  ·  Esc back ",
            McpAddScreen::Done => {
                if self.needs_login {
                    " l login  ·  Enter/Esc close "
                } else {
                    " Enter/Esc close "
                }
            }
            McpAddScreen::Error => " Enter/r retry  ·  Esc close ",
        };
        Line::from(Span::styled(text, Style::default().fg(Color::DarkGray)))
    }

    pub fn body_lines(&self, width: u16) -> Vec<Line<'static>> {
        let w = (width as usize).saturating_sub(4).max(24);
        let mut lines = Vec::new();
        lines.extend(progress_rail(self.screen));
        lines.push(Line::from(""));

        match self.screen {
            McpAddScreen::Url => {
                lines.push(Line::from(Span::styled(
                    "  What happens next",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(
                    "  1. Paste (or type) the HTTPS MCP endpoint URL.",
                ));
                lines.push(Line::from(
                    "  2. EdgeCrab discovers OAuth (PRM → AS → client registration).",
                ));
                lines.push(Line::from(
                    "  3. Review, save, then complete browser login if needed.",
                ));
                lines.push(Line::from(""));

                // Status chips
                let mut chips = vec![Span::raw("  ")];
                if self.url_from_paste && !self.url.is_empty() {
                    chips.push(chip("url ready", Color::Rgb(140, 220, 160)));
                    chips.push(Span::raw("  "));
                }
                chips.push(chip(
                    if self.allow_loopback {
                        "loopback on"
                    } else {
                        "loopback off"
                    },
                    if self.allow_loopback {
                        Color::Rgb(255, 200, 120)
                    } else {
                        Color::DarkGray
                    },
                ));
                lines.push(Line::from(chips));
                lines.push(Line::from(""));

                lines.push(Line::from(vec![
                    Span::styled("  URL  ", Style::default().fg(MCP_ADD_ACCENT)),
                    Span::styled(
                        format!("{}▌", truncate_middle(&self.url, w.saturating_sub(8))),
                        Style::default().fg(Color::White),
                    ),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Tip: copy the URL, then paste here (or press p / Enter for clipboard).",
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(Span::styled(
                    "  Example: https://mcp.example.com/mcp",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            McpAddScreen::Name => {
                lines.push(Line::from(Span::styled(
                    "  Config key for this server",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("  Name ", Style::default().fg(MCP_ADD_ACCENT)),
                    Span::styled(format!("{}▌", self.name), Style::default().fg(Color::White)),
                ]));
                lines.push(Line::from(""));
                for part in wrap_text(&format!("URL: {}", self.url), w.saturating_sub(2)) {
                    lines.push(Line::from(Span::styled(
                        format!("  {part}"),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            McpAddScreen::Discovering => {
                lines.push(Line::from(Span::styled(
                    "  Discovering OAuth…",
                    Style::default()
                        .fg(Color::Rgb(255, 200, 120))
                        .add_modifier(Modifier::ITALIC),
                )));
                lines.push(Line::from(""));
                for part in wrap_text(&self.url, w.saturating_sub(2)) {
                    lines.push(Line::from(format!("  {part}")));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Protected Resource Metadata → Authorization Server → DCR",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            McpAddScreen::Confirm => {
                if let Some(d) = &self.discovered {
                    lines.push(Line::from(Span::styled(
                        "  Review discovered OAuth",
                        Style::default()
                            .fg(MCP_ADD_ACCENT)
                            .add_modifier(Modifier::BOLD),
                    )));
                    lines.push(Line::from(""));
                    lines.push(detail_line("Name", &self.name));
                    lines.push(detail_line("Resource", &d.resource));
                    if let Some(rn) = &d.resource_name {
                        lines.push(detail_line("Title", rn));
                    }
                    lines.push(detail_line("Issuer", &d.issuer));
                    for part in wrap_text(
                        &format!("Authorize  {}", d.authorization_url),
                        w.saturating_sub(2),
                    ) {
                        lines.push(Line::from(format!("  {part}")));
                    }
                    for part in
                        wrap_text(&format!("Token      {}", d.token_url), w.saturating_sub(2))
                    {
                        lines.push(Line::from(format!("  {part}")));
                    }
                    lines.push(detail_line(
                        "Client",
                        d.client_id.as_deref().unwrap_or("(none)"),
                    ));
                    lines.push(detail_line("Auth", &d.auth_method));
                    lines.push(detail_line("Scopes", &d.scopes.join(" ")));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "  Press Enter to save  ·  l to save and open browser login",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            McpAddScreen::Done => {
                lines.push(Line::from(Span::styled(
                    format!(
                        "  Saved MCP server '{}'",
                        self.saved_name.as_deref().unwrap_or(&self.name)
                    ),
                    Style::default()
                        .fg(Color::Rgb(140, 220, 160))
                        .add_modifier(Modifier::BOLD),
                )));
                if self.needs_login {
                    lines.push(Line::from(""));
                    lines.push(Line::from(
                        "  OAuth login required — press l (or /mcp login <name>).",
                    ));
                }
            }
            McpAddScreen::Error => {
                lines.push(Line::from(Span::styled(
                    "  ⚠  Discovery failed",
                    Style::default()
                        .fg(Color::Rgb(239, 83, 80))
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                let err = self.error.clone().unwrap_or_else(|| "unknown error".into());
                for part in wrap_text(&err, w.saturating_sub(4)) {
                    lines.push(Line::from(Span::styled(
                        format!("     {part}"),
                        Style::default().fg(Color::Rgb(239, 83, 80)),
                    )));
                }
            }
        }

        if let Some(toast) = &self.toast {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {toast}"),
                Style::default().fg(Color::Rgb(140, 220, 160)),
            )));
        }
        lines
    }

    /// Build a RegisterMcpRequest from discovery results (no network).
    pub fn build_register_request(
        &self,
    ) -> Result<crate::mcp_register::RegisterMcpRequest, String> {
        let discovered = self
            .discovered
            .as_ref()
            .ok_or_else(|| "no discovery result".to_string())?;
        let mut req = crate::mcp_register::RegisterMcpRequest {
            name: self.name.clone(),
            url: Some(self.url.clone()),
            command: None,
            args: vec![],
            auth: crate::mcp_register::McpAuthKind::OAuth,
            token: None,
            token_url: Some(discovered.token_url.clone()),
            client_id: discovered.client_id.clone(),
            client_secret: discovered.client_secret.clone(),
            device_authorization_url: discovered.device_authorization_url.clone(),
            authorization_url: Some(discovered.authorization_url.clone()),
            redirect_url: Some(discovered.redirect_url.clone()),
            scopes: discovered.scopes.clone(),
            allow_loopback: self.allow_loopback,
            discover: Some(false),
            resource: Some(discovered.resource.clone()),
            issuer: Some(discovered.issuer.clone()),
            iss_parameter_supported: Some(discovered.iss_parameter_supported),
            auth_method: Some(discovered.auth_method.clone()),
            grant_type: Some(discovered.grant_type.clone()),
            use_pkce: Some(discovered.use_pkce),
        };
        req.apply_discovery(discovered);
        Ok(req)
    }
}

fn normalize_pasted_url(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == '<' || c == '>')
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn progress_rail(screen: McpAddScreen) -> Vec<Line<'static>> {
    let step_idx = match screen {
        McpAddScreen::Url => 0,
        McpAddScreen::Name => 1,
        McpAddScreen::Discovering => 2,
        McpAddScreen::Confirm => 3,
        McpAddScreen::Done => 4,
        McpAddScreen::Error => 2,
    };
    let labels = ["URL", "Name", "Discover", "Confirm", "Done"];
    let mut spans = vec![Span::raw("  ")];
    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  →  ", Style::default().fg(Color::DarkGray)));
        }
        let done = i < step_idx || matches!(screen, McpAddScreen::Done);
        let active = i == step_idx && !matches!(screen, McpAddScreen::Done);
        let (mark, style) = if done && !active {
            ("✓", Style::default().fg(Color::Rgb(140, 220, 160)))
        } else if active {
            (
                "●",
                Style::default()
                    .fg(MCP_ADD_ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ("○", Style::default().fg(Color::DarkGray))
        };
        spans.push(Span::styled(format!("{mark} {label}"), style));
    }
    vec![Line::from(spans)]
}

fn chip(label: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!("[{label}]"),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn detail_line(label: &str, value: &str) -> Line<'static> {
    Line::from(format!("  {label:<9} {value}"))
}

fn truncate_middle(s: &str, max: usize) -> String {
    if s.chars().count() <= max || max < 8 {
        return s.to_string();
    }
    let keep = (max - 1) / 2;
    let chars: Vec<char> = s.chars().collect();
    let head: String = chars.iter().take(keep).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(keep)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
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
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_tools::mcp_auth::DiscoveredMcpOauth;

    fn sample_discovered() -> DiscoveredMcpOauth {
        DiscoveredMcpOauth {
            resource: "https://mcp.example.com/mcp".into(),
            resource_name: Some("Example MCP".into()),
            issuer: "https://auth.example.com".into(),
            authorization_url: "https://auth.example.com/authorize".into(),
            token_url: "https://auth.example.com/token".into(),
            device_authorization_url: None,
            registration_endpoint: Some("https://auth.example.com/register".into()),
            client_id: Some("edgecrab-test".into()),
            client_secret: None,
            auth_method: "none".into(),
            redirect_url: "http://localhost:0/callback".into(),
            scopes: vec!["mcp:read".into()],
            use_pkce: true,
            grant_type: "authorization_code".into(),
            iss_parameter_supported: false,
        }
    }

    #[test]
    fn build_register_request_from_discovery() {
        let mut tui = McpAddTui::new(PathBuf::from("/tmp/config.yaml"));
        tui.url = "https://mcp.example.com/mcp".into();
        tui.name = "example".into();
        tui.discovered = Some(sample_discovered());
        let req = tui.build_register_request().expect("req");
        assert_eq!(req.name, "example");
        assert_eq!(req.resource.as_deref(), Some("https://mcp.example.com/mcp"));
        assert_eq!(req.client_id.as_deref(), Some("edgecrab-test"));
        assert!(!req.needs_discovery());
    }

    #[test]
    fn paste_fills_url_and_enter_advances() {
        let mut tui = McpAddTui::new(PathBuf::from("/tmp/config.yaml"));
        tui.open();
        tui.apply_paste("  https://mcp.example.com/mcp\n");
        assert_eq!(tui.url, "https://mcp.example.com/mcp");
        assert!(tui.url_from_paste);
        let action = tui.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(action, McpAddAction::Redraw);
        assert_eq!(tui.screen, McpAddScreen::Name);
    }

    #[test]
    fn empty_enter_requests_clipboard() {
        let mut tui = McpAddTui::new(PathBuf::from("/tmp/config.yaml"));
        tui.open();
        let action = tui.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(action, McpAddAction::LoadClipboard);
    }

    #[test]
    fn p_on_empty_url_loads_clipboard() {
        let mut tui = McpAddTui::new(PathBuf::from("/tmp/config.yaml"));
        tui.open();
        let action = tui.handle_key(KeyEvent::from(KeyCode::Char('p')));
        assert_eq!(action, McpAddAction::LoadClipboard);
    }

    #[test]
    fn p_in_url_types_normally() {
        let mut tui = McpAddTui::new(PathBuf::from("/tmp/config.yaml"));
        tui.open();
        tui.url = "htt".into();
        let action = tui.handle_key(KeyEvent::from(KeyCode::Char('p')));
        assert_eq!(action, McpAddAction::Redraw);
        assert_eq!(tui.url, "http");
    }
}
