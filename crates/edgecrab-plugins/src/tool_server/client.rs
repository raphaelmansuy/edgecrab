use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::error::PluginError;
use crate::host_api::{handle_host_request, is_host_method};
use crate::manifest::{PluginCapabilities, PluginExecConfig, PluginRestartPolicy};
use edgecrab_tools::registry::ToolContext;

struct ProcessState {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    restart_count: u32,
    last_used_at: Instant,
}

#[derive(Clone)]
pub struct ToolServerClient {
    plugin_dir: PathBuf,
    plugin_name: String,
    config: PluginExecConfig,
    capabilities: PluginCapabilities,
    state: Arc<Mutex<Option<ProcessState>>>,
    next_id: Arc<AtomicU64>,
}

impl ToolServerClient {
    pub fn new(
        plugin_dir: PathBuf,
        plugin_name: String,
        config: PluginExecConfig,
        capabilities: PluginCapabilities,
    ) -> Self {
        Self {
            plugin_dir,
            plugin_name,
            config,
            capabilities,
            state: Arc::new(Mutex::new(None)),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub async fn shutdown(&self) -> Result<(), PluginError> {
        let mut state = self.state.lock().await;
        if let Some(process) = state.as_mut() {
            let request = json!({
                "jsonrpc": "2.0",
                "id": self.next_id.fetch_add(1, Ordering::Relaxed),
                "method": "shutdown",
                "params": {},
            });
            let _ = process
                .stdin
                .write_all(request.to_string().as_bytes())
                .await;
            let _ = process.stdin.write_all(b"\n").await;
            let _ = process.stdin.flush().await;
            let _ = tokio::time::timeout(Duration::from_secs(2), process.child.wait()).await;
            let _ = process.child.kill().await;
        }
        *state = None;
        Ok(())
    }

    pub async fn tool_list(&self) -> Result<Vec<serde_json::Value>, PluginError> {
        let result = self.rpc("tools/list", json!({}), None).await?;
        Ok(result
            .as_array()
            .cloned()
            .or_else(|| {
                result
                    .get("tools")
                    .and_then(|value| value.as_array().cloned())
            })
            .unwrap_or_default())
    }

    pub async fn tool_call(
        &self,
        name: &str,
        arguments: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<serde_json::Value, PluginError> {
        self.rpc(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments,
            }),
            Some(ctx),
        )
        .await
    }

    pub async fn call_method(
        &self,
        method: &str,
        params: serde_json::Value,
        ctx: Option<&ToolContext>,
    ) -> Result<serde_json::Value, PluginError> {
        self.rpc(method, params, ctx).await
    }

    async fn rpc(
        &self,
        method: &str,
        params: serde_json::Value,
        ctx: Option<&ToolContext>,
    ) -> Result<serde_json::Value, PluginError> {
        let mut state = self.state.lock().await;
        self.ensure_process(&mut state).await?;

        let process = state
            .as_mut()
            .ok_or_else(|| PluginError::Process("plugin process unavailable".into()))?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        process
            .stdin
            .write_all(request.to_string().as_bytes())
            .await?;
        process.stdin.write_all(b"\n").await?;
        process.stdin.flush().await?;

        let mut line = String::new();
        loop {
            line.clear();
            let read = tokio::time::timeout(
                Duration::from_secs(self.config.call_timeout_secs.max(1)),
                process.stdout.read_line(&mut line),
            )
            .await
            .map_err(|_| PluginError::Rpc(format!("timeout waiting for {method} response")))??;
            if read == 0 {
                self.handle_process_failure(&mut state, method).await?;
                return Err(PluginError::Process(format!(
                    "plugin process closed stdout during {method}"
                )));
            }
            process.last_used_at = Instant::now();
            let response: serde_json::Value = serde_json::from_str(line.trim())?;
            if let Some(host_method) = response.get("method").and_then(|value| value.as_str()) {
                if is_host_method(host_method) {
                    let Some(ctx) = ctx else {
                        return Err(PluginError::Rpc(format!(
                            "plugin requested {host_method} without a host context"
                        )));
                    };
                    let host_response =
                        handle_host_request(&self.plugin_name, &self.capabilities, &response, ctx)
                            .await;
                    process
                        .stdin
                        .write_all(host_response.to_string().as_bytes())
                        .await?;
                    process.stdin.write_all(b"\n").await?;
                    process.stdin.flush().await?;
                    continue;
                }
                tracing::debug!(plugin = %self.plugin_name, method = host_method, "ignoring plugin notification");
                continue;
            }
            if response.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(PluginError::Rpc(error.to_string()));
            }
            return Ok(extract_result_payload(
                response.get("result").cloned().unwrap_or(Value::Null),
            ));
        }
    }

    async fn ensure_process(&self, state: &mut Option<ProcessState>) -> Result<(), PluginError> {
        let should_respawn = match state.as_mut() {
            None => true,
            Some(process) => {
                let idle_timeout = self.config.idle_timeout_secs.max(1);
                if process.last_used_at.elapsed() >= Duration::from_secs(idle_timeout) {
                    let _ = process.child.kill().await;
                    true
                } else {
                    process.child.try_wait()?.is_some()
                }
            }
        };

        if should_respawn {
            let restart_count = state.as_ref().map_or(0, |process| process.restart_count);
            *state = Some(self.spawn(restart_count).await?);
        }
        Ok(())
    }

    async fn handle_process_failure(
        &self,
        state: &mut Option<ProcessState>,
        method: &str,
    ) -> Result<(), PluginError> {
        let restart_count = state.as_ref().map_or(0, |process| process.restart_count);
        if !self.should_restart(restart_count) {
            *state = None;
            return Err(PluginError::Process(format!(
                "plugin process crashed during {method} and restart policy prevents recovery"
            )));
        }
        *state = Some(self.spawn(restart_count + 1).await?);
        Ok(())
    }

    fn should_restart(&self, restart_count: u32) -> bool {
        match self.config.restart_policy {
            PluginRestartPolicy::Never => false,
            PluginRestartPolicy::Once => restart_count == 0,
            PluginRestartPolicy::Always => restart_count < self.config.restart_max_attempts,
        }
    }

    async fn spawn(&self, restart_count: u32) -> Result<ProcessState, PluginError> {
        let mut command = Command::new(resolve_command(&self.plugin_dir, &self.config.command));
        command.args(&self.config.args);
        command.current_dir(resolve_cwd(&self.plugin_dir, self.config.cwd.as_deref()));
        command.envs(&self.config.env);
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::inherit());

        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| PluginError::Process("missing child stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PluginError::Process("missing child stdout".into()))?;
        let mut state = ProcessState {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            restart_count,
            last_used_at: Instant::now(),
        };

        let init = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "edgecrab",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            },
        });
        state.stdin.write_all(init.to_string().as_bytes()).await?;
        state.stdin.write_all(b"\n").await?;
        state.stdin.flush().await?;

        let mut line = String::new();
        let read = tokio::time::timeout(
            Duration::from_secs(self.config.startup_timeout_secs.max(1)),
            state.stdout.read_line(&mut line),
        )
        .await
        .map_err(|_| PluginError::Rpc("tool server initialize timeout".into()))??;
        if read == 0 {
            return Err(PluginError::Process(
                "tool server exited before initialize completed".into(),
            ));
        }
        let init_response: serde_json::Value = serde_json::from_str(line.trim())?;
        if let Some(error) = init_response.get("error") {
            return Err(PluginError::Rpc(error.to_string()));
        }
        let initialized = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        });
        state
            .stdin
            .write_all(initialized.to_string().as_bytes())
            .await?;
        state.stdin.write_all(b"\n").await?;
        state.stdin.flush().await?;
        state.last_used_at = Instant::now();
        Ok(state)
    }
}

fn extract_result_payload(result: serde_json::Value) -> serde_json::Value {
    if result
        .get("isError")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return json!({
            "isError": true,
            "content": result.get("content").cloned().unwrap_or(Value::Null),
        });
    }
    if let Some(content) = result.get("content").and_then(|value| value.as_array()) {
        let text = content
            .iter()
            .filter(|item| item.get("type").and_then(|value| value.as_str()) == Some("text"))
            .map(|item| {
                item.get("text")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return Value::String(text);
        }
    }
    result
}

fn resolve_command(plugin_dir: &Path, command: &str) -> PathBuf {
    let command_path = PathBuf::from(command);
    if command_path.is_absolute() {
        command_path
    } else if command.starts_with("./") || command.starts_with("../") {
        plugin_dir.join(command)
    } else {
        command_path
    }
}

fn resolve_cwd(plugin_dir: &Path, cwd: Option<&str>) -> PathBuf {
    match cwd {
        Some(".") | None => plugin_dir.to_path_buf(),
        Some(cwd) => plugin_dir.join(cwd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_relative_command_against_plugin_dir() {
        let dir = PathBuf::from("/tmp/plugin");
        assert_eq!(
            resolve_command(&dir, "./bin/server"),
            dir.join("./bin/server")
        );
        assert_eq!(resolve_command(&dir, "python3"), PathBuf::from("python3"));
    }
}
