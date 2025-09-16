use log::{error, info, warn};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

#[derive(Debug)]
pub struct DetectedServer {
    pub path: PathBuf,
    pub status: String,
    pub process: Option<Child>,
    pub endpoint: Option<String>, // To be filled in future steps
                                  // TODO: Add fields for metadata (name, version, schema, etc)
}

/// Scan the submodules directory for MCP server projects
pub fn scan_and_load_servers(submodules_dir: &str) -> Vec<DetectedServer> {
    let mut servers = Vec::new();
    let dir = Path::new(submodules_dir);
    if !dir.exists() || !dir.is_dir() {
        error!("Submodules directory not found: {}", submodules_dir);
        return servers;
    }
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            error!(
                "Failed to read submodules directory {}: {}",
                submodules_dir, e
            );
            return servers;
        }
    };
    for entry in read_dir {
        if let Ok(entry) = entry {
            let path = entry.path();
            if path.is_dir() {
                let cargo_toml = path.join("Cargo.toml");
                let mcp_server_bin = path.join("src/bin/mcp_server.rs");
                if cargo_toml.exists() && mcp_server_bin.exists() {
                    info!("Detected MCP server project at: {}", path.display());
                    // Try to start the server as a subprocess
                    let process = match Command::new("cargo")
                        .arg("run")
                        .arg("--bin")
                        .arg("mcp_server")
                        .arg("--release")
                        .current_dir(&path)
                        .spawn()
                    {
                        Ok(child) => {
                            info!("Started MCP server at {}", path.display());
                            Some(child)
                        }
                        Err(e) => {
                            error!("Failed to start MCP server at {}: {}", path.display(), e);
                            None
                        }
                    };
                    servers.push(DetectedServer {
                        path: path.clone(),
                        status: if process.is_some() {
                            "Started".to_string()
                        } else {
                            "Failed to start".to_string()
                        },
                        process,
                        endpoint: None,
                    });
                } else {
                    warn!("Directory {} does not appear to be a Rust MCP server (missing Cargo.toml or mcp_server.rs)", path.display());
                }
            }
        }
    }
    servers
}
