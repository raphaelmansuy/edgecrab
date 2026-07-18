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

fn catalog_entry(schema: &ToolSchema) -> CatalogEntry {
    let text = entry_search_text(schema);
    CatalogEntry {
        name: schema.name.clone(),
        description: schema.description.clone(),
        tokens: tokenize(&text),
    }
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
        .map(|schema| catalog_entry(schema))
        .collect()
}

/// Full-registry catalog for unknown-tool recovery (excludes the discovery meta-tool).
///
/// First principle: invent recovery searches the same truth store as `tool_search`,
/// not a lexicographic slice of names.
pub fn build_registry_catalog(schemas: &[ToolSchema]) -> Vec<CatalogEntry> {
    use crate::tool_schema_index::TOOL_SEARCH_NAME;
    schemas
        .iter()
        .filter(|s| s.name != TOOL_SEARCH_NAME)
        .map(catalog_entry)
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

/// True when user text looks like a workspace create/write task.
///
/// Used to bias prefetch toward `write_file` and away from `skill_manage`
/// (session `8d74ce9c`: create prompt promoted skills while write stayed deferred).
pub fn looks_like_create_file_intent(user_text: &str) -> bool {
    let lower = user_text.to_ascii_lowercase();
    let has_create_verb = lower.contains("write ")
        || lower.contains("write a")
        || lower.contains("create ")
        || lower.contains("scaffold")
        || lower.contains("generate ");
    // Path/extension tokens only — not vibe words like "game" / "file" (018 F4).
    let has_path_or_artifact = lower.contains("./")
        || lower.contains("demo/")
        || lower.contains("src/")
        || lower.contains(".html")
        || lower.contains(".js")
        || lower.contains(".css")
        || lower.contains(".rs")
        || lower.contains(".py")
        || lower.contains(".ts")
        || lower.contains(".tsx")
        || lower.contains(".md");
    has_create_verb && has_path_or_artifact
}

/// Silent turn-start prefetch: BM25 top-N deferred names for the user message.
///
/// Does not mutate the system prompt (cache law). Caller inserts into the
/// materialized set and rebuilds wire defs.
///
/// Create-file intents: prefer `write_file` over `skill_manage` when both
/// remain deferred (hot-set usually already includes `write_file`).
pub fn prefetch_tools_for_user_message(
    user_text: &str,
    schemas: &[ToolSchema],
    materialized: &HashSet<String>,
    max_prefetch: usize,
) -> Vec<String> {
    let query = user_text.trim();
    if query.is_empty() || max_prefetch == 0 {
        return Vec::new();
    }
    let catalog = build_deferred_catalog(schemas, materialized);
    let mut hits = search_deferred_catalog(&catalog, query, max_prefetch);

    if looks_like_create_file_intent(query) {
        let deferred_names: HashSet<&str> = catalog.iter().map(|e| e.name.as_str()).collect();
        // Prefer workspace create over skills package management.
        hits.retain(|n| n != "skill_manage");
        if deferred_names.contains("write_file") && !hits.iter().any(|n| n == "write_file") {
            hits.insert(0, "write_file".into());
            if hits.len() > max_prefetch {
                hits.truncate(max_prefetch);
            }
        }
    }
    hits
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
        let hits = search_deferred_catalog(&catalog, "browser navigate url", 1);
        assert!(hits.first().is_some_and(|n| n.contains("browser")));
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

    #[test]
    fn prefetch_returns_deferred_hits() {
        let schemas = vec![
            schema("browser_navigate", "Navigate headless browser to URL"),
            schema("memory_write", "Write persistent memory entry"),
        ];
        let hits = prefetch_tools_for_user_message(
            "please navigate the browser to a url",
            &schemas,
            &HashSet::new(),
            3,
        );
        assert!(hits.iter().any(|n| n.contains("browser")));
    }

    #[test]
    fn create_intent_excludes_skill_manage_from_prefetch() {
        // write_file is hot (not in deferred catalog). Prefetch must still
        // refuse to promote skill_manage for create-file prompts.
        let schemas = vec![
            schema(
                "skill_manage",
                "Create edit patch delete a skill or write supporting files",
            ),
            schema("web_search", "Search the web for facts"),
            schema("browser_navigate", "Navigate headless browser to URL"),
        ];
        let hits = prefetch_tools_for_user_message(
            "Write a complete html5 and javascript 3D game in ./demo/game001",
            &schemas,
            &HashSet::new(),
            3,
        );
        assert!(
            !hits.iter().any(|n| n == "skill_manage"),
            "create intent must not prefetch skill_manage: {hits:?}"
        );
    }

    #[test]
    fn looks_like_create_file_intent_game001() {
        assert!(looks_like_create_file_intent(
            "Write a complete html5 and javascript 3D game in ./demo/game001"
        ));
        assert!(!looks_like_create_file_intent("what is the weather today"));
    }
}
