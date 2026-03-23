+++
title = "Streaming Responses"
description = "Server-Sent Events, chunked transfers, and streaming bodies"
weight = 12
date = 2026-03-17
+++

Rapina supports streaming HTTP responses for use cases where the full body isn't available up front — LLM token streaming, Server-Sent Events, large file downloads, or any chunked transfer pattern.

Two response types are provided: **`StreamResponse`** for raw byte streams and **`SseResponse`** for Server-Sent Events with proper framing. Both are available from `use rapina::prelude::*`.

## Server-Sent Events

`SseResponse` produces a `text/event-stream` response following the [SSE specification](https://html.spec.whatwg.org/multipage/server-sent-events.html). The browser reconnects automatically on disconnect.

```rust
use rapina::prelude::*;
use rapina::response::BoxBodyError;

#[get("/events")]
#[public]
async fn events() -> SseResponse {
    let stream = futures_util::stream::iter(vec![
        Ok::<_, BoxBodyError>(SseEvent::new().data("connected")),
        Ok(SseEvent::new().event("update").data("new data")),
    ]);
    SseResponse::new(stream)
}
```

The response includes `Content-Type: text/event-stream`, `Cache-Control: no-cache`, and `Connection: keep-alive` headers automatically.

### SseEvent

Build events with the `SseEvent` builder. All fields are optional — at minimum you'll want `data()`.

```rust
SseEvent::new()
    .id("42")                    // Last-Event-ID for reconnection
    .event("message")            // Event type (defaults to "message" on the client)
    .data("hello world")         // Event data (multi-line supported)
    .retry(5000)                 // Client reconnect interval in ms
```

| Method | Description |
|--------|-------------|
| `.data(s)` | Sets the event data. Multi-line strings are split into multiple `data:` lines |
| `.event(s)` | Sets the event type |
| `.id(s)` | Sets the event ID for reconnection |
| `.retry(ms)` | Tells the client to reconnect after `ms` milliseconds on disconnect |
| `.json_data(&value)` | Serializes a value as JSON into the data field |

### JSON events

Use `json_data()` to serialize structured data:

```rust
#[derive(Serialize)]
struct TokenChunk {
    token: String,
    done: bool,
}

#[get("/generate")]
#[public]
async fn generate() -> SseResponse {
    let (tx, rx) = tokio::sync::mpsc::channel(32);

    tokio::spawn(async move {
        for word in ["Hello", "from", "Rapina!"] {
            let event = SseEvent::new()
                .event("token")
                .json_data(&TokenChunk {
                    token: word.to_string(),
                    done: false,
                })
                .unwrap();
            tx.send(Ok(event)).await.ok();
        }
        tx.send(Ok(SseEvent::new()
            .event("token")
            .json_data(&TokenChunk { token: String::new(), done: true })
            .unwrap()))
            .await
            .ok();
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    SseResponse::new(stream)
}
```

### Keep-alive

Proxies and load balancers may close idle connections. Enable periodic keep-alive comments to prevent this:

```rust
SseResponse::new(stream)
    .keep_alive(std::time::Duration::from_secs(15))
```

This sends a `: keep-alive\n\n` comment every 15 seconds. SSE clients ignore comments, so this is invisible to application code.

## Chunked Streams

`StreamResponse` wraps any `Stream<Item = Result<Bytes, BoxBodyError>>` and sends it as a chunked transfer-encoded response.

```rust
use rapina::prelude::*;
use rapina::response::BoxBodyError;

#[get("/download")]
#[public]
async fn download() -> StreamResponse {
    let chunks = futures_util::stream::iter(vec![
        Ok::<_, BoxBodyError>(bytes::Bytes::from("chunk 1\n")),
        Ok(bytes::Bytes::from("chunk 2\n")),
    ]);
    StreamResponse::new(chunks)
        .content_type("text/plain")
        .status(http::StatusCode::OK)
}
```

### Builder methods

| Method | Description |
|--------|-------------|
| `.status(code)` | Sets the HTTP status code (default: 200) |
| `.header(name, value)` | Adds a response header |
| `.content_type(ct)` | Shorthand for setting `Content-Type` |

## Middleware behavior

Streaming responses are detected by middleware via an internal marker. The **compression** and **cache** middlewares automatically skip buffering for streaming responses:

- **Compression**: streaming responses pass through uncompressed. Buffering the entire stream to compress it would defeat the purpose of streaming.
- **Cache**: streaming responses are never cached, even if the route has `#[cache(ttl = N)]`.

All other middleware (CORS, rate limiting, timeout, trace ID, request logging, authentication) works normally with streaming responses.

## Custom IntoResponse with streaming

You can build your own streaming response types using `BoxBody::new()`:

```rust
use rapina::prelude::*;
use rapina::response::{BoxBody, BoxBodyError};
use rapina::streaming::StreamingMarker;

struct MyStreamingResponse { /* ... */ }

impl IntoResponse for MyStreamingResponse {
    fn into_response(self) -> http::Response<BoxBody> {
        // Build your stream...
        let stream = futures_util::stream::iter(
            vec![Ok::<_, BoxBodyError>(bytes::Bytes::from("data"))]
        );
        let body = http_body_util::StreamBody::new(
            futures_util::StreamExt::map(stream, |r| {
                r.map(http_body::Frame::data)
            })
        );

        use http_body_util::BodyExt;
        let body = BoxBody::new(body.map_err(|e| -> BoxBodyError { e }));

        let mut response = http::Response::builder()
            .status(http::StatusCode::OK)
            .body(body)
            .expect("response builder should not fail");

        // Insert StreamingMarker so compression/cache middleware skip buffering
        response.extensions_mut().insert(StreamingMarker);
        response
    }
}
```

The key detail: insert `StreamingMarker` into response extensions so middleware knows not to buffer the body.
