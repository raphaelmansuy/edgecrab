use std::collections::BTreeMap;
use std::path::Path;

use chrono::Utc;
use edgecrab_security::{check_injection, check_memory_content};
use edgecrab_tools::registry::ToolContext;
use edgecrab_types::Message;
use serde_json::{Value, json};

use crate::manifest::PluginCapabilities;

const MAX_MEMORY_READ_KEYS: usize = 200;
const MAX_SESSION_SEARCH_LIMIT: usize = 50;
const MAX_INJECT_CHARS: usize = 32_000;
const MAX_TOOL_CALL_DEPTH: u32 = 3;

pub fn is_host_method(method: &str) -> bool {
    method.starts_with("host:") || method.starts_with("host/")
}

pub async fn handle_host_request(
    plugin_name: &str,
    capabilities: &PluginCapabilities,
    request: &Value,
    ctx: &ToolContext,
) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match handle_host_request_inner(plugin_name, capabilities, request, ctx).await {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message,
            }
        }),
    }
}

async fn handle_host_request_inner(
    plugin_name: &str,
    capabilities: &PluginCapabilities,
    request: &Value,
    ctx: &ToolContext,
) -> Result<Value, (i64, String)> {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or((-32600, "missing JSON-RPC method".into()))?;
    let method = normalize_method(method);
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    match method.as_str() {
        "host:platform_info" => Ok(json!({
            "platform": ctx.platform.to_string(),
            "session_id": ctx.session_id,
            "model": std::env::var("EDGECRAB_MODEL").ok(),
            "timestamp_utc": Utc::now().to_rfc3339(),
        })),
        "host:log" => {
            let level = params
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("info");
            let message = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match level {
                "trace" => tracing::trace!(target: "edgecrab_plugins::host_api", plugin = plugin_name, "{message}"),
                "debug" => tracing::debug!(target: "edgecrab_plugins::host_api", plugin = plugin_name, "{message}"),
                "warn" => tracing::warn!(target: "edgecrab_plugins::host_api", plugin = plugin_name, "{message}"),
                "error" => tracing::error!(target: "edgecrab_plugins::host_api", plugin = plugin_name, "{message}"),
                _ => tracing::info!(target: "edgecrab_plugins::host_api", plugin = plugin_name, "{message}"),
            }
            Ok(json!({ "ok": true }))
        }
        "host:memory_read" => {
            require_capability(capabilities, &method)?;
            let keys = params
                .get("keys")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .take(MAX_MEMORY_READ_KEYS)
                .collect::<Vec<_>>();
            let facts = read_memory_facts(&ctx.config.edgecrab_home, &keys)
                .map_err(|error| (-32002, error.to_string()))?;
            Ok(json!({ "facts": facts }))
        }
        "host:memory_write" => {
            require_capability(capabilities, &method)?;
            let key = params
                .get("key")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or((-32602, "missing key".into()))?;
            let value = params
                .get("value")
                .and_then(Value::as_str)
                .ok_or((-32602, "missing value".into()))?;
            if key.len() > 128 || value.len() > 4096 {
                return Err((-32602, "memory key/value exceeds limits".into()));
            }
            check_memory_content(value).map_err(|msg| (-32002, msg))?;
            write_memory_fact(&ctx.config.edgecrab_home, key, value)
                .map_err(|error| (-32002, error.to_string()))?;
            Ok(json!({ "ok": true }))
        }
        "host:session_search" => {
            require_capability(capabilities, &method)?;
            let query = params
                .get("query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or((-32602, "missing query".into()))?;
            let limit = params
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(10)
                .min(MAX_SESSION_SEARCH_LIMIT as u64) as usize;
            let Some(db) = &ctx.state_db else {
                return Err((-32006, "session database unavailable".into()));
            };
            let hits = db
                .search_sessions_rich(query, limit)
                .map_err(|error| (-32006, error.to_string()))?;
            Ok(json!({
                "hits": hits.into_iter().map(|hit| json!({
                    "session_id": hit.session.id,
                    "excerpt": hit.snippet,
                    "score": hit.score,
                })).collect::<Vec<_>>()
            }))
        }
        "host:secret_get" => {
            require_capability(capabilities, &method)?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or((-32602, "missing secret name".into()))?;
            if !capabilities.secrets.iter().any(|candidate| candidate == name) {
                return Err((-32003, format!("Secret not whitelisted: {name}")));
            }
            let value = std::env::var(name)
                .map_err(|_| (-32003, format!("Secret unavailable: {name}")))?;
            tracing::info!(target: "edgecrab_plugins::host_api", plugin = plugin_name, secret = name, "plugin secret read");
            Ok(json!({ "value": value }))
        }
        "host:inject_message" => {
            require_capability(capabilities, &method)?;
            let role = params
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user");
            let content = params
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if content.len() > MAX_INJECT_CHARS {
                return Err((-32602, "message content exceeds 32000 characters".into()));
            }
            if let Some(message) = check_injection(content) {
                return Err((-32004, message.into()));
            }
            let Some(queue) = &ctx.injected_messages else {
                return Err((
                    -32004,
                    "conversation injection is not available in the current runtime".into(),
                ));
            };
            let message = match role {
                "user" => Message::user(content),
                "assistant" => Message::assistant(content),
                _ => {
                    return Err((
                        -32602,
                        "inject_message role must be 'user' or 'assistant'".into(),
                    ))
                }
            };
            queue.lock().await.push(message);
            Ok(json!({ "ok": true, "role": role }))
        }
        "host:tool_call" => {
            require_capability(capabilities, &method)?;
            if ctx.delegate_depth >= MAX_TOOL_CALL_DEPTH {
                return Err((
                    -32005,
                    format!("Reentrancy limit exceeded (max depth: {MAX_TOOL_CALL_DEPTH})"),
                ));
            }
            let tool = params
                .get("tool")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or((-32602, "missing tool name".into()))?;
            if ctx.current_tool_name.as_deref() == Some(tool) {
                return Err((-32005, format!("Plugin cannot call itself via host:tool_call: {tool}")));
            }
            let Some(registry) = &ctx.tool_registry else {
                return Err((-32006, "tool registry unavailable".into()));
            };
            let args = params.get("args").cloned().unwrap_or_else(|| json!({}));
            let output = registry
                .dispatch(tool, args, ctx)
                .await
                .map_err(|error| (-32006, error.to_string()))?;
            Ok(json!({ "result": output }))
        }
        _ => Err((-32601, format!("unknown host method: {method}"))),
    }
}

fn normalize_method(method: &str) -> String {
    if let Some(rest) = method.strip_prefix("host/") {
        format!("host:{}", rest.replace('/', "_"))
    } else {
        method.to_string()
    }
}

fn require_capability(
    capabilities: &PluginCapabilities,
    method: &str,
) -> Result<(), (i64, String)> {
    if matches!(method, "host:platform_info" | "host:log") {
        return Ok(());
    }
    if capabilities.host.iter().any(|candidate| candidate == method) {
        return Ok(());
    }
    Err((-32001, format!("Capability not granted: {method}")))
}

fn read_memory_facts(
    edgecrab_home: &Path,
    keys: &[String],
) -> Result<BTreeMap<String, Option<String>>, std::io::Error> {
    let mut facts = parse_memory_facts(edgecrab_home)?;
    if keys.is_empty() {
        return Ok(facts.into_iter().map(|(k, v)| (k, Some(v))).collect());
    }

    let mut filtered = BTreeMap::new();
    for key in keys {
        filtered.insert(key.clone(), facts.remove(key));
    }
    Ok(filtered)
}

fn write_memory_fact(edgecrab_home: &Path, key: &str, value: &str) -> Result<(), std::io::Error> {
    let memories_dir = edgecrab_home.join("memories");
    std::fs::create_dir_all(&memories_dir)?;
    let memory_path = memories_dir.join("USER.md");
    let mut facts = parse_memory_facts(edgecrab_home)?;
    facts.insert(key.to_string(), value.to_string());

    let mut body = String::new();
    for (fact_key, fact_value) in facts {
        if !body.is_empty() {
            body.push_str("\n§\n");
        }
        body.push_str(&format!("{fact_key}: {fact_value}"));
    }
    std::fs::write(memory_path, body)
}

fn parse_memory_facts(edgecrab_home: &Path) -> Result<BTreeMap<String, String>, std::io::Error> {
    let mut facts = BTreeMap::new();
    for path in [
        edgecrab_home.join("memories").join("MEMORY.md"),
        edgecrab_home.join("memories").join("USER.md"),
    ] {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for chunk in content.split("\n§\n") {
            let line = chunk.trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if !key.is_empty() && !value.is_empty() {
                facts.insert(key.to_string(), value.to_string());
            }
        }
    }
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use edgecrab_state::{SessionDb, SessionRecord};
    use edgecrab_tools::config_ref::AppConfigRef;
    use edgecrab_tools::registry::ToolContext;
    use edgecrab_types::{Message, Platform};
    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    use super::*;

    fn test_ctx(home: &Path) -> ToolContext {
        ToolContext {
            task_id: "task".into(),
            cwd: home.to_path_buf(),
            session_id: "session-1".into(),
            user_task: None,
            cancel: CancellationToken::new(),
            config: AppConfigRef {
                edgecrab_home: home.to_path_buf(),
                ..AppConfigRef::default()
            },
            state_db: None,
            platform: Platform::Cli,
            process_table: None,
            provider: None,
            tool_registry: None,
            delegate_depth: 0,
            sub_agent_runner: None,
            delegation_event_tx: None,
            clarify_tx: None,
            approval_tx: None,
            on_skills_changed: None,
            gateway_sender: None,
            origin_chat: None,
            session_key: None,
            todo_store: None,
            current_tool_call_id: None,
            current_tool_name: Some("plugin_tool".into()),
            injected_messages: None,
            tool_progress_tx: None,
        }
    }

    #[tokio::test]
    async fn platform_info_requires_no_capability() {
        let home = TempDir::new().expect("tempdir");
        let response = handle_host_request(
            "demo",
            &PluginCapabilities::default(),
            &json!({"jsonrpc":"2.0","id":"1","method":"host:platform_info","params":{}}),
            &test_ctx(home.path()),
        )
        .await;

        assert_eq!(response["result"]["platform"], "cli");
    }

    #[tokio::test]
    async fn memory_write_and_read_round_trip() {
        let home = TempDir::new().expect("tempdir");
        let caps = PluginCapabilities {
            host: vec!["host:memory_read".into(), "host:memory_write".into()],
            ..PluginCapabilities::default()
        };
        let ctx = test_ctx(home.path());

        let _ = handle_host_request(
            "demo",
            &caps,
            &json!({"jsonrpc":"2.0","id":"1","method":"host:memory_write","params":{"key":"user_lang","value":"English"}}),
            &ctx,
        )
        .await;
        let response = handle_host_request(
            "demo",
            &caps,
            &json!({"jsonrpc":"2.0","id":"2","method":"host:memory_read","params":{"keys":["user_lang"]}}),
            &ctx,
        )
        .await;

        assert_eq!(response["result"]["facts"]["user_lang"], "English");
    }

    #[tokio::test]
    async fn session_search_returns_hits() {
        let home = TempDir::new().expect("tempdir");
        let db = Arc::new(SessionDb::open(&home.path().join("sessions.db")).expect("db"));
        db.save_session(&SessionRecord {
            id: "session-1".into(),
            source: "cli".into(),
            user_id: None,
            model: Some("test".into()),
            system_prompt: None,
            parent_session_id: None,
            started_at: 0.0,
            ended_at: None,
            end_reason: None,
            message_count: 0,
            tool_call_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            estimated_cost_usd: None,
            title: Some("demo".into()),
        })
        .expect("save");
        db.save_message("session-1", &Message::user("find this session"), 1.0)
            .expect("save message");

        let mut ctx = test_ctx(home.path());
        ctx.state_db = Some(db);
        let caps = PluginCapabilities {
            host: vec!["host:session_search".into()],
            ..PluginCapabilities::default()
        };
        let response = handle_host_request(
            "demo",
            &caps,
            &json!({"jsonrpc":"2.0","id":"1","method":"host:session_search","params":{"query":"find","limit":5}}),
            &ctx,
        )
        .await;

        assert_eq!(response["result"]["hits"].as_array().map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn inject_message_enqueues_runtime_message() {
        let home = TempDir::new().expect("tempdir");
        let caps = PluginCapabilities {
            host: vec!["host:inject_message".into()],
            ..PluginCapabilities::default()
        };
        let queue = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let mut ctx = test_ctx(home.path());
        ctx.injected_messages = Some(queue.clone());

        let response = handle_host_request(
            "demo",
            &caps,
            &json!({"jsonrpc":"2.0","id":"1","method":"host:inject_message","params":{"role":"assistant","content":"Created issue #42"}}),
            &ctx,
        )
        .await;

        assert_eq!(response["result"]["ok"], true);
        let queued = queue.lock().await.clone();
        assert_eq!(queued, vec![Message::assistant("Created issue #42")]);
    }
}
