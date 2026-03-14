# Rapina Project Instructions

This is a Rust web application built with the Rapina framework.

## Framework Overview

Rapina is an opinionated Rust web framework built on hyper. Routes are protected by default (JWT auth) unless marked `#[public]`. All response types must derive `Serialize` + `JsonSchema` for OpenAPI generation. Error responses always include a `trace_id`.

## Conventions

### Adding a new endpoint

1. Create or edit the handler in `src/<feature>/handlers.rs`
2. Use the proc macro: `#[get("/path")]`, `#[post("/path")]`, `#[put("/path")]`, `#[delete("/path")]`
3. Mark public routes with `#[public]` above the method macro
4. Use `#[errors(ErrorType)]` to document error responses for OpenAPI
5. If using `.discover()`, the route is auto-registered. Otherwise add it to the router in `main.rs`

### Extractors (in handler function signatures)

```rust
// Body (only one per handler)
body: Json<T>              // JSON body, T: Deserialize + JsonSchema
body: Validated<Json<T>>   // JSON body with validation, T: Deserialize + JsonSchema + Validate
body: Form<T>              // Form data

// Parts (multiple allowed)
id: Path<i32>              // URL path param (:id syntax)
params: Query<T>           // Query string
headers: Headers           // Full header map
state: State<T>            // App state
user: CurrentUser          // Authenticated user (id, claims)
ctx: Context               // Request context (trace_id, start_time)
db: Db                     // Database connection (requires database feature)
jar: Cookie<T>             // Cookie values
```

### Handler naming convention
- `list_<resources>` — GET collection
- `get_<resource>` — GET single item
- `create_<resource>` — POST
- `update_<resource>` — PUT
- `delete_<resource>` — DELETE

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

### Error handling pattern

Each feature module has its own error type:

```rust
// src/todos/error.rs
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

impl DocumentedError for TodoError {
    fn error_variants() -> Vec<ErrorVariant> {
        vec![
            ErrorVariant { status: 404, code: "NOT_FOUND", description: "Todo not found" },
            ErrorVariant { status: 500, code: "DATABASE_ERROR", description: "Database operation failed" },
        ]
    }
}
```

Use `Error::not_found()`, `Error::bad_request()`, `Error::unauthorized()`, etc. for quick errors.

### Project structure

Feature-first modules. Each feature directory is plural:

```
src/todos/handlers.rs    # not src/handlers/todos.rs
src/todos/dto.rs         # CreateTodo, UpdateTodo structs
src/todos/error.rs       # TodoError enum
src/todos/mod.rs         # pub mod dto; pub mod error; pub mod handlers;
```

Top-level shared files:
- `src/entity.rs` — all database entities via `schema!` macro
- `src/migrations/` — database migrations via `migrations!` macro

### DTOs
- Request types: `Create<Resource>`, `Update<Resource>` — derive `Deserialize` + `JsonSchema`
- Response types: derive `Serialize` + `JsonSchema`
- Update DTOs wrap fields in `Option<T>` for partial updates

### Testing

```rust
use rapina::testing::TestClient;

#[tokio::test]
async fn test_hello() {
    let app = Rapina::new().router(router);
    let client = TestClient::new(app).await;

    let res = client.get("/").send().await;
    assert_eq!(res.status(), StatusCode::OK);

    let body: MessageResponse = res.json();
    assert_eq!(body.message, "Hello from Rapina!");
}
```

`TestClient` supports `.get()`, `.post()`, `.put()`, `.delete()`, `.patch()`. Request builder has `.json()`, `.header()`, `.body()`. Response has `.status()`, `.json::<T>()`, `.text()`.

## Build & Run

```bash
rapina dev              # development with auto-reload
cargo build --release   # production build
rapina doctor           # check for common issues
rapina routes           # list all routes
```
