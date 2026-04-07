use edgecrab_tools::path_utils::{jail_read_path, jail_write_path};
use edgecrab_tools::registry::ToolContext;
use edgecrab_tools::tools::checkpoint::ensure_checkpoint;
use lsp_types::{DocumentChangeOperation, DocumentChanges, OneOf, TextEdit, Uri, WorkspaceEdit};
use serde_json::json;

use crate::error::{LspError, uri_to_path};
use crate::position::PositionEncoder;

pub fn apply_text_edits(original: &str, edits: &[TextEdit]) -> Result<String, LspError> {
    let mut ordered = edits.to_vec();
    ordered.sort_by(|a, b| {
        b.range
            .start
            .line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
            .then(b.range.end.line.cmp(&a.range.end.line))
            .then(b.range.end.character.cmp(&a.range.end.character))
    });

    let mut text = original.to_string();
    for edit in ordered {
        let range = PositionEncoder::to_byte_range(&text, edit.range)
            .ok_or_else(|| LspError::Protocol("invalid text edit range".into()))?;
        text.replace_range(range, &edit.new_text);
    }
    Ok(text)
}

fn apply_file_edits(
    ctx: &ToolContext,
    file_url: &Uri,
    edits: &[TextEdit],
) -> Result<serde_json::Value, LspError> {
    let path = uri_to_path(file_url)?;
    let rel = path.strip_prefix(&ctx.cwd).unwrap_or(&path);
    let rel_str = rel.to_string_lossy().to_string();
    let policy = ctx.config.file_path_policy(&ctx.cwd);
    let read_path = jail_read_path(&rel_str, &policy)
        .or_else(|_| jail_read_path(&path.to_string_lossy(), &policy))
        .map_err(|err| LspError::Other(err.to_string()))?;
    let write_path = jail_write_path(&read_path.to_string_lossy(), &policy)
        .map_err(|err| LspError::Other(err.to_string()))?;
    let original = std::fs::read_to_string(&read_path)?;
    let updated = apply_text_edits(&original, edits)?;
    std::fs::write(&write_path, &updated)?;
    Ok(json!({
        "file": write_path.display().to_string(),
        "changed": original != updated,
        "diff": similar::TextDiff::from_lines(&original, &updated).unified_diff().context_radius(2).to_string(),
    }))
}

pub fn apply_workspace_edit(
    ctx: &ToolContext,
    edit: &WorkspaceEdit,
) -> Result<Vec<serde_json::Value>, LspError> {
    ensure_checkpoint(ctx, "before lsp workspace edit");

    let mut changed = Vec::new();

    if let Some(changes) = &edit.changes {
        let mut ordered: Vec<_> = changes.iter().collect();
        ordered.sort_by(|(left_uri, _), (right_uri, _)| left_uri.as_str().cmp(right_uri.as_str()));
        for (url, edits) in ordered {
            changed.push(apply_file_edits(ctx, url, edits)?);
        }
    }

    if let Some(document_changes) = &edit.document_changes {
        match document_changes {
            DocumentChanges::Edits(edits) => {
                for change in edits {
                    changed.push(apply_file_edits(
                        ctx,
                        &change.text_document.uri,
                        &change
                            .edits
                            .iter()
                            .filter_map(|edit| match edit {
                                OneOf::Left(text_edit) => Some(text_edit.clone()),
                                OneOf::Right(_) => None,
                            })
                            .collect::<Vec<_>>(),
                    )?);
                }
            }
            DocumentChanges::Operations(ops) => {
                for op in ops {
                    if let DocumentChangeOperation::Edit(edit) = op {
                        changed.push(apply_file_edits(
                            ctx,
                            &edit.text_document.uri,
                            &edit
                                .edits
                                .iter()
                                .filter_map(|text_edit| match text_edit {
                                    OneOf::Left(text_edit) => Some(text_edit.clone()),
                                    OneOf::Right(_) => None,
                                })
                                .collect::<Vec<_>>(),
                        )?);
                    }
                }
            }
        }
    }

    Ok(changed)
}
