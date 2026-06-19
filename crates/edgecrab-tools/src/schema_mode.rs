//! Tool schema compaction for turn-1 token budget (spec 007 L1.2).
//!
//! `Full` — wire tool schemas verbatim.
//! `Compact` — truncate descriptions and strip per-property prose from JSON Schema.

use edgecrab_types::ToolSchema;
use serde_json::{Map, Value};

/// How tool JSON schemas are sent to the LLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSchemaMode {
    #[default]
    Full,
    Compact,
    /// Hot tools on wire + deferred index + `tool_search` materialization.
    Indexed,
}

impl ToolSchemaMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "compact" => Self::Compact,
            "indexed" => Self::Indexed,
            _ => Self::Full,
        }
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
    match mode {
        ToolSchemaMode::Full => schemas.to_vec(),
        ToolSchemaMode::Compact => schemas.iter().map(compact_tool_schema).collect(),
        ToolSchemaMode::Indexed => {
            crate::tool_schema_index::wire_schemas(schemas, &std::collections::HashSet::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
