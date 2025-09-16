use async_trait::async_trait;
use serde_json::Value;

pub type HandlerResult = Result<Value, Box<dyn std::error::Error + Send + Sync>>;

/// A simple MCP server trait
#[async_trait]
pub trait McpServer: Clone + Send + Sync + 'static {
    /// Handle a request with the given name and parameters
    async fn handle(&self, name: &str, params: Value) -> HandlerResult;
}
