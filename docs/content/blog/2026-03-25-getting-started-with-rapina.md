+++
title = "Getting Started with Rapina"
description = "Build your first Rust API from scratch — installation, routes, and a running server in under 10 minutes"
date = 2026-03-25

[taxonomies]
categories = ["tutorials"]
tags = ["getting-started", "rust", "api"]

[extra]
author = "Ricardo Uemura"
+++

Rapina is a Rust web framework designed to make building APIs fast and predictable. In this tutorial you'll go from zero to a running server with multiple routes, typed JSON responses, automatic error handling, and an OpenAPI spec — all in under 10 minutes.

**Prerequisites:** Rust 1.75+ installed ([rustup.rs](https://rustup.rs/))

---

## Install the CLI

The Rapina CLI handles scaffolding, development, and code generation.

```bash
cargo install rapina-cli
```

Verify it installed:

```bash
rapina --version
```

---

## Create a new project

```bash
rapina new my-app
cd my-app
```

The CLI creates a minimal project:

```
my-app/
├── Cargo.toml
├── .gitignore
└── src/
    └── main.rs
```

Open `src/main.rs`. You'll see one handler (`hello`) already wired up to `GET /`. That's your starting point.

---

## Start the development server

```bash
rapina dev
```

The server starts at `http://127.0.0.1:3000` with hot reload. Every time you save a file it rebuilds and restarts automatically.

Verify it's working:

```bash
curl http://127.0.0.1:3000/
```

```json
{"message": "Hello from Rapina!"}
```

You're up. Now let's build something real.

---

## Add your first route

You'll add a `GET /users` endpoint that returns a list of users.

Open `src/main.rs` and replace the contents with:

```rust
use rapina::prelude::*;
use rapina::middleware::RequestLogMiddleware;
use rapina::schemars;

// --- Types ---

#[derive(Serialize, JsonSchema)]
struct MessageResponse {
    message: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

// --- Handlers ---

#[get("/")]
async fn hello() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Hello from Rapina!".into(),
    })
}

#[get("/users")]
async fn list_users() -> Json<Vec<User>> {
    let users = vec![
        User { id: 1, name: "Alice".into(), email: "alice@example.com".into() },
        User { id: 2, name: "Bob".into(), email: "bob@example.com".into() },
    ];
    Json(users)
}

// --- Server ---

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_tracing(TracingConfig::new())
        .middleware(RequestLogMiddleware::new())
        .with_health_check(true)
        .openapi("My App", "0.1.0")
        .router(
            Router::new()
                .get("/", hello)
                .get("/users", list_users),
        )
        .listen("127.0.0.1:3000")
        .await
}
```

A few things to notice:

- `#[derive(Serialize, Deserialize, JsonSchema)]` makes the struct serializable to JSON and registers it in the OpenAPI spec automatically.
- `#[get("/users")]` registers the handler at `GET /users`. No separate router annotation needed — the handler function itself is the declaration.
- Routes are registered explicitly in `Router::new()`. This keeps the wiring visible and easy to follow.

Save the file. The server rebuilds in the background.

Test it:

```bash
curl http://127.0.0.1:3000/users
```

```json
[
  {"id": 1, "name": "Alice", "email": "alice@example.com"},
  {"id": 2, "name": "Bob", "email": "bob@example.com"}
]
```

---

## Add a route with a path parameter

Add `GET /users/:id` to retrieve a single user. Add this handler below `list_users`:

```rust
#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>> {
    let users = vec![
        User { id: 1, name: "Alice".into(), email: "alice@example.com".into() },
        User { id: 2, name: "Bob".into(), email: "bob@example.com".into() },
    ];

    let user = users
        .into_iter()
        .find(|u| u.id == *id)
        .ok_or_else(|| Error::not_found(format!("user {} not found", *id)))?;

    Ok(Json(user))
}
```

Then register it in `main`:

```rust
.router(
    Router::new()
        .get("/", hello)
        .get("/users", list_users)
        .get("/users/:id", get_user),
)
```

What's new here:

- `Path<u64>` extracts the `:id` segment from the URL as a typed `u64`. If the value can't be parsed, Rapina returns a `400` automatically — no validation code needed.
- The return type `Result<Json<User>>` lets the handler propagate errors with `?`.
- `Error::not_found(...)` produces a `404` response with a structured JSON body including a `trace_id`.

Test it:

```bash
curl http://127.0.0.1:3000/users/1
```

```json
{"id": 1, "name": "Alice", "email": "alice@example.com"}
```

```bash
curl http://127.0.0.1:3000/users/99
```

```json
{
  "error": {"code": "NOT_FOUND", "message": "user 99 not found"},
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

Every error from Rapina includes a `trace_id`. You can correlate a user-reported error directly to a log entry without any extra tooling.

---

## Add a POST route

Add `POST /users` to accept a new user:

```rust
#[derive(Deserialize, JsonSchema)]
struct CreateUser {
    name: String,
    email: String,
}

#[post("/users")]
async fn create_user(body: Json<CreateUser>) -> Json<User> {
    // In a real app, you'd persist this to a database.
    Json(User {
        id: 42,
        name: body.name.clone(),
        email: body.email.clone(),
    })
}
```

Register it:

```rust
.router(
    Router::new()
        .get("/", hello)
        .get("/users", list_users)
        .get("/users/:id", get_user)
        .post("/users", create_user),
)
```

Test it:

```bash
curl -X POST http://127.0.0.1:3000/users \
  -H "Content-Type: application/json" \
  -d '{"name": "Carol", "email": "carol@example.com"}'
```

```json
{"id": 42, "name": "Carol", "email": "carol@example.com"}
```

If the body is missing or has the wrong shape, Rapina returns a `422` with details — no extra validation code needed.

---

## Inspect your routes

```bash
rapina routes
```

```
GET    /
GET    /__rapina/health
GET    /users
GET    /users/:id
POST   /users
```

All registered routes visible at a glance.

---

## Export the OpenAPI spec

Rapina generates an OpenAPI spec from your code. No YAML to write, no annotations to maintain separately.

```bash
rapina openapi export -o openapi.json
```

The `User` and `CreateUser` structs became JSON Schema definitions automatically because of `#[derive(JsonSchema)]`. Every route, parameter, and response type is documented.

---

## The full file

Here's `src/main.rs` at the end of the tutorial:

```rust
use rapina::prelude::*;
use rapina::middleware::RequestLogMiddleware;
use rapina::schemars;

#[derive(Serialize, JsonSchema)]
struct MessageResponse {
    message: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[derive(Deserialize, JsonSchema)]
struct CreateUser {
    name: String,
    email: String,
}

#[get("/")]
async fn hello() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Hello from Rapina!".into(),
    })
}

#[get("/users")]
async fn list_users() -> Json<Vec<User>> {
    let users = vec![
        User { id: 1, name: "Alice".into(), email: "alice@example.com".into() },
        User { id: 2, name: "Bob".into(), email: "bob@example.com".into() },
    ];
    Json(users)
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>> {
    let users = vec![
        User { id: 1, name: "Alice".into(), email: "alice@example.com".into() },
        User { id: 2, name: "Bob".into(), email: "bob@example.com".into() },
    ];

    let user = users
        .into_iter()
        .find(|u| u.id == *id)
        .ok_or_else(|| Error::not_found(format!("user {} not found", *id)))?;

    Ok(Json(user))
}

#[post("/users")]
async fn create_user(body: Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 42,
        name: body.name.clone(),
        email: body.email.clone(),
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_tracing(TracingConfig::new())
        .middleware(RequestLogMiddleware::new())
        .with_health_check(true)
        .openapi("My App", "0.1.0")
        .router(
            Router::new()
                .get("/", hello)
                .get("/users", list_users)
                .get("/users/:id", get_user)
                .post("/users", create_user),
        )
        .listen("127.0.0.1:3000")
        .await
}
```

---

## What you built

| Route | Handler | Notes |
|-------|---------|-------|
| `GET /` | `hello` | Root endpoint |
| `GET /users` | `list_users` | Returns a typed `Vec<User>` |
| `GET /users/:id` | `get_user` | Path param, typed `404` on miss |
| `POST /users` | `create_user` | JSON body, `422` on bad input |

And for free: structured error responses with trace IDs, an OpenAPI spec, request logging, and a health check at `/__rapina/health`.

---

Want to keep going? Check out the [tutorial series](/tutorial) for deeper dives into authentication, validation, database integration, and more.
