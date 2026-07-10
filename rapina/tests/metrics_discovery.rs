//! Integration tests for `#[metric]` auto-discovery.
//!
//! Kept in its own file: each test file is a separate binary with its own
//! inventory submissions, so the `#[metric]` static here doesn't leak into
//! the apps built in `tests/metrics.rs`.

#![cfg(feature = "metrics")]

use std::sync::LazyLock;

use http::StatusCode;
use rapina::metric;
use rapina::prelude::*;
use rapina::prometheus::IntCounter;
use rapina::testing::TestClient;

#[metric]
static DISCOVERY_TEST_COUNTER: LazyLock<IntCounter> = LazyLock::new(|| {
    IntCounter::new("rapina_test_discovered_total", "Orders discovered in tests").unwrap()
});

#[tokio::test]
async fn discovered_metric_appears_in_metrics_endpoint() {
    DISCOVERY_TEST_COUNTER.inc();
    DISCOVERY_TEST_COUNTER.inc();

    let app = Rapina::new()
        .with_introspection(false)
        .enable_metrics()
        .discover();

    let client = TestClient::new(app).await;
    let body = client.get("/metrics").send().await.text();

    assert!(body.contains("rapina_test_discovered_total"));
    assert!(body.contains("Orders discovered in tests"));
    assert!(body.contains("rapina_test_discovered_total 2"));
}

#[tokio::test]
async fn metric_not_registered_without_discover() {
    let app = Rapina::new().with_introspection(false).enable_metrics();

    let client = TestClient::new(app).await;
    let body = client.get("/metrics").send().await.text();

    assert!(!body.contains("rapina_test_discovered_total"));
}

#[tokio::test]
#[should_panic(expected = "rapina_test_discovered_total")]
async fn duplicate_metric_name_panics_at_startup() {
    let app = Rapina::new()
        .with_introspection(false)
        .enable_metrics()
        .add_metric(DISCOVERY_TEST_COUNTER.clone())
        .discover();

    TestClient::new(app).await;
}

#[tokio::test]
async fn discover_without_metrics_still_builds() {
    let app = Rapina::new().with_introspection(false).discover();

    let client = TestClient::new(app).await;
    let response = client.get("/metrics").send().await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
