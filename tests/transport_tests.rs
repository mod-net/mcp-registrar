use mcp_registrar::transport::{McpServer, HandlerResult};
use mcp_registrar::transport::stdio_transport::StdioTransportServer;
use std::io;
use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::io::{AsyncRead, AsyncWrite, AsyncBufRead};
use std::pin::Pin;

// A mock McpServer implementation for testing
#[derive(Clone)]
struct MockMcpServer {
    calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    // Define what response to return for each method
    responses: Arc<Mutex<std::collections::HashMap<String, serde_json::Value>>>,
}

impl MockMcpServer {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    async fn add_response(&self, method: &str, response: serde_json::Value) {
        let mut responses = self.responses.lock().await;
        responses.insert(method.to_string(), response);
    }

    async fn get_calls(&self) -> Vec<(String, serde_json::Value)> {
        let calls = self.calls.lock().await;
        calls.clone()
    }
}

#[async_trait]
impl McpServer for MockMcpServer {
    async fn handle(&self, method: &str, params: serde_json::Value) -> HandlerResult {
        let mut calls = self.calls.lock().await;
        calls.push((method.to_string(), params.clone()));

        let responses = self.responses.lock().await;
        match responses.get(method) {
            Some(response) => Ok(response.clone()),
            None => Err(format!("No mock response for method: {}", method).into()),
        }
    }
}

// Mock IO implementation for testing the transport layer
#[derive(Clone)]
struct MockStdio {
    input: Arc<Mutex<Vec<String>>>,
    output: Arc<Mutex<Vec<String>>>,
    current_line: Arc<Mutex<Option<Vec<u8>>>>,
}

impl MockStdio {
    fn new(input: Vec<String>) -> Self {
        Self {
            input: Arc::new(Mutex::new(input)),
            output: Arc::new(Mutex::new(Vec::new())),
            current_line: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_output(&self) -> Vec<String> {
        let output = self.output.lock().await;
        output.clone()
    }
}

impl AsyncRead for MockStdio {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let input = self.input.clone();
        let current_line = self.current_line.clone();
        
        futures::executor::block_on(async move {
            let mut line_guard = current_line.lock().await;
            
            // If we don't have a current line, get one from input
            if line_guard.is_none() {
                let mut inputs = input.lock().await;
                if let Some(line) = inputs.first() {
                    *line_guard = Some(format!("{}\n", line).into_bytes());
                    inputs.remove(0);
                }
            }
            
            // Copy data from the current line to the buffer
            if let Some(line) = line_guard.as_mut() {
                let remaining = buf.remaining();
                let to_copy = std::cmp::min(remaining, line.len());
                buf.put_slice(&line[..to_copy]);
                line.drain(..to_copy);
                
                // If we've consumed the entire line, clear it
                if line.is_empty() {
                    *line_guard = None;
                }
            }
        });
        
        std::task::Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for MockStdio {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        let output = self.output.clone();
        let data = buf.to_vec();
        
        futures::executor::block_on(async move {
            let mut outputs = output.lock().await;
            let output_str = String::from_utf8_lossy(&data).to_string();
            outputs.push(output_str);
        });
        
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

impl AsyncBufRead for MockStdio {
    fn poll_fill_buf<'a>(
        self: Pin<&'a mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<&'a [u8], std::io::Error>> {
        let input = self.input.clone();
        let current_line = self.current_line.clone();
        
        // Use a static buffer to avoid lifetime issues
        // This is a bit of a hack, but it works for our test
        static mut BUFFER: [u8; 1024] = [0; 1024];
        static mut BUFFER_LEN: usize = 0;
        
        futures::executor::block_on(async move {
            let mut line_guard = current_line.lock().await;
            
            // If we don't have a current line, get one from input
            if line_guard.is_none() {
                let mut inputs = input.lock().await;
                if let Some(line) = inputs.first() {
                    *line_guard = Some(format!("{}\n", line).into_bytes());
                    inputs.remove(0);
                }
            }
            
            // Return the current line if we have one
            if let Some(line) = line_guard.as_ref() {
                // Copy the line to our static buffer
                unsafe {
                    BUFFER_LEN = line.len();
                    BUFFER[..line.len()].copy_from_slice(line);
                    std::task::Poll::Ready(Ok(&BUFFER[..BUFFER_LEN]))
                }
            } else {
                // Return an empty slice of the same type as the other branch
                unsafe {
                    BUFFER_LEN = 0;
                    std::task::Poll::Ready(Ok(&BUFFER[..0]))
                }
            }
        })
    }

    fn consume(
        self: Pin<&mut Self>,
        amt: usize,
    ) {
        let current_line = self.current_line.clone();
        futures::executor::block_on(async move {
            let mut line_guard = current_line.lock().await;
            if let Some(line) = line_guard.as_mut() {
                if amt >= line.len() {
                    *line_guard = None;
                } else {
                    line.drain(..amt);
                }
            }
        });
    }
}

// We need to test the StdioTransportServer by providing mock stdin/stdout
// This test verifies the basic request-response flow
#[tokio::test]
async fn test_stdio_transport_basic_flow() {
    // Setup a mock McpServer
    let server = MockMcpServer::new();
    
    // Configure mock responses
    server.add_response("hello", serde_json::json!("world")).await;
    
    // Create a transport wrapper
    let transport = StdioTransportServer::new(server.clone());
    
    // Mock the IO by sending a request
    let request = r#"{"method": "hello", "params": {"name": "test"}}"#;
    let mock_input = vec![request.to_string()];
    let mut mock_stdio = MockStdio::new(mock_input);
    let mut mock_stdio_clone = mock_stdio.clone();
    
    // Run the transport with mocked IO
    let transport_future = transport.serve_with_io(&mut mock_stdio, &mut mock_stdio_clone);
    tokio::time::timeout(std::time::Duration::from_millis(100), transport_future).await.unwrap().unwrap();
    
    // Verify the request was processed by checking that our mock server received the call
    let calls = server.get_calls().await;
    println!("Server calls: {:?}", calls);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "hello");
    assert_eq!(calls[0].1, serde_json::json!({"name": "test"}));
    
    // Verify the response
    let output = mock_stdio.get_output().await;
    println!("Mock stdio output: {:?}", output);
    assert_eq!(output.len(), 1);
    assert_eq!(output[0], "{\"result\": \"world\"}\n");
}

// Test error handling for invalid JSON
#[tokio::test]
async fn test_stdio_transport_invalid_json() {
    // Similar to the previous test but with invalid JSON input
    // The transport should return a proper error response
}

// Test handling of unknown methods
#[tokio::test]
async fn test_stdio_transport_unknown_method() {
    // Call a method that isn't registered in the mock
    // Verify the error response is correctly formatted
} 