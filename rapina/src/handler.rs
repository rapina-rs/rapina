//! Handler trait for named route handlers.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use http::Request;
use hyper::body::Incoming;

use crate::discovery::HeaderParamInfo;
use crate::error::ErrorVariant;
use crate::extract::PathParams;
use crate::response::BoxBody;
use crate::state::AppState;

type BoxFuture = Pin<Box<dyn Future<Output = hyper::Response<BoxBody>> + Send>>;

/// A named request handler.
///
/// Implemented by route macros (`#[get]`, `#[post]`, etc.) to provide
/// both handler logic and name for OpenAPI generation.
pub trait Handler: Clone + Send + Sync + 'static {
    /// Handler name used as operationId in OpenAPI.
    const NAME: &'static str;

    /// JSON Schema for the success response (if available).
    fn response_schema() -> Option<serde_json::Value> {
        None
    }

    /// JSON Schema for the request body (if available).
    fn request_schema() -> Option<serde_json::Value> {
        None
    }

    /// Content type for the request body (e.g., "application/json" or "application/x-www-form-urlencoded").
    fn request_content_type() -> Option<&'static str> {
        None
    }

    /// Whether the request body is required (true) or optional (false).
    /// Returns None if there is no request body.
    fn request_body_required() -> Option<bool> {
        None
    }

    /// Error variants for OpenAPI documentation.
    fn error_responses() -> Vec<ErrorVariant> {
        Vec::new()
    }

    /// Typed header parameters declared on this handler.
    fn header_parameters() -> Vec<HeaderParamInfo> {
        Vec::new()
    }

    /// Human-readable description of what this handler does.
    fn description() -> Option<&'static str> {
        None
    }

    /// Stable OpenAPI `operationId` override set via `id = "..."` on the route macro.
    /// When absent the handler name is used.
    fn operation_id() -> Option<&'static str> {
        None
    }

    /// Short one-line OpenAPI summary set via `summary = "..."` on the route macro.
    /// When absent a humanized version of the handler name is auto-generated.
    fn summary() -> Option<&'static str> {
        None
    }

    /// OpenAPI tags set via `tags = [...]` on the route macro.
    fn tags() -> &'static [&'static str] {
        &[]
    }

    /// Whether this operation is marked deprecated via `deprecated = true` on the route macro.
    fn deprecated() -> bool {
        false
    }

    /// Handle the request.
    fn call(&self, req: Request<Incoming>, params: PathParams, state: Arc<AppState>) -> BoxFuture;
}
