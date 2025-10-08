use std::path::{Path, PathBuf};

// Canonical environment accessors
// Keys
pub fn keys_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MODSDK_KEYS_DIR") { return PathBuf::from(dir); }
    if let Ok(dir) = std::env::var("MODNET_KEYS_DIR") { return PathBuf::from(dir); }
    if let Ok(home) = std::env::var("HOME") { return PathBuf::from(home).join(".modnet/keys"); }
    dirs::home_dir().unwrap_or(PathBuf::from("~")).join(".modnet/keys")
}

// Module API (client URL and server bind address)
pub fn module_api_url() -> Option<String> {
    if let Ok(v) = std::env::var("MODSDK_MODULE_API_URL") { return Some(v); }
    std::env::var("MODULE_API_URL").ok()
}

pub fn module_api_addr() -> String {
    std::env::var("MODSDK_MODULE_API_ADDR")
        .or_else(|_| std::env::var("MODULE_API_ADDR"))
        .unwrap_or_else(|_| "127.0.0.1:8090".into())
}

pub fn module_api_max_upload_bytes() -> usize {
    let mb: usize = std::env::var("MODSDK_MODULE_API_MAX_UPLOAD_MB")
        .or_else(|_| std::env::var("MODULE_API_MAX_UPLOAD_MB"))
        .ok()
        .and_then(|v| v.parse().ok()).unwrap_or(64);
    mb.saturating_mul(1024 * 1024)
}

// Chain
pub fn chain_rpc_url() -> String {
    std::env::var("MODSDK_CHAIN_RPC_URL")
        .or_else(|_| std::env::var("CHAIN_RPC_URL"))
        .unwrap_or_else(|_| "ws://127.0.0.1:9944".into())
}

// IPFS
pub fn ipfs_api_url() -> Option<String> {
    if let Ok(v) = std::env::var("MODSDK_IPFS_API_URL") { return Some(v); }
    if let Ok(v) = std::env::var("IPFS_API_URL") { return Some(v); }
    // Backward compat
    if let Ok(v) = std::env::var("IPFS_BASE_URL") {
        tracing::warn!("IPFS_BASE_URL is deprecated; use IPFS_API_URL");
        return Some(v);
    }
    None
}

pub fn ipfs_api_key() -> Option<String> { std::env::var("IPFS_API_KEY").ok() }

pub fn ipfs_gateway_url() -> Option<String> {
    if let Ok(v) = std::env::var("MODSDK_IPFS_GATEWAY_URL") { return Some(v); }
    if let Ok(v) = std::env::var("IPFS_GATEWAY_URL") { return Some(v); }
    if let Ok(v) = std::env::var("IPFS_GATEWAY") {
        tracing::warn!("IPFS_GATEWAY is deprecated; use IPFS_GATEWAY_URL");
        return Some(v);
    }
    None
}

// Registrar/cache
pub fn registry_cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("REGISTRY_CACHE_DIR") { return PathBuf::from(p); }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".cache").join("registry-scheduler")
}
