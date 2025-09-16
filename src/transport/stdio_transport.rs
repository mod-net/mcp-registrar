use crate::transport::McpServer;
use std::io;
use tokio::io::{AsyncBufRead, AsyncWrite, AsyncBufReadExt, AsyncWriteExt};

#[derive(Clone)]
pub struct StdioTransportServer<S: McpServer> {
    server: S,
}

impl<S: McpServer> StdioTransportServer<S> {
    pub fn new(server: S) -> Self {
        Self { server }
    }

    pub async fn serve_with_io<R: AsyncBufRead + AsyncBufReadExt + Unpin, W: AsyncWrite + AsyncWriteExt + Unpin>(
        &self,
        mut reader: R,
        mut writer: W,
    ) -> io::Result<()> {
        let server = self.server.clone();
        
        // Simple line-based protocol
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).await?;
            
            if n == 0 {
                // EOF
                break;
            }
            
            let response = match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(request) => {
                    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("unknown");
                    let params = request.get("params").unwrap_or(&serde_json::Value::Null).clone();
                    let id = request.get("id").cloned().unwrap_or(serde_json::Value::Null);
                    
                    match server.handle(method, params).await {
                        Ok(result) => {
                            let mut obj = serde_json::Map::new();
                            if !id.is_null() { obj.insert("id".into(), id.clone()); }
                            obj.insert("result".into(), result);
                            let s = serde_json::to_string(&serde_json::Value::Object(obj)).unwrap_or("{}".to_string());
                            format!("{}\n", s.replace("\":", "\": "))
                        }
                        Err(e) => {
                            let mut obj = serde_json::Map::new();
                            if !id.is_null() { obj.insert("id".into(), id.clone()); }
                            obj.insert(
                                "error".into(),
                                serde_json::json!({
                                    "message": e.to_string(),
                                })
                            );
                            let s = serde_json::to_string(&serde_json::Value::Object(obj)).unwrap_or("{}".to_string());
                            format!("{}\n", s.replace("\":", "\": "))
                        },
                    }
                },
                Err(e) => format!("{{\"error\": \"Invalid JSON: {}\" }}\n", e.to_string().replace("\"", "\\\"")),
            };
            
            writer.write_all(response.as_bytes()).await?;
            writer.flush().await?;
        }
        
        Ok(())
    }
}

pub trait TransportServer {
    fn serve(&self) -> impl std::future::Future<Output = io::Result<()>> + Send;
}

impl<S: McpServer> TransportServer for StdioTransportServer<S> {
    fn serve(&self) -> impl std::future::Future<Output = io::Result<()>> + Send {
        async move {
            let stdin = tokio::io::BufReader::new(tokio::io::stdin());
            let stdout = tokio::io::stdout();
            self.serve_with_io(stdin, stdout).await
        }
    }
} 
