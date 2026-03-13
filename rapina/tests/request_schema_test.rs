//! Integration tests for request body schema extraction in OpenAPI spec.
//!
//! These tests verify that the #[get], #[post], etc. macros correctly extract
//! request body schemas from Json<T> and Validated<Json<T>> extractors.

use rapina::prelude::*;
use rapina::testing::TestClient;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Deserialize, Validate, Serialize, JsonSchema)]
struct CreateUser {
    email: String,
    name: String,
}

#[post("/test-json-body")]
async fn test_json_body_handler(_body: Json<CreateUser>) -> &'static str {
    "ok"
}

#[post("/test-validated-body")]
async fn test_validated_body_handler(_body: Validated<Json<CreateUser>>) -> &'static str {
    "ok"
}

#[post("/test-no-body")]
async fn test_no_body_handler() -> &'static str {
    "ok"
}

#[tokio::test]
async fn test_json_body_request_schema_in_openapi() {
    let app = Rapina::new()
        .with_introspection(false)
        .discover()
        .openapi("test-api", "1.0");
    let client = TestClient::new(app).await;
    let response = client.get("/__rapina/openapi.json").send().await;
    let json: serde_json::Value = response.json();

    let path = &json["paths"]["/test-json-body"]["post"]["requestBody"];
    assert!(path.get("content").is_some(), "requestBody should be present");
    
    let schema = &path["content"]["application/json"]["schema"];
    assert!(schema.get("properties").is_some(), "schema should have properties");
    assert!(schema["properties"].get("email").is_some(), "should have email property");
    assert!(schema["properties"].get("name").is_some(), "should have name property");
}

#[tokio::test]
async fn test_validated_json_body_request_schema_in_openapi() {
    let app = Rapina::new()
        .with_introspection(false)
        .discover()
        .openapi("test-api", "1.0");
    let client = TestClient::new(app).await;
    let response = client.get("/__rapina/openapi.json").send().await;
    let json: serde_json::Value = response.json();

    let path = &json["paths"]["/test-validated-body"]["post"]["requestBody"];
    assert!(path.get("content").is_some(), "requestBody should be present for Validated<Json<T>>");
    
    let schema = &path["content"]["application/json"]["schema"];
    assert!(schema.get("properties").is_some(), "schema should have properties");
    assert!(schema["properties"].get("email").is_some(), "should have email property");
    assert!(schema["properties"].get("name").is_some(), "should have name property");
}

#[tokio::test]
async fn test_no_body_no_request_schema_in_openapi() {
    let app = Rapina::new()
        .with_introspection(false)
        .discover()
        .openapi("test-api", "1.0");
    let client = TestClient::new(app).await;
    let response = client.get("/__rapina/openapi.json").send().await;
    let json: serde_json::Value = response.json();

    let path = &json["paths"]["/test-no-body"]["post"]["requestBody"];
    assert!(path.is_null() || path.get("content").is_none(), "requestBody should NOT be present for no-body handler");
}
