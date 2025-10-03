use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit};
use base64::{engine::general_purpose, Engine as _};
use blake2::{Blake2b512, Digest as _};
use clap::{Parser, Subcommand, Args, ValueEnum};
use rand::RngCore;
use scrypt::Params;
use serde::{Deserialize, Serialize};
use registry_scheduler::config::env;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Scheme { Sr25519, Ed25519 }

fn print_json_compact(v: &serde_json::Value) -> anyhow::Result<()> {
  println!("");
  println!("{}", serde_json::to_string(v)?);
  Ok(())
}

// Render KeyJson for output, formatting byte_array as ["0xNN", ...]
fn render_key_json(kj: &KeyJson) -> serde_json::Value {
  let mut v = serde_json::to_value(kj).unwrap_or(serde_json::json!({}));
  if let Some(ba_hex) = kj.byte_array.as_ref() {
    // strip 0x and convert to bytes
    let s = if ba_hex.starts_with("0x") || ba_hex.starts_with("0X") { &ba_hex[2..] } else { ba_hex.as_str() };
    if s.len() % 2 == 0 {
      let bytes: Vec<serde_json::Value> = (0..s.len()).step_by(2)
        .filter_map(|i| u8::from_str_radix(&s[i..i+2], 16).ok())
        .map(|b| serde_json::Value::from(b))
        .collect();
      v["byte_array"] = serde_json::Value::Array(bytes);
    }
  }
  v
}
impl Scheme { fn as_str(&self)->&'static str { match self { Scheme::Sr25519=>"sr25519", Scheme::Ed25519=>"ed25519" } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyJson {
  scheme: String,
  #[serde(default = "default_network")] network: String,
  #[serde(skip_serializing_if = "Option::is_none")] byte_array: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] mnemonic_phrase: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] secret_phrase: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] public_key_hex: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] private_key_hex: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] ss58_address: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] key_type: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] is_pair: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")] is_multisig: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")] threshold: Option<u16>,
  #[serde(skip_serializing_if = "Option::is_none")] signers: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")] multisig_address: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] created_at: Option<String>,
}
fn default_network()->String { "substrate".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncBlobV1 {
  version: u8,
  kdf: String,
  salt: String,
  params: EncParams,
  nonce: String,
  ciphertext: String,
  // Optional public metadata for safe reads without decrypting (backward compatible)
  #[serde(skip_serializing_if = "Option::is_none")] scheme: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] network: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] byte_array: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] public_key_hex: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")] ss58_address: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncParams { n: u32, r: u32, p: u32 }

#[derive(Parser, Debug)]
#[command(name="keytools", about="Key tools for Modnet (Rust)")]
struct Cli {
  #[command(subcommand)]
  cmd: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
  Gen(GenArgs),
  #[command(name="gen-all")] GenAll(GenAllArgs),
  Multisig(MultisigArgs),
  #[command(name="key-save")] KeySave(KeySaveArgs),
  #[command(name="key-load")] KeyLoad(KeyLoadArgs),
  #[command(name="save")] Save(KeySaveArgs),
  #[command(name="load")] Load(KeyLoadArgs),
  List,
  Select(SelectArgs),
  Get(GetArgs),
}

#[derive(Args, Debug)]
struct GenArgs {
  #[arg(long, value_enum, default_value_t=Scheme::Sr25519)] scheme: Scheme,
  #[arg(long, default_value="substrate")] network: String,
  #[arg(long)] out: Option<String>,
  #[arg(long)] name: Option<String>,
  /// Positional base filename (sans .json) as a convenience
  #[arg()] input: Option<String>,
}
#[derive(Args, Debug)]
struct GenAllArgs { #[arg(long, default_value="substrate")] network: String, #[arg(long)] out_dir: Option<String>, #[arg(long)] aura_name: Option<String>, #[arg(long)] grandpa_name: Option<String> }
#[derive(Args, Debug)]
struct MultisigArgs { #[arg(long)] threshold: u16, #[arg(long, default_value_t=42)] ss58_prefix: u8, #[arg(long="signer")] signer: Vec<String> }
#[derive(Args, Debug)]
struct KeySaveArgs {
  #[arg(long, value_enum, default_value_t=Scheme::Sr25519)] scheme: Scheme,
  #[arg(long, default_value="substrate")] network: String,
  #[arg(long)] phrase: Option<String>,
  #[arg(long)] public: Option<String>,
  #[arg(long)] out: Option<String>,
  #[arg(long)] name: Option<String>,
  /// Positional base filename (sans .json) as a convenience
  #[arg()] input: Option<String>,
}
#[derive(Args, Debug)]
struct KeyLoadArgs {
  /// Load from an explicit file path
  #[arg(long)] file: Option<String>,
  /// Load from ~/.modnet/keys/<name>.json (name can be provided sans .json)
  #[arg(long)] name: Option<String>,
  /// Positional input treated as name (sans .json) for convenience
  #[arg()] input: Option<String>,
  #[arg(long)] password: Option<String>,
}
#[derive(Args, Debug)]
struct SelectArgs { #[arg(long)] index: Option<usize>, #[arg(long)] password: Option<String>, #[arg(long)] show: bool }
#[derive(Args, Debug)]
struct GetArgs {
  /// Provide a 0x-prefixed public key hex explicitly
  #[arg(long)] public_key: Option<String>,
  /// Provide an SS58 address explicitly
  #[arg(long)] ss58_address: Option<String>,
  /// Positional input defaults to SS58 address or 0x-hex public key
  #[arg()] input: Option<String>,
  /// Provide a key filename base (sans .json) to read public info from wrapper without decrypting
  #[arg(long)] name: Option<String>,
  /// Optional single field to print; otherwise prints JSON
  #[arg(long)] field: Option<String>,
  #[arg(value_enum, default_value_t=Scheme::Sr25519)] scheme: Scheme,
  #[arg(long, default_value="substrate")] network: String,
}

fn main() -> anyhow::Result<()> {
  let cli = Cli::parse();
  match cli.cmd {
    Commands::Gen(a)=>cmd_gen(a)?,
    Commands::GenAll(a)=>cmd_gen_all(a)?,
    Commands::Multisig(a)=>cmd_multisig(a)?,
    Commands::KeySave(a)|Commands::Save(a)=>cmd_key_save(a)?,
    Commands::KeyLoad(a)|Commands::Load(a)=>cmd_key_load(a)?,
    Commands::List=>cmd_list()?,
    Commands::Select(a)=>cmd_select(a)?,
    Commands::Get(a)=>cmd_get(a)?,
  }
  Ok(())
}

fn keys_dir() -> PathBuf { env::keys_dir() }
fn ensure_keys_dir() { let _=fs::create_dir_all(keys_dir()); }

fn require_subkey() { if which::which("subkey").is_err() { eprintln!("Error: 'subkey' not found on PATH"); std::process::exit(1);} }

fn run(cmd: &[&str]) -> anyhow::Result<String> {
  let out = Command::new(cmd[0]).args(&cmd[1..]).output()?;
  if !out.status.success() { anyhow::bail!("{} failed: {}", cmd.join(" "), String::from_utf8_lossy(&out.stderr)); }
  Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn parse_subkey(output:&str) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
  let mut phrase=None; let mut seed=None; let mut pubhex=None; let mut ss58=None;
  for line in output.lines().map(|l| l.trim()) {
    let l = line.to_lowercase();
    if l.starts_with("secret phrase") { phrase = line.split(':').nth(1).map(|s| s.trim().to_string()); }
    else if l.starts_with("secret seed") { seed = line.split(':').nth(1).map(|s| s.trim().to_string()); }
    else if l.starts_with("public key (hex)") { pubhex = line.split(':').nth(1).map(|s| s.trim().to_string()); }
    else if l.starts_with("ss58 address") { ss58 = line.split(':').nth(1).map(|s| s.trim().to_string()); }
  }
  (phrase, seed, pubhex, ss58)
}

fn cmd_gen(a: GenArgs) -> anyhow::Result<()> {
  require_subkey(); ensure_keys_dir();
  let out = run(&["subkey","generate","--scheme",a.scheme.as_str(),"--network",&a.network])?;
  let (phrase, seed, pubhex, ss58) = parse_subkey(&out);
  let kj = KeyJson{ scheme:a.scheme.as_str().into(), network:a.network, byte_array:ss58.as_ref().and_then(|s| ss58_to_bytes(s).ok()).map(|b| format!("0x{}", hex::encode(b))), mnemonic_phrase:None, secret_phrase:phrase, public_key_hex:pubhex, private_key_hex:seed, ss58_address:ss58.clone(), key_type:Some(a.scheme.as_str().into()), is_pair:Some(true), is_multisig:None, threshold:None, signers:None, multisig_address:None, created_at:Some(chrono::Utc::now().to_rfc3339()) };
  // If positional input is provided, use it as --name when --name is absent
  let effective_name = a.name.clone().or(a.input.clone());
  let out_path = resolve_out(a.out, effective_name, a.scheme.as_str());
  let enc = encrypt_key(&kj)?; fs::write(&out_path, serde_json::to_vec_pretty(&enc)?)?;
  // Add spacing between prompt and outputs
  println!("");
  println!("Saved generated key to {}", out_path.display());
  print_json_compact(&render_key_json(&kj))
}

fn cmd_gen_all(a: GenAllArgs) -> anyhow::Result<()> {
  // Compute explicit output paths that include role names, regardless of --out-dir
  let base_dir = a.out_dir.clone().unwrap_or_else(|| keys_dir().to_string_lossy().to_string());
  let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
  // Aura filename
  let aura_path = if let Some(name) = a.aura_name.as_ref() {
    let fname = if name.ends_with(".json") { name.clone() } else { format!("{}.json", name) };
    format!("{}/{}", base_dir, fname)
  } else {
    format!("{}/{}-aura-sr25519.json", base_dir, ts)
  };
  // GRANDPA filename
  let grandpa_path = if let Some(name) = a.grandpa_name.as_ref() {
    let fname = if name.ends_with(".json") { name.clone() } else { format!("{}.json", name) };
    format!("{}/{}", base_dir, fname)
  } else {
    format!("{}/{}-grandpa-ed25519.json", base_dir, ts)
  };

  let a1 = GenArgs{ scheme:Scheme::Sr25519, network:a.network.clone(), out: Some(aura_path), name: None, input: None };
  let a2 = GenArgs{ scheme:Scheme::Ed25519, network:a.network.clone(), out: Some(grandpa_path), name: None, input: None };
  cmd_gen(a1)?; cmd_gen(a2)?; Ok(())
}

// get: SAFE, does not decrypt files. Accepts SS58 or 0x public key and prints public info (optionally a field)
fn cmd_get(a: GetArgs) -> anyhow::Result<()> {
  // If --public-key provided, use subkey to derive SS58 (public-only)
  if let Some(public_hex) = a.public_key.as_ref() {
    require_subkey();
    let out = run(&["subkey","inspect","--network",&a.network,"--public","--scheme",a.scheme.as_str(), public_hex])?;
    let (_phrase, _seed, pubhex, ss58) = parse_subkey(&out);
    let kj = KeyJson { scheme:a.scheme.as_str().into(), network:a.network, byte_array:ss58.as_ref().and_then(|s| ss58_to_bytes(s).ok()).map(|b| format!("0x{}", hex::encode(b))), mnemonic_phrase:None, secret_phrase:None, public_key_hex:pubhex, private_key_hex:None, ss58_address:ss58, key_type:Some("ss58".into()), is_pair:Some(false), is_multisig:None, threshold:None, signers:None, multisig_address:None, created_at:Some(chrono::Utc::now().to_rfc3339()) };
    return output_value(render_key_json(&kj), a.field.as_deref());
  }
  // If --ss58-address provided explicitly, handle it
  if let Some(ss58) = a.ss58_address.as_ref() {
    let pk_bytes = ss58_to_bytes(ss58)?;
    let pubhex = format!("0x{}", hex::encode(pk_bytes));
    let kj = KeyJson { scheme:a.scheme.as_str().into(), network:a.network, byte_array:Some(format!("0x{}", hex::encode(pk_bytes))), mnemonic_phrase:None, secret_phrase:None, public_key_hex:Some(pubhex), private_key_hex:None, ss58_address:Some(ss58.clone()), key_type:Some("ss58".into()), is_pair:Some(false), is_multisig:None, threshold:None, signers:None, multisig_address:None, created_at:Some(chrono::Utc::now().to_rfc3339()) };
    return output_value(render_key_json(&kj), a.field.as_deref());
  }

  // Helper to emit public view from wrapper without decrypting
  fn wrapper_public_view(blob: &EncBlobV1, fallback_scheme: &str, fallback_network: &str) -> serde_json::Value {
    // Build byte_array as integers from blob.byte_array hex or from ss58
    let mut byte_arr_json: Option<serde_json::Value> = None;
    if let Some(ba_hex) = blob.byte_array.as_ref() {
      let s = if ba_hex.starts_with("0x") || ba_hex.starts_with("0X") { &ba_hex[2..] } else { ba_hex.as_str() };
      if s.len() % 2 == 0 {
        let arr: Vec<serde_json::Value> = (0..s.len()).step_by(2)
          .filter_map(|i| u8::from_str_radix(&s[i..i+2], 16).ok())
          .map(|b| serde_json::Value::from(b))
          .collect();
        byte_arr_json = Some(serde_json::Value::Array(arr));
      }
    } else if let Some(addr) = blob.ss58_address.as_ref() {
      if let Ok(bytes) = ss58_to_bytes(addr) {
        let arr: Vec<serde_json::Value> = bytes.iter().copied().map(|b| serde_json::Value::from(b)).collect();
        byte_arr_json = Some(serde_json::Value::Array(arr));
      }
    }
    serde_json::json!({
      "scheme": blob.scheme.as_deref().unwrap_or(fallback_scheme),
      "network": blob.network.as_deref().unwrap_or(fallback_network),
      "byte_array": byte_arr_json,
      "mnemonic_phrase": serde_json::Value::Null,
      "secret_phrase": serde_json::Value::Null,
      "public_key_hex": blob.public_key_hex.as_ref(),
      "private_key_hex": serde_json::Value::Null,
      "ss58_address": blob.ss58_address.as_ref(),
      "key_type": "ss58",
      "is_pair": false,
      "created_at": chrono::Utc::now().to_rfc3339(),
    })
  }

  // Support explicit --name without decrypting
  if let Some(nm) = a.name.as_ref() {
    let file = keys_dir().join(if nm.ends_with(".json") { nm.clone() } else { format!("{}.json", nm) });
    let blob: EncBlobV1 = serde_json::from_slice(&fs::read(&file)?)?;
    let v = wrapper_public_view(&blob, a.scheme.as_str(), &a.network);
    return output_value(v, a.field.as_deref());
  }

  // Finally, positional input: try SS58 first; if that fails, treat as name
  if let Some(inp) = a.input.as_ref() {
    if let Ok(pk_bytes) = ss58_to_bytes(inp) {
      let pubhex = format!("0x{}", hex::encode(pk_bytes));
      let kj = KeyJson { scheme:a.scheme.as_str().into(), network:a.network, byte_array:Some(format!("0x{}", hex::encode(pk_bytes))), mnemonic_phrase:None, secret_phrase:None, public_key_hex:Some(pubhex), private_key_hex:None, ss58_address:Some(inp.clone()), key_type:Some("ss58".into()), is_pair:Some(false), is_multisig:None, threshold:None, signers:None, multisig_address:None, created_at:Some(chrono::Utc::now().to_rfc3339()) };
      return output_value(render_key_json(&kj), a.field.as_deref());
    } else {
      let file = keys_dir().join(if inp.ends_with(".json") { inp.clone() } else { format!("{}.json", inp) });
      let blob: EncBlobV1 = serde_json::from_slice(&fs::read(&file)?)?;
      let v = wrapper_public_view(&blob, a.scheme.as_str(), &a.network);
      return output_value(v, a.field.as_deref());
    }
  }

  anyhow::bail!("Provide --public-key 0x<hex>, --ss58-address <addr>, or a name (positional/--name)")
}

fn cmd_multisig(a: MultisigArgs) -> anyhow::Result<()> {
  let mut signers_hex: Vec<Vec<u8>> = Vec::new();
  for s in &a.signer { let pk = ss58_to_bytes(s)?; signers_hex.push(pk.to_vec()); }
  signers_hex.sort();
  let mut hasher = Blake2b512::new(); hasher.update(b"modlpy/utilisig"); for pk in &signers_hex { hasher.update(pk); } hasher.update(a.threshold.to_le_bytes());
  let account_id: [u8;32] = hasher.finalize()[..32].try_into().unwrap();
  let address = ss58_encode(&account_id, a.ss58_prefix);
  let out = serde_json::json!({"threshold":a.threshold,"ss58_prefix":a.ss58_prefix,"account_id_hex":hex::encode(account_id),"ss58_address":address,"signers":a.signer});
  println!("");
  println!("{}", serde_json::to_string_pretty(&out)?); Ok(())
}

// helper to print either a single field or the whole JSON for safe get
fn output_value(v: serde_json::Value, field: Option<&str>) -> anyhow::Result<()> {
  if let Some(f) = field {
    match v.get(f) {
      Some(val) => { println!(""); if val.is_string() { println!("{}", val.as_str().unwrap()); } else { println!("{}", val); } },
      None => { anyhow::bail!("field not found") }
    }
    Ok(())
  } else {
    print_json_compact(&v)
  }
}

fn cmd_key_save(a: KeySaveArgs) -> anyhow::Result<()> {
  let kj = if let Some(ph) = a.phrase.as_ref() { from_phrase(ph, a.scheme.as_str(), &a.network)? } else if let Some(pu)=a.public.as_ref() { from_public(pu, a.scheme.as_str(), &a.network)? } else { eprint!("Enter secret phrase: "); io::stderr().flush().ok(); let p = read_line_hidden()?; if p.trim().is_empty(){ anyhow::bail!("Secret phrase cannot be empty"); } from_phrase(&p, a.scheme.as_str(), &a.network)? };
  // Allow positional input to act as --name if not provided
  let effective_name = a.name.clone().or(a.input.clone());
  let out_path = resolve_out(a.out.clone(), effective_name, a.scheme.as_str());
  let enc = encrypt_key(&kj)?; fs::write(&out_path, serde_json::to_vec_pretty(&enc)?)?;
  println!("");
  println!("Saved encrypted key to {}", out_path.display());
  println!("");
  Ok(())
}

fn cmd_key_load(a: KeyLoadArgs) -> anyhow::Result<()> {
  // Resolve input; if none, list keys and prompt selection
  let file_path = if let Some(f) = a.file.as_ref() {
    PathBuf::from(f)
  } else if let Some(nm) = a.name.as_ref().or(a.input.as_ref()) {
    let fname = if nm.ends_with(".json") { nm.clone() } else { format!("{}.json", nm) };
    keys_dir().join(fname)
  } else {
    // interactive select like cmd_select
    ensure_keys_dir();
    let mut rows: Vec<_> = fs::read_dir(keys_dir())?
      .filter_map(|e| e.ok())
      .filter(|e| e.path().is_file() && e.path().extension().map(|x| x == "json").unwrap_or(false))
      .map(|e| e.path())
      .collect();
    rows.sort();
    if rows.is_empty(){ println!("{{}}" ); return Ok(()); }
    for (i,p) in rows.iter().enumerate(){ println!("[{}] {}", i, p.file_name().unwrap().to_string_lossy()); }
    print!("Enter index: "); io::stdout().flush().ok(); let mut s=String::new(); io::stdin().read_line(&mut s)?; let i = s.trim().parse::<usize>().unwrap_or(0);
    if i>=rows.len(){ anyhow::bail!("Index out of range") }
    rows[i].clone()
  };
  let blob: EncBlobV1 = serde_json::from_slice(&fs::read(&file_path)?)?;
  let kj = decrypt_key(&blob, a.password.as_deref())?;
  print_json_compact(&render_key_json(&kj))
}

fn cmd_list() -> anyhow::Result<()> { ensure_keys_dir(); let mut rows: Vec<_>=fs::read_dir(keys_dir())?.filter_map(|e| e.ok()).filter(|e| e.path().is_file() && e.path().extension().map(|x|x=="json").unwrap_or(false)).map(|e| e.path()).collect(); rows.sort(); let items: Vec<_>=rows.iter().enumerate().map(|(i,p)| serde_json::json!({"index":i,"file":p.file_name().unwrap().to_string_lossy()})).collect(); let out=serde_json::json!({"keys_dir":keys_dir(),"items":items}); println!(""); println!("{}", serde_json::to_string_pretty(&out)?); Ok(()) }

fn cmd_select(a: SelectArgs) -> anyhow::Result<()> {
  ensure_keys_dir();
  let mut rows: Vec<_> = fs::read_dir(keys_dir())?
    .filter_map(|e| e.ok())
    .filter(|e| e.path().is_file() && e.path().extension().map(|x| x == "json").unwrap_or(false))
    .map(|e| e.path())
    .collect();
  rows.sort();
  if rows.is_empty(){ println!("{{}}"); return Ok(()); }
  let idx = if let Some(i)=a.index { if i>=rows.len(){ anyhow::bail!("Index out of range") } i } else { for (i,p) in rows.iter().enumerate(){ println!("[{}] {}", i, p.file_name().unwrap().to_string_lossy()); } print!("Enter index: "); io::stdout().flush().ok(); let mut s=String::new(); io::stdin().read_line(&mut s)?; s.trim().parse::<usize>().unwrap_or(0) };
  let chosen=&rows[idx];
  // Always decrypt and include the key content; prompt for password if not provided
  let blob: EncBlobV1 = serde_json::from_slice(&fs::read(chosen)?)?;
  let kj = decrypt_key(&blob, a.password.as_deref())?;
  let result = serde_json::json!({"index":idx, "selected": chosen, "key": render_key_json(&kj)});
  print_json_compact(&result)
}

 

fn resolve_out(out: Option<String>, name: Option<String>, scheme: &str) -> PathBuf {
  if let Some(p)=out { return PathBuf::from(p); }
  ensure_keys_dir(); let base = if let Some(n)=name { if n.ends_with(".json"){ n } else { format!("{}.json", n) } } else { format!("{}-{}.json", chrono::Utc::now().format("%Y%m%d-%H%M%S"), scheme) };
  keys_dir().join(base)
}

fn read_line_hidden() -> anyhow::Result<String> {
  // Fallback: simple read without echo; if unavailable, read plainly
  #[cfg(unix)]
  {
    use termios::*; use std::os::fd::AsRawFd;
    let fd = io::stdin().as_raw_fd(); let mut term = Termios::from_fd(fd)?; let old = term.clone(); term.c_lflag &= !ECHO; tcsetattr(fd, TCSANOW, &term)?; let mut s=String::new(); io::stdin().read_line(&mut s)?; tcsetattr(fd, TCSANOW, &old)?; Ok(s.trim_end_matches(['\n','\r']).to_string())
  }
  #[cfg(not(unix))]
  {
    let mut s=String::new(); io::stdin().read_line(&mut s)?; Ok(s.trim().to_string())
  }
}

fn from_phrase(phrase:&str, scheme:&str, network:&str) -> anyhow::Result<KeyJson> {
  require_subkey(); let out = run(&["subkey","inspect","--scheme",scheme,"--network",network, phrase])?; let (_ph, seed, pubh, ss58) = parse_subkey(&out);
  Ok(KeyJson{ scheme: scheme.into(), network: network.into(), byte_array:ss58.as_ref().and_then(|s| ss58_to_bytes(s).ok()).map(|b| format!("0x{}", hex::encode(b))), mnemonic_phrase: None, secret_phrase: Some(phrase.into()), public_key_hex: pubh, private_key_hex: seed, ss58_address: ss58, key_type: Some(scheme.into()), is_pair: Some(true), is_multisig: None, threshold: None, signers: None, multisig_address: None, created_at: Some(chrono::Utc::now().to_rfc3339()) })
}

fn from_public(public:&str, scheme:&str, network:&str) -> anyhow::Result<KeyJson> {
  require_subkey(); let out = run(&["subkey","inspect","--network",network,"--public","--scheme",scheme, public])?; let (_ph, seed, pubh, ss58) = parse_subkey(&out);
  Ok(KeyJson{ scheme: scheme.into(), network: network.into(), byte_array:ss58.as_ref().and_then(|s| ss58_to_bytes(s).ok()).map(|b| format!("0x{}", hex::encode(b))), mnemonic_phrase: None, secret_phrase: None, public_key_hex: pubh, private_key_hex: seed, ss58_address: ss58, key_type: Some("ss58".into()), is_pair: Some(false), is_multisig: None, threshold: None, signers: None, multisig_address: None, created_at: Some(chrono::Utc::now().to_rfc3339()) })
}

fn encrypt_key(kj:&KeyJson) -> anyhow::Result<EncBlobV1> {
  let payload = serde_json::to_vec(kj)?;
  let mut salt = [0u8;16]; rand::thread_rng().fill_bytes(&mut salt);
  let params = Params::new(14, 8, 1, 32)?; // N=2^14 = 16384, r=8, p=1
  // We prompt user for password interactively
  eprint!("Set password for key file: "); io::stderr().flush().ok(); let pw1 = read_line_hidden()?; eprint!("Confirm password: "); io::stderr().flush().ok(); let pw2 = read_line_hidden()?; if pw1!=pw2 { anyhow::bail!("Passwords do not match") }
  let mut key = [0u8;32]; scrypt::scrypt(pw1.as_bytes(), &salt, &params, &mut key)?;
  let nonce = {
    let mut n=[0u8;12]; rand::thread_rng().fill_bytes(&mut n); n
  };
  let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
  let ct = cipher
    .encrypt(Nonce::from_slice(&nonce), payload.as_ref())
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
  Ok(EncBlobV1{
    version:1,
    kdf:"scrypt".into(),
    salt: general_purpose::STANDARD.encode(&salt),
    params: EncParams{ n: 16384, r: 8, p:1 },
    nonce: general_purpose::STANDARD.encode(&nonce),
    ciphertext: general_purpose::STANDARD.encode(&ct),
    // store public metadata for safe reads
    scheme: Some(kj.scheme.clone()),
    network: Some(kj.network.clone()),
    byte_array: kj.byte_array.clone(),
    public_key_hex: kj.public_key_hex.clone(),
    ss58_address: kj.ss58_address.clone(),
  })
}

fn decrypt_key(blob:&EncBlobV1, password_opt: Option<&str>) -> anyhow::Result<KeyJson> {
  if blob.kdf.to_lowercase()!="scrypt" { anyhow::bail!("Unsupported KDF") }
  let salt = general_purpose::STANDARD.decode(&blob.salt)?;
  let n = blob.params.n.max(1);
  let r = blob.params.r.max(1);
  let p = blob.params.p.max(1);
  // Params::new takes log_n, so compute log2(n). Expect powers of two.
  // For powers of two, log2(n) = 31 - leading_zeros(n)
  let log_n = (31 - n.leading_zeros()) as u8;
  let params = Params::new(log_n, r, p, 32)?;
  let pw = match password_opt { Some(p)=>p.to_string(), None=>{ eprint!("Password for key file: "); io::stderr().flush().ok(); read_line_hidden()? } };
  let mut key=[0u8;32]; scrypt::scrypt(pw.as_bytes(), &salt, &params, &mut key)?;
  let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
  let nonce = general_purpose::STANDARD.decode(&blob.nonce)?; let ct = general_purpose::STANDARD.decode(&blob.ciphertext)?;
  let pt = cipher
    .decrypt(Nonce::from_slice(&nonce), ct.as_ref())
    .map_err(|_e| anyhow::anyhow!("Decryption failed: wrong password or corrupted key file"))?;
  let mut kj: KeyJson = serde_json::from_slice(&pt)?;
  // Ensure byte_array is hex string if bytes were provided
  if let Some(s) = kj.byte_array.as_ref() { if s.starts_with("0x")==false { kj.byte_array = Some(format!("0x{}", s)); } }
  Ok(kj)
}

fn ss58_to_bytes(addr:&str) -> anyhow::Result<[u8;32]> {
  let data = bs58::decode(addr).into_vec()?; if data.len()!=35 { anyhow::bail!("unsupported SS58 length") }
  let pubkey=&data[1..33]; let checksum=&data[33..35];
  let mut h = Blake2b512::new(); h.update(b"SS58PRE"); h.update(&data[..33]); let out=h.finalize(); if &out[..2]!=checksum { anyhow::bail!("invalid SS58 checksum") }
  let mut pk=[0u8;32]; pk.copy_from_slice(pubkey); Ok(pk)
}

fn ss58_encode(account_id:&[u8;32], addr_type:u8) -> String {
  let mut data = Vec::with_capacity(35); data.push(addr_type); data.extend_from_slice(account_id);
  let mut h = Blake2b512::new(); h.update(b"SS58PRE"); h.update(&data); let out=h.finalize(); let cs=&out[..2]; let mut full = data.clone(); full.extend_from_slice(cs); bs58::encode(full).into_string()
}
