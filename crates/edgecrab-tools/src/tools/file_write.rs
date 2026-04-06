//! # write_file — Create or overwrite files
//!
//! WHY full-file write: Some operations (generating new files, replacing
//! content entirely) are more natural as full writes. For partial edits,
//! the `patch` tool is preferred.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

use crate::path_utils::{jail_write_path, jail_write_path_create_dirs};
use crate::registry::{ToolContext, ToolHandler};
use crate::tools::checkpoint::ensure_checkpoint;

pub struct WriteFileTool;

#[derive(Deserialize)]
struct Args {
    path: String,
    content: String,
    #[serde(default)]
    create_dirs: bool,
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

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write_file".into(),
            description: "Write content to a file. Creates the file if it doesn't exist.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to working directory"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    },
                    "create_dirs": {
                        "type": "boolean",
                        "description": "Create parent directories if they don't exist (default: false)"
                    }
                },
                "required": ["path", "content"]
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

        let bytes_written = args.content.len();

        tokio::fs::write(&resolved, &args.content)
            .await
            .map_err(|e| ToolError::Other(format!("Cannot write '{}': {}", args.path, e)))?;

        Ok(format!("Wrote {} bytes to '{}'", bytes_written, args.path))
    }
}

inventory::submit!(&WriteFileTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
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

        let ctx = ctx_in(dir.path());
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
}
