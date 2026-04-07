use std::time::Instant;

use dashmap::DashMap;
use lsp_types::Uri;
use lsp_types::{Diagnostic, DiagnosticSeverity};

#[derive(Debug, Clone)]
pub struct CachedDiagnostics {
    pub diagnostics: Vec<Diagnostic>,
    pub received_at: Instant,
    pub server_id: String,
}

#[derive(Debug, Default)]
pub struct DiagnosticCache {
    cache: DashMap<Uri, CachedDiagnostics>,
}

impl DiagnosticCache {
    pub fn update(&self, uri: Uri, diagnostics: Vec<Diagnostic>, server_id: impl Into<String>) {
        self.cache.insert(
            uri,
            CachedDiagnostics {
                diagnostics,
                received_at: Instant::now(),
                server_id: server_id.into(),
            },
        );
    }

    pub fn get(&self, uri: &Uri) -> Option<Vec<Diagnostic>> {
        self.cache.get(uri).map(|entry| entry.diagnostics.clone())
    }

    pub fn clear_file(&self, uri: &Uri) {
        self.cache.remove(uri);
    }

    pub fn all_errors(&self) -> Vec<(Uri, Diagnostic)> {
        let mut items = Vec::new();
        for entry in &self.cache {
            for diagnostic in &entry.diagnostics {
                if diagnostic.severity == Some(DiagnosticSeverity::ERROR) {
                    items.push((entry.key().clone(), diagnostic.clone()));
                }
            }
        }
        items.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
        items
    }
}
