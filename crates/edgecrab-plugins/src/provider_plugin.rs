//! Pluggable provider / tool-override surface (gap 009).
//!
//! Native plugins can register LLM providers and wrap tools without forking
//! the EdgeCrab binary. Discovery is host-driven (`PluginHost`).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

/// Lightweight completion handle exposed to plugins (DIP — plugins depend on
/// this trait, not on `edgequake_llm` concrete types).
#[async_trait]
pub trait LlmHandle: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String, String>;
}

/// Event delivered to plugins during the agent lifecycle.
#[derive(Debug, Clone)]
pub enum PluginEvent {
    SessionStart { session_id: String },
    SessionEnd { session_id: String },
    BeforeToolCall { tool: String, args: Value },
    AfterToolCall { tool: String, result: String },
}

/// Context passed into plugin hooks.
pub struct PluginContext {
    pub session_id: String,
    pub llm: Option<Arc<dyn LlmHandle>>,
}

/// Extensibility contract for native / WASM / subprocess plugins.
pub trait EdgecrabPlugin: Send + Sync {
    fn name(&self) -> &str;

    /// Optional provider aliases this plugin contributes (`provider/model`).
    fn provider_aliases(&self) -> Vec<String> {
        Vec::new()
    }

    /// Optionally wrap a tool handler by name. Return `None` to leave unchanged.
    fn tool_override_hint(&self, _tool_name: &str) -> Option<String> {
        None
    }

    fn on_event(&self, _event: &PluginEvent, _ctx: &PluginContext) {}
}

/// In-process registry of discovered plugins.
#[derive(Default)]
pub struct PluginHost {
    plugins: Vec<Arc<dyn EdgecrabPlugin>>,
}

impl PluginHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Arc<dyn EdgecrabPlugin>) {
        self.plugins.push(plugin);
    }

    pub fn plugins(&self) -> &[Arc<dyn EdgecrabPlugin>] {
        &self.plugins
    }

    pub fn provider_aliases(&self) -> Vec<String> {
        let mut out = Vec::new();
        for p in &self.plugins {
            out.extend(p.provider_aliases());
        }
        out.sort();
        out.dedup();
        out
    }

    pub fn emit(&self, event: &PluginEvent, ctx: &PluginContext) {
        for p in &self.plugins {
            p.on_event(event, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy;

    impl EdgecrabPlugin for Dummy {
        fn name(&self) -> &str {
            "dummy"
        }
        fn provider_aliases(&self) -> Vec<String> {
            vec!["dummy/echo".into()]
        }
    }

    #[test]
    fn host_collects_aliases() {
        let mut host = PluginHost::new();
        host.register(Arc::new(Dummy));
        assert_eq!(host.provider_aliases(), vec!["dummy/echo".to_string()]);
    }
}
