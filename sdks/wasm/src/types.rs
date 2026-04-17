//! WASM types — Message, Role, StreamEvent.

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Message role in a conversation.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A conversation message.
#[wasm_bindgen]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    role: Role,
    content: String,
}

#[wasm_bindgen]
impl Message {
    /// Create a system message.
    pub fn system(content: &str) -> Message {
        Message {
            role: Role::System,
            content: content.to_string(),
        }
    }

    /// Create a user message.
    pub fn user(content: &str) -> Message {
        Message {
            role: Role::User,
            content: content.to_string(),
        }
    }

    /// Create an assistant message.
    pub fn assistant(content: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: content.to_string(),
        }
    }

    /// Create a tool result message.
    pub fn tool(content: &str) -> Message {
        Message {
            role: Role::Tool,
            content: content.to_string(),
        }
    }

    /// Get the role.
    #[wasm_bindgen(getter)]
    pub fn role(&self) -> Role {
        self.role
    }

    /// Get the content.
    #[wasm_bindgen(getter)]
    pub fn content(&self) -> String {
        self.content.clone()
    }
}

impl Message {
    /// Get role as string for API calls.
    pub(crate) fn role_str(&self) -> &str {
        match self.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }

    /// Get content as string for API calls.
    pub(crate) fn content_str(&self) -> &str {
        &self.content
    }

    /// Serialize to a JS object.
    pub(crate) fn to_js(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(self)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {e}")))
    }
}

/// A streaming event from the agent.
#[wasm_bindgen]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamEvent {
    /// Event type: "token" | "reasoning" | "tool_exec" | "tool_result" | "done" | "error"
    #[wasm_bindgen(js_name = "eventType")]
    pub(crate) event_type: String,
    /// Event data.
    pub(crate) data: String,
}

#[wasm_bindgen]
impl StreamEvent {
    /// Get the event type.
    #[wasm_bindgen(getter, js_name = "eventType")]
    pub fn event_type(&self) -> String {
        self.event_type.clone()
    }

    /// Get the event data.
    #[wasm_bindgen(getter)]
    pub fn data(&self) -> String {
        self.data.clone()
    }
}

impl StreamEvent {
    /// Create a token event.
    pub(crate) fn token(text: &str) -> Self {
        StreamEvent {
            event_type: "token".into(),
            data: text.into(),
        }
    }

    /// Create a done event.
    pub(crate) fn done() -> Self {
        StreamEvent {
            event_type: "done".into(),
            data: String::new(),
        }
    }

    /// Create an error event.
    pub(crate) fn error(msg: &str) -> Self {
        StreamEvent {
            event_type: "error".into(),
            data: msg.into(),
        }
    }

    /// Serialize to a JS object.
    pub(crate) fn to_js(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(self)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {e}")))
    }
}
