use mcp_registrar::models::tool::ToolInvocation;
use mcp_registrar::servers::tool_registry::{
    GetToolRequest, InvokeToolRequest, ListToolsRequest, RegisterToolRequest, ToolRegistryServer,
};
use mcp_registrar::transport::McpServer;
use std::collections::HashMap;

#[tokio::test]
async fn test_register_server() {
    let registry = ToolRegistryServer::new();
    // Register a server with simple id
    let server_params = serde_json::json!({"server_id": "test-server-1"});
    let register_result = registry
        .handle("RegisterServer", server_params)
        .await
        .unwrap();
    assert!(!register_result["server_id"].as_str().unwrap().is_empty());

    // Now we can register tools for this server
}

#[tokio::test]
async fn test_register_tool() {
    let registry = ToolRegistryServer::new();
    // Register a server first and use returned id
    let reg = registry
        .handle(
            "RegisterServer",
            serde_json::json!({"server_id": "test-server-2"}),
        )
        .await
        .unwrap();
    let server_id = reg["server_id"].as_str().unwrap().to_string();

    // Create a test tool
    let request = RegisterToolRequest {
        name: "TestTool".to_string(),
        description: "A test tool".to_string(),
        version: "1.0.0".to_string(),
        server_id,
        categories: vec!["testing".to_string()],
        parameters_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "param1": {"type": "string"},
                "param2": {"type": "number"}
            }
        })),
        returns_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "result": {"type": "string"}
            }
        })),
        metadata: Some(HashMap::new()),
    };

    // Convert request to JSON
    let params = serde_json::to_value(request).unwrap();

    // Call the register tool method
    let result = registry.handle("RegisterTool", params).await.unwrap();

    // Verify the result
    let tool_id = result["tool_id"].as_str().unwrap();
    assert!(!tool_id.is_empty());
}

#[tokio::test]
async fn test_list_tools() {
    let registry = ToolRegistryServer::new();
    let reg = registry
        .handle(
            "RegisterServer",
            serde_json::json!({"server_id": "test-server-3"}),
        )
        .await
        .unwrap();
    let server_id = reg["server_id"].as_str().unwrap().to_string();

    // Register a tool
    let request = RegisterToolRequest {
        name: "ListTool".to_string(),
        description: "A test tool for listing".to_string(),
        version: "1.0.0".to_string(),
        server_id,
        categories: vec!["listing".to_string()],
        parameters_schema: None,
        returns_schema: None,
        metadata: None,
    };

    registry
        .handle("RegisterTool", serde_json::to_value(request).unwrap())
        .await
        .unwrap();

    // List all tools
    let list_request = ListToolsRequest {
        server_id: None,
        category: None,
    };

    let list_result = registry
        .handle("ListTools", serde_json::to_value(list_request).unwrap())
        .await
        .unwrap();

    // Verify the result
    let tools = list_result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"].as_str().unwrap(), "ListTool");

    // Test filtering by category
    let filter_request = ListToolsRequest {
        server_id: None,
        category: Some("listing".to_string()),
    };

    let filter_result = registry
        .handle("ListTools", serde_json::to_value(filter_request).unwrap())
        .await
        .unwrap();
    let filtered_tools = filter_result["tools"].as_array().unwrap();
    assert_eq!(filtered_tools.len(), 1);

    // Test filtering by non-existent category
    let nonexistent_request = ListToolsRequest {
        server_id: None,
        category: Some("nonexistent".to_string()),
    };

    let nonexistent_result = registry
        .handle(
            "ListTools",
            serde_json::to_value(nonexistent_request).unwrap(),
        )
        .await
        .unwrap();
    let nonexistent_tools = nonexistent_result["tools"].as_array().unwrap();
    assert_eq!(nonexistent_tools.len(), 0);
}

#[tokio::test]
async fn test_get_tool() {
    let registry = ToolRegistryServer::new();
    let reg = registry
        .handle(
            "RegisterServer",
            serde_json::json!({"server_id": "test-server-4"}),
        )
        .await
        .unwrap();
    let server_id = reg["server_id"].as_str().unwrap().to_string();

    // Register a tool
    let request = RegisterToolRequest {
        name: "GetTool".to_string(),
        description: "A test tool for getting".to_string(),
        version: "1.0.0".to_string(),
        server_id,
        categories: vec!["getting".to_string()],
        parameters_schema: None,
        returns_schema: None,
        metadata: None,
    };

    let register_result = registry
        .handle("RegisterTool", serde_json::to_value(request).unwrap())
        .await
        .unwrap();
    let tool_id = register_result["tool_id"].as_str().unwrap().to_string();

    // Get the tool
    let get_request = GetToolRequest {
        tool_id: tool_id.clone(),
    };

    let get_result = registry
        .handle("GetTool", serde_json::to_value(get_request).unwrap())
        .await
        .unwrap();

    // Verify the result
    assert_eq!(get_result["tool"]["id"].as_str().unwrap(), tool_id);
    assert_eq!(get_result["tool"]["name"].as_str().unwrap(), "GetTool");
    assert_eq!(
        get_result["tool"]["description"].as_str().unwrap(),
        "A test tool for getting"
    );
}

#[tokio::test]
async fn test_invoke_tool() {
    // Use the real echo manifest to test invocation through executors
    let registry = ToolRegistryServer::new();
    registry.initialize().await.unwrap();

    // Find echo tool
    let list = registry
        .handle("ListTools", serde_json::json!({}))
        .await
        .unwrap();
    let tools = list["tools"].as_array().unwrap();
    let echo = tools
        .iter()
        .find(|t| t["id"].as_str() == Some("echo") || t["name"].as_str() == Some("Echo"))
        .unwrap();
    let echo_id = echo["id"].as_str().unwrap().to_string();

    let invocation = ToolInvocation {
        tool_id: echo_id,
        parameters: serde_json::json!({"text":"hello from test"}),
        context: None,
    };
    let req = InvokeToolRequest { invocation };
    let resp = registry
        .handle("InvokeTool", serde_json::to_value(req).unwrap())
        .await
        .unwrap();

    let content = resp["result"]["result"]["content"].as_array().unwrap();
    assert_eq!(content[0]["text"].as_str().unwrap(), "hello from test");
}
