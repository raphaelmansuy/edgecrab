//! # write_file — Create or overwrite files
//!
//! WHY full-file write: Some operations (generating new files, replacing
//! content entirely) are more natural as full writes. For partial edits,
//! the `patch` tool is preferred.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

use crate::edit_contract::{
    DEFAULT_MAX_MUTATION_PAYLOAD_BYTES, DEFAULT_MAX_MUTATION_PAYLOAD_KIB,
    enforce_write_payload_limit_with_max,
};
use crate::path_utils::{jail_write_path, jail_write_path_create_dirs};
use crate::registry::{ToolContext, ToolHandler};
use crate::tools::checkpoint::ensure_checkpoint;

pub struct WriteFileTool;

/// Intent declaration for the existing-file collision case (FP55).
///
/// `Overwrite` (default) preserves the FP51 behavior — rejection
/// includes a content preview and records a session snapshot so the
/// immediate retry succeeds without an extra `read_file` round trip.
///
/// `Abort` is the cheap path: a stat-only rejection (~120 bytes) with
/// no preview and no snapshot. Use when the caller's intent is to
/// create a new file and a collision should pick a different path.
#[derive(Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum IfExists {
    #[default]
    Overwrite,
    Abort,
}

#[derive(Deserialize)]
struct Args {
    path: String,
    content: String,
    #[serde(default)]
    create_dirs: bool,
    #[serde(default)]
    if_exists: IfExists,
}

#[async_trait]
impl ToolHandler for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn toolset(&self) -> &'static str {
        "file"
    }

    fn emoji(&self) -> &'static str {
        "✏️"
    }

    fn path_arguments(&self) -> &'static [&'static str] {
        &["path"]
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write_file".into(),
            description: format!(
                "Creates or fully replaces a file. Hard content limit: {DEFAULT_MAX_MUTATION_PAYLOAD_BYTES} bytes ({DEFAULT_MAX_MUTATION_PAYLOAD_KIB} KiB) per call. \
                 \n\
                 WHEN TO USE: \
                 (a) Brand-new file — set if_exists=\"abort\" so a path collision fails CHEAPLY (~120 bytes, no content preview); pick a different path. \
                 (b) Replace an existing file — set if_exists=\"overwrite\" (the default). On collision the rejection includes a short content preview AND records a session snapshot, so calling write_file AGAIN with the same args succeeds — you do NOT need a separate read_file round trip. \
                 \n\
                 PARTIAL EDITS: For ANY targeted modification to an existing file, use patch/apply_patch instead — far more token-efficient and avoids regenerating unchanged content. \
                 \n\
                 CONTRACT: `content` MUST be a string — use \"\" for an empty scaffold. For larger new files, write a small scaffold first then extend with patch/apply_patch."
            ),
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working directory"
                    },
                    "content": {
                        "type": "string",
                        "description": "Full content to write to the file. Use an empty string \"\" to create an empty scaffold."
                    },
                    "create_dirs": {
                        "type": "boolean",
                        "description": "Create parent directories if they don't exist. Set explicitly to true or false."
                    },
                    "if_exists": {
                        "type": "string",
                        "enum": ["overwrite", "abort"],
                        "description": "Intent when the file already exists. 'overwrite' (default): rejection on collision returns a content preview and records a session snapshot — retry the SAME call to succeed. 'abort': cheap rejection with no preview and no snapshot — use when intent is to create a NEW file and a different path should be chosen on collision."
                    }
                },
                "required": ["path", "content", "create_dirs"]
            }),
            strict: Some(true),
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "write_file".into(),
            message: e.to_string(),
        })?;

        // Auto-checkpoint before mutation
        ensure_checkpoint(ctx, &format!("before write_file: {}", args.path));

        let path_policy = ctx.config.file_path_policy(&ctx.cwd);

        // Path jail check — delegates security concern to path_utils (SRP).
        // For create_dirs=true the helper also creates parent directories.
        let resolved = if args.create_dirs {
            jail_write_path_create_dirs(&args.path, &path_policy)?
        } else {
            jail_write_path(&args.path, &path_policy)?
        };
        let file_exists = resolved.exists();
        let existing_file_is_empty = file_exists
            && std::fs::metadata(&resolved)
                .map(|meta| meta.is_file() && meta.len() == 0)
                .unwrap_or(false);

        if file_exists
            && !existing_file_is_empty
            && !crate::read_tracker::has_file_snapshot(&ctx.session_id, &resolved)
        {
            match args.if_exists {
                IfExists::Abort => {
                    // FP55 cheap path: model declared intent to create a NEW file.
                    // Return stat-only — no preview, no snapshot — so the model
                    // can quickly choose a different path.
                    let size = std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(0);
                    return Err(ToolError::InvalidArgs {
                        tool: "write_file".into(),
                        message: format!(
                            "'{path}' already exists ({size} bytes). \
                             Pick a different path, or call write_file again with \
                             if_exists=\"overwrite\" (the default) to replace it.",
                            path = args.path
                        ),
                    });
                }
                IfExists::Overwrite => {
                    // FP51 + FP55: include the current file content so the model
                    // can immediately compose a patch OR retry the write, AND
                    // record the read-tracker snapshot so the immediate retry
                    // satisfies the freshness guard without an extra read_file
                    // round trip.
                    //
                    // Token budget: cap at 600 chars to keep the rejection error
                    // small. The model only needs enough content to recognise the
                    // file and decide between retry / patch / abort. Full content
                    // remains accessible via read_file when truncation occurs.
                    const PREVIEW_LIMIT: usize = 600;
                    let current_content = std::fs::read_to_string(&resolved).unwrap_or_default();
                    let preview = crate::safe_truncate(&current_content, PREVIEW_LIMIT);
                    let truncated = preview.len() < current_content.len();
                    let trunc_note = if truncated {
                        format!(
                            "\n[...truncated — file has {} total bytes; \
                             read_file gives full content if needed.]",
                            current_content.len()
                        )
                    } else {
                        String::new()
                    };

                    // Record snapshot BEFORE returning the error so the
                    // immediate retry passes the freshness guard. Failure
                    // here is non-fatal — the model will retry, fail freshness,
                    // and learn to read_file. Logging would be noise.
                    let _ = crate::read_tracker::record_file_snapshot(
                        &ctx.session_id,
                        &resolved,
                    );

                    return Err(ToolError::InvalidArgs {
                        tool: "write_file".into(),
                        message: format!(
                            "'{path}' already exists. \
                             Snapshot recorded — retry the SAME write_file call to overwrite \
                             (no extra read_file needed). \
                             For targeted edits prefer patch/apply_patch — far more token-efficient.\n\
                             \n\
                             Current file content (preview):\n\
                             ---\n\
                             {preview}{trunc_note}\n\
                             ---",
                            path = args.path
                        ),
                    });
                }
            }
        }

        if file_exists {
            crate::read_tracker::guard_file_freshness(
                &ctx.session_id,
                "write_file",
                &args.path,
                &resolved,
            )?;
        }

        let content = args.content;

        enforce_write_payload_limit_with_max(
            "write_file",
            &args.path,
            &resolved,
            &content,
            ctx.config.max_write_payload_bytes(),
        )?;

        let bytes_written = content.len();

        tokio::fs::write(&resolved, &content)
            .await
            .map_err(|e| ToolError::Other(format!("Cannot write '{}': {}", args.path, e)))?;

        let _ = crate::read_tracker::record_file_snapshot(&ctx.session_id, &resolved);

        if bytes_written == 0 {
            Ok(format!(
                "Created empty scaffold at '{}'. Add content next with patch/apply_patch or another write_file call.",
                args.path
            ))
        } else {
            Ok(format!("Wrote {} bytes to '{}'", bytes_written, args.path))
        }
    }
}

inventory::submit!(&WriteFileTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::file_read::ReadFileTool;
    use tempfile::TempDir;

    fn ctx_in(dir: &std::path::Path) -> ToolContext {
        let mut ctx = ToolContext::test_context();
        ctx.cwd = dir.to_path_buf();
        ctx
    }

    #[tokio::test]
    async fn write_file_creates_new() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = WriteFileTool
            .execute(json!({"path": "new.txt", "content": "hello world"}), &ctx)
            .await
            .expect("write");

        assert!(result.contains("11 bytes"));
        let content = std::fs::read_to_string(dir.path().join("new.txt")).expect("read");
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn write_file_allows_new_empty_scaffold_with_empty_string() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = WriteFileTool
            .execute(json!({"path": "scaffold.md", "content": "", "create_dirs": false}), &ctx)
            .await
            .expect("create scaffold");

        assert!(result.contains("Created empty scaffold"));
        let content = std::fs::read_to_string(dir.path().join("scaffold.md")).expect("read");
        assert_eq!(content, "");
    }

    #[test]
    fn write_file_schema_is_strict_and_content_is_required_string() {
        let schema = WriteFileTool.schema();
        assert_eq!(schema.strict, Some(true));
        assert_eq!(schema.parameters["type"], "object");
        assert_eq!(schema.parameters["additionalProperties"], false);
        assert_eq!(
            schema.parameters["required"],
            json!(["path", "content", "create_dirs"])
        );
        // content must be "string" — NOT ["string", "null"]
        assert_eq!(
            schema.parameters["properties"]["content"]["type"],
            json!("string"),
            "content schema type must be 'string', not nullable. \
             See specs/improve_plan/06-write-file-fix.md for rationale."
        );
    }

    #[tokio::test]
    async fn write_file_rejects_missing_content_field() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        // Omitting content entirely should fail deserialization
        let result = WriteFileTool
            .execute(json!({"path": "test.txt", "create_dirs": false}), &ctx)
            .await;

        assert!(result.is_err(), "missing content field should be rejected");
    }

    #[tokio::test]
    async fn write_file_allows_empty_string_on_existing_empty_scaffold() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("audit.md"), "").expect("seed empty scaffold");
        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "write-file-empty-scaffold".into();

        let result = WriteFileTool
            .execute(json!({"path": "audit.md", "content": "", "create_dirs": false}), &ctx)
            .await
            .expect("existing empty scaffold should be reusable with empty string");

        assert!(result.contains("Created empty scaffold"));
        let content = std::fs::read_to_string(dir.path().join("audit.md")).expect("read");
        assert_eq!(content, "");
    }

    #[tokio::test]
    async fn write_file_creates_dirs() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let result = WriteFileTool
            .execute(
                json!({"path": "sub/dir/file.txt", "content": "nested", "create_dirs": true}),
                &ctx,
            )
            .await
            .expect("write");

        assert!(result.contains("6 bytes"));
        let content = std::fs::read_to_string(dir.path().join("sub/dir/file.txt")).expect("read");
        assert_eq!(content, "nested");
    }

    #[tokio::test]
    async fn write_file_overwrites() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("existing.txt"), "old").expect("seed");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "write-file-overwrite".into();

        ReadFileTool
            .execute(json!({"path": "existing.txt", "line_numbers": false}), &ctx)
            .await
            .expect("read existing file");

        WriteFileTool
            .execute(json!({"path": "existing.txt", "content": "new"}), &ctx)
            .await
            .expect("write");

        let content = std::fs::read_to_string(dir.path().join("existing.txt")).expect("read");
        assert_eq!(content, "new");
    }

    #[tokio::test]
    async fn write_file_allows_absolute_path_in_configured_allowed_root() {
        let dir = TempDir::new().expect("workspace");
        let extra = TempDir::new().expect("extra");
        let target = extra.path().join("shared.txt");

        let mut ctx = ctx_in(dir.path());
        ctx.config.file_allowed_roots = vec![extra.path().to_path_buf()];

        WriteFileTool
            .execute(
                json!({"path": target.to_string_lossy(), "content": "shared write"}),
                &ctx,
            )
            .await
            .expect("write");

        let content = std::fs::read_to_string(&target).expect("read");
        assert_eq!(content, "shared write");
    }

    #[tokio::test]
    async fn write_file_blocks_denylisted_subtree() {
        let dir = TempDir::new().expect("workspace");
        std::fs::create_dir_all(dir.path().join("secrets")).expect("create secrets");

        let mut ctx = ctx_in(dir.path());
        ctx.config.path_restrictions = vec![std::path::PathBuf::from("secrets")];

        let result = WriteFileTool
            .execute(
                json!({"path": "secrets/token.txt", "content": "nope", "create_dirs": true}),
                &ctx,
            )
            .await;

        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn write_file_maps_absolute_tmp_into_edgecrab_temp_root() {
        let dir = TempDir::new().expect("workspace");
        let edgecrab_home = TempDir::new().expect("edgecrab_home");
        let mut ctx = ctx_in(dir.path());
        ctx.config.edgecrab_home = edgecrab_home.path().to_path_buf();

        WriteFileTool
            .execute(
                json!({"path": "/tmp/summary.md", "content": "hello tmp"}),
                &ctx,
            )
            .await
            .expect("write virtual tmp");

        let content = std::fs::read_to_string(edgecrab_home.path().join("tmp/files/summary.md"))
            .expect("read mapped tmp file");
        assert_eq!(content, "hello tmp");
    }

    #[tokio::test]
    async fn write_file_rejects_oversized_payloads() {
        let dir = TempDir::new().expect("workspace");
        let ctx = ctx_in(dir.path());
        let oversized = "x".repeat(DEFAULT_MAX_MUTATION_PAYLOAD_BYTES + 1);

        let result = WriteFileTool
            .execute(json!({"path": "big.rs", "content": oversized}), &ctx)
            .await;

        let err = result.expect_err("oversized write must be rejected");
        assert!(
            err.to_string()
                .contains("Large single-call mutation payloads are unreliable")
        );
    }

    #[tokio::test]
    async fn write_file_rejects_oversized_overwrites() {
        let dir = TempDir::new().expect("workspace");
        std::fs::write(dir.path().join("existing.rs"), "fn main() {}\n").expect("seed");
        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "write-file-oversized-overwrite".into();

        ReadFileTool
            .execute(json!({"path": "existing.rs", "line_numbers": false}), &ctx)
            .await
            .expect("read existing file");

        let oversized = "y".repeat(DEFAULT_MAX_MUTATION_PAYLOAD_BYTES + 10);

        let result = WriteFileTool
            .execute(json!({"path": "existing.rs", "content": oversized}), &ctx)
            .await;

        let err = result.expect_err("oversized overwrite must be rejected");
        assert!(
            err.to_string()
                .contains("Refusing overwrite via write_file")
        );
    }

    #[tokio::test]
    async fn write_file_rejects_overwrite_when_file_changed_after_read() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("stale.txt"), "before\n").expect("seed");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "write-file-stale-guard".into();

        ReadFileTool
            .execute(json!({"path": "stale.txt", "line_numbers": false}), &ctx)
            .await
            .expect("read");

        std::fs::write(dir.path().join("stale.txt"), "external change\n").expect("modify");

        let err = WriteFileTool
            .execute(json!({"path": "stale.txt", "content": "replacement\n"}), &ctx)
            .await
            .expect_err("stale overwrite should be rejected");

        assert!(err.to_string().contains("modified since you last read it"));
        let content = std::fs::read_to_string(dir.path().join("stale.txt")).expect("read current");
        assert_eq!(content, "external change\n");
    }

    #[tokio::test]
    async fn write_file_rejects_blind_overwrite_without_prior_read() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("blind.txt"), "before\n").expect("seed");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "write-file-blind-overwrite".into();

        let err = WriteFileTool
            .execute(json!({"path": "blind.txt", "content": "replacement\n"}), &ctx)
            .await
            .expect_err("blind overwrite should be rejected");

        // FP55: rejection text must teach the model the retry protocol.
        let msg = err.to_string();
        assert!(
            msg.contains("already exists"),
            "expected existence message; got: {msg}"
        );
        assert!(
            msg.contains("Snapshot recorded"),
            "expected snapshot-recorded directive; got: {msg}"
        );
        assert!(
            msg.contains("retry the SAME write_file call"),
            "expected explicit retry directive; got: {msg}"
        );
        // File must NOT be touched on rejection.
        let content = std::fs::read_to_string(dir.path().join("blind.txt")).expect("read current");
        assert_eq!(content, "before\n");
    }

    #[tokio::test]
    async fn write_file_allows_overwrite_after_reread_refreshes_snapshot() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("fresh.txt"), "before\n").expect("seed");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "write-file-reread".into();

        ReadFileTool
            .execute(json!({"path": "fresh.txt", "line_numbers": false}), &ctx)
            .await
            .expect("read");
        std::fs::write(dir.path().join("fresh.txt"), "external change\n").expect("modify");
        ReadFileTool
            .execute(json!({"path": "fresh.txt", "line_numbers": false}), &ctx)
            .await
            .expect("reread");

        WriteFileTool
            .execute(json!({"path": "fresh.txt", "content": "replacement\n"}), &ctx)
            .await
            .expect("fresh overwrite after reread");

        let content = std::fs::read_to_string(dir.path().join("fresh.txt")).expect("read final");
        assert_eq!(content, "replacement\n");
    }

    // ─── FP55 — see specs/improve_plan/32-write-file-fp55.md ──────────────

    #[tokio::test]
    async fn fp55_overwrite_rejection_records_snapshot_so_immediate_retry_succeeds() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("doc.md"), "# original\nbody\n").expect("seed");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "fp55-retry-no-read".into();

        // First call: no prior read, default if_exists=overwrite -> reject + snapshot.
        let err = WriteFileTool
            .execute(
                json!({"path": "doc.md", "content": "# replacement\n", "create_dirs": false}),
                &ctx,
            )
            .await
            .expect_err("first call should be rejected");
        assert!(err.to_string().contains("Snapshot recorded"));
        assert!(err.to_string().contains("Current file content (preview)"));
        // File must NOT be modified by the rejection.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("doc.md")).expect("read"),
            "# original\nbody\n"
        );

        // Second call: SAME args, no read_file in between. Must succeed because
        // the snapshot was recorded by the first rejection.
        WriteFileTool
            .execute(
                json!({"path": "doc.md", "content": "# replacement\n", "create_dirs": false}),
                &ctx,
            )
            .await
            .expect("retry should succeed without an extra read_file");

        assert_eq!(
            std::fs::read_to_string(dir.path().join("doc.md")).expect("read final"),
            "# replacement\n"
        );
    }

    #[tokio::test]
    async fn fp55_abort_returns_cheap_error_no_preview_no_snapshot() {
        let dir = TempDir::new().expect("tmpdir");
        let original = "x".repeat(2_000); // larger than the 600-char preview window
        std::fs::write(dir.path().join("taken.txt"), &original).expect("seed");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "fp55-abort-cheap".into();

        let err = WriteFileTool
            .execute(
                json!({
                    "path": "taken.txt",
                    "content": "fresh content",
                    "create_dirs": false,
                    "if_exists": "abort",
                }),
                &ctx,
            )
            .await
            .expect_err("abort mode must reject existing path");

        let msg = err.to_string();
        assert!(msg.contains("already exists"), "got: {msg}");
        assert!(msg.contains("2000 bytes"), "must include stat: {msg}");
        // No content preview in cheap-abort path.
        assert!(
            !msg.contains("Current file content"),
            "abort path must NOT include preview; got: {msg}"
        );
        assert!(
            !msg.contains("Snapshot recorded"),
            "abort path must NOT record snapshot; got: {msg}"
        );
        // Snapshot must remain absent so a follow-up overwrite must still
        // pay the read_file cost (or the FP55 overwrite-with-preview cost).
        let resolved = dir.path().join("taken.txt");
        assert!(
            !crate::read_tracker::has_file_snapshot(&ctx.session_id, &resolved),
            "abort mode must not populate snapshot"
        );
    }

    #[tokio::test]
    async fn fp55_abort_then_overwrite_retry_records_snapshot_and_then_succeeds() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("seq.txt"), "old\n").expect("seed");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "fp55-abort-then-overwrite".into();

        // Step 1: abort -> cheap reject, no snapshot.
        WriteFileTool
            .execute(
                json!({
                    "path": "seq.txt",
                    "content": "new\n",
                    "create_dirs": false,
                    "if_exists": "abort",
                }),
                &ctx,
            )
            .await
            .expect_err("abort rejects collision");

        // Step 2: switch intent to overwrite -> reject WITH preview + snapshot.
        WriteFileTool
            .execute(
                json!({
                    "path": "seq.txt",
                    "content": "new\n",
                    "create_dirs": false,
                    "if_exists": "overwrite",
                }),
                &ctx,
            )
            .await
            .expect_err("overwrite first call rejects + snapshots");

        // Step 3: retry overwrite -> succeeds.
        WriteFileTool
            .execute(
                json!({
                    "path": "seq.txt",
                    "content": "new\n",
                    "create_dirs": false,
                    "if_exists": "overwrite",
                }),
                &ctx,
            )
            .await
            .expect("overwrite retry must succeed after snapshot");

        assert_eq!(
            std::fs::read_to_string(dir.path().join("seq.txt")).expect("read"),
            "new\n"
        );
    }

    #[tokio::test]
    async fn fp55_new_file_succeeds_under_either_mode() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        WriteFileTool
            .execute(
                json!({"path": "new1.txt", "content": "a", "if_exists": "abort"}),
                &ctx,
            )
            .await
            .expect("abort + new file must succeed");

        WriteFileTool
            .execute(
                json!({"path": "new2.txt", "content": "b", "if_exists": "overwrite"}),
                &ctx,
            )
            .await
            .expect("overwrite + new file must succeed");
    }

    #[tokio::test]
    async fn fp55_external_modification_between_reject_and_retry_fails_freshness() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("race.txt"), "v1\n").expect("seed");

        let mut ctx = ctx_in(dir.path());
        ctx.session_id = "fp55-race".into();

        // First call: reject + snapshot v1.
        WriteFileTool
            .execute(
                json!({"path": "race.txt", "content": "v2\n", "create_dirs": false}),
                &ctx,
            )
            .await
            .expect_err("first call rejects");

        // External modification between reject and retry. Sleep to ensure
        // mtime differs even on coarse-resolution filesystems.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.path().join("race.txt"), "external\n").expect("modify");

        let err = WriteFileTool
            .execute(
                json!({"path": "race.txt", "content": "v2\n", "create_dirs": false}),
                &ctx,
            )
            .await
            .expect_err("retry must fail because file changed externally");
        assert!(err.to_string().contains("modified since you last read it"));
    }

    #[test]
    fn fp55_schema_includes_if_exists_optional_enum() {
        let schema = WriteFileTool.schema();
        assert_eq!(schema.strict, Some(true));
        let if_exists = &schema.parameters["properties"]["if_exists"];
        assert_eq!(if_exists["type"], "string");
        assert_eq!(if_exists["enum"], json!(["overwrite", "abort"]));
        // if_exists must NOT be in the required list — keeps backward compat.
        let required = schema.parameters["required"].as_array().expect("required arr");
        assert!(
            !required.iter().any(|v| v == "if_exists"),
            "if_exists must remain optional to avoid breaking existing callers"
        );
    }

    #[test]
    fn fp55_schema_description_documents_retry_protocol() {
        let desc = WriteFileTool.schema().description;
        assert!(
            desc.contains("if_exists=\"abort\""),
            "description must document abort mode"
        );
        assert!(
            desc.contains("if_exists=\"overwrite\""),
            "description must document overwrite mode"
        );
        assert!(
            desc.to_lowercase().contains("retry")
                || desc.to_lowercase().contains("again")
                || desc.to_lowercase().contains("call write_file"),
            "description must document the retry protocol"
        );
        assert!(
            desc.contains("patch"),
            "description must still steer toward patch for partial edits"
        );
    }

    #[tokio::test]
    async fn fp55_unknown_if_exists_value_is_rejected() {
        let dir = TempDir::new().expect("tmpdir");
        let ctx = ctx_in(dir.path());

        let err = WriteFileTool
            .execute(
                json!({
                    "path": "x.txt",
                    "content": "v",
                    "create_dirs": false,
                    "if_exists": "skip",   // not in enum
                }),
                &ctx,
            )
            .await
            .expect_err("unknown enum variant must be rejected");
        assert!(matches!(err, ToolError::InvalidArgs { .. }));
    }
}
