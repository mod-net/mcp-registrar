use registry_scheduler::transport::stdio_transport::StdioTransportServer;
use registry_scheduler::transport::stdio_transport::TransportServer;
use registry_scheduler::servers::text_generator::TextGeneratorServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Create server from environment
    let server = TextGeneratorServer::from_env()?;

    // Run over stdio transport
    let transport = StdioTransportServer::new(server);
    transport.serve().await?;
    Ok(())
}
