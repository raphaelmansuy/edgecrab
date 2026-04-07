use edgecrab_tools::registry::ToolContext;
use edgequake_llm::{ChatMessage, CompletionOptions};
use lsp_types::Uri;
use lsp_types::{Diagnostic, DiagnosticSeverity};
use serde_json::{Value, json};

use crate::render::format_diagnostic;

fn context_snippet(text: &str, diagnostic: &Diagnostic) -> String {
    let start_line = diagnostic.range.start.line.saturating_sub(5) as usize;
    let end_line = (diagnostic.range.end.line + 6) as usize;
    text.lines()
        .enumerate()
        .filter(|(idx, _)| *idx >= start_line && *idx < end_line)
        .map(|(idx, line)| format!("{:>4}| {}", idx + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn enrich_diagnostics(
    ctx: &ToolContext,
    uri: &Uri,
    text: &str,
    diagnostics: &[Diagnostic],
) -> Result<Vec<Value>, edgecrab_types::ToolError> {
    let items: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let Some(provider) = ctx.provider.clone() else {
        return Ok(items
            .into_iter()
            .map(|diagnostic| {
                json!({
                    "original_diagnostic": format_diagnostic(diagnostic),
                    "explanation": "No enrichment model is available in this session.",
                    "suggested_fix": null,
                })
            })
            .collect());
    };

    let prompt = items
        .iter()
        .enumerate()
        .map(|(idx, diagnostic)| {
            let code = diagnostic
                .code
                .as_ref()
                .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "\"unknown\"".into()))
                .unwrap_or_else(|| "\"unknown\"".into());
            format!(
                "Diagnostic {idx}\nFile: {}\nCode: {code}\nMessage: {}\nRange: line {} col {} to line {} col {}\nContext:\n{}\n",
                uri.as_str(),
                diagnostic.message,
                diagnostic.range.start.line + 1,
                diagnostic.range.start.character + 1,
                diagnostic.range.end.line + 1,
                diagnostic.range.end.character + 1,
                context_snippet(text, diagnostic)
            )
        })
        .collect::<Vec<_>>()
        .join("\n---\n");

    let messages = vec![ChatMessage::user(format!(
        "You explain compiler and language-server diagnostics. Return strict JSON as an array. \
Each item must contain keys explanation and suggested_fix.\n\n{}",
        prompt
    ))];
    let options = CompletionOptions {
        temperature: Some(0.1),
        max_tokens: Some(2048),
        ..Default::default()
    };

    let response = provider
        .chat(&messages, Some(&options))
        .await
        .map_err(|err| edgecrab_types::ToolError::ExecutionFailed {
            tool: "lsp_enrich_diagnostics".into(),
            message: format!("enrichment request failed: {err}"),
        })?;
    let parsed: Vec<Value> = serde_json::from_str(response.content.trim()).unwrap_or_default();

    Ok(items
        .iter()
        .enumerate()
        .map(|(idx, diagnostic)| {
            let extra = parsed.get(idx).cloned().unwrap_or_else(|| {
                json!({
                    "explanation": response.content.trim(),
                    "suggested_fix": null,
                })
            });
            json!({
                "original_diagnostic": format_diagnostic(diagnostic),
                "explanation": extra.get("explanation").cloned().unwrap_or(Value::Null),
                "suggested_fix": extra.get("suggested_fix").cloned().unwrap_or(Value::Null),
            })
        })
        .collect())
}
