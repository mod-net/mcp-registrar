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
    }

    Ok(())
}
