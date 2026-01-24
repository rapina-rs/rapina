# Rapina

[![Crates.io](https://img.shields.io/crates/v/rapina.svg)](https://crates.io/crates/rapina)
[![Documentation](https://docs.rs/rapina/badge.svg)](https://docs.rs/rapina)
[![CI](https://github.com/arferreira/rapina/actions/workflows/ci.yml/badge.svg)](https://github.com/arferreira/rapina/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

> Predictable, auditable, and secure APIs — written by humans, accelerated by AI.

Rapina is a web framework for Rust inspired by FastAPI, focused on **productivity**, **type safety**, and **clear conventions**.

## Installation

Add Rapina to your `Cargo.toml`:

```toml
[dependencies]
rapina = "0.1.0-alpha.3"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

Or use the CLI to create a new project:

```bash
cargo install rapina-cli
rapina new my-app
cd my-app
rapina dev
```

## Why Rapina?

- **Opinionated** — Convention over configuration. 90% of apps require 10% of decisions.
- **Type-safe** — Typed extractors, typed errors, everything checked at compile time.
- **AI-friendly** — Predictable structure that humans and models understand.
- **Production-ready** — Standardized errors with `trace_id`, ready for observability.

## Quick Start

```rust
use rapina::prelude::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/", |_, _, _| async { "Hello, Rapina!" })
        .get("/users/:id", |_, params, _| async move {
            let id = params.get("id").cloned().unwrap_or_default();
            format!("User ID: {}", id)
        });

    Rapina::new()
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

## Features

### CLI Tools

Rapina comes with a powerful CLI for development:

```bash
# Install the CLI
cargo install rapina-cli

# Create a new project
rapina new my-app

# Start development server with hot reload
rapina dev

# Custom port and host
rapina dev -p 8080 --host 0.0.0.0

# OpenAPI tools
rapina openapi export -o openapi.json  # Export spec
rapina openapi check                    # Verify spec is up to date
rapina openapi diff --base main         # Detect breaking changes

# Introspection
rapina routes                           # List all registered routes
rapina doctor                           # Run API health checks
```

### Typed Extractors

```rust
// Path parameters
Router::new().get("/users/:id", |_, params, _| async move {
    let id = params.get("id").cloned().unwrap_or_default();
    format!("User: {}", id)
});

// JSON body
Router::new().post("/users", |req, _, _| async move {
    use http_body_util::BodyExt;
    let body = req.into_body().collect().await.unwrap().to_bytes();
    let user: User = serde_json::from_slice(&body).unwrap();
    Json(user)
});

// Query parameters
Router::new().get("/search", |req, _, _| async move {
    let query = req.uri().query().unwrap_or("");
    let params: SearchParams = serde_urlencoded::from_str(query).unwrap();
    Json(params)
});

// Application state
Router::new().get("/config", |_, _, state| async move {
    let config = state.get::<AppConfig>().unwrap();
    format!("DB: {}", config.db_url)
});
```

Available extractors: `Json`, `Path`, `Query`, `Form`, `Headers`, `State`, `Context`

### Middleware

```rust
use rapina::middleware::{TimeoutMiddleware, BodyLimitMiddleware, TraceIdMiddleware};
use std::time::Duration;

Rapina::new()
    .middleware(TraceIdMiddleware::new())
    .middleware(TimeoutMiddleware::new(Duration::from_secs(30)))
    .middleware(BodyLimitMiddleware::new(1024 * 1024)) // 1MB
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

### Standardized Errors

Every error returns a consistent envelope with `trace_id`:

```json
{
  "error": {
    "code": "NOT_FOUND",
    "message": "user not found"
  },
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

Built-in error constructors:

```rust
Error::bad_request("invalid input")      // 400
Error::unauthorized("login required")    // 401
Error::forbidden("access denied")        // 403
Error::not_found("user not found")       // 404
Error::conflict("already exists")        // 409
Error::validation("invalid email")       // 422
Error::rate_limited("too many requests") // 429
Error::internal("something went wrong")  // 500
```

### Domain Errors

Define typed domain errors with automatic API conversion:

```rust
use rapina::prelude::*;

enum UserError {
    NotFound(u64),
    EmailTaken(String),
}

impl IntoApiError for UserError {
    fn into_api_error(self) -> Error {
        match self {
            UserError::NotFound(id) => Error::not_found(format!("user {} not found", id)),
            UserError::EmailTaken(email) => Error::conflict(format!("email {} taken", email)),
        }
    }
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>, UserError> {
    let user = find_user(id).ok_or(UserError::NotFound(id))?;
    Ok(Json(user))
}
```

Benefits:
- Type-safe error handling
- Automatic conversion with `?` operator
- Consistent API responses
- Self-documenting code

### Route Introspection

Enable introspection to see all registered routes:

```rust
Rapina::new()
    .with_introspection(true)  // Enabled by default in debug builds
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

Access routes at `http://localhost:3000/__rapina/routes`:

```json
[
  {"method": "GET", "path": "/", "handler_name": "hello"},
  {"method": "GET", "path": "/users/:id", "handler_name": "get_user"}
]
```

### OpenAPI Specification

Rapina automatically generates OpenAPI 3.0 specs from your code:

```rust
#[derive(Serialize, JsonSchema)]  // Add JsonSchema for response schemas
struct User {
    id: u64,
    name: String,
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Json<User> {
    // ...
}

Rapina::new()
    .openapi("My API", "1.0.0")  // Enable OpenAPI
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

Access the spec at `http://localhost:3000/__rapina/openapi.json`

**CLI Tools for API Contract Management:**

```bash
# Export OpenAPI spec to file
rapina openapi export -o openapi.json

# Check if committed spec matches current code
rapina openapi check

# Detect breaking changes against another branch
rapina openapi diff --base main
```

**Breaking change detection:**

```bash
$ rapina openapi diff --base main

  → Comparing OpenAPI spec with main branch...

  ✗ Breaking changes:
    • Removed endpoint: /health
    • Removed method: DELETE /users/{id}

  ⚠ Non-breaking changes:
    • Added endpoint: /posts
    • Added field 'avatar' in GET /users/{id}

Error: Found 2 breaking change(s)
```

The `openapi.json` file becomes your API contract — commit it to your repo and CI will catch breaking changes before they're merged.

### Testing

Built-in test client for integration testing:

```rust
use rapina::testing::TestClient;

#[tokio::test]
async fn test_hello() {
    let app = Rapina::new()
        .router(Router::new().get("/", |_, _, _| async { "Hello!" }));

    let client = TestClient::new(app).await;
    let response = client.get("/").send().await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.text(), "Hello!");
}
```

### Application State

```rust
#[derive(Clone)]
struct AppConfig {
    db_url: String,
}

Rapina::new()
    .state(AppConfig { db_url: "postgres://...".to_string() })
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

## Roadmap

- [x] Basic router with path parameters
- [x] Extractors (`Json`, `Path`, `Query`, `Form`, `Headers`, `State`, `Context`)
- [x] Standardized error handling with `trace_id`
- [x] Middleware system (`Timeout`, `BodyLimit`, `TraceId`)
- [x] Dependency Injection / State
- [x] Request context with tracing
- [x] Route introspection endpoint
- [x] Test client for integration testing
- [x] CLI (`rapina new`, `rapina dev`)
- [x] Automatic OpenAPI with response schemas
- [x] OpenAPI CLI tools (`export`, `check`, `diff`)
- [ ] Validation (`Validated<T>`)
- [ ] Auth (Bearer JWT, `CurrentUser`)
- [ ] Observability (tracing, structured logs)

## Philosophy

Rapina is opinionated by design: a clear happy path, with escape hatches when needed.

| Principle | Description |
|-----------|-------------|
| Predictability | Clear conventions, obvious structure |
| Auditability | Typed contracts, traceable errors |
| Security | Guard rails by default |

## License

MIT
