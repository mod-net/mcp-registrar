use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn cache_root() -> PathBuf {
    std::env::var("REGISTRY_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            Path::new(&home).join(".cache").join("registry-scheduler")
        })
}

fn ensure_dir(p: &Path) {
    let _ = fs::create_dir_all(p);
}

fn key_to_path(key: &str) -> PathBuf {
    let mut sanitized = key.replace('/', "_").replace(':', "-");
    if sanitized.len() > 200 { sanitized.truncate(200); }
    cache_root().join("modules").join(sanitized)
}

pub fn read(key: &str) -> Option<Vec<u8>> {
    let path = key_to_path(key);
    if path.exists() {
        let mut f = fs::File::open(path).ok()?;
        let mut buf = Vec::new();
        let _ = f.read_to_end(&mut buf).ok()?;
        Some(buf)
    } else {
        None
    }
}

pub fn write(key: &str, bytes: &[u8]) {
    let dir = cache_root().join("modules");
    ensure_dir(&dir);
    let path = key_to_path(key);
    if let Ok(mut f) = fs::File::create(path) {
        let _ = f.write_all(bytes);
    }
}

