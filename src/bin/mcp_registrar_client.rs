use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::{engine::general_purpose, Engine as _};
use clap::{Args, Parser, Subcommand};
use mcp_registrar::config::env;
use reqwest::Url;
use scrypt::Params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use subxt_signer::{sr25519, SecretUri};

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

#[derive(Args, Debug)]
struct RegisterModuleArgs {
    /// Module API base URL (e.g., http://127.0.0.1:8090)
    #[arg(long, default_value = "http://127.0.0.1:8090")]
    module_api: Url,
    /// Path to artifact file to upload to IPFS
    #[arg(long)]
    artifact_file: PathBuf,
    /// Module owner SS58 address
    #[arg(long)]
    module_id: String,
    /// Keytools key name (stored in ~/.modnet/keys/<name>.json)
    #[arg(long)]
    key_name: Option<String>,
    /// Password to decrypt keytools key
    #[arg(long)]
    key_password: Option<String>,
    /// Alternative: explicit SURI to sign metadata and for registration
    #[arg(long)]
    suri: Option<String>,
    /// Chain RPC URL (for registration)
    #[arg(long, default_value = "ws://127.0.0.1:9944")]
    chain_rpc_url: String,
    /// Optional IPFS base (overrides server default)
    #[arg(long)]
    ipfs_base: Option<String>,
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

    /// End-to-end module registration against module-api
    #[command(name = "register-module")]
    RegisterModule(RegisterModuleArgs),

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
        Commands::UnregisterServer { id } => ("UnregisterServer", serde_json::json!({ "id": id })),
        Commands::GetServer { id } => ("GetServer", serde_json::json!({ "id": id })),
        Commands::ListServers => ("ListServers", serde_json::json!({})),
        Commands::UpdateServerStatus { id, status } => (
            "UpdateServerStatus",
            serde_json::json!({ "id": id, "status": status }),
        ),
        Commands::Heartbeat { id } => ("Heartbeat", serde_json::json!({ "id": id })),

        Commands::RegisterModule(args) => {
            // 1) Read artifact and compute digest
            let bytes = std::fs::read(&args.artifact_file)?;
            let mut h = Sha256::new();
            h.update(&bytes);
            let digest_hex = hex::encode(h.finalize());
            let digest_str = format!("sha256:{}", digest_hex);
            let artifact_b64 = general_purpose::STANDARD.encode(&bytes);

            // 2) Sign digest locally with provided SURI or keytools key
            let suri = if let Some(s) = &args.suri {
                s.clone()
            } else {
                let name = args
                    .key_name
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("--key-name or --suri required"))?;
                let pw = args
                    .key_password
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("--key-password required with --key-name"))?;
                load_suri_from_keytools(&name, &pw)?
            };
            let kp = sr25519::Keypair::from_uri(
                &SecretUri::from_str(&suri).map_err(|e| anyhow::anyhow!(format!("suri: {}", e)))?,
            )
            .map_err(|e| anyhow::anyhow!(format!("suri: {}", e)))?;
            let signature = kp.sign(digest_str.as_bytes());
            let signature_hex = hex::encode(signature.0);

            // 3) Publish (artifact upload + metadata), publish=false
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(cli.timeout_secs))
                .build()?;
            let mut body = serde_json::json!({
                "artifact_base64": artifact_b64,
                "module_id": args.module_id,
                "digest": digest_str,
                "signature": signature_hex,
                "publish": false,
                "chain_rpc_url": args.chain_rpc_url,
            });
            if let Some(b) = &args.ipfs_base {
                body["ipfs_base"] = serde_json::Value::String(b.clone());
            }
            let pub_resp = client
                .post(args.module_api.join("modules/publish")?)
                .json(&body)
                .send()
                .await?;
            let status = pub_resp.status();
            let text = pub_resp.text().await.unwrap_or_default();
            if !status.is_success() {
                anyhow::bail!("publish: {status} - {text}");
            }
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| anyhow::anyhow!("publish response parse error: {e}; body={text}"))?;
            let metadata_cid = v
                .get("metadata_cid")
                .and_then(|x| x.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing metadata_cid; body={text}"))?
                .to_string();

            // 4) Register on-chain via server (provide same creds)
            let mut reg = serde_json::json!({
                "module_id": args.module_id,
                "metadata_cid": metadata_cid,
                "chain_rpc_url": args.chain_rpc_url,
            });
            if let (Some(n), Some(pw)) = (args.key_name.clone(), args.key_password.clone()) {
                reg["key_name"] = serde_json::Value::String(n);
                reg["key_password"] = serde_json::Value::String(pw);
            } else {
                reg["suri"] = serde_json::Value::String(suri);
            }
            let reg_resp = client
                .post(args.module_api.join("modules/register")?)
                .json(&reg)
                .send()
                .await?;
            if !reg_resp.status().is_success() {
                let status = reg_resp.status();
                let txt = reg_resp.text().await.unwrap_or_default();
                anyhow::bail!("register: {status} - {txt}");
            }
            let out: Value = reg_resp.json().await?;
            return Ok(println!("{}", serde_json::to_string_pretty(&out)?));
        }
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

// ===== keytools decrypt helpers (minimal) =====
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

fn keys_dir() -> PathBuf {
    env::keys_dir()
}

fn load_suri_from_keytools(name: &str, password: &str) -> anyhow::Result<String> {
    let file = keys_dir().join(if name.ends_with(".json") {
        name.to_string()
    } else {
        format!("{}.json", name)
    });
    let blob: EncBlobV1 = serde_json::from_slice(&std::fs::read(&file)?)?;
    if blob.kdf.to_lowercase() != "scrypt" {
        anyhow::bail!("Unsupported KDF");
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
        .map_err(|_| anyhow::anyhow!("Decryption failed: wrong password or corrupted key file"))?;
    let kj: KeyJsonMinimal = serde_json::from_slice(&pt)?;
    if let Some(phrase) = kj.secret_phrase {
        Ok(phrase)
    } else {
        anyhow::bail!("key file does not contain a secret phrase; cannot build SURI")
    }
}
