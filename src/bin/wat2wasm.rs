use std::path::PathBuf;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "wat2wasm", about = "Convert a .wat text module to a .wasm binary")]
struct Args {
    /// Input .wat file
    in_wat: PathBuf,
    /// Output .wasm file
    out_wasm: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let wasm = wat::parse_file(&args.in_wat)?;
    std::fs::write(&args.out_wasm, wasm)?;
    eprintln!("Wrote {}", args.out_wasm.display());
    Ok(())
}

