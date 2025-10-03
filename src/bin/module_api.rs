use axum::{extract::{Path, Query, State, DefaultBodyLimit}, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use reqwest::blocking::{Client, multipart::{Form, Part}};
use subxt::{config::PolkadotConfig, OnlineClient};
use subxt::dynamic::{tx, storage, Value};
use subxt_signer::{sr25519, SecretUri};
use registry_scheduler::utils::{chain, ipfs, metadata};
use tower_http::cors::{CorsLayer, Any};
use std::str::FromStr;
use std::path::PathBuf;
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit};
use base64::{engine::general_purpose, Engine as _};
use scrypt::Params;
use registry_scheduler::config::env;

#[derive(Clone)]
struct AppState {
    // defaults from env
    chain_rpc_url: String,
    ipfs_base: Option<String>,
    ipfs_api_key: Option<String>,
}

// ===== keytools integration: load SURI from encrypted key file =====
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncBlobV1 { version: u8, kdf: String, salt: String, params: EncParams, nonce: String, ciphertext: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncParams { n: u32, r: u32, p: u32 }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyJsonMinimal { secret_phrase: Option<String> }

fn decrypt_key(blob:&EncBlobV1, password:&str) -> Result<KeyJsonMinimal, Box<dyn std::error::Error>> {
    if blob.kdf.to_lowercase()!="scrypt" { return Err("Unsupported KDF".into()); }
    let salt = general_purpose::STANDARD.decode(&blob.salt)?;
    let n = blob.params.n.max(1); let r = blob.params.r.max(1); let p = blob.params.p.max(1);
    let log_n = (31 - n.leading_zeros()) as u8; let params = Params::new(log_n, r, p, 32)?;
    let mut key=[0u8;32]; scrypt::scrypt(password.as_bytes(), &salt, &params, &mut key)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = general_purpose::STANDARD.decode(&blob.nonce)?; let ct = general_purpose::STANDARD.decode(&blob.ciphertext)?;
    let pt = cipher.decrypt(Nonce::from_slice(&nonce), ct.as_ref()).map_err(|_| "Decryption failed: wrong password or corrupted key file")?;
    let kj: KeyJsonMinimal = serde_json::from_slice(&pt)?;
    Ok(kj)
}

fn load_suri_from_keytools(name: &str, password: &str) -> Result<String, Box<dyn std::error::Error>> {
    let file = env::keys_dir().join(if name.ends_with(".json") { name.to_string() } else { format!("{}.json", name) });
    let blob: EncBlobV1 = serde_json::from_slice(&std::fs::read(&file)?)?;
    let kj = decrypt_key(&blob, password)?;
    if let Some(phrase) = kj.secret_phrase { Ok(phrase) } else { Err("key file does not contain a secret phrase; cannot build SURI".into()) }
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
struct DigestResponse { digest: String }

async fn publish_digest(State(state): State<AppState>, Json(req): Json<DigestRequest>) -> Result<Json<DigestResponse>, (axum::http::StatusCode, String)> {
    let mut artifact_uri = req.artifact_uri.clone().unwrap_or_default();
    if artifact_uri.is_empty() && req.artifact_base64.is_none() {
        return Err((axum::http::StatusCode::BAD_REQUEST, "provide artifact_uri or artifact_base64".into()));
    }
    if let Some(b64) = req.artifact_base64.as_ref() {
        use base64::Engine;
        let bytes = match base64::engine::general_purpose::STANDARD.decode(b64) { Ok(b) => b, Err(e) => return Err((axum::http::StatusCode::BAD_REQUEST, format!("artifact_base64: {}", e))) };
        let ipfs_base = req.ipfs_base.clone().or(state.ipfs_base.clone()).ok_or((axum::http::StatusCode::BAD_REQUEST, "missing ipfs_base".into()))?;
        let ipfs_api_key_eff = req.ipfs_api_key.clone().or(state.ipfs_api_key.clone());
        let cid = upload_bytes_to_commune_ipfs(&ipfs_base, &ipfs_api_key_eff, &bytes, "artifact.bin").map_err(internal)?;
        artifact_uri = format!("ipfs://{}", cid);
    } else if !artifact_uri.starts_with("ipfs://") {
        return Err((axum::http::StatusCode::BAD_REQUEST, "artifact_uri must be ipfs:// or provide artifact_base64".into()));
    }
    let art_bytes = ipfs::fetch_ipfs_bytes(&artifact_uri).await.map_err(internal)?;
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(&art_bytes);
    let digest = h.finalize();
    let digest_hex = hex::encode(digest);
    Ok(Json(DigestResponse { digest: format!("sha256:{}", digest_hex) }))
}

async fn register_build() -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    Err((axum::http::StatusCode::NOT_IMPLEMENTED, "register/build is not implemented; submit a fully signed extrinsic via register/submit".into()))
}

async fn register_submit() -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    Err((axum::http::StatusCode::NOT_IMPLEMENTED, "register/submit is not implemented; provide signed extrinsic or use /modules/register".into()))
}

#[derive(Deserialize)]
struct PublishRequest {
    // Either artifact_uri or artifact_base64 must be provided
    artifact_uri: Option<String>,
    artifact_base64: Option<String>,
    module_id: String,
    // client-provided cryptographic binding
    digest: String,      // e.g., "sha256:<hex>"
    signature: String,   // base64 or 128-hex sr25519 signature over digest with context "module_digest"
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

fn default_suri() -> String { "//Alice".to_string() }

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
struct RegisterResponse { ok: bool }

#[derive(Deserialize)]
struct QueryParams { raw: Option<bool>, no_verify: Option<bool> }

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

    let app = Router::new()
        .route("/modules/publish", post(publish))
        .route("/modules/publish/digest", post(publish_digest))
        .route("/modules/register/build", post(register_build))
        .route("/modules/register/submit", post(register_submit))
        .route("/modules/register", post(register))
        .route("/modules/{module_id}", get(query))
        .with_state(state)
        .layer(CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
        )
        .layer({
            DefaultBodyLimit::max(env::module_api_max_upload_bytes())
        });

    let addr: SocketAddr = env::module_api_addr().parse()?;
    tracing::info!("module_api listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn publish(State(state): State<AppState>, Json(req): Json<PublishRequest>) -> Result<Json<PublishResponse>, (axum::http::StatusCode, String)> {
    // Prepare artifact_uri
    let mut artifact_uri = req.artifact_uri.clone().unwrap_or_default();
    if artifact_uri.is_empty() && req.artifact_base64.is_none() {
        return Err((axum::http::StatusCode::BAD_REQUEST, "provide artifact_uri or artifact_base64".into()));
    }

    // If artifact_base64 is provided or artifact_uri is non-ipfs, upload
    if let Some(b64) = req.artifact_base64.as_ref() {
        use base64::Engine;
        let bytes = match base64::engine::general_purpose::STANDARD.decode(b64) { Ok(b) => b, Err(e) => return Err((axum::http::StatusCode::BAD_REQUEST, format!("artifact_base64: {}", e))) };
        let ipfs_base = req.ipfs_base.clone().or(state.ipfs_base.clone()).ok_or((axum::http::StatusCode::BAD_REQUEST, "missing ipfs_base".into()))?;
        let ipfs_api_key_eff = req.ipfs_api_key.clone().or(state.ipfs_api_key.clone());
        let cid = upload_bytes_to_commune_ipfs(&ipfs_base, &ipfs_api_key_eff, &bytes, "artifact.bin")
            .map_err(internal)?;
        artifact_uri = format!("ipfs://{}", cid);
    } else if !artifact_uri.starts_with("ipfs://") {
        let ipfs_base = req.ipfs_base.clone().or(state.ipfs_base.clone()).ok_or((axum::http::StatusCode::BAD_REQUEST, "missing ipfs_base".into()))?;
        // fetch the artifact via HTTP(s) and upload to ipfs
        let resp = reqwest::get(&artifact_uri).await.map_err(internal)?;
        if !resp.status().is_success() { return Err(internal(format!("fetch {} -> {}", artifact_uri, resp.status()))); }
        let bytes = resp.bytes().await.map_err(internal)?.to_vec();
        let ipfs_api_key_eff = req.ipfs_api_key.clone().or(state.ipfs_api_key.clone());
        let cid = upload_bytes_to_commune_ipfs(&ipfs_base, &ipfs_api_key_eff, &bytes, "artifact.bin")
            .map_err(internal)?;
        artifact_uri = format!("ipfs://{}", cid);
    }

    // Compose metadata using client-provided digest and signature
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
    }
    // Trust client inputs
    let md = Metadata {
        module_id: &req.module_id,
        artifact_uri: &artifact_uri,
        digest: req.digest.clone(),
        signature: req.signature.clone(),
        signature_scheme: Some("sr25519"),
        version: req.version.as_deref(),
    };
    let json = serde_json::to_string_pretty(&md).map_err(internal)?;

    // Upload metadata JSON
    let ipfs_base = req
        .ipfs_base
        .clone()
        .or(state.ipfs_base.clone())
        .ok_or((axum::http::StatusCode::BAD_REQUEST, "missing ipfs_base".into()))?;
    let ipfs_api_key_eff = req.ipfs_api_key.clone().or(state.ipfs_api_key.clone());
    let cid_md = upload_bytes_to_commune_ipfs(&ipfs_base, &ipfs_api_key_eff, json.as_bytes(), "metadata.json")
        .map_err(internal)?;

    // Optionally register on-chain
    let mut registered = false;
    if req.publish {
        let rpc = req
            .chain_rpc_url
            .clone()
            .unwrap_or_else(|| state.chain_rpc_url.clone());
        // Require configured key name/password for auto-register, no fallback
        let name = std::env::var("MODULE_API_KEY_NAME").map_err(|_| internal("MODULE_API_KEY_NAME not set"))?;
        let password = std::env::var("MODULE_API_KEY_PASSWORD").map_err(|_| internal("MODULE_API_KEY_PASSWORD not set"))?;
        let suri_from_key = load_suri_from_keytools(&name, &password).map_err(internal)?;
        register_on_chain(&rpc, &suri_from_key, &req.module_id, &cid_md).map_err(internal)?;
        registered = true;
    }

    Ok(Json(PublishResponse { metadata_cid: cid_md, artifact_uri, registered }))
}

async fn register(State(state): State<AppState>, Json(req): Json<RegisterRequest>) -> Result<Json<RegisterResponse>, (axum::http::StatusCode, String)> {
    let rpc = req.chain_rpc_url.clone().unwrap_or_else(|| state.chain_rpc_url.clone());
    // Validate signing inputs: either both key_name & key_password, or explicit suri
    if let (Some(name), Some(password)) = (req.key_name.as_ref(), req.key_password.as_ref()) {
        let suri_from_key = load_suri_from_keytools(name, password).map_err(internal)?;
        register_on_chain(&rpc, &suri_from_key, &req.module_id, &req.metadata_cid).map_err(internal)?;
    } else if let Some(suri) = req.suri.as_ref() {
        register_on_chain(&rpc, suri, &req.module_id, &req.metadata_cid).map_err(internal)?;
    } else {
        return Err((axum::http::StatusCode::BAD_REQUEST, "Provide either (key_name & key_password) or suri".into()));
    }
    Ok(Json(RegisterResponse { ok: true }))
}

async fn query(State(state): State<AppState>, Path(module_id): Path<String>, Query(q): Query<QueryParams>) -> Result<Json<QueryResponse>, (axum::http::StatusCode, String)> {
    let api = OnlineClient::<PolkadotConfig>::from_url(&state.chain_rpc_url).await.map_err(internal)?;
    let key = chain::decode_pubkey_from_owner(&module_id).map_err(internal)?;
    let addr = storage("Modules", "Modules", vec![Value::from_bytes(key.to_vec())]);
    let cid_thunk_opt = api.storage().at_latest().await.map_err(internal)?.fetch(&addr).await.map_err(internal)?;
    let cid = if let Some(thunk) = cid_thunk_opt {
        let bytes: Vec<u8> = thunk.as_type::<Vec<u8>>().map_err(internal)?;
        String::from_utf8(bytes).map_err(|_| internal("CID utf8"))?
    } else {
        return Err((axum::http::StatusCode::NOT_FOUND, "not found".into()));
    };
    if q.raw.unwrap_or(false) {
        return Ok(Json(QueryResponse::Raw { cid }));
    }
    let meta_uri = format!("ipfs://{}", cid);
    let meta_bytes = ipfs::fetch_ipfs_bytes(&meta_uri).await.map_err(internal)?;
    if q.no_verify.unwrap_or(false) {
        let v: serde_json::Value = serde_json::from_slice(&meta_bytes).map_err(internal)?;
        return Ok(Json(QueryResponse::Metadata { metadata: v }));
    }
    let md = metadata::parse_metadata_v1(&meta_bytes).map_err(internal)?;
    let art_bytes = if md.artifact_uri.starts_with("ipfs://") {
        ipfs::fetch_ipfs_bytes(&md.artifact_uri).await.map_err(internal)?
    } else if md.artifact_uri.starts_with("http://") || md.artifact_uri.starts_with("https://") {
        let resp = reqwest::get(&md.artifact_uri).await.map_err(internal)?;
        if !resp.status().is_success() { return Err(internal(format!("artifact {} -> {}", md.artifact_uri, resp.status()))); }
        resp.bytes().await.map_err(internal)?.to_vec()
    } else {
        return Err((axum::http::StatusCode::BAD_REQUEST, format!("unsupported artifact_uri: {}", md.artifact_uri)));
    };
    chain::verify_digest(&art_bytes, &md.digest).map_err(internal)?;
    chain::verify_signature_sr25519(&art_bytes, &Some(md.digest.clone()), &module_id, &md.signature).map_err(internal)?;
    let v: serde_json::Value = serde_json::from_slice(&meta_bytes).map_err(internal)?;
    Ok(Json(QueryResponse::Metadata { metadata: v }))
}

fn internal<E: std::fmt::Display>(e: E) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn upload_bytes_to_commune_ipfs(base: &str, api_key: &Option<String>, bytes: &[u8], filename: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = Client::builder().build()?;
    let base_trim = base.trim_end_matches('/');
    // Try FastAPI style first: POST /files/upload
    let url_upload = format!("{}/files/upload", base_trim);
    let part = Part::bytes(bytes.to_vec()).file_name(filename.to_string());
    let form = Form::new().part("file", part);
    let mut req = client.post(&url_upload).multipart(form);
    let api_key_eff = api_key.clone().or_else(|| std::env::var("IPFS_API_KEY").ok());
    if let Some(key) = api_key_eff.clone() { req = req.header("X-API-Key", key); }
    let resp = req.send()?;
    if resp.status().is_success() {
        let v: serde_json::Value = resp.json()?;
        if let Some(cid) = v.get("cid").and_then(|x| x.as_str()) { return Ok(cid.to_string()); }
        // fallthrough to kubo if shape unexpected
    }
    // Fallback to Kubo RPC: POST /api/v0/add?pin=true
    let url_add = format!("{}/api/v0/add?pin=true", base_trim);
    let part = Part::bytes(bytes.to_vec()).file_name(filename.to_string());
    let form = Form::new().part("file", part);
    let resp = client.post(&url_add).multipart(form).send()?;
    if !resp.status().is_success() { return Err(format!("kubo add {} -> {}", url_add, resp.status()).into()); }
    // Kubo returns JSON (text/plain); parse first line
    let text = resp.text()?;
    let first = text.lines().next().unwrap_or("");
    let v: serde_json::Value = serde_json::from_str(first).map_err(|e| format!("parse kubo add: {} | body: {}", e, first))?;
    let cid = v.get("Hash").and_then(|x| x.as_str()).ok_or("missing Hash in kubo add response")?;
    Ok(cid.to_string())
}

fn register_on_chain(rpc: &str, suri: &str, module_id: &str, metadata_cid: &str) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let api = OnlineClient::<PolkadotConfig>::from_url(rpc).await?;
        let kp = sr25519::Keypair::from_uri(&SecretUri::from_str(suri).map_err(|e| format!("suri: {}", e))?)
            .map_err(|e| format!("suri: {}", e))?;
        let key = chain::decode_pubkey_from_owner(module_id)?;
        let call = tx("Modules", "register_module", vec![Value::from_bytes(key.to_vec()), Value::from_bytes(metadata_cid.as_bytes().to_vec())]);
        let mut progress = api.tx().sign_and_submit_then_watch_default(&call, &kp).await?;
        while let Some(s) = progress.next().await { let _ = s?; }
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}
