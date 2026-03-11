//! Example demonstrating request validation with `Validated<T>`.
//!
//! Run with: `cargo run --example validation`
//!
//! Test endpoints:
//! - GET  /users - List users
//! - POST /users - Create a user (validated JSON body)
//!
//! Sending invalid data to POST /users returns 422 Unprocessable Entity:
//!
//! ```bash
//! curl -X POST http://localhost:3000/users -H 'Content-Type: application/json' \
//!   -d '{"email":"bad","password":"short","age":10}'
//! ```
//!
//! ```json
//! {
//!   "error": {
//!     "code": "VALIDATION_ERROR",
//!     "message": "validation failed",
//!     "details": {
//!       "name": [{ "code": "length", "message": null, "params": { "min": 1, "max": 50, "value": "" } }],
//!       "email": [{ "code": "email", "message": null, "params": { "value": "bad" } }],
//!       "password": [{ "code": "length", "message": null, "params": { "min": 8, "max": 128, "value": "short" } }],
//!       "age": [{ "code": "range", "message": null, "params": { "min": 18.0, "max": 150.0, "value": 10 } }]
//!     }
//!   }
//! }
//! ```

use rapina::prelude::*;

#[derive(Deserialize, Validate)]
struct CreateUser {
    #[validate(length(min = 1, max = 50))]
    name: String,
    #[validate(email)]
    email: String,
    #[validate(length(min = 8, max = 128))]
    password: String,
    #[validate(range(min = 18, max = 150))]
    age: u32,
}

#[derive(Serialize, JsonSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[post("/users")]
async fn create_user(body: Validated<Json<CreateUser>>) -> Json<User> {
    Json(User {
        id: 1,
        name: body.name.clone(),
        email: body.email.clone(),
    })
}

#[get("/users")]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![
        User {
            id: 1,
            name: "Alice".to_string(),
            email: "alice@test.com".to_string(),
        },
        User {
            id: 2,
            name: "Bob".to_string(),
            email: "bob@test.com".to_string(),
        },
    ])
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/users", list_users)
        .post("/users", create_user);

    println!("Endpoints:");
    println!("  GET  /users");
    println!("  POST /users");

    Rapina::new().router(router).listen("127.0.0.1:3000").await
}
