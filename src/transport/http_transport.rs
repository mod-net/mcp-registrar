use crate::transport::McpServer;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::io;
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[derive(Clone)]
pub struct HttpTransportServer<S: McpServer> {
    addr: SocketAddr,
    server: S,
}

#[derive(Clone)]
struct AppState<S: McpServer> {
    server: S,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    _jsonrpc: Option<String>,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    id: Option<Value>,
}

impl<S: McpServer> HttpTransportServer<S> {
    pub fn new(addr: SocketAddr, server: S) -> Self {
        Self { addr, server }
    }

    pub async fn serve(self) -> io::Result<()> {
        let state = AppState {
            server: self.server,
        };

        let router = Router::new()
            .route("/rpc", post(handle_rpc::<S>))
            .route("/health", get(health))
            .with_state(state);

        let listener = TcpListener::bind(self.addr).await?;
        axum::serve(listener, router.into_make_service())
            .await
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
    }
}

fn build_success_response(id: &Option<Value>, result: Value) -> Value {
    let mut obj = Map::new();
    obj.insert("jsonrpc".into(), Value::String("2.0".into()));
    if let Some(identifier) = id {
        obj.insert("id".into(), identifier.clone());
    }
    obj.insert("result".into(), result);
    Value::Object(obj)
}

fn build_error_response(id: &Option<Value>, code: i64, message: &str) -> Value {
    let mut error_obj = Map::new();
    error_obj.insert("code".into(), Value::Number(code.into()));
    error_obj.insert("message".into(), Value::String(message.to_string()));

    let mut obj = Map::new();
    obj.insert("jsonrpc".into(), Value::String("2.0".into()));
    if let Some(identifier) = id {
        obj.insert("id".into(), identifier.clone());
    }
    obj.insert("error".into(), Value::Object(error_obj));
    Value::Object(obj)
}

fn error_code_from_message(message: &str) -> i64 {
    if message.starts_with("Unknown method") {
        -32601
    } else if message.starts_with("Invalid params") {
        -32602
    } else {
        -32603
    }
}

async fn handle_rpc<S: McpServer>(
    State(state): State<AppState<S>>,
    Json(payload): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    if payload.method.is_empty() {
        let body = Json(build_error_response(
            &payload.id,
            -32600,
            "Invalid Request: missing method",
        ));
        return (StatusCode::BAD_REQUEST, body);
    }

    match state.server.handle(&payload.method, payload.params).await {
        Ok(result) => {
            let body = Json(build_success_response(&payload.id, result));
            (StatusCode::OK, body)
        }
        Err(err) => {
            let code = error_code_from_message(&err.to_string());
            let body = Json(build_error_response(&payload.id, code, &err.to_string()));
            (StatusCode::BAD_REQUEST, body)
        }
    }
}

async fn health() -> impl axum::response::IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}
