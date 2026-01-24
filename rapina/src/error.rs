//! Standardized error handling for Rapina applications.
//!
//! This module provides a consistent error type that automatically converts
//! to HTTP responses with structured JSON bodies including trace IDs.
//!
//! # Domain Errors
//!
//! For type-safe domain errors, implement the [`IntoApiError`] trait:
//!
//! ```rust
//! use rapina::error::{Error, IntoApiError};
//!
//! enum UserError {
//!     NotFound(u64),
//!     EmailTaken(String),
//! }
//!
//! impl IntoApiError for UserError {
//!     fn into_api_error(self) -> Error {
//!         match self {
//!             UserError::NotFound(id) => Error::not_found(format!("user {} not found", id)),
//!             UserError::EmailTaken(email) => Error::conflict(format!("email {} taken", email)),
//!         }
//!     }
//! }
//! ```

use serde::Serialize;
use std::fmt;

use crate::response::{BoxBody, IntoResponse};
use bytes::Bytes;
use http_body_util::Full;

/// The JSON structure returned for error responses.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// The error details.
    pub error: ErrorDetail,
    /// Unique identifier for request tracing.
    pub trace_id: String,
}

/// Detailed error information in the response body.
#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    /// Machine-readable error code (e.g., "NOT_FOUND", "BAD_REQUEST").
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// The main error type for Rapina applications.
///
/// Provides convenient constructors for common HTTP error codes and
/// automatically converts to structured JSON responses.
///
/// # Examples
///
/// ```
/// use rapina::error::Error;
///
/// // Create a 404 error
/// let err = Error::not_found("user not found");
///
/// // Create an error with additional details
/// let err = Error::bad_request("validation failed")
///     .with_details(serde_json::json!({"field": "email"}));
/// ```
#[derive(Debug)]
pub struct Error {
    /// HTTP status code.
    pub status: u16,
    /// Machine-readable error code.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional error details.
    pub details: Option<serde_json::Value>,
    /// Optional trace ID for this error.
    pub trace_id: Option<String>,
}

impl Error {
    /// Creates a new error with the given status code, code, and message.
    pub fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            details: None,
            trace_id: None,
        }
    }

    /// Adds additional details to the error.
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Sets the trace ID for this error.
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Creates a 400 Bad Request error.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, "BAD_REQUEST", message)
    }

    /// Creates a 401 Unauthorized error.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(401, "UNAUTHORIZED", message)
    }

    /// Creates a 403 Forbidden error.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(403, "FORBIDDEN", message)
    }

    /// Creates a 404 Not Found error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(404, "NOT_FOUND", message)
    }

    /// Creates a 409 Conflict error.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(409, "CONFLICT", message)
    }

    /// Creates a 422 Validation Error.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(422, "VALIDATION_ERROR", message)
    }

    /// Creates a 429 Rate Limited error.
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(429, "RATE_LIMITED", message)
    }

    /// Creates a 500 Internal Server Error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(500, "INTERNAL_ERROR", message)
    }

    /// Converts this error to an ErrorResponse with the given trace ID.
    pub fn to_response(&self, trace_id: String) -> ErrorResponse {
        ErrorResponse {
            error: ErrorDetail {
                code: self.code.clone(),
                message: self.message.clone(),
                details: self.details.clone(),
            },
            trace_id,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

/// Trait for converting domain errors into API errors.
///
/// Implement this trait on your domain error types to enable automatic
/// conversion to Rapina's [`Error`] type. This allows you to use `?` operator
/// and return domain errors from handlers.
///
/// # Example
///
/// ```rust
/// use rapina::error::{Error, IntoApiError};
///
/// enum OrderError {
///     NotFound(u64),
///     AlreadyShipped,
///     InsufficientStock { product: String, available: u32 },
/// }
///
/// impl IntoApiError for OrderError {
///     fn into_api_error(self) -> Error {
///         match self {
///             OrderError::NotFound(id) => {
///                 Error::not_found(format!("order {} not found", id))
///             }
///             OrderError::AlreadyShipped => {
///                 Error::conflict("order has already been shipped")
///             }
///             OrderError::InsufficientStock { product, available } => {
///                 Error::bad_request(format!("insufficient stock for {}", product))
///                     .with_details(serde_json::json!({
///                         "product": product,
///                         "available": available
///                     }))
///             }
///         }
///     }
/// }
/// ```
pub trait IntoApiError {
    /// Converts this error into an API error.
    fn into_api_error(self) -> Error;
}

impl<T: IntoApiError> From<T> for Error {
    fn from(err: T) -> Self {
        err.into_api_error()
    }
}

/// Metadata about an error variant for OpenAPI documentation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorVariant {
    /// HTTP status code (e.g., 404, 409).
    pub status: u16,
    /// Machine-readable error code (e.g., "NOT_FOUND").
    pub code: &'static str,
    /// Human-readable description for documentation.
    pub description: &'static str,
}

/// Trait for documenting domain errors in OpenAPI.
///
/// Implement this trait alongside [`IntoApiError`] to have your domain errors
/// automatically documented in the OpenAPI specification.
///
/// # Example
///
/// ```rust
/// use rapina::error::{Error, IntoApiError, DocumentedError, ErrorVariant};
///
/// enum UserError {
///     NotFound(u64),
///     EmailTaken(String),
/// }
///
/// impl IntoApiError for UserError {
///     fn into_api_error(self) -> Error {
///         match self {
///             UserError::NotFound(id) => Error::not_found(format!("user {} not found", id)),
///             UserError::EmailTaken(email) => Error::conflict(format!("email {} taken", email)),
///         }
///     }
/// }
///
/// impl DocumentedError for UserError {
///     fn error_variants() -> Vec<ErrorVariant> {
///         vec![
///             ErrorVariant {
///                 status: 404,
///                 code: "NOT_FOUND",
///                 description: "User not found",
///             },
///             ErrorVariant {
///                 status: 409,
///                 code: "CONFLICT",
///                 description: "Email already taken",
///             },
///         ]
///     }
/// }
/// ```
pub trait DocumentedError: IntoApiError {
    /// Returns all possible error variants for OpenAPI documentation.
    fn error_variants() -> Vec<ErrorVariant>;
}

impl IntoResponse for Error {
    fn into_response(self) -> http::Response<BoxBody> {
        // Use existing trace_id or generate new one as fallback
        let trace_id = self
            .trace_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let response = self.to_response(trace_id);
        let body = serde_json::to_vec(&response).unwrap_or_default();

        http::Response::builder()
            .status(self.status)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .unwrap()
    }
}

/// A type alias for `Result<T, Error>`.
///
/// This is the standard result type used throughout Rapina handlers.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    // Test domain error for the trait tests
    #[derive(Debug)]
    enum TestUserError {
        NotFound(u64),
        EmailTaken(String),
    }

    impl IntoApiError for TestUserError {
        fn into_api_error(self) -> Error {
            match self {
                TestUserError::NotFound(id) => Error::not_found(format!("user {} not found", id)),
                TestUserError::EmailTaken(email) => {
                    Error::conflict(format!("email {} already taken", email))
                }
            }
        }
    }

    impl DocumentedError for TestUserError {
        fn error_variants() -> Vec<ErrorVariant> {
            vec![
                ErrorVariant {
                    status: 404,
                    code: "NOT_FOUND",
                    description: "User not found",
                },
                ErrorVariant {
                    status: 409,
                    code: "CONFLICT",
                    description: "Email already taken",
                },
            ]
        }
    }

    #[test]
    fn test_into_api_error_not_found() {
        let domain_err = TestUserError::NotFound(42);
        let api_err: Error = domain_err.into_api_error();

        assert_eq!(api_err.status, 404);
        assert_eq!(api_err.code, "NOT_FOUND");
        assert_eq!(api_err.message, "user 42 not found");
    }

    #[test]
    fn test_into_api_error_conflict() {
        let domain_err = TestUserError::EmailTaken("test@example.com".to_string());
        let api_err: Error = domain_err.into_api_error();

        assert_eq!(api_err.status, 409);
        assert_eq!(api_err.code, "CONFLICT");
        assert_eq!(api_err.message, "email test@example.com already taken");
    }

    #[test]
    fn test_domain_error_from_conversion() {
        let domain_err = TestUserError::NotFound(123);
        let api_err = Error::from(domain_err);

        assert_eq!(api_err.status, 404);
        assert_eq!(api_err.code, "NOT_FOUND");
    }

    #[test]
    fn test_documented_error_variants() {
        let variants = TestUserError::error_variants();

        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].status, 404);
        assert_eq!(variants[0].code, "NOT_FOUND");
        assert_eq!(variants[1].status, 409);
        assert_eq!(variants[1].code, "CONFLICT");
    }

    #[test]
    fn test_error_new() {
        let err = Error::new(500, "TEST_ERROR", "test message");
        assert_eq!(err.status, 500);
        assert_eq!(err.code, "TEST_ERROR");
        assert_eq!(err.message, "test message");
        assert!(err.details.is_none());
        assert!(err.trace_id.is_none());
    }

    #[test]
    fn test_error_bad_request() {
        let err = Error::bad_request("invalid input");
        assert_eq!(err.status, 400);
        assert_eq!(err.code, "BAD_REQUEST");
        assert_eq!(err.message, "invalid input");
    }

    #[test]
    fn test_error_unauthorized() {
        let err = Error::unauthorized("not authenticated");
        assert_eq!(err.status, 401);
        assert_eq!(err.code, "UNAUTHORIZED");
    }

    #[test]
    fn test_error_forbidden() {
        let err = Error::forbidden("access denied");
        assert_eq!(err.status, 403);
        assert_eq!(err.code, "FORBIDDEN");
    }

    #[test]
    fn test_error_not_found() {
        let err = Error::not_found("resource not found");
        assert_eq!(err.status, 404);
        assert_eq!(err.code, "NOT_FOUND");
    }

    #[test]
    fn test_error_conflict() {
        let err = Error::conflict("already exists");
        assert_eq!(err.status, 409);
        assert_eq!(err.code, "CONFLICT");
    }

    #[test]
    fn test_error_validation() {
        let err = Error::validation("invalid data");
        assert_eq!(err.status, 422);
        assert_eq!(err.code, "VALIDATION_ERROR");
    }

    #[test]
    fn test_error_rate_limited() {
        let err = Error::rate_limited("too many requests");
        assert_eq!(err.status, 429);
        assert_eq!(err.code, "RATE_LIMITED");
    }

    #[test]
    fn test_error_internal() {
        let err = Error::internal("server error");
        assert_eq!(err.status, 500);
        assert_eq!(err.code, "INTERNAL_ERROR");
    }

    #[test]
    fn test_error_with_details() {
        let details = serde_json::json!({"field": "email", "error": "invalid format"});
        let err = Error::bad_request("validation failed").with_details(details.clone());
        assert_eq!(err.details, Some(details));
    }

    #[test]
    fn test_error_with_trace_id() {
        let err = Error::bad_request("test").with_trace_id("trace-123");
        assert_eq!(err.trace_id, Some("trace-123".to_string()));
    }

    #[test]
    fn test_error_display() {
        let err = Error::bad_request("invalid input");
        let display = format!("{}", err);
        assert_eq!(display, "BAD_REQUEST: invalid input");
    }

    #[test]
    fn test_error_to_response() {
        let err = Error::not_found("user not found");
        let response = err.to_response("trace-abc".to_string());
        assert_eq!(response.trace_id, "trace-abc");
        assert_eq!(response.error.code, "NOT_FOUND");
        assert_eq!(response.error.message, "user not found");
    }

    #[tokio::test]
    async fn test_error_into_response() {
        let err = Error::bad_request("test error").with_trace_id("my-trace");
        let response = err.into_response();

        assert_eq!(response.status(), 400);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["error"]["code"], "BAD_REQUEST");
        assert_eq!(json["error"]["message"], "test error");
        assert_eq!(json["trace_id"], "my-trace");
    }

    #[tokio::test]
    async fn test_error_into_response_generates_trace_id() {
        let err = Error::internal("error");
        let response = err.into_response();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Should have a generated UUID trace_id
        let trace_id = json["trace_id"].as_str().unwrap();
        assert_eq!(trace_id.len(), 36); // UUID format
    }

    #[test]
    fn test_error_response_skips_none_details() {
        let err = Error::bad_request("test");
        let response = err.to_response("trace".to_string());
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("details"));
    }

    #[test]
    fn test_error_response_includes_details() {
        let details = serde_json::json!({"key": "value"});
        let err = Error::bad_request("test").with_details(details);
        let response = err.to_response("trace".to_string());
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("details"));
        assert!(json.contains("key"));
    }

    #[test]
    fn test_error_is_std_error() {
        let err = Error::internal("test");
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_error_builder_chain() {
        let details = serde_json::json!({"field": "name"});
        let err = Error::validation("invalid")
            .with_details(details.clone())
            .with_trace_id("trace-123");

        assert_eq!(err.status, 422);
        assert_eq!(err.code, "VALIDATION_ERROR");
        assert_eq!(err.details, Some(details));
        assert_eq!(err.trace_id, Some("trace-123".to_string()));
    }
}
