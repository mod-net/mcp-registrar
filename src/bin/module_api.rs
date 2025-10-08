use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::anyhow;
use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Path, Query, State,
    },
    http::{
        header::{ACCEPT, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use futures::{SinkExt, StreamExt};
use jsonschema::Validator;
use mcp_registrar::{
    config::env,
    models::tool::ToolInvocation,
    monitoring,
    servers::{
        prompt_registry::PromptRegistryServer,
        resource_registry::ResourceRegistryServer,
        tool_registry::{InvokeToolRequest, InvokeToolResponse, ToolRegistryServer},
    },
    transport::{HandlerResult, McpServer},
    utils::{chain, ipfs, metadata},
};
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use scrypt::Params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use subxt::{
    config::PolkadotConfig,
    dynamic::{storage, tx, Value as SubxtValue},
    OnlineClient,
};
use subxt_signer::{sr25519, SecretUri};
use tokio::sync::mpsc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, error, info};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    // defaults from env
    chain_rpc_url: String,
    ipfs_base: Option<String>,
    ipfs_api_key: Option<String>,
}

#[derive(Debug, Clone)]
struct ModuleApiError {
    status: StatusCode,
    message: String,
}

impl ModuleApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
}

impl From<ModuleApiError> for (StatusCode, String) {
    fn from(value: ModuleApiError) -> Self {
        (value.status, value.message)
    }
}

#[derive(Clone)]
struct ModuleApiState {
    config: Arc<AppState>,
    dispatcher: Arc<ModuleMcpDispatcher>,
    sse_sessions: Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>,
    http_client: Client,
}

impl ModuleApiState {
    fn _config(&self) -> &AppState {
        self.config.as_ref()
    }

    fn dispatcher(&self) -> Arc<ModuleMcpDispatcher> {
        self.dispatcher.clone()
    }

    fn chain_rpc_url(&self) -> String {
        self.config.chain_rpc_url.clone()
    }

    fn ipfs_base(&self) -> Option<String> {
        self.config.ipfs_base.clone()
    }

    fn ipfs_api_key(&self) -> Option<String> {
        self.config.ipfs_api_key.clone()
    }

    fn sse_sessions(&self) -> Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>> {
        self.sse_sessions.clone()
    }

    fn http_client(&self) -> Client {
        self.http_client.clone()
    }
}

type ApiResult<T> = Result<T, (StatusCode, String)>;

fn resolve_ipfs_base(
    state: &ModuleApiState,
    override_base: Option<String>,
) -> Result<String, ModuleApiError> {
    override_base
        .or_else(|| state.ipfs_base())
        .ok_or_else(|| ModuleApiError::bad_request("missing ipfs_base"))
}

fn resolve_ipfs_api_key(state: &ModuleApiState, override_key: Option<String>) -> Option<String> {
    override_key.or_else(|| state.ipfs_api_key())
}

fn resolve_chain_rpc(state: &ModuleApiState, override_rpc: Option<String>) -> String {
    override_rpc.unwrap_or_else(|| state.chain_rpc_url())
}

#[derive(Debug, Deserialize)]
struct JsonRpcFrame {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Clone)]
struct ModuleMcpDispatcher {
    tool_registry: Arc<ToolRegistryServer>,
    prompt_registry: Arc<PromptRegistryServer>,
    resource_registry: Arc<ResourceRegistryServer>,
}

impl ModuleMcpDispatcher {
    fn new(
        tool_registry: Arc<ToolRegistryServer>,
        prompt_registry: Arc<PromptRegistryServer>,
        resource_registry: Arc<ResourceRegistryServer>,
    ) -> Self {
        Self {
            tool_registry,
            prompt_registry,
            resource_registry,
        }
    }

    async fn handle_initialize(&self, params: Value) -> HandlerResult {
        let client = params
            .get("clientInfo")
            .and_then(|c| c.as_object())
            .ok_or_else(|| anyhow!("Invalid params: missing clientInfo"))?;
        let _capabilities = params
            .get("capabilities")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow!("Invalid params: missing capabilities"))?;
        let proto = params
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let supported = ["2024-11-05", "2024-11-06", "2025-03-26"];
        if !proto.is_empty() && !supported.contains(&proto) {
            return Err(anyhow!(
                "Invalid params: unsupported protocolVersion {} (supported: {:?})",
                proto,
                supported
            )
            .into());
        }

        let name = client
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown-client");
        info!("MCP initialize from client: {}", name);

        Ok(json!({
            "serverInfo": {
                "name": "module-api",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "tools": {},
                "prompts": {},
                "resources": {},
                "metrics": {}
            },
            "protocolVersion": "2024-11-05"
        }))
    }

    async fn handle_tools_list(&self, _params: Value) -> HandlerResult {
        let tools = self
            .tool_registry
            .list_tools()
            .await
            .map_err(|e| anyhow!("Internal error: list tools failed: {}", e))?;

        let items: Vec<Value> = tools
            .into_iter()
            .map(|t| {
                json!({
                    "name": t.id,
                    "description": t.description,
                    "inputSchema": t.parameters_schema.unwrap_or(json!({ "type": "object" })),
                    "metadata": {
                        "version": t.version,
                        "categories": t.categories
                    }
                })
            })
            .collect();

        Ok(json!({ "tools": items, "nextCursor": Value::Null }))
    }

    async fn handle_tools_call(&self, params: Value) -> HandlerResult {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Invalid params: missing name"))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));

        let invocation = ToolInvocation {
            tool_id: name.to_string(),
            parameters: arguments,
            context: None,
        };
        let request = InvokeToolRequest { invocation };

        let raw = self
            .tool_registry
            .handle("InvokeTool", serde_json::to_value(request)?)
            .await?;

        let response: InvokeToolResponse = serde_json::from_value(raw)
            .map_err(|e| anyhow!("Internal error: decode InvokeToolResponse failed: {}", e))?;

        Ok(wrap_tool_result_for_mcp(response.result.result))
    }

    async fn handle_prompts_list(&self, _params: Value) -> HandlerResult {
        let value = self
            .prompt_registry
            .handle("ListPrompts", json!({}))
            .await?;

        let prompts = value
            .get("prompts")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let items: Vec<Value> = prompts
            .into_iter()
            .map(|p| {
                let mut args: Vec<Value> = Vec::new();
                if let Some(schema) = p.get("variables_schema") {
                    let required = schema
                        .get("required")
                        .and_then(|r| r.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let properties = schema
                        .get("properties")
                        .and_then(|r| r.as_object())
                        .cloned()
                        .unwrap_or_default();
                    for (name, prop) in properties.into_iter() {
                        let required_flag = required
                            .iter()
                            .any(|item| item.as_str() == Some(&name));
                        let description = prop
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        args.push(json!({
                            "name": name,
                            "required": required_flag,
                            "description": description
                        }));
                    }
                }
                json!({
                    "name": p.get("name").cloned().unwrap_or(Value::String("unknown".into())),
                    "description": p.get("description").cloned().unwrap_or(Value::String("".into())),
                    "arguments": args
                })
            })
            .collect();

        Ok(json!({ "prompts": items, "nextCursor": Value::Null }))
    }

    async fn handle_prompts_get(&self, params: Value) -> HandlerResult {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Invalid params: missing name"))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let list = self
            .prompt_registry
            .handle("ListPrompts", json!({}))
            .await?;
        let prompts = list
            .get("prompts")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let prompt = prompts
            .into_iter()
            .find(|p| p.get("name").and_then(|v| v.as_str()) == Some(name))
            .ok_or_else(|| anyhow!("Prompt not found: {}", name))?;

        if let Some(schema) = prompt.get("variables_schema") {
            if let Ok(validator) = Validator::new(schema) {
                if let Err(_err) = validator.validate(&arguments) {
                    return Err(anyhow!("Invalid params: prompt arguments failed schema").into());
                }
            }
        }

        let id = prompt
            .get("id")
            .cloned()
            .ok_or_else(|| anyhow!("Invalid prompt: missing id"))?;

        let render = json!({
            "render": {
                "prompt_id": id,
                "variables": arguments
            }
        });

        let rendered = self.prompt_registry.handle("RenderPrompt", render).await?;

        let text = rendered
            .get("result")
            .and_then(|r| r.get("rendered_text"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(json!({
            "content": [{"type": "text", "text": text}],
            "isError": false
        }))
    }

    async fn handle_resources_list(&self, _params: Value) -> HandlerResult {
        let value = self
            .resource_registry
            .handle("ListResources", json!({}))
            .await?;

        let items: Vec<Value> = value
            .get("resources")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|resource| {
                let id = resource.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = resource.get("name").and_then(|v| v.as_str()).unwrap_or("");
                json!({
                    "uri": format!("registry://resource/{}", id),
                    "name": name,
                    "mimeType": resource
                        .get("mime_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("text/plain")
                })
            })
            .collect();

        Ok(json!({ "resources": items, "nextCursor": Value::Null }))
    }

    async fn handle_resources_read(&self, params: Value) -> HandlerResult {
        let uri = params
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Invalid params: missing uri"))?;
        let resource_id = uri
            .strip_prefix("registry://resource/")
            .ok_or_else(|| anyhow!("Invalid params: unsupported uri scheme"))?;

        let parameters = params
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !parameters.is_object() {
            return Err(anyhow!("Invalid params: parameters must be an object").into());
        }

        let query = json!({
            "query": {
                "resource_id": resource_id,
                "parameters": parameters
            }
        });

        let response = self
            .resource_registry
            .handle("QueryResource", query)
            .await?;

        let result = response.get("result").cloned().unwrap_or_else(|| json!({}));

        let (mime, content_value) = if let Some(obj) = result.as_object() {
            match (obj.get("mimeType"), obj.get("text"), obj.get("data")) {
                (Some(mt), Some(text), _) if mt.is_string() && text.is_string() => (
                    mt.as_str().unwrap().to_string(),
                    json!({"text": text.as_str().unwrap()}),
                ),
                (Some(mt), _, Some(data)) if mt.is_string() && data.is_string() => (
                    mt.as_str().unwrap().to_string(),
                    json!({"data": data.as_str().unwrap()}),
                ),
                _ => (
                    "application/json".to_string(),
                    json!({"text": serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())}),
                ),
            }
        } else {
            (
                "application/json".to_string(),
                json!({"text": serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())}),
            )
        };

        let mut item = Map::new();
        item.insert("uri".into(), Value::String(uri.to_string()));
        item.insert("mimeType".into(), Value::String(mime));
        if let Some(text) = content_value.get("text") {
            item.insert("text".into(), text.clone());
        }
        if let Some(data) = content_value.get("data") {
            item.insert("data".into(), data.clone());
        }

        Ok(json!({ "contents": [Value::Object(item)] }))
    }

    async fn handle_metrics_get(&self) -> HandlerResult {
        let (invocations, errors, total_ms, max_ms, total_bytes) =
            monitoring::TOOL_METRICS.snapshot();
        Ok(json!({
            "tool": {
                "invocations": invocations,
                "errors": errors,
                "totalDurationMs": total_ms,
                "maxDurationMs": max_ms,
                "totalBytes": total_bytes
            }
        }))
    }
}

fn wrap_tool_result_for_mcp(inner: Value) -> Value {
    if let Some(arr) = inner.get("content").and_then(|c| c.as_array()) {
        let is_error = inner
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        return json!({
            "content": arr,
            "isError": is_error,
        });
    }

    let is_error = inner
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    match inner {
        Value::String(s) => json!({
            "content": [{ "type": "text", "text": s }],
            "isError": is_error,
        }),
        other => {
            let text = match serde_json::to_string_pretty(&other) {
                Ok(serialized) => serialized,
                Err(_) => other.to_string(),
            };
            json!({
                "content": [{ "type": "text", "text": text }],
                "isError": is_error,
            })
        }
    }
}

fn build_success_response(id: &Option<Value>, result: Value) -> Value {
    let mut obj = Map::new();
    obj.insert("jsonrpc".into(), Value::String("2.0".into()));
    if let Some(identifier) = id {
        obj.insert("id".into(), identifier.clone());
    }
    obj.insert("result".into(), result);
    Value::Object(obj)
}

fn build_error_response(id: &Option<Value>, code: i64, message: &str) -> Value {
    let mut error_obj = Map::new();
    error_obj.insert("code".into(), Value::Number(code.into()));
    error_obj.insert("message".into(), Value::String(message.to_string()));

    let mut obj = Map::new();
    obj.insert("jsonrpc".into(), Value::String("2.0".into()));
    if let Some(identifier) = id {
        obj.insert("id".into(), identifier.clone());
    }
    obj.insert("error".into(), Value::Object(error_obj));
    Value::Object(obj)
}

fn error_code_from_message(message: &str) -> i64 {
    if message.starts_with("Unknown method") || message.starts_with("Method not found") {
        -32601
    } else if message.starts_with("Invalid params") {
        -32602
    } else if message.starts_with("Invalid Request") {
        -32600
    } else {
        -32603
    }
}

// ===== keytools integration: load SURI from encrypted key file =====
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncBlobV1 {
    version: u8,
    kdf: String,
    salt: String,
    params: EncParams,
    nonce: String,
    ciphertext: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncParams {
    n: u32,
    r: u32,
    p: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyJsonMinimal {
    secret_phrase: Option<String>,
}

fn decrypt_key(
    blob: &EncBlobV1,
    password: &str,
) -> Result<KeyJsonMinimal, Box<dyn std::error::Error>> {
    if blob.kdf.to_lowercase() != "scrypt" {
        return Err("Unsupported KDF".into());
    }
    let salt = general_purpose::STANDARD.decode(&blob.salt)?;
    let n = blob.params.n.max(1);
    let r = blob.params.r.max(1);
    let p = blob.params.p.max(1);
    let log_n = (31 - n.leading_zeros()) as u8;
    let params = Params::new(log_n, r, p, 32)?;
    let mut key = [0u8; 32];
    scrypt::scrypt(password.as_bytes(), &salt, &params, &mut key)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = general_purpose::STANDARD.decode(&blob.nonce)?;
    let ct = general_purpose::STANDARD.decode(&blob.ciphertext)?;
    let pt = cipher
        .decrypt(Nonce::from_slice(&nonce), ct.as_ref())
        .map_err(|_| "Decryption failed: wrong password or corrupted key file")?;
    let kj: KeyJsonMinimal = serde_json::from_slice(&pt)?;
    Ok(kj)
}

fn load_suri_from_keytools(
    name: &str,
    password: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let file = env::keys_dir().join(if name.ends_with(".json") {
        name.to_string()
    } else {
        format!("{}.json", name)
    });
    let blob: EncBlobV1 = serde_json::from_slice(&std::fs::read(&file)?)?;
    let kj = decrypt_key(&blob, password)?;
    if let Some(phrase) = kj.secret_phrase {
        Ok(phrase)
    } else {
        Err("key file does not contain a secret phrase; cannot build SURI".into())
    }
}

#[derive(Deserialize)]
struct DigestRequest {
    artifact_uri: Option<String>,
    artifact_base64: Option<String>,
    // optional overrides
    ipfs_base: Option<String>,
    ipfs_api_key: Option<String>,
}

#[derive(Serialize)]
struct DigestResponse {
    digest: String,
}

async fn publish_digest(
    State(state): State<ModuleApiState>,
    Json(req): Json<DigestRequest>,
) -> ApiResult<Json<DigestResponse>> {
    let mut artifact_uri = req.artifact_uri.clone().unwrap_or_default();
    if artifact_uri.is_empty() && req.artifact_base64.is_none() {
        return Err(ModuleApiError::bad_request("provide artifact_uri or artifact_base64").into());
    }
    let http_client = state.http_client();
    if let Some(b64) = req.artifact_base64.as_ref() {
        debug!(size = b64.len(), "uploading artifact bytes to IPFS");
        let bytes = general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| ModuleApiError::bad_request(format!("artifact_base64: {}", e)))?;
        let ipfs_base: String = resolve_ipfs_base(&state, req.ipfs_base.clone())
            .map_err(|err: ModuleApiError| -> (StatusCode, String) { err.into() })?;
        let ipfs_api_key_eff = resolve_ipfs_api_key(&state, req.ipfs_api_key.clone());
        let cid = upload_bytes_to_commune_ipfs(
            &http_client,
            &ipfs_base,
            &ipfs_api_key_eff,
            &bytes,
            "artifact.bin",
        )
        .await
        .map_err(internal)?;
        artifact_uri = format!("ipfs://{}", cid);
    } else if !artifact_uri.starts_with("ipfs://") {
        return Err(ModuleApiError::bad_request(
            "artifact_uri must be ipfs:// or provide artifact_base64",
        )
        .into());
    }
    let art_bytes = ipfs::fetch_ipfs_bytes(&artifact_uri)
        .await
        .map_err(internal)?;
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(&art_bytes);
    let digest = h.finalize();
    let digest_hex = hex::encode(digest);
    Ok(Json(DigestResponse {
        digest: format!("sha256:{}", digest_hex),
    }))
}

async fn register_build() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "register/build is not implemented; submit a fully signed extrinsic via register/submit"
            .into(),
    ))
}

async fn register_submit() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "register/submit is not implemented; provide signed extrinsic or use /modules/register"
            .into(),
    ))
}

#[derive(Deserialize)]
struct PublishRequest {
    // Either artifact_uri or artifact_base64 must be provided
    artifact_uri: Option<String>,
    artifact_base64: Option<String>,
    module_id: String,
    // client-provided cryptographic binding
    digest: String,    // e.g., "sha256:<hex>"
    signature: String, // base64 or 128-hex sr25519 signature over digest with context "module_digest"
    #[serde(default)]
    version: Option<String>,
    // if true, client is expected to register on-chain via signed extrinsic (use register/build + register/submit)
    #[serde(default)]
    publish: bool,
    // overrides
    ipfs_base: Option<String>,
    ipfs_api_key: Option<String>,
    chain_rpc_url: Option<String>,
}

fn _default_suri() -> String {
    "//Alice".to_string()
}

#[derive(Serialize)]
struct PublishResponse {
    metadata_cid: String,
    artifact_uri: String,
    registered: bool,
}

#[derive(Deserialize)]
struct RegisterRequest {
    module_id: String,
    metadata_cid: String,
    // Optional explicit SURI if not using keytools name/password
    suri: Option<String>,
    chain_rpc_url: Option<String>,
    // optional: use keytools-stored key instead of SURI
    key_name: Option<String>,
    key_password: Option<String>,
}

#[derive(Serialize)]
struct RegisterResponse {
    ok: bool,
}

#[derive(Deserialize)]
struct QueryParams {
    raw: Option<bool>,
    no_verify: Option<bool>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum QueryResponse {
    Raw { cid: String },
    Metadata { metadata: serde_json::Value },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let state = AppState {
        chain_rpc_url: env::chain_rpc_url(),
        ipfs_base: env::ipfs_api_url(),
        ipfs_api_key: env::ipfs_api_key(),
    };

    let tool_registry = Arc::new(ToolRegistryServer::new());
    tool_registry.initialize().await?;
    let prompt_registry = Arc::new(PromptRegistryServer::new());
    let resource_registry = Arc::new(ResourceRegistryServer::new());

    let dispatcher = Arc::new(ModuleMcpDispatcher::new(
        tool_registry,
        prompt_registry,
        resource_registry,
    ));

    let http_client = Client::builder().build()?;
    let sse_sessions = Arc::new(Mutex::new(HashMap::new()));
    let shared_state = ModuleApiState {
        config: Arc::new(state),
        dispatcher,
        sse_sessions: sse_sessions.clone(),
        http_client,
    };

    let app = Router::new()
        .route("/modules/publish", post(publish))
        .route("/modules/publish/digest", post(publish_digest))
        .route("/modules/register/build", post(register_build))
        .route("/modules/register/submit", post(register_submit))
        .route("/modules/register", post(register))
        .route("/modules/{module_id}", get(query))
        .route("/mcp/sse", get(mcp_sse_stream).post(mcp_sse_post))
        .route("/mcp/ws", get(mcp_ws_upgrade))
        .with_state(shared_state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(DefaultBodyLimit::max(env::module_api_max_upload_bytes()));

    let addr: SocketAddr = env::module_api_addr().parse()?;
    tracing::info!("module_api listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn publish(
    State(state): State<ModuleApiState>,
    Json(req): Json<PublishRequest>,
) -> ApiResult<Json<PublishResponse>> {
    info!(module_id = %req.module_id, publish = req.publish, "modules/publish request received");
    let mut artifact_uri = req.artifact_uri.clone().unwrap_or_default();
    if artifact_uri.is_empty() && req.artifact_base64.is_none() {
        return Err(ModuleApiError::bad_request("provide artifact_uri or artifact_base64").into());
    }

    let ipfs_base: String = resolve_ipfs_base(&state, req.ipfs_base.clone())
        .map_err(|err: ModuleApiError| -> (StatusCode, String) { err.into() })?;
    let ipfs_api_key_eff = resolve_ipfs_api_key(&state, req.ipfs_api_key.clone());
    let http_client = state.http_client();

    if let Some(b64) = req.artifact_base64.as_ref() {
        let bytes = general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| ModuleApiError::bad_request(format!("artifact_base64: {}", e)))?;
        let cid = upload_bytes_to_commune_ipfs(
            &http_client,
            &ipfs_base,
            &ipfs_api_key_eff,
            &bytes,
            "artifact.bin",
        )
        .await
        .map_err(internal)?;
        artifact_uri = format!("ipfs://{}", cid);
    } else if !artifact_uri.starts_with("ipfs://") {
        debug!(uri = %artifact_uri, "fetching artifact from URI for IPFS upload");
        let resp = reqwest::get(&artifact_uri).await.map_err(internal)?;
        if !resp.status().is_success() {
            return Err(internal(format!(
                "fetch {} -> {}",
                artifact_uri,
                resp.status()
            )));
        }
        let bytes = resp.bytes().await.map_err(internal)?.to_vec();
        let cid = upload_bytes_to_commune_ipfs(
            &http_client,
            &ipfs_base,
            &ipfs_api_key_eff,
            &bytes,
            "artifact.bin",
        )
        .await
        .map_err(internal)?;
        artifact_uri = format!("ipfs://{}", cid);
    }

    #[derive(Serialize)]
    struct Metadata<'a> {
        module_id: &'a str,
        artifact_uri: &'a str,
        digest: String,
        signature: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature_scheme: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ipfs_base: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ipfs_api_key: Option<&'a str>,
    }

    let md = Metadata {
        module_id: &req.module_id,
        artifact_uri: &artifact_uri,
        digest: req.digest.clone(),
        signature: req.signature.clone(),
        signature_scheme: Some("sr25519"),
        version: req.version.as_deref(),
        ipfs_base: Some(ipfs_base.as_str()),
        ipfs_api_key: ipfs_api_key_eff.as_deref(),
    };
    let json = serde_json::to_string_pretty(&md).map_err(internal)?;

    let cid_md = upload_bytes_to_commune_ipfs(
        &http_client,
        &ipfs_base,
        &ipfs_api_key_eff,
        json.as_bytes(),
        "metadata.json",
    )
    .await
    .map_err(internal)?;
    info!(module_id = %req.module_id, metadata_cid = %cid_md, artifact_cid = %artifact_uri, "modules/publish stored metadata");

    let mut registered = false;
    if req.publish {
        let rpc = resolve_chain_rpc(&state, req.chain_rpc_url.clone());
        let name = std::env::var("MODULE_API_KEY_NAME")
            .map_err(|_| internal("MODULE_API_KEY_NAME not set"))?;
        let password = std::env::var("MODULE_API_KEY_PASSWORD")
            .map_err(|_| internal("MODULE_API_KEY_PASSWORD not set"))?;
        let suri_from_key = load_suri_from_keytools(&name, &password).map_err(internal)?;
        register_on_chain(&rpc, &suri_from_key, &req.module_id, &cid_md)
            .await
            .map_err(internal)?;
        registered = true;
    }

    Ok(Json(PublishResponse {
        metadata_cid: cid_md,
        artifact_uri,
        registered,
    }))
}

async fn register(
    State(state): State<ModuleApiState>,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<Json<RegisterResponse>> {
    info!(module_id = %req.module_id, metadata_cid = %req.metadata_cid, "modules/register request received");
    let rpc = resolve_chain_rpc(&state, req.chain_rpc_url.clone());
    // Validate signing inputs: either both key_name & key_password, or explicit suri
    if let (Some(name), Some(password)) = (req.key_name.as_ref(), req.key_password.as_ref()) {
        let suri_from_key = load_suri_from_keytools(name, password).map_err(internal)?;
        register_on_chain(&rpc, &suri_from_key, &req.module_id, &req.metadata_cid)
            .await
            .map_err(internal)?;
    } else if let Some(suri) = req.suri.as_ref() {
        register_on_chain(&rpc, suri, &req.module_id, &req.metadata_cid)
            .await
            .map_err(internal)?;
    } else {
        return Err(ModuleApiError::bad_request(
            "Provide either (key_name & key_password) or suri",
        )
        .into());
    }
    Ok(Json(RegisterResponse { ok: true }))
}

async fn query(
    State(state): State<ModuleApiState>,
    Path(module_id): Path<String>,
    Query(q): Query<QueryParams>,
) -> ApiResult<Json<QueryResponse>> {
    let api = OnlineClient::<PolkadotConfig>::from_url(&state.chain_rpc_url())
        .await
        .map_err(internal)?;
    let key = chain::decode_pubkey_from_owner(&module_id).map_err(internal)?;
    let addr = storage(
        "Modules",
        "Modules",
        vec![SubxtValue::from_bytes(key.to_vec())],
    );
    let cid_thunk_opt = api
        .storage()
        .at_latest()
        .await
        .map_err(internal)?
        .fetch(&addr)
        .await
        .map_err(internal)?;
    let cid = if let Some(thunk) = cid_thunk_opt {
        let bytes: Vec<u8> = thunk.as_type::<Vec<u8>>().map_err(internal)?;
        String::from_utf8(bytes).map_err(|_| internal("CID utf8"))?
    } else {
        return Err((StatusCode::NOT_FOUND, "not found".into()));
    };
    if q.raw.unwrap_or(false) {
        return Ok(Json(QueryResponse::Raw { cid }));
    }
    let meta_uri = format!("ipfs://{}", cid);
    let meta_bytes = ipfs::fetch_ipfs_bytes(&meta_uri).await.map_err(internal)?;
    let metadata_json: serde_json::Value = serde_json::from_slice(&meta_bytes).map_err(internal)?;
    if q.no_verify.unwrap_or(false) {
        return Ok(Json(QueryResponse::Metadata {
            metadata: metadata_json,
        }));
    }
    let md = metadata::parse_metadata_v1(&meta_bytes).map_err(internal)?;
    let art_bytes = if md.artifact_uri.starts_with("ipfs://") {
        ipfs::fetch_ipfs_bytes(&md.artifact_uri)
            .await
            .map_err(internal)?
    } else if md.artifact_uri.starts_with("http://") || md.artifact_uri.starts_with("https://") {
        let resp = reqwest::get(&md.artifact_uri).await.map_err(internal)?;
        if !resp.status().is_success() {
            return Err(internal(format!(
                "artifact {} -> {}",
                md.artifact_uri,
                resp.status()
            )));
        }
        resp.bytes().await.map_err(internal)?.to_vec()
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unsupported artifact_uri: {}", md.artifact_uri),
        ));
    };
    chain::verify_digest(&art_bytes, &md.digest).map_err(internal)?;
    Ok(Json(QueryResponse::Metadata {
        metadata: metadata_json,
    }))
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("internal error: {}", e),
    )
}

async fn mcp_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<ModuleApiState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(err) = handle_mcp_websocket(socket, state).await {
            error!("mcp websocket error: {}", err);
        }
    })
}

struct SessionEventStream {
    receiver: mpsc::Receiver<Value>,
    sessions: Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>,
    session_id: String,
}

impl SessionEventStream {
    fn new(
        receiver: mpsc::Receiver<Value>,
        sessions: Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>,
        session_id: String,
    ) -> Self {
        Self {
            receiver,
            sessions,
            session_id,
        }
    }
}

impl futures::Stream for SessionEventStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match Pin::new(&mut this.receiver).poll_recv(cx) {
            Poll::Ready(Some(value)) => match serde_json::to_string(&value) {
                Ok(payload) => {
                    let event = Event::default().data(payload);
                    Poll::Ready(Some(Ok(event)))
                }
                Err(err) => {
                    let fallback = json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": format!("serialize error: {}", err),
                        }
                    });
                    let event = Event::default().data(fallback.to_string());
                    Poll::Ready(Some(Ok(event)))
                }
            },
            Poll::Ready(None) => {
                if let Ok(mut sessions) = this.sessions.lock() {
                    sessions.remove(&this.session_id);
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

async fn mcp_sse_stream(
    State(state): State<ModuleApiState>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let session_id = Uuid::new_v4().to_string();
    let (sender, receiver) = mpsc::channel::<Value>(64);

    {
        let mut sessions = state.sse_sessions.lock().unwrap();
        sessions.insert(session_id.clone(), sender.clone());
    }

    let welcome = json!({
        "jsonrpc": "2.0",
        "method": "session/welcome",
        "params": { "sessionId": session_id.clone() }
    });
    let _ = sender.try_send(welcome);

    let stream = SessionEventStream::new(receiver, state.sse_sessions(), session_id);

    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}

async fn mcp_sse_post(
    State(state): State<ModuleApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, String)> {
    let raw_body = String::from_utf8_lossy(&body);
    let preview: String = raw_body.chars().take(200).collect();
    info!(
        body_len = body.len(),
        body_preview = %preview,
        headers = ?headers,
        "mcp_sse_post request"
    );

    let payload: Value = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid json body: {}", e)))?;
    info!(payload = %payload, "mcp_sse_post parsed payload");
    let (session_hint, frame_value) = extract_session_context(&headers, &payload)?;

    let frame: JsonRpcFrame = serde_json::from_value(frame_value)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid frame: {}", e)))?;

    let frame_id = frame.id.clone();
    let response = match handle_mcp_request(&state, frame).await {
        Ok(value) => value,
        Err(err) => {
            error!("sse handler error: {}", err);
            build_error_response(&frame_id, -32603, &err.to_string())
        }
    };

    if let Some(session_id) = session_hint {
        if let Some(sender) = {
            let sessions = state.sse_sessions.lock().unwrap();
            sessions.get(&session_id).cloned()
        } {
            if sender.send(response.clone()).await.is_ok() {
                let resp = Response::builder()
                    .status(StatusCode::ACCEPTED)
                    .header(CONTENT_TYPE, "text/event-stream")
                    .body(axum::body::Body::empty())
                    .map_err(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("response build error: {}", e),
                        )
                    })?;
                return Ok(resp);
            }
            info!(
                "session {} closed during delivery, falling back to direct response",
                session_id
            );
        } else {
            info!(
                "session {} not found, falling back to direct response",
                session_id
            );
        }
    }

    let prefers_json = headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|value| {
            let mut has_json = false;
            let mut has_sse = false;
            for item in value.split(',') {
                let trimmed = item.trim();
                if trimmed.starts_with("application/json") {
                    has_json = true;
                }
                if trimmed.contains("text/event-stream") {
                    has_sse = true;
                }
            }
            has_json && !has_sse
        })
        .unwrap_or(false);

    if prefers_json {
        let body = serde_json::to_vec(&response).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("encode response: {}", e),
            )
        })?;
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("response build error: {}", e),
                )
            })?;
        return Ok(resp);
    }

    let payload = serde_json::to_string(&response).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("encode response: {}", e),
        )
    })?;
    let body = format!("data: {}\n\n", payload);
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .body(axum::body::Body::from(body))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("response build error: {}", e),
            )
        })?;
    Ok(resp)
}

fn extract_session_context(
    headers: &HeaderMap,
    payload: &Value,
) -> Result<(Option<String>, Value), (StatusCode, String)> {
    if let Some(obj) = payload.as_object() {
        if let (Some(session_id), Some(frame)) = (
            obj.get("session_id").and_then(|v| v.as_str()),
            obj.get("frame"),
        ) {
            return Ok((Some(session_id.to_string()), frame.clone()));
        }
    }

    if let Some(header_val) = headers.get("x-mcp-session") {
        let session_id = header_val
            .to_str()
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("invalid X-MCP-Session header: {}", e),
                )
            })?
            .to_string();
        return Ok((Some(session_id), payload.clone()));
    }

    Ok((None, payload.clone()))
}

async fn handle_mcp_websocket(
    socket: WebSocket,
    state: ModuleApiState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut sender, mut receiver) = socket.split();

    while let Some(msg_result) = receiver.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(err) => {
                error!("websocket receive error: {}", err);
                break;
            }
        };

        let frame = match msg {
            Message::Text(text) => text.to_string(),
            Message::Binary(bytes) => String::from_utf8(bytes.to_vec()).unwrap_or_default(),
            Message::Close(_) => break,
            Message::Ping(data) => {
                sender.send(Message::Pong(data)).await?;
                continue;
            }
            Message::Pong(_) => continue,
        };

        if frame.trim().is_empty() {
            continue;
        }

        let parsed: Result<JsonRpcFrame, _> = serde_json::from_str(&frame);
        let response = match parsed {
            Ok(request) => handle_mcp_request(&state, request).await,
            Err(err) => {
                let msg = format!("Parse error: {}", err);
                let error_value = build_error_response(&None, -32700, &msg);
                Ok(error_value)
            }
        };

        match response {
            Ok(value) => {
                let serialized = serde_json::to_string(&value)?;
                sender.send(Message::Text(serialized.into())).await?;
            }
            Err(err) => {
                error!("handler error: {}", err);
            }
        }
    }

    Ok(())
}

async fn handle_mcp_request(
    state: &ModuleApiState,
    frame: JsonRpcFrame,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let method = frame.method.clone();
    let dispatcher = state.dispatcher();

    if method.is_empty() {
        let err = build_error_response(&frame.id, -32600, "Invalid Request: missing method");
        return Ok(err);
    }

    if method == "notifications/initialized" {
        return Ok(Value::Null);
    }

    let params = frame.params.clone();
    let result = match method.as_str() {
        "initialize" => dispatcher.handle_initialize(params).await,
        "tools/list" => dispatcher.handle_tools_list(params).await,
        "tools/call" => dispatcher.handle_tools_call(params).await,
        "prompts/list" => dispatcher.handle_prompts_list(params).await,
        "prompts/get" => dispatcher.handle_prompts_get(params).await,
        "resources/list" => dispatcher.handle_resources_list(params).await,
        "resources/read" => dispatcher.handle_resources_read(params).await,
        "metrics/get" => dispatcher.handle_metrics_get().await,
        other => Err(anyhow!("Method not found: {}", other).into()),
    };

    match result {
        Ok(value) => Ok(build_success_response(&frame.id, value)),
        Err(err) => {
            let msg = err.to_string();
            let code = error_code_from_message(&msg);
            Ok(build_error_response(&frame.id, code, &msg))
        }
    }
}

async fn upload_bytes_to_commune_ipfs(
    client: &Client,
    base: &str,
    api_key: &Option<String>,
    bytes: &[u8],
    filename: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let base_trim = base.trim_end_matches('/');
    let url_upload = format!("{}/files/upload", base_trim);
    let part = Part::bytes(bytes.to_vec()).file_name(filename.to_string());
    let form = Form::new().part("file", part);
    let mut req = client.post(&url_upload).multipart(form);
    let api_key_eff = api_key
        .clone()
        .or_else(|| std::env::var("IPFS_API_KEY").ok());
    if let Some(key) = api_key_eff.clone() {
        req = req.header("X-API-Key", key);
    }
    let resp = req.send().await?;
    if resp.status().is_success() {
        let v: serde_json::Value = resp.json().await?;
        if let Some(cid) = v.get("cid").and_then(|x| x.as_str()) {
            return Ok(cid.to_string());
        }
        // Fall through if response shape differs
    }

    let url_add = format!("{}/api/v0/add?pin=true", base_trim);
    let part = Part::bytes(bytes.to_vec()).file_name(filename.to_string());
    let form = Form::new().part("file", part);
    let resp = client.post(&url_add).multipart(form).send().await?;
    if !resp.status().is_success() {
        return Err(format!("kubo add {} -> {}", url_add, resp.status()).into());
    }
    let text = resp.text().await?;
    let first = text.lines().next().unwrap_or("");
    let v: serde_json::Value = serde_json::from_str(first)
        .map_err(|e| format!("parse kubo add: {} | body: {}", e, first))?;
    let cid = v
        .get("Hash")
        .and_then(|x| x.as_str())
        .ok_or("missing Hash in kubo add response")?;
    Ok(cid.to_string())
}

async fn register_on_chain(
    rpc: &str,
    suri: &str,
    module_id: &str,
    metadata_cid: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let api = OnlineClient::<PolkadotConfig>::from_url(rpc).await?;
    let kp =
        sr25519::Keypair::from_uri(&SecretUri::from_str(suri).map_err(|e| format!("suri: {}", e))?)
            .map_err(|e| format!("suri: {}", e))?;
    let key = chain::decode_pubkey_from_owner(module_id)?;
    let call = tx(
        "Modules",
        "register_module",
        vec![
            SubxtValue::from_bytes(key.to_vec()),
            SubxtValue::from_bytes(metadata_cid.as_bytes().to_vec()),
        ],
    );
    let mut progress = api
        .tx()
        .sign_and_submit_then_watch_default(&call, &kp)
        .await?;
    while let Some(s) = progress.next().await {
        let _ = s?;
    }
    Ok(())
}
