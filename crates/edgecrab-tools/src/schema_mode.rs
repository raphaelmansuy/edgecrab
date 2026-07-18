//! Tool schema compaction for turn-1 token budget (spec 007 L1.2).
//!
//! `Full` — wire tool schemas verbatim.
//! `Compact` — truncate descriptions and strip per-property prose from JSON Schema.
//! `Indexed` — hot wire + deferred category summary + `tool_search`.
//! `Auto` — Compact when enabled tool count ≤ threshold; else Indexed.

use edgecrab_types::ToolSchema;
use serde_json::{Map, Value};

use crate::tool_schema_index::AUTO_INDEXED_TOOL_COUNT_THRESHOLD;

/// How tool JSON schemas are sent to the LLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSchemaMode {
    Full,
    Compact,
    /// Hot tools on wire + deferred category summary + `tool_search` materialization.
    /// Rust `Default` for effective-mode fallbacks (product YAML default is `auto`).
    #[default]
    Indexed,
    /// Resolve to Compact or Indexed from enabled tool count.
    Auto,
}

impl ToolSchemaMode {
    /// Parse config string. Explicit modes; empty or unknown → [`Self::Auto`].
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "compact" => Self::Compact,
            "full" => Self::Full,
            "indexed" => Self::Indexed,
            "auto" => Self::Auto,
            _ => Self::Auto,
        }
    }

    /// Whether this mode (after resolve) uses progressive disclosure.
    pub fn is_indexed_effective(self) -> bool {
        matches!(self, Self::Indexed)
    }
}

/// Resolve `Auto` from enabled tool count; other modes pass through.
///
/// Never returns `Auto`. Threshold: [`AUTO_INDEXED_TOOL_COUNT_THRESHOLD`].
pub fn resolve_effective_schema_mode(mode: ToolSchemaMode, enabled_tool_count: usize) -> ToolSchemaMode {
    match mode {
        ToolSchemaMode::Auto => {
            if enabled_tool_count <= AUTO_INDEXED_TOOL_COUNT_THRESHOLD {
                ToolSchemaMode::Compact
            } else {
                ToolSchemaMode::Indexed
            }
        }
        other => other,
    }
}

/// First-principles compaction: keep names + types + required; drop prose bloat.
pub fn compact_tool_description(description: &str) -> String {
    let trimmed = description.trim();
    if let Some((idx, _)) = trimmed
        .char_indices()
        .find(|(_, c)| *c == '.' || *c == '\n')
        && idx >= 15
    {
        return trimmed[..idx].trim().to_string();
    }
    if trimmed.len() <= 160 {
        return trimmed.to_string();
    }
    crate::safe_truncate(trimmed, 160).to_string()
}

fn compact_json_schema(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, val) in map {
                if matches!(
                    key.as_str(),
                    "description" | "examples" | "example" | "title" | "default"
                ) {
                    continue;
                }
                if key == "properties"
                    && let Some(props) = val.as_object()
                {
                    let mut slim = Map::new();
                    for (prop, schema) in props {
                        slim.insert(prop.clone(), compact_property_schema(schema));
                    }
                    out.insert(key.clone(), Value::Object(slim));
                    continue;
                }
                out.insert(key.clone(), compact_json_schema(val));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(compact_json_schema).collect()),
        other => other.clone(),
    }
}

fn compact_property_schema(schema: &Value) -> Value {
    let mut out = compact_json_schema(schema);
    if let Some(obj) = out.as_object_mut() {
        obj.remove("description");
        obj.remove("examples");
        obj.remove("example");
    }
    out
}

pub fn compact_tool_schema(schema: &ToolSchema) -> ToolSchema {
    ToolSchema {
        name: schema.name.clone(),
        description: compact_tool_description(&schema.description),
        parameters: compact_json_schema(&schema.parameters),
        strict: schema.strict,
    }
}

pub fn prepare_schemas_for_mode(schemas: &[ToolSchema], mode: ToolSchemaMode) -> Vec<ToolSchema> {
    let mode = resolve_effective_schema_mode(mode, schemas.len());
    match mode {
        ToolSchemaMode::Full => schemas.to_vec(),
        ToolSchemaMode::Compact => schemas.iter().map(compact_tool_schema).collect(),
        ToolSchemaMode::Indexed => {
            crate::tool_schema_index::wire_schemas(schemas, &std::collections::HashSet::new(), false)
        }
        ToolSchemaMode::Auto => unreachable!("resolve_effective_schema_mode never returns Auto"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_indexed_parse_auto_for_empty() {
        assert_eq!(ToolSchemaMode::default(), ToolSchemaMode::Indexed);
        assert_eq!(ToolSchemaMode::parse(""), ToolSchemaMode::Auto);
        assert_eq!(ToolSchemaMode::parse("junk"), ToolSchemaMode::Auto);
        assert_eq!(ToolSchemaMode::parse("auto"), ToolSchemaMode::Auto);
        assert_eq!(ToolSchemaMode::parse("indexed"), ToolSchemaMode::Indexed);
        assert_eq!(ToolSchemaMode::parse("full"), ToolSchemaMode::Full);
        assert_eq!(ToolSchemaMode::parse("compact"), ToolSchemaMode::Compact);
    }

    #[test]
    fn resolve_auto_threshold() {
        assert_eq!(
            resolve_effective_schema_mode(ToolSchemaMode::Auto, 14),
            ToolSchemaMode::Compact
        );
        assert_eq!(
            resolve_effective_schema_mode(ToolSchemaMode::Auto, 15),
            ToolSchemaMode::Indexed
        );
        assert_eq!(
            resolve_effective_schema_mode(ToolSchemaMode::Full, 100),
            ToolSchemaMode::Full
        );
    }

    #[test]
    fn compact_description_uses_first_sentence() {
        let d = "Search the web for facts. Use when you need citations. Never guess.";
        let c = compact_tool_description(d);
        assert_eq!(c, "Search the web for facts");
    }

    #[test]
    fn compact_schema_strips_property_descriptions() {
        let schema = ToolSchema {
            name: "demo".into(),
            description: "A".repeat(200),
            parameters: json!({
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "long help text" }
                },
                "required": ["q"]
            }),
            strict: None,
        };
        let compact = compact_tool_schema(&schema);
        assert!(compact.description.len() < 200);
        assert!(
            compact.parameters["properties"]["q"]
                .get("description")
                .is_none()
        );
    }
}
