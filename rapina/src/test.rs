//! Test utilities for Rapina framework
//!
//! This module provides helpers for testing Rapina applications.

use bytes::Bytes;
use http::Request;
use serde::Serialize;
use std::sync::Arc;

use crate::context::RequestContext;
use crate::extract::PathParams;
use crate::response::{APPLICATION_JSON, FORM_CONTENT_TYPE};
use crate::state::AppState;

/// A test request builder for creating mock HTTP requests
pub struct TestRequest {
    method: http::Method,
    uri: String,
    headers: http::HeaderMap,
    body: Bytes,
}

impl TestRequest {
    /// Create a new GET request
    pub fn get(uri: &str) -> Self {
        Self {
            method: http::Method::GET,
            uri: uri.to_string(),
            headers: http::HeaderMap::new(),
            body: Bytes::new(),
        }
    }

    /// Create a new POST request
    pub fn post(uri: &str) -> Self {
        Self {
            method: http::Method::POST,
            uri: uri.to_string(),
            headers: http::HeaderMap::new(),
            body: Bytes::new(),
        }
    }

    /// Create a new PUT request
    pub fn put(uri: &str) -> Self {
        Self {
            method: http::Method::PUT,
            uri: uri.to_string(),
            headers: http::HeaderMap::new(),
            body: Bytes::new(),
        }
    }

    /// Create a new DELETE request
    pub fn delete(uri: &str) -> Self {
        Self {
            method: http::Method::DELETE,
            uri: uri.to_string(),
            headers: http::HeaderMap::new(),
            body: Bytes::new(),
        }
    }

    /// Add a header to the request
    pub fn header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(
            http::header::HeaderName::from_bytes(key.as_bytes()).unwrap(),
            http::header::HeaderValue::from_str(value).unwrap(),
        );
        self
    }

    /// Set a JSON body on the request
    pub fn json<T: Serialize>(mut self, body: &T) -> Self {
        self.body = Bytes::from(serde_json::to_vec(body).unwrap());
        self.headers.insert(
            http::header::CONTENT_TYPE,
            http::header::HeaderValue::from_static(APPLICATION_JSON),
        );
        self
    }

    /// Set a form body on the request
    pub fn form<T: Serialize>(mut self, body: &T) -> Self {
        self.body = Bytes::from(serde_urlencoded::to_string(body).unwrap());
        self.headers.insert(
            http::header::CONTENT_TYPE,
            http::header::HeaderValue::from_static(FORM_CONTENT_TYPE),
        );
        self
    }

    /// Set raw body bytes
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// Build the request into http::request::Parts and body bytes
    /// This is useful for testing extractors that use FromRequestParts
    pub fn into_parts(self) -> (http::request::Parts, Bytes) {
        let mut builder = Request::builder().method(self.method).uri(self.uri);

        for (key, value) in self.headers.iter() {
            builder = builder.header(key, value);
        }

        let request: Request<()> = builder.body(()).unwrap();
        let (mut parts, _) = request.into_parts();

        // Inject RequestContext into extensions
        parts.extensions.insert(RequestContext::new());

        (parts, self.body)
    }

    /// Build request parts with a custom RequestContext
    pub fn into_parts_with_context(self, ctx: RequestContext) -> (http::request::Parts, Bytes) {
        let mut builder = Request::builder().method(self.method).uri(self.uri);

        for (key, value) in self.headers.iter() {
            builder = builder.header(key, value);
        }

        let request: Request<()> = builder.body(()).unwrap();
        let (mut parts, _) = request.into_parts();

        parts.extensions.insert(ctx);

        (parts, self.body)
    }

    /// Get the body bytes
    pub fn get_body(&self) -> &Bytes {
        &self.body
    }
}

/// Helper to create empty path params
pub fn empty_params() -> PathParams {
    PathParams::new()
}

/// Helper to create path params from key-value pairs
pub fn params(pairs: &[(&str, &str)]) -> PathParams {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Helper to create an empty AppState
pub fn empty_state() -> Arc<AppState> {
    Arc::new(AppState::new())
}

/// Helper to create an AppState with a value
pub fn state_with<T: Send + Sync + 'static>(value: T) -> Arc<AppState> {
    Arc::new(AppState::new().with(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_builder_get() {
        let (parts, body) = TestRequest::get("/users").into_parts();
        assert_eq!(parts.method, http::Method::GET);
        assert_eq!(parts.uri.path(), "/users");
        assert!(body.is_empty());
    }

    #[test]
    fn test_request_builder_post_with_json() {
        #[derive(Serialize)]
        struct Data {
            name: String,
        }

        let (parts, body) = TestRequest::post("/users")
            .json(&Data {
                name: "test".to_string(),
            })
            .into_parts();

        assert_eq!(parts.method, http::Method::POST);
        assert_eq!(
            parts.headers.get("content-type").unwrap(),
            "application/json"
        );
        assert!(!body.is_empty());
    }

    #[test]
    fn test_request_builder_with_headers() {
        let (parts, _) = TestRequest::get("/")
            .header("x-custom", "value")
            .header("authorization", "Bearer token")
            .into_parts();

        assert_eq!(parts.headers.get("x-custom").unwrap(), "value");
        assert_eq!(parts.headers.get("authorization").unwrap(), "Bearer token");
    }

    #[test]
    fn test_request_builder_form() {
        #[derive(Serialize)]
        struct Form {
            username: String,
            password: String,
        }

        let (parts, body) = TestRequest::post("/login")
            .form(&Form {
                username: "user".to_string(),
                password: "pass".to_string(),
            })
            .into_parts();

        assert_eq!(
            parts.headers.get("content-type").unwrap(),
            "application/x-www-form-urlencoded"
        );
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("username=user"));
        assert!(body_str.contains("password=pass"));
    }

    #[test]
    fn test_params_helper() {
        let p = params(&[("id", "123"), ("name", "test")]);
        assert_eq!(p.get("id"), Some(&"123".to_string()));
        assert_eq!(p.get("name"), Some(&"test".to_string()));
    }

    #[test]
    fn test_empty_params() {
        let p = empty_params();
        assert!(p.is_empty());
    }

    #[test]
    fn test_request_has_context() {
        let (parts, _) = TestRequest::get("/").into_parts();
        let ctx = parts.extensions.get::<RequestContext>();
        assert!(ctx.is_some());
        assert!(!ctx.unwrap().trace_id.is_empty());
    }

    #[test]
    fn test_request_with_custom_context() {
        let custom_ctx = RequestContext::with_trace_id("custom-trace-123".to_string());
        let (parts, _) = TestRequest::get("/").into_parts_with_context(custom_ctx);

        let ctx = parts.extensions.get::<RequestContext>().unwrap();
        assert_eq!(ctx.trace_id, "custom-trace-123");
    }

    #[test]
    fn test_state_with_helper() {
        #[derive(Clone)]
        struct Config {
            name: String,
        }

        let state = state_with(Config {
            name: "test".to_string(),
        });
        let config = state.get::<Config>().unwrap();
        assert_eq!(config.name, "test");
    }
}
