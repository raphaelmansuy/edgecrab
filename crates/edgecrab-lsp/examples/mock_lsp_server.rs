use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

fn write_message(value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    let mut stdout = io::stdout().lock();
    write!(stdout, "Content-Length: {}\r\n\r\n", body.len())?;
    stdout.write_all(&body)?;
    stdout.flush()
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut content_length = None::<usize>;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = content_length else {
        return Ok(None);
    };
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

fn default_range() -> Value {
    json!({
        "start": { "line": 0, "character": 3 },
        "end": { "line": 0, "character": 8 }
    })
}

fn default_diagnostic() -> Value {
    json!({
        "range": default_range(),
        "severity": 1,
        "code": "E100",
        "source": "mock-lsp",
        "message": "mock type mismatch"
    })
}

fn code_action_for(uri: &str) -> Value {
    json!({
        "title": "Replace todo with 42",
        "kind": "quickfix",
        "isPreferred": true,
        "data": { "uri": uri }
    })
}

fn resolved_code_action(uri: &str) -> Value {
    json!({
        "title": "Replace todo with 42",
        "kind": "quickfix",
        "isPreferred": true,
        "data": { "uri": uri },
        "edit": {
            "changes": {
                uri: [{
                    "range": {
                        "start": { "line": 1, "character": 4 },
                        "end": { "line": 1, "character": 11 }
                    },
                    "newText": "42"
                }]
            }
        }
    })
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut docs: BTreeMap<String, String> = BTreeMap::new();

    while let Some(message) = read_message(&mut reader)? {
        let method = message.get("method").and_then(Value::as_str);
        match method {
            Some("initialize") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "capabilities": {
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "hoverProvider": true,
                            "documentSymbolProvider": true,
                            "workspaceSymbolProvider": true,
                            "implementationProvider": true,
                            "callHierarchyProvider": true,
                            "codeActionProvider": { "resolveProvider": true },
                            "renameProvider": true,
                            "documentFormattingProvider": true,
                            "documentRangeFormattingProvider": true,
                            "inlayHintProvider": true,
                            "semanticTokensProvider": {
                                "legend": {
                                    "tokenTypes": ["function", "variable"],
                                    "tokenModifiers": ["declaration"]
                                },
                                "full": true
                            },
                            "signatureHelpProvider": { "triggerCharacters": ["("] },
                            "linkedEditingRangeProvider": {},
                            "diagnosticProvider": {
                                "interFileDependencies": true,
                                "workspaceDiagnostics": true
                            }
                        }
                    }
                }))?;
            }
            Some("initialized") => {}
            Some("textDocument/didOpen") => {
                let uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                let text = message["params"]["textDocument"]["text"]
                    .as_str()
                    .unwrap_or_default();
                docs.insert(uri.to_string(), text.to_string());
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": {
                        "uri": uri,
                        "diagnostics": [default_diagnostic()]
                    }
                }))?;
            }
            Some("textDocument/didChange") => {
                let uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                let text = message["params"]["contentChanges"][0]["text"]
                    .as_str()
                    .unwrap_or_default();
                docs.insert(uri.to_string(), text.to_string());
            }
            Some("textDocument/didClose") => {}
            Some("textDocument/definition") | Some("textDocument/implementation") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = message["params"]["textDocumentPositionParams"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "uri": uri,
                        "range": default_range()
                    }]
                }))?;
            }
            Some("textDocument/references") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [
                        { "uri": uri, "range": default_range() },
                        {
                            "uri": uri,
                            "range": {
                                "start": { "line": 2, "character": 5 },
                                "end": { "line": 2, "character": 10 }
                            }
                        }
                    ]
                }))?;
            }
            Some("textDocument/hover") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "contents": { "kind": "markdown", "value": "```rust\nfn compute(value: i32) -> i32\n```" },
                        "range": default_range()
                    }
                }))?;
            }
            Some("textDocument/documentSymbol") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "name": "compute",
                        "kind": 12,
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 2, "character": 1 }
                        },
                        "selectionRange": default_range(),
                        "children": []
                    }]
                }))?;
            }
            Some("workspace/symbol") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = docs
                    .keys()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| "file:///tmp/mock.rs".into());
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "name": "compute",
                        "kind": 12,
                        "location": { "uri": uri, "range": default_range() },
                        "containerName": "main"
                    }]
                }))?;
            }
            Some("textDocument/prepareCallHierarchy") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = message["params"]["textDocumentPositionParams"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "name": "compute",
                        "kind": 12,
                        "detail": "fn compute(value: i32) -> i32",
                        "uri": uri,
                        "range": default_range(),
                        "selectionRange": default_range()
                    }]
                }))?;
            }
            Some("callHierarchy/incomingCalls") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let item = message["params"]["item"].clone();
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "from": item,
                        "fromRanges": [default_range()]
                    }]
                }))?;
            }
            Some("callHierarchy/outgoingCalls") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let item = message["params"]["item"].clone();
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "to": item,
                        "fromRanges": [default_range()]
                    }]
                }))?;
            }
            Some("textDocument/codeAction") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [code_action_for(uri)]
                }))?;
            }
            Some("codeAction/resolve") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = message["params"]["data"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": resolved_code_action(uri)
                }))?;
            }
            Some("textDocument/rename") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                let new_name = message["params"]["newName"].as_str().unwrap_or("renamed");
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "changes": {
                            uri: [{
                                "range": default_range(),
                                "newText": new_name
                            }]
                        }
                    }
                }))?;
            }
            Some("textDocument/formatting") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                let text = docs
                    .get(uri)
                    .cloned()
                    .unwrap_or_default()
                    .replace("todo!()", "42");
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 2, "character": 1 }
                        },
                        "newText": text
                    }]
                }))?;
            }
            Some("textDocument/rangeFormatting") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "range": {
                            "start": { "line": 1, "character": 4 },
                            "end": { "line": 1, "character": 11 }
                        },
                        "newText": "42"
                    }]
                }))?;
            }
            Some("textDocument/inlayHint") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "position": { "line": 0, "character": 20 },
                        "label": " -> i32"
                    }]
                }))?;
            }
            Some("textDocument/semanticTokens/full") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "data": [0, 3, 7, 0, 1]
                    }
                }))?;
            }
            Some("textDocument/signatureHelp") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "signatures": [{
                            "label": "compute(value: i32) -> i32",
                            "parameters": [{ "label": "value: i32" }]
                        }],
                        "activeSignature": 0,
                        "activeParameter": 0
                    }
                }))?;
            }
            Some("textDocument/prepareTypeHierarchy") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = message["params"]["textDocumentPositionParams"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or_default();
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [{
                        "name": "ComputeType",
                        "kind": 5,
                        "detail": "struct ComputeType",
                        "uri": uri,
                        "range": default_range(),
                        "selectionRange": default_range()
                    }]
                }))?;
            }
            Some("typeHierarchy/supertypes") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let mut item = message["params"]["item"].clone();
                item["name"] = Value::String("BaseType".into());
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [item]
                }))?;
            }
            Some("typeHierarchy/subtypes") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let mut item = message["params"]["item"].clone();
                item["name"] = Value::String("ChildType".into());
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [item]
                }))?;
            }
            Some("textDocument/linkedEditingRange") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "ranges": [
                            default_range(),
                            {
                                "start": { "line": 2, "character": 3 },
                                "end": { "line": 2, "character": 8 }
                            }
                        ]
                    }
                }))?;
            }
            Some("textDocument/diagnostic") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "kind": "full",
                        "items": [default_diagnostic()]
                    }
                }))?;
            }
            Some("workspace/diagnostic") => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                let uri = docs
                    .keys()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| "file:///tmp/mock.rs".into());
                write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "items": [{
                            "uri": uri,
                            "kind": "full",
                            "items": [default_diagnostic()]
                        }]
                    }
                }))?;
            }
            _ => {
                if let Some(id) = message.get("id").cloned() {
                    write_message(&json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": Value::Null
                    }))?;
                }
            }
        }
    }

    Ok(())
}
