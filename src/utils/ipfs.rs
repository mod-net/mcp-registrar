use crate::error::Error;
use crate::config::env;

/// Return gateway base from env or default.
fn gateway_base() -> String { env::ipfs_gateway_url().unwrap_or_else(|| "http://127.0.0.1:8080/ipfs/".to_string()) }

/// Given an `ipfs://<cid[/path]>` URI, return `<cid[/path]>`.
fn strip_ipfs_scheme(uri: &str) -> Option<String> {
    uri.strip_prefix("ipfs://").map(|s| s.to_string())
}

fn ipfs_base_url() -> String { env::ipfs_api_url().unwrap_or_else(|| "http://127.0.0.1:8000/api".to_string()) }

fn ipfs_provider() -> String { std::env::var("IPFS_PROVIDER").unwrap_or_else(|_| "gateway".to_string()) }

/// Fetch bytes from an ipfs:// URI via configured provider.
pub async fn fetch_ipfs_bytes(uri: &str) -> Result<Vec<u8>, Error> {
    let tail = strip_ipfs_scheme(uri).ok_or_else(|| Error::InvalidState("invalid ipfs uri".into()))?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| Error::Serialization(e.to_string()))?;

    match ipfs_provider().as_str() {
        // Use provider name `api` (or legacy `modnet`) to indicate custom API server.
        "api" | "modnet" => {
            // Expect /files/{cid}[/{path}] endpoint; use only first path segment as cid
            let mut split = tail.splitn(2, '/');
            let cid = split.next().unwrap_or("");
            let path_tail = split.next();
            let url = format!("{}/files/{}", ipfs_base_url(), cid);
            // For modnet provider, inner paths are not supported yet.
            // Ignore any trailing path and fetch the root CID only.
            // Future: fetch CAR and traverse to path if needed.
            if let Some(p) = path_tail {
                tracing::debug!("modnet ipfs: ignoring inner path segment '{}'", p);
            }
            let resp = client.get(&url).send().await.map_err(|e| Error::Serialization(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(Error::InvalidState(format!("modnet ipfs {} -> {}", url, resp.status())));
            }
            let bytes = resp.bytes().await.map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(bytes.to_vec())
        }
        // Kubo RPC: POST /api/v0/cat?arg=<cid[/path]>
        "kubo" => {
            let url = format!("{}/api/v0/cat?arg={}", ipfs_base_url(), tail);
            let resp = client.post(&url).send().await.map_err(|e| Error::Serialization(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(Error::InvalidState(format!("kubo cat {} -> {}", url, resp.status())));
            }
            let bytes = resp.bytes().await.map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(bytes.to_vec())
        }
        _ => {
            let url = format!("{}{}", gateway_base(), tail);
            let resp = client.get(&url).send().await.map_err(|e| Error::Serialization(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(Error::InvalidState(format!("ipfs gateway {} -> {}", url, resp.status())));
            }
            let bytes = resp.bytes().await.map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(bytes.to_vec())
        }
    }
}
