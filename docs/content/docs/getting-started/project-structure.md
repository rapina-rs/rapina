+++
title = "Project Structure"
description = "Understanding the project layout created by rapina new"
weight = 2
date = 2025-02-13
+++

When you run `rapina new my-app`, the CLI creates this structure:

```
my-app/
├── Cargo.toml
├── .gitignore
├── README.md
└── src/
    └── main.rs
```

That's it. One source file, no nested module tree, no framework boilerplate to wade through before you write your first handler. As your app grows, you add structure — Rapina doesn't force it upfront.

## main.rs

The entry point sets up the app and registers routes:

```rust
use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct MessageResponse {
    message: String,
}

#[derive(Serialize, JsonSchema)]
struct HealthResponse {
    status: String,
}

#[public]
#[get("/")]
async fn hello() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Hello from Rapina!".into(),
    })
}

#[public]
#[get("/health")]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

The `.discover()` call automatically finds all handlers annotated with route macros (`#[get]`, `#[post]`, etc.) — no manual router registration needed.

## Growing Your App

As your API grows, organize by domain, not by layer. This is the feature-first convention:

```
src/
├── main.rs
├── config.rs
├── users/
│   ├── mod.rs        # handlers
│   ├── models.rs     # DTOs, request/response types
│   └── errors.rs     # domain-specific errors
├── items/
│   ├── mod.rs
│   ├── models.rs
│   └── errors.rs
└── migrations/
    ├── mod.rs
    └── m20240101_000001_create_items.rs
```

Everything related to users lives in `src/users/`. Everything related to items lives in `src/items/`. When someone new joins the team, they don't need a mental map of the whole codebase to work on one feature.

Compare this to the layer-first approach you might be used to:

```
# Don't do this
src/
├── handlers/
│   ├── users.rs
│   └── items.rs
├── models/
│   ├── users.rs
│   └── items.rs
└── errors/
    ├── users.rs
    └── items.rs
```

Layer-first scatters related code across the tree. Adding a feature means touching three directories. Feature-first keeps it contained.

## Templates

The CLI offers three templates to match your starting point:

`rapina new my-app` — the default. Two routes, no database, minimal dependencies.

`rapina new my-app --template crud` — adds SQLite, a sample `items` module with full CRUD handlers, and a database migration.

`rapina new my-app --template auth` — adds JWT authentication with login and register endpoints, a `.env.example` for secrets, and a protected `/me` route.

Pick the one closest to what you're building and delete what you don't need.

## Conventions

A few naming patterns that keep Rapina projects consistent:

**Routes** are plural and versioned: `/v1/users`, `/v1/users/:id`.

**Handlers** are verb + resource: `create_user`, `list_users`, `get_user`, `delete_user`.

**Request types** describe the action: `CreateUserRequest`, `UpdateItemRequest`.

**Response types** describe the result: `UserResponse`, `ItemListResponse`.

These aren't enforced by the framework — they're conventions that make code predictable across projects.
