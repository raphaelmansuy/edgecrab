//! Structured browser navigate / snapshot / vision results (018 P6 Wave B).
//!
//! Assess + storm counters read typed JSON fields — never prose heuristics
//! ("loading", "spinner", "beautiful", "this page isn't working").

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Machine-readable browser perception / navigate outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructuredBrowserResult {
    pub ok: bool,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_text: Option<String>,
    #[serde(default)]
    pub is_chrome_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_count: Option<u32>,
    /// Human-readable body (snapshot tree, vision analysis) after the JSON envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

impl StructuredBrowserResult {
    pub fn navigate_ok(final_url: &str, title: &str) -> Self {
        let is_chrome_error = url_is_chrome_error(final_url);
        Self {
            ok: !is_chrome_error,
            tool: "browser_navigate".into(),
            final_url: Some(final_url.to_string()),
            title: Some(title.to_string()),
            error_text: None,
            is_chrome_error,
            node_count: None,
            body: None,
        }
    }

    pub fn navigate_err(url: &str, error_text: &str) -> Self {
        Self {
            ok: false,
            tool: "browser_navigate".into(),
            final_url: Some(url.to_string()),
            title: None,
            error_text: Some(error_text.to_string()),
            is_chrome_error: url_is_chrome_error(url),
            node_count: None,
            body: None,
        }
    }

    pub fn snapshot_ok(final_url: &str, body: &str, node_count: Option<u32>) -> Self {
        let is_chrome_error = url_is_chrome_error(final_url);
        // First principle: chrome-error URL ⇒ not evidence; empty a11y tree ⇒ thin.
        let nodes = node_count.unwrap_or(0);
        let ok = !is_chrome_error && nodes > 0;
        Self {
            ok,
            tool: "browser_snapshot".into(),
            final_url: Some(final_url.to_string()),
            title: None,
            error_text: None,
            is_chrome_error,
            node_count,
            body: Some(body.to_string()),
        }
    }

    /// Vision evidence requires a non-chrome URL **and** a CDP document-ready fact.
    /// LLM prose is never inspected for loaders/spinners.
    pub fn vision_ok(final_url: &str, analysis: &str, document_ready: bool) -> Self {
        let is_chrome_error = url_is_chrome_error(final_url);
        Self {
            ok: !is_chrome_error && !final_url.is_empty() && document_ready,
            tool: "browser_vision".into(),
            final_url: Some(final_url.to_string()),
            title: None,
            error_text: None,
            is_chrome_error,
            node_count: None,
            body: Some(analysis.to_string()),
        }
    }

    /// Serialize as a single JSON object (assess-friendly).
    pub fn to_tool_result_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            json!({
                "ok": self.ok,
                "tool": self.tool,
                "is_chrome_error": self.is_chrome_error,
            })
            .to_string()
        })
    }

    /// Human + machine: JSON line then optional prose body for the model.
    pub fn to_tool_result_text(&self) -> String {
        let mut out = self.to_tool_result_json();
        if let Some(body) = &self.body
            && !body.is_empty()
        {
            out.push('\n');
            out.push_str(body);
        } else if self.tool == "browser_navigate"
            && self.ok
            && let (Some(url), Some(title)) = (&self.final_url, &self.title)
        {
            out.push_str(&format!("\nNavigated to: {url}\nTitle: {title}"));
        }
        out
    }
}

/// Chrome error interstitial — URL *scheme/host* fact, not page prose.
pub fn url_is_chrome_error(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("chrome-error://") || lower.starts_with("chrome://error")
}

/// Parse structured envelope from a tool result (JSON prefix or full JSON).
pub fn parse_structured_browser_result(tool_result: &str) -> Option<StructuredBrowserResult> {
    let trimmed = tool_result.trim();
    // Full JSON object
    if let Ok(v) = serde_json::from_str::<StructuredBrowserResult>(trimmed)
        && !v.tool.is_empty()
    {
        return Some(v);
    }
    // JSON on first line, body after
    let first_line = trimmed.lines().next()?.trim();
    if first_line.starts_with('{')
        && let Ok(mut v) = serde_json::from_str::<StructuredBrowserResult>(first_line)
    {
        if v.body.is_none() {
            let rest = trimmed[first_line.len()..].trim();
            if !rest.is_empty() {
                v.body = Some(rest.to_string());
            }
        }
        return Some(v);
    }
    None
}

/// Whether a navigate/snapshot/vision result counts as success for storm counters.
pub fn structured_browser_nav_succeeded(tool_result: &str) -> Option<bool> {
    let parsed = parse_structured_browser_result(tool_result)?;
    if !matches!(
        parsed.tool.as_str(),
        "browser_navigate" | "browser_snapshot" | "browser_vision"
    ) {
        return None;
    }
    Some(parsed.ok && !parsed.is_chrome_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_browser_navigate_roundtrip() {
        let r = StructuredBrowserResult::navigate_ok("http://127.0.0.1:8000/", "3D Chess");
        let text = r.to_tool_result_text();
        let parsed = parse_structured_browser_result(&text).expect("parse");
        assert!(parsed.ok);
        assert!(!parsed.is_chrome_error);
        assert_eq!(parsed.final_url.as_deref(), Some("http://127.0.0.1:8000/"));
    }

    #[test]
    fn structured_browser_chrome_error_not_ok() {
        let r = StructuredBrowserResult::navigate_ok("chrome-error://chromewebdata/", "Error");
        assert!(!r.ok);
        assert!(r.is_chrome_error);
        assert_eq!(
            structured_browser_nav_succeeded(&r.to_tool_result_json()),
            Some(false)
        );
    }

    #[test]
    fn structured_browser_snapshot_requires_nodes_not_prose() {
        // Prose about errors must NOT flip chrome-error — only the URL scheme does.
        let thin = StructuredBrowserResult::snapshot_ok(
            "http://127.0.0.1:8000/",
            "This page isn't working — ERR_EMPTY_RESPONSE",
            Some(0),
        );
        assert!(!thin.ok);
        assert!(!thin.is_chrome_error);

        let chrome_url = StructuredBrowserResult::snapshot_ok(
            "chrome-error://chromewebdata/",
            "Board with pieces",
            Some(12),
        );
        assert!(!chrome_url.ok);
        assert!(chrome_url.is_chrome_error);

        let good = StructuredBrowserResult::snapshot_ok(
            "http://127.0.0.1:8000/",
            "Board with pieces @e1 @e2",
            Some(12),
        );
        assert!(good.ok);
    }

    #[test]
    fn structured_browser_vision_requires_document_ready() {
        let not_ready = StructuredBrowserResult::vision_ok(
            "http://127.0.0.1:8000/",
            "fullscreen near-black loading spinner, no game",
            false,
        );
        assert!(!not_ready.ok);

        let ready = StructuredBrowserResult::vision_ok(
            "http://127.0.0.1:8000/",
            "fullscreen near-black loading spinner, no game",
            true,
        );
        assert!(ready.ok);
        assert!(!ready.is_chrome_error);

        let bad = StructuredBrowserResult::vision_ok(
            "chrome-error://chromewebdata/",
            "looks like a beautiful chess UI",
            true,
        );
        assert!(!bad.ok);
        assert!(bad.is_chrome_error);
    }
}
