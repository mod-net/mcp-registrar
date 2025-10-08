use mcp_registrar::servers::resource_registry::ResourceRegistryServer;
use mcp_registrar::transport::stdio_transport::{StdioTransportServer, TransportServer};
use clap::Parser;

/// Resource Registry Server CLI
#[derive(Debug, Parser)]
#[command(name = "resource-registry")]
#[command(about = "Resource Registry Server - Registry for MCP resources and queries")]
struct Cli {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    // Parse command line arguments
    let _args = Cli::parse();
    
    // Create a new ResourceRegistryServer instance
    let server = ResourceRegistryServer::new();
    
    // Initialize stdio transport with the server
    tracing::info!("Starting Resource Registry server with stdio transport");
    let transport = StdioTransportServer::new(server);
    
    // Start the server
    transport.serve().await?;
    
    Ok(())
} 