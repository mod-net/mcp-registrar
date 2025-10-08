use thiserror::Error;

#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Tool error: {0}")]
    ToolError(String),

    #[error("Task error: {0}")]
    TaskError(String),

    #[error("Resource error: {0}")]
    ResourceError(String),

    #[error("Prompt error: {0}")]
    PromptError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, RegistryError>;
