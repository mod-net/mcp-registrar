use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

use crate::error::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ToolRuntime {
    Process(ProcessConfig),
    Wasm(WasmConfig),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessConfig {
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env_allowlist: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WasmConfig {
    pub module_path: PathBuf,
    #[serde(default = "default_export")] 
    pub export: String, // default "call"
}

fn default_export() -> String { "call".to_string() }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetworkPolicy {
    #[serde(rename = "deny")] 
    Deny,
    #[serde(rename = "egress-proxy")] 
    EgressProxy,
    #[serde(rename = "allow")] 
    Allow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Policy {
    pub timeout_ms: u64,
    pub memory_bytes: u64,
    pub cpu_time_ms: u64,
    pub max_output_bytes: usize,
    pub network: NetworkPolicy,
    #[serde(default)]
    pub preopen_tmp: bool,
    #[serde(default)]
    pub env_allowlist: Vec<(String, String)>,
}

#[async_trait::async_trait]
pub trait Executor: Send + Sync {
    async fn invoke(
        &self,
        tool_id: &str,
        runtime: &ToolRuntime,
        args_json: &serde_json::Value,
        policy: &Policy,
    ) -> Result<serde_json::Value, Error>;
}

pub mod executors {
    pub mod process;
    pub mod wasm;
}
pub mod manifest;
