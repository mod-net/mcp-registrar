use crate::models::tool::{Tool, ToolInvocation, ToolInvocationResult};
use crate::transport::{HandlerResult, McpServer};
use crate::utils::tool_storage::{FileToolStorage, ToolStorage};
use crate::servers::tool_runtime::{self, manifest, Executor, Policy, ToolRuntime};
use crate::servers::tool_runtime::executors::{process::ProcessExecutor, wasm::WasmExecutor};
use anyhow::Result;
use crate::servers::mcp_registrar::RegisterServerResponse as RegistrarRegisterServerResponse;
use async_trait::async_trait;
use chrono::Utc;
use serde::{
    de::Deserializer,
    Serialize, Serializer, Deserialize,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
// no local async reads needed here after refactor
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterToolRequest {
    pub name: String,
    pub description: String,
    pub version: String,
    pub server_id: String,
    pub categories: Vec<String>,
    pub parameters_schema: Option<serde_json::Value>,
    pub returns_schema: Option<serde_json::Value>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterToolResponse {
    pub tool_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListToolsRequest {
    pub server_id: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListToolsResponse {
    pub tools: Vec<Tool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetToolRequest {
    pub tool_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetToolResponse {
    pub tool: Tool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InvokeToolRequest {
    pub invocation: ToolInvocation,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InvokeToolResponse {
    pub result: ToolInvocationResult,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterServerRequest {
    pub name: String,
    pub description: String,
    pub version: String,
    pub schema_url: Option<String>,
    pub capabilities: Vec<String>,
    pub endpoint: String,
}

#[derive(Debug)]
pub struct ToolRegistryServer {
    tools: Arc<dyn ToolStorage>,
    registered_servers: Arc<TokioMutex<Vec<String>>>,
    // Add fields needed for serialization
    tools_path: PathBuf,
    manifests: Arc<TokioMutex<HashMap<String, StoredManifest>>>,
    proc_exec: Arc<ProcessExecutor>,
    wasm_exec: Arc<WasmExecutor>,
}

impl Clone for ToolRegistryServer {
    fn clone(&self) -> Self {
        Self {
            tools: self.tools.clone(),
            registered_servers: self.registered_servers.clone(),
            tools_path: self.tools_path.clone(),
            manifests: self.manifests.clone(),
            proc_exec: self.proc_exec.clone(),
            wasm_exec: self.wasm_exec.clone(),
        }
    }
}

#[derive(Debug)]
struct StoredManifest {
    _manifest: manifest::ToolManifest,
    runtime: ToolRuntime,
    policy: Policy,
    params_validator: Option<jsonschema::Validator>,
    returns_validator: Option<jsonschema::Validator>,
}

#[derive(Serialize, Deserialize)]
struct ToolRegistryServerData {
    tools_path: PathBuf,
}

impl Serialize for ToolRegistryServer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let data = ToolRegistryServerData {
            tools_path: self.tools_path.clone(),
        };
        data.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ToolRegistryServer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = ToolRegistryServerData::deserialize(deserializer)?;
        
        let tools = Arc::new(FileToolStorage::new(data.tools_path.clone()));
        
        Ok(ToolRegistryServer {
            tools: tools.clone() as Arc<dyn ToolStorage>,
            registered_servers: Arc::new(TokioMutex::new(Vec::new())),
            tools_path: data.tools_path,
            manifests: Arc::new(TokioMutex::new(HashMap::new())),
            proc_exec: Arc::new(ProcessExecutor),
            wasm_exec: Arc::new(WasmExecutor),
        })
    }
}

impl ToolRegistryServer {
    pub fn new() -> Self {
        let tools_path = std::env::current_dir()
            .map(|d| d.join("tools.json"))
            .unwrap_or_else(|_| PathBuf::from("tools.json"));
        info!(
            "Initializing ToolRegistryServer with tools path: {:?}",
            tools_path
        );
        Self {
            tools: Arc::new(FileToolStorage::new(tools_path.clone())),
            registered_servers: Arc::new(TokioMutex::new(Vec::new())),
            tools_path,
            manifests: Arc::new(TokioMutex::new(HashMap::new())),
            proc_exec: Arc::new(ProcessExecutor),
            wasm_exec: Arc::new(WasmExecutor),
        }
    }

    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing ToolRegistryServer");
        self.tools
            .initialize()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize tool storage: {}", e))?;

        // Ensure the implicit manifest-backed server id is registered so manifest tools can run
        {
            let mut servers = self.registered_servers.lock().await;
            if !servers.contains(&"manifest".to_string()) {
                servers.push("manifest".to_string());
            }
        }

        // Load manifests from tools/ directory
        let root = std::env::current_dir()
            .map_err(|e| anyhow::anyhow!("cwd error: {}", e))?
            .join("tools");
        let loaded = manifest::load_manifests(&root)
            .map_err(|e| anyhow::anyhow!("manifest load error: {}", e))?;

        let mut man_map = self.manifests.lock().await;
        for lt in loaded.iter() {
            // Map to Tool and persist to storage
            let tool = manifest::to_tool(&lt.manifest);
            // Build runtime and policy
            let runtime = match lt.manifest.runtime.as_str() {
                "process" => {
                    let cmd = lt.manifest.entry.get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let args: Vec<String> = lt.manifest.entry.get("args")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default();
                    ToolRuntime::Process(tool_runtime::ProcessConfig {
                        command: PathBuf::from(cmd),
                        args,
                        env_allowlist: vec![],
                    })
                }
                "python-uv-script" => {
                    // Map to: uv run [uv_args...] <script>
                    let script = lt.manifest.entry.get("script")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let mut args: Vec<String> = vec!["run".to_string()];
                    if let Some(arr) = lt.manifest.entry.get("uv_args").and_then(|v| v.as_array()) {
                        for v in arr.iter() {
                            if let Some(s) = v.as_str() { args.push(s.to_string()); }
                        }
                    }
                    args.push(script.to_string());
                    ToolRuntime::Process(tool_runtime::ProcessConfig {
                        command: PathBuf::from("uv"),
                        args,
                        env_allowlist: vec![],
                    })
                }
                "binary" => {
                    let cmd = lt.manifest.entry.get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let args: Vec<String> = lt.manifest.entry.get("args")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default();
                    ToolRuntime::Process(tool_runtime::ProcessConfig {
                        command: PathBuf::from(cmd),
                        args,
                        env_allowlist: vec![],
                    })
                }
                "wasm" => {
                    let wasm_path = lt.manifest.entry.get("wasm_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let export = lt.manifest.entry.get("export")
                        .and_then(|v| v.as_str()).unwrap_or("call").to_string();
                    ToolRuntime::Wasm(tool_runtime::WasmConfig {
                        module_path: PathBuf::from(wasm_path),
                        export,
                    })
                }
                other => {
                    warn!("Unknown runtime in manifest {}: {}", tool.id, other);
                    continue;
                }
            };
            // Parse minimal policy with defaults
            let pol = &lt.manifest.policy;
            let timeout_ms = pol.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(8000);
            let memory_bytes = pol.get("memory_bytes").and_then(|v| v.as_u64()).unwrap_or(128 * 1024 * 1024);
            let cpu_time_ms = pol.get("cpu_time_ms").and_then(|v| v.as_u64()).unwrap_or(2000);
            let max_output_bytes = pol.get("max_output_bytes").and_then(|v| v.as_u64()).unwrap_or(256 * 1024) as usize;
            let network = match pol.get("network").and_then(|v| v.as_str()) {
                Some("allow") => tool_runtime::NetworkPolicy::Allow,
                Some("egress-proxy") => tool_runtime::NetworkPolicy::EgressProxy,
                _ => tool_runtime::NetworkPolicy::Deny,
            };
            let preopen_tmp = pol.get("fs").and_then(|fs| fs.get("preopen_tmp")).and_then(|v| v.as_bool()).unwrap_or(false);
            let policy = Policy {
                timeout_ms,
                memory_bytes,
                cpu_time_ms,
                max_output_bytes,
                network,
                preopen_tmp,
                env_allowlist: vec![],
            };

            // Save to storage and manifest map
            if let Err(e) = self.tools.save_tool(tool.clone()).await {
                warn!("Failed to save tool {} from manifest: {}", tool.id, e);
            }
            // Compile schemas up-front
            let (params_validator, returns_validator) = {
                let p = lt.manifest.schema.parameters.clone()
                    .and_then(|s| jsonschema::Validator::new(&s).ok());
                let r = lt.manifest.schema.returns.clone()
                    .and_then(|s| jsonschema::Validator::new(&s).ok());
                (p, r)
            };
            man_map.insert(
                tool.id.clone(),
                StoredManifest {
                    _manifest: lt.manifest.clone(),
                    runtime,
                    policy,
                    params_validator,
                    returns_validator,
                },
            );
        }
        Ok(())
    }

    pub async fn register_server(&self, server_id: String) -> Result<String> {
        info!("Registering server: {}", server_id);
        let mut servers = self.registered_servers.lock().await;
        if !servers.contains(&server_id) {
            servers.push(server_id);
            info!("Server registered successfully");
        } else {
            warn!("Server already registered: {}", server_id);
        }
        Ok(servers.last().cloned().unwrap_or_default())
    }

    async fn register_tool(&self, request: RegisterToolRequest) -> Result<Tool, String> {
        // Check if server is registered
        let servers = self.registered_servers.lock().await;
        if !servers.contains(&request.server_id) {
            return Err(format!(
                "Server with ID {} not registered",
                request.server_id
            ));
        }

        // Generate a unique ID for the tool
        let tool_id = Uuid::new_v4().to_string();

        // Create a new Tool from the request using the Tool::new constructor
        let mut tool = Tool::new(
            tool_id,
            request.name,
            request.description,
            request.version,
            request.server_id,
            request.categories,
            request.parameters_schema,
            request.returns_schema,
        );

        // Add metadata if provided
        if let Some(metadata) = request.metadata {
            for (key, value) in metadata.into_iter() {
                tool = tool.with_metadata(&key, value);
            }
        }

        // Save the tool
        if let Err(e) = self.tools.save_tool(tool.clone()).await {
            return Err(format!("Failed to save tool: {}", e));
        }

        Ok(tool)
    }

    async fn get_tool(&self, id: &str) -> Result<Option<Tool>, anyhow::Error> {
        debug!("Getting tool: {}", id);
        self.tools.get_tool(id).await.map_err(|e| anyhow::anyhow!("Failed to get tool: {}", e))
    }

    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        debug!("Listing all tools");
        self.tools
            .list_tools()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list tools: {}", e))
    }

    pub async fn delete_tool(&self, id: &str) -> Result<()> {
        debug!("Deleting tool: {}", id);
        self.tools
            .delete_tool(id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete tool: {}", e))
    }

    async fn invoke_tool(
        &self,
        invocation: ToolInvocation,
    ) -> Result<ToolInvocationResult, String> {
        // Get the tool
        let tool = match self.get_tool(&invocation.tool_id).await {
            Ok(Some(tool)) => tool,
            Ok(None) => return Err(format!("Tool with ID {} not found", invocation.tool_id)),
            Err(e) => return Err(format!("Failed to get tool: {}", e)),
        };

        // Validate parameters
        if let Err(e) = tool.validate_parameters(&invocation.parameters) {
            return Err(e);
        }

        // Get the server endpoint using async lock
        let _server_endpoint = {
            let servers = self.registered_servers.lock().await;
            match servers.iter().find(|&s| s == &tool.server_id) {
                Some(endpoint) => endpoint.clone(),
                None => return Err(format!("Server with ID {} not registered", tool.server_id)),
            }
        };

        // Execute via runtime executor if a manifest exists
        let started_at = Utc::now();
        let result = if let Some(stored) = self.manifests.lock().await.get(&tool.id) {
            // Choose executor
            let args = invocation.parameters.clone();
            // Validate parameters against manifest schema if present
            if let Some(validator) = &stored.params_validator {
                if validator.validate(&args).is_err() {
                    return Err("Parameters failed schema validation".to_string());
                }
            }
            let exec_result = match &stored.runtime {
                ToolRuntime::Process(_) => self
                    .proc_exec
                    .invoke(&tool.id, &stored.runtime, &args, &stored.policy)
                    .await
                    .map_err(|e| e.to_string()),
                ToolRuntime::Wasm(_) => self
                    .wasm_exec
                    .invoke(&tool.id, &stored.runtime, &args, &stored.policy)
                    .await
                    .map_err(|e| e.to_string()),
            };
            let v = match exec_result {
                Ok(v) => v,
                Err(e) => {
                    println!("[wasm exec error] {}", e);
                    serde_json::json!({"content":[{"type":"text","text":format!("error: {}", e)}],"isError":true})
                },
            };
            // Optionally validate returns
            if let Some(validator) = &stored.returns_validator {
                if validator.validate(&v).is_err() {
                    warn!("tool {} returned payload that failed returns schema validation", tool.id);
                    return Err("Tool returned payload failing returns schema".to_string());
                }
            }
            v
        } else {
            serde_json::Value::Null
        };

        let completed_at = Utc::now();

        // Create the invocation result
        let invocation_result = ToolInvocationResult {
            invocation,
            result,
            error: None,
            started_at,
            completed_at,
        };

        Ok(invocation_result)
    }
}


impl std::error::Error for ToolRegistryServer {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self)
    }
}

impl std::fmt::Display for ToolRegistryServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ToolRegistryServer")
    }
}

#[async_trait]
impl McpServer for ToolRegistryServer {
    async fn handle(&self, method: &str, params: serde_json::Value) -> HandlerResult {
        match method {
            "RegisterTool" => {
                let request: RegisterToolRequest = serde_json::from_value(params)?;
                match self.register_tool(request).await {
                    Ok(tool) => Ok(serde_json::to_value(RegisterToolResponse {
                        tool_id: tool.id,
                    })?),
                    Err(e) => Err(e.into()),
                }
            }
            "ListTools" => {
                let request: ListToolsRequest = serde_json::from_value(params)?;
                let mut tools = self.list_tools().await?;
                if let Some(server_id) = request.server_id {
                    tools.retain(|t| t.server_id == server_id);
                }
                if let Some(category) = request.category {
                    tools.retain(|t| t.categories.iter().any(|c| c == &category));
                }
                Ok(serde_json::to_value(ListToolsResponse { tools })?)
            }
            "GetTool" => {
                let request: GetToolRequest = serde_json::from_value(params)?;
                match self.get_tool(&request.tool_id).await {
                    Ok(Some(tool)) => Ok(serde_json::to_value(GetToolResponse { tool })?),
                    Ok(None) => Err(format!("Tool not found: {}", request.tool_id).into()),
                    Err(e) => Err(e.into()),
                }
            }
            "InvokeTool" => {
                let request: InvokeToolRequest = serde_json::from_value(params)?;
                match self.invoke_tool(request.invocation).await {
                    Ok(result) => Ok(serde_json::to_value(InvokeToolResponse { result })?),
                    Err(e) => Err(e.into()),
                }
            }
            "RegisterServer" => {
                // Accept either a simple shape with {server_id} or a full RegisterServerRequest
                let server_id = if params.get("server_id").and_then(|v| v.as_str()).is_some() {
                    params["server_id"].as_str().unwrap().to_string()
                } else if params.get("name").is_some() {
                    // Generate a stable id for the registered server
                    let _request: RegisterServerRequest = serde_json::from_value(params.clone())?;
                    uuid::Uuid::new_v4().to_string()
                } else {
                    return Err("Missing server_id or name".into());
                };

                let registered_id = self.register_server(server_id).await?;
                Ok(serde_json::to_value(RegistrarRegisterServerResponse { server_id: registered_id })?)
            }
            _ => Err(format!("Unknown method: {}", method).into()),
        }
    }
}
