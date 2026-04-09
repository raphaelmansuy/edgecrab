use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::discovery::DiscoveredPlugin;
use crate::error::PluginError;
use crate::manifest::{
    PluginCapabilities, PluginExecConfig, PluginManifest, PluginMetadata, PluginRestartPolicy,
    PluginToolDefinition,
};
use crate::tool_server::client::ToolServerClient;
use crate::types::{PluginKind, PluginStatus};

const HERMES_HOST_SCRIPT: &str = r#"
import abc
import argparse
import importlib.util
import importlib.metadata
import io
import json
import os
import sys
import traceback
import types
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

source_kind = sys.argv[1]
source_value = sys.argv[2]
plugin_name = sys.argv[3]
plugin_dir = Path(source_value).resolve() if source_kind == "directory" else None
tools = {}
hooks = {}
cli_commands = {}
memory_provider = None
memory_provider_tool_names = set()
memory_provider_initialized = False
_next_request_id = 1000


def _ensure_package(name, paths=None):
    module = sys.modules.get(name)
    if module is None:
        module = types.ModuleType(name)
        module.__package__ = name
        module.__path__ = []
        sys.modules[name] = module
    if paths:
        existing = list(getattr(module, "__path__", []))
        for path in paths:
            if path not in existing:
                existing.append(path)
        module.__path__ = existing
    return module


def _find_plugins_root():
    if plugin_dir is None:
        return None
    current = plugin_dir
    while True:
        if current.name == "plugins":
            return current
        parent = current.parent
        if parent == current:
            return None
        current = parent


def _canonical_module_name():
    root = _find_plugins_root()
    if root is None:
        return None
    try:
        rel = plugin_dir.relative_to(root)
    except ValueError:
        return None
    if not rel.parts:
        return None
    return ".".join(("plugins",) + rel.parts)


def _display_hermes_home():
    home = os.environ.get("HERMES_HOME", "")
    if not home:
        return "~/.edgecrab"
    real_home = os.path.expanduser("~")
    if home.startswith(real_home):
        return "~" + home[len(real_home):]
    return home


def _install_runtime_shims():
    _ensure_package("agent")
    agent_memory_provider = types.ModuleType("agent.memory_provider")

    class MemoryProvider(metaclass=abc.ABCMeta):
        @property
        def name(self):
            return self.__class__.__name__.lower()

        def is_available(self):
            return True

        def initialize(self, session_id, **kwargs):
            return None

        def get_tool_schemas(self):
            return []

        def handle_tool_call(self, tool_name, args, **kwargs):
            raise NotImplementedError

        def prefetch(self, query, *, session_id=""):
            return ""

        def on_session_end(self, messages):
            return None

        def shutdown(self):
            return None

    agent_memory_provider.MemoryProvider = MemoryProvider
    sys.modules["agent.memory_provider"] = agent_memory_provider

    tools_pkg = _ensure_package("tools")
    tools_registry = types.ModuleType("tools.registry")

    def tool_error(message):
        return json.dumps({"error": str(message)})

    tools_registry.tool_error = tool_error
    sys.modules["tools.registry"] = tools_registry
    tools_pkg.registry = tools_registry

    hermes_constants = types.ModuleType("hermes_constants")

    def get_hermes_home():
        default_home = str(plugin_dir.parent) if plugin_dir is not None else "."
        return Path(os.environ.get("HERMES_HOME", default_home)).expanduser()

    def display_hermes_home():
        return _display_hermes_home()

    hermes_constants.get_hermes_home = get_hermes_home
    hermes_constants.display_hermes_home = display_hermes_home
    sys.modules["hermes_constants"] = hermes_constants

    canonical = _canonical_module_name()
    root = _find_plugins_root()
    if root is not None:
        repo_root = str(root.parent)
        if repo_root not in sys.path:
            sys.path.insert(0, repo_root)
        _ensure_package("plugins", [str(root)])
        if plugin_dir.parent != root:
            _ensure_package("plugins." + plugin_dir.parent.name, [str(plugin_dir.parent)])
    return canonical


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
    def register_tool(self, name, *args, **kwargs):
        toolset = kwargs.get("toolset")
        schema = kwargs.get("schema")
        handler = kwargs.get("handler")
        check_fn = kwargs.get("check_fn")
        requires_env = kwargs.get("requires_env")
        description = kwargs.get("description", "")

        # Hermes docs show two accepted forms:
        #   ctx.register_tool("name", schema, handler)
        #   ctx.register_tool(name="x", toolset="y", schema=..., handler=...)
        if args:
            if len(args) >= 1 and schema is None and isinstance(args[0], dict):
                schema = args[0]
            elif len(args) >= 1 and toolset is None:
                toolset = args[0]
            if len(args) >= 2 and handler is None and callable(args[1]):
                handler = args[1]
            elif len(args) >= 2 and schema is None and isinstance(args[1], dict):
                schema = args[1]
            if len(args) >= 3 and handler is None and callable(args[2]):
                handler = args[2]

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
            "check_fn": check_fn,
            "requires_env": requires_env or [],
        }

    def register_hook(self, hook_name, callback):
        hooks.setdefault(hook_name, []).append(callback)

    def register_memory_provider(self, provider):
        global memory_provider, memory_provider_tool_names
        memory_provider = provider
        try:
            schemas = provider.get_tool_schemas() or []
        except Exception:
            schemas = []
        memory_provider_tool_names = {
            schema.get("name")
            for schema in schemas
            if isinstance(schema, dict) and schema.get("name")
        }

    def inject_message(self, content, role="user"):
        result = _host_call("host:inject_message", {"content": content, "role": role})
        return bool(result.get("ok"))

    def register_cli_command(self, *args, **kwargs):
        name = kwargs.get("name")
        help_text = kwargs.get("help")
        setup_fn = kwargs.get("setup_fn")
        handler_fn = kwargs.get("handler_fn")
        description = kwargs.get("description", "")

        if args:
            if len(args) >= 1 and name is None:
                name = args[0]
            if len(args) >= 2 and help_text is None:
                help_text = args[1]
            if len(args) >= 3 and setup_fn is None and callable(args[2]):
                setup_fn = args[2]
            if len(args) >= 4 and handler_fn is None and callable(args[3]):
                handler_fn = args[3]

        if not name or not callable(setup_fn):
            return None

        cli_commands[name] = {
            "name": name,
            "help": help_text or "",
            "description": description or "",
            "setup_fn": setup_fn,
            "handler_fn": handler_fn,
        }
        return None


def _entry_points_for_group(group_name):
    eps = importlib.metadata.entry_points()
    if hasattr(eps, "select"):
        return list(eps.select(group=group_name))
    if isinstance(eps, dict):
        return list(eps.get(group_name, []))
    return [ep for ep in eps if getattr(ep, "group", None) == group_name]


def _load_entrypoint_module():
    for ep in _entry_points_for_group("hermes_agent.plugins"):
        if ep.name == plugin_name:
            return ep.load()
    raise ImportError(f"Entry point '{plugin_name}' not found in group 'hermes_agent.plugins'")


def _load_plugin():
    _install_runtime_shims()

    if source_kind == "entrypoint":
        module = _load_entrypoint_module()
    else:
        if plugin_dir is None:
            raise RuntimeError("directory plugin missing path")
        init_file = plugin_dir / "__init__.py"
        if not init_file.exists():
            raise FileNotFoundError(f"No __init__.py in {plugin_dir}")

        canonical_name = _canonical_module_name()
        module_name = canonical_name or f"edgecrab_hermes_plugins.{plugin_name.replace('-', '_')}"
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
    _register_memory_provider_package_alias(module)
    _load_conventional_cli_module()


def _memory_provider_module_base():
    if plugin_dir is None or memory_provider is None:
        return None
    return f"plugins.memory.{plugin_name.replace('-', '_')}"


def _register_memory_provider_package_alias(module):
    alias_base = _memory_provider_module_base()
    if alias_base is None or plugin_dir is None:
        return
    _ensure_package("plugins", [str(plugin_dir.parent)])
    _ensure_package("plugins.memory", [str(plugin_dir.parent)])
    module.__package__ = alias_base
    module.__path__ = [str(plugin_dir)]
    sys.modules[alias_base] = module


def _load_conventional_cli_module():
    if plugin_dir is None:
        return
    cli_file = plugin_dir / "cli.py"
    if not cli_file.exists():
        return

    alias_base = _memory_provider_module_base()
    if alias_base is not None:
        _ensure_package(alias_base, [str(plugin_dir)])
        module_name = f"{alias_base}.cli"
    else:
        canonical_name = _canonical_module_name()
        module_name = (
            f"{canonical_name}.cli"
            if canonical_name
            else f"edgecrab_hermes_plugins.{plugin_name.replace('-', '_')}.cli"
        )

    module = sys.modules.get(module_name)
    if module is None:
        spec = importlib.util.spec_from_file_location(module_name, cli_file)
        if spec is None or spec.loader is None:
            raise ImportError(f"Cannot create module spec for {cli_file}")
        module = importlib.util.module_from_spec(spec)
        parent_name = module_name.rsplit(".", 1)[0]
        module.__package__ = parent_name
        sys.modules[module_name] = module
        spec.loader.exec_module(module)

    register_cli = getattr(module, "register_cli", None)
    if not callable(register_cli):
        return

    command_name = getattr(memory_provider, "name", None) or plugin_name
    if command_name in cli_commands:
        return

    cli_commands[command_name] = {
        "name": command_name,
        "help": f"Manage {command_name}",
        "description": "",
        "setup_fn": register_cli,
        "handler_fn": None,
    }


def _tool_list():
    tool_entries = [
        {
            "name": entry["name"],
            "description": entry["description"],
            "inputSchema": entry["schema"].get("parameters")
            or {"type": "object", "additionalProperties": True},
        }
        for entry in tools.values()
        if _tool_is_available(entry)
    ]
    if memory_provider is not None:
        try:
            for schema in memory_provider.get_tool_schemas() or []:
                if not isinstance(schema, dict):
                    continue
                tool_entries.append({
                    "name": schema.get("name", ""),
                    "description": schema.get("description", ""),
                    "inputSchema": schema.get("parameters") or {"type": "object", "additionalProperties": True},
                })
        except Exception:
            pass
    return tool_entries


def _missing_requires_env(requirements):
    missing = []
    for requirement in requirements or []:
        if isinstance(requirement, str):
            name = requirement.strip()
        elif isinstance(requirement, dict):
            name = str(requirement.get("name") or "").strip()
        else:
            name = ""
        if name and not str(os.environ.get(name, "")).strip():
            missing.append(name)
    return missing


def _tool_is_available(entry):
    if _missing_requires_env(entry.get("requires_env")):
        return False
    check_fn = entry.get("check_fn")
    if check_fn is None:
        return True
    try:
        return bool(check_fn())
    except Exception:
        return False


def _provider_kwargs(params):
    kwargs = {}
    for key in ("platform", "agent_context", "user_id", "parent_session_id", "agent_identity", "agent_workspace"):
        value = params.get(key)
        if value is not None:
            kwargs[key] = value
    kwargs["hermes_home"] = os.environ.get("HERMES_HOME", "")
    return kwargs


def _ensure_memory_provider_initialized(params):
    global memory_provider_initialized
    if memory_provider is None or memory_provider_initialized:
        return
    session_id = params.get("session_id") or ""
    memory_provider.initialize(session_id, **_provider_kwargs(params))
    memory_provider_initialized = True


def _call_tool(params):
    name = params.get("name")
    entry = tools.get(name)
    if entry is None:
        if memory_provider is not None and name in memory_provider_tool_names:
            _ensure_memory_provider_initialized(params)
            arguments = params.get("arguments") or {}
            result = memory_provider.handle_tool_call(
                name,
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
        raise KeyError(f"Unknown tool: {name}")
    handler = entry.get("handler")
    if not _tool_is_available(entry):
        raise RuntimeError(f"Tool '{name}' is not currently available")
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
    if memory_provider is not None:
        try:
            if hook_name == "on_session_start":
                init_params = dict(kwargs)
                init_params["session_id"] = kwargs.get("session_id", "")
                _ensure_memory_provider_initialized(init_params)
            elif hook_name == "pre_llm_call":
                _ensure_memory_provider_initialized(kwargs)
                value = memory_provider.prefetch(
                    kwargs.get("user_message", ""),
                    session_id=kwargs.get("session_id", ""),
                )
                if value:
                    results.append({"context": value} if isinstance(value, str) else value)
            elif hook_name == "on_session_end" and hasattr(memory_provider, "on_session_end"):
                history = kwargs.get("messages") or kwargs.get("conversation_history") or []
                memory_provider.on_session_end(history)
            elif hook_name in ("on_session_finalize", "on_session_reset"):
                if hasattr(memory_provider, "shutdown"):
                    memory_provider.shutdown()
        except Exception as exc:
            results.append({"error": str(exc)})
    for callback in hooks.get(hook_name, []):
        try:
            value = callback(**kwargs)
            if value is not None:
                results.append(value)
        except Exception as exc:
            results.append({"error": str(exc)})
    return {"results": results}


def _introspect():
    hook_names = sorted(hooks.keys())
    if memory_provider is not None:
        for hook_name in ("on_session_start", "pre_llm_call", "on_session_end", "on_session_finalize", "on_session_reset"):
            if hook_name not in hook_names:
                hook_names.append(hook_name)
    return {
        "tools": _tool_list(),
        "hooks": sorted(hook_names),
        "cli_commands": [
            {
                "name": entry["name"],
                "help": entry["help"],
                "description": entry["description"],
            }
            for entry in sorted(cli_commands.values(), key=lambda item: item["name"])
        ],
        "memory_provider": getattr(memory_provider, "name", None),
    }


def _invoke_cli(params):
    command_name = params.get("command_name")
    argv = params.get("argv") or []
    entry = cli_commands.get(command_name)
    if entry is None:
        raise KeyError(f"Unknown CLI command: {command_name}")

    parser = argparse.ArgumentParser(
        prog=f"edgecrab {command_name}",
        description=entry.get("description") or entry.get("help") or None,
    )
    setup_fn = entry.get("setup_fn")
    if callable(setup_fn):
        setup_fn(parser)

    stdout = io.StringIO()
    stderr = io.StringIO()
    exit_code = 0

    with redirect_stdout(stdout), redirect_stderr(stderr):
        try:
            namespace = parser.parse_args(list(argv))
            handler = entry.get("handler_fn") or getattr(namespace, "func", None)
            if callable(handler):
                result = handler(namespace)
                if isinstance(result, int):
                    exit_code = result
            elif not hasattr(namespace, "func"):
                parser.print_help()
        except SystemExit as exc:
            code = exc.code if isinstance(exc.code, int) else 1
            exit_code = code

    return {
        "exit_code": exit_code,
        "stdout": stdout.getvalue(),
        "stderr": stderr.getvalue(),
    }


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
        elif method == "edgecrab/introspect":
            try:
                _write_message({"jsonrpc": "2.0", "id": request_id, "result": _introspect()})
            except Exception as exc:
                _write_message({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32012, "message": str(exc)}})
        elif method == "edgecrab/cli_invoke":
            try:
                _write_message({"jsonrpc": "2.0", "id": request_id, "result": _invoke_cli(request.get("params") or {})})
            except Exception as exc:
                _write_message({"jsonrpc": "2.0", "id": request_id, "error": {"code": -32013, "message": str(exc)}})
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
    #[serde(default, alias = "tools")]
    pub tools: Vec<String>,
    #[serde(default)]
    pub provides_hooks: Vec<String>,
    #[serde(default, alias = "hooks")]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub requires_env: Vec<HermesEnvRequirement>,
}

#[derive(Debug, Clone, Default)]
pub struct HermesCliCommand {
    pub name: String,
    pub help: String,
    pub description: String,
}

#[derive(Debug, Clone, Default)]
pub struct HermesEntrypointPlugin {
    pub name: String,
    pub value: String,
    pub module_path: Option<PathBuf>,
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
    let mut env = HashMap::new();
    let hermes_home = std::env::var("EDGECRAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".edgecrab")
        });
    env.insert("HERMES_HOME".into(), hermes_home.display().to_string());

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
                "directory".into(),
                path.to_string_lossy().to_string(),
                manifest.name.clone(),
            ],
            cwd: Some(".".into()),
            env,
            startup_timeout_secs: 10,
            call_timeout_secs: 60,
            restart_policy: PluginRestartPolicy::Once,
            restart_max_attempts: 3,
            idle_timeout_secs: 300,
        }),
        script: None,
        tools: hermes_declared_tools(manifest)
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

pub fn synthesize_entrypoint_manifest(
    entrypoint: &HermesEntrypointPlugin,
    home_override: Option<&Path>,
) -> PluginManifest {
    let mut env = HashMap::new();
    let hermes_home = home_override
        .map(Path::to_path_buf)
        .or_else(|| std::env::var("EDGECRAB_HOME").ok().map(PathBuf::from))
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".edgecrab")
        });
    env.insert("HERMES_HOME".into(), hermes_home.display().to_string());

    PluginManifest {
        plugin: PluginMetadata {
            name: entrypoint.name.clone(),
            version: "0.1.0".into(),
            description: format!("Hermes entry-point plugin '{}'", entrypoint.name),
            kind: PluginKind::Hermes,
            author: String::new(),
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
                "entrypoint".into(),
                entrypoint.value.clone(),
                entrypoint.name.clone(),
            ],
            cwd: entrypoint
                .module_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            env,
            startup_timeout_secs: 10,
            call_timeout_secs: 60,
            restart_policy: PluginRestartPolicy::Once,
            restart_max_attempts: 3,
            idle_timeout_secs: 300,
        }),
        script: None,
        tools: Vec::new(),
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

pub fn hermes_declared_tools(manifest: &HermesPluginManifest) -> Vec<String> {
    let mut names = manifest.provides_tools.clone();
    names.extend(manifest.tools.clone());
    names.sort();
    names.dedup();
    names
}

pub fn hermes_declared_hooks(manifest: &HermesPluginManifest) -> Vec<String> {
    let mut names = manifest.provides_hooks.clone();
    names.extend(manifest.hooks.clone());
    names.sort();
    names.dedup();
    names
}

#[derive(Debug, Clone, Default)]
pub struct HermesRuntimeSurface {
    pub tools: Vec<PluginToolDefinition>,
    pub hooks: Vec<String>,
    pub cli_commands: Vec<HermesCliCommand>,
    pub memory_provider: Option<String>,
}

pub fn introspect_runtime_surface(
    path: &Path,
    manifest: &HermesPluginManifest,
) -> HermesRuntimeSurface {
    let synthesized = synthesize_manifest(path, manifest);
    introspect_runtime_surface_from_manifest(
        path,
        &manifest.name,
        synthesized.exec.clone(),
        synthesized.capabilities,
    )
}

pub fn introspect_runtime_surface_for_entrypoint(
    entrypoint: &HermesEntrypointPlugin,
) -> HermesRuntimeSurface {
    let synthesized = synthesize_entrypoint_manifest(entrypoint, None);
    introspect_runtime_surface_from_manifest(
        entrypoint
            .module_path
            .as_deref()
            .unwrap_or_else(|| Path::new(".")),
        &entrypoint.name,
        synthesized.exec.clone(),
        synthesized.capabilities,
    )
}

fn introspect_runtime_surface_from_manifest(
    path: &Path,
    plugin_name: &str,
    exec: Option<PluginExecConfig>,
    capabilities: PluginCapabilities,
) -> HermesRuntimeSurface {
    let Some(exec) = exec else {
        return HermesRuntimeSurface::default();
    };

    let future = async {
        let client = ToolServerClient::new(
            path.to_path_buf(),
            plugin_name.to_string(),
            exec,
            capabilities,
        );
        let result = client
            .call_method("edgecrab/introspect", json!({}), None)
            .await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tool| {
                Some(PluginToolDefinition {
                    name: tool.get("name")?.as_str()?.to_string(),
                    description: tool
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect::<Vec<_>>();
        let hooks = result
            .get("hooks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(ToString::to_string))
            .collect::<Vec<_>>();
        let cli_commands = result
            .get("cli_commands")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| {
                Some(HermesCliCommand {
                    name: entry.get("name")?.as_str()?.to_string(),
                    help: entry
                        .get("help")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    description: entry
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect::<Vec<_>>();
        let memory_provider = result
            .get("memory_provider")
            .and_then(Value::as_str)
            .map(ToString::to_string);

        let _ = client.shutdown().await;
        Ok::<HermesRuntimeSurface, PluginError>(HermesRuntimeSurface {
            tools,
            hooks,
            cli_commands,
            memory_provider,
        })
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future)).unwrap_or_default()
    } else {
        tokio::runtime::Runtime::new()
            .ok()
            .and_then(|runtime| runtime.block_on(future).ok())
            .unwrap_or_default()
    }
}

pub fn discover_entrypoint_plugins() -> Result<Vec<HermesEntrypointPlugin>, PluginError> {
    const ENTRYPOINT_SCAN_SCRIPT: &str = r#"
import importlib.metadata
import json

def _entry_points_for_group(group_name):
    eps = importlib.metadata.entry_points()
    if hasattr(eps, "select"):
        return list(eps.select(group=group_name))
    if isinstance(eps, dict):
        return list(eps.get(group_name, []))
    return [ep for ep in eps if getattr(ep, "group", None) == group_name]

items = []
for ep in _entry_points_for_group("hermes_agent.plugins"):
    module_path = None
    try:
        module = ep.load()
        module_path = getattr(module, "__file__", None)
    except Exception:
        module_path = None
    items.append({
        "name": ep.name,
        "value": ep.value,
        "module_path": module_path,
    })

print(json.dumps(items))
"#;

    let output = std::process::Command::new(python_command())
        .args(["-c", ENTRYPOINT_SCAN_SCRIPT])
        .output()?;
    if !output.status.success() {
        return Err(PluginError::Process(format!(
            "failed to scan Hermes entry points: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let parsed: Vec<Value> = serde_json::from_slice(&output.stdout)?;
    Ok(parsed
        .into_iter()
        .filter_map(|entry| {
            Some(HermesEntrypointPlugin {
                name: entry.get("name")?.as_str()?.to_string(),
                value: entry.get("value")?.as_str()?.to_string(),
                module_path: entry
                    .get("module_path")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .and_then(|path| path.parent().map(Path::to_path_buf)),
            })
        })
        .collect())
}

pub async fn invoke_hook(
    plugin: &DiscoveredPlugin,
    hook_name: &str,
    kwargs: Value,
) -> Result<Vec<Value>, PluginError> {
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

pub async fn invoke_cli_command(
    plugin: &DiscoveredPlugin,
    command_name: &str,
    argv: &[String],
) -> Result<(i32, String, String), PluginError> {
    let Some(manifest) = plugin.manifest.clone() else {
        return Ok((0, String::new(), String::new()));
    };
    let Some(exec) = manifest.exec else {
        return Ok((0, String::new(), String::new()));
    };
    let client = ToolServerClient::new(
        plugin.path.clone(),
        plugin.name.clone(),
        exec,
        manifest.capabilities,
    );
    let response = client
        .call_method(
            "edgecrab/cli_invoke",
            json!({
                "command_name": command_name,
                "argv": argv,
            }),
            None,
        )
        .await?;
    let exit_code = response
        .get("exit_code")
        .and_then(Value::as_i64)
        .unwrap_or_default() as i32;
    let stdout = response
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let stderr = response
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((exit_code, stdout, stderr))
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
                value
                    .get("context")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)
            }
        })
        .collect()
}

pub fn supports_hook(plugin: &DiscoveredPlugin, hook_name: &str) -> bool {
    plugin.hooks.iter().any(|candidate| candidate == hook_name)
        && plugin.kind == PluginKind::Hermes
        && plugin.enabled
        && matches!(
            plugin.status,
            PluginStatus::Available
                | PluginStatus::Disabled
                | PluginStatus::SetupNeeded
                | PluginStatus::Unsupported
        )
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
        .filter(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.is_empty())
                .is_none()
        })
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::OnceLock;

    use super::*;

    use crate::types::{SkillSource, TrustLevel};
    use tempfile::TempDir;

    const REAL_HERMES_REPO_URL: &str = "https://github.com/NousResearch/hermes-agent";
    const REAL_HERMES_REF: &str = "268ee6bdce013c74c9a8dfbb13fd850423189322";

    fn real_hermes_repo() -> &'static PathBuf {
        static REPO: OnceLock<PathBuf> = OnceLock::new();
        REPO.get_or_init(|| {
            let path = std::env::temp_dir()
                .join(format!("edgecrab-hermes-agent-plugins-{REAL_HERMES_REF}"));
            if path.exists() {
                return path;
            }
            let status = Command::new("git")
                .args([
                    "clone",
                    "--depth",
                    "1",
                    REAL_HERMES_REPO_URL,
                    path.to_str().expect("utf8 path"),
                ])
                .status()
                .expect("git clone hermes-agent");
            if !status.success() {
                assert!(
                    path.join("plugins").is_dir(),
                    "failed to clone hermes-agent fixtures"
                );
                return path;
            }

            let status = Command::new("git")
                .args([
                    "-C",
                    path.to_str().expect("utf8 path"),
                    "checkout",
                    REAL_HERMES_REF,
                ])
                .status()
                .expect("git checkout hermes-agent ref");
            assert!(status.success(), "failed to checkout hermes-agent ref");
            path
        })
    }

    fn real_plugin_with_home(
        plugin_dir: &Path,
        home: &Path,
    ) -> (HermesPluginManifest, PluginManifest, DiscoveredPlugin) {
        let manifest = parse_hermes_manifest(plugin_dir).expect("real plugin manifest");
        let mut synthesized = synthesize_manifest(plugin_dir, &manifest);
        synthesized
            .exec
            .as_mut()
            .expect("exec config")
            .env
            .insert("HERMES_HOME".into(), home.display().to_string());

        let surface = introspect_runtime_surface(plugin_dir, &manifest);
        let plugin = DiscoveredPlugin {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            compatibility: None,
            kind: PluginKind::Hermes,
            status: PluginStatus::Available,
            path: plugin_dir.to_path_buf(),
            manifest: Some(synthesized.clone()),
            skill: None,
            tools: surface.tools.iter().map(|tool| tool.name.clone()).collect(),
            hooks: surface.hooks.clone(),
            trust_level: TrustLevel::Unverified,
            enabled: true,
            source: SkillSource::User,
            install_source: None,
            missing_env: Vec::new(),
            related_skills: Vec::new(),
            cli_commands: surface.cli_commands.clone(),
        };

        (manifest, synthesized, plugin)
    }

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

    fn write_memory_provider_plugin(dir: &Path) {
        std::fs::write(
            dir.join("plugin.yaml"),
            r#"
name: sqlite-memory
version: "1.0.0"
description: SQLite memory provider
hooks:
  - on_session_end
"#,
        )
        .expect("write manifest");
        std::fs::write(
            dir.join("__init__.py"),
            r###"
import json
import sqlite3

class SQLiteMemoryProvider:
    def __init__(self):
        self._conn = None
        self._session_id = ""

    @property
    def name(self):
        return "sqlite_memory"

    def initialize(self, session_id, **kwargs):
        self._session_id = session_id
        self._conn = sqlite3.connect(":memory:")
        self._conn.execute("CREATE VIRTUAL TABLE IF NOT EXISTS memories USING fts5(content)")

    def get_tool_schemas(self):
        return [
            {
                "name": "sqlite_retain",
                "description": "Store a fact.",
                "parameters": {
                    "type": "object",
                    "properties": {"content": {"type": "string"}},
                    "required": ["content"],
                },
            },
            {
                "name": "sqlite_recall",
                "description": "Recall a fact.",
                "parameters": {
                    "type": "object",
                    "properties": {"query": {"type": "string"}},
                    "required": ["query"],
                },
            },
        ]

    def handle_tool_call(self, tool_name, args, **kwargs):
        if tool_name == "sqlite_retain":
            self._conn.execute("INSERT INTO memories (content) VALUES (?)", (args.get("content", ""),))
            self._conn.commit()
            return json.dumps({"result": "stored"})
        if tool_name == "sqlite_recall":
            rows = self._conn.execute(
                "SELECT content FROM memories WHERE memories MATCH ? LIMIT 5",
                (args.get("query", ""),),
            ).fetchall()
            return json.dumps({"results": [row[0] for row in rows]})
        return json.dumps({"error": "unknown tool"})

    def prefetch(self, query, *, session_id=""):
        rows = self._conn.execute(
            "SELECT content FROM memories WHERE memories MATCH ? LIMIT 5",
            (query,),
        ).fetchall()
        if not rows:
            return ""
        return "## SQLite Memory\n" + "\n".join(row[0] for row in rows)

    def shutdown(self):
        if self._conn is not None:
            self._conn.close()
            self._conn = None

def register(ctx):
    ctx.register_memory_provider(SQLiteMemoryProvider())
"###,
        )
        .expect("write plugin");
    }

    fn write_memory_provider_plugin_with_cli(dir: &Path) {
        write_memory_provider_plugin(dir);
        std::fs::write(
            dir.join("cli.py"),
            r#"
def register_cli(subparser):
    subs = subparser.add_subparsers(dest="sqlite_command")
    subs.add_parser("status", help="Show sqlite memory status")
"#,
        )
        .expect("write cli");
    }

    fn discovered_plugin(dir: &Path) -> DiscoveredPlugin {
        let manifest = parse_hermes_manifest(dir).expect("manifest");
        DiscoveredPlugin {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            compatibility: None,
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
            install_source: None,
            missing_env: Vec::new(),
            related_skills: Vec::new(),
            cli_commands: Vec::new(),
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

    #[test]
    fn parses_real_hermes_hooks_alias() {
        let manifest: HermesPluginManifest = serde_yml::from_str(
            r#"
name: honcho
hooks:
  - on_session_end
"#,
        )
        .expect("manifest");

        assert_eq!(hermes_declared_hooks(&manifest), vec!["on_session_end"]);
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

        assert_eq!(
            extract_pre_llm_context(&results),
            vec!["Remember this context"]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn memory_provider_bridge_exposes_tools_and_prefetch_context() {
        let temp = TempDir::new().expect("tempdir");
        write_memory_provider_plugin(temp.path());

        let manifest = parse_hermes_manifest(temp.path()).expect("manifest");
        let surface = introspect_runtime_surface(temp.path(), &manifest);
        let tool_names = surface
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();

        assert!(tool_names.contains(&"sqlite_retain"));
        assert!(tool_names.contains(&"sqlite_recall"));
        assert!(surface.hooks.contains(&"pre_llm_call".to_string()));

        let plugin = DiscoveredPlugin {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            compatibility: None,
            kind: PluginKind::Hermes,
            status: PluginStatus::Available,
            path: temp.path().to_path_buf(),
            manifest: Some(synthesize_manifest(temp.path(), &manifest)),
            skill: None,
            tools: tool_names.iter().map(|name| (*name).to_string()).collect(),
            hooks: surface.hooks,
            trust_level: TrustLevel::Unverified,
            enabled: true,
            source: SkillSource::User,
            install_source: None,
            missing_env: Vec::new(),
            related_skills: Vec::new(),
            cli_commands: surface.cli_commands.clone(),
        };

        invoke_hook(
            &plugin,
            "on_session_start",
            json!({
                "session_id": "session-1",
                "platform": "cli",
            }),
        )
        .await
        .expect("initialization hook");

        let client = ToolServerClient::new(
            temp.path().to_path_buf(),
            plugin.name.clone(),
            synthesize_manifest(temp.path(), &manifest)
                .exec
                .expect("exec"),
            PluginCapabilities {
                host: vec!["host:inject_message".into()],
                ..PluginCapabilities::default()
            },
        );
        client
            .call_method(
                "tools/call",
                json!({
                    "name": "sqlite_retain",
                    "arguments": { "content": "User prefers rust" },
                    "session_id": "session-1",
                    "platform": "cli",
                }),
                None,
            )
            .await
            .expect("store memory");
        let recall = client
            .call_method(
                "tools/call",
                json!({
                    "name": "sqlite_recall",
                    "arguments": { "query": "rust" },
                    "session_id": "session-1",
                    "platform": "cli",
                }),
                None,
            )
            .await
            .expect("recall memory");
        assert!(recall.to_string().contains("User prefers rust"));
        client.shutdown().await.expect("shutdown");

        let results = invoke_hook(
            &plugin,
            "pre_llm_call",
            json!({
                "session_id": "session-1",
                "user_message": "rust",
                "conversation_history": [],
                "is_first_turn": false,
                "model": "test-model",
                "platform": "cli",
            }),
        )
        .await
        .expect("prefetch hook");

        let context = extract_pre_llm_context(&results).join("\n");
        if !context.is_empty() {
            assert!(context.contains("rust"));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn memory_provider_cli_py_register_cli_is_exposed_and_invocable() {
        let temp = TempDir::new().expect("tempdir");
        write_memory_provider_plugin_with_cli(temp.path());

        let manifest = parse_hermes_manifest(temp.path()).expect("manifest");
        let surface = introspect_runtime_surface(temp.path(), &manifest);
        assert_eq!(surface.memory_provider.as_deref(), Some("sqlite_memory"));
        assert!(
            surface
                .cli_commands
                .iter()
                .any(|command| command.name == "sqlite_memory")
        );

        let plugin = DiscoveredPlugin {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            compatibility: None,
            kind: PluginKind::Hermes,
            status: PluginStatus::Available,
            path: temp.path().to_path_buf(),
            manifest: Some(synthesize_manifest(temp.path(), &manifest)),
            skill: None,
            tools: surface.tools.iter().map(|tool| tool.name.clone()).collect(),
            hooks: surface.hooks,
            trust_level: TrustLevel::Unverified,
            enabled: true,
            source: SkillSource::User,
            install_source: None,
            missing_env: Vec::new(),
            related_skills: Vec::new(),
            cli_commands: surface.cli_commands.clone(),
        };

        let (exit_code, stdout, stderr) =
            invoke_cli_command(&plugin, "sqlite_memory", &["--help".into()])
                .await
                .expect("invoke cli");
        assert_eq!(exit_code, 0);
        assert!(stdout.contains("status"), "stdout:\n{stdout}");
        assert!(stderr.is_empty(), "stderr:\n{stderr}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn real_holographic_plugin_runs_end_to_end() {
        let home = TempDir::new().expect("tempdir");
        let plugin_dir = real_hermes_repo().join("plugins/memory/holographic");
        let (_manifest, synthesized, plugin) = real_plugin_with_home(&plugin_dir, home.path());

        assert!(plugin.tools.iter().any(|tool| tool == "fact_store"));
        assert!(plugin.tools.iter().any(|tool| tool == "fact_feedback"));
        assert!(plugin.hooks.iter().any(|hook| hook == "on_session_start"));
        assert!(plugin.hooks.iter().any(|hook| hook == "pre_llm_call"));
        assert!(plugin.hooks.iter().any(|hook| hook == "on_session_end"));

        invoke_hook(
            &plugin,
            "on_session_start",
            json!({
                "session_id": "real-holographic-session",
                "platform": "cli",
            }),
        )
        .await
        .expect("real holographic initialization");

        let client = ToolServerClient::new(
            plugin_dir.clone(),
            plugin.name.clone(),
            synthesized.exec.clone().expect("exec"),
            synthesized.capabilities.clone(),
        );
        client
            .call_method(
                "tools/call",
                json!({
                    "name": "fact_store",
                    "arguments": {
                        "action": "add",
                        "content": "User prefers Rust for systems work",
                        "category": "user_pref",
                    },
                    "session_id": "real-holographic-session",
                    "platform": "cli",
                }),
                None,
            )
            .await
            .expect("store fact through real holographic plugin");
        let search = client
            .call_method(
                "tools/call",
                json!({
                    "name": "fact_store",
                    "arguments": {
                        "action": "search",
                        "query": "Rust systems work",
                    },
                    "session_id": "real-holographic-session",
                    "platform": "cli",
                }),
                None,
            )
            .await
            .expect("search fact through real holographic plugin");
        assert!(
            search
                .to_string()
                .contains("User prefers Rust for systems work")
        );
        client.shutdown().await.expect("shutdown real holographic");

        let results = invoke_hook(
            &plugin,
            "pre_llm_call",
            json!({
                "session_id": "real-holographic-session",
                "user_message": "rust systems preferences",
                "conversation_history": [],
                "is_first_turn": false,
                "model": "test-model",
                "platform": "cli",
            }),
        )
        .await
        .expect("prefetch through real holographic plugin");
        let context = extract_pre_llm_context(&results).join("\n");
        if !context.is_empty() {
            assert!(context.contains("Rust"));
        }

        invoke_hook(
            &plugin,
            "on_session_end",
            json!({
                "messages": [
                    {"role": "user", "content": "Remember my Rust preference."}
                ],
                "session_id": "real-holographic-session",
                "platform": "cli",
            }),
        )
        .await
        .expect("real holographic shutdown hook");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn real_honcho_plugin_loads_with_hermes_package_shims() {
        let home = TempDir::new().expect("tempdir");
        let plugin_dir = real_hermes_repo().join("plugins/memory/honcho");
        let (_manifest, _synthesized, plugin) = real_plugin_with_home(&plugin_dir, home.path());

        assert!(plugin.tools.iter().any(|tool| tool == "honcho_profile"));
        assert!(plugin.tools.iter().any(|tool| tool == "honcho_search"));
        assert!(plugin.hooks.iter().any(|hook| hook == "on_session_start"));
        assert!(plugin.hooks.iter().any(|hook| hook == "pre_llm_call"));
        assert!(plugin.hooks.iter().any(|hook| hook == "on_session_end"));

        invoke_hook(
            &plugin,
            "on_session_start",
            json!({
                "session_id": "real-honcho-session",
                "platform": "cli",
            }),
        )
        .await
        .expect("real honcho initialization hook");

        let results = invoke_hook(
            &plugin,
            "pre_llm_call",
            json!({
                "session_id": "real-honcho-session",
                "user_message": "hello",
                "conversation_history": [],
                "is_first_turn": true,
                "model": "test-model",
                "platform": "cli",
            }),
        )
        .await
        .expect("real honcho pre_llm_call");

        assert!(extract_pre_llm_context(&results).is_empty());
        assert!(
            plugin
                .cli_commands
                .iter()
                .any(|command| command.name == "honcho")
        );

        let (exit_code, stdout, stderr) = invoke_cli_command(&plugin, "honcho", &["--help".into()])
            .await
            .expect("real honcho cli help");
        assert_eq!(exit_code, 0);
        assert!(stdout.contains("status"), "stdout:\n{stdout}");
        assert!(stdout.contains("sessions"), "stdout:\n{stdout}");
        assert!(stderr.is_empty(), "stderr:\n{stderr}");
    }
}
