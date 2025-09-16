use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResourceType {
    FileSystem,
    Database,
    RemoteApi,
    ObjectStore,
    MessageQueue,
    Cache,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// Unique identifier for the resource
    pub id: String,
    
    /// Human-readable name of the resource
    pub name: String,
    
    /// Detailed description of what the resource provides
    pub description: String,
    
    /// Type of resource
    pub resource_type: ResourceType,
    
    /// Server ID that provides this resource
    pub server_id: String,
    
    /// Access path or URL template for the resource
    pub access_path: String,
    
    /// When the resource was registered
    pub registered_at: DateTime<Utc>,
    
    /// Schema for the resource's data model (if applicable)
    pub schema: Option<serde_json::Value>,
    
    /// Schema for query parameters (if applicable)
    pub query_schema: Option<serde_json::Value>,
    
    /// Additional metadata about the resource
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceQuery {
    /// ID of the resource to query
    pub resource_id: String,
    
    /// Query parameters
    pub parameters: serde_json::Value,
    
    /// Query context
    pub context: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceQueryResult {
    /// The resource query that generated this result
    pub query: ResourceQuery,
    
    /// Result of the query
    pub result: serde_json::Value,
    
    /// Any error information if the query failed
    pub error: Option<String>,
    
    /// Time when the query started
    pub started_at: DateTime<Utc>,
    
    /// Time when the query completed
    pub completed_at: DateTime<Utc>,
}

impl Resource {
    /// Create a new resource
    pub fn new(
        id: String,
        name: String,
        description: String,
        resource_type: ResourceType,
        server_id: String,
        access_path: String,
        schema: Option<serde_json::Value>,
        query_schema: Option<serde_json::Value>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            resource_type,
            server_id,
            access_path,
            registered_at: Utc::now(),
            schema,
            query_schema,
            metadata: HashMap::new(),
        }
    }
    
    /// Add metadata to the resource
    pub fn with_metadata(mut self, key: &str, value: serde_json::Value) -> Self {
        self.metadata.insert(key.to_string(), value);
        self
    }
    
    /// Validate query parameters against the resource's schema
    pub fn validate_query(&self, parameters: &serde_json::Value) -> Result<(), String> {
        // In a real implementation, this would use more robust validation
        // For now, we'll just do a simple check if schema exists
        if let Some(schema) = &self.query_schema {
            if schema.is_object() && parameters.is_object() {
                // Simple validation to check that required fields are present
                if let Some(required) = schema.get("required") {
                    if let Some(required_fields) = required.as_array() {
                        for field in required_fields {
                            if let Some(field_name) = field.as_str() {
                                if !parameters.get(field_name).is_some() {
                                    return Err(format!("Required parameter '{}' is missing", field_name));
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
    fn test_resource_creation() {
        let id = "resource-1".to_string();
        let name = "Test Database".to_string();
        let description = "A test database resource".to_string();
        let resource_type = ResourceType::Database;
        let server_id = "server-1".to_string();
        let access_path = "postgresql://localhost:5432/testdb".to_string();
        
        let resource = Resource::new(
            id.clone(),
            name.clone(),
            description.clone(),
            resource_type,
            server_id.clone(),
            access_path.clone(),
            None,
            None,
        );
        
        assert_eq!(resource.id, id);
        assert_eq!(resource.name, name);
        assert_eq!(resource.description, description);
        assert_eq!(resource.resource_type, ResourceType::Database);
        assert_eq!(resource.server_id, server_id);
        assert_eq!(resource.access_path, access_path);
        assert!(resource.schema.is_none());
        assert!(resource.query_schema.is_none());
        assert!(resource.metadata.is_empty());
    }
    
    #[test]
    fn test_resource_with_metadata() {
        let resource = Resource::new(
            "resource-1".to_string(),
            "Test API".to_string(),
            "A test API resource".to_string(),
            ResourceType::RemoteApi,
            "server-1".to_string(),
            "https://api.example.com/v1".to_string(),
            None,
            None,
        )
        .with_metadata("region", serde_json::json!("us-west-1"))
        .with_metadata("rate_limit", serde_json::json!(100));
        
        assert_eq!(resource.metadata.len(), 2);
        assert_eq!(resource.metadata.get("region").unwrap(), &serde_json::json!("us-west-1"));
        assert_eq!(resource.metadata.get("rate_limit").unwrap(), &serde_json::json!(100));
    }
    
    #[test]
    fn test_resource_validate_query_no_schema() {
        let resource = Resource::new(
            "resource-1".to_string(),
            "Test Database".to_string(),
            "A test database resource".to_string(),
            ResourceType::Database,
            "server-1".to_string(),
            "postgresql://localhost:5432/testdb".to_string(),
            None,
            None,
        );
        
        // Without a schema, any query parameters should be valid
        let result = resource.validate_query(&serde_json::json!({"query": "SELECT * FROM users"}));
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_resource_validate_query_with_schema() {
        let query_schema = serde_json::json!({
            "type": "object",
            "required": ["query", "limit"],
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "number"},
                "offset": {"type": "number"}
            }
        });
        
        let resource = Resource::new(
            "resource-1".to_string(),
            "Test Database".to_string(),
            "A test database resource".to_string(),
            ResourceType::Database,
            "server-1".to_string(),
            "postgresql://localhost:5432/testdb".to_string(),
            None,
            Some(query_schema),
        );
        
        // Valid query parameters
        let valid_params = serde_json::json!({
            "query": "SELECT * FROM users",
            "limit": 10
        });
        assert!(resource.validate_query(&valid_params).is_ok());
        
        // Missing required parameter
        let invalid_params = serde_json::json!({
            "query": "SELECT * FROM users"
        });
        let result = resource.validate_query(&invalid_params);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Required parameter 'limit' is missing");
        
        // Non-object parameters
        let non_object_params = serde_json::json!("SELECT * FROM users");
        let result = resource.validate_query(&non_object_params);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Parameters must be an object");
    }
    
    #[test]
    fn test_resource_type_serialization() {
        // Standard resource types
        let db_type = ResourceType::Database;
        let serialized = serde_json::to_string(&db_type).unwrap();
        let deserialized: ResourceType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ResourceType::Database);
        
        // Custom resource type
        let custom_type = ResourceType::Other("CustomType".to_string());
        let serialized = serde_json::to_string(&custom_type).unwrap();
        let deserialized: ResourceType = serde_json::from_str(&serialized).unwrap();
        
        if let ResourceType::Other(custom_name) = deserialized {
            assert_eq!(custom_name, "CustomType");
        } else {
            panic!("Expected ResourceType::Other, got {:?}", deserialized);
        }
    }
    
    #[test]
    fn test_resource_serialization() {
        let resource = Resource::new(
            "resource-1".to_string(),
            "Test API".to_string(),
            "A test API resource".to_string(),
            ResourceType::RemoteApi,
            "server-1".to_string(),
            "https://api.example.com/v1".to_string(),
            Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "name": {"type": "string"}
                }
            })),
            Some(serde_json::json!({
                "type": "object",
                "required": ["endpoint"]
            })),
        )
        .with_metadata("region", serde_json::json!("us-west-1"));
        
        let serialized = serde_json::to_string(&resource).unwrap();
        let deserialized: Resource = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(deserialized.id, resource.id);
        assert_eq!(deserialized.name, resource.name);
        assert_eq!(deserialized.access_path, resource.access_path);
        
        // Check resource type serialization
        assert_eq!(deserialized.resource_type, ResourceType::RemoteApi);
        
        // Check metadata
        assert_eq!(deserialized.metadata.get("region").unwrap(), &serde_json::json!("us-west-1"));
        
        // Check schemas
        assert!(deserialized.schema.is_some());
        assert!(deserialized.query_schema.is_some());
        let query_schema = deserialized.query_schema.unwrap();
        assert_eq!(query_schema.get("required").unwrap(), &serde_json::json!(["endpoint"]));
    }
} 