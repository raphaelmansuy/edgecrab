use std::path::Path;
use std::sync::{Arc, Mutex};

use rhai::{Dynamic, Engine, EvalAltResult, ImmutableString, Scope};

use crate::error::PluginError;

#[derive(Clone, Default)]
pub struct ScriptOutput {
    pub emitted_messages: Arc<Mutex<Vec<String>>>,
}

pub struct ScriptRuntime {
    engine: Engine,
    ast: rhai::AST,
    output: ScriptOutput,
}

impl ScriptRuntime {
    pub fn load(
        path: &Path,
        max_operations: u64,
        max_call_depth: usize,
    ) -> Result<Self, PluginError> {
        let mut engine = Engine::new();
        engine.set_max_operations(max_operations);
        engine.set_max_call_levels(max_call_depth);
        engine.on_print(|message| tracing::info!(target: "plugin_script", "{message}"));
        let output = ScriptOutput::default();
        let emitted = output.emitted_messages.clone();
        engine.register_fn("log", |level: &str, msg: &str| {
            tracing::info!(target: "plugin_script", level, "{msg}");
        });
        engine.register_fn("get_env", |key: &str| {
            std::env::var(key).unwrap_or_default()
        });
        engine.register_fn("emit_message", move |msg: &str| {
            if let Ok(mut messages) = emitted.lock() {
                messages.push(msg.to_string());
            }
        });

        let source = std::fs::read_to_string(path)?;
        let ast = engine
            .compile(&source)
            .map_err(|error| PluginError::Script(error.to_string()))?;
        Ok(Self {
            engine,
            ast,
            output,
        })
    }

    pub fn call_tool(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, PluginError> {
        let mut scope = Scope::new();
        let args_json = serde_json::to_string(args)?;
        let result: Result<ImmutableString, Box<EvalAltResult>> = self.engine.call_fn(
            &mut scope,
            &self.ast,
            "tool_call",
            (tool_name.to_string(), args_json),
        );
        result
            .map(|value| value.to_string())
            .map_err(|error| PluginError::Script(error.to_string()))
    }

    pub fn run_hook(
        &self,
        hook_name: &str,
        payload: &serde_json::Value,
    ) -> Result<(), PluginError> {
        let mut scope = Scope::new();
        let payload_json = serde_json::to_string(payload)?;
        let _: Dynamic = self
            .engine
            .call_fn(
                &mut scope,
                &self.ast,
                "run_hook",
                (hook_name.to_string(), payload_json),
            )
            .map_err(|error| PluginError::Script(error.to_string()))?;
        Ok(())
    }

    pub fn take_emitted_messages(&self) -> Vec<String> {
        self.output
            .emitted_messages
            .lock()
            .map(|mut messages| std::mem::take(&mut *messages))
            .unwrap_or_default()
    }
}
