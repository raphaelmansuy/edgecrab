//! Deferred tool schema index (spec 007 L3 / tool-search parity).
//!
//! In `indexed` mode only a **hot** subset is sent on the wire; the rest appear
//! as a compact name index in the system prompt. `tool_search` materializes full
//! schemas on demand and adds them to the session wire set.

use std::collections::HashSet;

use edgecrab_types::ToolSchema;

use crate::schema_mode::compact_tool_description;
use crate::toolsets::INDEXED_HOT_TOOLS;

pub const TOOL_SEARCH_NAME: &str = "tool_search";

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
         Call `tool_search` with tool_names: [\"{tool_name}\"] first, then retry."
    )
}

/// Read the session materialized set (empty when unset or lock poisoned).
pub fn read_materialized_set(
    materialized: Option<&std::sync::Arc<std::sync::RwLock<HashSet<String>>>>,
) -> HashSet<String> {
    materialized
        .and_then(|set| set.read().ok())
        .map(|guard| guard.clone())
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
}
