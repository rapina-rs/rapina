+++
title = "Testing"
description = "Integration testing with TestClient"
weight = 9
date = 2026-03-04
+++

Rapina ships a `TestClient` that spins up a real HTTP server on a random port for each test. You write tests with `#[tokio::test]`, make actual HTTP requests, and assert on the responses. No mocking, no faking — the full middleware stack runs exactly as it would in production.

## Setup

`TestClient::new(app)` takes a `Rapina` builder and starts a background server. The server shuts down automatically when the client is dropped.

```rust
use rapina::prelude::*;
use rapina::testing::TestClient;
use http::StatusCode;

#[tokio::test]
async fn test_hello() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(http::Method::GET, "/", |_, _, _| async {
            "Hello, World!"
        }));

    let client = TestClient::new(app).await;
    let response = client.get("/").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "Hello, World!");
}
```

Call `.with_introspection(false)` in tests to disable the built-in `/__rapina/routes` endpoint and keep your test output clean.

## Making requests

`TestClient` exposes `.get()`, `.post()`, `.put()`, `.patch()`, and `.delete()` — each returns a builder you chain before calling `.send().await`.

```rust
// GET
let resp = client.get("/users/42").send().await;

// POST with JSON body
let resp = client
    .post("/users")
    .json(&serde_json::json!({ "name": "Alice", "email": "alice@example.com" }))
    .send()
    .await;

// PUT with custom header
let resp = client
    .put("/users/42")
    .header("authorization", "Bearer eyJ...")
    .json(&serde_json::json!({ "name": "Alice Updated" }))
    .send()
    .await;

// DELETE
let resp = client.delete("/users/42").send().await;
```

### Request builder API

| Method | Description |
|--------|-------------|
| `.header(key, value)` | Adds a request header |
| `.json(&T)` | Serializes `T` as JSON and sets `Content-Type: application/json` |
| `.form(&T)` | URL-encodes `T` and sets `Content-Type: application/x-www-form-urlencoded` |
| `.body(impl Into<Bytes>)` | Sets raw body bytes |
| `.send()` | Sends the request, returns `TestResponse` |

## Reading responses

Every `.send().await` returns a `TestResponse` with these methods:

| Method | Return type | Description |
|--------|-------------|-------------|
| `.status()` | `StatusCode` | HTTP status code |
| `.text()` | `String` | Body as UTF-8 text |
| `.json::<T>()` | `T` | Deserialize body as JSON (panics on failure) |
| `.try_json::<T>()` | `Result<T, serde_json::Error>` | Deserialize body as JSON (returns error) |
| `.headers()` | `&HeaderMap` | Response headers |
| `.bytes()` | `&Bytes` | Raw body bytes |

```rust
#[derive(serde::Deserialize)]
struct User {
    name: String,
    email: String,
}

let resp = client.get("/users/1").send().await;
assert_eq!(resp.status(), StatusCode::OK);

let user: User = resp.json();
assert_eq!(user.name, "Alice");
```

Use `.try_json()` when you want to handle deserialization errors gracefully instead of panicking.

## Testing with authentication

Rapina routes are protected by default when auth is enabled. Set up `with_auth()` on the app, then test both the rejection and the authenticated path.

```rust
#[tokio::test]
async fn test_protected_route_rejects_anonymous() {
    let auth_config = AuthConfig::new("test-secret", 3600);

    let app = Rapina::new()
        .with_introspection(false)
        .with_auth(auth_config)
        .discover();

    let client = TestClient::new(app).await;

    // No token — should be rejected
    let resp = client.get("/protected-endpoint").send().await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
```

To test an authenticated request, create a token with `AuthConfig::create_token()` and pass it as a Bearer header:

```rust
#[tokio::test]
async fn test_protected_route_with_token() {
    let auth_config = AuthConfig::new("test-secret", 3600);
    let token = auth_config.create_token("user-123").unwrap();

    let app = Rapina::new()
        .with_introspection(false)
        .with_auth(auth_config)
        .discover();

    let client = TestClient::new(app).await;

    let resp = client
        .get("/protected-endpoint")
        .header("authorization", &format!("Bearer {}", token))
        .send()
        .await;

    assert_eq!(resp.status(), StatusCode::OK);
}
```

Routes marked `#[public]` bypass auth entirely:

```rust
#[public]
#[get("/health")]
async fn health() -> &'static str {
    "ok"
}

#[tokio::test]
async fn test_public_route_needs_no_token() {
    let auth_config = AuthConfig::new("test-secret", 3600);

    let app = Rapina::new()
        .with_introspection(false)
        .with_auth(auth_config)
        .discover();

    let client = TestClient::new(app).await;
    let resp = client.get("/health").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
}
```

## Testing with middleware

The full middleware stack runs during tests, so you can assert on side effects like response headers.

```rust
use rapina::middleware::{TraceIdMiddleware, TRACE_ID_HEADER};

#[tokio::test]
async fn test_trace_id_is_added() {
    let app = Rapina::new()
        .with_introspection(false)
        .middleware(TraceIdMiddleware::new())
        .router(Router::new().route(http::Method::GET, "/", |_, _, _| async { "ok" }));

    let client = TestClient::new(app).await;

    let resp1 = client.get("/").send().await;
    let resp2 = client.get("/").send().await;

    // Every response gets a trace ID header
    let id1 = resp1.headers().get(TRACE_ID_HEADER).unwrap().to_str().unwrap();
    let id2 = resp2.headers().get(TRACE_ID_HEADER).unwrap().to_str().unwrap();

    assert_eq!(id1.len(), 36); // UUID v4
    assert_ne!(id1, id2);      // unique per request
}
```

Body limit middleware:

```rust
use rapina::middleware::BodyLimitMiddleware;

#[tokio::test]
async fn test_body_limit_rejects_large_payload() {
    let app = Rapina::new()
        .with_introspection(false)
        .middleware(BodyLimitMiddleware::new(64)) // 64 bytes max
        .router(
            Router::new().route(http::Method::POST, "/upload", |_, _, _| async { "ok" }),
        );

    let client = TestClient::new(app).await;

    // Small payload — accepted
    let resp = client.post("/upload").body("small").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
}
```

## Testing error responses

All Rapina errors return a consistent JSON envelope with `error.code`, `error.message`, and a `trace_id`. Parse it with `serde_json::Value`:

```rust
#[tokio::test]
async fn test_error_response_format() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/users/:id", |_, _, _| async {
                Error::not_found("user not found")
            }),
        );

    let client = TestClient::new(app).await;
    let resp = client.get("/users/999").send().await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let json: serde_json::Value = resp.json();
    assert_eq!(json["error"]["code"], "NOT_FOUND");
    assert_eq!(json["error"]["message"], "user not found");
    assert!(json["trace_id"].is_string());
}
```

Errors with details include them under `error.details`:

```rust
let app = Rapina::new()
    .with_introspection(false)
    .router(
        Router::new().route(http::Method::POST, "/users", |_, _, _| async {
            Error::validation("invalid input").with_details(serde_json::json!({
                "field": "email",
                "reason": "invalid format"
            }))
        }),
    );

let client = TestClient::new(app).await;
let resp = client.post("/users").send().await;

assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

let json: serde_json::Value = resp.json();
assert_eq!(json["error"]["details"]["field"], "email");
```

## Complete example

A full CRUD test for a small in-memory API:

```rust
use rapina::prelude::*;
use rapina::testing::TestClient;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
struct Todo {
    id: u64,
    title: String,
}

type TodoStore = Arc<Mutex<Vec<Todo>>>;

#[tokio::test]
async fn test_todo_crud() {
    let store: TodoStore = Arc::new(Mutex::new(Vec::new()));

    let app = Rapina::new()
        .with_introspection(false)
        .state(store.clone())
        .router(
            Router::new()
                .route(http::Method::POST, "/todos", |req, _, state: Arc<AppState>| async move {
                    use http_body_util::BodyExt;
                    let body = req.into_body().collect().await.unwrap().to_bytes();
                    let todo: Todo = serde_json::from_slice(&body).unwrap();
                    let store = state.get::<TodoStore>().unwrap();
                    store.lock().await.push(todo.clone());
                    (StatusCode::CREATED, Json(todo))
                })
                .route(http::Method::GET, "/todos/:id", |_, params, state: Arc<AppState>| async move {
                    let id: u64 = params.get("id").unwrap().parse().unwrap();
                    let store = state.get::<TodoStore>().unwrap();
                    let todos = store.lock().await;
                    match todos.iter().find(|t| t.id == id) {
                        Some(todo) => Json(todo.clone()).into_response(),
                        None => Error::not_found("todo not found").into_response(),
                    }
                })
                .route(http::Method::DELETE, "/todos/:id", |_, params, state: Arc<AppState>| async move {
                    let id: u64 = params.get("id").unwrap().parse().unwrap();
                    let store = state.get::<TodoStore>().unwrap();
                    let mut todos = store.lock().await;
                    todos.retain(|t| t.id != id);
                    StatusCode::NO_CONTENT
                }),
        );

    let client = TestClient::new(app).await;

    // Create
    let resp = client
        .post("/todos")
        .json(&Todo { id: 1, title: "Write tests".into() })
        .send()
        .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Todo = resp.json();
    assert_eq!(created.title, "Write tests");

    // Read
    let resp = client.get("/todos/1").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    let fetched: Todo = resp.json();
    assert_eq!(fetched, created);

    // Not found
    let resp = client.get("/todos/999").send().await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Delete
    let resp = client.delete("/todos/1").send().await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Confirm deletion
    let resp = client.get("/todos/1").send().await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
```
