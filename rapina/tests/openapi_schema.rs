use rapina::extract::Validated;
use rapina::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, Validate, JsonSchema)]
struct CreateUser {
    #[validate(email)]
    email: String,
    #[validate(length(min = 8))]
    name: String,
}

#[post("/users")]
async fn create_user(_body: Validated<Json<CreateUser>>) -> Json<CreateUser> {
    _body.into_inner()
}

#[tokio::test]
async fn test_openapi_request_schema_validated_json() {
    let app = Rapina::new()
        .discover()
        .openapi("Test API", "1.0.0")
        .with_openapi_path("/api-spec.json");

    let client = rapina::testing::TestClient::new(app).await;
    let response = client.get("/api-spec.json").send().await;

    assert_eq!(response.status(), 200);
    let spec: Value = response.json();

    // Check if the path exists in the spec
    let path_item = spec
        .get("paths")
        .and_then(|p| p.get("/users"))
        .expect("Path /users not found in spec");
    let post_op = path_item
        .get("post")
        .expect("POST operation not found for /users");

    // Check if request body schema is present and correct
    let request_body = post_op
        .get("requestBody")
        .expect("requestBody not found in spec");
    let content = request_body
        .get("content")
        .and_then(|c| c.get("application/json"))
        .expect("JSON content type not found in requestBody");
    let schema = content
        .get("schema")
        .expect("Schema not found in requestBody content");

    // Verify it's an inline schema for CreateUser (simplified check)
    assert_eq!(
        schema.get("type"),
        Some(&Value::String("object".to_string()))
    );
    assert!(
        schema
            .get("properties")
            .and_then(|p| p.get("email"))
            .is_some()
    );
    assert!(
        schema
            .get("properties")
            .and_then(|p| p.get("name"))
            .is_some()
    );
}

#[tokio::test]
async fn test_scalar_ui_with_custom_openapi_path() {
    let app = Rapina::new()
        .discover()
        .openapi("Test API", "1.0.0")
        .with_openapi_path("/my/custom/spec.json")
        .with_scalar("/docs");

    let client = rapina::testing::TestClient::new(app).await;

    // 1. Verify custom spec path is working
    let spec_resp = client.get("/my/custom/spec.json").send().await;
    assert_eq!(spec_resp.status(), 200);

    // 2. Verify Scalar UI contains the custom path
    let docs_resp = client.get("/docs").send().await;
    assert_eq!(docs_resp.status(), 200);
    let html = docs_resp.text();

    assert!(
        html.contains("data-url=\"/my/custom/spec.json\""),
        "Scalar HTML should contain the custom OpenAPI path"
    );
}
