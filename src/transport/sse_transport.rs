use std::net::SocketAddr;
use std::time::Duration;

use rmcp::server::McpServer;
use rmcp::transport::TransportServer;
use rmcp::transport::sse_server::{SseServer, SseServerConfig};
use rmcp::IntoTransport;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

pub struct SseTransportServer<S: McpServer> {
    server: S,
    bind_addr: SocketAddr,
    sse_path: String,
    post_path: String,
    keep_alive: Duration,
}

impl<S: McpServer> SseTransportServer<S> {
    pub fn new(
        server: S, 
        bind_addr: SocketAddr,
        sse_path: String,
        post_path: String,
    ) -> Self {
        Self { 
            server, 
            bind_addr,
            sse_path,
            post_path,
            keep_alive: Duration::from_secs(15),
        }
    }

    pub fn with_keep_alive(mut self, keep_alive: Duration) -> Self {
        self.keep_alive = keep_alive;
        self
    }
}

#[async_trait]
impl<S: McpServer + Send + Sync + Clone + 'static> TransportServer for SseTransportServer<S> {
    async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Create cancellation token for clean shutdown
        let ct = CancellationToken::new();

        // Configure the SSE server
        let config = SseServerConfig {
            bind: self.bind_addr,
            sse_path: self.sse_path.clone(),
            post_path: self.post_path.clone(),
            ct: ct.clone(),
            sse_keep_alive: Some(self.keep_alive),
        };

        // Create the SSE server
        let sse_server = SseServer::serve_with_config(config).await?;
        
        // Create a server service using the provided MCP server
        let server = self.server.clone();
        
        // Start the SSE service with the server
        let _ct = sse_server.with_service(move || {
            rmcp::serve_server(server.clone())
        });

        // Wait for cancellation
        ct.cancelled().await;
        
        Ok(())
    }
} 