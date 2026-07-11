//! OpenAI Chat Completions API shapes (subset used by the proxy).

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessageIn>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub tools: Option<Vec<ToolIn>>,
    #[serde(default)]
    pub tool_choice: Option<JsonValue>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessageIn {
    pub role: String,
    #[serde(default)]
    pub content: MessageContentIn,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallIn>>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum MessageContentIn {
    Text(String),
    Parts(Vec<ContentPartIn>),
}

impl Default for MessageContentIn {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl MessageContentIn {
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Parts(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    ContentPartIn::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPartIn {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolIn {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionIn,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FunctionIn {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub parameters: JsonValue,
    #[serde(default)]
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallIn {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    pub function: FunctionCallIn,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FunctionCallIn {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: &'static str,
    pub created: Option<u64>,
    pub model: String,
    pub choices: Vec<ChatChoiceOut>,
    pub usage: UsageOut,
}

#[derive(Debug, Serialize)]
pub struct ChatChoiceOut {
    pub index: u32,
    pub message: ChatMessageOut,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct ChatMessageOut {
    pub role: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallOut>>,
}

#[derive(Debug, Serialize)]
pub struct ToolCallOut {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: &'static str,
    pub function: FunctionCallOut,
}

#[derive(Debug, Serialize)]
pub struct FunctionCallOut {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Default)]
pub struct UsageOut {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Serialize)]
pub struct ModelsListResponse {
    pub object: &'static str,
    pub data: Vec<ModelObject>,
}

#[derive(Debug, Serialize)]
pub struct ModelObject {
    pub id: String,
    pub object: &'static str,
    pub created: Option<u64>,
    pub owned_by: String,
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
