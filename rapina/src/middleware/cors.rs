//! Cors
//!

use http::{Method, Request, Response, header};
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
            next.run(req).await
        })
    }
}
