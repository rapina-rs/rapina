+++
title = "What is Rapina?"
weight = 1
date = 2025-02-13
+++

Rapina is a Rust web framework for building APIs. Convention over configuration, protected by default, type-safe everywhere.

```rust
use rapina::prelude::*;

#[public]
#[get("/health")]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}
```

That's a complete handler. The `#[public]` attribute opts out of authentication — without it, every route requires a valid JWT. Let's look at what that means.

## Protected by Default

Most frameworks make you add authentication. Rapina makes you remove it. Every route requires a valid JWT unless explicitly marked `#[public]`:

```rust
// This route requires authentication — no attribute needed
#[get("/me")]
async fn me(user: CurrentUser) -> Json<UserResponse> {
    Json(UserResponse {
        id: user.sub.clone(),
        email: user.email.clone(),
    })
}

// This route is public — you have to opt in
#[public]
#[post("/auth/login")]
async fn login(body: Json<LoginRequest>) -> Result<Json<TokenResponse>> {
    // ...
}
```

If someone hits a protected route without a token, they get a `401` with a structured error. No middleware to configure, no guards to forget.

## Validation Built In

Request validation is a first-class citizen. Wrap your extractor in `Validated<T>` and Rapina handles the rest:

```rust
use validator::Validate;

#[derive(Deserialize, Validate, JsonSchema)]
struct CreateUser {
    #[validate(email)]
    email: String,

    #[validate(length(min = 8))]
    password: String,
}

#[post("/users")]
async fn create_user(body: Validated<Json<CreateUser>>) -> Result<Json<UserResponse>> {
    let user = body.into_inner();
    // user is already validated — if we got here, the data is good
    // ...
}
```

Invalid requests never reach your handler. The client gets a `422` with every validation error at once:

```json
{
  "error": "Validation failed",
  "code": "VALIDATION_ERROR",
  "trace_id": "req_abc123",
  "details": {
    "email": ["must be a valid email"],
    "password": ["must be at least 8 characters"]
  }
}
```

## Errors That Make Sense

Every error response follows the same shape. No guessing what format this endpoint returns when something goes wrong:

```json
{
  "error": "User not found",
  "code": "USER_NOT_FOUND",
  "trace_id": "req_def456"
}
```

The `trace_id` is generated per request. When a user reports a bug, you search your logs for that trace ID and see exactly what happened.

## Ship in Seconds

The CLI gets you from zero to running API:

```bash
cargo install rapina-cli
rapina new my-app
cd my-app
rapina dev
```

Once you're building, `rapina doctor` catches misconfigurations before production, and `rapina routes` shows every registered endpoint:

```
GET    /health     [public]
POST   /auth/login [public]
GET    /me
POST   /users
```

## OpenAPI for Free

Call `.openapi()` on your app and Rapina generates a full OpenAPI spec from your route macros and types. No annotations, no separate YAML files:

```rust
Rapina::new()
    .discover()
    .openapi("/docs/openapi.json")
    .listen("127.0.0.1:3000")
    .await
```

Your types are your documentation. `JsonSchema` derives on request and response structs flow directly into the generated spec.

## Built on Solid Foundations

Rapina is built directly on battle-tested crates:

- **Hyper** for HTTP handling
- **Tokio** for async runtime
- **SeaORM** for database operations (optional)

No layers of abstraction. Maximum control, maximum performance.
