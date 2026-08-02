//! Swagger UI endpoint for interactive API exploration.
//!
//! Enabled by the `swagger-ui` feature. Serves a self-contained HTML page that
//! loads the Swagger UI bundle from a CDN and points it at the OpenAPI JSON
//! endpoint (`/__rapina/openapi.json`).
//!
//! # Usage
//!
//! ```rust,ignore
//! Rapina::new()
//!     .openapi("My API", "1.0.0")
//!     .swagger_ui()                           // default path: /__rapina/swagger/
//!     // or: .swagger_ui_at("/__rapina/docs/")
//!     .discover()
//!     .listen("127.0.0.1:3000")
//!     .await?;
//! ```

use std::sync::Arc;

use http::{Request, Response, StatusCode, header::CONTENT_TYPE};
use hyper::body::Incoming;

use crate::{
    extract::PathParams,
    response::{BoxBody, full},
    state::AppState,
};

/// Stores the swagger-ui configuration in AppState.
#[derive(Debug, Clone)]
pub struct SwaggerUiConfig {
    /// Path where Swagger UI is served (e.g. `/__rapina/swagger/`).
    pub path: String,
    /// URL of the OpenAPI JSON spec (e.g. `/__rapina/openapi.json`).
    pub spec_url: String,
}

impl SwaggerUiConfig {
    pub fn new(path: impl Into<String>, spec_url: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            spec_url: spec_url.into(),
        }
    }
}

/// Generates the Swagger UI HTML page.
fn swagger_ui_html(spec_url: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Swagger UI</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
  <style>
    body {{ margin: 0; }}
    #swagger-ui {{ max-width: 1460px; margin: 0 auto; }}
  </style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-standalone-preset.js"></script>
  <script>
    window.onload = function() {{
      SwaggerUIBundle({{
        url: "{spec_url}",
        dom_id: '#swagger-ui',
        presets: [
          SwaggerUIBundle.presets.apis,
          SwaggerUIStandalonePreset
        ],
        layout: "StandaloneLayout",
        deepLinking: true,
        showExtensions: true,
        showCommonExtensions: true
      }});
    }};
  </script>
</body>
</html>
"#
    )
}

/// Handler that serves the Swagger UI HTML page.
pub async fn swagger_ui_handler(
    _req: Request<Incoming>,
    _params: PathParams,
    state: Arc<AppState>,
) -> Response<BoxBody> {
    let config = state.get::<SwaggerUiConfig>();

    match config {
        Some(config) => {
            let html = swagger_ui_html(&config.spec_url);
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "text/html; charset=utf-8")
                .body(full(bytes::Bytes::from(html)))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(CONTENT_TYPE, "text/plain")
            .body(full(bytes::Bytes::from_static(b"Swagger UI not configured")))
            .unwrap(),
    }
}
