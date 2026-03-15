# Rapina Project

This is a Rust web application built with [Rapina](https://github.com/rapina-rs/rapina), an opinionated web framework.

## Key Conventions

### Routes are protected by default
All routes require JWT authentication unless explicitly marked with `#[public]`:

```rust
#[public]
#[get("/health")]
async fn health() -> &'static str { "ok" }

// This route requires a valid JWT token
#[get("/me")]
async fn me(user: CurrentUser) -> Json<UserResponse> { ... }
```

### Handler pattern
Use proc macros for route registration. Handler names follow `verb_resource` convention:

```rust
#[get("/todos")]       async fn list_todos() -> ...
#[get("/todos/:id")]   async fn get_todo(id: Path<i32>) -> ...
#[post("/todos")]      async fn create_todo(body: Json<CreateTodo>) -> ...
#[put("/todos/:id")]   async fn update_todo(id: Path<i32>, body: Json<UpdateTodo>) -> ...
#[delete("/todos/:id")] async fn delete_todo(id: Path<i32>) -> ...
```

### Typed extractors
- `Json<T>` — request/response body (T must derive Serialize and/or Deserialize + JsonSchema)
- `Path<T>` — URL path parameter (`:id` syntax)
- `Query<T>` — query string parameters
- `State<T>` — shared application state
- `Validated<Json<T>>` — JSON body with validation (T must also derive Validate, returns 422 on failure)
- `CurrentUser` — authenticated user identity (requires auth to be configured)
- `Db` — database connection (requires database feature)

### Error handling
Return `Result<Json<T>>` from handlers. Use typed errors:

```rust
pub enum TodoError {
    DbError(DbError),
}

impl IntoApiError for TodoError {
    fn into_api_error(self) -> Error {
        match self {
            TodoError::DbError(e) => e.into_api_error(),
        }
    }
}
```

All error responses include a `trace_id` for debugging:
```json
{
  "error": { "code": "NOT_FOUND", "message": "Todo 42 not found" },
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

### Project structure (feature-first)
```
src/
├── main.rs          # App bootstrap with builder pattern
├── entity.rs        # Database entities (schema! macro)
├── migrations/      # Database migrations
└── todos/           # Feature module (always plural)
    ├── mod.rs
    ├── handlers.rs  # Route handlers
    ├── dto.rs       # Request/response types
    └── error.rs     # Domain errors
```

### Builder pattern
```rust
Rapina::new()
    .with_tracing(TracingConfig::new())
    .middleware(RequestLogMiddleware::new())
    .with_cors(CorsConfig::permissive())
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

## CLI Commands
- `rapina dev` — run with auto-reload
- `rapina doctor` — diagnose project issues
- `rapina routes` — list all registered routes
- `rapina add resource <name>` — scaffold a new CRUD resource
