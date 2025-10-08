use clap::Parser;
use mcp_registrar::servers::prompt_registry::PromptRegistryServer;
use mcp_registrar::transport::stdio_transport::{StdioTransportServer, TransportServer};

/// Prompt Registry Server CLI
#[derive(Debug, Parser)]
#[command(name = "prompt-registry")]
#[command(about = "Prompt Registry Server - Registry for MCP prompts and templates")]
struct Cli {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let _args = Cli::parse();

    // Create a new PromptRegistryServer instance
    let server = PromptRegistryServer::new();

    // Initialize stdio transport with the server
    tracing::info!("Starting Prompt Registry server with stdio transport");
    let transport = StdioTransportServer::new(server);

    // Start the server
    transport.serve().await?;

    Ok(())
}
