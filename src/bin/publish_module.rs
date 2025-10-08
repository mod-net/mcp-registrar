use base64::{engine::general_purpose, Engine as _};
use clap::Parser;
use mcp_registrar::config::env;
use mcp_registrar::utils::chain;
use reqwest::blocking::{
    multipart::{Form, Part},
    Client,
};
use schnorrkel::{signing_context, Keypair, MiniSecretKey};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use subxt::dynamic::{tx, Value};
use subxt::{config::PolkadotConfig, OnlineClient};
use subxt_signer::{sr25519, SecretUri};

#[derive(Parser, Debug)]
#[command(
    name = "publish-module",
    about = "Generate signed module metadata (v1)"
)]
struct Args {
    /// Path to artifact bytes (e.g., wasm)
    #[arg(long)]
    artifact: PathBuf,

    /// Module owner id (SS58 address)
    #[arg(long)]
    module_id: String,

    /// Mini secret seed as 64 hex chars (sr25519)
    #[arg(long, value_name = "HEX32")]
    secret_hex: String,

    /// Artifact URI to embed in metadata (e.g., ipfs://<cid>)
    #[arg(long)]
    artifact_uri: String,

    /// Optional version tag
    #[arg(long)]
    version: Option<String>,

    /// Output path for metadata JSON (default: stdout)
    #[arg(long)]
    out: Option<PathBuf>,

    /// If set, upload artifact + metadata to commune-ipfs and register on-chain
    #[arg(long, default_value_t = false)]
    publish: bool,

    /// IPFS API base (e.g., https://host:port). Defaults: IPFS_API_URL, then IPFS_BASE_URL
    #[arg(long)]
    ipfs_base: Option<String>,

    /// Optional API key header (X-API-Key). Defaults: IPFS_API_KEY
    #[arg(long)]
    ipfs_api_key: Option<String>,

    /// Chain RPC URL (ws/wss); defaults CHAIN_RPC_URL
    #[arg(long)]
    chain_rpc_url: Option<String>,

    /// Signer SURI for register extrinsic (e.g., //Alice)
    #[arg(long, default_value = "//Alice")]
    suri: String,
}

fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    let mut t = s.trim();
    if t.starts_with("0x") || t.starts_with("0X") {
        t = &t[2..];
    }
    if t.len() % 2 != 0 {
        return Err("hex length must be even".into());
    }
    (0..t.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&t[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let bytes = fs::read(&args.artifact)?;

    // Compute sha256 digest
    let mut h = Sha256::new();
    h.update(&bytes);
    let digest = h.finalize();
    let digest_hex = hex::encode(digest);
    let digest_tagged = format!("sha256:{}", digest_hex);

    // Sign digest using sr25519
    // Accept 64-hex (32 bytes) mini-secret, or 128-hex (64 bytes) where the first 32 bytes are the seed.
    let mut secret_hex_input = args.secret_hex.trim().to_string();
    if secret_hex_input.len() == 128 && secret_hex_input.chars().all(|c| c.is_ascii_hexdigit()) {
        // Common layout: 32-byte seed + 32-byte nonce/expansion; use the seed portion
        secret_hex_input = secret_hex_input[..64].to_string();
    }
    let seed = hex_to_bytes(&secret_hex_input).map_err(|e| format!("secret_hex: {}", e))?;
    if seed.len() != 32 {
        return Err("secret_hex must be 32 bytes (64 hex chars)".into());
    }
    let mini = MiniSecretKey::from_bytes(&seed).map_err(|e| format!("mini secret: {}", e))?;
    let kp: Keypair = mini.expand_to_keypair(schnorrkel::ExpansionMode::Ed25519);
    let ctx = signing_context(b"module_digest");
    let sig = kp.sign(ctx.bytes(&digest));
    let sig_b64 = general_purpose::STANDARD.encode(sig.to_bytes());

    // Compose metadata v1
    #[derive(serde::Serialize)]
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

    let mut md = Metadata {
        module_id: &args.module_id,
        artifact_uri: &args.artifact_uri,
        digest: digest_tagged,
        signature: sig_b64,
        signature_scheme: Some("sr25519"),
        version: args.version.as_deref(),
    };
    let mut json = serde_json::to_string_pretty(&md)?;

    if args.publish {
        // 1) Upload artifact (if artifact_uri not already ipfs://cid)
        let mut artifact_uri = args.artifact_uri.clone();
        if !artifact_uri.starts_with("ipfs://") {
            let ipfs_base = args
                .ipfs_base
                .clone()
                .or_else(|| env::ipfs_api_url())
                .ok_or("Set --ipfs-base or IPFS_API_URL for publish")?;
            let cid = upload_to_commune_ipfs(&ipfs_base, &args.ipfs_api_key, &args.artifact)?;
            artifact_uri = format!("ipfs://{}", cid);
            // update metadata
            md.artifact_uri = &artifact_uri;
            json = serde_json::to_string_pretty(&md)?;
        }

        // 2) Upload metadata JSON
        let ipfs_base = args
            .ipfs_base
            .clone()
            .or_else(|| env::ipfs_api_url())
            .ok_or("Set --ipfs-base or IPFS_API_URL for publish")?;
        let cid_md = upload_bytes_to_commune_ipfs(
            &ipfs_base,
            &args.ipfs_api_key,
            json.as_bytes(),
            "metadata.json",
        )?;

        // 3) Register on chain
        let rpc = args
            .chain_rpc_url
            .or_else(|| Some(env::chain_rpc_url()))
            .ok_or("Set --chain-rpc-url or CHAIN_RPC_URL for publish")?;
        register_on_chain(&rpc, &args.suri, &args.module_id, &cid_md)?;
        println!("Registered module: {} -> {}", args.module_id, cid_md);
        Ok(())
    } else {
        if let Some(out) = args.out {
            fs::write(out, json.as_bytes())?;
        } else {
            println!("{}", json);
        }
        eprintln!("Metadata generated. Next steps:\n  1) Upload this JSON to IPFS → metadata_cid\n  2) Submit pallet call register_module(key=SS58 pubkey bytes, cid=metadata_cid)");
        Ok(())
    }
}

fn upload_to_commune_ipfs(
    base: &str,
    api_key: &Option<String>,
    path: &PathBuf,
) -> Result<String, Box<dyn std::error::Error>> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("artifact.bin")
        .to_string();
    let bytes = fs::read(path)?;
    upload_bytes_to_commune_ipfs(base, api_key, &bytes, &file_name)
}

fn upload_bytes_to_commune_ipfs(
    base: &str,
    api_key: &Option<String>,
    bytes: &[u8],
    filename: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let client = Client::builder().build()?;
    // Try FastAPI style first: POST /files/upload
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
    let resp = req.send()?;
    if resp.status().is_success() {
        let v: serde_json::Value = resp.json()?;
        if let Some(cid) = v.get("cid").and_then(|x| x.as_str()) {
            return Ok(cid.to_string());
        }
        // fallthrough to kubo if shape unexpected
    }
    // Fallback to Kubo RPC: POST /api/v0/add?pin=true
    let url_add = format!("{}/api/v0/add?pin=true", base_trim);
    let part = Part::bytes(bytes.to_vec()).file_name(filename.to_string());
    let form = Form::new().part("file", part);
    let resp = client.post(&url_add).multipart(form).send()?;
    if !resp.status().is_success() {
        return Err(format!("kubo add {} -> {}", url_add, resp.status()).into());
    }
    // Kubo returns JSON (text/plain); parse first line
    let text = resp.text()?;
    let first = text.lines().next().unwrap_or("");
    let v: serde_json::Value = serde_json::from_str(first)
        .map_err(|e| format!("parse kubo add: {} | body: {}", e, first))?;
    let cid = v
        .get("Hash")
        .and_then(|x| x.as_str())
        .ok_or("missing Hash in kubo add response")?;
    Ok(cid.to_string())
}

fn register_on_chain(
    rpc: &str,
    suri: &str,
    module_id: &str,
    metadata_cid: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let api = OnlineClient::<PolkadotConfig>::from_url(rpc).await?;
        let kp = sr25519::Keypair::from_uri(
            &SecretUri::from_str(suri).map_err(|e| format!("suri: {}", e))?,
        )
        .map_err(|e| format!("suri: {}", e))?;
        let key = chain::decode_pubkey_from_owner(module_id)?;
        let call = tx(
            "Modules",
            "register_module",
            vec![
                Value::from_bytes(key.to_vec()),
                Value::from_bytes(metadata_cid.as_bytes().to_vec()),
            ],
        );
        let mut progress = api
            .tx()
            .sign_and_submit_then_watch_default(&call, &kp)
            .await?;
        while let Some(s) = progress.next().await {
            let _ = s?;
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}
