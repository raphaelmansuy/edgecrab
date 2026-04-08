//! # web — Web search and content extraction
//!
//! WHY two separate tools rather than one:
//! - `web_search` → structured query results (list of links + snippets)
//! - `web_extract` → full readable content from a known URL
//!
//! This matches the established split between search and extraction.
//!
//! ## web_search backend priority
//!
//! ```text
//!   web_search("Rust async book")
//!       │
//!       ├── FIRECRAWL_API_KEY set?
//!       │       └──→ api.firecrawl.dev/v2/search (premium search + scrape-ready results)
//!       │
//!       ├── TAVILY_API_KEY set?
//!       │       └──→ api.tavily.com/search (best results, free tier ~1000/mo)
//!       │
//!       ├── BRAVE_API_KEY set?
//!       │       └──→ api.search.brave.com (good results, free tier)
//!       │
//!       └── fallback: DuckDuckGo Instant Answer API (no key, limited coverage)
//!                 └──→ AbstractText + RelatedTopics → ranked results
//! ```
//!
//! ## web_extract
//!
//! ```text
//!   web_extract("https://doc.rust-lang.org/...")
//!       └──→ reqwest::get → readable HTML extraction OR EdgeParse PDF extraction
//! ```
//!
//! SSRF prevention is applied before any outbound request via
//! edgecrab-security::url_safety.
//!
//! ## How to enable richer search
//!
//! Set one of these environment variables in `~/.edgecrab/.env`:
//! - `FIRECRAWL_API_KEY=fc-...` (premium web search/scrape/crawl: https://firecrawl.dev)
//! - `TAVILY_API_KEY=tvly-...` (free tier: https://app.tavily.com)
//! - `BRAVE_API_KEY=BSA...` (free tier: https://api.search.brave.com/app/keys)

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashSet, VecDeque};
use std::sync::OnceLock;

use edgecrab_types::{ToolError, ToolSchema};
use reqwest::Url;

use crate::registry::{ToolContext, ToolHandler};
use crate::tools::browser::{browser_is_available, render_page_text};
use crate::tools::pdf_to_markdown::{extract_pdf_markdown_from_bytes, looks_like_pdf};

// ─── HTML stripping ────────────────────────────────────────────

/// Compiled regex for stripping HTML tags (compiled once, reused everywhere).
///
/// WHY OnceLock: Regex compilation is expensive. Compiling once at first
/// use and sharing the result eliminates per-call overhead.
static HTML_TAG_RE: OnceLock<Regex> = OnceLock::new();
static HREF_RE: OnceLock<Regex> = OnceLock::new();
static TITLE_RE: OnceLock<Regex> = OnceLock::new();
static META_DESCRIPTION_RE: OnceLock<Regex> = OnceLock::new();
static MAIN_RE: OnceLock<Regex> = OnceLock::new();
static ARTICLE_RE: OnceLock<Regex> = OnceLock::new();
static BODY_RE: OnceLock<Regex> = OnceLock::new();
static NOISE_BLOCK_RE: OnceLock<Regex> = OnceLock::new();
static BLOCK_BREAK_RE: OnceLock<Regex> = OnceLock::new();

fn html_tag_re() -> &'static Regex {
    HTML_TAG_RE.get_or_init(|| Regex::new(r"<[^>]+>").expect("valid regex"))
}

fn href_re() -> &'static Regex {
    HREF_RE
        .get_or_init(|| Regex::new(r#"(?is)<a\s[^>]*href=["']([^"'#]+)["']"#).expect("valid regex"))
}

fn title_re() -> &'static Regex {
    TITLE_RE.get_or_init(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("valid regex"))
}

fn meta_description_re() -> &'static Regex {
    META_DESCRIPTION_RE.get_or_init(|| {
        Regex::new(
            r#"(?is)<meta[^>]+(?:name|property)=["'](?:description|og:description)["'][^>]+content=["']([^"']+)["'][^>]*>"#,
        )
        .expect("valid regex")
    })
}

fn main_re() -> &'static Regex {
    MAIN_RE.get_or_init(|| Regex::new(r"(?is)<main\b[^>]*>(.*?)</main>").expect("valid regex"))
}

fn article_re() -> &'static Regex {
    ARTICLE_RE
        .get_or_init(|| Regex::new(r"(?is)<article\b[^>]*>(.*?)</article>").expect("valid regex"))
}

fn body_re() -> &'static Regex {
    BODY_RE.get_or_init(|| Regex::new(r"(?is)<body\b[^>]*>(.*?)</body>").expect("valid regex"))
}

fn noise_block_re() -> &'static Regex {
    NOISE_BLOCK_RE.get_or_init(|| {
        Regex::new(
            r"(?is)<(?:script|style|noscript|template|svg|canvas|iframe|nav|footer|header|aside|form)[^>]*>.*?</(?:script|style|noscript|template|svg|canvas|iframe|nav|footer|header|aside|form)>",
        )
        .expect("valid regex")
    })
}

fn block_break_re() -> &'static Regex {
    BLOCK_BREAK_RE.get_or_init(|| {
        Regex::new(
            r"(?is)</?(?:p|div|section|article|main|li|ul|ol|h[1-6]|tr|table|blockquote|pre|br)[^>]*>",
        )
        .expect("valid regex")
    })
}

/// Strip HTML tags and decode common entities, returning readable plain text.
fn strip_html(html: &str) -> String {
    let without_tags = html_tag_re().replace_all(html, " ");
    // Decode most common HTML entities
    without_tags
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        // Collapse whitespace runs
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_title(html: &str) -> String {
    title_re()
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map(|title| strip_html(title.as_str()))
        .unwrap_or_default()
}

fn extract_meta_description(html: &str) -> Option<String> {
    meta_description_re()
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map(|description| strip_html(description.as_str()))
        .filter(|description| !description.is_empty())
}

fn focus_html_fragment(html: &str) -> String {
    for re in [main_re(), article_re(), body_re()] {
        if let Some(captures) = re.captures(html) {
            if let Some(fragment) = captures.get(1) {
                return fragment.as_str().to_string();
            }
        }
    }
    html.to_string()
}

fn extract_readable_text(html: &str) -> String {
    let focused = focus_html_fragment(html);
    let without_noise = noise_block_re().replace_all(&focused, " ");
    let with_breaks = block_break_re().replace_all(&without_noise, "\n");
    let without_tags = html_tag_re().replace_all(&with_breaks, " ");
    let decoded = without_tags
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    decoded
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn extract_links(base_url: &Url, html: &str) -> Vec<String> {
    href_re()
        .captures_iter(html)
        .filter_map(|captures| captures.get(1).map(|m| m.as_str().trim().to_string()))
        .filter(|href| {
            !href.is_empty()
                && !href.starts_with("mailto:")
                && !href.starts_with("javascript:")
                && !href.starts_with("tel:")
        })
        .filter_map(|href| base_url.join(&href).ok())
        .map(|url| {
            let mut normalized = url;
            normalized.set_fragment(None);
            normalized.to_string()
        })
        .collect()
}

fn host_matches(base: &Url, candidate: &Url) -> bool {
    base.domain() == candidate.domain()
}

fn path_in_scope(base: &Url, candidate: &Url, allow_external_paths: bool) -> bool {
    if allow_external_paths {
        return true;
    }

    let base_path = base.path().trim_end_matches('/');
    let prefix = if base_path.is_empty() { "/" } else { base_path };
    candidate.path().starts_with(prefix)
}

fn rank_page(title: &str, content: &str, instructions: Option<&str>) -> i32 {
    let mut score = 0;
    if let Some(instructions) = instructions {
        let lowered = instructions.to_lowercase();
        for keyword in lowered.split_whitespace().filter(|s| s.len() > 2) {
            if title.to_lowercase().contains(keyword) {
                score += 3;
            }
            if content.to_lowercase().contains(keyword) {
                score += 1;
            }
        }
    }
    score
}

fn truncate_chars(text: String, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text, false);
    }

    let boundary = (0..=limit)
        .rev()
        .find(|&i| text.is_char_boundary(i))
        .unwrap_or(0);
    (
        format!("{}… [truncated at {} chars]", &text[..boundary], limit),
        true,
    )
}

fn is_pdf_response(url: &Url, content_type: &str, bytes: &[u8]) -> bool {
    url.path().to_ascii_lowercase().ends_with(".pdf")
        || content_type
            .to_ascii_lowercase()
            .contains("application/pdf")
        || looks_like_pdf(bytes)
}

#[derive(Clone, Serialize)]
struct ExtractedDocument {
    url: String,
    title: String,
    content: String,
    extractor: String,
    content_type: String,
    content_format: String,
    truncated: bool,
    meta_description: Option<String>,
}

struct FetchedPage {
    document: ExtractedDocument,
    links: Vec<String>,
}

#[derive(Clone, Copy)]
enum ContentBackend {
    Native,
    Firecrawl,
    Tavily,
    Browser,
}

fn has_firecrawl_api_key() -> bool {
    std::env::var("FIRECRAWL_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

fn has_tavily_api_key() -> bool {
    std::env::var("TAVILY_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

fn has_brave_api_key() -> bool {
    std::env::var("BRAVE_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

fn backend_override(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
    })
}

fn resolve_search_backend(preferred: Option<&str>) -> Result<SearchBackend, ToolError> {
    let choice = preferred
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .or_else(|| backend_override(&["EDGECRAB_WEB_SEARCH_BACKEND", "EDGECRAB_WEB_BACKEND"]));

    match choice.as_deref().unwrap_or("auto") {
        "auto" => {
            if has_firecrawl_api_key() {
                Ok(SearchBackend::Firecrawl)
            } else if has_tavily_api_key() {
                Ok(SearchBackend::Tavily)
            } else if has_brave_api_key() {
                Ok(SearchBackend::Brave)
            } else {
                Ok(SearchBackend::DuckDuckGo)
            }
        }
        "firecrawl" => {
            if has_firecrawl_api_key() {
                Ok(SearchBackend::Firecrawl)
            } else {
                Err(ToolError::ExecutionFailed {
                    tool: "web_search".into(),
                    message: "Search backend 'firecrawl' requires FIRECRAWL_API_KEY.".into(),
                })
            }
        }
        "tavily" => {
            if has_tavily_api_key() {
                Ok(SearchBackend::Tavily)
            } else {
                Err(ToolError::ExecutionFailed {
                    tool: "web_search".into(),
                    message: "Search backend 'tavily' requires TAVILY_API_KEY.".into(),
                })
            }
        }
        "brave" => {
            if has_brave_api_key() {
                Ok(SearchBackend::Brave)
            } else {
                Err(ToolError::ExecutionFailed {
                    tool: "web_search".into(),
                    message: "Search backend 'brave' requires BRAVE_API_KEY.".into(),
                })
            }
        }
        "duckduckgo" | "ddg" => Ok(SearchBackend::DuckDuckGo),
        other => Err(ToolError::InvalidArgs {
            tool: "web_search".into(),
            message: format!(
                "Unsupported search backend '{other}'. Use auto, firecrawl, tavily, brave, or duckduckgo."
            ),
        }),
    }
}

fn resolve_content_backend(
    preferred: Option<&str>,
    tool: &str,
) -> Result<ContentBackend, ToolError> {
    let choice = preferred
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            backend_override(&[
                if tool == "web_crawl" {
                    "EDGECRAB_WEB_CRAWL_BACKEND"
                } else {
                    "EDGECRAB_WEB_EXTRACT_BACKEND"
                },
                "EDGECRAB_WEB_BACKEND",
            ])
        });

    match choice.as_deref().unwrap_or("auto") {
        "auto" => {
            if has_firecrawl_api_key() {
                Ok(ContentBackend::Firecrawl)
            } else if has_tavily_api_key() {
                Ok(ContentBackend::Tavily)
            } else {
                Ok(ContentBackend::Native)
            }
        }
        "native" => Ok(ContentBackend::Native),
        "firecrawl" => {
            if has_firecrawl_api_key() {
                Ok(ContentBackend::Firecrawl)
            } else {
                Err(ToolError::ExecutionFailed {
                    tool: tool.into(),
                    message: "Backend 'firecrawl' requires FIRECRAWL_API_KEY.".into(),
                })
            }
        }
        "tavily" => {
            if has_tavily_api_key() {
                Ok(ContentBackend::Tavily)
            } else {
                Err(ToolError::ExecutionFailed {
                    tool: tool.into(),
                    message: "Backend 'tavily' requires TAVILY_API_KEY.".into(),
                })
            }
        }
        "browser" | "rendered" => {
            if browser_is_available() {
                Ok(ContentBackend::Browser)
            } else {
                Err(ToolError::ExecutionFailed {
                    tool: tool.into(),
                    message: "Backend 'browser' requires browser tools to be available.".into(),
                })
            }
        }
        other => Err(ToolError::InvalidArgs {
            tool: tool.into(),
            message: format!(
                "Unsupported backend '{other}'. Use auto, native, firecrawl, tavily, or browser."
            ),
        }),
    }
}

fn content_backend_name(backend: ContentBackend) -> &'static str {
    match backend {
        ContentBackend::Native => "native",
        ContentBackend::Firecrawl => "firecrawl",
        ContentBackend::Tavily => "tavily",
        ContentBackend::Browser => "browser",
    }
}

fn infer_title_from_url(url: &Url, fallback: &str) -> String {
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn extract_pdf_document(
    final_url: &Url,
    content_type: &str,
    body: &[u8],
    max_chars: usize,
    tool: &str,
) -> Result<ExtractedDocument, ToolError> {
    let markdown = extract_pdf_markdown_from_bytes(body, "document.pdf", tool)?;
    let (content, truncated) = truncate_chars(markdown, max_chars);
    Ok(ExtractedDocument {
        url: final_url.to_string(),
        title: infer_title_from_url(final_url, "document.pdf"),
        content,
        extractor: "edgeparse".into(),
        content_type: if content_type.is_empty() {
            "application/pdf".into()
        } else {
            content_type.to_string()
        },
        content_format: "markdown".into(),
        truncated,
        meta_description: None,
    })
}

fn extract_html_document(
    final_url: &Url,
    content_type: &str,
    html: &str,
    max_chars: usize,
) -> ExtractedDocument {
    let title = extract_title(html);
    let meta_description = extract_meta_description(html);
    let text = extract_readable_text(html);
    let content = if text.is_empty() {
        "(No readable text content found on this page.)".to_string()
    } else {
        text
    };
    let (content, truncated) = truncate_chars(content, max_chars);

    ExtractedDocument {
        url: final_url.to_string(),
        title,
        content,
        extractor: "readable_html".into(),
        content_type: content_type.to_string(),
        content_format: "text".into(),
        truncated,
        meta_description,
    }
}

fn should_try_rendered_fallback(
    document: &ExtractedDocument,
    html: &str,
    content_type: &str,
) -> bool {
    if !content_type.to_ascii_lowercase().contains("html") && !html.contains("<html") {
        return false;
    }
    if document.extractor != "readable_html" {
        return false;
    }

    let lower = html.to_ascii_lowercase();
    let likely_spa_shell = lower.contains("id=\"__next\"")
        || lower.contains("id='__next'")
        || lower.contains("id=\"__nuxt\"")
        || lower.contains("id='app'")
        || lower.contains("id=\"app\"")
        || lower.contains("data-reactroot")
        || lower.contains("ng-app")
        || lower.contains("application/json")
        || lower.contains("webpack");
    let script_blocks = lower.matches("<script").count();
    let content_too_thin = document.content.contains("No readable text content")
        || document.content.len() < 400
        || (document.meta_description.is_none()
            && document.title.is_empty()
            && document.content.len() < 900);

    content_too_thin && (likely_spa_shell || script_blocks >= 3)
}

fn merge_links(primary: Vec<String>, secondary: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    primary
        .into_iter()
        .chain(secondary)
        .filter(|link| seen.insert(link.clone()))
        .collect()
}

fn rendered_document_from_page(
    page: crate::tools::browser::RenderedPage,
    content_type: String,
    max_chars: usize,
) -> ExtractedDocument {
    let (content, truncated) = truncate_chars(page.text, max_chars);
    ExtractedDocument {
        url: page.url,
        title: page.title,
        content,
        extractor: "browser_render".into(),
        content_type,
        content_format: "text".into(),
        truncated,
        meta_description: page.meta_description,
    }
}

async fn maybe_upgrade_with_rendered_page(
    final_url: &Url,
    base_document: ExtractedDocument,
    html: &str,
    content_type: &str,
    max_chars: usize,
    ctx: &ToolContext,
) -> (ExtractedDocument, Vec<String>) {
    let static_links = extract_links(final_url, html);

    if !browser_is_available() || !should_try_rendered_fallback(&base_document, html, content_type)
    {
        return (base_document, static_links);
    }

    match render_page_text(&base_document.url, ctx).await {
        Ok(rendered_page) => {
            let rendered_links = rendered_page.links.clone();
            let rendered_document =
                rendered_document_from_page(rendered_page, content_type.to_string(), max_chars);
            if rendered_document.content.len() > base_document.content.len() {
                (rendered_document, merge_links(static_links, rendered_links))
            } else {
                (base_document, merge_links(static_links, rendered_links))
            }
        }
        Err(_) => (base_document, static_links),
    }
}

async fn fetch_native_document(
    final_url: &Url,
    content_type: &str,
    body: &[u8],
    max_chars: usize,
    tool: &str,
    ctx: &ToolContext,
    render_js_fallback: bool,
) -> Result<FetchedPage, ToolError> {
    if is_pdf_response(final_url, content_type, body) {
        return Ok(FetchedPage {
            document: extract_pdf_document(final_url, content_type, body, max_chars, tool)?,
            links: Vec::new(),
        });
    }

    let html = String::from_utf8_lossy(body).to_string();
    let base_document = extract_html_document(final_url, content_type, &html, max_chars);
    let (document, links) = if render_js_fallback {
        maybe_upgrade_with_rendered_page(
            final_url,
            base_document,
            &html,
            content_type,
            max_chars,
            ctx,
        )
        .await
    } else {
        (base_document, extract_links(final_url, &html))
    };

    Ok(FetchedPage { document, links })
}

async fn fetch_browser_document(
    url: &Url,
    content_type: &str,
    max_chars: usize,
    ctx: &ToolContext,
    tool: &str,
) -> Result<FetchedPage, ToolError> {
    let rendered = render_page_text(url.as_str(), ctx)
        .await
        .map_err(|e| match e {
            ToolError::PermissionDenied(_) | ToolError::InvalidArgs { .. } => e,
            _ => ToolError::ExecutionFailed {
                tool: tool.into(),
                message: format!("Browser render failed: {e}"),
            },
        })?;

    Ok(FetchedPage {
        links: rendered.links.clone(),
        document: rendered_document_from_page(rendered, content_type.to_string(), max_chars),
    })
}

// ─── Backend detection ─────────────────────────────────────────

enum SearchBackend {
    Firecrawl,
    Tavily,
    Brave,
    DuckDuckGo,
}

// ─── web_search ────────────────────────────────────────────────

pub struct WebSearchTool;

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    /// Maximum number of results to return (default: 5)
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    backend: Option<String>,
}

/// Top-level DuckDuckGo Instant Answer API response.
#[derive(Deserialize)]
#[allow(non_snake_case)]
struct DdgResponse {
    AbstractText: Option<String>,
    AbstractURL: Option<String>,
    RelatedTopics: Vec<DdgTopic>,
}

/// A related topic from the DDG response.
#[derive(Deserialize)]
struct DdgTopic {
    #[serde(rename = "Text")]
    text: Option<String>,
    #[serde(rename = "FirstURL")]
    first_url: Option<String>,
    /// Nested sub-topics group (ignored for now)
    #[serde(rename = "Topics")]
    _topics: Option<Vec<serde_json::Value>>,
}

/// A single normalized search result.
#[derive(Serialize)]
struct SearchResult {
    title: String,
    url: String,
    description: String,
}

/// Search via Tavily API (api_key from TAVILY_API_KEY env).
///
/// Tavily free tier: ~1000 searches/month. Get key at https://app.tavily.com
async fn search_tavily(query: &str, max: usize) -> Result<Vec<SearchResult>, ToolError> {
    let data = tavily_request(
        "search",
        json!({
            "query": query,
            "max_results": max,
            "search_depth": "basic",
            "include_answer": false,
        }),
        "web_search",
    )
    .await?;

    let results = data["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    let title = r["title"].as_str()?.to_string();
                    let url = r["url"].as_str()?.to_string();
                    let description = r["content"].as_str().unwrap_or("").to_string();
                    Some(SearchResult {
                        title,
                        url,
                        description,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(results)
}

fn firecrawl_metadata_text(metadata: &serde_json::Value, key: &str) -> Option<String> {
    match &metadata[key] {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Array(values) => values.iter().find_map(|value| {
            value
                .as_str()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        }),
        _ => None,
    }
}

async fn firecrawl_request(
    method: reqwest::Method,
    path_or_url: &str,
    payload: Option<serde_json::Value>,
    tool: &str,
) -> Result<serde_json::Value, ToolError> {
    let api_key = std::env::var("FIRECRAWL_API_KEY").map_err(|_| ToolError::ExecutionFailed {
        tool: tool.into(),
        message: "Firecrawl backend requires FIRECRAWL_API_KEY.".into(),
    })?;
    let client = build_client()?;
    let url = if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        path_or_url.to_string()
    } else {
        format!(
            "https://api.firecrawl.dev/v2/{}",
            path_or_url.trim_start_matches('/')
        )
    };
    let mut req = client
        .request(method, &url)
        .header("Authorization", format!("Bearer {api_key}"));

    if let Some(body) = payload {
        req = req.header("Content-Type", "application/json").json(&body);
    }

    let resp = req.send().await.map_err(|e| ToolError::ExecutionFailed {
        tool: tool.into(),
        message: format!("Firecrawl API error: {e}"),
    })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            tool: tool.into(),
            message: format!("Firecrawl API HTTP {status}: {text}"),
        });
    }

    resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        tool: tool.into(),
        message: format!("Firecrawl JSON parse error: {e}"),
    })
}

fn normalize_firecrawl_search_results(data: &serde_json::Value, max: usize) -> Vec<SearchResult> {
    let array = data["data"]["web"]
        .as_array()
        .or_else(|| data["data"].as_array());

    array
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let metadata = &value["metadata"];
            let title = value["title"]
                .as_str()
                .map(|value| value.to_string())
                .or_else(|| firecrawl_metadata_text(metadata, "title"))
                .unwrap_or_default();
            let url = value["url"]
                .as_str()
                .or_else(|| metadata["url"].as_str())
                .or_else(|| metadata["sourceURL"].as_str())?
                .to_string();
            let description = value["description"]
                .as_str()
                .map(|value| value.to_string())
                .or_else(|| firecrawl_metadata_text(metadata, "description"))
                .unwrap_or_default();
            Some(SearchResult {
                title,
                url,
                description,
            })
        })
        .take(max)
        .collect()
}

async fn search_firecrawl(query: &str, max: usize) -> Result<Vec<SearchResult>, ToolError> {
    let data = firecrawl_request(
        reqwest::Method::POST,
        "search",
        Some(json!({
            "query": query,
            "limit": max,
            "ignoreInvalidURLs": true,
        })),
        "web_search",
    )
    .await?;
    Ok(normalize_firecrawl_search_results(&data, max))
}

async fn tavily_request(
    endpoint: &str,
    payload: serde_json::Value,
    tool: &str,
) -> Result<serde_json::Value, ToolError> {
    let api_key = std::env::var("TAVILY_API_KEY").map_err(|_| ToolError::ExecutionFailed {
        tool: tool.into(),
        message: "Tavily backend requires TAVILY_API_KEY.".into(),
    })?;
    let client = build_client()?;
    let url = format!(
        "https://api.tavily.com/{}",
        endpoint.trim_start_matches('/')
    );
    let body = match payload {
        serde_json::Value::Object(mut map) => {
            map.insert("api_key".into(), serde_json::Value::String(api_key));
            serde_json::Value::Object(map)
        }
        _ => {
            return Err(ToolError::ExecutionFailed {
                tool: tool.into(),
                message: "Invalid Tavily payload shape.".into(),
            });
        }
    };

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: tool.into(),
            message: format!("Tavily API error: {e}"),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            tool: tool.into(),
            message: format!("Tavily API HTTP {status}: {text}"),
        });
    }

    resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        tool: tool.into(),
        message: format!("Tavily JSON parse error: {e}"),
    })
}

/// Search via Brave Search API (api_key from BRAVE_API_KEY env).
///
/// Brave free tier: 2000 searches/month. Get key at https://api.search.brave.com/app/keys
async fn search_brave(query: &str, max: usize) -> Result<Vec<SearchResult>, ToolError> {
    let api_key = std::env::var("BRAVE_API_KEY").unwrap_or_default();
    let client = build_client()?;

    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        urlencoding_encode(query),
        max.min(20)
    );

    let resp = client
        .get(&url)
        .header("X-Subscription-Token", api_key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "web_search".into(),
            message: format!("Brave Search API error: {e}"),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            tool: "web_search".into(),
            message: format!("Brave Search HTTP {status}: {text}"),
        });
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        tool: "web_search".into(),
        message: format!("Brave Search JSON parse error: {e}"),
    })?;

    let results = data["web"]["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    let title = r["title"].as_str()?.to_string();
                    let url = r["url"].as_str()?.to_string();
                    let description = r["description"].as_str().unwrap_or("").to_string();
                    Some(SearchResult {
                        title,
                        url,
                        description,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(results)
}

/// Search via DuckDuckGo Instant Answer API (no key required).
///
/// NOTE: This API only returns results for well-known entities and topics.
/// It does NOT provide full web search results. For general queries, set
/// TAVILY_API_KEY or BRAVE_API_KEY for proper search results.
async fn search_duckduckgo(query: &str, max: usize) -> Result<Vec<SearchResult>, ToolError> {
    let url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        urlencoding_encode(query)
    );

    validate_url(&url, "web_search")?;

    let client = build_client()?;
    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            "EdgeCrab/0.1 (agent; +https://github.com/raphaelmansuy/edgecrab)",
        )
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "web_search".into(),
            message: format!("HTTP error: {e}"),
        })?;

    let ddg: DdgResponse =
        resp.json::<DdgResponse>()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "web_search".into(),
                message: format!("JSON parse error: {e}"),
            })?;

    let mut results: Vec<SearchResult> = Vec::new();

    // Add the abstract if present
    if let (Some(text), Some(url)) = (&ddg.AbstractText, &ddg.AbstractURL) {
        if !text.is_empty() {
            results.push(SearchResult {
                title: "Summary".into(),
                url: url.clone(),
                description: text.clone(),
            });
        }
    }

    // Add related topics
    for topic in ddg.RelatedTopics.iter().take(max) {
        if let (Some(text), Some(url)) = (&topic.text, &topic.first_url) {
            if !text.is_empty() {
                results.push(SearchResult {
                    title: text.split(" - ").next().unwrap_or(text).to_string(),
                    url: url.clone(),
                    description: text.clone(),
                });
            }
        }
    }

    Ok(results)
}

fn normalize_firecrawl_document(
    value: &serde_json::Value,
    max_chars: usize,
    fallback_url: Option<&str>,
) -> Option<ExtractedDocument> {
    let metadata = &value["metadata"];
    let url = metadata["url"]
        .as_str()
        .or_else(|| metadata["sourceURL"].as_str())
        .or_else(|| value["url"].as_str())
        .or(fallback_url)
        .unwrap_or_default()
        .to_string();
    if url.is_empty() {
        return None;
    }

    let title = firecrawl_metadata_text(metadata, "title")
        .or_else(|| value["title"].as_str().map(|value| value.to_string()))
        .unwrap_or_default();
    let content_format = if value["markdown"].is_string() {
        "markdown"
    } else if value["html"].is_string() || value["rawHtml"].is_string() {
        "html"
    } else {
        "text"
    };
    let raw = value["markdown"]
        .as_str()
        .or_else(|| value["html"].as_str())
        .or_else(|| value["rawHtml"].as_str())
        .or_else(|| value["text"].as_str())
        .unwrap_or_default()
        .to_string();
    let (content, truncated) = truncate_chars(
        if raw.is_empty() {
            "(No readable text content found on this page.)".to_string()
        } else {
            raw
        },
        max_chars,
    );

    Some(ExtractedDocument {
        url,
        title,
        content,
        extractor: "firecrawl".into(),
        content_type: metadata["contentType"]
            .as_str()
            .unwrap_or("text/html")
            .to_string(),
        content_format: content_format.into(),
        truncated,
        meta_description: firecrawl_metadata_text(metadata, "description")
            .or_else(|| value["description"].as_str().map(|value| value.to_string()))
            .filter(|value| !value.is_empty()),
    })
}

fn normalize_tavily_document(
    value: &serde_json::Value,
    max_chars: usize,
    fallback_url: Option<&str>,
) -> Option<ExtractedDocument> {
    let url = value["url"]
        .as_str()
        .or(fallback_url)
        .unwrap_or_default()
        .to_string();
    if url.is_empty() {
        return None;
    }

    let title = value["title"].as_str().unwrap_or_default().to_string();
    let raw = value["raw_content"]
        .as_str()
        .or_else(|| value["content"].as_str())
        .unwrap_or_default()
        .to_string();
    let (content, truncated) = truncate_chars(
        if raw.is_empty() {
            "(No readable text content found on this page.)".to_string()
        } else {
            raw
        },
        max_chars,
    );

    Some(ExtractedDocument {
        url,
        title,
        content,
        extractor: "tavily".into(),
        content_type: value["content_type"]
            .as_str()
            .unwrap_or("text/html")
            .to_string(),
        content_format: "text".into(),
        truncated,
        meta_description: value["description"]
            .as_str()
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty()),
    })
}

async fn extract_via_firecrawl(
    url: &str,
    max_chars: usize,
) -> Result<ExtractedDocument, ToolError> {
    let data = firecrawl_request(
        reqwest::Method::POST,
        "scrape",
        Some(json!({
            "url": url,
            "formats": ["markdown"],
            "onlyMainContent": true,
        })),
        "web_extract",
    )
    .await?;

    normalize_firecrawl_document(&data["data"], max_chars, Some(url)).ok_or_else(|| {
        ToolError::ExecutionFailed {
            tool: "web_extract".into(),
            message: "Firecrawl extraction returned no document.".into(),
        }
    })
}

async fn extract_via_tavily(url: &str, max_chars: usize) -> Result<ExtractedDocument, ToolError> {
    let data = tavily_request(
        "extract",
        json!({
            "urls": [url],
            "include_images": false,
        }),
        "web_extract",
    )
    .await?;

    if let Some(document) = data["results"]
        .as_array()
        .and_then(|results| results.first())
        .and_then(|value| normalize_tavily_document(value, max_chars, Some(url)))
    {
        return Ok(document);
    }

    let failure = data["failed_results"]
        .as_array()
        .and_then(|results| results.first())
        .and_then(|value| value["error"].as_str())
        .unwrap_or("Tavily extraction returned no document.");

    Err(ToolError::ExecutionFailed {
        tool: "web_extract".into(),
        message: failure.to_string(),
    })
}

async fn collect_firecrawl_crawl_pages(
    mut response: serde_json::Value,
    max_chars: usize,
    instructions: Option<&str>,
) -> Result<Vec<CrawledPage>, ToolError> {
    let mut pages = Vec::new();
    let mut seen = HashSet::new();

    loop {
        if let Some(results) = response["data"].as_array() {
            for value in results {
                let Some(document) = normalize_firecrawl_document(value, max_chars, None) else {
                    continue;
                };
                if !seen.insert(document.url.clone()) {
                    continue;
                }
                let page_title = document.title.clone();
                let page_content = document.content.clone();
                pages.push(CrawledPage {
                    score: rank_page(&page_title, &page_content, instructions),
                    url: document.url,
                    title: document.title,
                    content: document.content,
                    depth: 0,
                    extractor: document.extractor,
                    content_type: document.content_type,
                    content_format: document.content_format,
                    truncated: document.truncated,
                    meta_description: document.meta_description,
                });
            }
        }

        let Some(next) = response["next"].as_str().filter(|next| !next.is_empty()) else {
            break;
        };
        response = firecrawl_request(reqwest::Method::GET, next, None, "web_crawl").await?;
    }

    Ok(pages)
}

fn firecrawl_same_path_patterns(start_url: &Url) -> Option<Vec<String>> {
    let path = start_url.path().trim_end_matches('/');
    if path.is_empty() || path == "/" {
        None
    } else {
        Some(vec![format!(
            "^/?{}(?:/.*)?$",
            regex::escape(path.trim_start_matches('/'))
        )])
    }
}

async fn crawl_via_firecrawl(
    start_url: &Url,
    instructions: Option<&str>,
    max_pages: usize,
    max_depth: usize,
    max_chars: usize,
    same_path_only: bool,
) -> Result<Vec<CrawledPage>, ToolError> {
    let mut payload = json!({
        "url": start_url.as_str(),
        "limit": max_pages,
        "maxDiscoveryDepth": max_depth,
        "allowExternalLinks": false,
        "allowSubdomains": false,
        "crawlEntireDomain": !same_path_only,
        "scrapeOptions": {
            "formats": ["markdown", "links"],
            "onlyMainContent": true,
        },
    });
    if let Some(instructions) = instructions {
        payload["prompt"] = serde_json::Value::String(instructions.to_string());
    }
    if same_path_only {
        if let Some(patterns) = firecrawl_same_path_patterns(start_url) {
            payload["includePaths"] = serde_json::Value::Array(
                patterns
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            );
        }
    }

    let started =
        firecrawl_request(reqwest::Method::POST, "crawl", Some(payload), "web_crawl").await?;
    let job_id = started["id"]
        .as_str()
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool: "web_crawl".into(),
            message: "Firecrawl crawl did not return a job id.".into(),
        })?;

    let mut attempts = 0usize;
    loop {
        let status = firecrawl_request(
            reqwest::Method::GET,
            &format!("crawl/{job_id}"),
            None,
            "web_crawl",
        )
        .await?;
        match status["status"].as_str().unwrap_or("completed") {
            "completed" => {
                return collect_firecrawl_crawl_pages(status, max_chars, instructions).await;
            }
            "failed" => {
                let failure = status["error"]
                    .as_str()
                    .or_else(|| {
                        status["data"].as_array().and_then(|data| {
                            data.iter()
                                .find_map(|value| value["metadata"]["error"].as_str())
                        })
                    })
                    .unwrap_or("Firecrawl crawl failed.");
                return Err(ToolError::ExecutionFailed {
                    tool: "web_crawl".into(),
                    message: failure.to_string(),
                });
            }
            _ => {
                attempts += 1;
                if attempts >= 45 {
                    return Err(ToolError::ExecutionFailed {
                        tool: "web_crawl".into(),
                        message: "Firecrawl crawl timed out waiting for completion.".into(),
                    });
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

async fn crawl_via_tavily(
    url: &str,
    instructions: Option<&str>,
    max_pages: usize,
    max_chars: usize,
) -> Result<Vec<CrawledPage>, ToolError> {
    let mut payload = json!({
        "url": url,
        "limit": max_pages,
        "extract_depth": "advanced",
    });
    if let Some(instructions) = instructions {
        payload["instructions"] = serde_json::Value::String(instructions.to_string());
    }

    let data = tavily_request("crawl", payload, "web_crawl").await?;
    let mut pages = Vec::new();

    if let Some(results) = data["results"].as_array() {
        for value in results {
            if let Some(document) = normalize_tavily_document(value, max_chars, Some(url)) {
                let page_title = document.title.clone();
                let page_content = document.content.clone();
                pages.push(CrawledPage {
                    score: rank_page(&page_title, &page_content, instructions),
                    url: document.url,
                    title: document.title,
                    content: document.content,
                    depth: 0,
                    extractor: document.extractor,
                    content_type: document.content_type,
                    content_format: document.content_format,
                    truncated: document.truncated,
                    meta_description: document.meta_description,
                });
            }
        }
    }

    Ok(pages)
}

#[async_trait]
impl ToolHandler for WebSearchTool {
    fn name(&self) -> &'static str {
        "web_search"
    }

    fn toolset(&self) -> &'static str {
        "web"
    }

    fn emoji(&self) -> &'static str {
        "🔍"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_search".into(),
            description: "Search the web for information. Returns titles, URLs, and snippets.\n\
                          Supports pluggable backends (auto, Firecrawl, Tavily, Brave, DuckDuckGo). \
                          For best results: set FIRECRAWL_API_KEY (https://firecrawl.dev), \
                          TAVILY_API_KEY (https://app.tavily.com, free tier) \
                          or BRAVE_API_KEY (https://api.search.brave.com/app/keys, free tier). \
                          Without an API key, falls back to DuckDuckGo Instant Answers which only \
                          covers well-known topics."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 5)"
                    },
                    "backend": {
                        "type": "string",
                        "description": "Optional backend override: auto, firecrawl, tavily, brave, or duckduckgo"
                    }
                },
                "required": ["query"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: SearchArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "web_search".into(),
                message: e.to_string(),
            })?;

        let max = args.max_results.unwrap_or(5).min(20);

        let (backend_name, note, results) = match resolve_search_backend(args.backend.as_deref())? {
            SearchBackend::Firecrawl => {
                ("Firecrawl", None, search_firecrawl(&args.query, max).await?)
            }
            SearchBackend::Tavily => ("Tavily", None, search_tavily(&args.query, max).await?),
            SearchBackend::Brave => ("Brave Search", None, search_brave(&args.query, max).await?),
            SearchBackend::DuckDuckGo => {
                let results = search_duckduckgo(&args.query, max).await?;
                (
                    "DuckDuckGo",
                    Some(
                        "DuckDuckGo Instant Answers is the no-key fallback and only covers well-known topics. Set TAVILY_API_KEY or BRAVE_API_KEY in ~/.edgecrab/.env for broader search."
                            .to_string(),
                    ),
                    results,
                )
            }
        };

        Ok(json!({
            "success": true,
            "query": args.query,
            "backend": backend_name,
            "note": note,
            "results": results,
        })
        .to_string())
    }
}

inventory::submit!(&WebSearchTool as &dyn ToolHandler);

// ─── web_extract ───────────────────────────────────────────────

pub struct WebExtractTool;

pub struct WebCrawlTool;

#[derive(Deserialize)]
struct ExtractArgs {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    urls: Option<Vec<String>>,
    /// Maximum characters of content to return
    #[serde(default)]
    max_chars: Option<usize>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    render_js_fallback: Option<bool>,
}

#[derive(Serialize)]
struct ExtractBatchEntry {
    url: String,
    success: bool,
    result: Option<ExtractedDocument>,
    error: Option<String>,
}

fn requested_extract_urls(args: &ExtractArgs) -> Result<Vec<String>, ToolError> {
    let mut requested = Vec::new();

    if let Some(url) = args.url.as_ref().filter(|url| !url.trim().is_empty()) {
        requested.push(url.trim().to_string());
    }

    if let Some(urls) = &args.urls {
        for url in urls {
            let trimmed = url.trim();
            if trimmed.is_empty() || requested.iter().any(|existing| existing == trimmed) {
                continue;
            }
            requested.push(trimmed.to_string());
        }
    }

    if requested.is_empty() {
        return Err(ToolError::InvalidArgs {
            tool: "web_extract".into(),
            message: "Provide either 'url' or 'urls'.".into(),
        });
    }

    requested.truncate(5);
    Ok(requested)
}

fn parse_extract_url(requested: &str) -> Result<Url, ToolError> {
    validate_url(requested, "web_extract")?;
    Url::parse(requested).map_err(|e| ToolError::InvalidArgs {
        tool: "web_extract".into(),
        message: format!("Invalid URL: {e}"),
    })
}

async fn extract_document_for_url(
    requested_url: &Url,
    backend: ContentBackend,
    max_chars: usize,
    render_js_fallback: bool,
    ctx: &ToolContext,
) -> Result<ExtractedDocument, ToolError> {
    match backend {
        ContentBackend::Firecrawl => extract_via_firecrawl(requested_url.as_str(), max_chars).await,
        ContentBackend::Tavily => extract_via_tavily(requested_url.as_str(), max_chars).await,
        ContentBackend::Browser => {
            fetch_browser_document(requested_url, "text/html", max_chars, ctx, "web_extract")
                .await
                .map(|page| page.document)
        }
        ContentBackend::Native => {
            let client = build_client()?;
            let resp = client
                .get(requested_url.as_str())
                .header(
                    "User-Agent",
                    "EdgeCrab/0.1 (agent; +https://github.com/raphaelmansuy/edgecrab)",
                )
                .send()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool: "web_extract".into(),
                    message: format!("HTTP error: {e}"),
                })?;

            if !resp.status().is_success() {
                return Err(ToolError::ExecutionFailed {
                    tool: "web_extract".into(),
                    message: format!("HTTP {}: {}", resp.status(), requested_url),
                });
            }

            let final_url = resp.url().clone();
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = resp.bytes().await.map_err(|e| ToolError::ExecutionFailed {
                tool: "web_extract".into(),
                message: format!("Body read error: {e}"),
            })?;

            fetch_native_document(
                &final_url,
                &content_type,
                body.as_ref(),
                max_chars,
                "web_extract",
                ctx,
                render_js_fallback,
            )
            .await
            .map(|page| page.document)
        }
    }
}

#[derive(Deserialize)]
struct CrawlArgs {
    url: String,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    max_pages: Option<usize>,
    #[serde(default)]
    max_depth: Option<usize>,
    #[serde(default)]
    max_chars_per_page: Option<usize>,
    #[serde(default)]
    same_path_only: Option<bool>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    render_js_fallback: Option<bool>,
}

#[derive(Serialize)]
struct CrawledPage {
    url: String,
    title: String,
    content: String,
    depth: usize,
    score: i32,
    extractor: String,
    content_type: String,
    content_format: String,
    truncated: bool,
    meta_description: Option<String>,
}

#[async_trait]
impl ToolHandler for WebExtractTool {
    fn name(&self) -> &'static str {
        "web_extract"
    }

    fn toolset(&self) -> &'static str {
        "web"
    }

    fn emoji(&self) -> &'static str {
        "🌐"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_extract".into(),
            description: "Extract readable content from one or more URLs. Accepts EdgeCrab's single `url` form and `urls` arrays (up to 5 URLs). Returns structured JSON with content, metadata, backend selection, PDF extraction via EdgeParse, and browser-rendered fallback for JS-heavy pages.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Single URL to extract content from. Provide this or `urls`."
                    },
                    "urls": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of URLs to extract (max 5 per call). Provide this or `url`.",
                        "maxItems": 5
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum characters to return (default: 8000)"
                    },
                    "backend": {
                        "type": "string",
                        "description": "Optional backend override: auto, native, firecrawl, tavily, or browser"
                    },
                    "render_js_fallback": {
                        "type": "boolean",
                        "description": "When true (default), try a browser-rendered fallback for JS-heavy pages when native extraction is too thin"
                    }
                }
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: ExtractArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "web_extract".into(),
                message: e.to_string(),
            })?;

        let requested_urls = requested_extract_urls(&args)?;
        let max_chars = args.max_chars.unwrap_or(8_000).min(50_000);
        let backend = resolve_content_backend(args.backend.as_deref(), "web_extract")?;
        let render_js_fallback = args.render_js_fallback.unwrap_or(true);
        let backend_name = content_backend_name(backend);
        let batch_mode = requested_urls.len() > 1 || args.urls.is_some();

        if !batch_mode {
            let only_url = &requested_urls[0];
            let parsed = parse_extract_url(only_url)?;
            let document =
                extract_document_for_url(&parsed, backend, max_chars, render_js_fallback, ctx)
                    .await?;

            return Ok(json!({
                "success": true,
                "backend": backend_name,
                "result": document.clone(),
                "results": [document],
            })
            .to_string());
        }

        let mut results = Vec::with_capacity(requested_urls.len());
        for requested in requested_urls {
            let entry = match parse_extract_url(&requested) {
                Ok(parsed) => match extract_document_for_url(
                    &parsed,
                    backend,
                    max_chars,
                    render_js_fallback,
                    ctx,
                )
                .await
                {
                    Ok(document) => ExtractBatchEntry {
                        url: requested,
                        success: true,
                        result: Some(document),
                        error: None,
                    },
                    Err(error) => ExtractBatchEntry {
                        url: requested,
                        success: false,
                        result: None,
                        error: Some(error.to_string()),
                    },
                },
                Err(error) => ExtractBatchEntry {
                    url: requested,
                    success: false,
                    result: None,
                    error: Some(error.to_string()),
                },
            };
            results.push(entry);
        }

        let success_count = results.iter().filter(|entry| entry.success).count();
        Ok(json!({
            "success": success_count > 0,
            "backend": backend_name,
            "results": results,
        })
        .to_string())
    }
}

inventory::submit!(&WebExtractTool as &dyn ToolHandler);

#[async_trait]
impl ToolHandler for WebCrawlTool {
    fn name(&self) -> &'static str {
        "web_crawl"
    }

    fn toolset(&self) -> &'static str {
        "web"
    }

    fn emoji(&self) -> &'static str {
        "🕸️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_crawl".into(),
            description: "Recursively crawl a website starting from a URL. Returns structured JSON with up to 20 in-scope pages, readable content, extraction metadata, backend selection, PDF support, and browser-rendered fallback for JS-heavy pages. Use instructions to bias which pages are kept in the final output.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Starting URL to crawl"
                    },
                    "instructions": {
                        "type": "string",
                        "description": "Optional focus instructions such as 'find API docs' or 'look for pricing pages'"
                    },
                    "max_pages": {
                        "type": "integer",
                        "description": "Maximum pages to return and visit (default: 8, max: 20)"
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum link depth from the start URL (default: 2, max: 4)"
                    },
                    "max_chars_per_page": {
                        "type": "integer",
                        "description": "Maximum readable characters to keep per page (default: 4000, max: 12000)"
                    },
                    "same_path_only": {
                        "type": "boolean",
                        "description": "When true, only follow links under the starting path prefix instead of the whole host"
                    },
                    "backend": {
                        "type": "string",
                        "description": "Optional backend override: auto, native, firecrawl, tavily, or browser"
                    },
                    "render_js_fallback": {
                        "type": "boolean",
                        "description": "When true (default), try browser-rendered extraction for thin JS-heavy pages during native crawl"
                    }
                },
                "required": ["url"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: CrawlArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "web_crawl".into(),
            message: e.to_string(),
        })?;

        let max_pages = args.max_pages.unwrap_or(8).clamp(1, 20);
        let max_depth = args.max_depth.unwrap_or(2).min(4);
        let max_chars_per_page = args.max_chars_per_page.unwrap_or(4_000).clamp(500, 12_000);
        let same_path_only = args.same_path_only.unwrap_or(false);
        let backend = resolve_content_backend(args.backend.as_deref(), "web_crawl")?;
        let render_js_fallback = args.render_js_fallback.unwrap_or(true);
        let backend_name = content_backend_name(backend);

        validate_url(&args.url, "web_crawl")?;
        let start_url = Url::parse(&args.url).map_err(|e| ToolError::InvalidArgs {
            tool: "web_crawl".into(),
            message: format!("Invalid URL: {e}"),
        })?;

        if matches!(backend, ContentBackend::Firecrawl | ContentBackend::Tavily) {
            let mut pages = match backend {
                ContentBackend::Firecrawl => {
                    crawl_via_firecrawl(
                        &start_url,
                        args.instructions.as_deref(),
                        max_pages,
                        max_depth,
                        max_chars_per_page,
                        same_path_only,
                    )
                    .await?
                }
                ContentBackend::Tavily => {
                    crawl_via_tavily(
                        start_url.as_str(),
                        args.instructions.as_deref(),
                        max_pages,
                        max_chars_per_page,
                    )
                    .await?
                }
                ContentBackend::Native | ContentBackend::Browser => unreachable!("handled below"),
            };
            pages.sort_by(|left, right| {
                right
                    .score
                    .cmp(&left.score)
                    .then(left.depth.cmp(&right.depth))
                    .then(left.url.cmp(&right.url))
            });
            pages.truncate(max_pages);

            return Ok(json!({
                "success": true,
                "backend": backend_name,
                "start_url": args.url,
                "instructions": args.instructions,
                "pages_visited": pages.len(),
                "results": pages,
            })
            .to_string());
        }

        let client = match backend {
            ContentBackend::Native => Some(build_client()?),
            ContentBackend::Browser | ContentBackend::Firecrawl | ContentBackend::Tavily => None,
        };
        let mut queue = VecDeque::from([(start_url.clone(), 0usize)]);
        let mut visited: HashSet<String> = HashSet::new();
        let mut pages: Vec<CrawledPage> = Vec::new();

        while let Some((current_url, depth)) = queue.pop_front() {
            let current_key = current_url.to_string();
            if !visited.insert(current_key.clone()) {
                continue;
            }

            validate_url(&current_key, "web_crawl")?;

            let fetched = match backend {
                ContentBackend::Browser => {
                    fetch_browser_document(
                        &current_url,
                        "text/html",
                        max_chars_per_page,
                        ctx,
                        "web_crawl",
                    )
                    .await?
                }
                ContentBackend::Native => {
                    let response = client
                        .as_ref()
                        .expect("native client")
                        .get(current_url.clone())
                        .header(
                            "User-Agent",
                            "EdgeCrab/0.1 (agent; +https://github.com/raphaelmansuy/edgecrab)",
                        )
                        .send()
                        .await
                        .map_err(|e| ToolError::ExecutionFailed {
                            tool: "web_crawl".into(),
                            message: format!("HTTP error fetching {current_key}: {e}"),
                        })?;

                    if !response.status().is_success() {
                        continue;
                    }

                    let final_url = response.url().clone();
                    let final_url_string = final_url.to_string();
                    validate_url(&final_url_string, "web_crawl")?;

                    let content_type = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    let body = response
                        .bytes()
                        .await
                        .map_err(|e| ToolError::ExecutionFailed {
                            tool: "web_crawl".into(),
                            message: format!("Body read error for {final_url_string}: {e}"),
                        })?;

                    fetch_native_document(
                        &final_url,
                        &content_type,
                        body.as_ref(),
                        max_chars_per_page,
                        "web_crawl",
                        ctx,
                        render_js_fallback,
                    )
                    .await?
                }
                ContentBackend::Firecrawl | ContentBackend::Tavily => {
                    unreachable!("handled earlier")
                }
            };

            pages.push(CrawledPage {
                score: rank_page(
                    &fetched.document.title,
                    &fetched.document.content,
                    args.instructions.as_deref(),
                ),
                url: fetched.document.url,
                title: fetched.document.title,
                content: fetched.document.content,
                depth,
                extractor: fetched.document.extractor,
                content_type: fetched.document.content_type,
                content_format: fetched.document.content_format,
                truncated: fetched.document.truncated,
                meta_description: fetched.document.meta_description,
            });

            if depth >= max_depth || visited.len() >= max_pages {
                continue;
            }

            for link in fetched.links {
                if visited.len() + queue.len() >= max_pages {
                    break;
                }
                let Ok(candidate) = Url::parse(&link) else {
                    continue;
                };
                if !host_matches(&start_url, &candidate) {
                    continue;
                }
                if !path_in_scope(&start_url, &candidate, !same_path_only) {
                    continue;
                }
                if visited.contains(candidate.as_str()) {
                    continue;
                }
                queue.push_back((candidate, depth + 1));
            }
        }

        pages.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then(left.depth.cmp(&right.depth))
                .then(left.url.cmp(&right.url))
        });
        pages.truncate(max_pages);

        Ok(json!({
            "success": true,
            "backend": backend_name,
            "start_url": args.url,
            "instructions": args.instructions,
            "pages_visited": visited.len(),
            "results": pages,
        })
        .to_string())
    }
}

inventory::submit!(&WebCrawlTool as &dyn ToolHandler);

// ─── Shared helpers ────────────────────────────────────────────

/// Percent-encode a query string for URL embedding.
fn urlencoding_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            other => {
                let bytes = other.to_string().into_bytes();
                bytes.iter().map(|b| format!("%{:02X}", b)).collect()
            }
        })
        .collect()
}

/// Validate a URL with the SSRF guard.
fn validate_url(url: &str, tool: &str) -> Result<(), ToolError> {
    match edgecrab_security::url_safety::is_safe_url(url) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ToolError::PermissionDenied(format!(
            "URL blocked by SSRF policy for tool '{tool}': {url}"
        ))),
        Err(e) => Err(ToolError::PermissionDenied(format!(
            "URL validation error in '{tool}': {e}"
        ))),
    }
}

/// Build a shared reqwest client with a sensible timeout.
///
/// WHY 15-second timeout: Balances responsiveness vs. slow websites.
/// This timeout balances responsiveness against slow websites.
fn build_client() -> Result<reqwest::Client, ToolError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "web".into(),
            message: format!("Failed to build HTTP client: {e}"),
        })
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_basic() {
        let html = "<h1>Hello</h1><p>World &amp; stuff</p>";
        let text = strip_html(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World & stuff"));
        assert!(!text.contains('<'));
    }

    #[test]
    fn strip_html_whitespace_collapsed() {
        let html = "<p>  multiple   spaces  </p>";
        let text = strip_html(html);
        assert_eq!(text, "multiple spaces");
    }

    #[test]
    fn readable_text_prefers_main_content_and_removes_noise() {
        let html = r#"
            <html>
              <body>
                <nav>Docs Pricing Blog</nav>
                <main>
                  <h1>EdgeCrab</h1>
                  <p>Web tools should return structured data.</p>
                </main>
                <footer>Footer links</footer>
                <script>console.log("noise")</script>
              </body>
            </html>
        "#;
        let text = extract_readable_text(html);
        assert!(text.contains("EdgeCrab"));
        assert!(text.contains("structured data"));
        assert!(!text.contains("Docs Pricing Blog"));
        assert!(!text.contains("console.log"));
    }

    #[test]
    fn meta_description_extracted() {
        let html = r#"<meta name="description" content="Fast web extraction for agents">"#;
        assert_eq!(
            extract_meta_description(html).as_deref(),
            Some("Fast web extraction for agents")
        );
    }

    #[test]
    fn truncate_chars_preserves_utf8() {
        let input = "🙂".repeat(10);
        let (output, truncated) = truncate_chars(input, 9);
        assert!(truncated);
        assert!(output.contains("truncated"));
    }

    #[test]
    fn pdf_detection_accepts_content_type_or_magic_bytes() {
        let url = Url::parse("https://example.com/report").expect("url");
        assert!(is_pdf_response(&url, "application/pdf", b"not pdf"));
        assert!(is_pdf_response(&url, "", b"%PDF-1.7"));
    }

    #[test]
    fn infer_title_from_url_falls_back_when_path_empty() {
        let url = Url::parse("https://example.com/").expect("url");
        assert_eq!(infer_title_from_url(&url, "document.pdf"), "document.pdf");
    }

    #[test]
    fn rendered_fallback_triggers_for_spa_shells_with_thin_content() {
        let document = ExtractedDocument {
            url: "https://example.com/app".into(),
            title: "".into(),
            content: "Loading...".into(),
            extractor: "readable_html".into(),
            content_type: "text/html".into(),
            content_format: "text".into(),
            truncated: false,
            meta_description: None,
        };
        let html = r#"
            <html>
              <body>
                <div id="__next"></div>
                <script src="/_next/static/chunks/main.js"></script>
                <script src="/_next/static/chunks/app.js"></script>
                <script>window.__DATA__ = {};</script>
              </body>
            </html>
        "#;
        assert!(should_try_rendered_fallback(&document, html, "text/html"));
    }

    #[test]
    fn merge_links_deduplicates_while_preserving_order() {
        let merged = merge_links(
            vec![
                "https://example.com/a".into(),
                "https://example.com/b".into(),
            ],
            vec![
                "https://example.com/b".into(),
                "https://example.com/c".into(),
            ],
        );
        assert_eq!(
            merged,
            vec![
                "https://example.com/a",
                "https://example.com/b",
                "https://example.com/c",
            ]
        );
    }

    #[test]
    fn tavily_document_normalization_preserves_shape() {
        let value = json!({
            "url": "https://example.com/doc",
            "title": "Example Doc",
            "raw_content": "alpha beta gamma",
            "content_type": "text/html",
            "description": "summary",
        });
        let document = normalize_tavily_document(&value, 100, None).expect("normalized document");
        assert_eq!(document.url, "https://example.com/doc");
        assert_eq!(document.extractor, "tavily");
        assert_eq!(document.meta_description.as_deref(), Some("summary"));
    }

    #[test]
    fn firecrawl_search_normalization_preserves_shape() {
        let value = json!({
            "data": {
                "web": [{
                    "title": "EdgeCrab Docs",
                    "url": "https://example.com/docs",
                    "description": "Structured web tooling",
                }]
            }
        });
        let results = normalize_firecrawl_search_results(&value, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "EdgeCrab Docs");
        assert_eq!(results[0].description, "Structured web tooling");
    }

    #[test]
    fn firecrawl_document_normalization_prefers_markdown_and_metadata() {
        let value = json!({
            "markdown": "# EdgeCrab",
            "metadata": {
                "url": "https://example.com/docs",
                "title": "EdgeCrab Docs",
                "description": "Premium web extraction",
                "contentType": "text/html",
            }
        });
        let document =
            normalize_firecrawl_document(&value, 100, None).expect("normalized document");
        assert_eq!(document.url, "https://example.com/docs");
        assert_eq!(document.extractor, "firecrawl");
        assert_eq!(document.content_format, "markdown");
        assert_eq!(
            document.meta_description.as_deref(),
            Some("Premium web extraction")
        );
    }

    #[test]
    fn urlencoding_spaces() {
        let encoded = urlencoding_encode("hello world");
        assert_eq!(encoded, "hello+world");
    }

    #[test]
    fn urlencoding_special_chars() {
        let encoded = urlencoding_encode("foo&bar=baz");
        assert!(!encoded.contains('&'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn web_search_available() {
        assert!(WebSearchTool.is_available());
    }

    #[test]
    fn web_extract_available() {
        assert!(WebExtractTool.is_available());
    }

    #[test]
    fn web_extract_schema_avoids_top_level_combinators() {
        let schema = WebExtractTool.schema();
        let params = schema.parameters;
        assert_eq!(params["type"], "object");
        assert!(
            params.get("anyOf").is_none(),
            "top-level anyOf is unsupported"
        );
        assert!(
            params.get("oneOf").is_none(),
            "top-level oneOf is unsupported"
        );
        assert!(
            params.get("allOf").is_none(),
            "top-level allOf is unsupported"
        );
        assert!(params.get("not").is_none(), "top-level not is unsupported");
    }

    #[test]
    fn requested_extract_urls_accepts_single_or_batch_contracts() {
        let single = requested_extract_urls(&ExtractArgs {
            url: Some("https://example.com/a".into()),
            urls: None,
            max_chars: None,
            backend: None,
            render_js_fallback: None,
        })
        .expect("single url");
        assert_eq!(single, vec!["https://example.com/a"]);

        let batch = requested_extract_urls(&ExtractArgs {
            url: Some("https://example.com/a".into()),
            urls: Some(vec![
                "https://example.com/a".into(),
                "https://example.com/b".into(),
                " https://example.com/c ".into(),
            ]),
            max_chars: None,
            backend: None,
            render_js_fallback: None,
        })
        .expect("batch urls");
        assert_eq!(
            batch,
            vec![
                "https://example.com/a",
                "https://example.com/b",
                "https://example.com/c",
            ]
        );
    }

    #[tokio::test]
    async fn web_extract_batch_returns_per_url_errors_without_network() {
        let ctx = crate::registry::ToolContext::test_context();
        let result = WebExtractTool
            .execute(
                json!({
                    "urls": [
                        "notaurl",
                        "http://127.0.0.1:8080/private"
                    ]
                }),
                &ctx,
            )
            .await
            .expect("batch response");

        let parsed: serde_json::Value = serde_json::from_str(&result).expect("json");
        let results = parsed["results"].as_array().expect("results array");
        assert_eq!(results.len(), 2);
        assert_eq!(parsed["success"], false);
        assert!(results.iter().all(|entry| entry["success"] == false));
    }

    #[test]
    fn web_crawl_available() {
        assert!(WebCrawlTool.is_available());
    }

    #[test]
    fn extract_links_resolves_relative_links() {
        let base = Url::parse("https://example.com/docs/").expect("url");
        let html = r##"
            <a href="guide.html">Guide</a>
            <a href="/docs/api">API</a>
            <a href="#fragment">Skip</a>
            <a href="mailto:test@example.com">Mail</a>
        "##;

        let links = extract_links(&base, html);
        assert!(links.contains(&"https://example.com/docs/guide.html".to_string()));
        assert!(links.contains(&"https://example.com/docs/api".to_string()));
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn path_scope_respects_prefix() {
        let base = Url::parse("https://example.com/docs/").expect("url");
        let docs = Url::parse("https://example.com/docs/api").expect("url");
        let blog = Url::parse("https://example.com/blog/post").expect("url");

        assert!(path_in_scope(&base, &docs, false));
        assert!(!path_in_scope(&base, &blog, false));
        assert!(path_in_scope(&base, &blog, true));
    }

    #[test]
    fn validate_url_blocks_private() {
        // 127.0.0.1 is a loopback — SSRF check should block it
        let result = validate_url("http://127.0.0.1:8080/secret", "test");
        assert!(result.is_err());
    }

    #[test]
    fn validate_url_allows_public() {
        // Public DNS should pass. Note: actual connectivity not required.
        let result = validate_url("https://www.rust-lang.org/", "test");
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires internet — run with cargo test -- --include-ignored"]
    async fn web_search_live_query() {
        // Integration test: tries whichever backend is configured.
        // Set TAVILY_API_KEY for richer results.
        let ctx = ToolContext::test_context();
        let result = WebSearchTool
            .execute(
                serde_json::json!({"query": "Rust programming language"}),
                &ctx,
            )
            .await;
        match result {
            Ok(text) => {
                assert!(!text.is_empty(), "search result should not be empty");
            }
            Err(e) => {
                // Network errors are acceptable in CI — don't panic
                eprintln!("web_search live test skipped: {e}");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires internet — run with cargo test -- --include-ignored"]
    async fn web_search_ddg_known_entity() {
        // DDG Instant Answer works for well-known entities like "Paris"
        let ctx = ToolContext::test_context();
        let result = WebSearchTool
            .execute(serde_json::json!({"query": "Paris France"}), &ctx)
            .await;
        match result {
            Ok(text) => {
                println!("DDG result (partial): {}", crate::safe_truncate(&text, 300));
            }
            Err(e) => {
                eprintln!("Skipped: {e}");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires internet — run with cargo test -- --include-ignored"]
    async fn web_extract_live_page() {
        // Integration test: fetch a real page and extract text.
        let ctx = ToolContext::test_context();
        let result = WebExtractTool
            .execute(
                serde_json::json!({"url": "https://www.rust-lang.org/"}),
                &ctx,
            )
            .await;
        match result {
            Ok(text) => {
                assert!(!text.is_empty(), "extracted text should not be empty");
                assert!(
                    text.to_lowercase().contains("rust"),
                    "page should mention Rust"
                );
            }
            Err(e) => {
                eprintln!("web_extract live test skipped: {e}");
            }
        }
    }
}
