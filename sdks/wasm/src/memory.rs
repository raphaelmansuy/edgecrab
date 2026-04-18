//! WASM MemoryManager — in-memory key/value store for browser/edge environments.

use std::collections::HashMap;

use wasm_bindgen::prelude::*;

/// In-memory key/value store for the WASM agent.
///
/// Unlike the native MemoryManager which persists to disk (MEMORY.md / USER.md),
/// this WASM variant stores entries in memory only. For persistence, users should
/// serialize via `toJSON()` and store in localStorage / IndexedDB.
#[wasm_bindgen]
#[derive(Clone, Debug, Default)]
pub struct MemoryManager {
    entries: HashMap<String, Vec<String>>,
}

#[wasm_bindgen]
impl MemoryManager {
    /// Create a new empty MemoryManager.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        MemoryManager {
            entries: HashMap::new(),
        }
    }

    /// Read all content for a key. Entries are joined with newlines.
    pub fn read(&self, key: &str) -> String {
        self.entries
            .get(key)
            .map(|v| v.join("\n"))
            .unwrap_or_default()
    }

    /// Write (append) a new entry for a key.
    pub fn write(&mut self, key: &str, value: &str) {
        self.entries
            .entry(key.to_string())
            .or_default()
            .push(value.to_string());
    }

    /// Remove an entry by substring match. Returns true if removed.
    pub fn remove(&mut self, key: &str, old_content: &str) -> bool {
        if let Some(entries) = self.entries.get_mut(key) {
            let before = entries.len();
            entries.retain(|e| !e.contains(old_content));
            entries.len() < before
        } else {
            false
        }
    }

    /// List all entries for a key.
    pub fn entries(&self, key: &str) -> JsValue {
        let arr = js_sys::Array::new();
        if let Some(items) = self.entries.get(key) {
            for item in items {
                arr.push(&JsValue::from_str(item));
            }
        }
        arr.into()
    }

    /// Serialize all memory to a JSON string (for persistence to localStorage/IndexedDB).
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.entries)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {e}")))
    }

    /// Load memory from a JSON string (restored from localStorage/IndexedDB).
    #[wasm_bindgen(js_name = "fromJSON")]
    pub fn from_json(json: &str) -> Result<MemoryManager, JsValue> {
        let entries: HashMap<String, Vec<String>> = serde_json::from_str(json)
            .map_err(|e| JsValue::from_str(&format!("Parse error: {e}")))?;
        Ok(MemoryManager { entries })
    }
}
