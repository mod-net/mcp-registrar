use registry_scheduler::transport::stdio_transport::{StdioTransportServer, TransportServer};
use registry_scheduler::servers::mcp_registrar::McpRegistrarServer;
use clap::Parser;

/// MCP Registrar Server CLI
#[derive(Debug, Parser)]
#[command(name = "mcp-registrar")]
#[command(about = "MCP Registrar Server - Central service directory for MCP servers")]
struct Cli {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    // Parse command line arguments
    let _args = Cli::parse();
    
    // Create a new McpRegistrarServer instance
    let server = McpRegistrarServer::new();
    
    // Initialize stdio transport with the server
    tracing::info!("Starting MCP Registrar server with stdio transport");
    let transport = StdioTransportServer::new(server);
    
    // Start the server
    transport.serve().await?;
    
    Ok(())
} 