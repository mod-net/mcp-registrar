use crate::transport::{McpServer, HandlerResult};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use crate::models::prompt::{Prompt, PromptRender, PromptRenderResult};
use serde::{Deserialize, Serialize};
use chrono::Utc;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterPromptRequest {
    pub name: String,
    pub description: String,
    pub server_id: String,
    pub template: String,
    pub variables_schema: Option<serde_json::Value>,
    pub tags: Vec<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterPromptResponse {
    pub prompt_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListPromptsRequest {
    pub server_id: Option<String>,
    pub tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListPromptsResponse {
    pub prompts: Vec<Prompt>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetPromptRequest {
    pub prompt_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetPromptResponse {
    pub prompt: Prompt,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenderPromptRequest {
    pub render: PromptRender,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenderPromptResponse {
    pub result: PromptRenderResult,
}

#[derive(Debug, Clone)]
pub struct PromptRegistryServer {
    prompts: Arc<Mutex<HashMap<String, Prompt>>>,
    prompt_servers: Arc<Mutex<HashMap<String, String>>>, // Maps server_id to endpoint
}

impl PromptRegistryServer {
    pub fn new() -> Self {
        Self {
            prompts: Arc::new(Mutex::new(HashMap::new())),
            prompt_servers: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    fn register_prompt(&self, request: RegisterPromptRequest) -> Result<String, String> {
        let prompt_id = Uuid::new_v4().to_string();
        
        let mut prompt = Prompt::new(
            prompt_id.clone(),
            request.name,
            request.description,
            request.server_id.clone(),
            request.template,
            request.variables_schema,
            request.tags,
        );
        
        // Add metadata if provided
        if let Some(metadata) = request.metadata {
            for (key, value) in metadata {
                prompt = prompt.with_metadata(&key, value);
            }
        }
        
        // Verify that the server exists
        {
            let servers = self.prompt_servers.lock().unwrap();
            if !servers.contains_key(&request.server_id) {
                return Err(format!("Server with ID {} not registered", request.server_id));
            }
        }
        
        // Store the prompt
        let mut prompts = self.prompts.lock().unwrap();
        prompts.insert(prompt_id.clone(), prompt);
        
        Ok(prompt_id)
    }
    
    fn list_prompts(&self, request: &ListPromptsRequest) -> Vec<Prompt> {
        let prompts = self.prompts.lock().unwrap();
        
        let mut result = Vec::new();
        for prompt in prompts.values() {
            // Filter by server_id if specified
            if let Some(ref server_id) = request.server_id {
                if prompt.server_id != *server_id {
                    continue;
                }
            }
            
            // Filter by tag if specified
            if let Some(ref tag) = request.tag {
                if !prompt.tags.contains(&tag.to_string()) {
                    continue;
                }
            }
            
            result.push(prompt.clone());
        }
        
        result
    }
    
    fn get_prompt(&self, prompt_id: &str) -> Option<Prompt> {
        let prompts = self.prompts.lock().unwrap();
        prompts.get(prompt_id).cloned()
    }
    
    fn render_prompt(&self, render: PromptRender) -> Result<PromptRenderResult, String> {
        // Get the prompt
        let prompt = match self.get_prompt(&render.prompt_id) {
            Some(prompt) => prompt,
            None => return Err(format!("Prompt with ID {} not found", render.prompt_id)),
        };
        
        // Render the prompt
        let rendered_text = prompt.render(&render.variables)?;
        
        // Create the render result
        let render_result = PromptRenderResult {
            render,
            rendered_text,
            error: None,
            rendered_at: Utc::now(),
        };
        
        Ok(render_result)
    }
    
    pub fn register_server(&self, server_id: String, endpoint: String) {
        let mut servers = self.prompt_servers.lock().unwrap();
        servers.insert(server_id, endpoint);
    }
}

#[async_trait]
impl McpServer for PromptRegistryServer {
    async fn handle(&self, name: &str, params: serde_json::Value) -> HandlerResult {
        match name {
            "RegisterPrompt" => {
                let request: RegisterPromptRequest = serde_json::from_value(params)?;
                match self.register_prompt(request) {
                    Ok(prompt_id) => Ok(serde_json::to_value(RegisterPromptResponse { prompt_id })?),
                    Err(e) => Err(format!("Failed to register prompt: {}", e).into()),
                }
            },
            "ListPrompts" => {
                let request: ListPromptsRequest = serde_json::from_value(params)?;
                let prompts = self.list_prompts(&request);
                Ok(serde_json::to_value(ListPromptsResponse { prompts })?)
            },
            "GetPrompt" => {
                let request: GetPromptRequest = serde_json::from_value(params)?;
                match self.get_prompt(&request.prompt_id) {
                    Some(prompt) => Ok(serde_json::to_value(GetPromptResponse { prompt })?),
                    None => Err(format!("Prompt not found: {}", request.prompt_id).into()),
                }
            },
            "RenderPrompt" => {
                let request: RenderPromptRequest = serde_json::from_value(params)?;
                match self.render_prompt(request.render) {
                    Ok(result) => Ok(serde_json::to_value(RenderPromptResponse { result })?),
                    Err(e) => Err(format!("Prompt rendering failed: {}", e).into()),
                }
            },
            "RegisterServer" => {
                let server_id = params["server_id"].as_str().ok_or("Missing server_id")?;
                let endpoint = params["endpoint"].as_str().ok_or("Missing endpoint")?;
                self.register_server(server_id.to_string(), endpoint.to_string());
                Ok(serde_json::json!({ "success": true }))
            },
            _ => Err(format!("Unknown method: {}", name).into()),
        }
    }
} 