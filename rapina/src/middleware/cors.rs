//! Cors
//!

use http::{HeaderValue, Method, Request, Response, StatusCode, header};
use hyper::body::Incoming;

use crate::context::RequestContext;
use crate::response::BoxBody;

use super::{BoxFuture, Middleware, Next};

#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allowed_origins: AllowedOrigins,
    pub allowed_methods: AllowedMethods,
    pub allowed_headers: AllowedHeaders,
}

impl CorsConfig {
    pub fn permissive() -> Self {
        // Allow everything for dev
        Self {
            allowed_origins: AllowedOrigins::Any,
            allowed_methods: AllowedMethods::Any,
            allowed_headers: AllowedHeaders::Any,
        }
    }

    pub fn with_origins(origins: Vec<String>) -> Self {
        // Specific origins
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

#[derive(Debug, Clone)]
pub enum AllowedHeaders {
    Any,
    List(Vec<header::HeaderName>),
}

#[derive(Debug, Clone)]
pub enum AllowedMethods {
    Any,
    List(Vec<Method>),
}

#[derive(Debug, Clone)]
pub enum AllowedOrigins {
    Any,
    Exact(Vec<String>),
}

pub struct CorsMiddleware {
    config: CorsConfig,
}

impl CorsMiddleware {
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

        // Vary header for caching
        builder = builder.header(header::VARY, "Origin");

        builder.body(BoxBody::default()).unwrap()
    }

    fn add_cors_headers(&self, response: &mut Response<BoxBody>, origin: &Option<HeaderValue>) {
        // Add CORS headers to the response
        todo!()
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
            // TODO: CORS logic

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
