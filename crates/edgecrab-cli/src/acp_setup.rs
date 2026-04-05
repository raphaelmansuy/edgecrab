//! ACP onboarding helpers for editor integration.
//!
//! WHY separate module: editor setup is file-generation logic, not runtime ACP
//! serving. Keeping it out of `main.rs` preserves single responsibility and
//! makes the JSON merge behavior testable in isolation.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

const VSCODE_SETTINGS_KEY: &str = "acpClient.agents";

pub fn run_init(workspace: Option<PathBuf>, force: bool) -> Result<()> {
    let workspace = resolve_workspace(workspace)?;
    let plan = init_workspace(&workspace, force)?;

    println!("ACP workspace setup complete.");
    println!("Workspace: {}", workspace.display());
    println!("Registry:  {}", plan.registry_manifest.display());
    println!("VS Code:   {}", plan.settings_path.display());
    println!();
    println!("Next steps:");
    println!("1. Install a VS Code ACP client extension if you have not already.");
    println!("2. Open this workspace in VS Code and reload the window.");
    println!("3. Pick `edgecrab` in the ACP agent list, then start asking for code changes.");
    println!();
    println!("Capabilities exposed in ACP mode:");
    println!("- Streaming responses and multi-turn sessions");
    println!("- File reads/writes/patches scoped to the editor workspace policy");
    println!("- Terminal commands with approval flow for risky actions");
    println!("- Skills, memory, browser, and coding tool access");

    if plan.path_hint_needed {
        println!();
        println!(
            "Note: the generated manifest launches `edgecrab acp`, so VS Code must be able to resolve `edgecrab` on PATH."
        );
    }

    Ok(())
}

#[derive(Debug)]
struct InitPlan {
    registry_manifest: PathBuf,
    settings_path: PathBuf,
    path_hint_needed: bool,
}

fn resolve_workspace(workspace: Option<PathBuf>) -> Result<PathBuf> {
    let workspace = match workspace {
        Some(path) => path,
        None => std::env::current_dir().context("failed to resolve current directory")?,
    };
    if !workspace.exists() {
        bail!("workspace does not exist: {}", workspace.display());
    }
    if !workspace.is_dir() {
        bail!("workspace is not a directory: {}", workspace.display());
    }
    workspace
        .canonicalize()
        .with_context(|| format!("failed to canonicalize workspace '{}'", workspace.display()))
}

fn init_workspace(workspace: &Path, force: bool) -> Result<InitPlan> {
    let registry_dir = workspace.join(".edgecrab").join("acp_registry");
    let manifest_path = registry_dir.join("agent.json");
    let settings_path = workspace.join(".vscode").join("settings.json");

    fs::create_dir_all(&registry_dir)
        .with_context(|| format!("failed to create registry dir '{}'", registry_dir.display()))?;
    fs::create_dir_all(
        settings_path
            .parent()
            .expect("settings path always has a parent"),
    )
    .with_context(|| {
        format!(
            "failed to create VS Code settings dir '{}'",
            settings_path
                .parent()
                .expect("settings path always has a parent")
                .display()
        )
    })?;

    let manifest = build_manifest();
    let manifest_text = serde_json::to_string_pretty(&manifest)
        .context("failed to serialize ACP registry manifest")?;
    fs::write(&manifest_path, format!("{manifest_text}\n")).with_context(|| {
        format!(
            "failed to write ACP registry manifest '{}'",
            manifest_path.display()
        )
    })?;

    let settings_value = load_settings_file(&settings_path, force)?;
    let updated_settings = upsert_vscode_agent(settings_value, &registry_dir);
    let settings_text = serde_json::to_string_pretty(&updated_settings)
        .context("failed to serialize VS Code settings")?;
    fs::write(&settings_path, format!("{settings_text}\n")).with_context(|| {
        format!(
            "failed to write VS Code settings '{}'",
            settings_path.display()
        )
    })?;

    Ok(InitPlan {
        registry_manifest: manifest_path,
        settings_path,
        path_hint_needed: !command_on_path("edgecrab"),
    })
}

fn build_manifest() -> Value {
    json!({
        "name": "edgecrab",
        "description": "EdgeCrab — an ACP-compatible agent with file, terminal, and skill tools. Launched as a child process and communicates via JSON-RPC 2.0 over stdio.",
        "version": env!("CARGO_PKG_VERSION"),
        "launch": {
            "type": "command",
            "command": "edgecrab acp"
        },
        "protocol": {
            "transport": "stdio",
            "format": "jsonrpc2"
        },
        "capabilities": {
            "session": {
                "fork": true,
                "list": true
            },
            "approval": true,
            "streaming": true
        },
        "editor_setup": {
            "vscode": {
                "setting": VSCODE_SETTINGS_KEY,
                "example": [
                    {
                        "name": "edgecrab",
                        "registryDir": "/path/to/workspace/.edgecrab/acp_registry"
                    }
                ]
            }
        }
    })
}

fn load_settings_file(path: &Path, force: bool) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read VS Code settings '{}'", path.display()))?;
    if content.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }

    match serde_json::from_str::<Value>(&content) {
        Ok(Value::Object(map)) => Ok(Value::Object(map)),
        Ok(_) if force => Ok(Value::Object(Map::new())),
        Ok(_) => bail!(
            "VS Code settings '{}' must contain a JSON object; rerun with --force to replace it",
            path.display()
        ),
        Err(_) if force => Ok(Value::Object(Map::new())),
        Err(err) => bail!(
            "VS Code settings '{}' is not valid JSON: {err}. Rerun with --force to replace it",
            path.display()
        ),
    }
}

fn upsert_vscode_agent(mut settings: Value, registry_dir: &Path) -> Value {
    let settings_object = settings
        .as_object_mut()
        .expect("settings JSON must be an object before upsert");

    let entry = json!({
        "name": "edgecrab",
        "registryDir": registry_dir.to_string_lossy(),
    });

    let agents = settings_object
        .entry(VSCODE_SETTINGS_KEY)
        .or_insert_with(|| Value::Array(Vec::new()));

    let mut existing_agents = match agents.take() {
        Value::Array(values) => values,
        _ => Vec::new(),
    };

    existing_agents.retain(|agent| {
        agent
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(|name| name != "edgecrab")
    });
    existing_agents.push(entry);

    *agents = Value::Array(existing_agents);
    settings
}

fn command_on_path(command: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            let candidate = dir.join(command);
            candidate.is_file()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn init_workspace_creates_registry_and_settings() {
        let workspace = TempDir::new().expect("workspace");

        let result = init_workspace(workspace.path(), false).expect("init");

        assert!(result.registry_manifest.exists());
        assert!(result.settings_path.exists());

        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(&result.registry_manifest).expect("manifest text"),
        )
        .expect("manifest json");
        assert_eq!(manifest["launch"]["command"], "edgecrab acp");

        let settings: Value = serde_json::from_str(
            &fs::read_to_string(&result.settings_path).expect("settings text"),
        )
        .expect("settings json");
        let agents = settings[VSCODE_SETTINGS_KEY]
            .as_array()
            .expect("acpClient.agents array");
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0]["name"], "edgecrab");
        assert_eq!(
            agents[0]["registryDir"],
            workspace
                .path()
                .join(".edgecrab")
                .join("acp_registry")
                .to_string_lossy()
                .as_ref()
        );
    }

    #[test]
    fn init_workspace_preserves_other_vscode_settings() {
        let workspace = TempDir::new().expect("workspace");
        let vscode_dir = workspace.path().join(".vscode");
        fs::create_dir_all(&vscode_dir).expect("create vscode dir");
        fs::write(
            vscode_dir.join("settings.json"),
            r#"{
  "editor.formatOnSave": true,
  "acpClient.agents": [
    {"name": "existing", "registryDir": "/tmp/existing"},
    {"name": "edgecrab", "registryDir": "/tmp/old"}
  ]
}
"#,
        )
        .expect("write settings");

        let result = init_workspace(workspace.path(), false).expect("init");
        let settings: Value = serde_json::from_str(
            &fs::read_to_string(&result.settings_path).expect("settings text"),
        )
        .expect("settings json");

        assert_eq!(settings["editor.formatOnSave"], true);
        let agents = settings[VSCODE_SETTINGS_KEY]
            .as_array()
            .expect("acpClient.agents array");
        assert_eq!(agents.len(), 2);
        assert!(agents.iter().any(|agent| agent["name"] == "existing"));
        assert_eq!(
            agents
                .iter()
                .filter(|agent| agent["name"] == "edgecrab")
                .count(),
            1
        );
    }

    #[test]
    fn init_workspace_rejects_invalid_settings_without_force() {
        let workspace = TempDir::new().expect("workspace");
        let vscode_dir = workspace.path().join(".vscode");
        fs::create_dir_all(&vscode_dir).expect("create vscode dir");
        fs::write(vscode_dir.join("settings.json"), "{ invalid json").expect("write settings");

        let err = init_workspace(workspace.path(), false).expect_err("invalid settings");
        assert!(err.to_string().contains("not valid JSON"));
    }

    #[test]
    fn init_workspace_force_replaces_invalid_settings() {
        let workspace = TempDir::new().expect("workspace");
        let vscode_dir = workspace.path().join(".vscode");
        fs::create_dir_all(&vscode_dir).expect("create vscode dir");
        fs::write(vscode_dir.join("settings.json"), "{ invalid json").expect("write settings");

        let result = init_workspace(workspace.path(), true).expect("force init");
        let settings: Value = serde_json::from_str(
            &fs::read_to_string(&result.settings_path).expect("settings text"),
        )
        .expect("settings json");
        assert!(settings[VSCODE_SETTINGS_KEY].is_array());
    }
}
