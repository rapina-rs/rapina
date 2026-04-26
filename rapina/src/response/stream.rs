//! Streaming response bodies: raw byte streams and Server-Sent Events.
//!
//! Both types implement [`IntoResponse`] and produce a [`BoxBody`] that
//! `compression` and `cache` middleware will skip via either the SSE
//! content-type rule or the `size_hint().exact() == None` rule.

use std::fmt::Write as _;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use futures_core::Stream;
use http::{HeaderValue, Response, StatusCode, header};
use http_body_util::{BodyExt, StreamBody};
use hyper::body::{Body, Frame, SizeHint};

use super::{BodyError, BoxBody, IntoResponse};

/// Streaming response of arbitrary chunked bytes.
///
/// Wraps any [`Stream`] of `Result<Bytes, BodyError>` so frames are written
/// to the wire as they arrive. Content-Type defaults to
/// `application/octet-stream`; override with [`StreamResponse::content_type`].
///
/// ```ignore
/// use rapina::response::StreamResponse;
/// use bytes::Bytes;
/// use futures_core::Stream;
///
/// async fn handler() -> StreamResponse<impl Stream<Item = Result<Bytes, _>>> {
///     let stream = async_stream::stream! {
///         yield Ok::<_, rapina::response::BodyError>(Bytes::from_static(b"hello "));
///         yield Ok(Bytes::from_static(b"world"));
///     };
///     StreamResponse::new(stream)
/// }
/// ```
pub struct StreamResponse<S> {
    stream: S,
    status: StatusCode,
    content_type: HeaderValue,
}

impl<S> StreamResponse<S>
where
    S: Stream<Item = Result<Bytes, BodyError>> + Send + 'static,
{
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            status: StatusCode::OK,
            content_type: HeaderValue::from_static("application/octet-stream"),
        }
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn content_type(mut self, content_type: &'static str) -> Self {
        self.content_type = HeaderValue::from_static(content_type);
        self
    }
}

impl<S> IntoResponse for StreamResponse<S>
where
    S: Stream<Item = Result<Bytes, BodyError>> + Send + 'static,
{
    fn into_response(self) -> Response<BoxBody> {
        let frames = FrameStream { inner: self.stream };
        let body = StreamBody::new(frames).boxed_unsync();
        Response::builder()
            .status(self.status)
            .header(header::CONTENT_TYPE, self.content_type)
            .body(body)
            .unwrap()
    }
}

/// Adapter: lifts `Stream<Item = Result<Bytes, E>>` to
/// `Stream<Item = Result<Frame<Bytes>, E>>` without pulling `futures-util`.
struct FrameStream<S> {
    inner: S,
}

impl<S> Stream for FrameStream<S>
where
    S: Stream<Item = Result<Bytes, BodyError>>,
{
    type Item = Result<Frame<Bytes>, BodyError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Safety: structural pin projection of the only field, never moved.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        match inner.poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Ok(b))) => Poll::Ready(Some(Ok(Frame::data(b)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
        }
    }
}

/// One Server-Sent Event.
///
/// Build with [`SseEvent::data`] and chain optional fields. Multi-line `data`
/// produces multiple `data:` lines per the EventSource spec.
#[derive(Clone, Debug, Default)]
pub struct SseEvent {
    data: String,
    event: Option<String>,
    id: Option<String>,
    retry: Option<u32>,
}

impl SseEvent {
    pub fn data(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            ..Default::default()
        }
    }

    pub fn event(mut self, name: impl Into<String>) -> Self {
        self.event = Some(name.into());
        self
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn retry(mut self, ms: u32) -> Self {
        self.retry = Some(ms);
        self
    }

    /// Encode this event in the `text/event-stream` wire format.
    ///
    /// Field order: `event`, `id`, `retry`, `data` (one line per line of input),
    /// terminated by a blank line. LF only, no CR.
    pub fn encode(&self) -> Bytes {
        let mut out = String::with_capacity(self.data.len() + 32);
        if let Some(name) = &self.event {
            out.push_str("event: ");
            out.push_str(name);
            out.push('\n');
        }
        if let Some(id) = &self.id {
            out.push_str("id: ");
            out.push_str(id);
            out.push('\n');
        }
        if let Some(retry) = self.retry {
            let _ = writeln!(out, "retry: {retry}");
        }
        for line in self.data.split('\n') {
            out.push_str("data: ");
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
        Bytes::from(out)
    }
}

const SSE_KEEP_ALIVE_DEFAULT: Duration = Duration::from_secs(15);
const SSE_KEEP_ALIVE_FRAME: &[u8] = b":\n\n";

/// Server-Sent Events response.
///
/// Wraps a [`Stream`] of `Result<SseEvent, BodyError>` and interleaves
/// keep-alive comment frames (`:\n\n`) when the user stream is idle for
/// longer than the configured interval. Defaults to 15 seconds; pass `None`
/// to [`SseResponse::keep_alive`] to disable.
///
/// Sets `Content-Type: text/event-stream`, `Cache-Control: no-cache`, and
/// `X-Accel-Buffering: no` so reverse proxies do not buffer the stream.
pub struct SseResponse<S> {
    stream: S,
    keep_alive: Option<Duration>,
}

impl<S> SseResponse<S>
where
    S: Stream<Item = Result<SseEvent, BodyError>> + Send + 'static,
{
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            keep_alive: Some(SSE_KEEP_ALIVE_DEFAULT),
        }
    }

    pub fn keep_alive(mut self, interval: Option<Duration>) -> Self {
        self.keep_alive = interval;
        self
    }
}

impl<S> IntoResponse for SseResponse<S>
where
    S: Stream<Item = Result<SseEvent, BodyError>> + Send + 'static,
{
    fn into_response(self) -> Response<BoxBody> {
        let body = SseBody {
            stream: self.stream,
            keep_alive: self.keep_alive.map(KeepAliveState::new),
        }
        .boxed_unsync();
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header("X-Accel-Buffering", "no")
            .body(body)
            .unwrap()
    }
}

struct KeepAliveState {
    interval: Duration,
    sleep: Pin<Box<tokio::time::Sleep>>,
}

impl KeepAliveState {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            sleep: Box::pin(tokio::time::sleep(interval)),
        }
    }

    fn reset(&mut self) {
        self.sleep
            .as_mut()
            .reset(tokio::time::Instant::now() + self.interval);
    }
}

/// Custom [`Body`] impl that owns the user stream and the keep-alive timer.
///
/// Polling order on each frame: try the user stream first; on `Pending`,
/// poll the keep-alive timer; if the timer fires, emit `:\n\n` and reset.
struct SseBody<S> {
    stream: S,
    keep_alive: Option<KeepAliveState>,
}

impl<S> Body for SseBody<S>
where
    S: Stream<Item = Result<SseEvent, BodyError>> + Send + 'static,
{
    type Data = Bytes;
    type Error = BodyError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BodyError>>> {
        // Safety: stream and keep_alive are never moved out; structural pin
        // projection on stream so Stream::poll_next can run.
        let this = unsafe { self.get_unchecked_mut() };
        let stream = unsafe { Pin::new_unchecked(&mut this.stream) };

        match stream.poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => {
                if let Some(ka) = this.keep_alive.as_mut() {
                    ka.reset();
                }
                Poll::Ready(Some(Ok(Frame::data(event.encode()))))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => match this.keep_alive.as_mut() {
                Some(ka) => match ka.sleep.as_mut().poll(cx) {
                    Poll::Ready(()) => {
                        ka.reset();
                        Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(
                            SSE_KEEP_ALIVE_FRAME,
                        )))))
                    }
                    Poll::Pending => Poll::Pending,
                },
                None => Poll::Pending,
            },
        }
    }

    fn size_hint(&self) -> SizeHint {
        // Streaming: no exact size. Compression and cache middleware key off
        // this to skip buffering.
        SizeHint::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_event_data_only() {
        let bytes = SseEvent::data("hello").encode();
        assert_eq!(&bytes[..], b"data: hello\n\n");
    }

    #[test]
    fn test_sse_event_multiline_data() {
        let bytes = SseEvent::data("line1\nline2").encode();
        assert_eq!(&bytes[..], b"data: line1\ndata: line2\n\n");
    }

    #[test]
    fn test_sse_event_with_event_name() {
        let bytes = SseEvent::data("payload").event("update").encode();
        assert_eq!(&bytes[..], b"event: update\ndata: payload\n\n");
    }

    #[test]
    fn test_sse_event_with_id() {
        let bytes = SseEvent::data("payload").id("42").encode();
        assert_eq!(&bytes[..], b"id: 42\ndata: payload\n\n");
    }

    #[test]
    fn test_sse_event_with_retry() {
        let bytes = SseEvent::data("payload").retry(5000).encode();
        assert_eq!(&bytes[..], b"retry: 5000\ndata: payload\n\n");
    }

    #[test]
    fn test_sse_event_full() {
        let bytes = SseEvent::data("payload")
            .event("update")
            .id("7")
            .retry(1000)
            .encode();
        assert_eq!(
            &bytes[..],
            b"event: update\nid: 7\nretry: 1000\ndata: payload\n\n"
        );
    }

    #[test]
    fn test_keep_alive_frame_format() {
        // The wire format the SSE spec defines for a no-op comment.
        assert_eq!(SSE_KEEP_ALIVE_FRAME, b":\n\n");
    }

    #[tokio::test]
    async fn test_stream_response_size_hint_is_none() {
        use async_stream_helper::stream;
        let s = stream();
        let resp = StreamResponse::new(s).into_response();
        // Streaming detection contract: compression and cache key off this.
        assert_eq!(resp.body().size_hint().exact(), None);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/octet-stream"
        );
    }

    #[tokio::test]
    async fn test_sse_response_headers_and_size_hint() {
        use async_stream_helper::sse_stream;
        let s = sse_stream();
        let resp = SseResponse::new(s).into_response();
        assert_eq!(resp.body().size_hint().exact(), None);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache"
        );
        assert_eq!(resp.headers().get("X-Accel-Buffering").unwrap(), "no");
    }

    /// Tiny helper module to build streams without async-stream as a dep.
    mod async_stream_helper {
        use super::*;
        use std::pin::Pin;
        use std::task::{Context, Poll};

        pub fn stream() -> impl Stream<Item = Result<Bytes, BodyError>> + Send + 'static {
            OneShot {
                item: Some(Ok(Bytes::from_static(b"hello"))),
            }
        }

        pub fn sse_stream() -> impl Stream<Item = Result<SseEvent, BodyError>> + Send + 'static {
            OneShot {
                item: Some(Ok(SseEvent::data("hi"))),
            }
        }

        struct OneShot<T> {
            item: Option<Result<T, BodyError>>,
        }

        impl<T: Unpin> Stream for OneShot<T> {
            type Item = Result<T, BodyError>;
            fn poll_next(
                mut self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<Self::Item>> {
                Poll::Ready(self.item.take())
            }
        }
    }
}
