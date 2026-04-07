use std::path::Path;
use std::sync::Arc;

use dashmap::DashMap;
use lsp_types::Uri;
use lsp_types::notification::{DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    VersionedTextDocumentIdentifier,
};

use crate::error::{LspError, path_to_uri};
use crate::protocol::ServerConnection;

const DEFAULT_VERSION: i32 = 1;

#[derive(Debug, Clone)]
pub struct OpenDocument {
    pub version: i32,
    pub text: String,
    pub ref_count: usize,
}

#[derive(Debug, Default)]
pub struct DocumentSyncLayer {
    open_docs: DashMap<Uri, OpenDocument>,
    versions: DashMap<Uri, i32>,
}

pub struct DocumentSyncGuard {
    layer: Arc<DocumentSyncLayer>,
    connection: ServerConnection,
    uri: Uri,
}

impl Drop for DocumentSyncGuard {
    fn drop(&mut self) {
        let layer = Arc::clone(&self.layer);
        let connection = self.connection.clone();
        let uri = self.uri.clone();
        tokio::spawn(async move {
            layer.release(connection, uri).await;
        });
    }
}

impl DocumentSyncLayer {
    pub async fn ensure_open(
        self: &Arc<Self>,
        connection: ServerConnection,
        path: &Path,
        language_id: &str,
        file_limit_bytes: u64,
    ) -> Result<DocumentSyncGuard, LspError> {
        let uri = path_to_uri(path)?;
        let text = read_file_for_sync(path, file_limit_bytes)?;

        if let Some(mut entry) = self.open_docs.get_mut(&uri) {
            if entry.text != text {
                let version = self.next_version(&uri);
                connection
                    .notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: uri.clone(),
                            version,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: text.clone(),
                        }],
                    })
                    .await?;
                entry.version = version;
                entry.text = text;
            }
            entry.ref_count += 1;
            return Ok(DocumentSyncGuard {
                layer: Arc::clone(self),
                connection,
                uri,
            });
        }

        let version = self.next_version(&uri);
        connection
            .notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: language_id.to_string(),
                    version,
                    text: text.clone(),
                },
            })
            .await?;

        self.open_docs.insert(
            uri.clone(),
            OpenDocument {
                version,
                text,
                ref_count: 1,
            },
        );

        Ok(DocumentSyncGuard {
            layer: Arc::clone(self),
            connection,
            uri,
        })
    }

    pub async fn refresh_from_disk(
        &self,
        connection: &ServerConnection,
        path: &Path,
        file_limit_bytes: u64,
    ) -> Result<(), LspError> {
        let uri = path_to_uri(path)?;
        let text = read_file_for_sync(path, file_limit_bytes)?;
        let Some(mut entry) = self.open_docs.get_mut(&uri) else {
            return Ok(());
        };
        if entry.text == text {
            return Ok(());
        }
        let version = self.next_version(&uri);
        connection
            .notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier { uri, version },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.clone(),
                }],
            })
            .await?;
        entry.version = version;
        entry.text = text;
        Ok(())
    }

    async fn release(&self, connection: ServerConnection, uri: Uri) {
        let should_close = if let Some(mut entry) = self.open_docs.get_mut(&uri) {
            entry.ref_count = entry.ref_count.saturating_sub(1);
            entry.ref_count == 0
        } else {
            false
        };

        if should_close {
            self.open_docs.remove(&uri);
            let _ = connection
                .notify::<DidCloseTextDocument>(DidCloseTextDocumentParams {
                    text_document: TextDocumentIdentifier { uri },
                })
                .await;
        }
    }

    fn next_version(&self, uri: &Uri) -> i32 {
        if let Some(mut version) = self.versions.get_mut(uri) {
            *version += 1;
            *version
        } else {
            self.versions.insert(uri.clone(), DEFAULT_VERSION);
            DEFAULT_VERSION
        }
    }
}

pub fn read_file_for_sync(path: &Path, file_limit_bytes: u64) -> Result<String, LspError> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > file_limit_bytes {
        return Err(LspError::FileTooLarge {
            path: path.display().to_string(),
            size: metadata.len(),
            limit: file_limit_bytes,
        });
    }
    Ok(std::fs::read_to_string(path)?)
}
