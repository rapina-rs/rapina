//! Streaming response types for chunked transfer and Server-Sent Events.
//!
//! This module provides [`StreamResponse`] for raw chunked byte streams and
//! [`SseResponse`] for Server-Sent Events with proper `text/event-stream`
//! content type, keep-alive, and `data:` / `event:` / `id:` framing.
//!
//! # Server-Sent Events
//!
//! ```rust,ignore
//! use rapina::streaming::{SseResponse, SseEvent};
//! use futures_util::stream;
//!
//! #[get("/events")]
//! async fn events() -> SseResponse {
//!     let events = stream::iter(vec![
//!         Ok(SseEvent::new().data("hello")),
//!         Ok(SseEvent::new().event("update").data("world")),
//!     ]);
//!     SseResponse::new(events)
//! }
//! ```
//!
//! # Chunked Streams
//!
//! ```rust,ignore
//! use rapina::streaming::StreamResponse;
//! use futures_util::stream;
//!
//! #[get("/download")]
//! async fn download() -> StreamResponse {
//!     let chunks = stream::iter(vec![
//!         Ok(bytes::Bytes::from("chunk 1")),
//!         Ok(bytes::Bytes::from("chunk 2")),
//!     ]);
//!     StreamResponse::new(chunks)
//! }
//! ```

use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;
use http::{Response, StatusCode, header};
use http_body::Frame;
use http_body_util::StreamBody;

use crate::response::{BoxBody, BoxBodyError, IntoResponse};

/// A streaming HTTP response.
///
/// Wraps any `Stream<Item = Result<Bytes, BoxBodyError>>` and sends it
/// as a chunked transfer-encoded response.
pub struct StreamResponse {
    status: StatusCode,
    headers: Vec<(http::header::HeaderName, http::header::HeaderValue)>,
    stream: Pin<Box<dyn Stream<Item = Result<Frame<Bytes>, BoxBodyError>> + Send>>,
}

impl StreamResponse {
    /// Creates a new streaming response from a stream of byte chunks.
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, BoxBodyError>> + Send + 'static,
    {
        use futures_util::StreamExt;
        Self {
            status: StatusCode::OK,
            headers: Vec::new(),
            stream: Box::pin(stream.map(|result| result.map(Frame::data))),
        }
    }

    /// Sets the HTTP status code.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Adds a response header.
    pub fn header(
        mut self,
        name: http::header::HeaderName,
        value: http::header::HeaderValue,
    ) -> Self {
        self.headers.push((name, value));
        self
    }

    /// Sets the Content-Type header.
    pub fn content_type(self, content_type: &'static str) -> Self {
        self.header(
            header::CONTENT_TYPE,
            http::header::HeaderValue::from_static(content_type),
        )
    }
}

/// Marks a response as streaming so middleware can detect it.
#[derive(Clone, Copy, Debug)]
pub struct StreamingMarker;

impl IntoResponse for StreamResponse {
    fn into_response(self) -> Response<BoxBody> {
        use http_body_util::BodyExt;

        let body = StreamBody::new(self.stream);
        let body = BoxBody::new(body.map_err(|e| -> BoxBodyError { e }));

        let mut response = Response::builder()
            .status(self.status)
            .body(body)
            .expect("streaming response builder should not fail");

        for (name, value) in self.headers {
            response.headers_mut().insert(name, value);
        }

        // Mark as streaming for middleware
        response.extensions_mut().insert(StreamingMarker);
        response
    }
}

/// A Server-Sent Events response.
///
/// Produces a `text/event-stream` response with proper SSE framing.
pub struct SseResponse {
    stream: Pin<Box<dyn Stream<Item = Result<SseEvent, BoxBodyError>> + Send>>,
    keep_alive: Option<std::time::Duration>,
}

impl SseResponse {
    /// Creates a new SSE response from a stream of events.
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<SseEvent, BoxBodyError>> + Send + 'static,
    {
        Self {
            stream: Box::pin(stream),
            keep_alive: None,
        }
    }

    /// Enables keep-alive comments at the given interval.
    ///
    /// Sends a `: keep-alive\n\n` comment periodically to prevent
    /// proxy/load-balancer timeouts.
    pub fn keep_alive(mut self, interval: std::time::Duration) -> Self {
        self.keep_alive = Some(interval);
        self
    }
}

impl IntoResponse for SseResponse {
    fn into_response(self) -> Response<BoxBody> {
        use futures_util::StreamExt;
        use http_body_util::BodyExt;

        let event_stream = self
            .stream
            .map(|result| result.map(|event| Frame::data(event.into_bytes())));

        let stream: Pin<Box<dyn Stream<Item = Result<Frame<Bytes>, BoxBodyError>> + Send>> =
            if let Some(interval) = self.keep_alive {
                // Use unfold to interleave keep-alive comments between events.
                // Unlike stream::select, this terminates when the event stream ends.
                let merged = futures_util::stream::unfold(
                    (event_stream.fuse(), interval),
                    |(mut events, interval)| async move {
                        loop {
                            let sleep = tokio::time::sleep(interval);
                            tokio::pin!(sleep);
                            tokio::select! {
                                biased;
                                item = futures_util::StreamExt::next(&mut events) => {
                                    return item.map(|result| (result, (events, interval)));
                                }
                                _ = &mut sleep => {
                                    let comment = Bytes::from_static(b": keep-alive\n\n");
                                    return Some((Ok(Frame::data(comment)), (events, interval)));
                                }
                            }
                        }
                    },
                );
                Box::pin(merged)
            } else {
                Box::pin(event_stream)
            };

        let body = StreamBody::new(stream);
        let body = BoxBody::new(body.map_err(|e| -> BoxBodyError { e }));

        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .body(body)
            .expect("SSE response builder should not fail");

        response.extensions_mut().insert(StreamingMarker);
        response
    }
}

/// A single Server-Sent Event.
#[derive(Debug, Clone, Default)]
pub struct SseEvent {
    id: Option<String>,
    event: Option<String>,
    data: Option<String>,
    retry: Option<u64>,
}

impl SseEvent {
    /// Creates a new empty SSE event.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the event ID.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the event type.
    pub fn event(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }

    /// Sets the event data.
    pub fn data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }

    /// Sets the retry interval in milliseconds.
    pub fn retry(mut self, ms: u64) -> Self {
        self.retry = Some(ms);
        self
    }

    /// Sets the data field to the JSON serialization of `value`.
    pub fn json_data<T: serde::Serialize>(self, value: &T) -> Result<Self, serde_json::Error> {
        let json = serde_json::to_string(value)?;
        Ok(self.data(json))
    }

    /// Serializes the event into the SSE wire format.
    fn into_bytes(self) -> Bytes {
        let mut buf = String::new();
        if let Some(id) = self.id {
            buf.push_str("id: ");
            buf.push_str(&id);
            buf.push('\n');
        }
        if let Some(event) = self.event {
            buf.push_str("event: ");
            buf.push_str(&event);
            buf.push('\n');
        }
        if let Some(data) = self.data {
            for line in data.lines() {
                buf.push_str("data: ");
                buf.push_str(line);
                buf.push('\n');
            }
        }
        if let Some(retry) = self.retry {
            buf.push_str("retry: ");
            buf.push_str(&retry.to_string());
            buf.push('\n');
        }
        buf.push('\n'); // End of event
        Bytes::from(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_event_data_only() {
        let event = SseEvent::new().data("hello");
        let bytes = event.into_bytes();
        assert_eq!(bytes, Bytes::from("data: hello\n\n"));
    }

    #[test]
    fn test_sse_event_all_fields() {
        let event = SseEvent::new()
            .id("42")
            .event("message")
            .data("hello world")
            .retry(5000);
        let bytes = event.into_bytes();
        assert_eq!(
            bytes,
            Bytes::from("id: 42\nevent: message\ndata: hello world\nretry: 5000\n\n")
        );
    }

    #[test]
    fn test_sse_event_multiline_data() {
        let event = SseEvent::new().data("line1\nline2\nline3");
        let bytes = event.into_bytes();
        assert_eq!(
            bytes,
            Bytes::from("data: line1\ndata: line2\ndata: line3\n\n")
        );
    }

    #[test]
    fn test_sse_event_empty() {
        let event = SseEvent::new();
        let bytes = event.into_bytes();
        assert_eq!(bytes, Bytes::from("\n"));
    }

    #[test]
    fn test_sse_event_id_only() {
        let event = SseEvent::new().id("123");
        let bytes = event.into_bytes();
        assert_eq!(bytes, Bytes::from("id: 123\n\n"));
    }

    #[test]
    fn test_sse_event_event_only() {
        let event = SseEvent::new().event("ping");
        let bytes = event.into_bytes();
        assert_eq!(bytes, Bytes::from("event: ping\n\n"));
    }

    #[test]
    fn test_sse_event_retry_only() {
        let event = SseEvent::new().retry(3000);
        let bytes = event.into_bytes();
        assert_eq!(bytes, Bytes::from("retry: 3000\n\n"));
    }

    #[test]
    fn test_sse_event_json_data() {
        #[derive(serde::Serialize)]
        struct Msg {
            text: String,
        }
        let msg = Msg {
            text: "hello".to_string(),
        };
        let event = SseEvent::new().json_data(&msg).unwrap();
        let bytes = event.into_bytes();
        assert_eq!(bytes, Bytes::from("data: {\"text\":\"hello\"}\n\n"));
    }

    #[test]
    fn test_stream_response_into_response_has_streaming_marker() {
        let stream = futures_util::stream::iter(vec![Ok(Bytes::from("chunk"))]);
        let response = StreamResponse::new(stream).into_response();
        assert!(response.extensions().get::<StreamingMarker>().is_some());
    }

    #[test]
    fn test_stream_response_custom_status() {
        let stream = futures_util::stream::iter(vec![Ok(Bytes::from("data"))]);
        let response = StreamResponse::new(stream)
            .status(StatusCode::PARTIAL_CONTENT)
            .into_response();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    }

    #[test]
    fn test_stream_response_custom_headers() {
        let stream = futures_util::stream::iter(vec![Ok(Bytes::from("data"))]);
        let response = StreamResponse::new(stream)
            .content_type("application/octet-stream")
            .into_response();
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_sse_response_into_response_headers() {
        let stream =
            futures_util::stream::iter(vec![Ok::<_, BoxBodyError>(SseEvent::new().data("hi"))]);
        let response = SseResponse::new(stream).into_response();

        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache"
        );
        assert_eq!(
            response.headers().get(header::CONNECTION).unwrap(),
            "keep-alive"
        );
        assert!(response.extensions().get::<StreamingMarker>().is_some());
    }

    #[tokio::test]
    async fn test_stream_response_body_content() {
        use http_body_util::BodyExt;

        let stream =
            futures_util::stream::iter(vec![Ok(Bytes::from("hello ")), Ok(Bytes::from("world"))]);
        let response = StreamResponse::new(stream).into_response();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, Bytes::from("hello world"));
    }

    #[tokio::test]
    async fn test_sse_response_body_content() {
        use http_body_util::BodyExt;

        let stream = futures_util::stream::iter(vec![
            Ok::<_, BoxBodyError>(SseEvent::new().data("first")),
            Ok(SseEvent::new().event("update").data("second")),
        ]);
        let response = SseResponse::new(stream).into_response();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(text, "data: first\n\nevent: update\ndata: second\n\n");
    }
}
