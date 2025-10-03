use clap::Parser;
use registry_scheduler::utils::{chain, ipfs, metadata};
use subxt::{config::PolkadotConfig, OnlineClient};
use subxt::dynamic::{storage, Value};
use registry_scheduler::config::env;

#[derive(Parser, Debug)]
#[command(name = "query-module", about = "Retrieve a module mapping and metadata by SS58 or 0x pubkey hex")] 
struct Args {
    /// Module id: SS58 address (e.g., 5G...) or 0x<64-hex> public key
    #[arg(long)]
    module_id: String,

    /// Output raw CID only
    #[arg(long, default_value_t = false)]
    raw: bool,

    /// Skip signature verification when printing pointer
    #[arg(long, default_value_t = false)]
    no_verify: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let rpc = env::chain_rpc_url();
    let api = OnlineClient::<PolkadotConfig>::from_url(&rpc).await?;

    // Decode module id to raw 32-byte pubkey
    let key = chain::decode_pubkey_from_owner(&args.module_id)?;

    // Fetch storage: Modules::Modules(key)
    let addr = storage("Modules", "Modules", vec![Value::from_bytes(key.to_vec())]);
    let cid_thunk_opt = api
        .storage()
        .at_latest()
        .await?
        .fetch(&addr)
        .await?;

    let cid = if let Some(thunk) = cid_thunk_opt {
        let bytes: Vec<u8> = thunk.as_type::<Vec<u8>>()?;
        match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("CID is not valid UTF-8");
                std::process::exit(2);
            }
        }
    } else {
        eprintln!("No mapping found for module id");
        std::process::exit(1);
    };
    if args.raw {
        println!("{}", cid);
        return Ok(());
    }

    // Fetch metadata JSON and print
    let meta_uri = format!("ipfs://{}", cid);
    let meta_bytes = ipfs::fetch_ipfs_bytes(&meta_uri).await?;
    let md = metadata::parse_metadata_v1(&meta_bytes)?;

    if args.no_verify {
        println!("{}", String::from_utf8_lossy(&meta_bytes));
        return Ok(());
    }

    // Optionally verify pointer and print normalized pointer JSON
    let art_bytes = if md.artifact_uri.starts_with("ipfs://") {
        ipfs::fetch_ipfs_bytes(&md.artifact_uri).await?
    } else if md.artifact_uri.starts_with("http://") || md.artifact_uri.starts_with("https://") {
        let resp = reqwest::get(&md.artifact_uri).await?;
        if !resp.status().is_success() { Err(format!("artifact {} -> {}", md.artifact_uri, resp.status()))? };
        resp.bytes().await?.to_vec()
    } else {
        eprintln!("unsupported artifact_uri: {}", md.artifact_uri);
        std::process::exit(2);
    };

    chain::verify_digest(&art_bytes, &md.digest)?;
    chain::verify_signature_sr25519(&art_bytes, &Some(md.digest.clone()), &args.module_id, &md.signature)?;

    let pointer = chain::ModulePointer {
        module_id: md.module_id,
        uri: md.artifact_uri,
        owner: args.module_id,
        digest: Some(md.digest),
        signature: Some(md.signature),
        version: md.version,
    };
    println!("{}", serde_json::to_string_pretty(&pointer)?);
    Ok(())
}
