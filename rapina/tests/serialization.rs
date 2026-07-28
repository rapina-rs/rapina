//! Tests for JSON serialization in response types.

use http::StatusCode;
use rapina::prelude::*;
use rapina::testing::TestClient;
use serde::Serialize;
use std::fmt;

/// A type whose `Serialize` implementation returns an error, simulating any
/// real-world serialization failure (e.g. custom validation in `Serialize`,
/// IO errors on non-Vec sinks, etc.).
struct BrokenSerialize;

impl Serialize for BrokenSerialize {
    fn serialize<S: serde::Serializer>(
        &self,
        _serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("serialization failed"))
    }
}

impl fmt::Debug for BrokenSerialize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrokenSerialize")
    }
}

#[tokio::test]
async fn test_status_code_json_tuple_returns_500_on_serialization_failure() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/bad", |_, _, _| async {
                (StatusCode::OK, Json(BrokenSerialize))
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/bad").send().await;

    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "serialization failure must produce 500, not the handler's original status"
    );

    let json: serde_json::Value = response.json();
    assert!(
        json.get("error").is_some() || json.get("code").is_some(),
        "response body must contain error details, got: {json}"
    );
}

#[tokio::test]
async fn test_status_code_json_tuple_500_on_201_with_serialization_failure() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::POST, "/create", |_, _, _| async {
                (StatusCode::CREATED, Json(BrokenSerialize))
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.post("/create").send().await;

    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "serialization failure must override even an explicit 201"
    );

    let json: serde_json::Value = response.json();
    assert!(
        json.get("error").is_some() || json.get("code").is_some(),
        "response body must contain error details, got: {json}"
    );
}

#[tokio::test]
async fn test_json_only_returns_500_on_serialization_failure() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/bad", |_, _, _| async {
                Json(BrokenSerialize)
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/bad").send().await;

    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "Json<T> wrapper must also produce 500 on serialization failure"
    );

    let json: serde_json::Value = response.json();
    assert!(
        json.get("error").is_some() || json.get("code").is_some(),
        "response body must contain error details, got: {json}"
    );
}

#[derive(Serialize, Debug)]
struct GoodData {
    value: i32,
}

#[tokio::test]
async fn test_valid_tuple_serialization_still_works() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/ok", |_, _, _| async {
                (StatusCode::OK, Json(GoodData { value: 42 }))
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/ok").send().await;

    assert_eq!(response.status(), StatusCode::OK);

    let json: serde_json::Value = response.json();
    assert_eq!(json["value"], 42);
}

#[tokio::test]
async fn test_valid_json_wrapper_still_works() {
    let app = Rapina::new()
        .with_introspection(false)
        .router(
            Router::new().route(http::Method::GET, "/ok", |_, _, _| async {
                Json(GoodData { value: 99 })
            }),
        );

    let client = TestClient::new(app).await;
    let response = client.get("/ok").send().await;

    assert_eq!(response.status(), StatusCode::OK);

    let json: serde_json::Value = response.json();
    assert_eq!(json["value"], 99);
}
