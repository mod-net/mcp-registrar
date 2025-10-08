use mcp_registrar::servers::mcp_registrar::{McpRegistrarServer, RegisterServerRequest};
use mcp_registrar::transport::McpServer;

#[tokio::test]
async fn test_registrar_register_server() {
    let registrar = McpRegistrarServer::new();

    // Create a test request
    let request = RegisterServerRequest {
        name: "TestServer".to_string(),
        description: "A test server".to_string(),
        version: "1.0.0".to_string(),
        schema_url: Some("http://example.com/schema".to_string()),
        capabilities: vec!["test".to_string()],
        endpoint: "http://localhost:8080".to_string(),
    };

    // Convert request to JSON
    let params = serde_json::to_value(request).unwrap();

    // Call the register method
    let result = registrar.handle("RegisterServer", params).await.unwrap();

    // Verify the result
    let response: serde_json::Value = result;
    assert!(response.get("server_id").is_some());
    let server_id = response["server_id"].as_str().unwrap();

    // List the servers and verify our server is in the list
    let list_result = registrar
        .handle("ListServers", serde_json::json!({}))
        .await
        .unwrap();
    let servers = list_result["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["id"].as_str().unwrap(), server_id);
    assert_eq!(servers[0]["name"].as_str().unwrap(), "TestServer");
    assert_eq!(servers[0]["status"].as_str().unwrap(), "Active");
}

#[tokio::test]
async fn test_registrar_get_server() {
    let registrar = McpRegistrarServer::new();

    // Register a server first
    let request = RegisterServerRequest {
        name: "TestServer".to_string(),
        description: "A test server".to_string(),
        version: "1.0.0".to_string(),
        schema_url: None,
        capabilities: vec![],
        endpoint: "http://localhost:8080".to_string(),
    };

    let register_result = registrar
        .handle("RegisterServer", serde_json::to_value(request).unwrap())
        .await
        .unwrap();
    let server_id = register_result["server_id"].as_str().unwrap().to_string();

    // Get the server
    let get_params = serde_json::json!({ "id": server_id });
    let get_result = registrar.handle("GetServer", get_params).await.unwrap();

    // Verify the result
    assert_eq!(get_result["id"].as_str().unwrap(), server_id);
    assert_eq!(get_result["name"].as_str().unwrap(), "TestServer");
}

#[tokio::test]
async fn test_registrar_update_server_status() {
    let registrar = McpRegistrarServer::new();

    // Register a server first
    let request = RegisterServerRequest {
        name: "TestServer".to_string(),
        description: "A test server".to_string(),
        version: "1.0.0".to_string(),
        schema_url: None,
        capabilities: vec![],
        endpoint: "http://localhost:8080".to_string(),
    };

    let register_result = registrar
        .handle("RegisterServer", serde_json::to_value(request).unwrap())
        .await
        .unwrap();
    let server_id = register_result["server_id"].as_str().unwrap().to_string();

    // Update the server status
    let update_params = serde_json::json!({
        "id": server_id,
        "status": "inactive"
    });

    let update_result = registrar
        .handle("UpdateServerStatus", update_params)
        .await
        .unwrap();

    // Verify the status was updated - serde will serialize the enum value
    assert_eq!(update_result["status"].as_str().unwrap(), "Inactive");

    // Get the server to make sure the status is persisted
    let get_params = serde_json::json!({ "id": server_id });
    let get_result = registrar.handle("GetServer", get_params).await.unwrap();
    assert_eq!(get_result["status"].as_str().unwrap(), "Inactive");
}

#[tokio::test]
async fn test_registrar_unregister_server() {
    let registrar = McpRegistrarServer::new();

    // Register a server first
    let request = RegisterServerRequest {
        name: "TestServer".to_string(),
        description: "A test server".to_string(),
        version: "1.0.0".to_string(),
        schema_url: None,
        capabilities: vec![],
        endpoint: "http://localhost:8080".to_string(),
    };

    let register_result = registrar
        .handle("RegisterServer", serde_json::to_value(request).unwrap())
        .await
        .unwrap();
    let server_id = register_result["server_id"].as_str().unwrap().to_string();

    // Unregister the server
    let unregister_params = serde_json::json!({ "id": server_id });
    let unregister_result = registrar
        .handle("UnregisterServer", unregister_params)
        .await
        .unwrap();

    // Verify the unregister was successful
    assert_eq!(unregister_result["success"].as_bool().unwrap(), true);

    // Try to get the server, which should now fail
    let get_params = serde_json::json!({ "id": server_id });
    let get_result = registrar.handle("GetServer", get_params).await;

    assert!(get_result.is_err());
}

#[tokio::test]
async fn test_registrar_heartbeat() {
    let registrar = McpRegistrarServer::new();

    // Register a server first
    let request = RegisterServerRequest {
        name: "TestServer".to_string(),
        description: "A test server".to_string(),
        version: "1.0.0".to_string(),
        schema_url: None,
        capabilities: vec![],
        endpoint: "http://localhost:8080".to_string(),
    };

    let register_result = registrar
        .handle("RegisterServer", serde_json::to_value(request).unwrap())
        .await
        .unwrap();
    let server_id = register_result["server_id"].as_str().unwrap().to_string();

    // First, make the server inactive
    let update_params = serde_json::json!({
        "id": server_id,
        "status": "inactive"
    });
    registrar
        .handle("UpdateServerStatus", update_params)
        .await
        .unwrap();

    // Send a heartbeat to make it active again
    let heartbeat_params = serde_json::json!({ "id": server_id });
    let heartbeat_result = registrar
        .handle("Heartbeat", heartbeat_params)
        .await
        .unwrap();

    // Verify the heartbeat was successful
    assert_eq!(heartbeat_result["success"].as_bool().unwrap(), true);

    // Get the server to verify its status is now active
    let get_params = serde_json::json!({ "id": server_id });
    let get_result = registrar.handle("GetServer", get_params).await.unwrap();
    assert_eq!(get_result["status"].as_str().unwrap(), "Active");
}
