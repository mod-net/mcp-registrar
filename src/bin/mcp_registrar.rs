use clap::{ArgAction, Parser};
use mcp_registrar::servers::mcp_registrar::McpRegistrarServer;
use mcp_registrar::transport::stdio_transport::TransportServer;
use mcp_registrar::transport::{stdio_transport::StdioTransportServer, HttpTransportServer};
use std::net::SocketAddr;
use tracing;
use tracing_subscriber;

/// MCP Registrar Server CLI
#[derive(Debug, Parser)]
#[command(name = "mcp-registrar")]
#[command(about = "MCP Registrar Server - Central service directory for MCP servers")]
struct Cli {
    /// Optional HTTP address (e.g. 127.0.0.1:8080) to expose JSON-RPC over HTTP
    #[arg(long)]
    http_addr: Option<SocketAddr>,

    /// Disable stdio transport (HTTP-only mode)
    #[arg(long, action = ArgAction::SetTrue)]
    no_stdio: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args = Cli::parse();

    // Create a new McpRegistrarServer instance
    let server = McpRegistrarServer::new();

    let http_enabled = args.http_addr.is_some();
    let stdio_enabled = !args.no_stdio;

    if !http_enabled && !stdio_enabled {
        return Err(
            "At least one transport must be enabled (specify --http-addr or omit --no-stdio)"
                .into(),
        );
    }

    match (stdio_enabled, http_enabled) {
        (true, true) => {
            tracing::info!(?args.http_addr, "Starting MCP Registrar server with HTTP transport");
            tracing::info!("Starting MCP Registrar server with stdio transport");

            let http_server = HttpTransportServer::new(args.http_addr.unwrap(), server.clone());
            let stdio_server = StdioTransportServer::new(server);

            tokio::try_join!(
                async move {
                    stdio_server
                        .serve()
                        .await
                        .map_err(|err| anyhow::Error::new(err))
                },
                async move {
                    http_server
                        .serve()
                        .await
                        .map_err(|err| anyhow::Error::new(err))
                }
            )?;
        }
        (true, false) => {
            tracing::info!("Starting MCP Registrar server with stdio transport");
            let stdio_server = StdioTransportServer::new(server);
            stdio_server.serve().await?;
        }
        (false, true) => {
            tracing::info!(?args.http_addr, "Starting MCP Registrar server with HTTP transport");
            let http_server = HttpTransportServer::new(args.http_addr.unwrap(), server);
            http_server
                .serve()
                .await
                .map_err(|err| anyhow::Error::new(err))?;
        }
        (false, false) => unreachable!(),
    }

    Ok(())
}
