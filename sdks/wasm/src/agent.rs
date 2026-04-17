//! WASM Agent — the primary entry point for browser/edge environments.

use js_sys::{Array, Function, Object, Promise, Reflect};
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::memory::MemoryManager;
use crate::tool::Tool;
use crate::types::{Message, StreamEvent};

/// Options for creating a new Agent.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentOptions {
    /// API key for the LLM provider.
    pub api_key: Option<String>,
    /// Base URL for the API (override default provider endpoint).
    pub base_url: Option<String>,
    /// Maximum tool-use iterations per turn.
    pub max_iterations: Option<u32>,
    /// Sampling temperature.
    pub temperature: Option<f64>,
    /// Custom instructions appended to system prompt.
    pub instructions: Option<String>,
    /// Enable streaming mode.
    pub streaming: Option<bool>,
}

/// The EdgeCrab Agent — lite WASM variant for browser/edge environments.
///
/// Provides the core agent loop with custom JS-native tools.
/// Built-in tools (file, terminal, browser) are not available in WASM.
#[wasm_bindgen]
pub struct Agent {
    model: String,
    api_key: Option<String>,
    base_url: Option<String>,
    max_iterations: u32,
    temperature: Option<f64>,
    instructions: Option<String>,
    streaming: bool,
    tools: Vec<Tool>,
    messages: Vec<Message>,
    session_id: String,
    memory: MemoryManager,
}

#[wasm_bindgen]
impl Agent {
    /// Create a new Agent with the given model and options.
    ///
    /// ```js
    /// const agent = new Agent("openai/gpt-4o", { apiKey: "sk-..." });
    /// ```
    #[wasm_bindgen(constructor)]
    pub fn new(model: &str, options: JsValue) -> Result<Agent, JsValue> {
        let opts: AgentOptions = if options.is_undefined() || options.is_null() {
            AgentOptions::default()
        } else {
            serde_wasm_bindgen::from_value(options)
                .map_err(|e| JsValue::from_str(&format!("Invalid options: {e}")))?
        };

        let session_id = format!(
            "wasm_{}",
            js_sys::Date::now() as u64
        );

        Ok(Agent {
            model: model.to_string(),
            api_key: opts.api_key,
            base_url: opts.base_url,
            max_iterations: opts.max_iterations.unwrap_or(90),
            temperature: opts.temperature,
            instructions: opts.instructions,
            streaming: opts.streaming.unwrap_or(false),
            tools: Vec::new(),
            messages: Vec::new(),
            session_id,
            memory: MemoryManager::new(),
        })
    }

    /// Register a custom tool with the agent.
    ///
    /// ```js
    /// agent.addTool(Tool.create({
    ///   name: "my_tool",
    ///   description: "Does something",
    ///   parameters: { input: { type: "string" } },
    ///   handler: async (args) => JSON.stringify({ result: "ok" }),
    /// }));
    /// ```
    #[wasm_bindgen(js_name = "addTool")]
    pub fn add_tool(&mut self, tool: Tool) {
        self.tools.push(tool);
    }

    /// Send a message and get the response.
    ///
    /// Returns a Promise<string>.
    pub async fn chat(&mut self, message: &str) -> Result<JsValue, JsValue> {
        self.messages.push(Message::user(message));
        let text = self.run_agent_loop().await?;
        Ok(JsValue::from_str(&text))
    }

    /// Stream events from the agent.
    ///
    /// Returns a Promise<StreamEvent[]>.
    pub async fn stream(&mut self, message: &str) -> Result<JsValue, JsValue> {
        self.messages.push(Message::user(message));
        let text = self.run_agent_loop().await?;

        // Return events array
        let events = Array::new();
        events.push(&StreamEvent::token(&text).to_js()?);
        events.push(&StreamEvent::done().to_js()?);
        Ok(events.into())
    }

    /// Get the current model name.
    #[wasm_bindgen(getter)]
    pub fn model(&self) -> String {
        self.model.clone()
    }

    /// Get the current session ID.
    #[wasm_bindgen(getter, js_name = "sessionId")]
    pub fn session_id(&self) -> String {
        self.session_id.clone()
    }

    /// Hot-swap the model at runtime.
    #[wasm_bindgen(js_name = "setModel")]
    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
    }

    /// Enable or disable pseudo-streaming mode.
    #[wasm_bindgen(js_name = "setStreaming")]
    pub fn set_streaming(&mut self, enabled: bool) {
        self.streaming = enabled;
    }

    /// Get the conversation history as an array of message objects.
    #[wasm_bindgen(js_name = "getHistory")]
    pub fn get_history(&self) -> Result<JsValue, JsValue> {
        let arr = Array::new();
        for msg in &self.messages {
            arr.push(&msg.to_js()?);
        }
        Ok(arr.into())
    }

    /// Get the number of registered tools.
    #[wasm_bindgen(js_name = "toolCount")]
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// List registered tool names.
    #[wasm_bindgen(js_name = "toolNames")]
    pub fn tool_names(&self) -> JsValue {
        let arr = Array::new();
        for tool in &self.tools {
            arr.push(&JsValue::from_str(&tool.name()));
        }
        arr.into()
    }

    /// Interrupt the current agent run.
    pub fn interrupt(&self) {
        // In WASM, we rely on JS-side AbortController for cancellation.
        // This is a no-op placeholder for API compatibility.
    }

    /// Start a new session (reset conversation).
    #[wasm_bindgen(js_name = "newSession")]
    pub fn new_session(&mut self) {
        self.messages.clear();
        self.session_id = format!("wasm_{}", js_sys::Date::now() as u64);
    }

    /// Fork the agent into an isolated copy.
    pub fn fork(&self) -> Result<Agent, JsValue> {
        Ok(Agent {
            model: self.model.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            max_iterations: self.max_iterations,
            temperature: self.temperature,
            instructions: self.instructions.clone(),
            streaming: self.streaming,
            tools: self.tools.clone(),
            messages: self.messages.clone(),
            session_id: format!("wasm_{}", js_sys::Date::now() as u64),
            memory: self.memory.clone(),
        })
    }

    /// Get the MemoryManager for reading/writing agent memory.
    #[wasm_bindgen(getter)]
    pub fn memory(&self) -> MemoryManager {
        self.memory.clone()
    }
}

impl Agent {
    async fn run_agent_loop(&mut self) -> Result<String, JsValue> {
        let mut conversation = self.build_api_messages();
        let max_iters = self.max_iterations.max(1);

        for _ in 0..max_iters {
            let json_val = self.call_llm_once(&conversation).await?;
            let choices = Reflect::get(&json_val, &"choices".into())?;
            let first = Reflect::get(&choices, &0u32.into())?;
            let message = Reflect::get(&first, &"message".into())?;
            let content = Self::extract_message_content(&message)?;
            let tool_calls_val = Reflect::get(&message, &"tool_calls".into())
                .unwrap_or(JsValue::UNDEFINED);
            let tool_calls = if tool_calls_val.is_undefined() || tool_calls_val.is_null() {
                Array::new()
            } else {
                Array::from(&tool_calls_val)
            };

            if tool_calls.length() > 0 {
                let mut assistant_msg = json!({ "role": "assistant" });
                if content.is_empty() {
                    assistant_msg["content"] = serde_json::Value::Null;
                } else {
                    assistant_msg["content"] = json!(content.clone());
                    self.messages.push(Message::assistant(&content));
                }
                assistant_msg["tool_calls"] = serde_wasm_bindgen::from_value(tool_calls_val.clone())
                    .map_err(|e| JsValue::from_str(&format!("tool_calls decode error: {e}")))?;
                conversation.push(assistant_msg);

                for call in tool_calls.iter() {
                    let function = Reflect::get(&call, &"function".into())?;
                    let name = Reflect::get(&function, &"name".into())?
                        .as_string()
                        .unwrap_or_default();
                    let args_str = Reflect::get(&function, &"arguments".into())?
                        .as_string()
                        .unwrap_or_else(|| "{}".to_string());
                    let call_id = Reflect::get(&call, &"id".into())?
                        .as_string()
                        .unwrap_or_else(|| format!("call_{}", js_sys::Date::now() as u64));

                    let args_json: serde_json::Value = serde_json::from_str(&args_str)
                        .unwrap_or_else(|_| json!({ "raw": args_str }));
                    let args_js = serde_wasm_bindgen::to_value(&args_json)
                        .map_err(|e| JsValue::from_str(&format!("args encode error: {e}")))?;

                    let result_text = if let Some(tool) = self.tools.iter().find(|t| t.name() == name) {
                        let value = tool.execute(args_js).await?;
                        if let Some(text) = value.as_string() {
                            text
                        } else {
                            js_sys::JSON::stringify(&value)
                                .ok()
                                .and_then(|s| s.as_string())
                                .unwrap_or_else(|| String::from("null"))
                        }
                    } else {
                        json!({ "error": format!("Unknown tool: {name}") }).to_string()
                    };

                    self.messages.push(Message::tool(&result_text));
                    conversation.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": result_text,
                    }));
                }
                continue;
            }

            self.messages.push(Message::assistant(&content));
            return Ok(content);
        }

        Err(JsValue::from_str("Agent exceeded max_iterations while processing tool calls"))
    }

    fn build_api_messages(&self) -> Vec<serde_json::Value> {
        let mut all_messages = Vec::new();
        if let Some(ref instructions) = self.instructions {
            all_messages.push(json!({
                "role": "system",
                "content": instructions,
            }));
        }

        all_messages.extend(self.messages.iter().map(|m| {
            json!({
                "role": m.role_str(),
                "content": m.content_str(),
            })
        }));

        all_messages
    }

    fn extract_message_content(message: &JsValue) -> Result<String, JsValue> {
        let content = Reflect::get(message, &"content".into())?;
        if let Some(text) = content.as_string() {
            return Ok(text);
        }

        if Array::is_array(&content) {
            let parts = Array::from(&content);
            let mut out = String::new();
            for part in parts.iter() {
                if let Ok(text) = Reflect::get(&part, &"text".into()) {
                    if let Some(s) = text.as_string() {
                        out.push_str(&s);
                    }
                }
            }
            return Ok(out);
        }

        Ok(String::new())
    }

    /// Internal: call the LLM API once using the current conversation payload.
    async fn call_llm_once(&self, all_messages: &[serde_json::Value]) -> Result<JsValue, JsValue> {
        // Determine API endpoint
        let base_url = self.base_url.as_deref().unwrap_or(
            if self.model.starts_with("anthropic/") {
                "https://api.anthropic.com/v1"
            } else {
                "https://api.openai.com/v1"
            },
        );

        let api_key = self.api_key.as_deref().unwrap_or("");

        // Build tool schemas
        let tool_schemas: Vec<serde_json::Value> = self
            .tools
            .iter()
            .map(|t| t.to_schema())
            .collect();

        // Strip provider prefix for API call
        let model_name = self
            .model
            .split('/')
            .last()
            .unwrap_or(&self.model);

        let mut body = json!({
            "model": model_name,
            "messages": all_messages,
        });

        if !tool_schemas.is_empty() {
            body["tools"] = json!(tool_schemas);
        }
        if let Some(temp) = self.temperature {
            body["temperature"] = json!(temp);
        }

        // Use global fetch
        let headers = Object::new();
        Reflect::set(
            &headers,
            &"Content-Type".into(),
            &"application/json".into(),
        )?;
        if !api_key.is_empty() {
            Reflect::set(
                &headers,
                &"Authorization".into(),
                &format!("Bearer {api_key}").into(),
            )?;
        }

        let opts = Object::new();
        Reflect::set(&opts, &"method".into(), &"POST".into())?;
        Reflect::set(&opts, &"headers".into(), &headers)?;
        Reflect::set(
            &opts,
            &"body".into(),
            &JsValue::from_str(&body.to_string()),
        )?;

        let url = format!("{base_url}/chat/completions");
        let window = js_sys::global();
        let fetch_fn: Function = Reflect::get(&window, &"fetch".into())?
            .dyn_into()
            .map_err(|_| JsValue::from_str("fetch not available in this environment"))?;

        let promise: Promise = fetch_fn
            .call2(&JsValue::NULL, &JsValue::from_str(&url), &opts)?
            .dyn_into()
            .map_err(|_| JsValue::from_str("fetch did not return a Promise"))?;

        let response = JsFuture::from(promise).await?;
        let ok = Reflect::get(&response, &"ok".into())?
            .as_bool()
            .unwrap_or(false);
        let status = Reflect::get(&response, &"status".into())?
            .as_f64()
            .unwrap_or(0.0) as i32;

        // Parse response
        let json_fn: Function = Reflect::get(&response, &"json".into())?
            .dyn_into()
            .map_err(|_| JsValue::from_str("response.json() not available"))?;

        let json_promise: Promise = json_fn
            .call0(&response)?
            .dyn_into()
            .map_err(|_| JsValue::from_str("json() did not return a Promise"))?;

        let json_val = JsFuture::from(json_promise).await?;

        if !ok {
            let detail = js_sys::JSON::stringify(&json_val)
                .ok()
                .and_then(|s| s.as_string())
                .unwrap_or_else(|| String::from("unknown API error"));
            return Err(JsValue::from_str(&format!("LLM request failed with status {status}: {detail}")));
        }

        Ok(json_val)
    }
}
