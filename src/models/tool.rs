use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::string::ToString;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Unique identifier for the tool
    pub id: String,

    /// Human-readable name of the tool
    pub name: String,

    /// Detailed description of what the tool does
    pub description: String,

    /// Version string, typically following semantic versioning
    pub version: String,

    /// Server ID that provides this tool
    pub server_id: String,

    /// Categories this tool belongs to (e.g., "math", "data-processing")
    pub categories: Vec<String>,

    /// When the tool was registered
    pub registered_at: DateTime<Utc>,

    /// Schema for the tool's parameters
    pub parameters_schema: Option<serde_json::Value>,

    /// Schema for the tool's return value
    pub returns_schema: Option<serde_json::Value>,

    /// Additional metadata about the tool
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ToString for Tool {
    fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    /// ID of the tool to invoke
    pub tool_id: String,

    /// Parameters to pass to the tool
    pub parameters: serde_json::Value,

    /// Invocation context (e.g., user ID, session information)
    pub context: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocationResult {
    /// The tool invocation that generated this result
    pub invocation: ToolInvocation,

    /// Result of the tool invocation
    pub result: serde_json::Value,

    /// Any error information if the invocation failed
    pub error: Option<String>,

    /// Time when the invocation started
    pub started_at: DateTime<Utc>,

    /// Time when the invocation completed
    pub completed_at: DateTime<Utc>,
}

impl Tool {
    /// Create a new tool instance
    pub fn new(
        id: String,
        name: String,
        description: String,
        version: String,
        server_id: String,
        categories: Vec<String>,
        parameters_schema: Option<serde_json::Value>,
        returns_schema: Option<serde_json::Value>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            version,
            server_id,
            categories,
            registered_at: Utc::now(),
            parameters_schema,
            returns_schema,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the tool
    pub fn with_metadata(mut self, key: &str, value: serde_json::Value) -> Self {
        self.metadata.insert(key.to_string(), value);
        self
    }

    /// Validate parameters against the tool's schema
    pub fn validate_parameters(&self, parameters: &serde_json::Value) -> Result<(), String> {
        // In a real implementation, this would use JSON Schema validation
        // For now, we'll just do a simple check if schema exists
        if let Some(schema) = &self.parameters_schema {
            if schema.is_object() && parameters.is_object() {
                // Simple validation to check that required fields are present
                if let Some(required) = schema.get("required") {
                    if let Some(required_fields) = required.as_array() {
                        for field in required_fields {
                            if let Some(field_name) = field.as_str() {
                                if !parameters.get(field_name).is_some() {
                                    return Err(format!(
                                        "Required parameter '{}' is missing",
                                        field_name
                                    ));
                                }
                            }
                        }
                    }
                }
                return Ok(());
            }
            return Err("Parameters must be an object".to_string());
        }
        // If no schema defined, accept any parameters
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_creation() {
        let id = "tool-1".to_string();
        let name = "Test Tool".to_string();
        let description = "A test tool".to_string();
        let version = "1.0.0".to_string();
        let server_id = "server-1".to_string();
        let categories = vec!["test".to_string(), "utility".to_string()];

        let tool = Tool::new(
            id.clone(),
            name.clone(),
            description.clone(),
            version.clone(),
            server_id.clone(),
            categories.clone(),
            None,
            None,
        );

        assert_eq!(tool.id, id);
        assert_eq!(tool.name, name);
        assert_eq!(tool.description, description);
        assert_eq!(tool.version, version);
        assert_eq!(tool.server_id, server_id);
        assert_eq!(tool.categories, categories);
        assert!(tool.parameters_schema.is_none());
        assert!(tool.returns_schema.is_none());
        assert!(tool.metadata.is_empty());
    }

    #[test]
    fn test_tool_with_metadata() {
        let tool = Tool::new(
            "tool-1".to_string(),
            "Test Tool".to_string(),
            "A test tool".to_string(),
            "1.0.0".to_string(),
            "server-1".to_string(),
            vec!["test".to_string()],
            None,
            None,
        )
        .with_metadata("author", serde_json::json!("Test Author"))
        .with_metadata("tags", serde_json::json!(["tag1", "tag2"]));

        assert_eq!(tool.metadata.len(), 2);
        assert_eq!(
            tool.metadata.get("author").unwrap(),
            &serde_json::json!("Test Author")
        );
        assert_eq!(
            tool.metadata.get("tags").unwrap(),
            &serde_json::json!(["tag1", "tag2"])
        );
    }

    #[test]
    fn test_tool_validate_parameters_no_schema() {
        let tool = Tool::new(
            "tool-1".to_string(),
            "Test Tool".to_string(),
            "A test tool".to_string(),
            "1.0.0".to_string(),
            "server-1".to_string(),
            vec!["test".to_string()],
            None,
            None,
        );

        // Without a schema, any parameters should be valid
        let result = tool.validate_parameters(&serde_json::json!({"param1": "value1"}));
        assert!(result.is_ok());
    }

    #[test]
    fn test_tool_validate_parameters_with_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["param1", "param2"],
            "properties": {
                "param1": {"type": "string"},
                "param2": {"type": "number"}
            }
        });

        let tool = Tool::new(
            "tool-1".to_string(),
            "Test Tool".to_string(),
            "A test tool".to_string(),
            "1.0.0".to_string(),
            "server-1".to_string(),
            vec!["test".to_string()],
            Some(schema),
            None,
        );

        // Valid parameters
        let valid_params = serde_json::json!({
            "param1": "value1",
            "param2": 42
        });
        assert!(tool.validate_parameters(&valid_params).is_ok());

        // Missing required parameter
        let invalid_params = serde_json::json!({
            "param1": "value1"
        });
        let result = tool.validate_parameters(&invalid_params);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Required parameter 'param2' is missing"
        );

        // Non-object parameters
        let non_object_params = serde_json::json!("not an object");
        let result = tool.validate_parameters(&non_object_params);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Parameters must be an object");
    }

    #[test]
    fn test_tool_serialization() {
        let tool = Tool::new(
            "tool-1".to_string(),
            "Test Tool".to_string(),
            "A test tool".to_string(),
            "1.0.0".to_string(),
            "server-1".to_string(),
            vec!["test".to_string(), "utility".to_string()],
            Some(serde_json::json!({
                "type": "object",
                "required": ["param1"]
            })),
            Some(serde_json::json!({
                "type": "object"
            })),
        )
        .with_metadata("author", serde_json::json!("Test Author"));

        let serialized = serde_json::to_string(&tool).unwrap();
        let deserialized: Tool = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.id, tool.id);
        assert_eq!(deserialized.name, tool.name);
        assert_eq!(deserialized.categories, tool.categories);
        assert_eq!(
            deserialized.metadata.get("author").unwrap(),
            &serde_json::json!("Test Author")
        );

        // Check that the schemas were properly serialized and deserialized
        assert!(deserialized.parameters_schema.is_some());
        let schema = deserialized.parameters_schema.unwrap();
        assert_eq!(
            schema.get("required").unwrap(),
            &serde_json::json!(["param1"])
        );
    }
}
