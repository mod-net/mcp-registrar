use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    /// Unique identifier for the prompt
    pub id: String,
    
    /// Human-readable name of the prompt
    pub name: String,
    
    /// Description of what the prompt is for
    pub description: String,
    
    /// Server ID that provides this prompt
    pub server_id: String,
    
    /// Template text of the prompt
    pub template: String,
    
    /// When the prompt was registered
    pub registered_at: DateTime<Utc>,
    
    /// Schema for the prompt's variables
    pub variables_schema: Option<serde_json::Value>,
    
    /// Tags for categorizing and searching prompts
    pub tags: Vec<String>,
    
    /// Additional metadata about the prompt
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRender {
    /// ID of the prompt to render
    pub prompt_id: String,
    
    /// Variables to inject into the prompt template
    pub variables: serde_json::Value,
    
    /// Context for prompt rendering (e.g., user ID, conversation history)
    pub context: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRenderResult {
    /// The prompt render request that generated this result
    pub render: PromptRender,
    
    /// Result of the prompt render
    pub rendered_text: String,
    
    /// Any error information if the render failed
    pub error: Option<String>,
    
    /// Time when the render was completed
    pub rendered_at: DateTime<Utc>,
}

impl Prompt {
    /// Create a new prompt
    pub fn new(
        id: String,
        name: String,
        description: String,
        server_id: String,
        template: String,
        variables_schema: Option<serde_json::Value>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            server_id,
            template,
            registered_at: Utc::now(),
            variables_schema,
            tags,
            metadata: HashMap::new(),
        }
    }
    
    /// Add metadata to the prompt
    pub fn with_metadata(mut self, key: &str, value: serde_json::Value) -> Self {
        self.metadata.insert(key.to_string(), value);
        self
    }
    
    /// Validate variables against the prompt's schema
    pub fn validate_variables(&self, variables: &serde_json::Value) -> Result<(), String> {
        // In a real implementation, this would use JSON Schema validation
        if let Some(schema) = &self.variables_schema {
            if schema.is_object() && variables.is_object() {
                // Simple validation to check that required fields are present
                if let Some(required) = schema.get("required") {
                    if let Some(required_fields) = required.as_array() {
                        for field in required_fields {
                            if let Some(field_name) = field.as_str() {
                                if !variables.get(field_name).is_some() {
                                    return Err(format!("Required variable '{}' is missing", field_name));
                                }
                            }
                        }
                    }
                }
                return Ok(());
            }
            return Err("Variables must be an object".to_string());
        }
        // If no schema defined, accept any variables
        Ok(())
    }
    
    /// Render the prompt with the provided variables
    pub fn render(&self, variables: &serde_json::Value) -> Result<String, String> {
        // Validate variables
        self.validate_variables(variables)?;
        
        // In a real implementation, this would use a proper template engine
        // For now, we'll just do a simple string replacement
        let mut rendered = self.template.clone();
        
        if let Some(vars) = variables.as_object() {
            for (key, value) in vars {
                let placeholder = format!("{{{{{}}}}}", key);
                if let Some(value_str) = value.as_str() {
                    rendered = rendered.replace(&placeholder, value_str);
                } else {
                    let value_str = value.to_string();
                    rendered = rendered.replace(&placeholder, &value_str);
                }
            }
        }
        
        Ok(rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_creation() {
        let id = "prompt-1".to_string();
        let name = "Test Prompt".to_string();
        let description = "A test prompt".to_string();
        let server_id = "server-1".to_string();
        let template = "Hello, {{name}}!".to_string();
        let tags = vec!["test".to_string(), "greeting".to_string()];
        
        let prompt = Prompt::new(
            id.clone(),
            name.clone(),
            description.clone(),
            server_id.clone(),
            template.clone(),
            None,
            tags.clone(),
        );
        
        assert_eq!(prompt.id, id);
        assert_eq!(prompt.name, name);
        assert_eq!(prompt.description, description);
        assert_eq!(prompt.server_id, server_id);
        assert_eq!(prompt.template, template);
        assert_eq!(prompt.tags, tags);
        assert!(prompt.variables_schema.is_none());
        assert!(prompt.metadata.is_empty());
    }
    
    #[test]
    fn test_prompt_with_metadata() {
        let prompt = Prompt::new(
            "prompt-1".to_string(),
            "Test Prompt".to_string(),
            "A test prompt".to_string(),
            "server-1".to_string(),
            "Hello, {{name}}!".to_string(),
            None,
            vec!["test".to_string()],
        )
        .with_metadata("author", serde_json::json!("Test Author"))
        .with_metadata("version", serde_json::json!("1.0.0"));
        
        assert_eq!(prompt.metadata.len(), 2);
        assert_eq!(prompt.metadata.get("author").unwrap(), &serde_json::json!("Test Author"));
        assert_eq!(prompt.metadata.get("version").unwrap(), &serde_json::json!("1.0.0"));
    }
    
    #[test]
    fn test_prompt_render_simple() {
        let prompt = Prompt::new(
            "prompt-1".to_string(),
            "Test Prompt".to_string(),
            "A test prompt".to_string(),
            "server-1".to_string(),
            "Hello, {{name}}!".to_string(),
            None,
            vec!["test".to_string()],
        );
        
        let variables = serde_json::json!({
            "name": "World"
        });
        
        let result = prompt.render(&variables).unwrap();
        assert_eq!(result, "Hello, World!");
    }
    
    #[test]
    fn test_prompt_render_complex() {
        let prompt = Prompt::new(
            "prompt-1".to_string(),
            "Test Prompt".to_string(),
            "A test prompt".to_string(),
            "server-1".to_string(),
            "Hello, {{name}}! You are {{age}} years old and live in {{location}}.".to_string(),
            None,
            vec!["test".to_string()],
        );
        
        let variables = serde_json::json!({
            "name": "Alice",
            "age": 30,
            "location": "Wonderland"
        });
        
        let result = prompt.render(&variables).unwrap();
        assert_eq!(result, "Hello, Alice! You are 30 years old and live in Wonderland.");
    }
    
    #[test]
    fn test_prompt_render_with_schema_validation() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["name", "age"],
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "number"},
                "location": {"type": "string"}
            }
        });
        
        let prompt = Prompt::new(
            "prompt-1".to_string(),
            "Test Prompt".to_string(),
            "A test prompt".to_string(),
            "server-1".to_string(),
            "Hello, {{name}}! You are {{age}} years old.".to_string(),
            Some(schema),
            vec!["test".to_string()],
        );
        
        // Valid variables
        let valid_vars = serde_json::json!({
            "name": "Alice",
            "age": 30
        });
        assert!(prompt.render(&valid_vars).is_ok());
        
        // Missing required variable
        let invalid_vars = serde_json::json!({
            "name": "Alice"
        });
        let result = prompt.render(&invalid_vars);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Required variable 'age' is missing");
    }
    
    #[test]
    fn test_prompt_serialization() {
        let prompt = Prompt::new(
            "prompt-1".to_string(),
            "Test Prompt".to_string(),
            "A test prompt".to_string(),
            "server-1".to_string(),
            "Hello, {{name}}!".to_string(),
            Some(serde_json::json!({
                "type": "object",
                "required": ["name"]
            })),
            vec!["test".to_string(), "greeting".to_string()],
        )
        .with_metadata("author", serde_json::json!("Test Author"));
        
        let serialized = serde_json::to_string(&prompt).unwrap();
        let deserialized: Prompt = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(deserialized.id, prompt.id);
        assert_eq!(deserialized.name, prompt.name);
        assert_eq!(deserialized.template, prompt.template);
        assert_eq!(deserialized.tags, prompt.tags);
        assert_eq!(deserialized.metadata.get("author").unwrap(), &serde_json::json!("Test Author"));
        
        // Check that the schema was properly serialized and deserialized
        assert!(deserialized.variables_schema.is_some());
        let schema = deserialized.variables_schema.unwrap();
        assert_eq!(schema.get("required").unwrap(), &serde_json::json!(["name"]));
    }
} 