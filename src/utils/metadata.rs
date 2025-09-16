use serde::{Deserialize, Serialize};
use crate::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMetadataV1 {
    pub module_id: String,               // SS58 of signer/owner
    pub artifact_uri: String,            // ipfs://<cid> (module bytes)
    pub digest: String,                  // "sha256:<hex>" or hex/base64
    pub signature: String,               // sr25519 signature (hex or base64)
    #[serde(default)]
    pub signature_scheme: Option<String>, // default: sr25519
    #[serde(default)]
    pub version: Option<String>,
}

impl ModuleMetadataV1 {
    pub fn signature_scheme(&self) -> &str { self.signature_scheme.as_deref().unwrap_or("sr25519") }
}

/// Parse JSON bytes into ModuleMetadataV1.
pub fn parse_metadata_v1(bytes: &[u8]) -> Result<ModuleMetadataV1, Error> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    // Allow either direct shape or wrapped { "module": { ... } }
    let obj = if let Some(m) = v.get("module").cloned() { m } else { v };
    let md: ModuleMetadataV1 = serde_json::from_value(obj)
        .map_err(|e| Error::Serialization(format!("metadata parse error: {}", e)))?;
    Ok(md)
}

