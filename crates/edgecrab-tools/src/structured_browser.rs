//! Structured browser navigate / snapshot / vision results (018 P6 Wave B + 019 Wave A).
//!
//! Assess + storm counters read typed JSON fields — never prose heuristics
//! ("loading", "spinner", "beautiful", "this page isn't working").
//!
//! **ContentClass** separates transport success from content success (019 FP2).

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Content-layer classification (transport ≠ content ≠ task).
///
/// Deterministic markers only — no soft “looks broken” heuristics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentClass {
    /// Usable document for evidence purposes.
    #[default]
    Ok,
    /// Chrome interstitial (`chrome-error://`).
    ChromeError,
    /// Known static server / HTTP error page titles (exact allowlist).
    HttpErrorPage,
    /// Snapshot with zero interactive/a11y nodes.
    EmptyDocument,
    /// Tool transport/unavailable failure (no document).
    TransportFail,
    /// App-level failure overlay (exact a11y/title markers — e.g. "3D failed to load").
    AppFail,
}

impl ContentClass {
    /// True when this class can count as visual perception evidence.
    pub fn is_evidence(self) -> bool {
        matches!(self, ContentClass::Ok)
    }

    /// True when this is a content/transport failure fingerprint class.
    pub fn is_failure(self) -> bool {
        !self.is_evidence()
    }
}

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
    /// Content-layer class (019). Default Ok for backward-compat JSON without field.
    #[serde(default)]
    pub content_class: ContentClass,
}

impl StructuredBrowserResult {
    pub fn navigate_ok(final_url: &str, title: &str) -> Self {
        let is_chrome_error = url_is_chrome_error(final_url);
        let content_class = classify_browser_content(
            Some(final_url),
            Some(title),
            None,
            is_chrome_error,
            None,
            true,
        );
        Self {
            ok: content_class.is_evidence(),
            tool: "browser_navigate".into(),
            final_url: Some(final_url.to_string()),
            title: Some(title.to_string()),
            error_text: None,
            is_chrome_error,
            node_count: None,
            body: None,
            content_class,
        }
    }

    pub fn navigate_err(url: &str, error_text: &str) -> Self {
        let is_chrome_error = url_is_chrome_error(url);
        let content_class = if is_chrome_error {
            ContentClass::ChromeError
        } else {
            ContentClass::TransportFail
        };
        Self {
            ok: false,
            tool: "browser_navigate".into(),
            final_url: Some(url.to_string()),
            title: None,
            error_text: Some(error_text.to_string()),
            is_chrome_error,
            node_count: None,
            body: None,
            content_class,
        }
    }

    pub fn snapshot_ok(final_url: &str, body: &str, node_count: Option<u32>) -> Self {
        let is_chrome_error = url_is_chrome_error(final_url);
        // First principle: chrome-error URL ⇒ not evidence; empty a11y tree ⇒ thin.
        let content_class = classify_browser_content(
            Some(final_url),
            None,
            Some(body),
            is_chrome_error,
            node_count,
            true,
        );
        Self {
            ok: content_class.is_evidence(),
            tool: "browser_snapshot".into(),
            final_url: Some(final_url.to_string()),
            title: None,
            error_text: None,
            is_chrome_error,
            node_count,
            body: Some(body.to_string()),
            content_class,
        }
    }

    /// Vision evidence requires a non-chrome URL **and** a CDP document-ready fact.
    /// LLM prose is never inspected for loaders/spinners.
    pub fn vision_ok(final_url: &str, analysis: &str, document_ready: bool) -> Self {
        let is_chrome_error = url_is_chrome_error(final_url);
        let transport_ok = document_ready && !final_url.is_empty();
        let content_class = classify_browser_content(
            Some(final_url),
            None,
            Some(analysis),
            is_chrome_error,
            None,
            transport_ok,
        );
        Self {
            ok: content_class.is_evidence(),
            tool: "browser_vision".into(),
            final_url: Some(final_url.to_string()),
            title: None,
            error_text: None,
            is_chrome_error,
            node_count: None,
            body: Some(analysis.to_string()),
            content_class,
        }
    }

    /// Recompute `ok` + `content_class` from fields (after deserialize).
    pub fn reclassify(&mut self) {
        let transport_ok = self.error_text.is_none();
        self.content_class = classify_browser_content(
            self.final_url.as_deref(),
            self.title.as_deref(),
            self.body.as_deref(),
            self.is_chrome_error,
            self.node_count,
            transport_ok,
        );
        // Snapshot empty nodes
        if self.tool == "browser_snapshot"
            && self.content_class == ContentClass::Ok
            && self.node_count.unwrap_or(0) == 0
        {
            self.content_class = ContentClass::EmptyDocument;
        }
        self.ok = self.content_class.is_evidence();
    }

    /// Serialize as a single JSON object (assess-friendly).
    pub fn to_tool_result_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            json!({
                "ok": self.ok,
                "tool": self.tool,
                "is_chrome_error": self.is_chrome_error,
                "content_class": self.content_class,
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

/// Deterministic content classification (019 Wave A — no flaky heuristics).
///
/// Exact title allowlist for HttpErrorPage; chrome-error URL scheme for ChromeError.
pub fn classify_browser_content(
    url: Option<&str>,
    title: Option<&str>,
    body: Option<&str>,
    is_chrome_error: bool,
    node_count: Option<u32>,
    transport_ok: bool,
) -> ContentClass {
    if is_chrome_error || url.map(url_is_chrome_error).unwrap_or(false) {
        return ContentClass::ChromeError;
    }
    if !transport_ok {
        return ContentClass::TransportFail;
    }
    if let Some(t) = title {
        let tl = t.trim().to_ascii_lowercase();
        // Exact / known static server titles only (Python http.server, common 404 pages).
        if matches!(
            tl.as_str(),
            "error response"
                | "404 not found"
                | "403 forbidden"
                | "500 internal server error"
                | "not found"
        ) {
            return ContentClass::HttpErrorPage;
        }
    }
    // Python http.server body marker (deterministic prefix, not free "error" scan).
    if let Some(b) = body {
        let head: String = b.chars().take(200).collect();
        if head.contains("Error response") && head.contains("Error code:") {
            return ContentClass::HttpErrorPage;
        }
        // FastAPI / JSON API wrong-service on port (exact prefix markers).
        if head.contains("\"detail\":\"Not Found\"") || head.contains("\"detail\": \"Not Found\"") {
            return ContentClass::HttpErrorPage;
        }
        // App-level fail overlays — exact markers only (022 C1, no soft heuristics).
        if app_fail_marker_present(b) {
            return ContentClass::AppFail;
        }
    }
    if let Some(t) = title
        && app_fail_marker_present(t)
    {
        return ContentClass::AppFail;
    }
    if let Some(0) = node_count {
        return ContentClass::EmptyDocument;
    }
    ContentClass::Ok
}

/// Exact app-failure markers (case-insensitive substring of known boot overlays).
fn app_fail_marker_present(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // Exact product failure strings — not free-form "broken" / "error".
    const MARKERS: &[&str] = &[
        "3d failed to load",
        "webgl failed",
        "failed to initialize webgl",
        "three.js failed",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
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
    if let Ok(mut v) = serde_json::from_str::<StructuredBrowserResult>(trimmed)
        && !v.tool.is_empty()
    {
        // Reclassify so legacy JSON without content_class still gets HttpErrorPage.
        v.is_chrome_error = v.is_chrome_error
            || v.final_url
                .as_deref()
                .map(url_is_chrome_error)
                .unwrap_or(false);
        v.reclassify();
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
        v.is_chrome_error = v.is_chrome_error
            || v.final_url
                .as_deref()
                .map(url_is_chrome_error)
                .unwrap_or(false);
        v.reclassify();
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
    Some(parsed.ok && parsed.content_class.is_evidence())
}

/// Content class from a tool result, if structured browser envelope present.
pub fn browser_content_class(tool_result: &str) -> Option<ContentClass> {
    parse_structured_browser_result(tool_result).map(|p| p.content_class)
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
        assert_eq!(parsed.content_class, ContentClass::Ok);
        assert_eq!(parsed.final_url.as_deref(), Some("http://127.0.0.1:8000/"));
    }

    #[test]
    fn structured_browser_chrome_error_not_ok() {
        let r = StructuredBrowserResult::navigate_ok("chrome-error://chromewebdata/", "Error");
        assert!(!r.ok);
        assert!(r.is_chrome_error);
        assert_eq!(r.content_class, ContentClass::ChromeError);
        assert_eq!(
            structured_browser_nav_succeeded(&r.to_tool_result_json()),
            Some(false)
        );
    }

    #[test]
    fn nf_u1_error_response_title_is_http_error_page() {
        let r = StructuredBrowserResult::navigate_ok("http://127.0.0.1:8000/", "Error response");
        assert!(!r.ok);
        assert_eq!(r.content_class, ContentClass::HttpErrorPage);
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
        assert_eq!(thin.content_class, ContentClass::EmptyDocument);

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
        assert_eq!(good.content_class, ContentClass::Ok);
    }

    #[test]
    fn structured_browser_vision_requires_document_ready() {
        let not_ready = StructuredBrowserResult::vision_ok(
            "http://127.0.0.1:8000/",
            "fullscreen near-black loading spinner, no game",
            false,
        );
        assert!(!not_ready.ok);
        assert_eq!(not_ready.content_class, ContentClass::TransportFail);

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

    #[test]
    fn reclassify_legacy_json_without_content_class_field() {
        let raw = r#"{"ok":true,"tool":"browser_navigate","final_url":"http://127.0.0.1:8000/","title":"Error response","is_chrome_error":false}"#;
        let parsed = parse_structured_browser_result(raw).expect("parse");
        assert_eq!(parsed.content_class, ContentClass::HttpErrorPage);
        assert!(!parsed.ok);
    }

    #[test]
    fn app_fail_marker_3d_failed_to_load() {
        let body = r#"- Page: ♔ 3D Chess
heading "3D failed to load"
button "Reload""#;
        let r = StructuredBrowserResult::snapshot_ok("http://127.0.0.1:8000/", body, Some(9));
        assert_eq!(r.content_class, ContentClass::AppFail);
        assert!(!r.ok);
    }

    #[test]
    fn api_not_found_json_is_http_error_page() {
        let body = r#"body pre: {"detail":"Not Found"}"#;
        let class = classify_browser_content(
            Some("http://127.0.0.1:8765/"),
            Some(""),
            Some(body),
            false,
            Some(0),
            true,
        );
        // Empty nodes win as EmptyDocument when node_count=0, but body marker should
        // still classify as HttpErrorPage when nodes unknown.
        let class2 = classify_browser_content(
            Some("http://127.0.0.1:8765/"),
            Some(""),
            Some(body),
            false,
            None,
            true,
        );
        assert_eq!(class2, ContentClass::HttpErrorPage);
        let _ = class;
    }
}
