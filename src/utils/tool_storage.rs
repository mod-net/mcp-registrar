use crate::models::tool::Tool;
use anyhow::{Context, Result};
use serde::{
    de::{self, Deserializer, MapAccess, SeqAccess, Visitor},
    ser::SerializeStruct,
    Serialize, Serializer, Deserialize,
};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, info, warn};

#[async_trait::async_trait]
pub trait ToolStorage: Send + Sync + fmt::Debug {
    async fn initialize(&self) -> Result<(), String>;
    async fn save_tool(&self, tool: Tool) -> Result<(), String>;
    async fn get_tool(&self, id: &str) -> Result<Option<Tool>, String>;
    async fn list_tools(&self) -> Result<Vec<Tool>, String>;
    async fn delete_tool(&self, id: &str) -> Result<(), String>;
}

#[derive(Debug)]
pub struct FileToolStorage {
    file_path: PathBuf,
    tools: Arc<TokioMutex<HashMap<String, Tool>>>,
}

impl FileToolStorage {
    pub fn new(file_path: PathBuf) -> Self {
        info!("Creating FileToolStorage with path: {:?}", file_path);
        Self {
            file_path,
            tools: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    pub async fn initialize(&self) -> Result<(), String> {
        info!("Initializing FileToolStorage");
        self.load_from_file().await.map_err(|e| e.to_string())
    }

    async fn load_from_file(&self) -> Result<()> {
        info!("Loading tools from file: {:?}", self.file_path);
        if self.file_path.exists() {
            let contents = fs::read_to_string(&self.file_path)
                .await
                .context("Failed to read tools file")?;
            let tools: HashMap<String, Tool> =
                serde_json::from_str(&contents).context("Failed to parse tools file")?;
            let mut tools_lock = self.tools.lock().await;
            *tools_lock = tools;
            info!("Loaded {} tools from file", tools_lock.len());
        } else {
            warn!("Tools file does not exist, starting with empty storage");
        }
        Ok(())
    }

    async fn save_to_file(&self, tools: &HashMap<String, Tool>) -> Result<()> {
        info!("Saving {} tools to file: {:?}", tools.len(), self.file_path);
        let contents = serde_json::to_string_pretty(tools).context("Failed to serialize tools")?;
        // Atomic write: write to temp file in same directory, then rename
        let tmp_path = self
            .file_path
            .with_extension("json.tmp");
        fs::write(&tmp_path, &contents)
            .await
            .context("Failed to write temp tools file")?;
        if let Err(e) = fs::rename(&tmp_path, &self.file_path).await {
            warn!("Atomic rename failed ({}). Falling back to direct write.", e);
            fs::write(&self.file_path, &contents)
                .await
                .context("Failed to write tools file (fallback)")?;
            // Best-effort cleanup
            let _ = fs::remove_file(&tmp_path).await;
        }
        info!("Successfully saved tools to file (atomic)");
        Ok(())
    }
}

impl Serialize for FileToolStorage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("FileToolStorage", 1)?;
        state.serialize_field("file_path", &self.file_path)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for FileToolStorage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field { FilePath }

        struct FileToolStorageVisitor;

        impl<'de> Visitor<'de> for FileToolStorageVisitor {
            type Value = PathBuf;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct FileToolStorage")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let file_path = seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                Ok(file_path)
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut file_path = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::FilePath => {
                            if file_path.is_some() {
                                return Err(de::Error::duplicate_field("file_path"));
                            }
                            file_path = Some(map.next_value()?);
                        }
                    }
                }
                let file_path = file_path.ok_or_else(|| de::Error::missing_field("file_path"))?;
                Ok(file_path)
            }
        }

        const FIELDS: &'static [&'static str] = &["file_path"];
        deserializer
            .deserialize_struct("FileToolStorage", FIELDS, FileToolStorageVisitor)
            .map(|file_path| FileToolStorage {
                file_path,
                tools: Arc::new(TokioMutex::new(HashMap::new())),
            })
    }
}

#[async_trait::async_trait]
impl ToolStorage for FileToolStorage {
    async fn initialize(&self) -> Result<(), String> {
        self.load_from_file().await.map_err(|e| e.to_string())
    }

    async fn save_tool(&self, tool: Tool) -> Result<(), String> {
        debug!("Saving tool: {}", tool.id);
        let mut tools = self.tools.lock().await;
        tools.insert(tool.id.clone(), tool);
        let tools_clone = tools.clone();
        drop(tools);
        self.save_to_file(&tools_clone)
            .await
            .map_err(|e| e.to_string())
    }

    async fn get_tool(&self, id: &str) -> Result<Option<Tool>, String> {
        debug!("Getting tool: {}", id);
        let tools = self.tools.lock().await;
        Ok(tools.get(id).cloned())
    }

    async fn list_tools(&self) -> Result<Vec<Tool>, String> {
        debug!("Listing all tools");
        let tools = self.tools.lock().await;
        Ok(tools.values().cloned().collect())
    }

    async fn delete_tool(&self, id: &str) -> Result<(), String> {
        debug!("Deleting tool: {}", id);
        let mut tools = self.tools.lock().await;
        tools.remove(id);
        let tools_clone = tools.clone();
        drop(tools);
        self.save_to_file(&tools_clone)
            .await
            .map_err(|e| e.to_string())
    }
}
