//! Deferred tool schema index (spec 007 L3 / tool-search parity).
//!
//! In `indexed` mode only a **hot** subset is sent on the wire; the rest appear
//! as a compact category summary in the system prompt (no per-tool name dump).
//! `tool_search` materializes full schemas on demand and adds them to the
//! session wire set.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, RwLock};

use edgecrab_types::ToolSchema;

use crate::schema_mode::compact_tool_schema;
use crate::toolsets::INDEXED_HOT_TOOLS;

pub const TOOL_SEARCH_NAME: &str = "tool_search";

/// Default cap on non-hot tools materialized onto the wire (`0` = unlimited).
pub const DEFAULT_MAX_MATERIALIZED_TOOLS: usize = 12;

/// Max deferred tools loaded via `tool_search(toolset=…)`.
pub const MAX_TOOLSET_MATERIALIZE: usize = 8;

/// Auto mode: Compact when enabled count ≤ this; Indexed when above.
pub const AUTO_INDEXED_TOOL_COUNT_THRESHOLD: usize = 14;

/// Turn-start BM25 prefetch cap (silent materialize before first LLM call).
pub const DEFAULT_PREFETCH_LIMIT: usize = 3;

/// How schemas are emitted in materialize outcomes / tool_search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializeSchemaStyle {
    Compact,
    Full,
}

/// Outcome of [`materialize_tool_names`].
#[derive(Debug, Clone, Default)]
pub struct MaterializeOutcome {
    pub activated: Vec<String>,
    pub already_wire: Vec<String>,
    pub not_found: Vec<String>,
    pub evicted: Vec<String>,
    pub schemas: Vec<ToolSchema>,
    /// Curated argument examples for activated tools (materialize enrichment).
    pub input_examples: std::collections::HashMap<String, Vec<serde_json::Value>>,
}

/// Session-scoped materialized wire set with optional LRU eviction.
#[derive(Debug, Clone, Default)]
pub struct MaterializedToolSet {
    set: HashSet<String>,
    /// LRU order: front = coldest, back = hottest.
    order: VecDeque<String>,
}

impl MaterializedToolSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.set.contains(name)
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn names(&self) -> HashSet<String> {
        self.set.clone()
    }

    /// Insert `name` (non-hot). When `max > 0` and over capacity, evict coldest.
    /// Returns names that were evicted.
    pub fn insert(&mut self, name: impl Into<String>, max: usize) -> Vec<String> {
        let name = name.into();
        if is_hot_tool(&name) {
            return Vec::new();
        }
        if self.set.contains(&name) {
            self.order.retain(|n| n != &name);
            self.order.push_back(name);
            return Vec::new();
        }
        self.set.insert(name.clone());
        self.order.push_back(name);
        let mut evicted = Vec::new();
        if max > 0 {
            while self.set.len() > max {
                let Some(cold) = self.order.pop_front() else {
                    break;
                };
                self.set.remove(&cold);
                evicted.push(cold);
            }
        }
        evicted
    }

    /// Mark a materialized tool as most-recently used (on successful dispatch).
    pub fn touch(&mut self, name: &str) {
        if !self.set.contains(name) {
            return;
        }
        self.order.retain(|n| n != name);
        self.order.push_back(name.to_string());
    }
}

/// Whether `name` is on the LLM wire in indexed mode.
pub fn is_on_wire(name: &str, materialized: &HashSet<String>) -> bool {
    is_hot_tool(name) || materialized.contains(name)
}

/// True when indexed mode should block dispatch until `tool_search` materializes the tool.
pub fn is_deferred_not_on_wire(name: &str, materialized: &HashSet<String>) -> bool {
    !is_on_wire(name, materialized)
}

/// Tool-result text when a deferred tool is invoked before materialization.
///
/// Must be a typed `tool_error` payload (`tool_is_error:true`) so storm/failure
/// counters and recovery materialization see honesty (006 pptx forensics).
pub fn deferred_tool_error_response(tool_name: &str) -> String {
    use edgecrab_types::{RecoveryAction, RecoveryFeedbackBuilder, ToolError};
    ToolError::Unavailable {
        tool: tool_name.into(),
        reason: format!(
            "Tool `{tool_name}` is enabled but not on your wire schema yet. \
             Call `tool_search` first, then retry."
        ),
    }
    .with_recovery(
        RecoveryFeedbackBuilder::new("deferred_not_on_wire")
            .message("Materialize deferred tool before calling it")
            .suggestion(
                RecoveryAction::CallToolFirst,
                serde_json::json!({
                    "tool": TOOL_SEARCH_NAME,
                    "tool_names": [tool_name],
                    "then": { "tool": tool_name },
                }),
            )
            .build(),
    )
    .to_llm_response()
}

/// Read the session materialized set (empty when unset or lock poisoned).
pub fn read_materialized_set(
    materialized: Option<&Arc<RwLock<MaterializedToolSet>>>,
) -> HashSet<String> {
    materialized
        .and_then(|set| set.read().ok())
        .map(|guard| guard.names())
        .unwrap_or_default()
}

/// Whether `name` is always on the wire in indexed mode (hot tier).
pub fn is_hot_tool(name: &str) -> bool {
    name == TOOL_SEARCH_NAME || INDEXED_HOT_TOOLS.contains(&name)
}

/// Count schemas on the wire vs deferred (indexed mode observability).
pub fn wire_partition_counts(
    schemas: &[ToolSchema],
    materialized: &HashSet<String>,
) -> (usize, usize) {
    let (wire, deferred) = partition_schemas(schemas, materialized);
    (wire.len(), deferred.len())
}

/// Split enabled schemas into wire (hot + materialized + meta) vs deferred index.
pub fn partition_schemas<'a>(
    schemas: &'a [ToolSchema],
    materialized: &HashSet<String>,
) -> (Vec<&'a ToolSchema>, Vec<&'a ToolSchema>) {
    let mut wire = Vec::new();
    let mut deferred = Vec::new();
    for schema in schemas {
        if is_hot_tool(&schema.name) || materialized.contains(&schema.name) {
            wire.push(schema);
        } else {
            deferred.push(schema);
        }
    }
    (wire, deferred)
}

/// Schemas to send on the LLM wire in indexed mode (hot + materialized).
///
/// When `hot_schema_full` is true (local providers), hot/`tool_search` keep full
/// fidelity; materialized long-tail tools stay compact.
pub fn wire_schemas(
    schemas: &[ToolSchema],
    materialized: &HashSet<String>,
    hot_schema_full: bool,
) -> Vec<ToolSchema> {
    let (wire, _) = partition_schemas(schemas, materialized);
    wire.iter()
        .map(|s| {
            if hot_schema_full && is_hot_tool(&s.name) {
                (*s).clone()
            } else {
                compact_tool_schema(s)
            }
        })
        .collect()
}

/// System-prompt block: deferred count + toolset categories (no per-tool names).
///
/// Progressive disclosure (July 2026): invent risk rises when every deferred
/// name is listed. Categories orient search; `tool_search` is the dictionary.
pub fn format_deferred_index(deferred_count: usize, categories: &[String]) -> String {
    if deferred_count == 0 {
        return String::new();
    }
    let mut cats: Vec<&str> = categories.iter().map(String::as_str).collect();
    cats.sort_unstable();
    cats.dedup();
    let categories_line = if cats.is_empty() {
        String::new()
    } else {
        format!("\n\nCategories: {}", cats.join(", "))
    };
    format!(
        "## Deferred tools\n\n\
         {deferred_count} tools are enabled but not on the wire.\n\
         Call `tool_search` with `query` (preferred), `toolset` (pack), or exact `tool_names`, then retry.\
         {categories_line}"
    )
}

/// Materialize deferred tool names onto the session wire set (DRY for tool_search + prefetch).
pub fn materialize_tool_names(
    names: &[String],
    all_schemas: &[ToolSchema],
    materialized: &Arc<RwLock<MaterializedToolSet>>,
    max: usize,
    schema_style: MaterializeSchemaStyle,
) -> MaterializeOutcome {
    let known: HashSet<&str> = all_schemas.iter().map(|s| s.name.as_str()).collect();
    let mut outcome = MaterializeOutcome::default();

    for name in names {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !known.contains(trimmed) {
            outcome.not_found.push(trimmed.to_string());
            continue;
        }
        if is_hot_tool(trimmed) {
            outcome.already_wire.push(trimmed.to_string());
            continue;
        }
        if let Ok(mut guard) = materialized.write() {
            outcome
                .evicted
                .extend(guard.insert(trimmed.to_string(), max));
        }
        if let Some(schema) = all_schemas.iter().find(|s| s.name == trimmed) {
            // Multi-action tools need property prose after materialize (game001 skill_manage thrash).
            let effective_style = if trimmed == "skill_manage" {
                MaterializeSchemaStyle::Full
            } else {
                schema_style
            };
            outcome.schemas.push(match effective_style {
                MaterializeSchemaStyle::Compact => compact_tool_schema(schema),
                MaterializeSchemaStyle::Full => schema.clone(),
            });
        }
        outcome.activated.push(trimmed.to_string());
        let examples = crate::tool_input_examples::input_examples_for_tool(trimmed);
        if !examples.is_empty() {
            outcome.input_examples.insert(trimmed.to_string(), examples);
        }
    }
    outcome
}

/// Names from a toolset pack (deferred only, sorted, capped).
pub fn deferred_names_for_toolset(
    _toolset: &str,
    toolset_members: &[&str],
    known: &HashSet<&str>,
    max: usize,
) -> Vec<String> {
    let mut names: Vec<String> = toolset_members
        .iter()
        .map(|n| (*n).to_string())
        .filter(|n| known.contains(n.as_str()) && !is_hot_tool(n))
        .collect();
    names.sort();
    names.truncate(max);
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema(name: &str) -> ToolSchema {
        ToolSchema {
            name: name.into(),
            description: format!("Does {name} things."),
            parameters: json!({
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "help" }
                }
            }),
            strict: None,
        }
    }

    #[test]
    fn hot_tools_stay_on_wire() {
        let schemas = vec![
            schema("read_file"),
            schema("browser_navigate"),
            schema("tool_search"),
        ];
        let materialized = HashSet::new();
        let (wire, deferred) = partition_schemas(&schemas, &materialized);
        let wire_names: Vec<_> = wire.iter().map(|s| s.name.as_str()).collect();
        assert!(wire_names.contains(&"read_file"));
        assert!(wire_names.contains(&"tool_search"));
        assert_eq!(deferred.len(), 1);
        assert_eq!(deferred[0].name, "browser_navigate");
    }

    #[test]
    fn materialized_promotes_to_wire() {
        let schemas = vec![schema("browser_navigate")];
        let mut materialized = HashSet::new();
        materialized.insert("browser_navigate".into());
        let (wire, deferred) = partition_schemas(&schemas, &materialized);
        assert_eq!(wire.len(), 1);
        assert!(deferred.is_empty());
    }

    #[test]
    fn deferred_not_on_wire_until_materialized() {
        assert!(is_deferred_not_on_wire("browser_navigate", &HashSet::new()));
        let mut materialized = HashSet::new();
        materialized.insert("browser_navigate".into());
        assert!(!is_deferred_not_on_wire("browser_navigate", &materialized));
    }

    #[test]
    fn deferred_tool_error_is_typed_tool_error() {
        let body = deferred_tool_error_response("vision_analyze");
        let parsed = edgecrab_types::parse_tool_error_payload(&body)
            .expect("deferred soft-fail must be tool_error JSON");
        assert_eq!(parsed.response_type, "tool_error");
        assert!(
            parsed.error.contains("vision_analyze")
                || parsed.tool.as_deref() == Some("vision_analyze")
        );
        assert!(
            parsed
                .recovery_feedback
                .as_ref()
                .is_some_and(|r| !r.suggestions.is_empty()),
            "must include CallToolFirst(tool_search) recovery"
        );
    }

    #[test]
    fn format_index_has_count_categories_not_tool_names() {
        let text = format_deferred_index(3, &["browser".into(), "memory".into()]);
        assert!(text.contains("3 tools are enabled"));
        assert!(text.contains("tool_search"));
        assert!(text.contains("Categories: browser, memory"));
        assert!(
            !text.contains("browser_click"),
            "must not dump individual deferred tool names"
        );
    }

    #[test]
    fn format_index_empty_when_zero_deferred() {
        assert!(format_deferred_index(0, &["browser".into()]).is_empty());
    }

    #[test]
    fn materialized_lru_evicts_coldest() {
        let mut set = MaterializedToolSet::new();
        assert!(set.insert("browser_navigate", 2).is_empty());
        assert!(set.insert("browser_click", 2).is_empty());
        let evicted = set.insert("web_crawl", 2);
        assert_eq!(evicted, vec!["browser_navigate".to_string()]);
        assert!(!set.contains("browser_navigate"));
        assert!(set.contains("browser_click"));
        assert!(set.contains("web_crawl"));
    }

    #[test]
    fn materialized_lru_touch_keeps_hot_tools() {
        let mut set = MaterializedToolSet::new();
        set.insert("browser_navigate", 2);
        set.insert("browser_click", 2);
        set.touch("browser_navigate");
        let evicted = set.insert("web_crawl", 2);
        assert_eq!(evicted, vec!["browser_click".to_string()]);
        assert!(set.contains("browser_navigate"));
    }

    #[test]
    fn materialize_tool_names_activates_deferred() {
        let schemas = vec![schema("browser_navigate"), schema("read_file")];
        let set = Arc::new(RwLock::new(MaterializedToolSet::new()));
        let out = materialize_tool_names(
            &[
                "browser_navigate".into(),
                "read_file".into(),
                "missing".into(),
            ],
            &schemas,
            &set,
            12,
            MaterializeSchemaStyle::Compact,
        );
        assert_eq!(out.activated, vec!["browser_navigate".to_string()]);
        assert_eq!(out.already_wire, vec!["read_file".to_string()]);
        assert_eq!(out.not_found, vec!["missing".to_string()]);
        assert!(set.read().unwrap().contains("browser_navigate"));
        assert!(
            out.schemas[0].parameters["properties"]["q"]
                .get("description")
                .is_none()
        );
        assert!(
            out.input_examples.contains_key("browser_navigate"),
            "materialize must attach curated input_examples"
        );
    }

    #[test]
    fn materialize_write_file_is_hot_already_on_wire() {
        let schemas = vec![schema("write_file")];
        let set = Arc::new(RwLock::new(MaterializedToolSet::new()));
        let out = materialize_tool_names(
            &["write_file".into()],
            &schemas,
            &set,
            12,
            MaterializeSchemaStyle::Compact,
        );
        assert!(out.already_wire.iter().any(|n| n == "write_file"));
        assert!(out.activated.is_empty());
        // Curated examples still available for tool_search / docs.
        let ex = crate::tool_input_examples::input_examples_for_tool("write_file");
        assert!(ex[0].get("path").is_some());
        assert!(ex[0].get("content").is_some());
    }

    #[test]
    fn materialize_skill_manage_keeps_full_schema() {
        let schemas = vec![ToolSchema {
            name: "skill_manage".into(),
            description: "Manage skills.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "op" },
                    "content": { "type": "string", "description": "body" }
                },
                "required": ["action", "name"]
            }),
            strict: None,
        }];
        let set = Arc::new(RwLock::new(MaterializedToolSet::new()));
        let out = materialize_tool_names(
            &["skill_manage".into()],
            &schemas,
            &set,
            12,
            MaterializeSchemaStyle::Compact,
        );
        assert_eq!(out.activated, vec!["skill_manage".to_string()]);
        assert!(
            out.schemas[0].parameters["properties"]["action"]
                .get("description")
                .is_some(),
            "skill_manage must materialize Full (keep property prose)"
        );
    }

    #[test]
    fn wire_schemas_local_hot_keeps_property_descriptions() {
        let schemas = vec![schema("read_file"), schema("browser_navigate")];
        let mut mat = HashSet::new();
        mat.insert("browser_navigate".into());
        let wire = wire_schemas(&schemas, &mat, true);
        let hot = wire.iter().find(|s| s.name == "read_file").unwrap();
        let deferred = wire.iter().find(|s| s.name == "browser_navigate").unwrap();
        assert!(
            hot.parameters["properties"]["q"]
                .get("description")
                .is_some()
        );
        assert!(
            deferred.parameters["properties"]["q"]
                .get("description")
                .is_none()
        );
    }

    #[test]
    fn deferred_names_for_toolset_caps_and_skips_hot() {
        let known: HashSet<&str> = ["read_file", "write_file", "patch", "zzz"]
            .into_iter()
            .collect();
        let members = ["zzz", "write_file", "read_file", "patch"];
        let names = deferred_names_for_toolset("file", &members, &known, 8);
        // write_file / read_file / patch are hot — only non-hot members remain.
        assert_eq!(names, vec!["zzz".to_string()]);
    }
}
