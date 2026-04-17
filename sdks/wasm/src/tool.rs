//! WASM Tool — custom tool definitions for browser/edge environments.

use wasm_bindgen::prelude::*;

/// A custom tool definition for the WASM agent.
///
/// ```js
/// const tool = Tool.create({
///   name: "get_weather",
///   description: "Get weather for a city",
///   parameters: {
///     type: "object",
///     properties: { city: { type: "string" } },
///     required: ["city"]
///   },
///   handler: async (args) => JSON.stringify({ temp: 72 }),
/// });
/// ```
#[wasm_bindgen]
#[derive(Clone)]
pub struct Tool {
    tool_name: String,
    description: String,
    parameters: serde_json::Value,
    handler: js_sys::Function,
}

#[wasm_bindgen]
impl Tool {
    /// Create a new Tool from a JS definition object.
    ///
    /// The definition must have: `name` (string), `description` (string),
    /// `handler` (async function). Optional: `parameters` (object).
    pub fn create(definition: JsValue) -> Result<Tool, JsValue> {
        let name = js_sys::Reflect::get(&definition, &"name".into())?
            .as_string()
            .ok_or_else(|| JsValue::from_str("Tool 'name' is required and must be a string"))?;

        let description = js_sys::Reflect::get(&definition, &"description".into())?
            .as_string()
            .ok_or_else(|| {
                JsValue::from_str("Tool 'description' is required and must be a string")
            })?;

        let handler_val = js_sys::Reflect::get(&definition, &"handler".into())?;
        let handler: js_sys::Function = handler_val
            .dyn_into()
            .map_err(|_| JsValue::from_str("Tool 'handler' is required and must be a function"))?;

        let params_val = js_sys::Reflect::get(&definition, &"parameters".into())?;
        let parameters = if params_val.is_undefined() || params_val.is_null() {
            serde_json::json!({"type": "object", "properties": {}})
        } else {
            serde_wasm_bindgen::from_value(params_val)
                .map_err(|e| JsValue::from_str(&format!("Invalid parameters: {e}")))?
        };

        Ok(Tool {
            tool_name: name,
            description,
            parameters,
            handler,
        })
    }

    /// Get the tool name.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.tool_name.clone()
    }

    /// Get the tool description.
    #[wasm_bindgen(getter)]
    pub fn description(&self) -> String {
        self.description.clone()
    }

    /// Get the tool schema in OpenAI-compatible format.
    #[wasm_bindgen(js_name = "toSchema")]
    pub fn to_schema_js(&self) -> JsValue {
        let schema = self.to_schema();
        serde_wasm_bindgen::to_value(&schema).unwrap_or(JsValue::NULL)
    }
}

impl Tool {
    /// Get the schema as a serde_json::Value (internal use).
    pub(crate) fn to_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.tool_name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }

    /// Execute the handler with the given arguments.
    pub(crate) async fn execute(&self, args: JsValue) -> Result<JsValue, JsValue> {
        let result = self.handler.call1(&JsValue::NULL, &args)?;

        // If it returns a Promise, await it
        if let Ok(promise) = result.clone().dyn_into::<js_sys::Promise>() {
            wasm_bindgen_futures::JsFuture::from(promise).await
        } else {
            Ok(result)
        }
    }
}
