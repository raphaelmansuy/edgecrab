use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use chrono::Utc;
use edgecrab_plugins::PluginKind;
use edgecrab_plugins::config::PluginsConfig;
use edgecrab_plugins::manifest::PluginManifest;
use edgecrab_plugins::tool_server::client::ToolServerClient;
use edgecrab_plugins::{
    DiscoveredPlugin, discover_plugins, extract_pre_llm_context, invoke_hermes_cli_command,
    invoke_hermes_hook,
};
use edgecrab_types::Platform;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

const REAL_HERMES_REPO_URL: &str = "https://github.com/NousResearch/hermes-agent";
const REAL_HERMES_REF: &str = "268ee6bdce013c74c9a8dfbb13fd850423189322";
const REAL_EVEY_REPO_URL: &str = "https://github.com/42-evey/hermes-plugins";
const REAL_EVEY_REF: &str = "816c99efdd3ed86e3d253632415b9482123c50cd";

fn edgecrab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgecrab"))
}

fn run_edgecrab(home: &Path, args: &[&str]) -> String {
    run_edgecrab_with_env(home, args, &[])
}

fn run_edgecrab_with_env(home: &Path, args: &[&str], envs: &[(&str, &str)]) -> String {
    let edgecrab_home = home.join(".edgecrab");
    let mut command = edgecrab();
    command
        .env("HOME", home)
        .env("EDGECRAB_HOME", &edgecrab_home);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.args(args).output().expect("run edgecrab");

    assert!(
        output.status.success(),
        "edgecrab {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn create_entrypoint_plugin_package(dir: &Path) {
    fs::create_dir_all(dir.join("entrypoint_demo")).expect("package dir");
    fs::write(
        dir.join("pyproject.toml"),
        r#"
[build-system]
requires = ["setuptools>=68"]
build-backend = "setuptools.build_meta"

[project]
name = "edgecrab-hermes-entrypoint-demo"
version = "0.1.0"

[project.entry-points."hermes_agent.plugins"]
entrypoint-demo = "entrypoint_demo"
"#,
    )
    .expect("pyproject");
    fs::write(
        dir.join("entrypoint_demo/__init__.py"),
        r#"
import argparse
import json

_SCHEMA = {
    "name": "entry_add",
    "description": "Add two integers.",
    "parameters": {
        "type": "object",
        "properties": {
            "a": {"type": "integer"},
            "b": {"type": "integer"}
        },
        "required": ["a", "b"]
    }
}

def _add(args, **kwargs):
    return json.dumps({"result": int(args.get("a", 0)) + int(args.get("b", 0))})

def _handle_cli(args):
    if getattr(args, "subcommand", None) == "status":
        print("entrypoint ok")
    else:
        print("usage: edgecrab entry-demo status")
        return 1

def _setup_cli(parser):
    subs = parser.add_subparsers(dest="subcommand")
    subs.add_parser("status", help="Show entrypoint status")
    parser.set_defaults(func=_handle_cli)

def register(ctx):
    ctx.register_tool("entry_add", _SCHEMA, _add)
    ctx.register_cli_command(
        name="entry-demo",
        help="Manage entrypoint demo",
        setup_fn=_setup_cli,
        handler_fn=_handle_cli,
    )
"#,
    )
    .expect("package module");
}

fn build_python_venv(root: &Path, package_dir: &Path) -> PathBuf {
    let venv_dir = root.join("venv");
    let status = Command::new("python3")
        .args(["-m", "venv", venv_dir.to_str().expect("utf8 venv path")])
        .status()
        .expect("create venv");
    assert!(status.success(), "failed to create venv");

    let pip = venv_dir.join("bin/pip");
    let status = Command::new(&pip)
        .args(["install", package_dir.to_str().expect("utf8 package path")])
        .status()
        .expect("pip install entrypoint package");
    assert!(status.success(), "failed to install entrypoint package");

    venv_dir.join("bin/python")
}

fn plugins_config(edgecrab_home: &Path) -> PluginsConfig {
    PluginsConfig {
        install_dir: edgecrab_home.join("plugins"),
        quarantine_dir: edgecrab_home.join("plugins").join(".quarantine"),
        ..PluginsConfig::default()
    }
}

fn discover_installed_plugin(edgecrab_home: &Path, name: &str) -> DiscoveredPlugin {
    discover_plugins(&plugins_config(edgecrab_home), Platform::Cli)
        .expect("plugin discovery")
        .plugins
        .into_iter()
        .find(|plugin| plugin.name == name)
        .expect("plugin discovered")
}

fn plugin_client(plugin: &DiscoveredPlugin) -> ToolServerClient {
    let manifest: PluginManifest = plugin.manifest.clone().expect("plugin manifest");
    ToolServerClient::new(
        plugin.path.clone(),
        plugin.name.clone(),
        manifest.exec.expect("plugin exec config"),
        manifest.capabilities,
    )
}

fn parse_tool_json(result: Value) -> Value {
    serde_json::from_str(result.as_str().expect("json string tool result")).expect("valid json")
}

fn write_guide_style_calculator_plugin(dir: &Path) {
    fs::create_dir_all(dir.join("data")).expect("data dir");
    fs::write(
        dir.join("plugin.yaml"),
        r#"
name: calculator
version: 1.0.0
description: Math calculator plugin for exact arithmetic and unit conversion
provides_tools:
  - calculate
  - unit_convert
provides_hooks:
  - post_tool_call
"#,
    )
    .expect("plugin manifest");
    fs::write(
        dir.join("schemas.py"),
        r#"
CALCULATE = {
    "name": "calculate",
    "description": "Evaluate a mathematical expression for exact arithmetic.",
    "parameters": {
        "type": "object",
        "properties": {
            "expression": {"type": "string"}
        },
        "required": ["expression"]
    }
}

UNIT_CONVERT = {
    "name": "unit_convert",
    "description": "Convert a value between supported units.",
    "parameters": {
        "type": "object",
        "properties": {
            "value": {"type": "number"},
            "from_unit": {"type": "string"},
            "to_unit": {"type": "string"}
        },
        "required": ["value", "from_unit", "to_unit"]
    }
}
"#,
    )
    .expect("schemas");
    fs::write(
        dir.join("tools.py"),
        r#"
import json
from pathlib import Path

_SAFE_GLOBALS = {"__builtins__": {}}


def _load_units():
    data = Path(__file__).with_name("data") / "units.json"
    return json.loads(data.read_text())


def calculate(args: dict, **kwargs) -> str:
    expression = str(args.get("expression", "")).strip()
    if not expression:
        return json.dumps({"error": "No expression provided"})
    try:
        result = eval(expression, _SAFE_GLOBALS, {})
        return json.dumps({"expression": expression, "result": result})
    except Exception as exc:
        return json.dumps({"expression": expression, "error": str(exc)})


def unit_convert(args: dict, **kwargs) -> str:
    value = args.get("value")
    from_unit = str(args.get("from_unit", "")).lower()
    to_unit = str(args.get("to_unit", "")).lower()
    if value is None or not from_unit or not to_unit:
        return json.dumps({"error": "Need value, from_unit, and to_unit"})

    units = _load_units()
    if from_unit not in units or to_unit not in units:
        return json.dumps({"error": f"Cannot convert {from_unit} -> {to_unit}"})

    base_value = float(value) * float(units[from_unit])
    result = base_value / float(units[to_unit])
    return json.dumps({
        "input": f"{value} {from_unit}",
        "result": round(result, 6),
        "output": f"{round(result, 6)} {to_unit}"
    })
"#,
    )
    .expect("tools");
    fs::write(
        dir.join("__init__.py"),
        r#"
import json
import os
from pathlib import Path

from . import schemas, tools


def _on_post_tool_call(tool_name, args, result, task_id, **kwargs):
    log_path = Path(os.environ["HERMES_HOME"]) / "calculator-hook.jsonl"
    entry = {
        "tool_name": tool_name,
        "task_id": task_id,
        "args": args,
        "result": result,
    }
    with log_path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(entry) + "\n")


def register(ctx):
    ctx.register_tool("calculate", schemas.CALCULATE, tools.calculate)
    ctx.register_tool("unit_convert", schemas.UNIT_CONVERT, tools.unit_convert)
    ctx.register_hook("post_tool_call", _on_post_tool_call)
"#,
    )
    .expect("init");
    fs::write(
        dir.join("data/units.json"),
        r#"{"m": 1.0, "km": 1000.0, "mi": 1609.34}"#,
    )
    .expect("units");
    fs::write(
        dir.join("SKILL.md"),
        r#"---
name: calculator-skill
description: Use calculator tools for exact arithmetic and unit conversion.
compatibility: Requires calculator plugin
metadata:
  hermes:
    related_skills: [arithmetic-playbook]
---

# Calculator Skill

Use `calculate` when the user wants exact arithmetic.
Use `unit_convert` for metric and imperial conversions.
"#,
    )
    .expect("skill");
}

fn real_hermes_repo() -> &'static PathBuf {
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let path =
            std::env::temp_dir().join(format!("edgecrab-hermes-agent-cli-{REAL_HERMES_REF}"));
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
        assert!(status.success(), "failed to clone hermes-agent");

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

fn real_evey_repo() -> &'static PathBuf {
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("edgecrab-hermes-evey-cli-{REAL_EVEY_REF}"));
        if path.exists() {
            return path;
        }

        let status = Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                REAL_EVEY_REPO_URL,
                path.to_str().expect("utf8 path"),
            ])
            .status()
            .expect("git clone 42-evey/hermes-plugins");
        assert!(status.success(), "failed to clone 42-evey/hermes-plugins");

        let status = Command::new("git")
            .args([
                "-C",
                path.to_str().expect("utf8 path"),
                "checkout",
                REAL_EVEY_REF,
            ])
            .status()
            .expect("git checkout 42-evey/hermes-plugins ref");
        assert!(
            status.success(),
            "failed to checkout 42-evey/hermes-plugins ref"
        );
        path
    })
}

fn repo_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root")
    })
}

fn repo_plugin(path: &str) -> PathBuf {
    repo_root().join(path)
}

fn repo_source_cache_key(repo: &str, roots: &[(&str, &str)]) -> String {
    let mut key = format!("repo={repo}");
    for (kind, location) in roots {
        key.push('|');
        key.push_str(kind);
        key.push(':');
        key.push_str(location);
    }
    key
}

fn repo_source_cache_path(edgecrab_home: &Path, source_name: &str, cache_key: &str) -> PathBuf {
    let digest = Sha256::digest(cache_key.as_bytes());
    edgecrab_home
        .join("plugins")
        .join(".hub")
        .join("cache")
        .join(format!("{source_name}-repo-{digest:x}.json"))
}

fn repo_entry_description_cache_path(
    edgecrab_home: &Path,
    source_name: &str,
    repo: &str,
    repo_path: &str,
    kind: PluginKind,
) -> PathBuf {
    let key = format!("{source_name}:{repo}:{repo_path}:{kind:?}");
    let digest = Sha256::digest(key.as_bytes());
    edgecrab_home
        .join("plugins")
        .join(".hub")
        .join("cache")
        .join("descriptions")
        .join(format!("{source_name}-{digest:x}.json"))
}

fn write_cached_repo_index(
    edgecrab_home: &Path,
    source_name: &str,
    cache_key: &str,
    entries: Value,
) {
    let path = repo_source_cache_path(edgecrab_home, source_name, cache_key);
    fs::create_dir_all(path.parent().expect("repo cache parent")).expect("repo cache dir");
    fs::write(
        path,
        serde_json::to_vec(&json!({
            "fetched_at": Utc::now().timestamp(),
            "entries": entries,
        }))
        .expect("serialize repo cache"),
    )
    .expect("write repo cache");
}

fn write_cached_repo_description(
    edgecrab_home: &Path,
    source_name: &str,
    repo: &str,
    repo_path: &str,
    kind: PluginKind,
    description: &str,
) {
    let path = repo_entry_description_cache_path(edgecrab_home, source_name, repo, repo_path, kind);
    fs::create_dir_all(path.parent().expect("description cache parent"))
        .expect("description cache dir");
    fs::write(
        path,
        serde_json::to_vec(&json!({
            "fetched_at": Utc::now().timestamp(),
            "description": description,
        }))
        .expect("serialize description cache"),
    )
    .expect("write description cache");
}

#[tokio::test(flavor = "multi_thread")]
async fn guide_style_hermes_plugin_installs_and_runs_end_to_end() {
    let home = tempdir().expect("temp home");
    let plugin_dir = home.path().join("calculator");
    write_guide_style_calculator_plugin(&plugin_dir);

    let install_out = run_edgecrab(
        home.path(),
        &[
            "plugins",
            "install",
            "--force",
            plugin_dir.to_str().expect("utf8 path"),
        ],
    );
    assert!(install_out.contains("Plugin 'calculator' installed and enabled."));

    let info = run_edgecrab(home.path(), &["plugins", "info", "calculator"]);
    assert!(info.contains("Kind:         hermes"), "info:\n{info}");
    assert!(
        info.contains("Tools:        calculate, unit_convert"),
        "info:\n{info}"
    );
    assert!(
        info.contains("Compatibility:  Requires calculator plugin"),
        "info:\n{info}"
    );
    assert!(
        info.contains("Related:      arithmetic-playbook"),
        "info:\n{info}"
    );

    let edgecrab_home = home.path().join(".edgecrab");
    let plugin = discover_installed_plugin(&edgecrab_home, "calculator");
    assert!(plugin.hooks.iter().any(|hook| hook == "post_tool_call"));
    assert!(plugin.skill.is_some());

    let client = plugin_client(&plugin);
    let calculate = client
        .call_method(
            "tools/call",
            json!({
                "name": "calculate",
                "arguments": {"expression": "2**10 + 24"},
                "session_id": "guide-plugin-session",
                "platform": "cli",
            }),
            None,
        )
        .await
        .expect("calculate call");
    let calculate = parse_tool_json(calculate);
    assert_eq!(calculate["result"], json!(1048));

    let convert = client
        .call_method(
            "tools/call",
            json!({
                "name": "unit_convert",
                "arguments": {"value": 5, "from_unit": "km", "to_unit": "mi"},
                "session_id": "guide-plugin-session",
                "platform": "cli",
            }),
            None,
        )
        .await
        .expect("convert call");
    let convert = parse_tool_json(convert);
    assert_eq!(convert["output"], json!("3.106864 mi"));
    client.shutdown().await.expect("shutdown guide plugin");

    invoke_hermes_hook(
        &plugin,
        "post_tool_call",
        json!({
            "tool_name": "calculate",
            "args": {"expression": "2**10 + 24"},
            "result": {"result": 1048},
            "task_id": "guide-plugin-task",
            "session_id": "guide-plugin-session",
            "platform": "cli",
        }),
    )
    .await
    .expect("post tool hook");

    let hook_log =
        fs::read_to_string(edgecrab_home.join("calculator-hook.jsonl")).expect("hook log");
    assert!(
        hook_log.contains("\"tool_name\": \"calculate\"")
            || hook_log.contains("\"tool_name\":\"calculate\"")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn repo_example_calculator_plugin_installs_and_runs_end_to_end() {
    let home = tempdir().expect("temp home");
    let plugin_dir = repo_plugin("plugins/productivity/calculator");

    let install_out = run_edgecrab(
        home.path(),
        &[
            "plugins",
            "install",
            "--force",
            plugin_dir.to_str().expect("utf8 path"),
        ],
    );
    assert!(install_out.contains("Plugin 'calculator' installed and enabled."));

    let info = run_edgecrab(home.path(), &["plugins", "info", "calculator"]);
    assert!(info.contains("Kind:         hermes"), "info:\n{info}");
    assert!(
        info.contains("Tools:        calculate, unit_convert"),
        "info:\n{info}"
    );
    assert!(
        info.contains("Compatibility:  Requires calculator plugin"),
        "info:\n{info}"
    );

    let edgecrab_home = home.path().join(".edgecrab");
    let plugin = discover_installed_plugin(&edgecrab_home, "calculator");
    let client = plugin_client(&plugin);

    let calculate = client
        .call_method(
            "tools/call",
            json!({
                "name": "calculate",
                "arguments": {"expression": "((7 + 5) * 3) - 4"},
                "session_id": "repo-calculator-session",
                "platform": "cli",
            }),
            None,
        )
        .await
        .expect("calculate call");
    let calculate = parse_tool_json(calculate);
    assert_eq!(calculate["ok"], json!(true));
    assert_eq!(calculate["result"], json!(32));

    let convert = client
        .call_method(
            "tools/call",
            json!({
                "name": "unit_convert",
                "arguments": {"value": 1609.34, "from_unit": "m", "to_unit": "mi"},
                "session_id": "repo-calculator-session",
                "platform": "cli",
            }),
            None,
        )
        .await
        .expect("convert call");
    let convert = parse_tool_json(convert);
    assert_eq!(convert["ok"], json!(true));
    assert_eq!(convert["output"]["value"], json!(1));
    client.shutdown().await.expect("shutdown calculator");

    invoke_hermes_hook(
        &plugin,
        "post_tool_call",
        json!({
            "tool_name": "calculate",
            "args": {"expression": "((7 + 5) * 3) - 4"},
            "result": {"ok": true, "result": 32},
            "task_id": "repo-calculator-task",
            "session_id": "repo-calculator-session",
            "platform": "cli",
        }),
    )
    .await
    .expect("post tool hook");

    let hook_log =
        fs::read_to_string(edgecrab_home.join("calculator-hook.jsonl")).expect("hook log");
    assert!(
        hook_log.contains("\"tool_name\": \"calculate\"")
            || hook_log.contains("\"tool_name\":\"calculate\"")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn repo_example_json_toolbox_plugin_installs_runs_and_exposes_cli() {
    let home = tempdir().expect("temp home");
    let plugin_dir = repo_plugin("plugins/developer/json-toolbox");

    let install_out = run_edgecrab(
        home.path(),
        &[
            "plugins",
            "install",
            "--force",
            plugin_dir.to_str().expect("utf8 path"),
        ],
    );
    assert!(install_out.contains("Plugin 'json-toolbox' installed and enabled."));

    let info = run_edgecrab(home.path(), &["plugins", "info", "json-toolbox"]);
    assert!(info.contains("Kind:         hermes"), "info:\n{info}");
    assert!(
        info.contains("Tools:        json_pointer_get, json_validate")
            || info.contains("Tools:        json_validate, json_pointer_get"),
        "info:\n{info}"
    );
    assert!(info.contains("CLI:          json-toolbox"), "info:\n{info}");

    let edgecrab_home = home.path().join(".edgecrab");
    let plugin = discover_installed_plugin(&edgecrab_home, "json-toolbox");
    let client = plugin_client(&plugin);

    let pointer = client
        .call_method(
            "tools/call",
            json!({
                "name": "json_pointer_get",
                "arguments": {
                    "content": "{\"meta\":{\"name\":\"edgecrab\"},\"items\":[{\"id\":7}]}",
                    "pointer": "/items/0/id"
                },
                "session_id": "repo-json-toolbox-session",
                "platform": "cli",
            }),
            None,
        )
        .await
        .expect("json pointer call");
    let pointer = parse_tool_json(pointer);
    assert_eq!(pointer["ok"], json!(true));
    assert_eq!(pointer["value"], json!(7));
    client.shutdown().await.expect("shutdown json toolbox");

    let sample = home.path().join("sample.json");
    fs::write(&sample, "{\"z\":1,\"a\":2}").expect("write sample");
    let pretty = run_edgecrab(
        home.path(),
        &[
            "json-toolbox",
            "pretty",
            sample.to_str().expect("utf8 sample path"),
        ],
    );
    assert!(pretty.contains("\"a\": 2"), "pretty:\n{pretty}");
    assert!(pretty.contains("\"z\": 1"), "pretty:\n{pretty}");

    let validate = run_edgecrab(
        home.path(),
        &[
            "json-toolbox",
            "validate",
            sample.to_str().expect("utf8 sample path"),
        ],
    );
    assert_eq!(validate.trim(), "valid");
}

#[tokio::test(flavor = "multi_thread")]
async fn real_hermes_holographic_plugin_installs_and_runs_end_to_end() {
    let home = tempdir().expect("temp home");
    let plugin_dir = real_hermes_repo().join("plugins/memory/holographic");

    let install_out = run_edgecrab(
        home.path(),
        &[
            "plugins",
            "install",
            "--force",
            plugin_dir.to_str().expect("utf8 path"),
        ],
    );
    assert!(install_out.contains("Plugin 'holographic' installed and enabled."));

    let info = run_edgecrab(home.path(), &["plugins", "info", "holographic"]);
    assert!(info.contains("Kind:         hermes"), "info:\n{info}");
    assert!(
        info.contains("Tools:        fact_feedback, fact_store"),
        "info:\n{info}"
    );

    let edgecrab_home = home.path().join(".edgecrab");
    let plugin = discover_installed_plugin(&edgecrab_home, "holographic");
    assert!(plugin.hooks.iter().any(|hook| hook == "on_session_start"));
    assert!(plugin.hooks.iter().any(|hook| hook == "pre_llm_call"));
    assert!(plugin.hooks.iter().any(|hook| hook == "on_session_end"));

    invoke_hermes_hook(
        &plugin,
        "on_session_start",
        json!({
            "session_id": "real-holographic-session",
            "platform": "cli",
        }),
    )
    .await
    .expect("holographic init");

    let client = plugin_client(&plugin);
    let _ = client
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
        .expect("store fact");
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
        .expect("search fact");
    assert!(
        search
            .as_str()
            .expect("search result text")
            .contains("User prefers Rust for systems work")
    );
    client.shutdown().await.expect("shutdown holographic");

    let results = invoke_hermes_hook(
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
    .expect("pre llm call");
    let context = extract_pre_llm_context(&results).join("\n");
    if !context.is_empty() {
        assert!(context.contains("Rust"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn real_hermes_honcho_memory_cli_is_invocable_end_to_end() {
    let home = tempdir().expect("temp home");

    let install_out = run_edgecrab(
        home.path(),
        &[
            "plugins",
            "install",
            "--force",
            "hub:hermes-plugins/plugins/memory/honcho",
        ],
    );
    assert!(install_out.contains("Plugin 'honcho' installed and enabled."));

    let edgecrab_home = home.path().join(".edgecrab");
    let plugin = discover_installed_plugin(&edgecrab_home, "honcho");
    assert!(
        plugin
            .cli_commands
            .iter()
            .any(|command| command.name == "honcho")
    );

    let (exit_code, stdout, stderr) =
        invoke_hermes_cli_command(&plugin, "honcho", &["--help".into()])
            .await
            .expect("invoke honcho cli");
    assert_eq!(exit_code, 0);
    assert!(stdout.contains("status"), "stdout:\n{stdout}");
    assert!(stdout.contains("sessions"), "stdout:\n{stdout}");
    assert!(stdout.contains("tokens"), "stdout:\n{stdout}");
    assert!(stderr.is_empty(), "stderr:\n{stderr}");
}

#[test]
fn real_hermes_optional_skill_installs_from_local_repo_dir() {
    let home = tempdir().expect("temp home");
    let skill_dir = real_hermes_repo().join("optional-skills/security/1password");

    let install_out = run_edgecrab(
        home.path(),
        &["plugins", "install", skill_dir.to_str().expect("utf8 path")],
    );
    assert!(install_out.contains("Plugin '1password' installed and enabled."));

    let info = run_edgecrab(home.path(), &["plugins", "info", "1password"]);
    assert!(info.contains("Kind:         skill"), "info:\n{info}");
    assert!(info.contains("State:        setup-needed"), "info:\n{info}");
    assert!(
        info.contains("Missing Env:  OP_SERVICE_ACCOUNT_TOKEN"),
        "info:\n{info}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn real_evey_telemetry_plugin_installs_and_runs_end_to_end() {
    let home = tempdir().expect("temp home");
    let plugin_dir = real_evey_repo().join("evey-telemetry");

    let install_out = run_edgecrab(
        home.path(),
        &[
            "plugins",
            "install",
            plugin_dir.to_str().expect("utf8 path"),
        ],
    );
    assert!(install_out.contains("Plugin 'evey-telemetry' installed and enabled."));

    let info = run_edgecrab(home.path(), &["plugins", "info", "evey-telemetry"]);
    assert!(info.contains("Kind:         hermes"), "info:\n{info}");
    assert!(
        info.contains("Tools:        telemetry_query"),
        "info:\n{info}"
    );

    let edgecrab_home = home.path().join(".edgecrab");
    let plugin = discover_installed_plugin(&edgecrab_home, "evey-telemetry");
    let client = plugin_client(&plugin);
    let result = client
        .call_method(
            "tools/call",
            json!({
                "name": "telemetry_query",
                "arguments": {"query_type": "session_metrics"},
                "session_id": "real-evey-telemetry-session",
                "platform": "cli",
            }),
            None,
        )
        .await
        .expect("telemetry query");
    let payload = parse_tool_json(result);
    assert_eq!(payload["status"], json!("ok"));
    assert_eq!(payload["metrics"]["tool_calls"], json!(0));
    client.shutdown().await.expect("shutdown evey telemetry");

    let telemetry_log = home.path().join(".hermes/telemetry/events.jsonl");
    let telemetry = fs::read_to_string(telemetry_log).expect("telemetry log");
    assert!(telemetry.contains("evey-telemetry"));
}

#[tokio::test(flavor = "multi_thread")]
async fn real_evey_status_plugin_installs_and_runs_end_to_end() {
    let home = tempdir().expect("temp home");
    let plugin_dir = real_evey_repo().join("evey-status");

    let install_out = run_edgecrab(
        home.path(),
        &[
            "plugins",
            "install",
            plugin_dir.to_str().expect("utf8 path"),
        ],
    );
    assert!(install_out.contains("Plugin 'evey-status' installed and enabled."));

    let info = run_edgecrab(home.path(), &["plugins", "info", "evey-status"]);
    assert!(info.contains("Kind:         hermes"), "info:\n{info}");
    assert!(info.contains("Tools:        status_check"), "info:\n{info}");

    let edgecrab_home = home.path().join(".edgecrab");
    let plugin = discover_installed_plugin(&edgecrab_home, "evey-status");
    let client = plugin_client(&plugin);
    let result = client
        .call_method(
            "tools/call",
            json!({
                "name": "status_check",
                "arguments": {},
                "session_id": "real-evey-status-session",
                "platform": "cli",
            }),
            None,
        )
        .await
        .expect("status check");
    let payload = parse_tool_json(result);
    assert!(
        payload["summary"]
            .as_str()
            .expect("status summary")
            .contains("Dashboard unreachable")
    );
    assert_eq!(payload["recommendation"], json!("cautious"));
    client.shutdown().await.expect("shutdown evey status");
}

#[test]
fn pip_entrypoint_hermes_plugin_is_discovered_and_cli_invocable_end_to_end() {
    let home = tempdir().expect("temp home");
    let package_dir = home.path().join("entrypoint-demo-package");
    create_entrypoint_plugin_package(&package_dir);
    let python = build_python_venv(home.path(), &package_dir);
    let python_str = python.to_str().expect("utf8 python path");

    let list = run_edgecrab_with_env(
        home.path(),
        &["plugins", "list"],
        &[("EDGECRAB_PLUGIN_PYTHON", python_str)],
    );
    assert!(list.contains("entrypoint-demo"), "list:\n{list}");

    let info = run_edgecrab_with_env(
        home.path(),
        &["plugins", "info", "entrypoint-demo"],
        &[("EDGECRAB_PLUGIN_PYTHON", python_str)],
    );
    assert!(info.contains("Kind:         hermes"), "info:\n{info}");
    assert!(info.contains("Tools:        entry_add"), "info:\n{info}");
    assert!(info.contains("CLI:          entry-demo"), "info:\n{info}");

    let cli = run_edgecrab_with_env(
        home.path(),
        &["entry-demo", "status"],
        &[("EDGECRAB_PLUGIN_PYTHON", python_str)],
    );
    assert_eq!(cli.trim(), "entrypoint ok");
}

#[test]
fn official_repo_example_plugins_are_displayed_in_plugin_search() {
    let home = tempdir().expect("temp home");
    let edgecrab_home = home.path().join(".edgecrab");

    let cache_key = repo_source_cache_key("raphaelmansuy/edgecrab", &[("hermes", "tree:plugins")]);
    write_cached_repo_index(
        &edgecrab_home,
        "edgecrab-official",
        &cache_key,
        json!([
            {
                "name": "calculator",
                "repo_path": "plugins/productivity/calculator",
                "tags": ["productivity"],
                "kind": "hermes",
                "tools": ["calculate", "unit_convert"],
                "requires_env": [],
            },
            {
                "name": "json-toolbox",
                "repo_path": "plugins/developer/json-toolbox",
                "tags": ["developer"],
                "kind": "hermes",
                "tools": ["json_validate", "json_pointer_get"],
                "requires_env": [],
            }
        ]),
    );
    write_cached_repo_description(
        &edgecrab_home,
        "edgecrab-official",
        "raphaelmansuy/edgecrab",
        "plugins/productivity/calculator",
        PluginKind::Hermes,
        "Safe arithmetic and unit conversion for exact, dependency-free calculations",
    );
    write_cached_repo_description(
        &edgecrab_home,
        "edgecrab-official",
        "raphaelmansuy/edgecrab",
        "plugins/developer/json-toolbox",
        PluginKind::Hermes,
        "Validate, format, and inspect JSON payloads with deterministic JSON Pointer lookups",
    );

    let output = run_edgecrab(
        home.path(),
        &["plugins", "search", "--source", "edgecrab", "json"],
    );
    assert!(output.contains("json-toolbox"), "output:\n{output}");
    assert!(
        output.contains("hub:edgecrab-official/plugins/developer/json-toolbox"),
        "output:\n{output}"
    );

    let output = run_edgecrab(
        home.path(),
        &["plugins", "search", "--source", "edgecrab", "calculator"],
    );
    assert!(output.contains("calculator"), "output:\n{output}");
    assert!(
        output.contains("hub:edgecrab-official/plugins/productivity/calculator"),
        "output:\n{output}"
    );
}
