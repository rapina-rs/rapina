+++
title = "Dependency Injection"
description = "Rapina AppState and dependency injections methods"
weight = 3
date = 2026-03-06
+++

Rapina uses a type-safe container called `AppState` to share services across handlers. You register values once at startup — database pools, config structs, HTTP clients — and inject them anywhere via the `State<T>` extractor.

`AppState` is backed by a `HashMap<TypeId, Arc<dyn Any + Send + Sync>>`. Each type gets one slot, keyed by its Rust type identity:

```
AppState
  TypeId(AppConfig)          -> Arc<AppConfig>
  TypeId(EmailClient)        -> Arc<EmailClient>
  TypeId(DatabaseConnection) -> Arc<DatabaseConnection>
  TypeId(...)                -> Arc<...>
```

At startup the state is wrapped in an `Arc` and shared across all requests. Per request, only the `Arc` is cloned — no data is ever copied.

## State

### Registering State

Call `.state()` for each service you want to share. The only requirement is that the type is `Send + Sync + 'static` — the value is immediately wrapped in `Arc`, so `Clone` is not required:

```rust
use rapina::prelude::*;

struct AppConfig {
    name: String,
    base_url: String,
}

#[get("/info")]
async fn info(config: State<AppConfig>) -> String {
    config.name.clone()
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .state(AppConfig {
            name: "my-api".to_string(),
            base_url: "https://api.example.com".to_string(),
        })
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

### Multiple State Types

Each type is stored independently. Inject as many as needed per handler:

```rust
struct AppConfig { base_url: String }

struct EmailClient { api_key: String }

#[post("/invite")]
async fn send_invite(
    user: CurrentUser,
    config: State<AppConfig>,
    email: State<EmailClient>,
    body: Json<InviteRequest>,
) -> Result<()> {
    let url = format!("{}/invite/{}", config.base_url, body.token);
    email.send(&body.address, &url).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .state(AppConfig { base_url: "https://api.example.com".to_string() })
        .state(EmailClient { api_key: std::env::var("EMAIL_KEY").unwrap() })
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

### Trait Objects

`.state()` requires a concrete type at registration time. When you want handlers to depend on an **interface** rather than an implementation, use `.state_arc()` instead.

`.state_arc()` accepts a pre-built `Arc<T>` where `T` can be an unsized trait object (`Arc<dyn MyTrait>`), with no newtype wrapper required:

```rust
use rapina::prelude::*;
use std::sync::Arc;

trait UserRepo: Send + Sync {
    async fn find_all(&self) -> Vec<User>;
}

struct PgUserRepo { /* pool */ }

impl UserRepo for PgUserRepo {
    async fn find_all(&self) -> Vec<User> { /* ... */ }
}

#[get("/users")]
async fn list_users(repo: State<Arc<dyn UserRepo>>) -> Json<Vec<User>> {
    Json(repo.find_all().await)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let repo: Arc<dyn UserRepo> = Arc::new(PgUserRepo { /* pool */ });

    Rapina::new()
        .state_arc(repo)
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

Handlers receive `State<Arc<dyn UserRepo>>`. The `Deref` chain (`State` → `Arc` → `dyn UserRepo`) lets you call trait methods directly without any extra unwrapping.

### Mutable Shared State

`AppState` already wraps every value in `Arc`, so nothing is copied per-request. When you need to **mutate** state at runtime — not just read it — use `Arc<RwLock<T>>` or `Arc<Mutex<T>>` for interior mutability:

```rust
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

struct TodoStore(Arc<RwLock<HashMap<String, Todo>>>);

impl TodoStore {
    fn new() -> Self {
        Self(Arc::new(RwLock::new(HashMap::new())))
    }
}

#[get("/todos")]
async fn list_todos(store: State<TodoStore>) -> Json<Vec<Todo>> {
    let todos = store.0.read().unwrap().values().cloned().collect();
    Json(todos)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .state(TodoStore::new())
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

### Missing State

If a handler requests `State<T>` but `T` was never registered, the request returns `500 Internal Server Error`:

```
State not registered for type 'my_crate::AppConfig'. Did you forget to call .state() or .state_arc()?
```

### Overwriting State

Calling `.state()` twice with the same type silently overwrites the first value — no error or warning is emitted:

```rust
Rapina::new()
    .state(AppConfig { name: "first".to_string() })
    .state(AppConfig { name: "second".to_string() }) // "first" is gone
```

Each type has exactly one slot in the container.

---

## Graceful Shutdown

When the server receives `SIGINT` or `SIGTERM`, it stops accepting new connections and waits for in-flight requests to finish. Use `.shutdown_timeout()` to control how long it waits, and `.on_shutdown()` to register async cleanup hooks.

Hooks run after connections drain (or the timeout expires), in the order they were registered.

**Closing a database pool on shutdown:**

```rust
use std::time::Duration;

let pool = build_db_pool().await;
let pool_for_shutdown = pool.clone();

Rapina::new()
    .state(pool)
    .shutdown_timeout(Duration::from_secs(30))
    .on_shutdown(move || async move {
        pool_for_shutdown.close().await;
        tracing::info!("database pool closed");
    })
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

`.state(pool)` consumes `pool` by value, so a handle is cloned beforehand. Both point to the same underlying data via `Arc` — the hook uses `pool_for_shutdown` to run cleanup before the state is dropped.

**Multiple hooks run in order:**

```rust
Rapina::new()
    .shutdown_timeout(Duration::from_secs(15))
    .on_shutdown(|| async { tracing::info!("step 1: draining queue") })
    .on_shutdown(|| async { tracing::info!("step 2: closing db pool") })
    .on_shutdown(|| async { tracing::info!("step 3: flushing metrics") })
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

The default timeout is 30 seconds. After the timeout, remaining connections are dropped and shutdown proceeds.

---

## Health Checks

Enable built-in health endpoints with `.enable_health_check()`:

```rust
Rapina::new()
    .enable_health_check()
    .listen("127.0.0.1:3000")
    .await
```

When the value comes from config or environment, use `.with_health_check(bool)` to keep the builder chain intact:

```rust
let cfg = Config::from_env();

Rapina::new()
    .with_health_check(cfg.health_check_enabled)
    .listen("127.0.0.1:3000")
    .await
```

This registers three endpoints:

| Endpoint | Purpose |
|---|---|
| `GET /__rapina/health` | Alias for `/ready` — simple setups and load balancers |
| `GET /__rapina/health/live` | Kubernetes liveness probe — always `200` |
| `GET /__rapina/health/ready` | Kubernetes readiness probe — runs all checks |

Point your Kubernetes probes at the dedicated endpoints:

```yaml
livenessProbe:
  httpGet:
    path: /__rapina/health/live
    port: 3000

readinessProbe:
  httpGet:
    path: /__rapina/health/ready
    port: 3000
```

The liveness probe **never** checks external dependencies — a DB outage should pull the pod from the load balancer (readiness failure), not restart it (liveness failure).

Custom checks

`.enable_health_check()` registers the HTTP endpoints. `.add_health_check()` registers a function that runs inside those endpoints — they do different things:

- **`.enable_health_check()`** (or `.with_health_check(bool)` for dynamic config) — turns on the `/__rapina/health` routes. Without it, those paths return 404.
- **`.add_health_check("name", fn)`** — adds a dependency check (Redis, Stripe, etc.) that runs on every `/ready` request. Optional — the endpoint works fine without any custom checks.

```rust
Rapina::new()
    .enable_health_check()                          // or: .with_health_check(cfg.health_check_enabled)
    .add_health_check("redis", || async {
        redis_ping().await.is_ok()
    })
    .add_health_check("stripe", || async {
        stripe_ping().await.is_ok()
    })
    .listen("127.0.0.1:3000")
    .await
```

When all checks pass the response is `{"status": "ok"}`. When any check fails, the status is `503` and the body identifies which checks failed:

```json
{
  "status": "error",
  "checks": {
    "db": "ok",
    "redis": "error"
  }
}
```

---

## Going Further

- [Database](@/docs/core-concepts/database.md) — `.with_database()`, the `Db` extractor, and migrations
- [Middleware](@/docs/core-concepts/middleware.md) — CORS, rate limiting, compression, caching, and custom middleware
- [Authentication](@/docs/core-concepts/authentication.md) — JWT with `.with_auth()` and `#[public]` routes
- [Metrics](@/docs/core-concepts/metrics.md) — Prometheus scraping with `.with_metrics()`
- [OpenAPI](@/docs/core-concepts/openapi.md) — generated spec with `.openapi()`
- [WebSocket](@/docs/core-concepts/websockets.md) — real-time push with `.with_relay()` and the `Relay` extractor

---

## Complete Example

A production-ready setup combining state, database, auth, middleware, observability, and graceful shutdown:

```rust
use rapina::prelude::*;
use rapina::auth::AuthConfig;
use rapina::database::{DatabaseConfig, Db};
use rapina::middleware::{CorsConfig, RateLimitConfig};
use rapina::observability::TracingConfig;
use std::time::Duration;

struct AppConfig {
    app_name: String,
    frontend_url: String,
}

// Public routes — no token required
#[post("/auth/login")]
#[public]
async fn login(body: Json<LoginRequest>) -> Result<Json<TokenResponse>> {
    // validate credentials and issue JWT...
}

// Protected routes
#[get("/users")]
async fn list_users(db: Db, _user: CurrentUser) -> Result<Json<Vec<User>>> {
    let users = UserEntity::find().all(db.conn()).await?;
    Ok(Json(users))
}

#[get("/me")]
async fn me(user: CurrentUser, config: State<AppConfig>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "id": user.id,
        "app": config.app_name,
    }))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = AppConfig {
        app_name: "my-api".to_string(),
        frontend_url: "https://app.example.com".to_string(),
    };

    let frontend_url = config.frontend_url.clone();

    Rapina::new()
        // Observability (init first so all startup logs are captured)
        .with_tracing(TracingConfig::default())
        // Application state
        .state(config)
        // Database
        .with_database(DatabaseConfig::from_env()?).await?
        .run_migrations::<migrations::Migrator>().await?
        // Middleware
        .with_cors(CorsConfig::with_origins(vec![frontend_url]))
        .with_rate_limit(RateLimitConfig::per_minute(200))
        // Auth — #[public] handlers are exempted automatically
        .with_auth(AuthConfig::from_env()?)
        // OpenAPI + metrics
        .openapi("My API", "1.0.0")
        .with_metrics(true)
        // Graceful shutdown
        .shutdown_timeout(Duration::from_secs(30))
        .on_shutdown(|| async {
            tracing::info!("shutting down gracefully");
        })
        // Routes
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```
