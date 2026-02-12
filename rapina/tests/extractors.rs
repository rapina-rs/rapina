//! Integration tests for request extractors.

use http::StatusCode;
use rapina::prelude::*;
use rapina::testing::TestClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// JSON Extractor Tests

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct User {
    name: String,
    email: String,
}

#[tokio::test]
async fn test_json_extraction() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |req, _, _| async move {
                use http_body_util::BodyExt;
                let body = req.into_body().collect().await.unwrap().to_bytes();
                let user: User = serde_json::from_slice(&body).unwrap();
                Json(user)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .post("/users")
        .json(&User {
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
        })
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let user: User = response.json();
    assert_eq!(user.name, "Alice");
    assert_eq!(user.email, "alice@example.com");
}

#[tokio::test]
async fn test_json_extraction_invalid_json() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |req, _, _| async move {
                use http_body_util::BodyExt;
                let body = req.into_body().collect().await.unwrap().to_bytes();
                match serde_json::from_slice::<User>(&body) {
                    Ok(user) => Json(serde_json::json!(user)).into_response(),
                    Err(_) => Error::bad_request("invalid JSON").into_response(),
                }
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .post("/users")
        .header("content-type", "application/json")
        .body("not valid json")
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_json_response() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/user", |_, _, _| async {
                Json(User {
                    name: "Bob".to_string(),
                    email: "bob@test.com".to_string(),
                })
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/user").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("application/json")
    );

    let user: User = response.json();
    assert_eq!(user.name, "Bob");
}

// Query Extractor Tests

#[derive(Debug, Deserialize)]
struct Pagination {
    page: Option<u32>,
    limit: Option<u32>,
}

#[tokio::test]
async fn test_query_extraction() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/items", |req, _, _| async move {
                let query = req.uri().query().unwrap_or("");
                let params: Pagination = serde_urlencoded::from_str(query).unwrap_or(Pagination {
                    page: None,
                    limit: None,
                });
                format!(
                    "page={}, limit={}",
                    params.page.unwrap_or(1),
                    params.limit.unwrap_or(10)
                )
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/items?page=2&limit=20").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "page=2, limit=20");
}

#[tokio::test]
async fn test_query_extraction_optional_params() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/items", |req, _, _| async move {
                let query = req.uri().query().unwrap_or("");
                let params: Pagination = serde_urlencoded::from_str(query).unwrap_or(Pagination {
                    page: None,
                    limit: None,
                });
                format!(
                    "page={}, limit={}",
                    params.page.unwrap_or(1),
                    params.limit.unwrap_or(10)
                )
            }),
        );

    let client = TestClient::new(app).await;

    // No query params - should use defaults
    let response = client.get("/items").send().await;
    assert_eq!(response.text(), "page=1, limit=10");

    // Only page param
    let response = client.get("/items?page=5").send().await;
    assert_eq!(response.text(), "page=5, limit=10");
}

// Path Extractor Tests

#[tokio::test]
async fn test_path_extraction_u64() {
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
async fn test_path_extraction_string() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::GET,
            "/users/:username",
            |_, params, _| async move {
                let username = params.get("username").cloned().unwrap_or_default();
                format!("Hello, {}!", username)
            },
        ));

    let client = TestClient::new(app).await;
    let response = client.get("/users/alice").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "Hello, alice!");
}

#[tokio::test]
async fn test_path_extraction_multiple_params() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(
            http::Method::GET,
            "/users/:user_id/posts/:post_id",
            |_, params, _| async move {
                let user_id = params.get("user_id").cloned().unwrap_or_default();
                let post_id = params.get("post_id").cloned().unwrap_or_default();
                format!("User {} - Post {}", user_id, post_id)
            },
        ));

    let client = TestClient::new(app).await;
    let response = client.get("/users/10/posts/99").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "User 10 - Post 99");
}

// Headers Extractor Tests

#[tokio::test]
async fn test_headers_extraction() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/auth", |req, _, _| async move {
                let auth = req
                    .headers()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("none");
                format!("Auth: {}", auth)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .get("/auth")
        .header("authorization", "Bearer secret-token")
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "Auth: Bearer secret-token");
}

#[tokio::test]
async fn test_headers_extraction_missing() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/auth", |req, _, _| async move {
                match req.headers().get("authorization") {
                    Some(_) => "authenticated".to_string(),
                    None => "not authenticated".to_string(),
                }
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/auth").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "not authenticated");
}

#[tokio::test]
async fn test_custom_header() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/custom", |req, _, _| async move {
                let custom = req
                    .headers()
                    .get("x-custom-header")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("missing");
                format!("Custom: {}", custom)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .get("/custom")
        .header("x-custom-header", "my-value")
        .send()
        .await;

    assert_eq!(response.text(), "Custom: my-value");
}

// Form Extractor Tests

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LoginForm {
    username: String,
    password: String,
}

#[tokio::test]
async fn test_form_extraction() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/login", |req, _, _| async move {
                use http_body_util::BodyExt;

                let content_type = req
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                if !content_type.contains("application/x-www-form-urlencoded") {
                    return Error::bad_request("expected form data").into_response();
                }

                let body = req.into_body().collect().await.unwrap().to_bytes();
                match serde_urlencoded::from_bytes::<LoginForm>(&body) {
                    Ok(form) => format!("Welcome, {}!", form.username).into_response(),
                    Err(_) => Error::bad_request("invalid form").into_response(),
                }
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .post("/login")
        .form(&serde_json::json!({
            "username": "alice",
            "password": "secret123"
        }))
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "Welcome, alice!");
}

// State Extractor Tests

#[derive(Clone)]
struct AppConfig {
    app_name: String,
    version: String,
}

#[tokio::test]
async fn test_state_extraction() {
    use rapina::state::AppState;

    let app = Rapina::new()
        .with_introspection(false)
        .state(AppConfig {
            app_name: "MyApp".to_string(),
            version: "1.0.0".to_string(),
        })
        .router(Router::new().route(
            http::Method::GET,
            "/info",
            |_, _, state: Arc<AppState>| async move {
                let config = state.get::<AppConfig>().unwrap();
                format!("{} v{}", config.app_name, config.version)
            },
        ));

    let client = TestClient::new(app).await;
    let response = client.get("/info").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "MyApp v1.0.0");
}

#[tokio::test]
async fn test_multiple_state_types() {
    use rapina::state::AppState;

    #[derive(Clone)]
    struct DbConfig {
        url: String,
    }

    #[derive(Clone)]
    struct CacheConfig {
        ttl: u32,
    }

    let app = Rapina::new()
        .with_introspection(false)
        .state(DbConfig {
            url: "postgres://localhost".to_string(),
        })
        .state(CacheConfig { ttl: 3600 })
        .router(Router::new().route(
            http::Method::GET,
            "/config",
            |_, _, state: Arc<AppState>| async move {
                let db = state.get::<DbConfig>().unwrap();
                let cache = state.get::<CacheConfig>().unwrap();
                format!("DB: {}, Cache TTL: {}", db.url, cache.ttl)
            },
        ));

    let client = TestClient::new(app).await;
    let response = client.get("/config").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "DB: postgres://localhost, Cache TTL: 3600");
}

// Context Extractor Tests

#[tokio::test]
async fn test_context_trace_id() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/trace", |req, _, _| async move {
                use rapina::context::RequestContext;
                let ctx = req.extensions().get::<RequestContext>().unwrap();
                format!("Trace ID length: {}", ctx.trace_id.len())
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/trace").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    // UUID is 36 characters
    assert_eq!(response.text(), "Trace ID length: 36");
}

// Validated Extractor Tests

#[derive(Debug, Deserialize, Validate)]
struct CreateUser {
    #[validate(length(min = 1, max = 50))]
    name: String,
    #[validate(email)]
    email: String,
}

#[tokio::test]
async fn test_validated_extraction_valid() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |req, _, _| async move {
                use http_body_util::BodyExt;
                let body = req.into_body().collect().await.unwrap().to_bytes();
                let user: CreateUser = match serde_json::from_slice(&body) {
                    Ok(u) => u,
                    Err(_) => return Error::bad_request("invalid JSON").into_response(),
                };

                if let Err(e) = user.validate() {
                    return Error::validation("validation failed")
                        .with_details(serde_json::to_value(e).unwrap_or_default())
                        .into_response();
                }

                format!("Created user: {}", user.name).into_response()
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .post("/users")
        .json(&serde_json::json!({
            "name": "Alice",
            "email": "alice@example.com"
        }))
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "Created user: Alice");
}

#[tokio::test]
async fn test_validated_extraction_invalid_email() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |req, _, _| async move {
                use http_body_util::BodyExt;
                let body = req.into_body().collect().await.unwrap().to_bytes();
                let user: CreateUser = match serde_json::from_slice(&body) {
                    Ok(u) => u,
                    Err(_) => return Error::bad_request("invalid JSON").into_response(),
                };

                if let Err(e) = user.validate() {
                    return Error::validation("validation failed")
                        .with_details(serde_json::to_value(e).unwrap_or_default())
                        .into_response();
                }

                format!("Created user: {}", user.name).into_response()
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .post("/users")
        .json(&serde_json::json!({
            "name": "Alice",
            "email": "not-an-email"
        }))
        .send()
        .await;

    assert_eq!(response.status(), 422); // Validation error
}

#[tokio::test]
async fn test_validated_extraction_empty_name() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |req, _, _| async move {
                use http_body_util::BodyExt;
                let body = req.into_body().collect().await.unwrap().to_bytes();
                let user: CreateUser = match serde_json::from_slice(&body) {
                    Ok(u) => u,
                    Err(_) => return Error::bad_request("invalid JSON").into_response(),
                };

                if let Err(e) = user.validate() {
                    return Error::validation("validation failed")
                        .with_details(serde_json::to_value(e).unwrap_or_default())
                        .into_response();
                }

                format!("Created user: {}", user.name).into_response()
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .post("/users")
        .json(&serde_json::json!({
            "name": "",
            "email": "alice@example.com"
        }))
        .send()
        .await;

    assert_eq!(response.status(), 422); // Validation error
}

// Cookie Extractor Tests

#[tokio::test]
async fn test_cookie_extraction() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/dashboard", |req, _, _| async move {
                let cookie_header = req
                    .headers()
                    .get("cookie")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                // Parse session_id from cookie
                let session_id = cookie_header
                    .split(';')
                    .find_map(|pair| {
                        let (key, value) = pair.trim().split_once('=')?;
                        if key == "session_id" {
                            Some(value.to_string())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();

                format!("Session: {}", session_id)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .get("/dashboard")
        .header("cookie", "session_id=abc123")
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "Session: abc123");
}

#[tokio::test]
async fn test_cookie_extraction_multiple_cookies() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/user", |req, _, _| async move {
                let cookie_header = req
                    .headers()
                    .get("cookie")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                let cookies: std::collections::HashMap<String, String> = cookie_header
                    .split(';')
                    .filter_map(|pair| {
                        let (key, value) = pair.trim().split_once('=')?;
                        Some((key.to_string(), value.to_string()))
                    })
                    .collect();

                let session = cookies.get("session_id").cloned().unwrap_or_default();
                let user = cookies.get("user_id").cloned().unwrap_or_default();

                format!("Session: {}, User: {}", session, user)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client
        .get("/user")
        .header("cookie", "session_id=abc123; user_id=user456")
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "Session: abc123, User: user456");
}

#[tokio::test]
async fn test_cookie_extraction_missing() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/dashboard", |req, _, _| async move {
                let cookie_header = req
                    .headers()
                    .get("cookie")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                if cookie_header.is_empty() {
                    return Error::bad_request("missing cookies").into_response();
                }

                "ok".into_response()
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/dashboard").send().await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
