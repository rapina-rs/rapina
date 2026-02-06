<p align="center">
  <img src="docs/static/images/logo.png" alt="Rapina" width="120" />
</p>

<h1 align="center">Rapina</h1>

<p align="center">
  <strong>Predictable, auditable, and secure APIs — Easy to learn, hard to break.</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/rapina"><img src="https://img.shields.io/crates/v/rapina.svg" alt="Crates.io"></a>
  <a href="https://docs.rs/rapina"><img src="https://docs.rs/rapina/badge.svg" alt="Documentation"></a>
  <a href="https://github.com/arferreira/rapina/actions/workflows/ci.yml"><img src="https://github.com/arferreira/rapina/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://discord.gg/ttRYzbHh"><img src="https://img.shields.io/badge/Discord-Join-5865F2?logo=discord&logoColor=white" alt="Discord"></a>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
</p>

---

Rapina is a web framework for Rust inspired by FastAPI, focused on **productivity**, **type safety**, and **clear conventions**.

## Quick Start

```bash
cargo install rapina-cli
rapina new my-app
cd my-app
rapina dev
```

Or add to an existing project:

```toml
[dependencies]
rapina = "0.2.0"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

```rust
use rapina::prelude::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> String {
    format!("User ID: {}", id.into_inner())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/", hello)
        .get("/users/:id", get_user);

    Rapina::new()
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

## Why Rapina?

| Principle              | Description |
|------------------------|-------------|
| **Opinionated**        | Convention over configuration. Clear defaults, escape hatches when needed. |
| **Type-safe**          | Typed extractors, typed errors, everything checked at compile time. |
| **AI-friendly**        | Predictable patterns that humans and LLMs understand equally well. |
| **Batteries-included** | Standardized errors with `trace_id`, JWT auth, observability built-in. |

## Project Status

This project is currently in **Alpha** 🚧.

We are committed to **minimizing breaking changes** to ensure a smooth developer experience. However, until the `1.0.0`
release, major architectural changes may still happen if strictly necessary for long-term stability.

## Features

### Typed Extractors

Clean, type-safe parameter extraction:

```rust
#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> Result<Json<User>> {
    let user = find_user(id.into_inner()).await?;
    Ok(Json(user))
}

#[post("/users")]
async fn create_user(body: Json<CreateUser>) -> Result<Json<User>> {
    let user = save_user(body.into_inner()).await?;
    Ok(Json(user))
}

#[get("/search")]
async fn search(query: Query<SearchParams>) -> Json<Vec<Item>> {
    let results = search_items(&query).await;
    Json(results)
}
```

Available extractors: `Path`, `Json`, `Query`, `Form`, `Headers`, `State`, `CurrentUser`

### Configuration

Type-safe configuration with fail-fast validation:

```rust
#[derive(Config)]
struct Settings {
    #[env = "DATABASE_URL"]
    database_url: String,

    #[env = "PORT"]
    #[default = "3000"]
    port: u16,
}

fn main() {
    load_dotenv();
    let config = Settings::from_env().expect("Missing config");
}
```

### Authentication

Protected by default — all routes require JWT unless marked `#[public]`:

```rust
#[public]
#[post("/login")]
async fn login(body: Json<LoginRequest>, auth: State<AuthConfig>) -> Result<Json<TokenResponse>> {
    let token = auth.create_token(&body.username)?;
    Ok(Json(TokenResponse::new(token, auth.expiration())))
}

#[get("/me")]
async fn me(user: CurrentUser) -> Json<UserResponse> {
    Json(UserResponse { id: user.id })
}
```

```rust
Rapina::new()
    .with_auth(AuthConfig::from_env()?)
    .public_route("POST", "/login")
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

### Standardized Errors

Every error includes a `trace_id` for debugging:

```json
{
  "error": { "code": "NOT_FOUND", "message": "user not found" },
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

```rust
Error::bad_request("invalid input")   // 400
Error::unauthorized("login required") // 401
Error::not_found("user not found")    // 404
Error::validation("invalid email")    // 422
Error::internal("something went wrong") // 500
```

### OpenAPI

Automatic OpenAPI 3.0 generation with CLI tools:

```bash
rapina openapi export -o openapi.json  # Export spec
rapina openapi check                    # Verify spec matches code
rapina openapi diff --base main         # Detect breaking changes
```

### Rate Limiting

Protect your API from abuse with token bucket rate limiting:

```rust
Rapina::new()
    .with_rate_limit(RateLimitConfig::per_minute(100))
    .router(router)
    .listen("127.0.0.1:3000")
    .await
```

Returns `429 Too Many Requests` with `Retry-After` header when exceeded.

### CLI

```bash
rapina new my-app          # Create new project
rapina dev                 # Dev server with hot reload
rapina routes              # List all routes
rapina doctor              # Health checks
```

## Documentation

Full documentation available at [userapina.com](https://userapina.com/)

- [Getting Started](https://userapina.com/guide/getting-started/)
- [Configuration](https://userapina.com/guide/configuration/)
- [Authentication](https://userapina.com/guide/authentication/)
- [CLI Reference](https://userapina.com/cli/)

## Philosophy

Rapina is opinionated by design: a clear happy path, with escape hatches when needed.

| Principle | Description |
|-----------|-------------|
| **Predictability** | Clear conventions, obvious structure |
| **Auditability** | Typed contracts, traceable errors |
| **Security** | Protected by default, guard rails built-in |
| **AI-friendly** | Patterns that LLMs can understand and generate |

## License

MIT
