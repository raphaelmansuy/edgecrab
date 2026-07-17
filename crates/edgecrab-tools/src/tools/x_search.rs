//! `x_search` — X (Twitter) search via xAI Responses API (gap 027 / Hermes parity).
//!
//! Gated on `XAI_API_KEY` or SuperGrok OAuth token in auth store. Opt-in toolset `x_search`.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_security::url_safety::is_safe_url;
use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

const DEFAULT_BASE: &str = "https://api.x.ai/v1";
const DEFAULT_MODEL: &str = "grok-4-latest";

pub struct XSearchTool;

#[derive(Debug, Deserialize)]
struct XSearchArgs {
    query: String,
    #[serde(default = "default_max")]
    max_results: u32,
    #[serde(default)]
    from_date: Option<String>,
    #[serde(default)]
    to_date: Option<String>,
}

fn default_max() -> u32 {
    10
}

fn resolve_xai_bearer() -> Result<(String, String), ToolError> {
    if let Ok(key) = std::env::var("XAI_API_KEY") {
        let key = key.trim().to_string();
        if !key.is_empty() {
            let base = std::env::var("XAI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE.into());
            return Ok((key, base));
        }
    }
    // SuperGrok / xAI OAuth from ~/.edgecrab/auth.json (Hermes-compatible shape).
    if let Some(home) = std::env::var_os("EDGECRAB_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".edgecrab")))
    {
        let auth_path = home.join("auth.json");
        if let Ok(raw) = std::fs::read_to_string(&auth_path)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw)
        {
            for key in ["xai", "xai_oauth", "grok"] {
                if let Some(token) = val
                    .pointer(&format!("/providers/{key}/access_token"))
                    .or_else(|| val.pointer(&format!("/providers/{key}/token")))
                    .and_then(|v| v.as_str())
                {
                    let token = token.trim();
                    if !token.is_empty() {
                        return Ok((token.to_string(), DEFAULT_BASE.into()));
                    }
                }
            }
        }
    }
    Err(ToolError::ExecutionFailed {
        tool: "x_search".into(),
        message: "No xAI credentials. Set XAI_API_KEY or run `edgecrab auth add grok`.".into(),
    })
}

fn validate_date(label: &str, value: &str) -> Result<(), ToolError> {
    if value.len() != 10
        || value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
    {
        return Err(ToolError::InvalidArgs {
            tool: "x_search".into(),
            message: format!("{label} must be YYYY-MM-DD, got {value}"),
        });
    }
    Ok(())
}

#[async_trait]
impl ToolHandler for XSearchTool {
    fn name(&self) -> &'static str {
        "x_search"
    }
    fn toolset(&self) -> &'static str {
        "x_search"
    }
    fn emoji(&self) -> &'static str {
        "🐦"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "x_search".into(),
            description: "Search X (Twitter) posts and threads via xAI x_search. \
                 Requires XAI_API_KEY or SuperGrok OAuth. Opt-in toolset `x_search`."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 },
                    "from_date": { "type": "string", "description": "YYYY-MM-DD lower bound" },
                    "to_date": { "type": "string", "description": "YYYY-MM-DD upper bound" }
                },
                "required": ["query"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: XSearchArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "x_search".into(),
            message: e.to_string(),
        })?;
        if args.query.trim().is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "x_search".into(),
                message: "query must not be empty".into(),
            });
        }
        if let Some(ref d) = args.from_date {
            validate_date("from_date", d)?;
        }
        if let Some(ref d) = args.to_date {
            validate_date("to_date", d)?;
        }
        if let (Some(from), Some(to)) = (&args.from_date, &args.to_date)
            && from > to
        {
            return Err(ToolError::InvalidArgs {
                tool: "x_search".into(),
                message: "from_date must be <= to_date".into(),
            });
        }

        let (bearer, base) = resolve_xai_bearer()?;
        let url = format!("{}/responses", base.trim_end_matches('/'));
        match is_safe_url(&url) {
            Ok(true) => {}
            Ok(false) => {
                return Err(ToolError::ExecutionFailed {
                    tool: "x_search".into(),
                    message: format!("blocked unsafe xAI URL: {url}"),
                });
            }
            Err(e) => {
                return Err(ToolError::ExecutionFailed {
                    tool: "x_search".into(),
                    message: format!("URL validation failed: {e}"),
                });
            }
        }

        let mut user_content = args.query.clone();
        if let Some(ref from) = args.from_date {
            user_content.push_str(&format!("\nfrom_date={from}"));
        }
        if let Some(ref to) = args.to_date {
            user_content.push_str(&format!("\nto_date={to}"));
        }
        user_content.push_str(&format!(
            "\nReturn up to {} relevant posts with citations.",
            args.max_results.clamp(1, 50)
        ));

        let body = json!({
            "model": DEFAULT_MODEL,
            "input": [{"role": "user", "content": user_content}],
            "tools": [{"type": "x_search"}],
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "x_search".into(),
                message: format!("http client: {e}"),
            })?;

        let resp = client
            .post(&url)
            .bearer_auth(&bearer)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "x_search".into(),
                message: format!("xAI request failed: {e}"),
            })?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| ToolError::ExecutionFailed {
            tool: "x_search".into(),
            message: format!("failed to read xAI body: {e}"),
        })?;

        if !status.is_success() {
            return Err(ToolError::ExecutionFailed {
                tool: "x_search".into(),
                message: format!(
                    "xAI HTTP {status}: {}",
                    text.chars().take(400).collect::<String>()
                ),
            });
        }

        Ok(json!({
            "success": true,
            "query": args.query,
            "max_results": args.max_results.clamp(1, 50),
            "raw": serde_json::from_str::<serde_json::Value>(&text).unwrap_or(json!({ "text": text })),
        })
        .to_string())
    }
}

inventory::submit!(&XSearchTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_name() {
        assert_eq!(XSearchTool.schema().name, "x_search");
    }

    #[test]
    fn date_validation() {
        assert!(validate_date("from_date", "2026-01-01").is_ok());
        assert!(validate_date("from_date", "01-01-2026").is_err());
    }

    #[tokio::test]
    async fn missing_creds_errors() {
        let had = std::env::var("XAI_API_KEY").ok();
        unsafe { std::env::remove_var("XAI_API_KEY") };
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe { std::env::set_var("EDGECRAB_HOME", tmp.path()) };
        let ctx = ToolContext::test_context();
        let err = XSearchTool
            .execute(json!({"query": "rust"}), &ctx)
            .await
            .expect_err("creds required");
        let msg = format!("{err:?}");
        assert!(msg.contains("xAI") || msg.contains("credentials"));
        unsafe { std::env::remove_var("EDGECRAB_HOME") };
        if let Some(v) = had {
            unsafe { std::env::set_var("XAI_API_KEY", v) };
        }
    }
}
