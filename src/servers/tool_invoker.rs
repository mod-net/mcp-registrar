use serde_json::Value;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;

/// A trait for invoking tools that doesn't have the Sized bound
pub trait ToolInvoker: Send + Sync + 'static {
    /// Create a new instance of the tool invoker
    fn new() -> Self
    where
        Self: Sized;

    /// Invoke a tool with the given name and parameters
    fn invoke_tool(
        &self,
        tool: String,
        arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, Box<dyn Error + Send + Sync>>> + Send + 'static>>;
}
