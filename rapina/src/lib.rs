//! # Rapina
//!
//! A fast, type-safe web framework for Rust inspired by FastAPI.
//!
//! Rapina focuses on **productivity**, **type safety**, and **clear conventions**,
//! making it easy to build production-ready APIs.
//!
//! ## Features
//!
//! - **Type-safe extractors** - Parse request data with compile-time guarantees
//! - **Declarative routing** - Use proc macros like `#[get]`, `#[post]` for clean route definitions
//! - **Middleware system** - Composable middleware with async support
//! - **Structured errors** - Standardized error responses with `trace_id` for debugging
//! - **Validation** - Built-in request validation using the `validator` crate
//! - **Observability** - Integrated tracing for structured logging
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use rapina::prelude::*;
//!
//! #[get("/")]
//! async fn hello() -> &'static str {
//!     "Hello, Rapina!"
//! }
//!
//! #[get("/users/:id")]
//! async fn get_user(id: Path<u64>) -> Result<Json<serde_json::Value>> {
//!     let id = id.into_inner();
//!     Ok(Json(serde_json::json!({ "id": id })))
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     let router = Router::new()
//!         .get("/", hello)
//!         .get("/users/:id", get_user);
//!
//!     Rapina::new()
//!         .router(router)
//!         .listen("127.0.0.1:3000")
//!         .await
//! }
//! ```
//!
//! ## Extractors
//!
//! Rapina provides several extractors for parsing request data:
//!
//! - [`Json`](extract::Json) - Parse JSON request bodies
//! - [`Path`](extract::Path) - Extract path parameters
//! - [`Query`](extract::Query) - Parse query string parameters
//! - [`Form`](extract::Form) - Parse URL-encoded form data
//! - [`Headers`](extract::Headers) - Access request headers
//! - [`Cookie`](extract::Cookie) - Extract and deserialize cookies
//! - [`State`](extract::State) - Access application state
//! - [`Context`](extract::Context) - Access request context with trace_id
//! - [`Validated`](extract::Validated) - Validate extracted data
//!
//! ## Middleware
//!
//! Built-in middleware for common use cases:
//!
//! - [`TimeoutMiddleware`](middleware::TimeoutMiddleware) - Request timeout handling
//! - [`BodyLimitMiddleware`](middleware::BodyLimitMiddleware) - Limit request body size
//! - [`TraceIdMiddleware`](middleware::TraceIdMiddleware) - Add trace IDs to requests
//! - [`RequestLogMiddleware`](middleware::RequestLogMiddleware) - Structured request logging
//! - [`RateLimitMiddleware`](middleware::RateLimitMiddleware) - Token bucket rate limiting
//! - [`CompressionMiddleware`](middleware::CompressionMiddleware) - Response compression (gzip, deflate)
//!
//! ## Introspection
//!
//! Access route metadata for documentation and tooling:
//!
//! - [`RouteInfo`](introspection::RouteInfo) - Metadata about registered routes
//!
//! ## Testing
//!
//! Integration testing utilities:
//!
//! - [`TestClient`](testing::TestClient) - Test client for integration testing

pub mod app;
pub mod auth;
pub mod config;
pub mod context;
pub mod error;
pub mod extract;
pub mod handler;
pub mod introspection;
pub mod middleware;
pub mod observability;
pub mod openapi;
pub mod response;
pub mod router;
pub mod server;
pub mod state;
pub mod test;
pub mod testing;

/// Convenient re-exports for common Rapina types.
///
/// This module re-exports the most commonly used types so you can
/// import everything you need with a single `use` statement:
///
/// ```
/// use rapina::prelude::*;
/// ```
pub mod prelude {
    pub use crate::app::Rapina;
    pub use crate::auth::{AuthConfig, CurrentUser, TokenResponse};
    pub use crate::config::{
        ConfigError, get_env, get_env_or, get_env_parsed, get_env_parsed_or, load_dotenv,
    };
    pub use crate::context::RequestContext;
    pub use crate::error::{DocumentedError, Error, ErrorVariant, IntoApiError, Result};
    pub use crate::extract::{Context, Cookie, Form, Headers, Json, Path, Query, State, Validated};
    pub use crate::introspection::RouteInfo;
    pub use crate::middleware::{
        CompressionConfig, KeyExtractor, Middleware, Next, RateLimitConfig,
    };
    pub use crate::observability::TracingConfig;
    pub use crate::response::IntoResponse;
    pub use crate::router::Router;

    pub use http::{Method, StatusCode};
    pub use schemars::JsonSchema;
    pub use serde::{Deserialize, Serialize};
    pub use tracing;
    pub use validator::Validate;

    pub use rapina_macros::{Config, delete, get, post, public, put};
}

// Re-export schemars and http crates so users don't need to add them to their Cargo.toml
pub use http;
pub use schemars;
