//! Integration tests for the Prometheus metrics feature.

#![cfg(feature = "metrics")]

use http::StatusCode;
use rapina::metrics::MetricsRegistry;
use rapina::prelude::*;
use rapina::testing::TestClient;

// ── helpers ──────────────────────────────────────────────────────────────────

fn app_with_metrics() -> rapina::app::Rapina {
    Rapina::new()
        .with_introspection(false)
        .with_metrics(true)
        .router(
            Router::new()
                .route(http::Method::GET, "/health", |_, _, _| async { "ok" })
                .route(http::Method::GET, "/users/:id", |_, _, _| async {
                    StatusCode::OK
                })
                .route(http::Method::POST, "/users", |_, _, _| async {
                    StatusCode::CREATED
                }),
        )
}

// ── /metrics endpoint ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_metrics_endpoint_returns_200() {
    let client = TestClient::new(app_with_metrics()).await;
    let response = client.get("/metrics").send().await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_metrics_endpoint_content_type() {
    let client = TestClient::new(app_with_metrics()).await;
    let response = client.get("/metrics").send().await;

    let content_type = response
        .headers()
        .get("content-type")
        .expect("content-type header missing")
        .to_str()
        .unwrap();

    assert!(content_type.contains("text/plain"));
    assert!(content_type.contains("version=0.0.4"));
}

#[tokio::test]
async fn test_metrics_endpoint_contains_all_metric_names() {
    let client = TestClient::new(app_with_metrics()).await;

    // Generate one real request so CounterVec/HistogramVec emit HELP+TYPE lines.
    client.get("/health").send().await;

    let body = client.get("/metrics").send().await.text();

    assert!(body.contains("http_requests_total"));
    assert!(body.contains("http_request_duration_seconds"));
    assert!(body.contains("http_requests_in_flight"));
}

#[tokio::test]
async fn test_metrics_endpoint_prometheus_format() {
    let client = TestClient::new(app_with_metrics()).await;
    let body = client.get("/metrics").send().await.text();

    assert!(body.contains("# HELP"));
    assert!(body.contains("# TYPE"));
}

// ── counter increments ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_metrics_counter_increments_on_request() {
    let client = TestClient::new(app_with_metrics()).await;

    client.get("/health").send().await;

    let body = client.get("/metrics").send().await.text();
    // After one GET /health 200, the counter label set must appear
    assert!(body.contains(r#"method="GET""#));
    assert!(body.contains(r#"path="/health""#));
    assert!(body.contains(r#"status="200""#));
}

#[tokio::test]
async fn test_metrics_counter_accumulates() {
    let client = TestClient::new(app_with_metrics()).await;

    client.get("/health").send().await;
    client.get("/health").send().await;
    client.get("/health").send().await;

    let body = client.get("/metrics").send().await.text();
    // Three requests → counter value 3 (plus the /metrics call itself, but different labels)
    assert!(body.contains(r#"path="/health""#));
    // The line for GET /health 200 should show 3
    assert!(body.contains("} 3"));
}

#[tokio::test]
async fn test_metrics_duration_histogram_populated() {
    let client = TestClient::new(app_with_metrics()).await;

    client.get("/health").send().await;

    let body = client.get("/metrics").send().await.text();
    // Histogram emits _bucket, _sum, _count suffixes
    assert!(body.contains("http_request_duration_seconds_bucket"));
    assert!(body.contains("http_request_duration_seconds_sum"));
    assert!(body.contains("http_request_duration_seconds_count"));
}

// ── path normalisation ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_metrics_numeric_path_segments_normalised() {
    let client = TestClient::new(app_with_metrics()).await;

    client.get("/users/42").send().await;

    let body = client.get("/metrics").send().await.text();
    // The raw ID must NOT appear as a label value
    assert!(!body.contains(r#"path="/users/42""#));
    // The normalised form must appear instead
    assert!(body.contains(r#"path="/users/:id""#));
}

// ── disabled by default ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_metrics_disabled_by_default() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(http::Method::GET, "/", |_, _, _| async { "ok" }));

    let client = TestClient::new(app).await;
    let response = client.get("/metrics").send().await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ── MetricsRegistry unit-level via state ─────────────────────────────────────

#[test]
fn test_metrics_registry_new_does_not_panic() {
    let _r = MetricsRegistry::new();
}

#[test]
fn test_metrics_registry_encode_returns_text() {
    let r = MetricsRegistry::new();
    let out = r.encode();
    assert!(!out.is_empty());
    assert!(out.contains("# TYPE"));
}
