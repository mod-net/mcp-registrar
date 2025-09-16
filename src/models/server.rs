use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub schema_url: Option<String>,
    pub capabilities: Vec<String>,
    pub registered_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    pub status: ServerStatus,
    pub endpoint: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum ServerStatus {
    Active,
    Inactive,
    Error,
}

impl ServerInfo {
    pub fn new(
        id: String,
        name: String,
        description: String,
        version: String,
        schema_url: Option<String>,
        capabilities: Vec<String>,
        endpoint: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            description,
            version,
            schema_url,
            capabilities,
            registered_at: now,
            last_heartbeat: now,
            status: ServerStatus::Active,
            endpoint,
        }
    }
    
    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_server_info_creation() {
        let id = "server-1".to_string();
        let name = "Test Server".to_string();
        let description = "A test server".to_string();
        let version = "1.0.0".to_string();
        let schema_url = Some("https://example.com/schema.json".to_string());
        let capabilities = vec!["capability1".to_string(), "capability2".to_string()];
        let endpoint = "http://localhost:8080".to_string();
        
        let server = ServerInfo::new(
            id.clone(),
            name.clone(),
            description.clone(),
            version.clone(),
            schema_url.clone(),
            capabilities.clone(),
            endpoint.clone(),
        );
        
        assert_eq!(server.id, id);
        assert_eq!(server.name, name);
        assert_eq!(server.description, description);
        assert_eq!(server.version, version);
        assert_eq!(server.schema_url, schema_url);
        assert_eq!(server.capabilities, capabilities);
        assert_eq!(server.endpoint, endpoint);
        assert_eq!(server.status, ServerStatus::Active);
        assert_eq!(server.registered_at, server.last_heartbeat);
    }
    
    #[test]
    fn test_server_heartbeat_update() {
        let server_info = ServerInfo::new(
            "server-1".to_string(),
            "Test Server".to_string(),
            "A test server".to_string(),
            "1.0.0".to_string(),
            None,
            vec!["capability1".to_string()],
            "http://localhost:8080".to_string(),
        );
        
        let initial_heartbeat = server_info.last_heartbeat;
        
        // Sleep to ensure time difference
        sleep(Duration::from_millis(10));
        
        let mut server_info_copy = server_info.clone();
        server_info_copy.update_heartbeat();
        
        assert!(server_info_copy.last_heartbeat > initial_heartbeat);
    }
    
    #[test]
    fn test_server_status_serialization() {
        // Test active status
        let active_status = ServerStatus::Active;
        let serialized = serde_json::to_string(&active_status).unwrap();
        let deserialized: ServerStatus = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ServerStatus::Active);
        
        // Test inactive status
        let inactive_status = ServerStatus::Inactive;
        let serialized = serde_json::to_string(&inactive_status).unwrap();
        let deserialized: ServerStatus = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ServerStatus::Inactive);
        
        // Test error status
        let error_status = ServerStatus::Error;
        let serialized = serde_json::to_string(&error_status).unwrap();
        let deserialized: ServerStatus = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ServerStatus::Error);
    }
    
    #[test]
    fn test_server_info_serialization() {
        let server_info = ServerInfo::new(
            "server-1".to_string(),
            "Test Server".to_string(),
            "A test server".to_string(),
            "1.0.0".to_string(),
            Some("https://example.com/schema.json".to_string()),
            vec!["capability1".to_string(), "capability2".to_string()],
            "http://localhost:8080".to_string(),
        );
        
        let serialized = serde_json::to_string(&server_info).unwrap();
        let deserialized: ServerInfo = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(deserialized.id, server_info.id);
        assert_eq!(deserialized.name, server_info.name);
        assert_eq!(deserialized.description, server_info.description);
        assert_eq!(deserialized.version, server_info.version);
        assert_eq!(deserialized.schema_url, server_info.schema_url);
        assert_eq!(deserialized.capabilities, server_info.capabilities);
        assert_eq!(deserialized.endpoint, server_info.endpoint);
        assert_eq!(deserialized.status, server_info.status);
        
        // Compare timestamps as ISO 8601 strings to avoid precision issues
        assert_eq!(
            deserialized.registered_at.to_rfc3339(),
            server_info.registered_at.to_rfc3339()
        );
        assert_eq!(
            deserialized.last_heartbeat.to_rfc3339(),
            server_info.last_heartbeat.to_rfc3339()
        );
    }
} 