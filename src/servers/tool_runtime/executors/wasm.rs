use crate::error::Error;
use crate::servers::tool_runtime::{Executor, Policy, ToolRuntime};
use crate::utils::{ipfs, chain, module_cache};
use std::path::Path;
use wasmtime::{Config, Engine, Linker, Module, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, add_to_linker};

#[derive(Debug)]
pub struct WasmExecutor;

#[async_trait::async_trait]
impl Executor for WasmExecutor {
    async fn invoke(
        &self,
        tool_id: &str,
        runtime: &ToolRuntime,
        args_json: &serde_json::Value,
        policy: &Policy,
    ) -> Result<serde_json::Value, Error> {
        // Load runtime configuration
        let cfg = match runtime {
            ToolRuntime::Wasm(cfg) => cfg,
            _ => return Err(Error::InvalidState("WasmExecutor received non-wasm runtime".into())),
        };
        let module_path = &cfg.module_path;
        if !Path::new(module_path).exists() {
            return Err(Error::InvalidState(format!(
                "wasm module not found at {:?}",
                module_path
            )));
        }

        // Prepare wasmtime engine with fuel metering
        let mut config = Config::new();
        config.consume_fuel(true);
        // Note: memory limits are planned; fuel limit enforced below.
        let engine = Engine::new(&config).map_err(|e| Error::Serialization(e.to_string()))?;

        // Prepare module bytes (supports chain://, ipfs://, or local file)
        let module_bytes: Vec<u8> = {
            let path_str = module_path.to_string_lossy();
            if path_str.starts_with("chain://") {
                let mp = chain::resolve_chain_uri(&path_str).await?;
                // Try cache by digest if available
                if let Some(d) = &mp.digest {
                    if let Some(bytes) = module_cache::read(&format!("sha256-{}", d)) { bytes } else {
                        let fetched = if mp.uri.starts_with("ipfs://") {
                            ipfs::fetch_ipfs_bytes(&mp.uri).await?
                        } else if mp.uri.starts_with("http://") || mp.uri.starts_with("https://") {
                            reqwest::get(&mp.uri).await.map_err(|e| Error::Serialization(e.to_string()))?
                                .bytes().await.map_err(|e| Error::Serialization(e.to_string()))?.to_vec()
                        } else {
                            tokio::fs::read(&mp.uri).await.map_err(|e| Error::Serialization(e.to_string()))?
                        };
                        // Verify digest if provided
                        chain::verify_digest(&fetched, d)?;
                        // Optional signature verify if present
                        if let Some(sig) = &mp.signature { chain::verify_signature_sr25519(&fetched, &mp.digest, &mp.owner, sig)?; }
                        module_cache::write(&format!("sha256-{}", d), &fetched);
                        fetched
                    }
                } else if mp.uri.starts_with("ipfs://") {
                    // Cache by CID when no digest is available
                    let cid_key = format!("cid-{}", mp.uri.trim_start_matches("ipfs://").split('/').next().unwrap_or(""));
                    if let Some(bytes) = module_cache::read(&cid_key) { bytes } else {
                        let fetched = ipfs::fetch_ipfs_bytes(&mp.uri).await?;
                        module_cache::write(&cid_key, &fetched);
                        fetched
                    }
                } else if mp.uri.starts_with("http://") || mp.uri.starts_with("https://") {
                    reqwest::get(&mp.uri).await.map_err(|e| Error::Serialization(e.to_string()))?
                        .bytes().await.map_err(|e| Error::Serialization(e.to_string()))?.to_vec()
                } else {
                    tokio::fs::read(&mp.uri).await.map_err(|e| Error::Serialization(e.to_string()))?
                }
            } else if path_str.starts_with("ipfs://") {
                let cid_key = format!("cid-{}", path_str.trim_start_matches("ipfs://").split('/').next().unwrap_or(""));
                if let Some(bytes) = module_cache::read(&cid_key) { bytes } else {
                    let fetched = ipfs::fetch_ipfs_bytes(&path_str).await?;
                    module_cache::write(&cid_key, &fetched);
                    fetched
                }
            } else {
                tokio::fs::read(&*module_path)
                    .await
                    .map_err(|e| Error::Serialization(e.to_string()))?
            }
        };

        // Serialize arguments once
        let args_str = serde_json::to_string(args_json)?;
        let export_name = cfg.export.clone();

        // Execute synchronously inside spawn_blocking, but enforce async timeout
        let module_path = module_path.clone();
        let max_bytes = policy.max_output_bytes;
        let timeout = std::time::Duration::from_millis(policy.timeout_ms);
        let fuel_budget: u64 = std::cmp::max(1_000_000, policy.cpu_time_ms.saturating_mul(10_000)) as u64;

        let tool_id_s = tool_id.to_string();
        let fut = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, Error> {
            let started = std::time::Instant::now();
            // Load module
            let module = Module::new(&engine, &module_bytes)
                .map_err(|e| Error::Serialization(format!("wasm load error: {}", e)))?;

            // Build WASI context (no preopens, no env by default)
            let wasi = WasiCtxBuilder::new().build();
            let mut store = Store::new(&engine, wasi);
            // Add fuel (v16 API uses set_fuel)
            store.set_fuel(fuel_budget).map_err(|e| Error::Serialization(e.to_string()))?;

            // Linker with WASI (safe even if module does not import WASI)
            let mut linker: Linker<WasiCtx> = Linker::new(&engine);
            add_to_linker(&mut linker, |cx| cx)
                .map_err(|e| Error::Serialization(e.to_string()))?;

            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|e| Error::Serialization(e.to_string()))?;

            // Expect pointer/length string ABI with optional alloc/free helpers
            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| Error::InvalidState("wasm module missing exported memory".into()))?;

            let alloc = instance
                .get_typed_func::<i32, i32>(&mut store, "alloc")
                .map_err(|_| Error::InvalidState("wasm module missing required alloc(i32)->i32".into()))?;

            // Allocate and write input bytes
            let input_bytes = args_str.as_bytes();
            if input_bytes.len() > i32::MAX as usize {
                return Err(Error::InvalidState("arguments too large for wasm input".into()));
            }
            let in_len = input_bytes.len() as i32;
            let in_ptr = alloc
                .call(&mut store, in_len)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            memory
                .write(&mut store, in_ptr as usize, input_bytes)
                .map_err(|e| Error::Serialization(e.to_string()))?;

            // Locate exported call function
            let call = instance
                .get_typed_func::<(i32, i32), (i32, i32)>(&mut store, &export_name)
                .map_err(|_| Error::InvalidState(format!(
                    "wasm export '{}' with (i32,i32)->(i32,i32) not found",
                    export_name
                )))?;

            // Invoke
            let (out_ptr, out_len) = call
                .call(&mut store, (in_ptr, in_len))
                .map_err(|e| Error::Serialization(e.to_string()))?;

            // Read output
            if out_len < 0 {
                return Err(Error::InvalidState("negative length from wasm".into()));
            }
            let out_len_usize = out_len as usize;
            if out_len_usize > max_bytes {
                return Err(Error::InvalidState("tool output too large".into()));
            }
            let mut out = vec![0u8; out_len_usize];
            memory
                .read(&mut store, out_ptr as usize, &mut out)
                .map_err(|e| Error::Serialization(e.to_string()))?;

            // Best-effort free (optional)
            if let Ok(free) = instance.get_typed_func::<(i32, i32), ()>(&mut store, "free") {
                let _ = free.call(&mut store, (in_ptr, in_len));
                let _ = free.call(&mut store, (out_ptr, out_len));
            }

            // Decode and parse JSON (robust to trailing bytes)
            let s = String::from_utf8(out).map_err(|e| Error::Serialization(e.to_string()))?;
            let s_trim = if let Some(idx) = s.rfind('}') { &s[..=idx] } else { s.as_str() };
            let v: serde_json::Value = serde_json::from_str(s_trim)?;
            let duration_ms = started.elapsed().as_millis();
            let bytes = s_trim.len();
            tracing::info!("wasm tool {} completed in {} ms ({} bytes)", tool_id_s, duration_ms, bytes);
            crate::monitoring::TOOL_METRICS.record(duration_ms as u64, bytes as u64, false);
            Ok(v)
        });

        match tokio::time::timeout(timeout, fut).await {
            Ok(join) => join.map_err(|e| Error::Other(Box::new(e)))?,
            Err(_) => Err(Error::InvalidState(format!("wasm tool {} timed out", tool_id)))
        }
    }
}
