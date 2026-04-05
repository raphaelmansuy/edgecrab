//! Tool call types for LLM function calling.

use serde::{Deserialize, Serialize};

/// A tool call requested by the LLM (OpenAI function-calling format).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
    /// Gemini 3.x thought signature. Opaque blob that must round-trip through
    /// conversation history so Gemini can resume its reasoning state after a
    /// function call.  None for all non-Gemini providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    /// JSON-encoded arguments string
    pub arguments: String,
}

impl ToolCall {
    /// Parse the JSON arguments string into a Value.
    pub fn parsed_args(&self) -> std::result::Result<serde_json::Value, serde_json::Error> {
        serde_json::from_str(&self.function.arguments)
    }

    /// Convert from edgequake-llm's ToolCall into our internal type.
    ///
    /// WHY a manual conversion: edgequake-llm uses `call_type` while we
    /// use `r#type` (matching OpenAI's raw JSON key). Both represent the
    /// same concept so this is a straightforward field rename.
    pub fn from_llm(tc: &edgequake_llm::ToolCall) -> Self {
        Self {
            id: tc.id.clone(),
            r#type: tc.call_type.clone(),
            function: FunctionCall {
                name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
            },
            thought_signature: tc.thought_signature.clone(),
        }
    }

    /// Convert to edgequake-llm's ToolCall for ChatMessage construction.
    pub fn to_llm(&self) -> edgequake_llm::ToolCall {
        edgequake_llm::ToolCall {
            id: self.id.clone(),
            call_type: self.r#type.clone(),
            function: edgequake_llm::FunctionCall {
                name: self.function.name.clone(),
                arguments: self.function.arguments.clone(),
            },
            thought_signature: self.thought_signature.clone(),
        }
    }
}

/// Tool schema in OpenAI function-calling format — sent to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_roundtrip() {
        let tc = ToolCall {
            id: "call_abc123".into(),
            r#type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"src/lib.rs","line_start":1}"#.into(),
            },
            thought_signature: None,
        };
        let json = serde_json::to_string(&tc).expect("serialize");
        let deser: ToolCall = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(tc, deser);
    }

    #[test]
    fn parsed_args_valid_json() {
        let tc = ToolCall {
            id: "1".into(),
            r#type: "function".into(),
            function: FunctionCall {
                name: "write_file".into(),
                arguments: r#"{"path":"out.txt","content":"hello"}"#.into(),
            },
            thought_signature: None,
        };
        let args = tc.parsed_args().expect("valid json");
        assert_eq!(args["path"], "out.txt");
        assert_eq!(args["content"], "hello");
    }

    #[test]
    fn parsed_args_invalid_json() {
        let tc = ToolCall {
            id: "1".into(),
            r#type: "function".into(),
            function: FunctionCall {
                name: "bad".into(),
                arguments: "not json".into(),
            },
            thought_signature: None,
        };
        assert!(tc.parsed_args().is_err());
    }

    #[test]
    fn tool_schema_roundtrip() {
        let schema = ToolSchema {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
            strict: Some(true),
        };
        let json = serde_json::to_string(&schema).expect("serialize");
        let deser: ToolSchema = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(schema, deser);
    }
}
