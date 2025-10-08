use mcp_registrar::servers::resource_registry::{
    ResourceRegistryServer, RegisterResourceRequest, ListResourcesRequest, 
    GetResourceRequest, QueryResourceRequest
};
use mcp_registrar::transport::McpServer;
use mcp_registrar::models::resource::{ResourceType, ResourceQuery};
use std::collections::HashMap;

#[tokio::test]
async fn test_register_server() {
    let registry = ResourceRegistryServer::new();
    
    // Register a server first
    let server_params = serde_json::json!({
        "server_id": "test-resource-server-1",
        "endpoint": "http://localhost:8080/resource-server"
    });
    
    let register_result = registry.handle("RegisterServer", server_params).await.unwrap();
    assert_eq!(register_result["success"].as_bool().unwrap(), true);
}

#[tokio::test]
async fn test_register_resource() {
    let registry = ResourceRegistryServer::new();
    
    // Register a server first
    let server_params = serde_json::json!({
        "server_id": "test-resource-server-2",
        "endpoint": "http://localhost:8080/resource-server"
    });
    registry.handle("RegisterServer", server_params).await.unwrap();
    
    // Create a test resource
    let request = RegisterResourceRequest {
        name: "TestResource".to_string(),
        description: "A test resource".to_string(),
        resource_type: ResourceType::Database,
        server_id: "test-resource-server-2".to_string(),
        access_path: "/api/resources/test".to_string(),
        schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "name": {"type": "string"}
            }
        })),
        query_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "filter": {"type": "string"}
            }
        })),
        metadata: Some(HashMap::new()),
    };
    
    // Convert request to JSON
    let params = serde_json::to_value(request).unwrap();
    
    // Call the register resource method
    let result = registry.handle("RegisterResource", params).await.unwrap();
    
    // Verify the result
    let resource_id = result["resource_id"].as_str().unwrap();
    assert!(!resource_id.is_empty());
}

#[tokio::test]
async fn test_list_resources() {
    let registry = ResourceRegistryServer::new();
    
    // Register a server first
    let server_params = serde_json::json!({
        "server_id": "test-resource-server-3",
        "endpoint": "http://localhost:8080/resource-server"
    });
    registry.handle("RegisterServer", server_params).await.unwrap();
    
    // Register a resource
    let request = RegisterResourceRequest {
        name: "ListResource".to_string(),
        description: "A test resource for listing".to_string(),
        resource_type: ResourceType::FileSystem,
        server_id: "test-resource-server-3".to_string(),
        access_path: "/api/resources/list".to_string(),
        schema: None,
        query_schema: None,
        metadata: None,
    };
    
    registry.handle("RegisterResource", serde_json::to_value(request).unwrap()).await.unwrap();
    
    // List all resources
    let list_request = ListResourcesRequest {
        server_id: None,
        resource_type: None,
    };
    
    let list_result = registry.handle("ListResources", serde_json::to_value(list_request).unwrap()).await.unwrap();
    
    // Verify the result
    let resources = list_result["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["name"].as_str().unwrap(), "ListResource");
    
    // Test filtering by resource type
    let filter_request = ListResourcesRequest {
        server_id: None,
        resource_type: Some(ResourceType::FileSystem),
    };
    
    let filter_result = registry.handle("ListResources", serde_json::to_value(filter_request).unwrap()).await.unwrap();
    let filtered_resources = filter_result["resources"].as_array().unwrap();
    assert_eq!(filtered_resources.len(), 1);
    
    // Test filtering by non-matching resource type
    let nonmatching_request = ListResourcesRequest {
        server_id: None,
        resource_type: Some(ResourceType::RemoteApi),
    };
    
    let nonmatching_result = registry.handle("ListResources", serde_json::to_value(nonmatching_request).unwrap()).await.unwrap();
    let nonmatching_resources = nonmatching_result["resources"].as_array().unwrap();
    assert_eq!(nonmatching_resources.len(), 0);
}

#[tokio::test]
async fn test_get_resource() {
    let registry = ResourceRegistryServer::new();
    
    // Register a server first
    let server_params = serde_json::json!({
        "server_id": "test-resource-server-4",
        "endpoint": "http://localhost:8080/resource-server"
    });
    registry.handle("RegisterServer", server_params).await.unwrap();
    
    // Register a resource
    let request = RegisterResourceRequest {
        name: "GetResource".to_string(),
        description: "A test resource for getting".to_string(),
        resource_type: ResourceType::Database,
        server_id: "test-resource-server-4".to_string(),
        access_path: "/api/resources/get".to_string(),
        schema: None,
        query_schema: None,
        metadata: None,
    };
    
    let register_result = registry.handle("RegisterResource", serde_json::to_value(request).unwrap()).await.unwrap();
    let resource_id = register_result["resource_id"].as_str().unwrap().to_string();
    
    // Get the resource
    let get_request = GetResourceRequest {
        resource_id: resource_id.clone(),
    };
    
    let get_result = registry.handle("GetResource", serde_json::to_value(get_request).unwrap()).await.unwrap();
    
    // Verify the result
    assert_eq!(get_result["resource"]["id"].as_str().unwrap(), resource_id);
    assert_eq!(get_result["resource"]["name"].as_str().unwrap(), "GetResource");
    assert_eq!(get_result["resource"]["description"].as_str().unwrap(), "A test resource for getting");
}

#[tokio::test]
async fn test_query_resource() {
    let registry = ResourceRegistryServer::new();
    
    // Register a server first
    let server_params = serde_json::json!({
        "server_id": "test-resource-server-5",
        "endpoint": "http://localhost:8080/resource-server"
    });
    registry.handle("RegisterServer", server_params).await.unwrap();
    
    // Register a resource
    let request = RegisterResourceRequest {
        name: "QueryResource".to_string(),
        description: "A test resource for querying".to_string(),
        resource_type: ResourceType::Database,
        server_id: "test-resource-server-5".to_string(),
        access_path: "/api/resources/query".to_string(),
        schema: None,
        query_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "filter": {"type": "string"}
            }
        })),
        metadata: None,
    };
    
    let register_result = registry.handle("RegisterResource", serde_json::to_value(request).unwrap()).await.unwrap();
    let resource_id = register_result["resource_id"].as_str().unwrap().to_string();
    
    // Query the resource
    let query = ResourceQuery {
        resource_id: resource_id.clone(),
        parameters: serde_json::json!({"filter": "test filter"}),
        context: None,
    };
    
    let query_request = QueryResourceRequest {
        query,
    };
    
    let query_result = registry.handle("QueryResource", serde_json::to_value(query_request).unwrap()).await.unwrap();
    
    // Verify the result - this is simulated in the mock implementation
    assert!(query_result["result"]["result"].is_object());
    assert_eq!(query_result["result"]["result"]["status"].as_str().unwrap(), "success");
    assert_eq!(query_result["result"]["result"]["resource_id"].as_str().unwrap(), resource_id);
} 