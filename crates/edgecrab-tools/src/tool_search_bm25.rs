//! BM25 catalog search for deferred tools — Hermes `tool_search.search_catalog` parity.

use std::collections::HashSet;

use edgecrab_types::ToolSchema;

use crate::tool_schema_index::{is_hot_tool, partition_schemas};

/// One deferrable tool entry for BM25 retrieval.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub name: String,
    pub description: String,
    pub tokens: Vec<String>,
}

fn tokenize(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

fn entry_search_text(schema: &ToolSchema) -> String {
    let name_words = schema.name.replace(['_', '.', '-'], " ");
    let param_names: String = schema
        .parameters
        .get("properties")
        .and_then(|v| v.as_object())
        .map(|props| props.keys().cloned().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    format!("{name_words} {} {param_names}", schema.description)
}

/// Build searchable catalog from deferred (non-wire) schemas.
pub fn build_deferred_catalog(
    schemas: &[ToolSchema],
    materialized: &HashSet<String>,
) -> Vec<CatalogEntry> {
    let (_, deferred) = partition_schemas(schemas, materialized);
    deferred
        .iter()
        .filter(|s| !is_hot_tool(&s.name))
        .map(|schema| {
            let text = entry_search_text(schema);
            CatalogEntry {
                name: schema.name.clone(),
                description: schema.description.clone(),
                tokens: tokenize(&text),
            }
        })
        .collect()
}

fn bm25_score(
    query_tokens: &[String],
    doc_tokens: &[String],
    avg_dl: f64,
    doc_freq: &std::collections::HashMap<String, usize>,
    n_docs: usize,
) -> f64 {
    if doc_tokens.is_empty() {
        return 0.0;
    }
    const K1: f64 = 1.5;
    const B: f64 = 0.75;
    let dl = doc_tokens.len() as f64;
    let mut doc_tf: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for t in doc_tokens {
        *doc_tf.entry(t.as_str()).or_insert(0) += 1;
    }
    let mut score = 0.0;
    for q in query_tokens {
        let df = doc_freq.get(q).copied().unwrap_or(0);
        if df == 0 {
            continue;
        }
        let idf = ((n_docs as f64 - df as f64 + 0.5) / (df as f64 + 0.5) + 1.0).ln();
        let tf = doc_tf.get(q.as_str()).copied().unwrap_or(0) as f64;
        if tf == 0.0 {
            continue;
        }
        let norm = tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * dl / avg_dl.max(1.0)));
        score += idf * norm;
    }
    score
}

/// Return top `limit` deferred tool names for `query` by BM25 (+ name substring fallback).
pub fn search_deferred_catalog(catalog: &[CatalogEntry], query: &str, limit: usize) -> Vec<String> {
    if catalog.is_empty() || limit == 0 {
        return Vec::new();
    }
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let doc_lengths: Vec<usize> = catalog.iter().map(|e| e.tokens.len()).collect();
    let avg_dl = doc_lengths.iter().sum::<usize>() as f64 / doc_lengths.len().max(1) as f64;
    let mut doc_freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for entry in catalog {
        let seen: HashSet<&str> = entry.tokens.iter().map(String::as_str).collect();
        for t in seen {
            *doc_freq.entry(t.to_string()).or_insert(0) += 1;
        }
    }
    let n_docs = catalog.len();

    let mut scored: Vec<(f64, &CatalogEntry)> = catalog
        .iter()
        .filter_map(|entry| {
            let s = bm25_score(&query_tokens, &entry.tokens, avg_dl, &doc_freq, n_docs);
            if s > 0.0 { Some((s, entry)) } else { None }
        })
        .collect();

    if scored.is_empty() {
        let ql = query.to_ascii_lowercase();
        for entry in catalog {
            if entry.name.to_ascii_lowercase().contains(&ql) {
                scored.push((0.1, entry));
            }
        }
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, e)| e.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema(name: &str, desc: &str) -> ToolSchema {
        ToolSchema {
            name: name.into(),
            description: desc.into(),
            parameters: json!({
                "type": "object",
                "properties": { "url": { "type": "string" } }
            }),
            strict: None,
        }
    }

    #[test]
    fn bm25_finds_browser_tools_by_query() {
        let schemas = vec![
            schema("browser_navigate", "Navigate headless browser to URL"),
            schema("browser_snapshot", "Capture accessibility snapshot of page"),
            schema("memory_write", "Write persistent memory entry"),
        ];
        let materialized = HashSet::new();
        let catalog = build_deferred_catalog(&schemas, &materialized);
        let hits = search_deferred_catalog(&catalog, "browser navigate url", 3);
        assert!(hits.first().is_some_and(|n| n.contains("browser")));
        assert!(!hits.contains(&"memory_write".to_string()));
    }

    #[test]
    fn substring_fallback_matches_partial_name() {
        let schemas = vec![schema(
            "ha_get_states",
            "Fetch Home Assistant entity states",
        )];
        let catalog = build_deferred_catalog(&schemas, &HashSet::new());
        let hits = search_deferred_catalog(&catalog, "ha_get", 5);
        assert!(hits.contains(&"ha_get_states".to_string()));
    }
}
