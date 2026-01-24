//! Integration tests for error handling functionality.

use http::StatusCode;
use rapina::prelude::*;
use rapina::testing::TestClient;

#[tokio::test]
async fn test_error_400_bad_request() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/bad", |_, _, _| async {
                Error::bad_request("invalid input")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/bad").send().await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "BAD_REQUEST");
    assert_eq!(json["error"]["message"], "invalid input");
    assert!(json["trace_id"].is_string());
}

#[tokio::test]
async fn test_error_401_unauthorized() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/protected", |_, _, _| async {
                Error::unauthorized("authentication required")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/protected").send().await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "UNAUTHORIZED");
    assert_eq!(json["error"]["message"], "authentication required");
}

#[tokio::test]
async fn test_error_403_forbidden() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/admin", |_, _, _| async {
                Error::forbidden("access denied")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/admin").send().await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "FORBIDDEN");
    assert_eq!(json["error"]["message"], "access denied");
}

#[tokio::test]
async fn test_error_404_not_found() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/users/:id", |_, _, _| async {
                Error::not_found("user not found")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/users/999").send().await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "NOT_FOUND");
    assert_eq!(json["error"]["message"], "user not found");
}

#[tokio::test]
async fn test_error_409_conflict() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |_, _, _| async {
                Error::conflict("user already exists")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.post("/users").send().await;

    assert_eq!(response.status(), StatusCode::CONFLICT);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "CONFLICT");
    assert_eq!(json["error"]["message"], "user already exists");
}

#[tokio::test]
async fn test_error_422_validation() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |_, _, _| async {
                Error::validation("validation failed")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.post("/users").send().await;

    assert_eq!(response.status(), 422);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(json["error"]["message"], "validation failed");
}

#[tokio::test]
async fn test_error_429_rate_limited() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/api", |_, _, _| async {
                Error::rate_limited("too many requests")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/api").send().await;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "RATE_LIMITED");
    assert_eq!(json["error"]["message"], "too many requests");
}

#[tokio::test]
async fn test_error_500_internal() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/crash", |_, _, _| async {
                Error::internal("something went wrong")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/crash").send().await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "INTERNAL_ERROR");
    assert_eq!(json["error"]["message"], "something went wrong");
}

#[tokio::test]
async fn test_error_with_details() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |_, _, _| async {
                Error::validation("validation failed").with_details(serde_json::json!({
                    "errors": [
                        {"field": "email", "message": "invalid email format"},
                        {"field": "password", "message": "too short"}
                    ]
                }))
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.post("/users").send().await;

    assert_eq!(response.status(), 422);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
    assert!(json["error"]["details"]["errors"].is_array());

    let errors = json["error"]["details"]["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0]["field"], "email");
}

#[tokio::test]
async fn test_error_with_custom_trace_id() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/error", |_, _, _| async {
                Error::bad_request("test error").with_trace_id("custom-trace-123")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/error").send().await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let json: serde_json::Value = response.json();
    assert_eq!(json["trace_id"], "custom-trace-123");
}

#[tokio::test]
async fn test_error_trace_id_is_uuid_by_default() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/error", |_, _, _| async {
                Error::bad_request("test error")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/error").send().await;

    let json: serde_json::Value = response.json();
    let trace_id = json["trace_id"].as_str().unwrap();

    // UUID format: 8-4-4-4-12 = 36 characters
    assert_eq!(trace_id.len(), 36);
    assert_eq!(trace_id.chars().filter(|c| *c == '-').count(), 4);
}

#[tokio::test]
async fn test_error_response_content_type() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/error", |_, _, _| async {
                Error::bad_request("test")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/error").send().await;

    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("application/json"));
}

#[tokio::test]
async fn test_result_ok_returns_success() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/result", |_, _, _| async {
                let result: std::result::Result<&str, Error> = Ok("success");
                result
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/result").send().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text(), "success");
}

#[tokio::test]
async fn test_result_err_returns_error() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/result", |_, _, _| async {
                let result: std::result::Result<&str, Error> = Err(Error::not_found("not found"));
                result
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/result").send().await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_custom_error_status() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/custom", |_, _, _| async {
                Error::new(418, "IM_A_TEAPOT", "I'm a teapot")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/custom").send().await;

    assert_eq!(response.status().as_u16(), 418);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "IM_A_TEAPOT");
    assert_eq!(json["error"]["message"], "I'm a teapot");
}

#[tokio::test]
async fn test_error_without_details_omits_field() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/error", |_, _, _| async {
                Error::bad_request("simple error")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/error").send().await;

    let json: serde_json::Value = response.json();
    // details should not be present when None
    assert!(json["error"]["details"].is_null());
}

#[tokio::test]
async fn test_error_chaining() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/users", |_, _, _| async {
                Error::validation("invalid input")
                    .with_details(serde_json::json!({"field": "email"}))
                    .with_trace_id("trace-abc-123")
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.post("/users").send().await;

    assert_eq!(response.status(), 422);

    let json: serde_json::Value = response.json();
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(json["error"]["message"], "invalid input");
    assert_eq!(json["error"]["details"]["field"], "email");
    assert_eq!(json["trace_id"], "trace-abc-123");
}

#[tokio::test]
async fn test_router_404_response() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(Router::new().route(http::Method::GET, "/exists", |_, _, _| async { "found" }));

    let client = TestClient::new(app).await;
    let response = client.get("/not-exists").send().await;

    // Router returns plain 404, not JSON error
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
