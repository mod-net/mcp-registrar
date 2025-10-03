use crate::error::Error;
use crate::utils::{ipfs, chain, metadata};
use crate::utils::chain::ModulePointer;
use crate::config::env;

/// Resolve a `chain://<SS58>` module id using Substrate RPC, fetch signed metadata from IPFS,
/// verify digest + signature with the SS58 key, and return a verified ModulePointer to the artifact.
pub async fn resolve_via_rpc(module_uri: &str) -> Result<ModulePointer, Error> {
    let id = module_uri.strip_prefix("chain://").ok_or_else(|| Error::InvalidState("invalid chain uri".into()))?;
    let url = env::chain_rpc_url();

    // Connect
    let api = subxt::OnlineClient::<subxt::config::PolkadotConfig>::from_url(&url)
        .await
        .map_err(|e| Error::Serialization(format!("rpc connect: {}", e)))?;

    // Decode SS58 -> raw pubkey bytes
    let key_bytes = chain::decode_pubkey_from_owner(id)?.to_vec();

    // Storage address: Modules::Modules(key)
    use subxt::dynamic::{storage, Value};
    let addr = storage("Modules", "Modules", vec![Value::from_bytes(key_bytes)]);
    let cid_thunk_opt = api
        .storage()
        .at_latest()
        .await
        .map_err(|e| Error::Serialization(format!("rpc at_latest: {}", e)))?
        .fetch(&addr)
        .await
        .map_err(|e| Error::Serialization(format!("rpc fetch: {}", e)))?;

    let cid_str = if let Some(thunk) = cid_thunk_opt {
        let val = thunk.to_value().map_err(|e| Error::Serialization(format!("to_value: {}", e)))?;
        match val {
            subxt::dynamic::Value::Bytes(bytes) => String::from_utf8(bytes.to_vec()).map_err(|_| Error::Serialization("cid utf8".into()))?,
            other => return Err(Error::Serialization(format!("unexpected storage value: {:?}", other))),
        }
    } else { return Err(Error::NotFound); };
    // Treat on-chain CID as metadata JSON CID (v1)
    let metadata_uri = format!("ipfs://{}", cid_str);
    let meta_bytes = ipfs::fetch_ipfs_bytes(&metadata_uri).await?;
    let md = metadata::parse_metadata_v1(&meta_bytes)?;

    // Enforce owner binding to SS58 id
    if md.module_id != id { return Err(Error::InvalidState("metadata.owner mismatch".into())); }
    if md.signature_scheme() != "sr25519" { return Err(Error::InvalidState("unsupported signature_scheme".into())); }

    // Fetch artifact and verify digest + signature
    let artifact_uri = &md.artifact_uri;
    let art_bytes = if artifact_uri.starts_with("ipfs://") {
        ipfs::fetch_ipfs_bytes(artifact_uri).await?
    } else if artifact_uri.starts_with("http://") || artifact_uri.starts_with("https://") {
        // Soft support: HTTP fetch (not recommended); reuse reqwest
        let resp = reqwest::get(artifact_uri).await.map_err(|e| Error::Serialization(e.to_string()))?;
        if !resp.status().is_success() { return Err(Error::InvalidState(format!("artifact {} -> {}", artifact_uri, resp.status()))); }
        resp.bytes().await.map(|b| b.to_vec()).map_err(|e| Error::Serialization(e.to_string()))?
    } else {
        return Err(Error::InvalidState("unsupported artifact_uri".into()));
    };

    chain::verify_digest(&art_bytes, &md.digest)?;
    chain::verify_signature_sr25519(&art_bytes, &Some(md.digest.clone()), id, &md.signature)?;

    Ok(ModulePointer {
        module_id: id.to_string(),
        uri: artifact_uri.to_string(),
        owner: id.to_string(),
        digest: Some(md.digest),
        signature: Some(md.signature),
        version: md.version,
    })
}
