use tower_http::cors::{Any, CorsLayer};

/// Creates a CORS middleware layer for Axum-based transports like SSE
pub fn create_cors_middleware() -> CorsLayer {
    CorsLayer::new()
        // Allow requests from any origin
        .allow_origin(Any)
        // Allow common HTTP methods
        .allow_methods(Any)
        // Allow common headers
        .allow_headers(Any)
        // Allow credentials
        .allow_credentials(true)
        // Expose all headers in response
        .expose_headers(Any)
}

/// Creates a CORS middleware layer with specific allowed origins
pub fn create_cors_middleware_with_origins(origins: Vec<&str>) -> CorsLayer {
    let origins = origins
        .into_iter()
        .filter_map(|origin| origin.parse().ok())
        .collect::<Vec<_>>();

    CorsLayer::new()
        // Set allowed origins
        .allow_origin(origins)
        // Allow common HTTP methods
        .allow_methods(Any)
        // Allow common headers
        .allow_headers(Any)
        // Allow credentials
        .allow_credentials(true)
        // Expose all headers in response
        .expose_headers(Any)
} 