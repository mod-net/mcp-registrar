use mcp_registrar::models::tool::ToolInvocation;
use mcp_registrar::servers::tool_registry::{InvokeToolRequest, ToolRegistryServer};
use mcp_registrar::transport::McpServer;

#[tokio::test]
async fn invoke_echo_process_tool_via_registry() {
    let registry = ToolRegistryServer::new();
    // Load manifests from tools/**/tool.json and persist to storage
    registry.initialize().await.expect("init tool registry");

    // List tools and find the echo tool that came from manifest loading
    let list_resp = registry
        .handle("ListTools", serde_json::json!({}))
        .await
        .expect("list tools");
    let tools = list_resp["tools"].as_array().expect("tools array");
    assert!(
        tools.len() >= 1,
        "expected at least one tool loaded from manifests"
    );
    let echo = tools
        .iter()
        .find(|t| t["id"].as_str() == Some("echo") || t["name"].as_str() == Some("Echo"))
        .expect("echo tool present");
    let echo_id = echo["id"].as_str().unwrap().to_string();

    // Invoke echo with a sample payload and assert the process executor path returns the echoed text
    let payload = "hello via process";
    let invocation = ToolInvocation {
        tool_id: echo_id,
        parameters: serde_json::json!({"text": payload}),
        context: None,
    };
    let req = InvokeToolRequest { invocation };
    let resp = registry
        .handle("InvokeTool", serde_json::to_value(req).unwrap())
        .await
        .expect("invoke echo");

    // Response is a ToolInvocationResult with `result` being the tool's returned JSON
    let content = resp["result"]["result"]["content"]
        .as_array()
        .expect("content array");
    assert_eq!(content.len(), 1);
    let text = content[0]["text"].as_str().unwrap();
    assert_eq!(text, payload);
}

#[tokio::test]
async fn invoke_echo_with_missing_param_fails_validation() {
    let registry = ToolRegistryServer::new();
    registry.initialize().await.expect("init tool registry");

    // Find echo tool id
    let list_resp = registry
        .handle("ListTools", serde_json::json!({}))
        .await
        .unwrap();
    let tools = list_resp["tools"].as_array().unwrap();
    let echo = tools
        .iter()
        .find(|t| t["id"].as_str() == Some("echo") || t["name"].as_str() == Some("Echo"))
        .unwrap();
    let echo_id = echo["id"].as_str().unwrap().to_string();

    // Omit required 'text' parameter; expect handler error
    let invocation = ToolInvocation {
        tool_id: echo_id,
        parameters: serde_json::json!({}),
        context: None,
    };
    let req = InvokeToolRequest { invocation };
    let err = registry
        .handle("InvokeTool", serde_json::to_value(req).unwrap())
        .await
        .err()
        .expect("expected schema validation error");
    // Accept any error; message shape may vary by error wrapping
    assert!(!err.to_string().is_empty());
}
