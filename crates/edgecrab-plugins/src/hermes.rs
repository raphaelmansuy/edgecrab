use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::discovery::DiscoveredPlugin;
use crate::error::PluginError;
use crate::manifest::{PluginCapabilities, PluginExecConfig, PluginManifest, PluginMetadata, PluginRestartPolicy, PluginToolDefinition};
use crate::tool_server::client::ToolServerClient;
use crate::types::{PluginKind, PluginStatus};

const HERMES_HOST_SCRIPT: &str = r#"
import importlib.util
import json
import sys
import traceback
import types
from pathlib import Path

plugin_dir = Path(sys.argv[1]).resolve()
plugin_name = sys.argv[2]
tools = {}
hooks = {}
_next_request_id = 1000


def _read_message():
    line = sys.stdin.readline()
    if not line:
        raise EOFError("stdin closed")
    return json.loads(line)


def _write_message(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def _host_call(method, params):
    global _next_request_id
    _next_request_id += 1
    request_id = _next_request_id
    _write_message({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
        "params": params,
    })
    while True:
        response = _read_message()
        if response.get("id") != request_id:
            continue
        if "error" in response:
            return {"ok": False, "error": response["error"]}
        return response.get("result") or {}


class PluginContext:
    def register_tool(
        self,
        name,
        toolset=None,
        schema=None,
        handler=None,
        check_fn=None,
        requires_env=None,
        is_async=False,
        description="",
        emoji="",
    ):
        desc = description or (schema or {}).get("description") or f"Hermes plugin tool: {name}"
        tools[name] = {
            "name": name,
            "toolset": toolset or plugin_name,
            "schema": schema or {
                "name": name,
                "description": desc,
                "parameters": {"type": "object", "additionalProperties": True},
            },
            "description": desc,
            "handler": handler,
        }

    def register_hook(self, hook_name, callback):
        hooks.setdefault(hook_name, []).append(callback)

    def inject_message(self, content, role="user"):
        result = _host_call("host:inject_message", {"content": content, "role": role})
        return bool(result.get("ok"))

    def register_cli_command(self, *args, **kwargs):
        return None


def _load_plugin():
    init_file = plugin_dir / "__init__.py"
    if not init_file.exists():
        raise FileNotFoundError(f"No __init__.py in {plugin_dir}")

    module_name = f"edgecrab_hermes_plugins.{plugin_name.replace('-', '_')}"
    if "edgecrab_hermes_plugins" not in sys.modules:
        pkg = types.ModuleType("edgecrab_hermes_plugins")
        pkg.__path__ = []
        pkg.__package__ = "edgecrab_hermes_plugins"
        sys.modules["edgecrab_hermes_plugins"] = pkg

    spec = importlib.util.spec_from_file_location(
        module_name,
        init_file,
        submodule_search_locations=[str(plugin_dir)],
    )
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot create module spec for {init_file}")

    module = importlib.util.module_from_spec(spec)
    module.__package__ = module_name
    module.__path__ = [str(plugin_dir)]
    sys.modules[module_name] = module
    spec.loader.exec_module(module)

    register_fn = getattr(module, "register", None)
    if register_fn is None:
        raise RuntimeError(f"Plugin '{plugin_name}' has no register(ctx) function")
    register_fn(PluginContext())


def _tool_list():
    return [
        {
            "name": entry["name"],
            "description": entry["description"],
            "inputSchema": entry["schema"].get("parameters")
            or {"type": "object", "additionalProperties": True},
        }
        for entry in tools.values()
    ]


def _call_tool(params):
    name = params.get("name")
    entry = tools.get(name)
    if entry is None:
        raise KeyError(f"Unknown tool: {name}")
    handler = entry.get("handler")
    if handler is None:
        raise RuntimeError(f"Tool '{name}' has no handler")
    arguments = params.get("arguments") or {}
    result = handler(
        arguments,
        task_id=params.get("task_id"),
        session_id=params.get("session_id"),
        platform=params.get("platform"),
    )
    if result is None:
        text = ""
    elif isinstance(result, str):
        text = result
    else:
        text = json.dumps(result)
    return {"content": [{"type": "text", "text": text}]}


def _run_hook(params):
    hook_name = params.get("hook_name")
    kwargs = params.get("kwargs") or {}
    results = []
    for callback in hooks.get(hook_name, []):
        try:
            value = callback(**kwargs)
            if value is not None:
                results.append(value)
        except Exception as exc:
            results.append({"error": str(exc)})
    return {"results": results}


def main():
    _load_plugin()
    while True:
        try:
            request = _read_message()
        except EOFError:
            return
        method = request.get("method")
        request_id = request.get("id")
        if method == "initialize":
            _write_message({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {"serverInfo": {"name": plugin_name}},
            })
        elif method == "notifications/initialized":
            continue
        elif method == "tools/list":
            _write_message({"jsonrpc": "2.0", "id": request_id, "result": {"tools": _tool_list()}})
        elif method == "tools/call":
            try:
                result = _call_tool(request.get("params") or {})
                _write_message({"jsonrpc": "2.0", "id": request_id, "result": result})
            except Exception as exc:
                _write_message({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32010, "message": str(exc)}})
        elif method == "hooks/run":
            try:
                result = _run_hook(request.get("params") or {})
                _write_message({"jsonrpc": "2.0", "id": request_id, "result": result})
            except Exception as exc:
                _write_message({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32011, "message": str(exc)}})
        elif method == "shutdown":
            _write_message({"jsonrpc": "2.0", "id": request_id, "result": {}})
            return
        else:
            _write_message({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": f"Unknown method: {method}"}})


if __name__ == "__main__":
    try:
        main()
    except EOFError:
        pass
    except Exception:
        traceback.print_exc(file=sys.stderr)
        raise
"#;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HermesPluginManifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub provides_tools: Vec<String>,
    #[serde(default)]
    pub provides_hooks: Vec<String>,
    #[serde(default)]
    pub requires_env: Vec<HermesEnvRequirement>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum HermesEnvRequirement {
    Name(String),
    Detailed(HermesEnvRequirementDetails),
}

#[derive(Debug, Clone, Deserialize)]
pub struct HermesEnvRequirementDetails {
    pub name: String,
}

pub fn looks_like_hermes_plugin(path: &Path) -> bool {
    path.join("__init__.py").is_file()
        && (path.join("plugin.yaml").is_file() || path.join("plugin.yml").is_file())
}

pub fn parse_hermes_manifest(path: &Path) -> Result<HermesPluginManifest, PluginError> {
    let manifest_path = hermes_manifest_path(path).ok_or_else(|| PluginError::MissingManifest {
        path: path.join("plugin.yaml"),
    })?;
    let content = std::fs::read_to_string(&manifest_path)?;
    let mut manifest: HermesPluginManifest =
        serde_yml::from_str(&content).map_err(|error| PluginError::InvalidManifest {
            path: manifest_path.clone(),
            message: error.to_string(),
        })?;
    if manifest.name.trim().is_empty() {
        manifest.name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("hermes-plugin")
            .to_string();
    }
    Ok(manifest)
}

pub fn hermes_manifest_path(path: &Path) -> Option<PathBuf> {
    let yaml = path.join("plugin.yaml");
    if yaml.is_file() {
        return Some(yaml);
    }
    let yml = path.join("plugin.yml");
    if yml.is_file() {
        return Some(yml);
    }
    None
}

pub fn synthesize_manifest(path: &Path, manifest: &HermesPluginManifest) -> PluginManifest {
    PluginManifest {
        plugin: PluginMetadata {
            name: manifest.name.clone(),
            version: if manifest.version.trim().is_empty() {
                "0.1.0".into()
            } else {
                manifest.version.clone()
            },
            description: if manifest.description.trim().is_empty() {
                format!("Hermes-compatible plugin '{}'", manifest.name)
            } else {
                manifest.description.clone()
            },
            kind: PluginKind::Hermes,
            author: manifest.author.clone(),
            license: String::new(),
            homepage: None,
            min_edgecrab_version: None,
        },
        exec: Some(PluginExecConfig {
            command: python_command(),
            args: vec![
                "-u".into(),
                "-c".into(),
                HERMES_HOST_SCRIPT.into(),
                path.to_string_lossy().to_string(),
                manifest.name.clone(),
            ],
            cwd: Some(".".into()),
            env: HashMap::new(),
            startup_timeout_secs: 10,
            call_timeout_secs: 60,
            restart_policy: PluginRestartPolicy::Once,
            restart_max_attempts: 3,
            idle_timeout_secs: 300,
        }),
        script: None,
        tools: manifest
            .provides_tools
            .iter()
            .map(|name| PluginToolDefinition {
                name: name.clone(),
                description: format!("Hermes plugin tool: {name}"),
            })
            .collect(),
        capabilities: PluginCapabilities {
            host: vec!["host:inject_message".into()],
            ..PluginCapabilities::default()
        },
        trust: None,
        integrity: None,
    }
}

fn python_command() -> String {
    std::env::var("EDGECRAB_PLUGIN_PYTHON")
        .or_else(|_| std::env::var("PYTHON"))
        .unwrap_or_else(|_| "python3".into())
}

pub async fn invoke_hook(plugin: &DiscoveredPlugin, hook_name: &str, kwargs: Value) -> Result<Vec<Value>, PluginError> {
    let Some(manifest) = plugin.manifest.clone() else {
        return Ok(Vec::new());
    };
    let Some(exec) = manifest.exec else {
        return Ok(Vec::new());
    };
    let client = ToolServerClient::new(
        plugin.path.clone(),
        plugin.name.clone(),
        exec,
        manifest.capabilities,
    );
    let response = client
        .call_method(
            "hooks/run",
            json!({
                "hook_name": hook_name,
                "kwargs": kwargs,
            }),
            None,
        )
        .await?;
    Ok(response
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub fn extract_pre_llm_context(results: &[Value]) -> Vec<String> {
    results
        .iter()
        .filter_map(|value| {
            if let Some(text) = value.as_str() {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            } else {
                value.get("context")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)
            }
        })
        .collect()
}

pub fn supports_hook(plugin: &DiscoveredPlugin, hook_name: &str) -> bool {
    plugin
        .hooks
        .iter()
        .any(|candidate| candidate == hook_name)
        && plugin.kind == PluginKind::Hermes
        && plugin.enabled
        && matches!(plugin.status, PluginStatus::Available | PluginStatus::Disabled | PluginStatus::SetupNeeded | PluginStatus::Unsupported)
}

pub fn missing_required_env(manifest: &HermesPluginManifest) -> Vec<String> {
    manifest
        .requires_env
        .iter()
        .map(|requirement| match requirement {
            HermesEnvRequirement::Name(name) => name.as_str(),
            HermesEnvRequirement::Detailed(details) => details.name.as_str(),
        })
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .filter(|name| std::env::var(name).ok().filter(|value| !value.is_empty()).is_none())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::types::{SkillSource, TrustLevel};
    use tempfile::TempDir;

    fn write_plugin(dir: &Path) {
        std::fs::write(
            dir.join("plugin.yaml"),
            r#"
name: hermes-demo
version: "1.0.0"
description: Demo Hermes plugin
provides_tools:
  - hello_world
provides_hooks:
  - pre_llm_call
"#,
        )
        .expect("write manifest");
        std::fs::write(
            dir.join("__init__.py"),
            r#"
def register(ctx):
    ctx.register_hook("pre_llm_call", lambda **kwargs: {"context": "Remember this context"})
"#,
        )
        .expect("write plugin");
    }

    fn discovered_plugin(dir: &Path) -> DiscoveredPlugin {
        let manifest = parse_hermes_manifest(dir).expect("manifest");
        DiscoveredPlugin {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            kind: PluginKind::Hermes,
            status: PluginStatus::Available,
            path: dir.to_path_buf(),
            manifest: Some(synthesize_manifest(dir, &manifest)),
            skill: None,
            tools: manifest.provides_tools,
            hooks: manifest.provides_hooks,
            trust_level: TrustLevel::Unverified,
            enabled: true,
            source: SkillSource::User,
            missing_env: Vec::new(),
        }
    }

    #[test]
    fn detects_hermes_plugin_directory() {
        let temp = TempDir::new().expect("tempdir");
        write_plugin(temp.path());

        assert!(looks_like_hermes_plugin(temp.path()));
        let manifest = parse_hermes_manifest(temp.path()).expect("manifest");
        assert_eq!(manifest.name, "hermes-demo");
    }

    #[test]
    fn collects_missing_required_env_from_both_manifest_forms() {
        let manifest: HermesPluginManifest = serde_yml::from_str(
            r#"
name: hermes-demo
requires_env:
  - SIMPLE_TOKEN
  - name: DETAILED_TOKEN
    description: Detailed token
"#,
        )
        .expect("manifest");

        let missing = missing_required_env(&manifest);
        assert!(missing.contains(&"SIMPLE_TOKEN".to_string()));
        assert!(missing.contains(&"DETAILED_TOKEN".to_string()));
    }

    #[tokio::test]
    async fn invokes_pre_llm_hook_via_python_bridge() {
        let temp = TempDir::new().expect("tempdir");
        write_plugin(temp.path());

        let plugin = discovered_plugin(temp.path());
        let results = invoke_hook(
            &plugin,
            "pre_llm_call",
            json!({
                "session_id": "s1",
                "user_message": "hello",
                "conversation_history": [],
                "is_first_turn": true,
                "model": "test-model",
                "platform": "cli",
            }),
        )
        .await
        .expect("hook invocation");

        assert_eq!(extract_pre_llm_context(&results), vec!["Remember this context"]);
    }
}
