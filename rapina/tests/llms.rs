//! Integration tests for the `/__rapina/llms.txt` endpoint.
//!
//! Uses unique `/llms-*` path prefixes to avoid inventory collisions.

use http::StatusCode;
use rapina::prelude::*;
use rapina::testing::TestClient;

// ── Test handlers ──────────────────────────────────────────────────────────

#[get("/llms-users")]
async fn llms_list_users() -> &'static str {
    "users"
}

#[post("/llms-users")]
async fn llms_create_user() -> &'static str {
    "created"
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_llms_txt_returns_200_when_enabled() {
    let app = Rapina::new()
        .with_introspection(false)
        .enable_llms_txt()
        .discover();

    let client = TestClient::new(app).await;
    let resp = client.get("/__rapina/llms.txt").send().await;

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_llms_txt_content_type_is_text_plain() {
    let app = Rapina::new()
        .with_introspection(false)
        .enable_llms_txt()
        .discover();

    let client = TestClient::new(app).await;
    let resp = client.get("/__rapina/llms.txt").send().await;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/plain"),
        "Expected text/plain, got: {}",
        content_type
    );
}

#[tokio::test]
async fn test_llms_txt_body_contains_routes_section() {
    let app = Rapina::new()
        .with_introspection(false)
        .enable_llms_txt()
        .discover();

    let client = TestClient::new(app).await;
    let resp = client.get("/__rapina/llms.txt").send().await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text();
    assert!(body.contains("## Routes"), "Missing '## Routes' section");
}

#[tokio::test]
async fn test_llms_txt_body_contains_registered_routes() {
    let app = Rapina::new()
        .with_introspection(false)
        .enable_llms_txt()
        .discover();

    let client = TestClient::new(app).await;
    let resp = client.get("/__rapina/llms.txt").send().await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text();
    assert!(
        body.contains("GET /llms-users"),
        "Expected 'GET /llms-users' in llms.txt body"
    );
    assert!(
        body.contains("POST /llms-users"),
        "Expected 'POST /llms-users' in llms.txt body"
    );
}

#[tokio::test]
async fn test_llms_txt_returns_404_when_disabled() {
    let app = Rapina::new()
        .with_introspection(false)
        .disable_llms_txt()
        .discover();

    let client = TestClient::new(app).await;
    let resp = client.get("/__rapina/llms.txt").send().await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_llms_txt_does_not_expose_internal_rapina_routes() {
    let app = Rapina::new()
        .with_introspection(true)
        .enable_llms_txt()
        .discover();

    let client = TestClient::new(app).await;
    let resp = client.get("/__rapina/llms.txt").send().await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text();
    assert!(
        !body.contains("/__rapina"),
        "llms.txt should not expose internal /__rapina routes"
    );
}
