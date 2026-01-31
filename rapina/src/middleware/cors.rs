//! CORS (Cross-Origin Resource Sharing) middleware.
//!
//! Provides configurable CORS support for Rapina applications,
//! handling preflight OPTIONS requests and adding appropriate headers.

use http::{HeaderValue, Method, Request, Response, StatusCode, header};
use hyper::body::Incoming;

use crate::context::RequestContext;
use crate::response::BoxBody;

use super::{BoxFuture, Middleware, Next};

/// Configuration for CORS middleware.
///
/// Use `permissive()` for development or `with_origins()` for production.
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// Allowed origins for CORS requests.
    pub allowed_origins: AllowedOrigins,
    /// Allowed HTTP methods.
    pub allowed_methods: AllowedMethods,
    /// Allowed request headers.
    pub allowed_headers: AllowedHeaders,
}

impl CorsConfig {
    /// Creates a permissive CORS config that allows all origins, methods, and headers.
    ///
    /// Suitable for development. Do not use in production.
    pub fn permissive() -> Self {
        Self {
            allowed_origins: AllowedOrigins::Any,
            allowed_methods: AllowedMethods::Any,
            allowed_headers: AllowedHeaders::Any,
        }
    }

    /// Creates a CORS config with specific allowed origins.
    ///
    /// Uses sensible defaults for methods (GET, POST, PUT, PATCH, DELETE, OPTIONS)
    /// and headers (Accept, Authorization).
    pub fn with_origins(origins: Vec<String>) -> Self {
        Self {
            allowed_methods: AllowedMethods::List(vec![
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ]),
            allowed_origins: AllowedOrigins::Exact(origins),
            allowed_headers: AllowedHeaders::List(vec![header::ACCEPT, header::AUTHORIZATION]),
        }
    }
}

/// Specifies which headers are allowed in CORS requests.
#[derive(Debug, Clone)]
pub enum AllowedHeaders {
    /// Allow any headers.
    Any,
    /// Allow only specific headers.
    List(Vec<header::HeaderName>),
}

/// Specifies which HTTP methods are allowed in CORS requests.
#[derive(Debug, Clone)]
pub enum AllowedMethods {
    /// Allow any method.
    Any,
    /// Allow only specific methods.
    List(Vec<Method>),
}

/// Specifies which origins are allowed for CORS requests.
#[derive(Debug, Clone)]
pub enum AllowedOrigins {
    /// Allow any origin (`*`).
    Any,
    /// Allow only specific origins.
    Exact(Vec<String>),
}

/// Middleware that handles CORS headers and preflight requests.
#[derive(Debug, Clone)]
pub struct CorsMiddleware {
    config: CorsConfig,
}

impl CorsMiddleware {
    /// Creates a new CORS middleware with the given configuration.
    pub fn new(config: CorsConfig) -> Self {
        Self { config }
    }

    fn preflight_response(&self, origin: &Option<HeaderValue>) -> Response<BoxBody> {
        let mut builder = Response::builder().status(StatusCode::NO_CONTENT);

        // Set Access-Control-Allow-Origin
        match &self.config.allowed_origins {
            AllowedOrigins::Any => {
                builder = builder.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
            }
            AllowedOrigins::Exact(origins) => {
                if let Some(req_origin) = origin {
                    let origin_str = req_origin.to_str().unwrap_or("");
                    if origins.iter().any(|o| o == origin_str) {
                        builder =
                            builder.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, req_origin.clone());
                    }
                }
            }
        }

        // Set Access-Control-Allow-Methods
        let methods_value = match &self.config.allowed_methods {
            AllowedMethods::Any => "*".to_string(),
            AllowedMethods::List(methods) => methods
                .iter()
                .map(|m| m.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        };
        builder = builder.header(header::ACCESS_CONTROL_ALLOW_METHODS, methods_value);

        // Set Access-Control-Allow-Headers
        let headers_value = match &self.config.allowed_headers {
            AllowedHeaders::Any => "*".to_string(),
            AllowedHeaders::List(headers) => headers
                .iter()
                .map(|h| h.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        };
        builder = builder.header(header::ACCESS_CONTROL_ALLOW_HEADERS, headers_value);

        builder = builder.header(header::VARY, "Origin");

        builder.body(BoxBody::default()).unwrap()
    }

    fn add_cors_headers(&self, response: &mut Response<BoxBody>, origin: &Option<HeaderValue>) {
        let headers = response.headers_mut();

        // Set Access-Control-Allow-Origin
        match &self.config.allowed_origins {
            AllowedOrigins::Any => {
                headers.insert(
                    header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    HeaderValue::from_static("*"),
                );
            }
            AllowedOrigins::Exact(origins) => {
                if let Some(req_origin) = origin {
                    let origin_str = req_origin.to_str().unwrap_or("");
                    if origins.iter().any(|o| o == origin_str) {
                        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, req_origin.clone());
                    }
                }
            }
        }

        // Vary header
        headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
}

impl Middleware for CorsMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let origin = req.headers().get(header::ORIGIN).cloned();

            // if it's OPTIONS (preflight), return early with 204 + CORS headers
            if req.method() == Method::OPTIONS {
                return self.preflight_response(&origin);
            }

            let mut response = next.run(req).await;
            self.add_cors_headers(&mut response, &origin);
            response
        })
    }
}
