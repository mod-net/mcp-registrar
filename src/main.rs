use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::fmt::init;

use registry_scheduler::cli::cli_parser::{parse_args, Command};
use registry_scheduler::models::tool::ToolInvocation;
use registry_scheduler::servers::mcp_registrar::{McpRegistrarServer, RegisterServerRequest};
use registry_scheduler::servers::prompt_registry::PromptRegistryServer;
use registry_scheduler::servers::resource_registry::ResourceRegistryServer;
use registry_scheduler::servers::task_executor::TaskExecutor;
use registry_scheduler::servers::task_scheduler::{DummyToolRegistry, TaskSchedulerServer};
use registry_scheduler::servers::tool_registry::{
    InvokeToolRequest, InvokeToolResponse, ListToolsRequest, ListToolsResponse,
    RegisterToolRequest, RegisterToolResponse, ToolRegistryServer,
};
use registry_scheduler::transport::stdio_transport::{StdioTransportServer, TransportServer};
use registry_scheduler::utils::task_storage::{FileTaskStorage, TaskStorage};
use registry_scheduler::McpServer;
use registry_scheduler::TaskMetricsCollector;
use std::fs;
use std::io::{self, BufRead};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    init();

    // Parse command line arguments
    let command = parse_args();

    match command {
        Command::RegisterTool => {
            // Create a new tool registry server
            let registry = ToolRegistryServer::new();

            // Initialize the storage
            if let Err(e) = registry.initialize().await {
                eprintln!("Failed to initialize tool registry: {}", e);
                return Ok(());
            }

            // First, register the server
            let server_request = RegisterServerRequest {
                name: "Example Tool Registry".to_string(),
                description: "A sample tool registry server".to_string(),
                version: "1.0.0".to_string(),
                schema_url: None,
                capabilities: vec!["tool_registration".to_string()],
                endpoint: "stdio://tool_registry".to_string(),
            };

            // Register the server using the full RegisterServerRequest
            let server_result = registry
                .handle("RegisterServer", serde_json::to_value(server_request)?)
                .await;
            match server_result {
                Ok(_) => {
                    // Now prepare a tool registration request using the server name as the server_id
                    let request = RegisterToolRequest {
                        name: "ExampleTool".to_string(),
                        description: "A sample tool for demonstration".to_string(),
                        version: "1.0.0".to_string(),
                        server_id: "Example Tool Registry".to_string(), // Use the server name as server_id
                        categories: vec!["utility".to_string()],
                        parameters_schema: Some(json!({
                            "type": "object",
                            "properties": {
                                "input": {"type": "string"}
                            }
                        })),
                        returns_schema: Some(json!({
                            "type": "object",
                            "properties": {
                                "output": {"type": "string"}
                            }
                        })),
                        metadata: Some(HashMap::from([
                            ("author".to_string(), json!("Cascade AI")),
                            ("license".to_string(), json!("MIT")),
                        ])),
                    };

                    // Register the tool via handle method
                    let result = registry
                        .handle("RegisterTool", serde_json::to_value(request)?)
                        .await;
                    match result {
                        Ok(response) => {
                            let tool_response: RegisterToolResponse =
                                serde_json::from_value(response)?;
                            println!(
                                "Tool registered successfully. Tool ID: {}",
                                tool_response.tool_id
                            );

                            // List tools via handle method
                            let list_request = ListToolsRequest {
                                server_id: Some("Example Tool Registry".to_string()),
                                category: None,
                            };
                            let list_result = registry
                                .handle("ListTools", serde_json::to_value(list_request)?)
                                .await;

                            match list_result {
                                Ok(tools_value) => {
                                    let list_response: ListToolsResponse =
                                        serde_json::from_value(tools_value)?;
                                    println!("Registered Tools:");
                                    for tool in list_response.tools {
                                        println!("- {} ({})", tool.name, tool.description);
                                    }
                                }
                                Err(e) => eprintln!("Failed to list tools: {}", e),
                            }
                        }
                        Err(e) => eprintln!("Failed to register tool: {}", e),
                    }
                }
                Err(e) => eprintln!("Failed to register server: {}", e),
            }
        }
        Command::StartRegistrar => {
            let registrar = McpRegistrarServer::new();
            tracing::info!("Starting MCP Registrar server with stdio transport");
            let transport = StdioTransportServer::new(registrar);
            transport.serve().await?;
        }
        Command::StartToolRegistry => {
            let registry = ToolRegistryServer::new();
            tracing::info!("Starting Tool Registry server with stdio transport");
            let transport = StdioTransportServer::new(registry);
            transport.serve().await?;
        }
        Command::StartResourceRegistry => {
            let registry = ResourceRegistryServer::new();
            tracing::info!("Starting Resource Registry server with stdio transport");
            let transport = StdioTransportServer::new(registry);
            transport.serve().await?;
        }
        Command::StartPromptRegistry => {
            let registry = PromptRegistryServer::new();
            tracing::info!("Starting Prompt Registry server with stdio transport");
            let transport = StdioTransportServer::new(registry);
            transport.serve().await?;
        }
        Command::StartTaskScheduler => {
            let storage: Arc<dyn TaskStorage> =
                Arc::new(FileTaskStorage::new(PathBuf::from("tasks.json")));
            let scheduler = TaskSchedulerServer::new(
                Arc::new(TaskExecutor::new(
                    Arc::new(DummyToolRegistry {}),
                    storage.clone(),
                    Arc::new(TaskMetricsCollector::new()),
                )),
                storage.clone(),
                Arc::new(TaskMetricsCollector::new()),
            );
            tracing::info!("Starting Task Scheduler server with stdio transport");
            let transport = StdioTransportServer::new(scheduler);
            transport.serve().await?;
        }
        Command::ListTools => {
            // Create a new tool registry server
            let registry = ToolRegistryServer::new();

            // Initialize the storage
            if let Err(e) = registry.initialize().await {
                eprintln!("Failed to initialize tool registry: {}", e);
                return Ok(());
            }

            // First, register the server
            let server_request = RegisterServerRequest {
                name: "Example Tool Registry".to_string(),
                description: "A sample tool registry server".to_string(),
                version: "1.0.0".to_string(),
                schema_url: None,
                capabilities: vec!["tool_registration".to_string()],
                endpoint: "stdio://tool_registry".to_string(),
            };

            // Register the server using the full RegisterServerRequest
            let server_result = registry
                .handle("RegisterServer", serde_json::to_value(server_request)?)
                .await;

            match server_result {
                Ok(_) => {
                    // List tools via handle method
                    let list_request = ListToolsRequest {
                        server_id: Some("Example Tool Registry".to_string()),
                        category: None,
                    };
                    let list_result = registry
                        .handle("ListTools", serde_json::to_value(list_request)?)
                        .await;

                    match list_result {
                        Ok(tools_value) => {
                            let list_response: ListToolsResponse =
                                serde_json::from_value(tools_value)?;
                            println!("Registered Tools:");
                            if list_response.tools.is_empty() {
                                println!("No tools registered.");
                            } else {
                                for tool in list_response.tools {
                                    println!("- {} ({})", tool.name, tool.description);
                                    println!("  ID: {}", tool.id);
                                    println!("  Version: {}", tool.version);
                                    println!("  Categories: {}", tool.categories.join(", "));
                                    if !tool.metadata.is_empty() {
                                        println!("  Metadata:");
                                        for (key, value) in tool.metadata {
                                            println!("    {}: {}", key, value);
                                        }
                                    }
                                    println!();
                                }
                            }
                        }
                        Err(e) => eprintln!("Failed to list tools: {}", e),
                    }
                }
                Err(e) => eprintln!("Failed to register server: {}", e),
            }
        }
        Command::ExecuteTool {
            tool_id,
            parameters,
        } => {
            // Create a new tool registry server
            let registry = ToolRegistryServer::new();

            // Initialize the storage
            if let Err(e) = registry.initialize().await {
                eprintln!("Failed to initialize tool registry: {}", e);
                return Ok(());
            }

            // First, register the server
            let server_request = RegisterServerRequest {
                name: "Example Tool Registry".to_string(),
                description: "A sample tool registry server".to_string(),
                version: "1.0.0".to_string(),
                schema_url: None,
                capabilities: vec!["tool_registration".to_string()],
                endpoint: "stdio://tool_registry".to_string(),
            };

            // Register the server using the full RegisterServerRequest
            let server_result = registry
                .handle("RegisterServer", serde_json::to_value(server_request)?)
                .await;

            match server_result {
                Ok(_) => {
                    // Parse the parameters as JSON
                    let parameters_json = serde_json::from_str(&parameters)?;

                    // Create the tool invocation request
                    let invocation = ToolInvocation {
                        tool_id,
                        parameters: parameters_json,
                        context: None,
                    };

                    let invoke_request = InvokeToolRequest { invocation };

                    // Execute the tool via handle method
                    let result = registry
                        .handle("InvokeTool", serde_json::to_value(invoke_request)?)
                        .await;

                    match result {
                        Ok(response) => {
                            let invoke_response: InvokeToolResponse =
                                serde_json::from_value(response)?;
                            println!("Tool execution result:");
                            println!("Status: {}", invoke_response.result.result["status"]);
                            println!("Message: {}", invoke_response.result.result["message"]);
                            println!("Tool ID: {}", invoke_response.result.result["tool_id"]);
                            println!("Tool Name: {}", invoke_response.result.result["tool_name"]);
                            println!("Started at: {}", invoke_response.result.started_at);
                            println!("Completed at: {}", invoke_response.result.completed_at);
                        }
                        Err(e) => eprintln!("Failed to execute tool: {}", e),
                    }
                }
                Err(e) => eprintln!("Failed to register server: {}", e),
            }
        }
        Command::ScaffoldModule {
            name,
            runtime,
            version,
            description,
            categories,
            deps,
            uv_args,
            command,
            args,
            adapter,
            adapter_lang,
            adapter_mode,
            adapter_arg_style,
        } => {
            // Create tools/<name>/ directory
            let base = PathBuf::from("tools").join(&name);
            fs::create_dir_all(&base)?;

            // Build categories array
            let cats: Vec<String> = categories
                .split(|c| c == ',' || c == ' ')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();

            // Construct manifest JSON
            let mut manifest = serde_json::json!({
                "id": name,
                "name": name,
                "version": version,
                "runtime": runtime,
                "description": if description.is_empty() { format!("{} module", name) } else { description.clone() },
                "schema": {
                    "parameters": {
                        "type": "object",
                        "properties": {"text": {"type": "string"}},
                        "required": ["text"],
                        "additionalProperties": false
                    },
                    "returns": {"type": "object"}
                },
                "policy": {
                    "timeout_ms": 8000,
                    "memory_bytes": 134217728u64,
                    "cpu_time_ms": 2000,
                    "max_output_bytes": 262144,
                    "network": "deny",
                    "fs": {"preopen_tmp": false}
                },
                "metadata": {"categories": cats}
            });

            match runtime.as_str() {
                "python-uv-script" => {
                    // Write script with PEP 723 header
                    let script_path = base.join(format!("{}.py", name));
                    let deps_list: Vec<&str> = deps
                        .split(|c| c == ',' || c == ' ')
                        .filter(|s| !s.is_empty())
                        .collect();
                    let mut header = String::from("# /// script\n# requires-python = \">=3.10\"\n# dependencies = [\n");
                    for d in &deps_list {
                        header.push_str(&format!("#   \"{}\",\n", d));
                    }
                    header.push_str("# ]\n# ///\n\n");
                    let body = r#"import sys, json

def main():
    line = sys.stdin.readline()
    try:
        payload = json.loads(line) if line else {"arguments": {}}
    except Exception as e:
        print(json.dumps({"isError": True, "error": f"invalid JSON: {e}"}))
        return
    args = payload.get("arguments", {})
    text = args.get("text", "")
    print(json.dumps({"echo": text}))

if __name__ == "__main__":
    main()
"#;
                    fs::write(&script_path, format!("{}{}", header, body))?;

                    // entry
                    let mut entry = serde_json::Map::new();
                    entry.insert("script".into(), serde_json::Value::String(script_path.to_string_lossy().into_owned()));
                    let uv: Vec<String> = uv_args
                        .split(' ')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect();
                    if !uv.is_empty() {
                        entry.insert("uv_args".into(), serde_json::Value::Array(uv.into_iter().map(serde_json::Value::String).collect()));
                    }
                    manifest["entry"] = serde_json::Value::Object(entry);
                }
                "binary" => {
                    let default_args: Vec<String> = args
                        .split(' ')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect();

                    if adapter && adapter_lang == "python" {
                        // Generate a Python adapter that maps JSON arguments to CLI flags and wraps stdout into MCP content
                        let adapter_path = base.join("adapter.py");
                        let template = r#"#!/usr/bin/env python3
import sys, json, subprocess, shlex

BINARY_COMMAND = __CMD__
BINARY_DEFAULT_ARGS = __DARGS__
ARG_STYLE = __ARG_STYLE__  # 'gnu' or 'posix'
MODE = __MODE__  # 'auto', 'text', or 'json'

def build_argv(arguments: dict):
    argv = [BINARY_COMMAND, *BINARY_DEFAULT_ARGS]
    for key, val in arguments.items():
        flag = f"--{key}" if ARG_STYLE == 'gnu' else f"-{key}"
        if isinstance(val, bool):
            if val:
                argv.append(flag)
        elif isinstance(val, (int, float, str)):
            argv.extend([flag, str(val)])
        elif isinstance(val, list):
            for item in val:
                if isinstance(item, bool):
                    if item:
                        argv.append(flag)
                else:
                    argv.extend([flag, str(item)])
        else:
            argv.extend([flag, json.dumps(val)])
    return argv

def main():
    line = sys.stdin.readline()
    try:
        payload = json.loads(line) if line else {"arguments": {}}
    except Exception as e:
        print(json.dumps({"content":[{"type":"text","text":"invalid JSON: " + str(e)}],"isError":True}))
        return
    args = payload.get("arguments", {})
    argv = build_argv(args if isinstance(args, dict) else {})
    try:
        proc = subprocess.run(argv, capture_output=True, text=True)
        out = proc.stdout.strip()
        err = proc.stderr.strip()
        is_error = proc.returncode != 0
        if MODE in ("json", "auto"):
            try:
                parsed = json.loads(out) if out else None
                if parsed is not None:
                    print(json.dumps({"content":[{"type":"json","json": parsed}],"isError": is_error}))
                    return
            except Exception:
                if MODE == "json":
                    print(json.dumps({"content":[{"type":"text","text": out or err}],"isError": True}))
                    return
        # default text wrapping
        print(json.dumps({"content":[{"type":"text","text": out}],"isError": is_error}))
    except FileNotFoundError:
        print(json.dumps({"content":[{"type":"text","text":"binary not found: " + str(BINARY_COMMAND)}],"isError": True}))

if __name__ == "__main__":
    main()
"#;
                        let mut py = template.to_string();
                        py = py.replace("__CMD__", &serde_json::to_string(&command)?);
                        py = py.replace("__DARGS__", &serde_json::to_string(&default_args)?);
                        py = py.replace("__ARG_STYLE__", &serde_json::to_string(&adapter_arg_style)?);
                        py = py.replace("__MODE__", &serde_json::to_string(&adapter_mode)?);
                        fs::write(&adapter_path, py)?;

                        let mut entry = serde_json::Map::new();
                        entry.insert("command".into(), serde_json::Value::String("python3".to_string()));
                        entry.insert("args".into(), serde_json::json!([adapter_path.to_string_lossy()]));
                        manifest["entry"] = serde_json::Value::Object(entry);
                    } else {
                        // No adapter, run the binary directly
                        let mut entry = serde_json::Map::new();
                        entry.insert("command".into(), serde_json::Value::String(command.clone()));
                        if !default_args.is_empty() {
                            entry.insert("args".into(), serde_json::Value::Array(default_args.into_iter().map(serde_json::Value::String).collect()));
                        }
                        manifest["entry"] = serde_json::Value::Object(entry);
                    }
                }
                "process" => {
                    // Treat as pass-through for author-provided command/args (not typical via scaffolder)
                    manifest["entry"] = serde_json::json!({"command": "", "args": []});
                }
                other => {
                    eprintln!("Unsupported runtime for scaffolding: {}", other);
                    return Ok(());
                }
            }

            let manifest_path = base.join("tool.json");
            fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
            println!("Scaffolded module at {}", base.display());
        }
        Command::RegistryTool => {
            // Initialize in-process tool registry
            let registry = ToolRegistryServer::new();
            if let Err(e) = registry.initialize().await {
                eprintln!("{}", serde_json::to_string(&json!({"isError": true, "error": format!("init failed: {}", e)}))?);
                return Ok(());
            }

            // Read a single JSON line from stdin
            let mut line = String::new();
            let stdin = io::stdin();
            let _ = stdin.lock().read_line(&mut line);
            let payload: serde_json::Value = match serde_json::from_str(line.trim()) {
                Ok(v) => v,
                Err(e) => {
                    println!("{}", serde_json::to_string(&json!({"isError": true, "error": format!("invalid JSON: {}", e)}))?);
                    return Ok(());
                }
            };
            let args = payload.get("arguments").cloned().unwrap_or(json!({}));
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");

            match action {
                "list" | "list_tools" => {
                    match registry.handle("ListTools", json!({})).await {
                        Ok(res) => {
                            let tools = res.get("tools").cloned().unwrap_or(json!([]));
                            println!("{}", serde_json::to_string(&json!({"tools": tools}))?);
                        }
                        Err(e) => println!("{}", serde_json::to_string(&json!({"isError": true, "error": e.to_string()}))?),
                    }
                }
                "invoke" | "call" => {
                    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let parameters = args.get("arguments").cloned().unwrap_or(json!({}));
                    let req = json!({"invocation": {"tool_id": name, "parameters": parameters}});
                    match registry.handle("InvokeTool", req).await {
                        Ok(res) => {
                            // Return the underlying tool result if present
                            let out = res.get("result").and_then(|r| r.get("result")).cloned().unwrap_or(json!({}));
                            println!("{}", serde_json::to_string(&out)?);
                        }
                        Err(e) => println!("{}", serde_json::to_string(&json!({"isError": true, "error": e.to_string()}))?),
                    }
                }
                other => {
                    println!("{}", serde_json::to_string(&json!({"isError": true, "error": format!("unknown action: {}", other)}))?);
                }
            }
        }
    }

    Ok(())
}
