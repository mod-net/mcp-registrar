use registry_scheduler::servers::tool_registry::ToolRegistryServer;
use registry_scheduler::transport::McpServer;
use serde_json::json;

#[tokio::test]
async fn wasm_echo_tool_invokes() {
    let server = ToolRegistryServer::new();
    server.initialize().await.expect("init");

    // List and ensure our wasm tool is present
    let list = server
        .handle("ListTools", json!({}))
        .await
        .expect("list");
    let tools = list["tools"].as_array().cloned().unwrap_or_default();
    if !tools.iter().any(|t| t["id"] == "echo-wasm") {
        eprintln!("echo-wasm not present; skipping wasm test");
        return;
    }

    // Invoke the wasm tool (ignores args)
    let inv = json!({
        "invocation": { "tool_id": "echo-wasm", "parameters": {} }
    });
    let res = server
        .handle("InvokeTool", inv)
        .await
        .expect("invoke");
    let out = res["result"]["result"].clone();
    assert_eq!(out["isError"], json!(false));
    let text = out["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("hello from wasm"));
}

