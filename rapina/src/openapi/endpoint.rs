//! OpenAPI endpoint for exposing the API specification

use std::sync::Arc;

use http::{Request, Response, StatusCode};
use hyper::body::Incoming;

use crate::{extract::PathParams, openapi::OpenApiSpec, response::BoxBody, state::AppState};

/// Registry for storing the OpenAPI spec
#[derive(Debug, Clone)]
pub struct OpenApiRegistry {
    spec: OpenApiSpec,
}

impl OpenApiRegistry {
    pub fn new(spec: OpenApiSpec) -> Self {
        Self { spec }
    }

    pub fn spec(&self) -> &OpenApiSpec {
        &self.spec
    }
}

/// Handler for the OpenAPI endpoint
///
/// Returns the OpenAPI specification as JSON
pub async fn openapi_spec(
    _req: Request<Incoming>,
    _params: PathParams,
    state: Arc<AppState>,
) -> Response<BoxBody> {
    let registry = state.get::<OpenApiRegistry>();

    match registry {
        Some(registry) => {
            let json = serde_json::to_vec_pretty(registry.spec()).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(http_body_util::Full::new(bytes::Bytes::from(json)))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .body(http_body_util::Full::new(bytes::Bytes::from(
                r#"{"error": "OpenAPI spec not configured"}"#,
            )))
            .unwrap(),
    }
}
