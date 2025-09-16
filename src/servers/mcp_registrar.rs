use crate::models::server::{ServerInfo, ServerStatus};
use crate::servers::server_loader;
use crate::transport::{HandlerResult, McpServer};
use async_trait::async_trait;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterServerRequest {
    pub name: String,
    pub description: String,
    pub version: String,
    pub schema_url: Option<String>,
    pub capabilities: Vec<String>,
    pub endpoint: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterServerResponse {
    pub server_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerListResponse {
    pub servers: Vec<ServerInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpRegistrarServer {
    servers: Arc<Mutex<HashMap<String, ServerInfo>>>,
}

impl McpRegistrarServer {
    pub fn new() -> Self {
        let servers = Arc::new(Mutex::new(HashMap::new()));
        // Gate auto-detection behind env to keep tests deterministic
        let autodetect = std::env::var("MCP_REGISTRAR_AUTODETECT").unwrap_or_default();
        if autodetect == "1" || autodetect.eq_ignore_ascii_case("true") {
            let detected = server_loader::scan_and_load_servers("submodules");
            info!(
                "Registrar detected {} MCP server(s) in submodules.",
                detected.len()
            );
            for s in &detected {
                info!("  - {} [{}]", s.path.display(), s.status);
            }
            if let Some(first) = detected.first() {
                let server_id = Uuid::new_v4().to_string();
                let server = ServerInfo::new(
                    server_id.clone(),
                    format!(
                        "{}",
                        first.path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    "Auto-registered MCP server".to_string(),
                    "0.1.0".to_string(),
                    None,                                           // schema_url
                    vec!["auto".to_string()],                       // capabilities
                    format!("http://localhost:8000/{}", server_id), // endpoint (placeholder)
                );
                let mut servers_map = servers.lock().unwrap();
                servers_map.insert(server_id, server);
                info!("Auto-registered first detected MCP server in registry.");
            }
        }
        Self { servers }
    }

    fn register_server(&self, request: RegisterServerRequest) -> String {
        let server_id = Uuid::new_v4().to_string();

        let server = ServerInfo::new(
            server_id.clone(),
            request.name,
            request.description,
            request.version,
            request.schema_url,
            request.capabilities,
            request.endpoint,
        );

        let mut servers = self.servers.lock().unwrap();
        servers.insert(server_id.clone(), server);

        server_id
    }

    fn unregister_server(&self, id: &str) -> bool {
        let mut servers = self.servers.lock().unwrap();
        servers.remove(id).is_some()
    }

    fn get_server(&self, id: &str) -> Option<ServerInfo> {
        let servers = self.servers.lock().unwrap();
        servers.get(id).cloned()
    }

    fn list_servers(&self) -> Vec<ServerInfo> {
        let servers = self.servers.lock().unwrap();
        servers.values().cloned().collect()
    }

    fn update_server_status(&self, id: &str, status: ServerStatus) -> Option<ServerInfo> {
        let mut servers = self.servers.lock().unwrap();
        if let Some(server) = servers.get_mut(id) {
            server.status = status;
            server.update_heartbeat();
            return Some(server.clone());
        }
        None
    }
}

#[async_trait]
impl McpServer for McpRegistrarServer {
    async fn handle(&self, name: &str, params: serde_json::Value) -> HandlerResult {
        match name {
            "RegisterServer" => {
                let request: RegisterServerRequest = serde_json::from_value(params)?;
                let server_id = self.register_server(request);
                Ok(serde_json::to_value(RegisterServerResponse { server_id })?)
            }
            "UnregisterServer" => {
                let id = params["id"].as_str().ok_or("Missing server id")?;
                let success = self.unregister_server(id);
                Ok(serde_json::json!({ "success": success }))
            }
            "GetServer" => {
                let id = params["id"].as_str().ok_or("Missing server id")?;
                match self.get_server(id) {
                    Some(server) => Ok(serde_json::to_value(server)?),
                    None => Err(format!("Server not found: {}", id).into()),
                }
            }
            "ListServers" => {
                let servers = self.list_servers();
                Ok(serde_json::to_value(ServerListResponse { servers })?)
            }
            "UpdateServerStatus" => {
                let id = params["id"].as_str().ok_or("Missing server id")?;
                let status_str = params["status"].as_str().ok_or("Missing status")?;
                let status = match status_str {
                    "active" => ServerStatus::Active,
                    "inactive" => ServerStatus::Inactive,
                    "error" => ServerStatus::Error,
                    _ => return Err(format!("Invalid status: {}", status_str).into()),
                };

                match self.update_server_status(id, status) {
                    Some(server) => Ok(serde_json::to_value(server)?),
                    None => Err(format!("Server not found: {}", id).into()),
                }
            }
            "Heartbeat" => {
                let id = params["id"].as_str().ok_or("Missing server id")?;
                match self.update_server_status(id, ServerStatus::Active) {
                    Some(_server) => Ok(serde_json::json!({ "success": true })),
                    None => Err(format!("Server not found: {}", id).into()),
                }
            }
            _ => Err(format!("Unknown method: {}", name).into()),
        }
    }
}
