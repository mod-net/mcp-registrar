use crate::transport::{HandlerResult, McpServer};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;
use std::env;
use std::time::Duration;

#[derive(Clone)]
pub struct TextGeneratorServer {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    default_model: Option<String>,
}

impl TextGeneratorServer {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| "OPENAI_API_KEY is not set")?;
        let base_url = env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let default_model = env::var("OPENAI_MODEL").ok();

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .gzip(true)
            .brotli(true)
            .build()?;

        Ok(Self {
            http,
            base_url,
            api_key,
            default_model,
        })
    }

    async fn handle_chat_completions(&self, mut body: Value) -> HandlerResult {
        // Ensure model is present; if not, use default from env
        if body.get("model").is_none() {
            if let Some(model) = &self.default_model {
                body["model"] = Value::String(model.clone());
            }
        }
        if body.get("model").is_none() {
            return Err("model is required (set OPENAI_MODEL or include in request)".into());
        }

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(format!("OpenAI API error ({}): {}", status, text).into());
        }
        let json: Value = serde_json::from_str(&text)?;
        Ok(json)
    }
}

#[async_trait]
impl McpServer for TextGeneratorServer {
    async fn handle(&self, name: &str, params: Value) -> HandlerResult {
        match name {
            // Accept a few common method aliases
            "ChatCompletionsCreate" | "chat.completions.create" | "CreateChatCompletion" | "chat_completions" => {
                self.handle_chat_completions(params).await
            }
            _ => Err(format!("Unknown method: {}", name).into()),
        }
    }
}
