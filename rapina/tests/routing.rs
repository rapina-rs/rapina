//! Integration tests for routing functionality.

use http::{Method, StatusCode};
use rapina::prelude::*;
use rapina::testing::TestClient;

#[tokio::test]
async fn test_basic_get_route() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(http::Method::GET, "/", |_, _, _| async { "Hello, World!" }));

    let client = TestClient::new(app).await;
    let response = client.get("/").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "Hello, World!");
}

#[tokio::test]
async fn test_basic_post_route() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |_, _, _| async {
                StatusCode::CREATED
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.post("/users").send().await;

    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_put_route() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(Method::PUT, "/users/:id", |_, _, _| async {
                StatusCode::OK
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.put("/users/123").send().await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_delete_route() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(Method::DELETE, "/users/:id", |_, _, _| async {
                StatusCode::NO_CONTENT
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.delete("/users/456").send().await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_404_for_unknown_route() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(http::Method::GET, "/exists", |_, _, _| async { "found" }));

    let client = TestClient::new(app).await;
    let response = client.get("/does-not-exist").send().await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_method_not_matching() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/resource", |_, _, _| async {
                "get response"
            }),
        );

    let client = TestClient::new(app).await;

    // GET should work
    let response = client.get("/resource").send().await;
    assert_eq!(response.status(), StatusCode::OK);

    // POST should return 404 (method doesn't match)
    let response = client.post("/resource").send().await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_path_parameter_extraction() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/users/:id", |_, params, _| async move {
                let id = params.get("id").cloned().unwrap_or_default();
                format!("User ID: {}", id)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/users/42").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "User ID: 42");
}

#[tokio::test]
async fn test_multiple_path_parameters() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::GET,
            "/users/:user_id/posts/:post_id",
            |_, params, _| async move {
                let user_id = params.get("user_id").cloned().unwrap_or_default();
                let post_id = params.get("post_id").cloned().unwrap_or_default();
                format!("User: {}, Post: {}", user_id, post_id)
            },
        ));

    let client = TestClient::new(app).await;
    let response = client.get("/users/10/posts/20").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "User: 10, Post: 20");
}

#[tokio::test]
async fn test_multiple_routes() {
    let app = Rapina::new().with_introspection(false).router(
        Router::new()
            .route(http::Method::GET, "/", |_, _, _| async { "home" })
            .route(http::Method::GET, "/about", |_, _, _| async { "about" })
            .route(http::Method::GET, "/contact", |_, _, _| async { "contact" })
            .route(http::Method::POST, "/submit", |_, _, _| async {
                "submitted"
            }),
    );

    let client = TestClient::new(app).await;

    assert_eq!(client.get("/").send().await.text(), "home");
    assert_eq!(client.get("/about").send().await.text(), "about");
    assert_eq!(client.get("/contact").send().await.text(), "contact");
    assert_eq!(client.post("/submit").send().await.text(), "submitted");
}

#[tokio::test]
async fn test_route_with_trailing_slash() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/users", |_, _, _| async {
                "users list"
            }),
        );

    let client = TestClient::new(app).await;

    // Without trailing slash should match
    let response = client.get("/users").send().await;
    assert_eq!(response.status(), StatusCode::OK);

    // With trailing slash might not match (depends on implementation)
    let response = client.get("/users/").send().await;
    // This tests current behavior - trailing slash is a different route
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_named_routes_for_introspection() {
    let app = Rapina::new().with_introspection(false).router(
        Router::new()
            .get_named("/users", "list_users", |_, _, _| async { "users" })
            .post_named("/users", "create_user", |_, _, _| async {
                StatusCode::CREATED
            }),
    );

    let client = TestClient::new(app).await;

    let response = client.get("/users").send().await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = client.post("/users").send().await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_introspection_endpoint() {
    let app = Rapina::new().with_introspection(true).router(
        Router::new()
            .get_named("/health", "health_check", |_, _, _| async { "ok" })
            .get_named("/users", "list_users", |_, _, _| async { "users" }),
    );

    let client = TestClient::new(app).await;
    let response = client.get("/__rapina/routes").send().await;

    assert_eq!(response.status(), StatusCode::OK);

    let routes: Vec<serde_json::Value> = response.json();
    assert!(routes.len() >= 2); // At least our 2 routes + introspection endpoint

    // Check that our routes are included
    let route_paths: Vec<&str> = routes
        .iter()
        .filter_map(|r| r.get("path").and_then(|p| p.as_str()))
        .collect();
    assert!(route_paths.contains(&"/health"));
    assert!(route_paths.contains(&"/users"));
}
