//! Response types and conversion traits.
//!
//! This module defines the [`IntoResponse`] trait which allows various types
//! to be converted into HTTP responses.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{Response, StatusCode, header::CONTENT_TYPE};
use http_body::Frame;
use http_body_util::Full;

pub(crate) const APPLICATION_JSON: &str = "application/json";
pub(crate) const APPLICATION_PROBLEM_JSON: &str = "application/problem+json";
pub(crate) const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded";
#[cfg(feature = "metrics")]
pub(crate) const PROMETHEUS_TEXT_FORMAT: &str = "text/plain; version=0.0.4; charset=utf-8";
const TEXT_PLAIN_UTF8: &str = "text/plain; charset=utf-8";

/// Error type for streaming response bodies.
pub type BoxBodyError = Box<dyn std::error::Error + Send + Sync>;

/// The body type used for HTTP responses.
///
/// A type-erased body that supports both buffered (`Full<Bytes>`) and streaming
/// body types behind a single trait object. Only requires `Send` (not `Sync`)
/// so that `Pin<Box<dyn Stream + Send>>` based bodies work.
pub struct BoxBody {
    inner: Pin<Box<dyn http_body::Body<Data = Bytes, Error = BoxBodyError> + Send>>,
}

impl BoxBody {
    /// Creates a new `BoxBody` from any body implementing `http_body::Body`.
    pub fn new<B>(body: B) -> Self
    where
        B: http_body::Body<Data = Bytes, Error = BoxBodyError> + Send + 'static,
    {
        Self {
            inner: Box::pin(body),
        }
    }
}

impl http_body::Body for BoxBody {
    type Data = Bytes;
    type Error = BoxBodyError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.inner.as_mut().poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

/// Wraps a `Full<Bytes>` into a `BoxBody`.
pub fn full_body(body: Full<Bytes>) -> BoxBody {
    use http_body_util::BodyExt;
    BoxBody::new(body.map_err(|never| match never {}))
}

/// Creates an empty `BoxBody`.
pub fn empty_body() -> BoxBody {
    full_body(Full::new(Bytes::new()))
}

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
            .body(full_body(Full::new(Bytes::from(self.to_owned()))))
            .unwrap()
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Response<BoxBody> {
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, TEXT_PLAIN_UTF8)
            .body(full_body(Full::new(Bytes::from(self))))
            .unwrap()
    }
}

/// Zero-copy wrapper for static string responses.
///
/// Use this instead of `&str` when returning compile-time string literals
/// from handlers to avoid heap allocation. `Bytes::from_static` is used
/// internally so the response body references the static data directly.
///
/// # Example
///
/// ```ignore
/// #[get("/health")]
/// async fn health() -> StaticStr {
///     StaticStr("ok")
/// }
/// ```
pub struct StaticStr(pub &'static str);

impl IntoResponse for StaticStr {
    fn into_response(self) -> Response<BoxBody> {
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, TEXT_PLAIN_UTF8)
            .body(full_body(Full::new(Bytes::from_static(self.0.as_bytes()))))
            .unwrap()
    }
}

impl IntoResponse for StatusCode {
    fn into_response(self) -> Response<BoxBody> {
        Response::builder().status(self).body(empty_body()).unwrap()
    }
}

impl IntoResponse for (StatusCode, String) {
    fn into_response(self) -> Response<BoxBody> {
        Response::builder()
            .status(self.0)
            .header(CONTENT_TYPE, TEXT_PLAIN_UTF8)
            .body(full_body(Full::new(Bytes::from(self.1))))
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
            .body(full_body(Full::new(Bytes::from("test"))))
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

    #[tokio::test]
    async fn test_static_str_into_response() {
        let response = StaticStr("Hello, World!").into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"Hello, World!");
    }

    #[tokio::test]
    async fn test_static_str_empty() {
        let response = StaticStr("").into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_static_str_is_zero_copy() {
        let response = StaticStr("static content").into_response();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        // Bytes::from_static produces a Bytes that references the original
        // static data without allocating. Verify the content is correct.
        assert_eq!(&body[..], b"static content");
    }
}
