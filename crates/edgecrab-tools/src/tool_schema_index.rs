//! Deferred tool schema index (spec 007 L3 / tool-search parity).
//!
//! In `indexed` mode only a **hot** subset is sent on the wire; the rest appear
//! as a compact name index in the system prompt. `tool_search` materializes full
//! schemas on demand and adds them to the session wire set.

use std::collections::{HashSet, VecDeque};

use edgecrab_types::ToolSchema;

use crate::schema_mode::compact_tool_description;
use crate::toolsets::INDEXED_HOT_TOOLS;

pub const TOOL_SEARCH_NAME: &str = "tool_search";

/// Default cap on non-hot tools materialized onto the wire (`0` = unlimited).
pub const DEFAULT_MAX_MATERIALIZED_TOOLS: usize = 12;

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
pub fn deferred_tool_error_response(tool_name: &str) -> String {
    format!(
        "Tool `{tool_name}` is enabled but not on your wire schema yet. \
         Call `tool_search` with tool_names: [\"{tool_name}\"] or query: \"<what you need>\" first, then retry."
    )
}

/// Read the session materialized set (empty when unset or lock poisoned).
pub fn read_materialized_set(
    materialized: Option<&std::sync::Arc<std::sync::RwLock<MaterializedToolSet>>>,
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

/// Schemas to send on the LLM wire in indexed mode (hot + materialized, compact).
pub fn wire_schemas(schemas: &[ToolSchema], materialized: &HashSet<String>) -> Vec<ToolSchema> {
    let (wire, _) = partition_schemas(schemas, materialized);
    wire.iter()
        .map(|s| crate::schema_mode::compact_tool_schema(s))
        .collect()
}

/// System-prompt block listing deferred tools (names + one-line hints).
pub fn format_deferred_index(deferred: &[&ToolSchema]) -> String {
    if deferred.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = deferred
        .iter()
        .map(|s| {
            let desc = compact_tool_description(&s.description);
            format!("- **{}**: {}", s.name, desc)
        })
        .collect();
    format!(
        "## Deferred tools\n\n\
         {count} tools are enabled but not on the wire yet. \
         Call `tool_search` with `tool_names` before invoking any of these:\n\n\
         {body}",
        count = deferred.len(),
        body = lines.join("\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema(name: &str) -> ToolSchema {
        ToolSchema {
            name: name.into(),
            description: format!("Does {name} things."),
            parameters: json!({"type": "object", "properties": {}}),
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
    fn format_index_lists_deferred_names() {
        let a = schema("browser_click");
        let text = format_deferred_index(&[&a]);
        assert!(text.contains("browser_click"));
        assert!(text.contains("tool_search"));
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
}
