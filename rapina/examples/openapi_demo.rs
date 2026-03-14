use rapina::prelude::*;
use rapina::extract::Validated;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, Validate, JsonSchema)]
struct CreateUser {
    #[validate(email)]
    email: String,
    #[validate(length(min = 8))]
    name: String,
}

#[post("/users")]
async fn create_user(body: Validated<Json<CreateUser>>) -> Json<CreateUser> {
    println!("Creating user: {:?}", body.0.0);
    body.into_inner()
}

#[get("/health")]
async fn health() -> &'static str {
    "OK"
}

#[tokio::main]
async fn main() {
    // 1. Initialize the app
    // 2. Discover routes
    // 3. Configure OpenAPI with a CUSTOM path
    // 4. Enable Scalar UI
    let app = Rapina::new()
        .discover()
        .openapi("OpenAPI Demo API", "1.0.0")
        .with_openapi_path("/api/v1/spec.json") // Showcase custom path
        .with_scalar("/docs");                  // Showcase Scalar UI

    println!("Starting server on http://127.0.0.1:3000");
    println!("OpenAPI Spec: http://127.0.0.1:3000/api/v1/spec.json");
    println!("Scalar Docs: http://127.0.0.1:3000/docs");

    app.listen("127.0.0.1:3000").await.unwrap();
}
