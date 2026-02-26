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

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use crate::app::Rapina;
    use crate::router::Router;
    use crate::testing::TestClient;

    #[tokio::test]
    async fn test_openapi_spec_returns_200_with_json_content_type() {
        let app = Rapina::new()
            .with_introspection(false)
            .openapi("Test API", "1.0.0")
            .router(Router::new().route(http::Method::GET, "/users", |_, _, _| async { "ok" }));

        let client = TestClient::new(app).await;
        let response = client.get("/__rapina/openapi.json").send().await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn test_openapi_spec_body_is_valid_json_with_spec_structure() {
        let app = Rapina::new()
            .with_introspection(false)
            .openapi("My API", "2.0.0")
            .router(Router::new().route(http::Method::GET, "/items", |_, _, _| async { "ok" }));

        let client = TestClient::new(app).await;
        let response = client.get("/__rapina/openapi.json").send().await;

        let body: serde_json::Value = response.json();
        assert_eq!(body["openapi"], "3.0.3");
        assert_eq!(body["info"]["title"], "My API");
        assert_eq!(body["info"]["version"], "2.0.0");
        assert!(body["paths"].is_object());
        assert!(body["paths"].get("/items").is_some());
    }

    #[tokio::test]
    async fn test_openapi_spec_without_registry_returns_404() {
        let app = Rapina::new()
            .with_introspection(false)
            .router(Router::new().route(http::Method::GET, "/items", |_, _, _| async { "ok" }));

        let client = TestClient::new(app).await;
        let response = client.get("/__rapina/openapi.json").send().await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
