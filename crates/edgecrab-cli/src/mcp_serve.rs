//! `edgecrab mcp serve` — expose EdgeCrab tools as an MCP server over stdio.

use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;

use edgecrab_tools::registry::{ToolContext, ToolRegistry};
use serde_json::{Value, json};

pub async fn run_mcp_serve(registry: Arc<ToolRegistry>) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = std::io::stdout();
    let mut line = String::new();
    let ctx = ToolContext::minimal();

    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                write_rpc(
                    &mut stdout,
                    None,
                    None,
                    Some(json!({"code": -32700, "message": format!("parse error: {e}")})),
                )?;
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(json!({}));

        match method {
            "initialize" => {
                write_rpc(
                    &mut stdout,
                    id,
                    Some(json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "edgecrab", "version": env!("CARGO_PKG_VERSION") }
                    })),
                    None,
                )?;
            }
            "notifications/initialized" | "initialized" => {
                if id.is_some() && id != Some(Value::Null) {
                    write_rpc(&mut stdout, id, Some(json!({})), None)?;
                }
            }
            "ping" => write_rpc(&mut stdout, id, Some(json!({})), None)?,
            "tools/list" => {
                let defs = registry.get_definitions(None, None, &ctx);
                let tools: Vec<Value> = defs
                    .into_iter()
                    .map(|schema| {
                        json!({
                            "name": schema.name,
                            "description": schema.description,
                            "inputSchema": schema.parameters,
                        })
                    })
                    .collect();
                write_rpc(&mut stdout, id, Some(json!({ "tools": tools })), None)?;
            }
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                if name.is_empty() {
                    write_rpc(
                        &mut stdout,
                        id,
                        None,
                        Some(json!({"code": -32602, "message": "missing tool name"})),
                    )?;
                    continue;
                }
                match registry.dispatch(&name, arguments, &ctx).await {
                    Ok(text) => write_rpc(
                        &mut stdout,
                        id,
                        Some(json!({
                            "content": [{ "type": "text", "text": text }],
                            "isError": false
                        })),
                        None,
                    )?,
                    Err(e) => write_rpc(
                        &mut stdout,
                        id,
                        Some(json!({
                            "content": [{ "type": "text", "text": e.to_string() }],
                            "isError": true
                        })),
                        None,
                    )?,
                }
            }
            "" => write_rpc(
                &mut stdout,
                id,
                None,
                Some(json!({"code": -32600, "message": "missing method"})),
            )?,
            other => write_rpc(
                &mut stdout,
                id,
                None,
                Some(json!({
                    "code": -32601,
                    "message": format!("method not found: {other}")
                })),
            )?,
        }
    }
    Ok(())
}

fn write_rpc(
    out: &mut impl Write,
    id: Option<Value>,
    result: Option<Value>,
    error: Option<Value>,
) -> anyhow::Result<()> {
    let mut msg = json!({ "jsonrpc": "2.0" });
    if let Some(id) = id {
        msg["id"] = id;
    }
    if let Some(result) = result {
        msg["result"] = result;
    }
    if let Some(error) = error {
        msg["error"] = error;
    }
    writeln!(out, "{}", serde_json::to_string(&msg)?)?;
    out.flush()?;
    Ok(())
}
