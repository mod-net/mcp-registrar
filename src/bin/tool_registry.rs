use clap::Parser;
use mcp_registrar::servers::tool_registry::ToolRegistryServer;
use mcp_registrar::transport::stdio_transport::{StdioTransportServer, TransportServer};

/// Tool Registry Server CLI
#[derive(Debug, Parser)]
#[command(name = "tool-registry")]
#[command(about = "Tool Registry Server - Registry for MCP tools and invocation")]
struct Cli {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let _args = Cli::parse();

    // Create a new ToolRegistryServer instance
    let server = ToolRegistryServer::new();
    // Initialize registry (loads tools/**/tool.json)
    server.initialize().await?;

    // Initialize stdio transport with the server
    tracing::info!("Starting Tool Registry server with stdio transport");
    let transport = StdioTransportServer::new(server);

    // Start the server
    transport.serve().await?;

    Ok(())
}
