+++
title = "Extractors"
description = "Parse request data with type safety"
weight = 3
+++

Extractors automatically parse request data and inject it into your handlers. If parsing fails, they return appropriate error responses.

## Available Extractors

| Extractor | Description |
|-----------|-------------|
| `Path<T>` | URL path parameters |
| `Query<T>` | Query string parameters |
| `Json<T>` | JSON request body |
| `Form<T>` | URL-encoded form data |
| `Headers` | Request headers |
| `State<T>` | Application state |
| `Context` | Request context (trace_id) |
| `Cookie<T>` | Typed cookie access |
| `CurrentUser` | Authenticated user (JWT) |
| `Validated<T>` | Validated extractor |

## Path Parameters

Extract values from URL path segments:

```rust
#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> String {
    format!("User ID: {}", id.into_inner())
}

#[get("/posts/:year/:month")]
async fn archive(year: Path<u32>, month: Path<u32>) -> String {
    format!("{}/{}", year.into_inner(), month.into_inner())
}
```

## Query Parameters

Parse query strings into typed structs:

```rust
#[derive(Deserialize)]
struct Pagination {
    page: Option<u32>,
    limit: Option<u32>,
}

#[get("/users")]
async fn list_users(query: Query<Pagination>) -> String {
    let page = query.0.page.unwrap_or(1);
    let limit = query.0.limit.unwrap_or(20);
    format!("Page {} with {} items", page, limit)
}
```

## JSON Body

Parse JSON request bodies:

```rust
#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

#[post("/users")]
async fn create_user(body: Json<CreateUser>) -> Json<User> {
    let input = body.into_inner();
    // Create user...
    Json(user)
}
```

## Form Data

Parse URL-encoded form submissions:

```rust
#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

#[post("/login")]
async fn login(form: Form<LoginForm>) -> Result<Json<TokenResponse>> {
    let credentials = form.into_inner();
    // Authenticate...
}
```

## Headers

Access request headers:

```rust
#[get("/debug")]
async fn debug(headers: Headers) -> String {
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    format!("User-Agent: {}", user_agent)
}
```

## Application State

Access shared application state:

```rust
#[derive(Clone)]
struct AppConfig {
    app_name: String,
}

#[get("/info")]
async fn info(config: State<AppConfig>) -> String {
    format!("App: {}", config.into_inner().app_name)
}
```

## Cookies

Deserialize cookies into typed structs:

```rust
#[derive(Deserialize)]
struct Session {
    session_id: String,
}

#[get("/dashboard")]
async fn dashboard(session: Cookie<Session>) -> String {
    format!("Session: {}", session.into_inner().session_id)
}
```

Returns 400 Bad Request if required cookies are missing or malformed.

## Request Context

Access the request context with trace ID:

```rust
#[get("/trace")]
async fn trace(ctx: Context) -> String {
    format!("Trace ID: {}", ctx.trace_id())
}
```

## Validation

Validate extracted data using the `validator` crate:

```rust
use validator::Validate;

#[derive(Deserialize, Validate)]
struct CreateUser {
    #[validate(email)]
    email: String,

    #[validate(length(min = 8))]
    password: String,
}

#[post("/users")]
async fn create_user(body: Validated<Json<CreateUser>>) -> Json<User> {
    // body is guaranteed to be valid
    let input = body.into_inner().into_inner();
    // ...
}
```

If validation fails, returns 422 with validation error details.

## Multiple Extractors

You can use multiple extractors in a single handler:

```rust
#[post("/users/:id/posts")]
async fn create_post(
    id: Path<u64>,
    user: CurrentUser,
    body: Json<CreatePost>,
) -> Result<Json<Post>> {
    // All extractors available
}
```

> **Note:** Only one body-consuming extractor (`Json`, `Form`) can be used per handler.
