use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::tool::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub runtime: String, // "process" | "wasm"
    pub description: Option<String>,
    pub entry: serde_json::Value,
    #[serde(default)]
    pub schema: ManifestSchema,
    #[serde(default)]
    pub policy: serde_json::Value,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManifestSchema {
    pub parameters: Option<serde_json::Value>,
    pub returns: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct LoadedTool {
    pub manifest: ToolManifest,
    pub manifest_path: PathBuf,
}

pub fn load_manifests(root: &Path) -> anyhow::Result<Vec<LoadedTool>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() != "tool.json" {
            continue;
        }
        let p = entry.path();
        let content = fs::read_to_string(p)?;
        match serde_json::from_str::<ToolManifest>(&content) {
            Ok(m) => out.push(LoadedTool {
                manifest: m,
                manifest_path: p.to_path_buf(),
            }),
            Err(e) => {
                tracing::warn!("failed to parse manifest {:?}: {}", p, e);
            }
        }
    }
    Ok(out)
}

pub fn to_tool(manifest: &ToolManifest) -> Tool {
    // Map manifest to existing Tool model
    let description = manifest
        .description
        .clone()
        .unwrap_or_else(|| manifest.name.clone());
    let categories = manifest
        .metadata
        .get("categories")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();

    Tool::new(
        manifest.id.clone(),
        manifest.name.clone(),
        description,
        manifest.version.clone(),
        "manifest".to_string(),
        categories,
        manifest.schema.parameters.clone(),
        manifest.schema.returns.clone(),
    )
}
