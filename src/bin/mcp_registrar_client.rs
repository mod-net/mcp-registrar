use clap::{Parser, Subcommand};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "mcp-registrar-client",
    about = "HTTP client for the MCP Registrar JSON-RPC API"
)]
struct Cli {
    /// Base URL of the registrar HTTP endpoint (e.g. http://127.0.0.1:8080)
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    base_url: Url,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 10)]
    timeout_secs: u64,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Register a new MCP server
    RegisterServer {
        /// Server name
        #[arg(long)]
        name: String,
        /// Server description
        #[arg(long)]
        description: String,
        /// Semantic version of the server
        #[arg(long)]
        version: String,
        /// JSON schema URL describing the server (optional)
        #[arg(long)]
        schema_url: Option<String>,
        /// Capabilities as a comma-separated list
        #[arg(long, value_delimiter = ',')]
        capabilities: Vec<String>,
        /// External endpoint advertised by the server
        #[arg(long)]
        endpoint: String,
    },

    /// Unregister a server by id
    UnregisterServer {
        #[arg(long)]
        id: String,
    },

    /// Fetch a single server by id
    GetServer {
        #[arg(long)]
        id: String,
    },

    /// List all registered servers
    ListServers,

    /// Update a server status
    UpdateServerStatus {
        #[arg(long)]
        id: String,
        #[arg(long, value_parser = parse_status)]
        status: String,
    },

    /// Send a heartbeat to a server
    Heartbeat {
        #[arg(long)]
        id: String,
    },
}

#[derive(Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    params: Value,
}

#[derive(Deserialize, Debug)]
struct JsonRpcResponse {
    #[serde(default)]
    error: Option<JsonRpcError>,
    #[serde(default)]
    result: Option<Value>,
}

#[derive(Deserialize, Debug)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cli.timeout_secs))
        .build()?;

    let (method, params) = match &cli.command {
        Commands::RegisterServer {
            name,
            description,
            version,
            schema_url,
            capabilities,
            endpoint,
        } => (
            "RegisterServer",
            serde_json::json!({
                "name": name,
                "description": description,
                "version": version,
                "schema_url": schema_url,
                "capabilities": capabilities,
                "endpoint": endpoint,
            }),
        ),
        Commands::UnregisterServer { id } => (
            "UnregisterServer",
            serde_json::json!({ "id": id }),
        ),
        Commands::GetServer { id } => ("GetServer", serde_json::json!({ "id": id })),
        Commands::ListServers => ("ListServers", serde_json::json!({})),
        Commands::UpdateServerStatus { id, status } => (
            "UpdateServerStatus",
            serde_json::json!({ "id": id, "status": status }),
        ),
        Commands::Heartbeat { id } => ("Heartbeat", serde_json::json!({ "id": id })),
    };

    let request = JsonRpcRequest {
        jsonrpc: "2.0",
        id: 1,
        method: method.to_string(),
        params,
    };

    let response = client
        .post(cli.base_url.join("rpc")?)
        .json(&request)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("HTTP error: {status} - {body}");
    }

    let rpc_response: JsonRpcResponse = response.json().await?;
    if let Some(error) = rpc_response.error {
        anyhow::bail!("RPC error {}: {}", error.code, error.message);
    }

    let result = rpc_response.result.unwrap_or(Value::Null);
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn parse_status(value: &str) -> Result<String, String> {
    match value {
        "active" | "inactive" | "error" => Ok(value.to_string()),
        other => Err(format!(
            "Invalid status '{other}'. Expected one of: active, inactive, error"
        )),
    }
}
