//! # pdf_to_markdown — Local PDF to Markdown conversion via EdgeParse
//!
//! WHY this tool exists: `web_extract` already uses EdgeParse for remote PDFs,
//! but local workflows need the same parser without shelling out or copy/paste.
//! This tool converts a local PDF file into Markdown using the integrated
//! `edgeparse-core` library.
//!
//! EdgeParse is a fast structural PDF parser, not OCR. It works best on digital
//! PDFs with embedded text and layout objects. Scanned image-only PDFs may
//! produce little or no text unless the PDF already contains searchable text.

use std::path::Path;

use async_trait::async_trait;
use edgeparse_core::api::config::{ImageOutput, OutputFormat, ProcessingConfig};
use serde::{Deserialize, Serialize};
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

use crate::path_utils::{jail_read_path_multi, jail_write_path_create_dirs};
use crate::registry::{ToolContext, ToolHandler};

const DEFAULT_MAX_CHARS: usize = 20_000;
const PDF_HEADER: &[u8] = b"%PDF-";

pub struct PdfToMarkdownTool;

#[derive(Deserialize)]
struct Args {
    path: String,
    #[serde(default)]
    output_path: Option<String>,
    #[serde(default = "default_max_chars")]
    max_chars: usize,
}

#[derive(Serialize)]
struct PdfToMarkdownResult {
    success: bool,
    path: String,
    output_path: Option<String>,
    extractor: &'static str,
    parsing_mode: &'static str,
    content_format: &'static str,
    truncated: bool,
    total_chars: usize,
    markdown: String,
}

fn default_max_chars() -> usize {
    DEFAULT_MAX_CHARS
}

fn edgeparse_config() -> ProcessingConfig {
    ProcessingConfig {
        formats: vec![OutputFormat::Markdown],
        image_output: ImageOutput::Off,
        ..ProcessingConfig::default()
    }
}

pub(crate) fn looks_like_pdf(bytes: &[u8]) -> bool {
    let scan_len = bytes.len().min(1024);
    bytes[..scan_len]
        .windows(PDF_HEADER.len())
        .any(|window| window == PDF_HEADER)
}

pub(crate) fn extract_pdf_markdown_from_bytes(
    bytes: &[u8],
    display_name: &str,
    tool: &str,
) -> Result<String, ToolError> {
    if !looks_like_pdf(bytes) {
        return Err(ToolError::InvalidArgs {
            tool: tool.into(),
            message: format!(
                "'{display_name}' does not look like a PDF. EdgeParse is a PDF parser, not a generic binary decoder."
            ),
        });
    }

    let doc =
        edgeparse_core::convert_bytes(bytes, display_name, &edgeparse_config()).map_err(|e| {
            ToolError::ExecutionFailed {
                tool: tool.into(),
                message: format!("EdgeParse PDF extraction failed: {e}"),
            }
        })?;

    edgeparse_core::output::markdown::to_markdown(&doc).map_err(|e| ToolError::ExecutionFailed {
        tool: tool.into(),
        message: format!("EdgeParse markdown rendering failed: {e}"),
    })
}

pub(crate) fn extract_pdf_markdown_from_path(path: &Path, tool: &str) -> Result<String, ToolError> {
    let bytes = std::fs::read(path).map_err(|e| ToolError::ExecutionFailed {
        tool: tool.into(),
        message: format!("Failed to read PDF '{}': {e}", path.display()),
    })?;
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.pdf");
    extract_pdf_markdown_from_bytes(&bytes, display_name, tool)
}

fn truncate_chars(text: &str, limit: usize) -> (String, bool) {
    let total_chars = text.chars().count();
    if total_chars <= limit {
        return (text.to_string(), false);
    }

    let truncated: String = text.chars().take(limit).collect();
    (format!("{truncated}… [truncated at {limit} chars]"), true)
}

#[async_trait]
impl ToolHandler for PdfToMarkdownTool {
    fn name(&self) -> &'static str {
        "pdf_to_markdown"
    }

    fn toolset(&self) -> &'static str {
        "file"
    }

    fn emoji(&self) -> &'static str {
        "📕"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "pdf_to_markdown".into(),
            description: "Convert a local PDF file to Markdown using the integrated EdgeParse parser. This is fast structural PDF parsing, not OCR: it works best on digital PDFs with embedded text and layout objects. Optionally save the generated Markdown to a local file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Local path to a PDF file inside the allowed workspace or configured roots"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Optional local path where the full generated Markdown should be written"
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum number of characters to return inline in the tool result (default: 20000)"
                    }
                },
                "required": ["path"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "pdf_to_markdown".into(),
            message: e.to_string(),
        })?;

        let path_policy = ctx.config.file_path_policy(&ctx.cwd);
        // Gateway adapters cache inbound PDF attachments to document_cache_dir().
        // Trust this directory so agents can process files received via messaging
        // platforms (WhatsApp, Telegram, etc.) without requiring it in allowed_roots.
        let document_cache = ctx.config.document_cache_dir();
        let resolved = jail_read_path_multi(&args.path, &path_policy, &[document_cache.as_path()])?;
        let markdown = extract_pdf_markdown_from_path(&resolved, "pdf_to_markdown")?;

        let output_path = if let Some(path) = args.output_path.as_deref() {
            let resolved_output = jail_write_path_create_dirs(path, &path_policy)?;
            std::fs::write(&resolved_output, &markdown).map_err(|e| {
                ToolError::ExecutionFailed {
                    tool: "pdf_to_markdown".into(),
                    message: format!(
                        "Failed to write Markdown output '{}': {e}",
                        resolved_output.display()
                    ),
                }
            })?;
            Some(resolved_output.display().to_string())
        } else {
            None
        };

        let total_chars = markdown.chars().count();
        let (markdown, truncated) = truncate_chars(&markdown, args.max_chars);
        let result = PdfToMarkdownResult {
            success: true,
            path: resolved.display().to_string(),
            output_path,
            extractor: "edgeparse",
            parsing_mode: "fast-structural-parse-not-ocr",
            content_format: "markdown",
            truncated,
            total_chars,
            markdown,
        };

        serde_json::to_string(&result).map_err(|e| ToolError::ExecutionFailed {
            tool: "pdf_to_markdown".into(),
            message: format!("Failed to serialize result: {e}"),
        })
    }
}

inventory::submit!(&PdfToMarkdownTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx_in(dir: &std::path::Path) -> ToolContext {
        let mut ctx = ToolContext::test_context();
        ctx.cwd = dir.to_path_buf();
        ctx
    }

    fn fixture_pdf() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../skills/research/ml-paper-writing/templates/icml2026/example_paper.pdf")
    }

    #[test]
    fn looks_like_pdf_accepts_offset_header() {
        assert!(looks_like_pdf(b"junk%PDF-1.7\nrest"));
    }

    #[tokio::test]
    async fn pdf_to_markdown_extracts_markdown_from_fixture() {
        let dir = TempDir::new().expect("tmpdir");
        let pdf_path = dir.path().join("paper.pdf");
        std::fs::copy(fixture_pdf(), &pdf_path).expect("copy fixture pdf");

        let ctx = ctx_in(dir.path());
        let result = PdfToMarkdownTool
            .execute(json!({"path": "paper.pdf", "max_chars": 4000}), &ctx)
            .await
            .expect("extract pdf");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");

        assert_eq!(value["success"], true);
        assert_eq!(value["extractor"], "edgeparse");
        assert_eq!(value["content_format"], "markdown");
        assert!(
            value["markdown"]
                .as_str()
                .is_some_and(|markdown| markdown.len() > 200),
            "expected substantial markdown output: {result}"
        );
    }

    #[tokio::test]
    async fn pdf_to_markdown_can_write_output_file() {
        let dir = TempDir::new().expect("tmpdir");
        let pdf_path = dir.path().join("paper.pdf");
        std::fs::copy(fixture_pdf(), &pdf_path).expect("copy fixture pdf");

        let ctx = ctx_in(dir.path());
        let result = PdfToMarkdownTool
            .execute(
                json!({"path": "paper.pdf", "output_path": "out/paper.md", "max_chars": 128}),
                &ctx,
            )
            .await
            .expect("extract pdf");
        let value: serde_json::Value = serde_json::from_str(&result).expect("json");
        let output_path = dir.path().join("out/paper.md");
        let expected_output = output_path.canonicalize().expect("canonical output");
        let reported_output =
            std::path::PathBuf::from(value["output_path"].as_str().expect("output_path string"))
                .canonicalize()
                .expect("canonical reported output");

        assert!(output_path.exists(), "expected markdown file to be written");
        let saved = std::fs::read_to_string(&output_path).expect("read saved markdown");
        assert!(saved.len() > 200, "expected full markdown to be saved");
        assert_eq!(reported_output, expected_output);
        assert_eq!(value["truncated"], true);
    }

    #[tokio::test]
    async fn pdf_to_markdown_rejects_non_pdf_input() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("notes.txt"), "not a pdf").expect("write txt");

        let ctx = ctx_in(dir.path());
        let err = PdfToMarkdownTool
            .execute(json!({"path": "notes.txt"}), &ctx)
            .await
            .expect_err("non-pdf should fail");

        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }
}
