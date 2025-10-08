use mcp_registrar::transport::stdio_transport::StdioTransportServer;
use mcp_registrar::transport::stdio_transport::TransportServer;
use mcp_registrar::servers::text_generator::TextGeneratorServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Create server from environment
    let server = TextGeneratorServer::from_env()?;

    // Run over stdio transport
    let transport = StdioTransportServer::new(server);
    transport.serve().await?;
    Ok(())
}
