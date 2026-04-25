# Rapina Project

This is a Rust web application built with [Rapina](https://github.com/rapina-rs/rapina), an opinionated web framework.

## Rules

- `State<T>` must be the first handler argument, before `Path`, `Json`, `Query`, and `Validated`.
- Add `#[public]` to any route that must not require JWT. Omitting it returns 401 in production.
- Prefer `Validated<Json<T>>` over `Json<T>`. `Validated` returns 422 with field-level errors automatically.
- Return typed errors with `trace_id`. Never return plain strings or `String`-bodied responses from handlers.
- Run `rapina doctor` before concluding the app is misconfigured.

## Don't

- Don't import from `axum::` directly. Use `rapina::` re-exports.
- Don't hand-roll JSON error responses. Use the error envelope.
- Don't `.unwrap()` in handlers. Use `?` with typed errors.

## Handler pattern

Use proc macros for route registration. Handler names follow `verb_resource` convention:

```rust
#[get("/todos")]        async fn list_todos() -> ...
#[get("/todos/:id")]    async fn get_todo(id: Path<i32>) -> ...
#[post("/todos")]       async fn create_todo(body: Json<CreateTodo>) -> ...
#[put("/todos/:id")]    async fn update_todo(id: Path<i32>, body: Json<UpdateTodo>) -> ...
#[delete("/todos/:id")] async fn delete_todo(id: Path<i32>) -> ...
```

Routes require JWT by default. Use `#[public]` to opt out:

```rust
#[public]
#[post("/auth/login")]
async fn login(body: Json<LoginRequest>) -> Result<Json<TokenResponse>> { ... }
```

## Project structure (feature-first)

```
src/
├── main.rs          # App bootstrap
├── entity.rs        # Database entities (schema! macro)
├── migrations/      # Database migrations
└── todos/           # Feature module (always plural)
    ├── mod.rs
    ├── handlers.rs  # Route handlers
    ├── dto.rs       # Request/response types (Create*, Update*)
    └── error.rs     # Domain errors
```

## Builder pattern

```rust
Rapina::new()
    .with_tracing(TracingConfig::new())
    .middleware(RequestLogMiddleware::new())
    .with_cors(CorsConfig::permissive())
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

## CLI

- `rapina dev` — run with auto-reload
- `rapina doctor` — diagnose project issues
- `rapina routes` — list all registered routes
- `rapina add resource <name>` — scaffold a new CRUD resource
