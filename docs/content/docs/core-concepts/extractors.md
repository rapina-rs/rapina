+++
title = "Extractors"
description = "Parse request data with type safety"
weight = 2
date = 2025-02-13
+++

Extractors automatically parse request data and inject it into your handlers. If parsing fails, they return appropriate error responses.

## Available Extractors

| Extractor                        | Description                            |
| -------------------------------- | -------------------------------------- |
| [`Path<T>`](#path-parameters)    | URL path parameters                    |
| [`Query<T>`](#query-parameters)  | Query string parameters                |
| [`Json<T>`](#json-body)          | JSON request body                      |
| [`Form<T>`](#form-data)          | URL-encoded form data                  |
| [`Headers`](#headers)            | Request headers                        |
| [`State<T>`](#application-state) | Application state                      |
| [`Context`](#request-context)    | Request context (trace_id)             |
| [`Cookie<T>`](#cookies)          | Typed cookie access                    |
| [`CurrentUser`](#currentuser)    | Authenticated user (JWT)               |
| [`Validated<T>`](#validation)    | Validated extractor                    |
| [`Paginate`](#paginate)          | Pagination params (requires feature)   |
| [`Db`](#db)                      | Database connection (requires feature) |
| ['Multipart'](#multipart)        | Extractor for Multipart form data(requires feature) |

## Accessing Extractor Values

Every Rapina extractor implements `Deref` to its inner type. This means you can access fields and methods directly without unwrapping:

```rust
#[get("/users/:id")]
async fn get_user(id: Path<u64>, config: State<AppConfig>) -> String {
    // Deref lets you access fields directly
    format!("User {} on {}", *id, config.app_name)
}

#[post("/users")]
async fn create_user(body: Json<CreateUser>) -> String {
    // Access struct fields through the extractor
    format!("Hello, {}", body.name)
}
```

**When to use what:**

- **Direct field access** — `body.name`, `config.app_name`, `query.page`. Works anywhere you need `&T` thanks to auto-deref. This is the common case.
- **Explicit deref (`*`)** — `*id`, `*count`. Needed for primitives in format strings or when passing a `Copy` value where the compiler needs the concrete type.
- **`into_inner()`** — when you need to _own_ the value. Moving it into a struct, passing it to a function that takes `T` (not `&T`), or consuming it in a builder chain.

Avoid using `.0` to access extractor contents — it's an implementation detail. Deref or `into_inner()` are always clearer.

## Path Parameters

Extract values from URL path segments:

Path parameters are stored in a stack-allocated buffer — routes with up to 4 parameters incur zero heap allocation during extraction.

```rust
// Single parameter
#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> String {
    format!("User ID: {}", *id)
}

// Multiple parameters — destructure the tuple
#[get("/posts/:year/:month")]
async fn archive(Path((year, month)): Path<(u32, u32)>) -> String {
    format!("{}/{}", year, month)
}

// Named struct — parameters matched by field name
#[derive(Deserialize)]
struct PostParams {
    year: u32,
    month: u32,
    slug: String,
}

#[get("/posts/:year/:month/:slug")]
async fn get_post(Path(p): Path<PostParams>) -> String {
    format!("{}/{}/{}", p.year, p.month, p.slug)
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
    let page = query.page.unwrap_or(1);
    let limit = query.limit.unwrap_or(20);
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
    // Access fields directly through Deref
    let user = User::new(&body.name, &body.email);
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
    // Access fields directly through Deref
    authenticate(&form.username, &form.password).await
}
```

## Headers

Access all request headers with `Headers`:

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

When you know the header name upfront, prefer `Header<T>`. The name is derived
from the parameter name (snake_case to kebab-case) and the value is parsed into
`T`, so a missing header returns `400 MISSING_HEADER` and an unparseable one
returns `400 INVALID_HEADER` without any manual checks:

```rust
#[get("/user-agent")]
async fn show_user_agent(user_agent: Header<String>) -> String {
    // `user_agent` reads the `user-agent` header
    format!("User-Agent: {}", *user_agent)
}
```

Parsing happens through the `FromHeaderStr` trait, which always receives the raw
header value as `&str`. The impl for `String` owns that slice with `to_owned()`,
so `Header<String>` holds an allocated copy. You can borrow it back as `&str`
through `Deref` without consuming the extractor, or call `into_inner()` to take
the owned `String`:

```rust
#[get("/detect-client")]
async fn detect_client(user_agent: Header<String>) -> String {
    // Borrowed as `&str` via `Deref`, no allocation:
    if user_agent.starts_with("curl/") {
        return "cli client".to_string();
    }

    // Or take ownership of the parsed value:
    user_agent.into_inner()
}
```

Besides `String`, `FromHeaderStr` is implemented for `bool`, `uuid::Uuid`, and
the primitive integer and float types. Override the derived name with
`#[header("...")]`, and use `Option<Header<T>>` for optional headers:

```rust
#[get("/whoami")]
async fn whoami(
    #[header("x-api-key")] key: Header<String>,
    x_retry_count: Option<Header<u64>>,
) -> String {
    let retries = x_retry_count.map(|h| h.into_inner()).unwrap_or(0);

    format!("{} after {} retries", *key, retries)
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
    format!("App: {}", config.app_name)
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
    format!("Session: {}", session.session_id)
}
```

Returns 400 Bad Request if required cookies are missing or malformed.

## CurrentUser

Access the authenticated user from JWT claims:

```rust
#[get("/me")]
async fn me(user: CurrentUser) -> Json<UserResponse> {
    Json(UserResponse {
        id: user.id,
        email: user.claims.sub.clone(),
    })
}
```

The `CurrentUser` extractor provides:

- `user.id` - The user ID from the JWT `sub` claim
- `user.claims` - The full JWT claims

Returns 401 Unauthorized if the request lacks a valid JWT token.

> **Note:** This extractor requires authentication to be configured. See [Authentication](authentication.md) for setup details.

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
    // Validated also implements Deref — access fields directly
    let user = User::new(&body.email, &body.password);
    Json(user)
}
```

If validation fails, returns 422 with validation error details.

## Paginate

Parse pagination parameters from the query string:

```rust
use rapina::database::Db;

#[get("/users")]
async fn list_users(db: Db, page: Paginate) -> Result<Paginated<user::Model>> {
    page.exec(User::find(), db.conn()).await
}
```

The `Paginate` extractor reads `?page=1&per_page=20` from the query string:

| Parameter  | Default | Description             |
| ---------- | ------- | ----------------------- |
| `page`     | 1       | Page number (1-indexed) |
| `per_page` | 20      | Items per page          |

Returns 422 Validation Error when:

- `page` < 1
- `per_page` < 1
- `per_page` exceeds the configured maximum (default: 100)

> **Note:** This extractor requires the database feature. See [Pagination](pagination.md) for complete details and configuration.

## Db

Access the database connection for SeaORM operations:

```rust
use rapina::database::{Db, DbError};
use rapina::sea_orm::{EntityTrait, ActiveModelTrait, Set};

#[get("/posts")]
async fn list_posts(db: Db) -> Result<Json<Vec<PostResponse>>> {
    let posts = Post::find()
        .all(db.conn())
        .await
        .map_err(DbError::from)?;

    Ok(Json(posts.into_iter().map(PostResponse::from).collect()))
}

#[post("/posts")]
async fn create_post(body: Json<CreatePost>, db: Db) -> Result<Json<PostResponse>> {
    let post = post::ActiveModel {
        title: Set(body.title.clone()),
        content: Set(body.content.clone()),
        ..Default::default()
    };

    let post = post.insert(db.conn())
        .await
        .map_err(DbError::from)?;

    Ok(Json(PostResponse::from(post)))
}
```

The `Db` extractor provides:

- `db.conn()` - A reference to the SeaORM database connection

> **Note:** This extractor requires the database feature. See [Database](database.md) for setup and entity definitions.

## Multiple Extractors

You can use multiple extractors in a single handler. Body-consuming extractors (`Json`, `Form`, `Validated<Json<T>>`, `Validated<Form<T>>`) **must be the last parameter**:

```rust
#[post("/users/:id/posts")]
async fn create_post(
    id: Path<u64>,
    user: CurrentUser,
    body: Json<CreatePost>,  // body consumer must be last
) -> Result<Json<Post>> {
    // All extractors available
}
```

Parts-only extractors (`Path`, `Query`, `Headers`, `State`, `Context`, `Cookie`, `CurrentUser`, `Db`) can appear in any order before the last parameter.

> **Note:** Only one body-consuming extractor can be used per handler. If you need both JSON and form data, choose one.

## Multipart

Read `multipart/form-data` request bodies one field at a time:

```rust
use rapina::prelude::*;

#[post("/upload")]
async fn upload(mut multipart: Multipart) -> Result<String> {
    let mut result = String::new();

    while let Some(mut field) = multipart.next_field().await? {
        // Metadata borrows the field, so copy it out before reading the body.
        let name = field.name().unwrap_or("unknown").to_string();
        let file_name = field.file_name().map(|s| s.to_string());
        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        match file_name {
            // No filename: a plain form field, safe to decode as text.
            None => {
                let value = field.text().await?;
                result.push_str(&format!("Field: {name} = {value}\n"));
            }
            // A file: stream it so a large upload never lands in memory.
            Some(file_name) => {
                let mut total = 0;
                while let Some(chunk) = field.chunk().await? {
                    total += chunk.len();
                }
                result.push_str(&format!(
                    "File: {file_name} ({content_type}) = {total} bytes\n"
                ));
            }
        }
    }

    Ok(result)
}
```

The `Multipart` extractor provides:

- `next_field()` - Yields the next `Field`, or `None` at the end of the body

Each `Field` exposes three metadata accessors, available before the body is read:

- `name()` - The field name from `Content-Disposition`
- `file_name()` - The filename from `Content-Disposition`, or `None` for a plain form field
- `content_type()` - The field's `Content-Type`, when the client sent one

And three ways to read the body - pick exactly one per field:

- `chunk()` - Yields the next chunk, or `None` when the field is exhausted
- `bytes()` - Collects the whole field into a `Bytes`
- `text()` - Collects the whole field into a `String`

Use `file_name()` to tell the two kinds of field apart: a field with a filename is a
file and should stay opaque bytes, while a field without one is a form value. Do not
use `text()` to make that call.

`bytes()` and `text()` take the field by value and `chunk()` borrows it mutably

> **Note:** `bytes()` and `text()` buffer the entire field in memory, and
> `BodyLimitMiddleware` is opt-in, so there is no size ceiling by default. Prefer
> `chunk()` for uploads whose size you do not control.

> **Note:** This extractor requires the `multipart` feature.

