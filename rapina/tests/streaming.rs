//! Integration tests for streaming responses.

use http::StatusCode;
use rapina::cache::{CACHE_STATUS_HEADER, CacheConfig};
use rapina::middleware::CompressionConfig;
use rapina::prelude::*;
use rapina::response::BoxBodyError;
use rapina::streaming::{SseEvent, SseResponse, StreamResponse};
use rapina::testing::TestClient;

#[tokio::test]
async fn test_stream_response_through_test_client() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/stream", |_, _, _| async {
                let chunks = futures_util::stream::iter(vec![
                    Ok::<_, BoxBodyError>(bytes::Bytes::from("hello ")),
                    Ok(bytes::Bytes::from("world")),
                ]);
                StreamResponse::new(chunks)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/stream").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "hello world");
}

#[tokio::test]
async fn test_sse_response_through_test_client() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/events", |_, _, _| async {
                let events = futures_util::stream::iter(vec![
                    Ok::<_, BoxBodyError>(SseEvent::new().data("first")),
                    Ok(SseEvent::new().event("update").data("second")),
                ]);
                SseResponse::new(events)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/events").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-cache");
    assert_eq!(
        response.text(),
        "data: first\n\nevent: update\ndata: second\n\n"
    );
}

#[tokio::test]
async fn test_streaming_response_bypasses_compression() {
    let app = Rapina::new()
        .with_introspection(false)
        .with_compression(CompressionConfig::new(0, 6)) // min_size=0 to compress everything
        .router(
            Router::new().route(http::Method::GET, "/stream", |_, _, _| async {
                let data = "x".repeat(2000); // Large enough to normally trigger compression
                let chunks = futures_util::stream::iter(vec![Ok::<_, BoxBodyError>(
                    bytes::Bytes::from(data),
                )]);
                StreamResponse::new(chunks).content_type("text/plain")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .get("/stream")
        .header("accept-encoding", "gzip")
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    // Should NOT be compressed — no content-encoding header
    assert!(response.headers().get("content-encoding").is_none());
    // Body should be the raw uncompressed data
    assert_eq!(response.text().len(), 2000);
}

#[tokio::test]
async fn test_streaming_response_bypasses_cache() {
    let app = Rapina::new()
        .with_introspection(false)
        .with_cache(CacheConfig::in_memory(100))
        .await
        .unwrap()
        .router(
            Router::new().route(http::Method::GET, "/stream", |_, _, _| async {
                let chunks = futures_util::stream::iter(vec![Ok::<_, BoxBodyError>(
                    bytes::Bytes::from("streamed"),
                )]);
                StreamResponse::new(chunks)
            }),
        );

    let client = TestClient::new(app).await;

    let r1 = client.get("/stream").send().await;
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r1.text(), "streamed");
    // Should not have cache status header
    assert!(r1.headers().get(CACHE_STATUS_HEADER).is_none());

    let r2 = client.get("/stream").send().await;
    assert_eq!(r2.text(), "streamed");
    // Still no cache status — streaming responses are never cached
    assert!(r2.headers().get(CACHE_STATUS_HEADER).is_none());
}

#[tokio::test]
async fn test_sse_response_with_json_data() {
    #[derive(serde::Serialize)]
    struct Update {
        count: u32,
    }

    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/events", |_, _, _| async {
                let events = futures_util::stream::iter(vec![Ok::<_, BoxBodyError>(
                    SseEvent::new()
                        .event("update")
                        .json_data(&Update { count: 42 })
                        .unwrap(),
                )]);
                SseResponse::new(events)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/events").send().await;

    assert_eq!(response.text(), "event: update\ndata: {\"count\":42}\n\n");
}

#[tokio::test]
async fn test_non_streaming_response_still_compressed() {
    let app = Rapina::new()
        .with_introspection(false)
        .with_compression(CompressionConfig::new(0, 6))
        .router(
            Router::new().route(http::Method::GET, "/data", |_, _, _| async {
                "x".repeat(2000)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .get("/data")
        .header("accept-encoding", "gzip")
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    // Non-streaming responses should still be compressed
    assert_eq!(response.headers().get("content-encoding").unwrap(), "gzip");
}
