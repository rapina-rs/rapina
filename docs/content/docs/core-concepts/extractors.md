+++
title = "Extractors"
description = "Parse request data with type safety"
weight = 2
date = 2025-02-13
+++

Extractors automatically parse request data and inject it into your handlers. If parsing fails, they return appropriate error responses.

## Available Extractors

| Extractor | Description |
|-----------|-------------|
| [`Path<T>`](#path-parameters) | URL path parameters |
| [`Query<T>`](#query-parameters) | Query string parameters |
| [`Json<T>`](#json-body) | JSON request body |
| [`Form<T>`](#form-data) | URL-encoded form data |
| [`Headers`](#headers) | Request headers |
| [`State<T>`](#application-state) | Application state |
| [`Context`](#request-context) | Request context (trace_id) |
| [`Cookie<T>`](#cookies) | Typed cookie access |
| [`CurrentUser`](#currentuser) | Authenticated user (JWT) |
| [`Validated<T>`](#validation) | Validated extractor |
| [`Paginate`](#paginate) | Pagination params (requires feature) |
| [`Db`](#db) | Database connection (requires feature) |

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
- **`into_inner()`** — when you need to *own* the value. Moving it into a struct, passing it to a function that takes `T` (not `&T`), or consuming it in a builder chain.

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

| Parameter | Default | Description |
|-----------|---------|-------------|
| `page` | 1 | Page number (1-indexed) |
| `per_page` | 20 | Items per page |

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
