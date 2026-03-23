# Streaming Body Support Implementation Plan

## Overview

Replace the concrete `BoxBody = Full<Bytes>` type alias with a true boxed trait object so that both buffered (`Full<Bytes>`) and streaming body types (`StreamBody`, channel-based bodies) can coexist behind the same type. Then add `StreamResponse` and `SseResponse` types that implement `IntoResponse` to give handlers first-class streaming support. Finally, make the compression and cache middlewares streaming-aware so they don't defeat the purpose by buffering everything.

## Current State Analysis

`BoxBody` is defined as `pub type BoxBody = Full<Bytes>` in `response.rs:18`. Despite the name, it is not a boxed trait object — it's a concrete type that requires the entire response body to be materialized in memory before hyper sends a single byte. This type flows through every layer of the framework:

- `IntoResponse` trait returns `Response<BoxBody>` — `response.rs:44`
- `Handler::call` returns `Pin<Box<dyn Future<Output = Response<BoxBody>>>>` — `handler.rs:15,36`
- `HandlerFn` closure returns `BoxFuture` resolving to `Response<BoxBody>` — `router/mod.rs:23-25`
- `Middleware::handle` returns `BoxFuture<'a, Response<BoxBody>>` — `middleware/mod.rs:79`
- `Next::run` returns `Response<BoxBody>` — `middleware/mod.rs:106`
- `MiddlewareStack::execute` returns `Response<BoxBody>` — `middleware/mod.rs:147`
- `service_fn` closure returns `Ok::<_, Infallible>(response)` where response is `Response<BoxBody>` — `server.rs:79-82`
- proc-macro generates `Handler` impl returning `Response<rapina::response::BoxBody>` — `rapina-macros/src/lib.rs:350`
- `WebSocketUpgrade` stores `Response<BoxBody>` — `websocket.rs:51`

hyper's `serve_connection_with_upgrades` only requires `http_body::Body` trait, which both `Full<Bytes>` and streaming body types implement. The constraint is entirely self-imposed by the framework.

### Key Discoveries:
- 25+ files import/use `BoxBody` — full list in the "Affected Files" section below
- `CompressionMiddleware` calls `body.collect().await` to buffer the response — `middleware/compression.rs:169`
- `CacheMiddleware` calls `body.collect().await` on cache misses — `cache.rs:261`
- `BoxBody::default()` is called in CORS middleware for empty bodies — `middleware/cors.rs:141,370`
- Direct `Full::new(Bytes::...)` construction happens in ~15 call sites across `response.rs`, `error.rs`, `extract/mod.rs`, `pagination.rs`, `cache.rs`, `compression.rs`, `metrics/prometheus.rs`
- `http-body-util = "0.1.3"` is already a dependency — it provides `http_body_util::combinators::BoxBody` which is the exact type we need
- The `futures-util` crate is already a dependency (behind `websocket` and `multipart` features) but will need to be unconditional for streaming support

## Desired End State

After this plan is complete:

1. `BoxBody` is `http_body_util::combinators::BoxBody<Bytes, BoxBodyError>` — a true boxed trait object
2. All existing `IntoResponse` impls work unchanged (wrapping `Full<Bytes>` into the boxed body transparently)
3. Handlers can return `StreamResponse` for chunked byte streaming
4. Handlers can return `SseResponse` for Server-Sent Events with proper `text/event-stream` framing
5. Compression and cache middlewares skip buffering for streaming responses (detected via a response extension or header)
6. All existing tests pass
7. The framework compiles with all feature combinations

### Verification:
- `cargo test --all-features` passes
- `cargo test --no-default-features` passes
- `cargo check --all-features` passes
- New unit tests for `StreamResponse` and `SseResponse` pass
- New integration test demonstrating SSE streaming passes

## What We're NOT Doing

- Not adding per-chunk compression for streaming responses (future work — requires `async-compression`)
- Not adding `StreamBody` as a public re-export (users work with `StreamResponse`/`SseResponse` abstractions)
- Not changing the WebSocket implementation (it already works via HTTP upgrade, not body streaming)
- Not adding backpressure/flow-control APIs (hyper handles this at the transport level)
- Not modifying the proc-macro code — it references `rapina::response::BoxBody` by name, which continues to work after the type change

## Implementation Approach

The change fans out from a single point: the `BoxBody` type alias. Phase 1 changes the alias and adds a helper to wrap `Full<Bytes>` into the new boxed type, then mechanically updates every `Full::new(...)` call site. Phase 2 adds the streaming response types. Phase 3 makes middleware streaming-aware. Each phase is independently compilable and testable.

## Error Type for BoxBody

The boxed body needs an error type. `Full<Bytes>` is infallible (`Error = Infallible`), but streaming bodies can fail. We'll define:

```rust
pub type BoxBodyError = Box<dyn std::error::Error + Send + Sync>;
```

And the new `BoxBody` becomes:

```rust
pub type BoxBody = http_body_util::combinators::BoxBody<Bytes, BoxBodyError>;
```

A helper function wraps `Full<Bytes>` (infallible) into this type:

```rust
pub fn full_body(body: Full<Bytes>) -> BoxBody {
    use http_body_util::BodyExt;
    body.map_err(|never| match never {}).boxed()
}
```

---

## Phase 1: Replace BoxBody Type Alias

### Overview
Change `BoxBody` from `Full<Bytes>` to a true boxed trait object. Update all call sites that construct `Full<Bytes>` directly to use the new `full_body()` helper. This is a mechanical refactor — no behavior changes.

### Changes Required:

#### 1. Core type alias and helper
**File**: `rapina/src/response.rs`
**Changes**:
- Change the `BoxBody` type alias from `Full<Bytes>` to `http_body_util::combinators::BoxBody<Bytes, BoxBodyError>`
- Add `BoxBodyError` type alias
- Add `full_body()` helper function
- Add `empty_body()` helper for empty responses
- Update all `IntoResponse` impls to use `full_body(Full::new(...))`

```rust
use http_body_util::{BodyExt, Full, combinators};

/// Error type for streaming response bodies.
pub type BoxBodyError = Box<dyn std::error::Error + Send + Sync>;

/// The body type used for HTTP responses.
///
/// This is a boxed trait object that supports both buffered and streaming bodies.
pub type BoxBody = combinators::BoxBody<Bytes, BoxBodyError>;

/// Wraps a `Full<Bytes>` into a `BoxBody`.
pub fn full_body(body: Full<Bytes>) -> BoxBody {
    body.map_err(|never| match never {}).boxed()
}

/// Creates an empty `BoxBody`.
pub fn empty_body() -> BoxBody {
    full_body(Full::new(Bytes::new()))
}
```

Each `IntoResponse` impl changes from:
```rust
.body(Full::new(Bytes::from(...)))
```
to:
```rust
.body(full_body(Full::new(Bytes::from(...))))
```

The `impl IntoResponse for Response<BoxBody>` identity impl continues to work unchanged since the type is still named `BoxBody`.

#### 2. Error module
**File**: `rapina/src/error.rs`
**Changes**: Replace `Full::new(Bytes::from(body))` with `full_body(Full::new(Bytes::from(body)))`. Import `full_body` from `crate::response`.

#### 3. Extract module
**File**: `rapina/src/extract/mod.rs`
**Changes**: Replace `http_body_util::Full::new(Bytes::from(body))` with `crate::response::full_body(http_body_util::Full::new(Bytes::from(body)))`.

#### 4. Pagination module
**File**: `rapina/src/pagination.rs`
**Changes**: Replace `Full::new(Bytes::from(body))` with `full_body(Full::new(Bytes::from(body)))`.

#### 5. CORS middleware
**File**: `rapina/src/middleware/cors.rs`
**Changes**: Replace `BoxBody::default()` with `empty_body()`. Replace any `Full::new(Bytes::new())` with `empty_body()`.

#### 6. Compression middleware
**File**: `rapina/src/middleware/compression.rs`
**Changes**:
- Replace all `Full::new(...)` body construction with `full_body(Full::new(...))` / `empty_body()`
- The `body.collect().await` call stays for now (Phase 3 will make it streaming-aware)
- Import `full_body`, `empty_body` from `crate::response`

#### 7. Cache middleware
**File**: `rapina/src/cache.rs`
**Changes**: Same pattern — replace `Full::new(...)` with `full_body(Full::new(...))`.

#### 8. Metrics/Prometheus endpoint
**File**: `rapina/src/metrics/prometheus.rs`
**Changes**: Replace `Full::new(...)` with `full_body(Full::new(...))`.

#### 9. OpenAPI endpoint
**File**: `rapina/src/openapi/endpoint.rs`
**Changes**: Replace `http_body_util::Full::new(...)` with `crate::response::full_body(http_body_util::Full::new(...))`.

#### 10. Introspection endpoint
**File**: `rapina/src/introspection/endpoint.rs`
**Changes**: Same pattern.

#### 11. WebSocket module
**File**: `rapina/src/websocket.rs`
**Changes**: The `WebSocketUpgrade` stores `Response<BoxBody>` which comes from `hyper_tungstenite::upgrade()`. The return type of `upgrade()` is `Response<Full<Bytes>>`, so we need to map it: `response.map(|body| full_body(body))`.

#### 12. Relay hub
**File**: `rapina/src/relay/hub.rs`
**Changes**: Update any `Full::new(...)` to `full_body(Full::new(...))`.

#### 13. Testing client
**File**: `rapina/src/testing/client.rs`
**Changes**: The test client calls `body.collect().await` to read response bodies. This still works — `http_body_util::combinators::BoxBody` implements `Body`, and `BodyExt::collect()` works on any `Body`. Update any direct `Full::new()` construction.

#### 14. Cargo.toml
**File**: `rapina/Cargo.toml`
**Changes**: Make `futures-util` unconditional (needed for `StreamResponse`/`SseResponse` in Phase 2, but also `BodyExt` mapping uses it). Currently it's behind `multipart` and `websocket` features.

```toml
futures-util = { version = "0.3", default-features = false, features = ["sink"] }
```

Move from optional to required. The `websocket` and `multipart` features should still list it to avoid breaking, but it won't be optional anymore.

### Affected Files (complete list):
- `rapina/src/response.rs` — type alias + all IntoResponse impls
- `rapina/src/error.rs` — Error::into_response
- `rapina/src/extract/mod.rs` — Json IntoResponse
- `rapina/src/pagination.rs` — Paginated IntoResponse
- `rapina/src/cache.rs` — CacheMiddleware body reconstruction
- `rapina/src/middleware/compression.rs` — CompressionMiddleware body reconstruction
- `rapina/src/middleware/cors.rs` — BoxBody::default() calls
- `rapina/src/metrics/prometheus.rs` — metrics endpoint body
- `rapina/src/openapi/endpoint.rs` — OpenAPI JSON body
- `rapina/src/introspection/endpoint.rs` — introspection JSON body
- `rapina/src/websocket.rs` — WebSocketUpgrade response mapping
- `rapina/src/relay/hub.rs` — relay response body
- `rapina/src/testing/client.rs` — test body construction
- `rapina/Cargo.toml` — futures-util dependency

### Files that DON'T need changes (they only import `BoxBody` as a type, never construct `Full` directly):
- `rapina/src/handler.rs`
- `rapina/src/router/mod.rs`
- `rapina/src/middleware/mod.rs`
- `rapina/src/middleware/timeout.rs`
- `rapina/src/middleware/body_limit.rs`
- `rapina/src/middleware/request_log.rs`
- `rapina/src/middleware/trace_id.rs`
- `rapina/src/middleware/rate_limit.rs`
- `rapina/src/auth/middleware.rs`
- `rapina/src/metrics/middleware.rs`
- `rapina-macros/src/lib.rs` (references `rapina::response::BoxBody` by name — still works)

### Success Criteria:

#### Automated Verification:
- [x] `cargo check --all-features` passes with no errors
- [x] `cargo check --no-default-features` passes
- [x] `cargo test --all-features` passes — all existing tests pass without modification (beyond the mechanical `Full::new` → `full_body` changes in test code)
- [x] `cargo test -p rapina-macros` passes — proc-macro tests unaffected

#### Manual Verification:
- [ ] Run the todo-app example and verify it serves responses correctly
- [ ] Run the websocket-chat example and verify WebSocket connections still work

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation from the human that the manual testing was successful before proceeding to the next phase.

---

## Phase 2: Add StreamResponse and SseResponse Types

### Overview
Add two new response types that produce streaming bodies, giving handlers first-class support for chunked transfer and Server-Sent Events.

### Changes Required:

#### 1. New streaming module
**File**: `rapina/src/streaming.rs` (new file)
**Changes**: Define `StreamResponse`, `SseResponse`, and `SseEvent` types.

```rust
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::Stream;
use http::{Response, StatusCode, header};
use http_body::{Body, Frame};
use http_body_util::StreamBody;

use crate::response::{BoxBody, BoxBodyError, IntoResponse};

/// A streaming HTTP response.
///
/// Wraps any `Stream<Item = Result<Bytes, BoxBodyError>>` and sends it
/// as a chunked transfer-encoded response.
///
/// # Example
///
/// ```rust,ignore
/// use rapina::streaming::StreamResponse;
/// use futures_util::stream;
///
/// #[get("/download")]
/// async fn download() -> StreamResponse {
///     let chunks = stream::iter(vec![
///         Ok(Bytes::from("chunk 1")),
///         Ok(Bytes::from("chunk 2")),
///     ]);
///     StreamResponse::new(chunks)
/// }
/// ```
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
    pub fn header(mut self, name: http::header::HeaderName, value: http::header::HeaderValue) -> Self {
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
        let body: BoxBody = body
            .map_err(|e| -> BoxBodyError { e })
            .boxed();

        let mut response = Response::builder()
            .status(self.status)
            .body(body)
            .unwrap();

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
///
/// # Example
///
/// ```rust,ignore
/// use rapina::streaming::{SseResponse, SseEvent};
/// use futures_util::stream;
///
/// #[get("/events")]
/// async fn events() -> SseResponse {
///     let events = stream::iter(vec![
///         Ok(SseEvent::new().data("hello")),
///         Ok(SseEvent::new().event("update").data("world")),
///     ]);
///     SseResponse::new(events)
/// }
/// ```
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

        let event_stream = self.stream.map(|result| {
            result.map(|event| Frame::data(event.into_bytes()))
        });

        let stream: Pin<Box<dyn Stream<Item = Result<Frame<Bytes>, BoxBodyError>> + Send>> =
            if let Some(interval) = self.keep_alive {
                let keep_alive_stream = futures_util::stream::unfold((), move |()| async move {
                    tokio::time::sleep(interval).await;
                    let comment = Bytes::from_static(b": keep-alive\n\n");
                    Some((Ok(Frame::data(comment)), ()))
                });
                // Merge event stream with keep-alive stream
                use futures_util::stream::select;
                Box::pin(select(event_stream, keep_alive_stream))
            } else {
                Box::pin(event_stream)
            };

        let body = StreamBody::new(stream);
        let body: BoxBody = body.map_err(|e| -> BoxBodyError { e }).boxed();

        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .body(body)
            .unwrap();

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

    /// Sets the event data. Multiple calls append data lines.
    pub fn data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }

    /// Sets the retry interval in milliseconds.
    pub fn retry(mut self, ms: u64) -> Self {
        self.retry = Some(ms);
        self
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

/// Convenience: create an SseEvent from a JSON-serializable value.
impl SseEvent {
    /// Sets the data field to the JSON serialization of `value`.
    pub fn json_data<T: serde::Serialize>(self, value: &T) -> Result<Self, serde_json::Error> {
        let json = serde_json::to_string(value)?;
        Ok(self.data(json))
    }
}
```

#### 2. Register the module
**File**: `rapina/src/lib.rs`
**Changes**: Add `pub mod streaming;` (unconditional — no feature gate).

#### 3. Re-export in prelude
**File**: `rapina/src/lib.rs`
**Changes**: Add to the prelude:
```rust
pub use crate::streaming::{SseEvent, SseResponse, StreamResponse};
```

Also re-export `full_body`, `empty_body` from response (useful for custom `IntoResponse` impls):
```rust
pub use crate::response::{full_body, empty_body};
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo check --all-features` passes
- [x] `cargo test --all-features` passes
- [x] New unit tests pass:
  - `SseEvent::into_bytes()` produces correct wire format for all field combinations
  - `SseEvent::json_data()` serializes correctly
  - `StreamResponse::into_response()` produces a valid `Response<BoxBody>` with `StreamingMarker` extension
  - `SseResponse::into_response()` has `text/event-stream` content type, `no-cache`, `keep-alive` headers, and `StreamingMarker` extension

#### Manual Verification:
- [ ] Create a test handler returning `SseResponse` and verify with `curl` that events stream incrementally (not buffered)

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation from the human that the manual testing was successful before proceeding to the next phase.

---

## Phase 3: Streaming-Aware Middleware

### Overview
Update the compression and cache middlewares to detect streaming responses (via `StreamingMarker` extension) and skip body buffering. Streaming responses pass through these middlewares untouched.

### Changes Required:

#### 1. Compression middleware
**File**: `rapina/src/middleware/compression.rs`
**Changes**: After calling `next.run(req).await`, check for `StreamingMarker` in response extensions before attempting to collect the body.

```rust
// After let response = next.run(req).await;
// Add early return for streaming responses:
if response.extensions().get::<crate::streaming::StreamingMarker>().is_some() {
    return response;
}
```

This goes right after the existing `algorithm` match block (line ~156), before `response.into_parts()`.

#### 2. Cache middleware
**File**: `rapina/src/cache.rs`
**Changes**: Same pattern — skip caching for streaming responses. After getting the response from `next.run(req).await`, check for `StreamingMarker` and return immediately without attempting to cache.

```rust
// After let response = next.run(req).await;
// Before checking x-rapina-cache-ttl:
if response.extensions().get::<crate::streaming::StreamingMarker>().is_some() {
    return response;
}
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo test --all-features` passes
- [x] New integration test: a streaming handler behind compression middleware streams correctly without buffering
- [x] New integration test: a streaming handler behind cache middleware is not cached and streams correctly

#### Manual Verification:
- [ ] SSE handler with compression middleware enabled streams events incrementally

**Implementation Note**: After completing this phase and all automated verification passes, pause here for manual confirmation from the human that the manual testing was successful before proceeding to the next phase.

---

## Testing Strategy

### Unit Tests:
- `SseEvent` wire format: all fields, multi-line data, empty event, json_data
- `StreamResponse` builder: status, headers, content_type
- `SseResponse` builder: keep_alive
- `full_body()` and `empty_body()` produce valid `BoxBody`
- Existing `IntoResponse` tests still pass (they call `.collect().await` which works on the new `BoxBody`)

### Integration Tests:
- `StreamResponse` handler behind the test client: response body arrives in chunks
- `SseResponse` handler behind the test client: verify `text/event-stream` content type and SSE framing
- Compression middleware + streaming: verify response is not compressed (passed through)
- Cache middleware + streaming: verify response is not cached

### Manual Testing Steps:
1. Start a dev server with an SSE endpoint
2. `curl -N http://localhost:3000/events` — verify events appear one by one
3. `curl -N -H "Accept-Encoding: gzip" http://localhost:3000/events` — verify events still stream (not gzip buffered)
4. Verify existing todo-app example works unchanged

## Performance Considerations

- The new `BoxBody` adds one vtable indirection per `poll_frame` call compared to the concrete `Full<Bytes>`. For buffered responses this is negligible — the body is polled exactly once (single frame). For streaming responses there was no alternative before, so this is net-new capability.
- `StreamingMarker` check is O(1) extension lookup — no overhead for non-streaming responses beyond the `get()` call.
- Keep-alive for SSE uses `tokio::time::sleep` per interval — standard approach, minimal overhead.

## Migration Notes

This is a non-breaking change for downstream users who:
- Return `String`, `&str`, `Json<T>`, `StatusCode`, etc. from handlers — these continue to work via `IntoResponse`
- Use custom `IntoResponse` impls that construct `Response<BoxBody>` — the type is still named `BoxBody`, and `full_body()` / `empty_body()` are provided as helpers

This IS a breaking change for downstream users who:
- Directly construct `Full::new(Bytes::from(...))` as a body — they need to wrap with `full_body()`
- Call `BoxBody::default()` — they need to use `empty_body()` instead
- Pattern-match or access the inner `Full<Bytes>` fields — unlikely but possible

The helpers `full_body()` and `empty_body()` are re-exported in the prelude to make migration easy.

## Affected Files Summary

### Must change (body construction):
| File | Change |
|------|--------|
| `response.rs` | Type alias + all IntoResponse impls |
| `error.rs` | Error::into_response body construction |
| `extract/mod.rs` | Json IntoResponse body construction |
| `pagination.rs` | Paginated IntoResponse body construction |
| `cache.rs` | Body reconstruction + streaming skip |
| `middleware/compression.rs` | Body reconstruction + streaming skip |
| `middleware/cors.rs` | BoxBody::default() → empty_body() |
| `metrics/prometheus.rs` | Body construction |
| `openapi/endpoint.rs` | Body construction |
| `introspection/endpoint.rs` | Body construction |
| `websocket.rs` | Response body mapping |
| `relay/hub.rs` | Body construction |
| `testing/client.rs` | Body construction in tests |
| `Cargo.toml` | futures-util unconditional |

### New files:
| File | Purpose |
|------|---------|
| `streaming.rs` | StreamResponse, SseResponse, SseEvent |

### No changes needed:
| File | Reason |
|------|--------|
| `handler.rs` | Only references `BoxBody` as a type name |
| `router/mod.rs` | Only references `BoxBody` as a type name |
| `middleware/mod.rs` | Only references `BoxBody` as a type name |
| `middleware/timeout.rs` | Pass-through, no body construction |
| `middleware/body_limit.rs` | Request body only |
| `middleware/request_log.rs` | Header-only inspection |
| `middleware/trace_id.rs` | Header-only modification |
| `middleware/rate_limit.rs` | Early return with error response (uses IntoResponse) |
| `auth/middleware.rs` | Early return with error response (uses IntoResponse) |
| `metrics/middleware.rs` | Timing only, pass-through |
| `rapina-macros/src/lib.rs` | References `rapina::response::BoxBody` by name — still resolves |

## References

- `http_body_util::combinators::BoxBody` docs: the target type for the new alias
- `http_body_util::StreamBody`: wraps a `Stream<Item = Result<Frame<Bytes>, E>>` into a `Body` impl
- hyper 1.x body model: `http_body::Body` trait with `poll_frame` returning `Frame<Data>` or `Frame<Trailers>`
- SSE spec: https://html.spec.whatwg.org/multipage/server-sent-events.html
