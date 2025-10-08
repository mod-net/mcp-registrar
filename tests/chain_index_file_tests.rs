use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test_chain_index_file_resolution_all() {
    // Prepare a simple map-shaped index file
    let mut tmp = NamedTempFile::new().expect("tmp");
    let json = serde_json::json!({
        "echo-wasm": {
            "module_id": "echo-wasm",
            "uri": "ipfs://bafybeigdyrzt",
            "owner": "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty",
            "digest": null,
            "signature": null,
            "version": "1.0.0"
        }
    });
    write!(tmp, "{}", json.to_string()).unwrap();
    tmp.as_file_mut().flush().unwrap();
    let path = tmp.into_temp_path();
    std::env::set_var("CHAIN_INDEX_FILE", path.to_str().unwrap());
    // Ensure CHAIN_INDEX_URL does not interfere
    std::env::remove_var("CHAIN_INDEX_URL");

    let mp = mcp_registrar::utils::chain::resolve_chain_uri("chain://echo-wasm")
        .await
        .expect("resolve via file");
    assert_eq!(mp.module_id, "echo-wasm");
    assert_eq!(mp.uri, "ipfs://bafybeigdyrzt");
    // cleanup
    let _ = path.close();

    // Wrapped map
    let mut tmp = NamedTempFile::new().expect("tmp");
    let json = serde_json::json!({
        "modules": {
            "tool-a": {
                "uri": "file:///tmp/a.wasm",
                "owner": "0x".to_string() + &"11".repeat(32),
                "version": "0.1.0"
            }
        }
    });
    write!(tmp, "{}", json.to_string()).unwrap();
    tmp.as_file_mut().flush().unwrap();
    let path = tmp.into_temp_path();
    std::env::set_var("CHAIN_INDEX_FILE", path.to_str().unwrap());
    std::env::remove_var("CHAIN_INDEX_URL");

    let mp = mcp_registrar::utils::chain::resolve_chain_uri("chain://tool-a")
        .await
        .expect("resolve via file wrapped");
    assert_eq!(mp.module_id, "tool-a");
    assert_eq!(mp.uri, "file:///tmp/a.wasm");
    assert_eq!(mp.version.as_deref(), Some("0.1.0"));
    let _ = path.close();

    // Array
    let mut tmp = NamedTempFile::new().expect("tmp");
    let json = serde_json::json!([
        {"module_id": "m1", "uri": "ipfs://cid1", "owner": "0x".to_string() + &"22".repeat(32)},
        {"module_id": "m2", "uri": "ipfs://cid2", "owner": "0x".to_string() + &"33".repeat(32)}
    ]);
    write!(tmp, "{}", json.to_string()).unwrap();
    tmp.as_file_mut().flush().unwrap();
    let path = tmp.into_temp_path();
    std::env::set_var("CHAIN_INDEX_FILE", path.to_str().unwrap());
    std::env::remove_var("CHAIN_INDEX_URL");

    let mp = mcp_registrar::utils::chain::resolve_chain_uri("chain://m2")
        .await
        .expect("resolve via file array");
    assert_eq!(mp.module_id, "m2");
    assert_eq!(mp.uri, "ipfs://cid2");
    let _ = path.close();
}
