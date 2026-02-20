<p align="center">
  <img src="docs/static/images/logo.png" alt="Rapina" width="120" />
</p>

<h1 align="center">Rapina</h1>

<p align="center">
  <strong>A Rust web framework for APIs. So simple it feels like cheating.</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/rapina"><img src="https://img.shields.io/crates/v/rapina.svg" alt="Crates.io"></a>
  <a href="https://docs.rs/rapina"><img src="https://docs.rs/rapina/badge.svg" alt="Documentation"></a>
  <a href="https://github.com/arferreira/rapina/actions/workflows/ci.yml"><img src="https://github.com/arferreira/rapina/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://discord.gg/Z4ww64YBQj"><img src="https://img.shields.io/badge/Discord-Join-5865F2?logo=discord&logoColor=white" alt="Discord"></a>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
</p>

---

```rust
use rapina::prelude::*;

#[get("/users")]
async fn list_users(db: Db) -> Result<Json<Vec<User>>> {
    let users = User::find_all(db.conn()).await?;
    Ok(Json(users))
}

#[post("/users")]
async fn create_user(input: Validated<Json<CreateUser>>, db: Db) -> Result<Json<User>> {
    let user = User::create(db.conn(), input.into_inner()).await?;
    Ok(Json(user))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .discover()
        .listen("0.0.0.0:3000")
        .await
}
```

No router configuration. No manual wiring. Annotate your handlers, call `.discover()`, ship.

## Get started

```bash
cargo install rapina-cli
rapina new my-app
cd my-app
rapina dev
```

Your API is running. That's it.

## What you get

**Auto-discovery** — annotate handlers with `#[get]`, `#[post]`, `#[put]`, `#[delete]`. Rapina finds them at startup. Your `main.rs` stays three lines.

**Database from day one** — define your schema declaratively. `author: User` becomes a foreign key. `posts: Vec<Post>` becomes a relationship. SeaORM entities are auto-generated.

```rust
schema! {
    User {
        name: String,
        email: String,
        posts: Vec<Post>
    }

    Post {
        title: String,
        body: String,
        author: User
    }
}
```

**Auth that works** — JWT authentication, protected by default. Mark public routes with `#[public]`. Access the current user with the `CurrentUser` extractor.

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

**CRUD in one command** — scaffold an entire resource with handlers, DTOs, errors, schema, and migration.

```bash
rapina add resource Post
```

**OpenAPI built-in** — spec generation, breaking change detection, and validation from the CLI.

```bash
rapina openapi export -o openapi.json
rapina openapi diff --base main
```

**Production middleware** — rate limiting, compression, CORS, and Prometheus metrics out of the box.

```rust
Rapina::new()
    .with_rate_limit(RateLimitConfig::per_minute(100))
    .with_compression(CompressionConfig::default())
    .with_cors(CorsConfig::permissive())
    .discover()
    .listen("0.0.0.0:3000")
    .await
```

**CLI for everything** — create, develop, test, inspect, generate.

```bash
rapina new my-app              # Scaffold a new project
rapina dev                     # Dev server with hot reload
rapina test --coverage         # Tests with coverage report
rapina test --watch            # Watch mode
rapina routes                  # List all routes
rapina doctor                  # Health checks and diagnostics
rapina migrate new             # Create a migration
rapina add resource Post       # Full CRUD scaffolding
```

## 10 extractors

Everything you need to pick apart a request, type-safe and compile-time checked.

`Json<T>` · `Form<T>` · `Path<T>` · `Query<T>` · `Headers` · `Cookie<T>` · `State<T>` · `Validated<T>` · `CurrentUser` · `Db`

## Standardized errors

Every error includes a `trace_id`. No more guessing in production.

```json
{
  "error": { "code": "NOT_FOUND", "message": "user not found" },
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

## Project status

Rapina is in active development. We ship fast and we ship often — 9 releases since January 2026. The API is stabilizing but breaking changes may still occur before `1.0.0`.

See the [roadmap](https://userapina.com/roadmap/) for what's coming next.

## Documentation

Full documentation at [userapina.com](https://userapina.com/)

- [Getting Started](https://userapina.com/guide/getting-started/)
- [Database](https://userapina.com/guide/database/)
- [Authentication](https://userapina.com/guide/authentication/)
- [CLI Reference](https://userapina.com/cli/)

## Contributing

Contributions are welcome. Check out the [open issues](https://github.com/arferreira/rapina/issues) or join us on [Discord](https://discord.gg/Z4ww64YBQj).


## License

MIT
