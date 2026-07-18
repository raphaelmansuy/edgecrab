//! Message types for LLM conversations.
//!
//! Models the OpenAI/Anthropic message format with extensions for
//! tool calls, reasoning blocks, and multimodal content.

use serde::{Deserialize, Serialize};

/// Conversation message — the fundamental unit of LLM interaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<crate::ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool name — present on tool result messages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Extracted reasoning/thinking content (e.g. from `<think>` blocks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Wall-clock (unix seconds) when the message was created — forensics only.
    /// Never sent to providers; set on push / restored from session DB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<f64>,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            role: Role::User,
            content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            finish_reason: None,
            created_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Content can be a simple string or multimodal parts (text + images).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

impl Message {
    pub fn user(text: &str) -> Self {
        Self {
            role: Role::User,
            content: Some(Content::Text(text.to_string())),
            ..Default::default()
        }
    }

    pub fn assistant(text: &str) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(Content::Text(text.to_string())),
            ..Default::default()
        }
    }

    pub fn system(text: &str) -> Self {
        Self {
            role: Role::System,
            content: Some(Content::Text(text.to_string())),
            ..Default::default()
        }
    }

    pub fn tool_result(tool_call_id: &str, name: &str, content: &str) -> Self {
        Self {
            role: Role::Tool,
            content: Some(Content::Text(content.to_string())),
            tool_call_id: Some(tool_call_id.to_string()),
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    /// Build a tool result message, promoting computer_use multimodal JSON to parts.
    pub fn tool_result_from_output(tool_call_id: &str, name: &str, content: &str) -> Self {
        Self::tool_result_from_output_with_policy(tool_call_id, name, content, true)
    }

    /// Like [`tool_result_from_output`], but when `store_inline_images` is false,
    /// never promotes inline screenshots into `Content::Parts` (Hermes session cache parity).
    pub fn tool_result_from_output_with_policy(
        tool_call_id: &str,
        name: &str,
        content: &str,
        store_inline_images: bool,
    ) -> Self {
        let content = if store_inline_images {
            content.to_string()
        } else {
            crate::multimodal::strip_inline_images_from_tool_output(name, content)
        };
        let content = content.as_str();
        if name == "computer_use"
            && let Some(value) = crate::multimodal::parse_multimodal_value(content)
        {
            // Session downgrade: keep compact JSON/text only (API attach policy is separate).
            if !store_inline_images {
                return Self::tool_result(tool_call_id, name, content);
            }
            // Path-only envelope: compact JSON in history; image attached at API boundary.
            if crate::multimodal::multimodal_disk_image(&value).is_some()
                && !crate::multimodal::multimodal_value_has_inline_image(&value)
            {
                return Self::tool_result(tool_call_id, name, content);
            }

            let summary = value
                .get("text_summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mut parts = vec![ContentPart::Text { text: summary }];
            if let Some(items) = value.get("content").and_then(|c| c.as_array()) {
                for item in items {
                    if item.get("type").and_then(|t| t.as_str()) == Some("image_url")
                        && let Some(url) = item
                            .get("image_url")
                            .and_then(|iu| iu.get("url"))
                            .and_then(|u| u.as_str())
                    {
                        parts.push(ContentPart::ImageUrl {
                            image_url: ImageUrl {
                                url: url.to_string(),
                                detail: Some("auto".into()),
                            },
                        });
                    }
                }
            }
            return Self {
                role: Role::Tool,
                content: Some(Content::Parts(parts)),
                tool_call_id: Some(tool_call_id.to_string()),
                name: Some(name.to_string()),
                ..Default::default()
            };
        }
        Self::tool_result(tool_call_id, name, content)
    }

    /// Persist a tool result using the multimodal attach policy for this session.
    pub fn tool_result_for_session_policy(
        tool_call_id: &str,
        name: &str,
        raw_output: &str,
        store_inline_images: bool,
    ) -> Self {
        Self::tool_result_from_output_with_policy(
            tool_call_id,
            name,
            raw_output,
            store_inline_images,
        )
    }

    /// Assistant message that requested tool calls.
    ///
    /// WHY store tool_calls on assistant messages: When rebuilding the
    /// chat history for the LLM API, we need to pair each tool result
    /// with the assistant message that requested it. Without storing
    /// the original tool_calls, we lose the correlation and the LLM
    /// can't understand the conversation flow.
    pub fn assistant_with_tool_calls(text: &str, tool_calls: Vec<crate::ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: if text.is_empty() {
                None
            } else {
                Some(Content::Text(text.to_string()))
            },
            tool_calls: Some(tool_calls),
            ..Default::default()
        }
    }

    /// Summary message injected by context compression.
    pub fn system_summary(text: String) -> Self {
        Self {
            role: Role::System,
            content: Some(Content::Text(text)),
            name: Some("context_summary".to_string()),
            ..Default::default()
        }
    }

    /// Extract plaintext from content, joining multimodal parts.
    pub fn text_content(&self) -> String {
        match &self.content {
            Some(Content::Text(t)) => t.clone(),
            Some(Content::Parts(parts)) => parts
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            None => String::new(),
        }
    }

    /// True if this message has tool call requests from the assistant.
    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls
            .as_ref()
            .is_some_and(|calls| !calls.is_empty())
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_roundtrip() {
        let msg = Message::user("hello world");
        let json = serde_json::to_string(&msg).expect("serialize");
        let deser: Message = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(msg, deser);
        assert_eq!(deser.text_content(), "hello world");
    }

    #[test]
    fn assistant_message_with_tool_calls() {
        let msg = Message {
            role: Role::Assistant,
            content: Some(Content::Text("I'll read that file.".into())),
            tool_calls: Some(vec![crate::ToolCall {
                id: "call_1".into(),
                r#type: "function".into(),
                function: crate::FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"src/main.rs"}"#.into(),
                },
                thought_signature: None,
            }]),
            tool_call_id: None,
            name: None,
            reasoning: None,
            finish_reason: None,
            created_at: None,
        };
        assert!(msg.has_tool_calls());
        let json = serde_json::to_string(&msg).expect("serialize");
        let deser: Message = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(msg, deser);
    }

    #[test]
    fn tool_result_message() {
        let msg = Message::tool_result("call_1", "read_file", "fn main() {}");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(msg.text_content(), "fn main() {}");
    }

    #[test]
    fn multimodal_content_text_extraction() {
        let msg = Message {
            role: Role::User,
            content: Some(Content::Parts(vec![
                ContentPart::Text {
                    text: "Look at this:".into(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: "data:image/png;base64,abc".into(),
                        detail: Some("high".into()),
                    },
                },
                ContentPart::Text {
                    text: "What do you see?".into(),
                },
            ])),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            finish_reason: None,
            created_at: None,
        };
        assert_eq!(msg.text_content(), "Look at this:\nWhat do you see?");
    }

    #[test]
    fn empty_content_returns_empty_string() {
        let msg = Message {
            role: Role::Assistant,
            content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            finish_reason: None,
            created_at: None,
        };
        assert_eq!(msg.text_content(), "");
    }

    #[test]
    fn role_display() {
        assert_eq!(format!("{}", Role::System), "system");
        assert_eq!(format!("{}", Role::User), "user");
        assert_eq!(format!("{}", Role::Assistant), "assistant");
        assert_eq!(format!("{}", Role::Tool), "tool");
    }

    #[test]
    fn role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            let json = serde_json::to_string(&role).expect("serialize");
            let deser: Role = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(role, deser);
        }
    }
}

/// Property-based tests for Message fuzzing
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_role() -> impl Strategy<Value = Role> {
        prop_oneof![
            Just(Role::System),
            Just(Role::User),
            Just(Role::Assistant),
            Just(Role::Tool),
        ]
    }

    fn arb_content() -> impl Strategy<Value = Content> {
        prop_oneof![
            ".*".prop_map(Content::Text),
            prop::collection::vec(".*".prop_map(|t| ContentPart::Text { text: t }), 0..5)
                .prop_map(Content::Parts),
        ]
    }

    fn arb_message() -> impl Strategy<Value = Message> {
        (arb_role(), proptest::option::of(arb_content())).prop_map(|(role, content)| Message {
            role,
            content,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            finish_reason: None,
            created_at: None,
        })
    }

    proptest! {
        #[test]
        fn message_serde_roundtrip(msg in arb_message()) {
            let json = serde_json::to_string(&msg).expect("serialize");
            let deser: Message = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(msg, deser);
        }

        #[test]
        fn text_content_never_panics(msg in arb_message()) {
            let _ = msg.text_content(); // should never panic
        }
    }
}
