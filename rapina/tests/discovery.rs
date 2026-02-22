//! Integration tests for route auto-discovery.
//!
//! IMPORTANT: `inventory` collects from the entire test binary.
//! All handlers across test files share the same collection.
//! Use unique `/disc-*` path prefixes to avoid collisions.

use http::StatusCode;
use rapina::prelude::*;
use rapina::testing::TestClient;

// ── Discovered handlers ─────────────────────────────────────────────────────

#[get("/disc-hello")]
async fn disc_hello() -> &'static str {
    "hello from discovery"
}

#[post("/disc-echo")]
async fn disc_echo() -> &'static str {
    "echoed"
}

#[put("/disc-update")]
async fn disc_update() -> &'static str {
    "updated"
}

#[delete("/disc-remove")]
async fn disc_remove() -> StatusCode {
    StatusCode::NO_CONTENT
}

// ── Public handlers (both orderings) ────────────────────────────────────────

// #[public] ABOVE #[get] — PublicMarker path
#[public]
#[get("/disc-pub-above")]
async fn disc_pub_above() -> &'static str {
    "public above"
}

// #[public] BELOW #[get] — is_public path
#[get("/disc-pub-below")]
#[public]
async fn disc_pub_below() -> &'static str {
    "public below"
}

// Non-public handler (should be blocked by auth)
#[get("/disc-protected")]
async fn disc_protected() -> &'static str {
    "protected"
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_discovered_get_route() {
    let app = Rapina::new().with_introspection(false).discover();
    let client = TestClient::new(app).await;

    let resp = client.get("/disc-hello").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "hello from discovery");
}

#[tokio::test]
async fn test_discovered_post_route() {
    let app = Rapina::new().with_introspection(false).discover();
    let client = TestClient::new(app).await;

    let resp = client.post("/disc-echo").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "echoed");
}

#[tokio::test]
async fn test_discovered_put_route() {
    let app = Rapina::new().with_introspection(false).discover();
    let client = TestClient::new(app).await;

    let resp = client.put("/disc-update").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "updated");
}

#[tokio::test]
async fn test_discovered_delete_route() {
    let app = Rapina::new().with_introspection(false).discover();
    let client = TestClient::new(app).await;

    let resp = client.delete("/disc-remove").send().await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_discover_and_router_are_additive() {
    let manual_router = Router::new().route(http::Method::GET, "/disc-manual", |_, _, _| async {
        "manual route"
    });

    let app = Rapina::new()
        .with_introspection(false)
        .router(manual_router)
        .discover();

    let client = TestClient::new(app).await;

    // Discovered route works
    let resp = client.get("/disc-hello").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "hello from discovery");

    // Manual route also works
    let resp = client.get("/disc-manual").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "manual route");
}

#[tokio::test]
async fn test_public_above_route_macro_bypasses_auth() {
    let auth_config = AuthConfig::new("test-secret-disc", 3600);

    let app = Rapina::new()
        .with_introspection(false)
        .with_auth(auth_config)
        .discover();

    let client = TestClient::new(app).await;

    // #[public] above #[get] — should be accessible without token
    let resp = client.get("/disc-pub-above").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "public above");
}

#[tokio::test]
async fn test_public_below_route_macro_bypasses_auth() {
    let auth_config = AuthConfig::new("test-secret-disc", 3600);

    let app = Rapina::new()
        .with_introspection(false)
        .with_auth(auth_config)
        .discover();

    let client = TestClient::new(app).await;

    // #[public] below #[get] — should be accessible without token
    let resp = client.get("/disc-pub-below").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "public below");
}

#[tokio::test]
async fn test_non_public_discovered_route_blocked_by_auth() {
    let auth_config = AuthConfig::new("test-secret-disc", 3600);

    let app = Rapina::new()
        .with_introspection(false)
        .with_auth(auth_config)
        .discover();

    let client = TestClient::new(app).await;

    // Non-public route should be blocked (401)
    let resp = client.get("/disc-protected").send().await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_discovered_routes_appear_in_introspection() {
    let app = Rapina::new().with_introspection(true).discover();

    let client = TestClient::new(app).await;
    let resp = client.get("/__rapina/routes").send().await;

    assert_eq!(resp.status(), StatusCode::OK);

    let routes: Vec<serde_json::Value> = resp.json();
    let paths: Vec<&str> = routes
        .iter()
        .filter_map(|r| r.get("path").and_then(|p| p.as_str()))
        .collect();

    assert!(paths.contains(&"/disc-hello"));
    assert!(paths.contains(&"/disc-echo"));
    assert!(paths.contains(&"/disc-pub-above"));
    assert!(paths.contains(&"/disc-pub-below"));
}
