use clap::Parser;
use mcp_registrar::utils::chain::decode_pubkey_from_owner;
use subxt::{OnlineClient, config::PolkadotConfig};
use subxt::dynamic::{tx, Value};
use subxt_signer::{sr25519::Keypair, SecretUri};
use std::str::FromStr;
use mcp_registrar::config::env;

#[derive(Parser, Debug)]
#[command(name = "register-module", about = "Register module metadata CID on-chain")]
struct Args {
    /// Module id (SS58 address or 64-hex public key)
    #[arg(long)]
    module_id: String,

    /// Metadata CID (string stored on-chain)
    #[arg(long)]
    metadata_cid: String,

    /// Signer SURI (e.g., //Alice or mnemonic). Defaults to //Alice for dev.
    #[arg(long, default_value = "//Alice")]
    suri: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let url = env::chain_rpc_url();
    let api = OnlineClient::<PolkadotConfig>::from_url(&url).await?;

    // Build signer
    let kp = Keypair::from_uri(&SecretUri::from_str(&args.suri).map_err(|e| format!("suri: {}", e))?)
        .map_err(|e| format!("suri: {}", e))?;

    // Prepare call: Modules::register_module(key: Vec<u8>, cid: Vec<u8>)
    let key = decode_pubkey_from_owner(&args.module_id).expect("decode module_id").to_vec();
    let cid = args.metadata_cid.into_bytes();
    let call = tx("Modules", "register_module", vec![Value::from_bytes(key), Value::from_bytes(cid)]);

    // Submit and watch
    let mut progress = api.tx().sign_and_submit_then_watch_default(&call, &kp).await?;
    while let Some(status) = progress.next().await {
        let status = status?;
        if let Some(in_block) = status.as_in_block() {
            eprintln!("Included in block: {:?}", in_block.block_hash());
        }
        if let Some(finalized) = status.as_finalized() {
            eprintln!("Finalized in block: {:?}", finalized.block_hash());
            break;
        }
    }
    Ok(())
}
