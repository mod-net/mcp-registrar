use crate::error::Error;
use base64::{engine::general_purpose, Engine as _};
use blake2::Blake2b512;
use bs58;
use schnorrkel::{PublicKey as Sr25519PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulePointer {
    pub module_id: String,
    pub uri: String,               // ipfs://..., https://..., file://..., etc.
    pub owner: String,             // on-chain owner/account id
    pub digest: Option<String>,    // hex/base64 digest
    pub signature: Option<String>, // owner-signed digest
    pub version: Option<String>,
}

#[async_trait::async_trait]
pub trait ChainIndex: Send + Sync {
    async fn fetch_module_pointer(&self, module_id: &str) -> Result<ModulePointer, Error>;
}

/// HTTP implementation that calls a chain indexer or gateway.
/// Expected endpoint shape: GET {base}/modules/{id}
#[derive(Clone)]
pub struct HttpChainIndex {
    base: String,
    client: reqwest::Client,
}

impl HttpChainIndex {
    pub fn from_env() -> Result<Self, Error> {
        let base = std::env::var("CHAIN_INDEX_URL")
            .map_err(|_| Error::InvalidState("CHAIN_INDEX_URL not set".into()))?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(Self { base, client })
    }
}

#[async_trait::async_trait]
impl ChainIndex for HttpChainIndex {
    async fn fetch_module_pointer(&self, module_id: &str) -> Result<ModulePointer, Error> {
        let url = format!("{}/modules/{}", self.base.trim_end_matches('/'), module_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Serialization(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::InvalidState(format!(
                "chain index {} -> {}",
                url,
                resp.status()
            )));
        }
        let mp: ModulePointer = resp
            .json()
            .await
            .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(mp)
    }
}

/// File-based chain index for tests and offline/dev use.
/// Supports the following JSON shapes:
/// 1) Object map: { "<module_id>": { ModulePointer... }, ... }
/// 2) Wrapped map: { "modules": { "<module_id>": { ... } } }
/// 3) Array: [ { ModulePointer... }, ... ] where each item includes `module_id`.
#[derive(Clone)]
pub struct FileChainIndex {
    path: String,
}

impl FileChainIndex {
    pub fn from_env() -> Result<Self, Error> {
        let path = std::env::var("CHAIN_INDEX_FILE")
            .map_err(|_| Error::InvalidState("CHAIN_INDEX_FILE not set".into()))?;
        Ok(Self { path })
    }

    fn load_map(&self) -> Result<HashMap<String, ModulePointer>, Error> {
        let p = Path::new(&self.path);
        let data = fs::read_to_string(p)?;
        let v: serde_json::Value = serde_json::from_str(&data)?;
        // Try wrapped map { modules: { id: mp } }
        if let Some(mods) = v.get("modules").and_then(|m| m.as_object()) {
            let mut out = HashMap::new();
            for (k, mv) in mods {
                // Ensure module_id is present before deserializing
                let obj = mv.clone();
                let mut obj_map = obj
                    .as_object()
                    .cloned()
                    .ok_or_else(|| Error::Serialization("expected object".into()))?;
                obj_map
                    .entry("module_id".to_string())
                    .or_insert(serde_json::Value::String(k.clone()));
                let mp: ModulePointer = serde_json::from_value(serde_json::Value::Object(obj_map))?;
                out.insert(k.clone(), mp);
            }
            return Ok(out);
        }
        // Try direct map { id: mp }
        if let Some(map) = v.as_object() {
            let mut out = HashMap::new();
            let mut any = false;
            for (k, mv) in map {
                if mv.is_object() {
                    let mut obj_map = mv.as_object().cloned().unwrap_or_default();
                    obj_map
                        .entry("module_id".to_string())
                        .or_insert(serde_json::Value::String(k.clone()));
                    if let Ok(mp) =
                        serde_json::from_value::<ModulePointer>(serde_json::Value::Object(obj_map))
                    {
                        out.insert(k.clone(), mp);
                        any = true;
                    }
                }
            }
            if any {
                return Ok(out);
            }
        }
        // Try array [ mp, ... ]
        if let Some(arr) = v.as_array() {
            let mut out = HashMap::new();
            for mv in arr {
                let mp: ModulePointer = serde_json::from_value(mv.clone())?;
                out.insert(mp.module_id.clone(), mp);
            }
            return Ok(out);
        }
        Err(Error::InvalidState(
            "unsupported CHAIN_INDEX_FILE format".into(),
        ))
    }
}

#[async_trait::async_trait]
impl ChainIndex for FileChainIndex {
    async fn fetch_module_pointer(&self, module_id: &str) -> Result<ModulePointer, Error> {
        let map = self.load_map()?;
        map.get(module_id).cloned().ok_or_else(|| Error::NotFound)
    }
}

/// Resolve chain://<module_id> to a concrete URI by querying the index.
pub async fn resolve_chain_uri(module_uri: &str) -> Result<ModulePointer, Error> {
    let id = module_uri
        .strip_prefix("chain://")
        .ok_or_else(|| Error::InvalidState("invalid chain uri".into()))?;
    // Precedence: CHAIN_INDEX_FILE (local) > CHAIN_INDEX_URL (HTTP)
    if std::env::var("CHAIN_INDEX_FILE").is_ok() {
        let index = FileChainIndex::from_env()?;
        return index.fetch_module_pointer(id).await;
    }
    if std::env::var("CHAIN_INDEX_URL").is_ok() {
        let index = HttpChainIndex::from_env()?;
        return index.fetch_module_pointer(id).await;
    }
    #[cfg(feature = "chain-rpc")]
    {
        if std::env::var("CHAIN_RPC_URL").is_ok() {
            return crate::utils::chain_rpc::resolve_via_rpc(module_uri).await;
        }
    }
    Err(Error::InvalidState("Set CHAIN_INDEX_FILE or CHAIN_INDEX_URL to resolve chain:// (or enable feature `chain-rpc` and set CHAIN_RPC_URL)".into()))
}

/// Verify bytes against a provided digest string.
/// Supports plain hex, `sha256:<hex>`, or base64 (with optional `sha256:` prefix).
pub fn verify_digest(bytes: &[u8], digest_str: &str) -> Result<(), Error> {
    let s = digest_str.trim();
    let algo_trimmed = s.strip_prefix("sha256:").unwrap_or(s);
    // Try hex first
    let expected: Vec<u8> =
        if algo_trimmed.chars().all(|c| c.is_ascii_hexdigit()) && algo_trimmed.len() % 2 == 0 {
            (0..algo_trimmed.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&algo_trimmed[i..i + 2], 16).unwrap_or(0))
                .collect()
        } else {
            general_purpose::STANDARD
                .decode(algo_trimmed)
                .map_err(|e| Error::Serialization(e.to_string()))?
        };
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = hasher.finalize();
    if actual.as_slice() == expected.as_slice() {
        Ok(())
    } else {
        Err(Error::InvalidState("module digest mismatch".into()))
    }
}

pub fn decode_pubkey_from_owner(owner: &str) -> Result<[u8; 32], Error> {
    // Accept hex public key (64 hex chars)
    let o = owner.trim();
    if o.len() == 64 && o.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = u8::from_str_radix(&o[i * 2..i * 2 + 2], 16)
                .map_err(|e| Error::Serialization(e.to_string()))?;
        }
        return Ok(out);
    }
    // Proper SS58 decode (AccountId 32 bytes). Format (common case): [address_type(1)] [pubkey(32)] [checksum(2)]
    let data = bs58::decode(o)
        .into_vec()
        .map_err(|e| Error::Serialization(e.to_string()))?;
    if data.len() != 35 {
        return Err(Error::InvalidState("unsupported SS58 length".into()));
    }
    let _addr_type = data[0]; // for Substrate generic it's 42 (0x2A), but accept any for now
    let pubkey = &data[1..33];
    let checksum = &data[33..35];
    // Verify checksum: first 2 bytes of blake2b("SS58PRE" ++ [type|pubkey])
    let mut h = Blake2b512::new();
    h.update(b"SS58PRE");
    h.update(&data[0..33]);
    let out = h.finalize();
    let cs = &out[0..2];
    if cs != checksum {
        return Err(Error::InvalidState("invalid SS58 checksum".into()));
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(pubkey);
    Ok(pk)
}

/// Verify sr25519 signature over the SHA-256 digest of the module bytes.
pub fn verify_signature_sr25519(
    module_bytes: &[u8],
    digest_opt: &Option<String>,
    owner: &str,
    sig_b64_or_hex: &str,
) -> Result<(), Error> {
    // Compute digest or use provided to match signing surface
    let digest_bytes = if let Some(d) = digest_opt {
        // Normalize expected digest to raw bytes
        let s = d.trim();
        let body = s.strip_prefix("sha256:").unwrap_or(s);
        if body.chars().all(|c| c.is_ascii_hexdigit()) && body.len() % 2 == 0 {
            (0..body.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&body[i..i + 2], 16).unwrap_or(0))
                .collect::<Vec<u8>>()
        } else {
            general_purpose::STANDARD
                .decode(body)
                .map_err(|e| Error::Serialization(e.to_string()))?
        }
    } else {
        let mut h = Sha256::new();
        h.update(module_bytes);
        h.finalize().to_vec()
    };

    let pk_raw = decode_pubkey_from_owner(owner)?;
    let pk =
        Sr25519PublicKey::from_bytes(&pk_raw).map_err(|e| Error::Serialization(e.to_string()))?;
    // Decode signature (hex or base64)
    let sig_bytes = if sig_b64_or_hex.trim().chars().all(|c| c.is_ascii_hexdigit())
        && sig_b64_or_hex.len() == 128
    {
        (0..sig_b64_or_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&sig_b64_or_hex[i..i + 2], 16).unwrap_or(0))
            .collect::<Vec<u8>>()
    } else {
        general_purpose::STANDARD
            .decode(sig_b64_or_hex.trim())
            .map_err(|e| Error::Serialization(e.to_string()))?
    };
    let sig = Signature::from_bytes(&sig_bytes).map_err(|e| Error::Serialization(e.to_string()))?;
    pk.verify_simple(b"module_digest", &digest_bytes, &sig)
        .map_err(|_| Error::InvalidState("invalid sr25519 signature".into()))
}
