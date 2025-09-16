# Rust Implementation Guide for Registry API

This guide explains how to implement the Registry API server in Rust using Axum and Serde.

## Dependencies

Add these to your `Cargo.toml`:

```toml
[dependencies]
axum = "0.7"
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tower-http = { version = "0.5", features = ["cors"] }
async-trait = "0.1"
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
```

## Type Definitions

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Artifact Types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Image,
    File,
    Text,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactReference {
    #[serde(rename = "type")]
    pub artifact_type: ArtifactType,
    pub mime_type: String,
    pub url: Option<String>,
    pub data: Option<String>,  // Base64 encoded for binary data
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

// Schema Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaParameter {
    pub description: String,
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaProperties {
    pub properties: HashMap<String, SchemaParameter>,
    pub required: Option<Vec<String>>,
    #[serde(rename = "type")]
    pub schema_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: SchemaProperties,
}

// Tool Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub version: String,
    pub server_id: String,
    pub categories: Vec<String>,
    pub parameters_schema: ToolSchema,
    pub returns_schema: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produces_artifacts: Option<Vec<ArtifactType>>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub url: String,
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryTools {
    pub category: String,
    pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsResponse {
    pub by_category: Vec<CategoryTools>,
    pub by_server: Vec<Server>,
}

// OpenAI Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub name: Option<String>,
    pub tool_call_id: Option<String>,
}

// Execution Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: serde_json::Value,
    pub artifacts: Option<Vec<ArtifactReference>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResponse {
    pub tool_name: String,
    pub result: ToolResult,
    pub execution_time: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteChainResponse {
    pub results: Vec<ExecuteResponse>,
    pub total_execution_time: f64,
}

// Request Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteChainRequest {
    pub tools: Vec<ExecuteRequest>,
}
```

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("Execution error: {0}")]
    ExecutionError(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match self {
            Self::ToolNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            Self::InvalidArguments(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            Self::ExecutionError(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(serde_json::json!({
            "error": {
                "message": error_message,
                "code": status.as_u16()
            }
        }));

        (status, body).into_response()
    }
}
```

## API Implementation

```rust
use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::State,
};
use std::sync::Arc;

// Registry state
pub struct Registry {
    servers: HashMap<String, Server>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    pub async fn list_tools(&self) -> ToolsResponse {
        let mut all_tools = Vec::new();
        for server in self.servers.values() {
            all_tools.extend(server.tools.clone());
        }

        // Group by category
        let mut categories = HashMap::new();
        for tool in &all_tools {
            for category in &tool.categories {
                categories
                    .entry(category.clone())
                    .or_insert_with(Vec::new)
                    .push(tool.clone());
            }
        }

        let by_category = categories
            .into_iter()
            .map(|(category, tools)| CategoryTools { category, tools })
            .collect();

        ToolsResponse {
            by_category,
            by_server: self.servers.values().cloned().collect(),
        }
    }

    pub async fn execute_tool(&self, request: ExecuteRequest) -> Result<ExecuteResponse, ApiError> {
        let tool = self.find_tool(&request.name)
            .ok_or_else(|| ApiError::ToolNotFound(request.name.clone()))?;

        // Validate arguments against schema
        self.validate_arguments(&tool.parameters_schema, &request.arguments)?;

        // Execute tool (implement your tool execution logic here)
        let start_time = std::time::Instant::now();
        let result = self.execute_tool_impl(tool, request.arguments).await?;
        let execution_time = start_time.elapsed().as_secs_f64();

        Ok(ExecuteResponse {
            tool_name: tool.name.clone(),
            result,
            execution_time,
        })
    }
}

// API routes
pub fn create_router(registry: Arc<Registry>) -> Router {
    Router::new()
        .route("/tools", get(list_tools))
        .route("/execute", post(execute_tool))
        .route("/execute_chain", post(execute_chain))
        .with_state(registry)
}

async fn list_tools(
    State(registry): State<Arc<Registry>>,
) -> Json<ToolsResponse> {
    Json(registry.list_tools().await)
}

async fn execute_tool(
    State(registry): State<Arc<Registry>>,
    Json(request): Json<ExecuteRequest>,
) -> Result<Json<ExecuteResponse>, ApiError> {
    let response = registry.execute_tool(request).await?;
    Ok(Json(response))
}

async fn execute_chain(
    State(registry): State<Arc<Registry>>,
    Json(request): Json<ExecuteChainRequest>,
) -> Result<Json<ExecuteChainResponse>, ApiError> {
    let start_time = std::time::Instant::now();
    let mut results = Vec::new();

    for tool_request in request.tools {
        let response = registry.execute_tool(tool_request).await?;
        results.push(response);
    }

    Ok(Json(ExecuteChainResponse {
        results,
        total_execution_time: start_time.elapsed().as_secs_f64(),
    }))
}
```

## Server Setup

```rust
#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create registry
    let registry = Arc::new(Registry::new());

    // Add example tools
    // ... (implement your tool registration logic)

    // Create router
    let app = create_router(registry)
        .layer(tower_http::cors::CorsLayer::permissive());

    // Run server
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 7600));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
```

## Tool Implementation

To implement a tool:

1. Create a struct that represents your tool:
```rust
pub struct ExampleTool {
    name: String,
    description: String,
    version: String,
}

#[async_trait::async_trait]
impl ToolExecutor for ExampleTool {
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ApiError> {
        let input = args.get("input")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::InvalidArguments("input is required".to_string()))?;

        Ok(ToolResult {
            output: serde_json::json!({
                "message": format!("Processed: {}", input)
            }),
            artifacts: Some(vec![ArtifactReference {
                artifact_type: ArtifactType::Text,
                mime_type: "text/plain".to_string(),
                url: None,
                data: Some(input.to_string()),
                metadata: HashMap::new(),
            }]),
        })
    }

    fn get_schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: SchemaProperties {
                properties: {
                    let mut map = HashMap::new();
                    map.insert("input".to_string(), SchemaParameter {
                        description: "Input text to process".to_string(),
                        param_type: "string".to_string(),
                        default: None,
                    });
                    map
                },
                required: Some(vec!["input".to_string()]),
                schema_type: "object".to_string(),
            },
        }
    }
}
```

## Best Practices

1. **Error Handling**: Use the `ApiError` enum for consistent error handling across the application.

2. **Validation**: Always validate tool arguments against their schema before execution.

3. **Async/Await**: Use async/await for all I/O operations and tool execution.

4. **State Management**: Use `Arc` for sharing the registry state across threads.

5. **CORS**: Configure CORS appropriately for your deployment environment.

6. **Logging**: Use the `tracing` crate for structured logging.

7. **Testing**: Write unit tests for tool implementations and integration tests for API endpoints.

## Testing

Here's an example of how to test your implementation:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_tools() {
        let registry = Registry::new();
        // Add test tools
        let response = registry.list_tools().await;
        assert!(!response.by_category.is_empty());
    }

    #[tokio::test]
    async fn test_execute_tool() {
        let registry = Registry::new();
        // Add test tools
        let request = ExecuteRequest {
            name: "ExampleTool".to_string(),
            arguments: serde_json::json!({
                "input": "test"
            }),
        };
        let response = registry.execute_tool(request).await.unwrap();
        assert_eq!(response.tool_name, "ExampleTool");
    }
}
```
