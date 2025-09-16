use registry_scheduler::servers::tool_registry::{ToolRegistryServer, InvokeToolRequest};
use registry_scheduler::servers::prompt_registry::PromptRegistryServer;
use registry_scheduler::servers::resource_registry::ResourceRegistryServer;
use registry_scheduler::McpServer;
use registry_scheduler::models::tool::ToolInvocation;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn write_error(stdout: &mut impl Write, id: &serde_json::Value, code: i64, message: &str) -> io::Result<()> {
    let mut obj = serde_json::Map::new();
    obj.insert("jsonrpc".into(), Value::String("2.0".into()));
    if !id.is_null() { obj.insert("id".into(), id.clone()); }
    obj.insert("error".into(), json!({ "code": code, "message": message }));
    writeln!(stdout, "{}", Value::Object(obj))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize registry (loads manifests and sets up executors)
    let rt = tokio::runtime::Runtime::new()?;
    let registry = ToolRegistryServer::new();
    rt.block_on(registry.initialize())?;
    let prompt_registry = PromptRegistryServer::new();
    let resource_registry = ResourceRegistryServer::new();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line_res in stdin.lock().lines() {
        let line = match line_res {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };

        let frame: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = write_error(&mut stdout, &Value::Null, -32700, &format!("Parse error: {}", e));
                continue;
            }
        };

        let id = frame.get("id").cloned().unwrap_or(Value::Null);
        let method_val = frame.get("method").cloned().unwrap_or(Value::Null);
        let method = method_val.as_str().unwrap_or("");
        let params = frame.get("params").cloned().unwrap_or(Value::Null);

        // Basic JSON-RPC validation
        if method.is_empty() {
            let _ = write_error(&mut stdout, &id, -32600, "Invalid Request: missing method");
            continue;
        }

        // Handle notifications with no response
        if method == "notifications/initialized" {
            continue;
        }

        let result = rt.block_on(async {
            match method {
                "initialize" => {
                    // Validate required params
                    let client = params.get("clientInfo").and_then(|c| c.as_object());
                    let _cap = params.get("capabilities").and_then(|c| c.as_object());
                    if client.is_none() { return Err("Invalid params: missing clientInfo".into()); }
                    let proto = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("");
                    // For now we only speak 2024-11-05 strictly
                    let protocol = "2024-11-05";
                    if !proto.is_empty() && proto != protocol {
                        return Err(format!("Invalid params: unsupported protocolVersion {}, expected {}", proto, protocol));
                    }
                    Ok(json!({
                        "serverInfo": { "name": "registry-scheduler", "version": env!("CARGO_PKG_VERSION") },
                        "capabilities": { "tools": {}, "prompts": {}, "resources": {} },
                        "protocolVersion": protocol
                    }))
                }
                "tools/list" => {
                    // List tools from registry and map to MCP Tool format
                    let tools = registry.list_tools().await.map_err(|e| e.to_string())?;
                    let items: Vec<Value> = tools.into_iter().map(|t| {
                        json!({
                            "name": t.id,
                            "description": t.description,
                            "inputSchema": t.parameters_schema.unwrap_or(json!({"type":"object"}))
                        })
                    }).collect();
                    Ok(json!({ "tools": items, "nextCursor": null }))
                }
                "tools/call" => {
                    let name = params.get("name").and_then(|v| v.as_str()).ok_or("Invalid params: missing name")?.to_string();
                    let arguments = params.get("arguments").cloned().unwrap_or(Value::Object(Default::default()));
                    // Invoke registry path using tool id = name
                    let inv = ToolInvocation { tool_id: name, parameters: arguments, context: None };
                    let req = InvokeToolRequest { invocation: inv };
                    let v = registry.handle("InvokeTool", serde_json::to_value(req).unwrap()).await
                        .map_err(|e| e.to_string())?;
                    // Extract tool output (CallToolResult payload) for MCP result
                    let inner = v.get("result").and_then(|r| r.get("result")).cloned()
                        .ok_or("Internal error: malformed invocation result")?;
                    Ok(inner)
                }
                "prompts/list" => {
                    // Return prompts in MCP shape; current registry is in-memory and may be empty
                    let list = prompt_registry
                        .handle("ListPrompts", json!({}))
                        .await
                        .map_err(|e| e.to_string())?;
                    // Map prompts -> {name, description, arguments[]}
                    let prompts = list["prompts"].as_array().cloned().unwrap_or_default();
                    let items: Vec<Value> = prompts
                        .into_iter()
                        .map(|p| {
                            let name = p["name"].clone();
                            let description = p["description"].clone();
                            // Derive arguments from variables_schema if present
                            let mut args: Vec<Value> = Vec::new();
                            if let Some(schema) = p.get("variables_schema") {
                                let required = schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default();
                                let props = schema.get("properties").and_then(|o| o.as_object()).cloned().unwrap_or_default();
                                for (k, v) in props.iter() {
                                    let req = required.iter().any(|r| r.as_str() == Some(k));
                                    let desc = v.get("description").and_then(|d| d.as_str()).unwrap_or("");
                                    args.push(json!({"name": k, "required": req, "description": desc }));
                                }
                            }
                            json!({"name": name, "description": description, "arguments": args})
                        })
                        .collect();
                    Ok(json!({"prompts": items, "nextCursor": null}))
                }
                "prompts/get" => {
                    let name = params.get("name").and_then(|v| v.as_str()).ok_or("Invalid params: missing name")?.to_string();
                    let args = params.get("arguments").cloned().unwrap_or(json!({}));
                    // Find prompt by name via list; then render
                    let list = prompt_registry.handle("ListPrompts", json!({})).await.map_err(|e| e.to_string())?;
                    let prompts = list["prompts"].as_array().cloned().unwrap_or_default();
                    let prompt = prompts.into_iter().find(|p| p["name"].as_str() == Some(&name))
                        .ok_or_else(|| format!("Prompt not found: {}", name))?;
                    // Validate against variables_schema if present
                    if let Some(schema) = prompt.get("variables_schema") {
                        if let Ok(compiled) = jsonschema::JSONSchema::compile(schema) {
                            if let Err(_e) = compiled.validate(&args) {
                                return Err("Invalid params: prompt arguments failed schema".into());
                            }
                        }
                    }
                    let id = prompt["id"].clone();
                    let render = json!({"render": {"prompt_id": id, "variables": args}});
                    let rendered = prompt_registry.handle("RenderPrompt", render).await.map_err(|e| e.to_string())?;
                    let text = rendered["result"]["rendered_text"].as_str().unwrap_or("").to_string();
                    Ok(json!({"content": [{"type":"text","text": text}], "isError": false}))
                }
                "resources/list" => {
                    // Map resources to MCP shape: {uri, name, mimeType}
                    let list = resource_registry.handle("ListResources", json!({})).await.map_err(|e| e.to_string())?;
                    let items: Vec<Value> = list["resources"].as_array().cloned().unwrap_or_default().into_iter().map(|r| {
                        let id = r["id"].as_str().unwrap_or("");
                        let name = r["name"].as_str().unwrap_or("");
                        json!({"uri": format!("registry://resource/{}", id), "name": name, "mimeType": "text/plain"})
                    }).collect();
                    Ok(json!({"resources": items, "nextCursor": null}))
                }
                "resources/read" => {
                    let uri = params.get("uri").and_then(|v| v.as_str()).ok_or("Invalid params: missing uri")?;
                    let id = uri.strip_prefix("registry://resource/").ok_or("Invalid params: unsupported uri scheme")?;
                    let parameters = params.get("parameters").cloned().unwrap_or(json!({}));
                    if !parameters.is_object() { return Err("Invalid params: parameters must be an object".into()); }
                    // Query the resource via registry
                    let query = json!({"query": {"resource_id": id, "parameters": parameters }});
                    let qr = resource_registry
                        .handle("QueryResource", query)
                        .await
                        .map_err(|e| e.to_string())?;
                    let result = qr.get("result").cloned().unwrap_or(json!({}));

                    // Map result to MCP contents semantics
                    // Supported shapes:
                    // - { mimeType, text }
                    // - { mimeType, data }  // data is base64
                    // - any JSON -> application/json text
                    let (mime, content_val) = if let Some(obj) = result.as_object() {
                        match (obj.get("mimeType"), obj.get("text"), obj.get("data")) {
                            (Some(mt), Some(text), _) if mt.is_string() && text.is_string() => (
                                mt.as_str().unwrap().to_string(),
                                json!({"text": text.as_str().unwrap()}),
                            ),
                            (Some(mt), _, Some(data)) if mt.is_string() && data.is_string() => (
                                mt.as_str().unwrap().to_string(),
                                json!({"data": data.as_str().unwrap()}),
                            ),
                            _ => (
                                "application/json".to_string(),
                                json!({"text": serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())}),
                            ),
                        }
                    } else {
                        (
                            "application/json".to_string(),
                            json!({"text": serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())}),
                        )
                    };

                    let mut item = serde_json::Map::new();
                    item.insert("uri".into(), Value::String(uri.to_string()));
                    item.insert("mimeType".into(), Value::String(mime));
                    if let Some(t) = content_val.get("text") { item.insert("text".into(), t.clone()); }
                    if let Some(d) = content_val.get("data") { item.insert("data".into(), d.clone()); }
                    Ok(json!({"contents": [Value::Object(item)]}))
                }
                "metrics/get" => {
                    // Return executor/tool metrics snapshot
                    let (inv, err, total_ms, max_ms, total_bytes) = registry_scheduler::monitoring::TOOL_METRICS.snapshot();
                    Ok(json!({
                        "tool": {
                            "invocations": inv,
                            "errors": err,
                            "totalDurationMs": total_ms,
                            "maxDurationMs": max_ms,
                            "totalBytes": total_bytes
                        }
                    }))
                }
                _ => Err(format!("Method not found: {}", method)),
            }
        });

        match result {
            Ok(res) => {
                let mut obj = serde_json::Map::new();
                obj.insert("jsonrpc".into(), Value::String("2.0".into()));
                if !id.is_null() { obj.insert("id".into(), id.clone()); }
                obj.insert("result".into(), res);
                writeln!(stdout, "{}", Value::Object(obj))?;
            }
            Err(msg) => {
                // Map common errors to JSON-RPC codes
                let code = if msg.starts_with("Method not found") { -32601 }
                    else if msg.starts_with("Invalid params") { -32602 }
                    else { -32603 };
                write_error(&mut stdout, &id, code, &msg)?;
            }
        }
        stdout.flush()?;
    }

    Ok(())
}
