//! Response types and conversion traits.
//!
//! This module defines the [`IntoResponse`] trait which allows various types
//! to be converted into HTTP responses.

use bytes::Bytes;
use http::{Response, StatusCode, header::CONTENT_TYPE};
use http_body_util::Full;

pub(crate) const APPLICATION_JSON: &str = "application/json";
pub(crate) const APPLICATION_PROBLEM_JSON: &str = "application/problem+json";
pub(crate) const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded";
#[cfg(feature = "metrics")]
pub(crate) const PROMETHEUS_TEXT_FORMAT: &str = "text/plain; version=0.0.4; charset=utf-8";
const TEXT_PLAIN_UTF8: &str = "text/plain; charset=utf-8";

/// The body type used for HTTP responses.
pub type BoxBody = Full<Bytes>;

/// Trait for types that can be converted into an HTTP response.
///
/// Implement this trait to allow your type to be returned from handlers.
/// Rapina provides implementations for common types like strings,
/// status codes, and JSON.
///
/// # Examples
///
/// ```
/// use rapina::response::{BoxBody, IntoResponse};
/// use http::Response;
///
/// struct MyResponse {
///     message: String,
/// }
///
/// impl IntoResponse for MyResponse {
///     fn into_response(self) -> Response<BoxBody> {
///         self.message.into_response()
///     }
/// }
/// ```
pub trait IntoResponse {
    /// Converts this type into an HTTP response.
    fn into_response(self) -> Response<BoxBody>;
}

impl IntoResponse for Response<BoxBody> {
    fn into_response(self) -> Response<BoxBody> {
        self
    }
}

impl IntoResponse for &str {
    fn into_response(self) -> Response<BoxBody> {
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, TEXT_PLAIN_UTF8)
            .body(Full::new(Bytes::from(self.to_owned())))
            .unwrap()
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Response<BoxBody> {
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, TEXT_PLAIN_UTF8)
            .body(Full::new(Bytes::from(self.to_owned())))
            .unwrap()
    }
}

impl IntoResponse for StatusCode {
    fn into_response(self) -> Response<BoxBody> {
        Response::builder()
            .status(self)
            .body(Full::new(Bytes::new()))
            .unwrap()
    }
}

impl IntoResponse for (StatusCode, String) {
    fn into_response(self) -> Response<BoxBody> {
        Response::builder()
            .status(self.0)
            .header(CONTENT_TYPE, TEXT_PLAIN_UTF8)
            .body(Full::new(Bytes::from(self.1)))
            .unwrap()
    }
}

impl<T: IntoResponse, E: IntoResponse> IntoResponse for std::result::Result<T, E> {
    fn into_response(self) -> Response<BoxBody> {
        match self {
            Ok(v) => v.into_response(),
            Err(e) => e.into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn test_str_into_response() {
        let response = "hello".into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"hello");
    }

    #[tokio::test]
    async fn test_string_into_response() {
        let response = "world".to_string().into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"world");
    }

    #[tokio::test]
    async fn test_status_code_into_response() {
        let response = StatusCode::NOT_FOUND.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_status_code_ok() {
        let response = StatusCode::OK.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_status_code_created() {
        let response = StatusCode::CREATED.into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_status_code_no_content() {
        let response = StatusCode::NO_CONTENT.into_response();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_tuple_into_response() {
        let response = (StatusCode::CREATED, "created".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"created");
    }

    #[tokio::test]
    async fn test_tuple_with_error_status() {
        let response = (StatusCode::BAD_REQUEST, "bad request".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_response_into_response_identity() {
        let original = Response::builder()
            .status(StatusCode::ACCEPTED)
            .body(Full::new(Bytes::from("test")))
            .unwrap();

        let response = original.into_response();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn test_result_ok_into_response() {
        let result: std::result::Result<&str, StatusCode> = Ok("success");
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"success");
    }

    #[test]
    fn test_result_err_into_response() {
        let result: std::result::Result<&str, StatusCode> = Err(StatusCode::INTERNAL_SERVER_ERROR);
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
