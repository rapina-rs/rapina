+++
title = "Middleware"
description = "Rate limiting, compression, CORS, timeout, and custom middleware"
weight = 7
date = 2025-02-23
+++

Middleware are functions that intercept requests before they reach your handler and responses before they are sent to the client. In Rapina, middleware is composed as a chain — each piece can inspect, modify, or terminate the request lifecycle.

```rust
use rapina::prelude::*;
use rapina::middleware::{CorsConfig, CompressionConfig};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_cors(CorsConfig::permissive())
        .with_compression(CompressionConfig::default())
        .with_rate_limit(RateLimitConfig::per_minute(60))
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

## Rate Limiting (`with_rate_limit`)

The rate limiting middleware protects your API from abuse using the **token bucket** algorithm — each client receives a quota of tokens that refills continuously.

```rust
use rapina::prelude::*;

Rapina::new()
    .with_rate_limit(RateLimitConfig::per_minute(60))
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

When the limit is exceeded, Rapina responds with `429 Too Many Requests` and includes a `Retry-After` header indicating how many seconds the client should wait.

### Configuration

`RateLimitConfig` is available directly from `use rapina::prelude::*`.

| Constructor | Description |
|-------------|-------------|
| `RateLimitConfig::new(rps, burst)` | Rate in requests per second with burst capacity |
| `RateLimitConfig::per_minute(n)` | Shorthand: `n` requests per minute |

```rust
// 2 req/s with a burst of 10
RateLimitConfig::new(2.0, 10)

// 120 req/min (≈ 2 req/s, burst of 120)
RateLimitConfig::per_minute(120)
```

### Key extraction

By default, the limit is applied per client IP (read from `X-Forwarded-For` — leftmost entry — or `X-Real-IP`). Falls back to `"unknown"` if neither header is present.

Use `KeyExtractor::Custom` for different strategies, such as limiting per authenticated user:

```rust
use rapina::prelude::*;
use std::sync::Arc;

let config = RateLimitConfig::per_minute(100)
    .with_key_extractor(KeyExtractor::Custom(Arc::new(|req| {
        // Use the authenticated user ID as the key
        req.headers()
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("anonymous")
            .to_string()
    })));

Rapina::new()
    .with_rate_limit(config)
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

### Internal behavior

- Inactive client buckets are cleaned up every **1000 requests** if the bucket has been idle for more than **10 minutes**
- The `burst` field defines how many requests can be processed in a burst before the rate kicks in; new clients start with a full burst

---

## Response Compression (`with_compression`)

The compression middleware automatically reduces response sizes, negotiating the algorithm with the client via the `Accept-Encoding` header.

```rust
use rapina::prelude::*;
use rapina::middleware::CompressionConfig;

Rapina::new()
    .with_compression(CompressionConfig::default())
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

### Supported algorithms

| Algorithm | Header set |
|-----------|------------|
| Gzip | `Content-Encoding: gzip` |
| Deflate | `Content-Encoding: deflate` |

Gzip takes priority if the client accepts both.

### Configuration

```rust
use rapina::middleware::CompressionConfig;

// Default: minimum 1KB, level 6
CompressionConfig::default()

// Custom configuration
CompressionConfig::new(
    2048, // minimum size in bytes (2KB)
    9,    // compression level (0-9, clamped to max 9)
)
```

| Field | Default | Description |
|-------|---------|-------------|
| `min_size` | `1024` | Responses smaller than this value are not compressed |
| `level` | `6` | Compression level (0 = no compression, 9 = maximum) |

### When compression is applied

Compression is only applied when **all** of the following conditions are met:

1. The client sends `Accept-Encoding` with `gzip` or `deflate`
2. The response does not already have a `Content-Encoding` header
3. The `Content-Type` is compressible (text, JSON, XML, JavaScript) — responses with no `Content-Type` are also treated as compressible
4. The response body is larger than `min_size`
5. The compressed result is smaller than the original

The `Vary: Accept-Encoding` header is added automatically to ensure correct proxy caching.

---

## CORS (`with_cors`)

The CORS (Cross-Origin Resource Sharing) middleware controls which origins can access your API.

```rust
use rapina::prelude::*;
use rapina::middleware::CorsConfig;

Rapina::new()
    .with_cors(CorsConfig::permissive())
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

### Configuration modes

**`CorsConfig::permissive()`** — ideal for development, allows everything:

```rust
use rapina::middleware::CorsConfig;

// Allows any origin, method, and header
CorsConfig::permissive()
```

**`CorsConfig::with_origins(vec![...])`** — recommended for production:

```rust
use rapina::middleware::CorsConfig;

CorsConfig::with_origins(vec![
    "https://app.example.com".to_string(),
    "https://admin.example.com".to_string(),
])
```

With specific origins, the default allowed methods are `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, and `OPTIONS`, and the default allowed headers are `Accept` and `Authorization`.

### Advanced configuration

For full control, build the configuration manually:

```rust
use rapina::middleware::{CorsConfig, AllowedOrigins, AllowedMethods, AllowedHeaders};
use http::{header, Method};

let cors = CorsConfig {
    allowed_origins: AllowedOrigins::Exact(vec![
        "https://app.example.com".to_string(),
    ]),
    allowed_methods: AllowedMethods::List(vec![
        Method::GET,
        Method::POST,
        Method::DELETE,
    ]),
    allowed_headers: AllowedHeaders::List(vec![
        header::ACCEPT,
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
    ]),
};
```

### Preflight requests

`OPTIONS` requests are intercepted automatically and answered with a `204 No Content` response containing the correct CORS headers — they never reach your handler. The `Vary: Origin` header is added to all responses.

---

## Timeout, Body Limit, and Trace ID

These middleware are available out of the box. Register them via `.middleware()` like any other middleware:

```rust
use rapina::prelude::*;
use rapina::middleware::{TimeoutMiddleware, BodyLimitMiddleware, TraceIdMiddleware};
use std::time::Duration;

Rapina::new()
    .middleware(TraceIdMiddleware::new())
    .middleware(TimeoutMiddleware::new(Duration::from_secs(30)))
    .middleware(BodyLimitMiddleware::new(1024 * 1024)) // 1MB
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

### Timeout

Cancels requests that take longer than the configured limit and responds with `500 Internal Server Error`.

```rust
use rapina::middleware::TimeoutMiddleware;
use std::time::Duration;

// Default: 30 seconds
TimeoutMiddleware::default()

// Custom duration
TimeoutMiddleware::new(Duration::from_secs(10))
```

### Body Limit

Rejects requests whose `Content-Length` exceeds the limit with `400 Bad Request`.

```rust
use rapina::middleware::BodyLimitMiddleware;

// Default: 1MB (1,048,576 bytes)
BodyLimitMiddleware::default()

// Custom limit: 5MB
BodyLimitMiddleware::new(5 * 1024 * 1024)
```

Note: this check relies on the `Content-Length` header. Requests that omit this header and stream a large body bypass this middleware.

### Trace ID

Associates a unique identifier with each request to facilitate distributed tracing.

```rust
use rapina::middleware::TraceIdMiddleware;

TraceIdMiddleware::new()
```

Behavior:

- If the request includes an `x-trace-id` header (from an upstream system), that value is used and propagated
- Otherwise, a **UUID v4** is generated automatically
- The trace ID is injected into all response headers as `x-trace-id`

```
# Incoming request with an upstream trace ID
X-Trace-ID: 550e8400-e29b-41d4-a716-446655440000

# Response always echoes the trace ID back
X-Trace-ID: 550e8400-e29b-41d4-a716-446655440000
```

The trace ID is also available in your handlers via the `Context` extractor:

```rust
use rapina::prelude::*;

#[get("/")]
async fn handler(ctx: Context) -> String {
    format!("trace: {}", ctx.trace_id())
}
```

---

## Custom Middleware

Implement the `Middleware` trait to create your own middleware. Receive the request, do what you need, then call `next.run(req)` to continue the chain.

```rust
use rapina::prelude::*;
use rapina::middleware::BoxFuture;
use rapina::response::BoxBody;
use hyper::{Request, Response, body::Incoming};

pub struct ApiKeyMiddleware {
    valid_keys: Vec<String>,
}

impl Middleware for ApiKeyMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let api_key = req
                .headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok());

            match api_key {
                Some(key) if self.valid_keys.contains(&key.to_string()) => {
                    next.run(req).await
                }
                _ => Error::unauthorized("missing or invalid API key").into_response(),
            }
        })
    }
}
```

Register it with `.middleware()`:

```rust
Rapina::new()
    .middleware(ApiKeyMiddleware {
        valid_keys: vec!["secret-key-123".to_string()],
    })
    .discover()
    .listen("127.0.0.1:3000")
    .await
```

### Injecting data into the request

Use `req.extensions_mut()` to pass typed values to downstream handlers and middleware. The middleware injects the value; the handler receives it via a custom `FromRequestParts` implementation.

```rust
use rapina::prelude::*;
use rapina::middleware::BoxFuture;
use rapina::response::BoxBody;
use rapina::extract::{FromRequestParts, PathParams};
use rapina::state::AppState;
use hyper::{Request, Response, body::Incoming};
use http::request::Parts;
use std::sync::Arc;

// 1. Define the type to carry through the pipeline
#[derive(Clone)]
pub struct TenantId(pub String);

// 2. Implement FromRequestParts so handlers can receive it as a parameter
impl FromRequestParts for TenantId {
    async fn from_request_parts(
        parts: &Parts,
        _params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        parts
            .extensions
            .get::<TenantId>()
            .cloned()
            .ok_or_else(|| Error::internal("TenantId not set by middleware"))
    }
}

// 3. Middleware injects the value into extensions
pub struct TenantMiddleware;

impl Middleware for TenantMiddleware {
    fn handle<'a>(
        &'a self,
        mut req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let tenant_id = req
                .headers()
                .get("x-tenant-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("default")
                .to_string();

            req.extensions_mut().insert(TenantId(tenant_id));
            next.run(req).await
        })
    }
}
```

In the handler, use `TenantId` directly as a parameter:

```rust
use rapina::prelude::*;

#[get("/data")]
async fn handler(tenant: TenantId) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "tenant": tenant.0 }))
}
```

### Modifying the response

Middleware can also intercept and modify the response after the handler runs:

```rust
use rapina::prelude::*;
use rapina::middleware::BoxFuture;
use rapina::response::BoxBody;
use hyper::{Request, Response, body::Incoming};

pub struct SecurityHeadersMiddleware;

impl Middleware for SecurityHeadersMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            let mut response = next.run(req).await;

            let headers = response.headers_mut();
            headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
            headers.insert("X-Frame-Options", "DENY".parse().unwrap());
            headers.insert(
                "Strict-Transport-Security",
                "max-age=31536000; includeSubDomains".parse().unwrap(),
            );

            response
        })
    }
}
```

---

## Middleware ordering

Middleware in Rapina executes in **FIFO order** — the first middleware registered is the first to process the request and the last to process the response.

```
Request  →  [Middleware A]  →  [Middleware B]  →  [Middleware C]  →  Handler
Response ←  [Middleware A]  ←  [Middleware B]  ←  [Middleware C]  ←  Handler
```

**Note about authentication:** `.with_auth()` does not add middleware immediately at the call site. Auth middleware is always appended at the end of the stack during server startup (after all middleware registered via `.middleware()`, `.with_cors()`, `.with_rate_limit()`, and `.with_compression()`). This ensures route discovery has completed before auth is configured.

### Recommended order

Order middleware from outermost to innermost responsibility:

```rust
use rapina::prelude::*;
use rapina::middleware::{CorsConfig, CompressionConfig, TraceIdMiddleware};

Rapina::new()
    // 1. Trace ID first: all subsequent logs carry the request ID
    .middleware(TraceIdMiddleware::new())

    // 2. CORS: must answer preflights before any other processing
    .with_cors(CorsConfig::with_origins(vec![
        "https://app.example.com".to_string(),
    ]))

    // 3. Rate limiting: reject abuse before consuming resources
    .with_rate_limit(RateLimitConfig::per_minute(60))

    // 4. Compression: applied to all responses including error responses
    .with_compression(CompressionConfig::default())

    // 5. Application-specific custom middleware
    .middleware(SecurityHeadersMiddleware)

    // Auth is always appended last by the framework, regardless of
    // where .with_auth() appears in the chain
    .with_auth(auth_config)

    .discover()
    .listen("127.0.0.1:3000")
    .await
```

### Why order matters

| Middleware | Position | Reason |
|------------|----------|--------|
| Trace ID | First | All subsequent logs carry the request ID |
| CORS | Before rate limit | Answers `OPTIONS` preflights immediately, before any limit is consumed |
| Rate limiting | Before auth | Avoids JWT validation for clients that will be blocked anyway |
| Compression | After rate limit | Compresses the final response, including error bodies |
| Auth | Last (framework-managed) | Always added after all user-registered middleware |

---

## Complete example

```rust
use rapina::prelude::*;
use rapina::middleware::{BoxFuture, CorsConfig, CompressionConfig, TraceIdMiddleware};
use rapina::response::BoxBody;
use hyper::{Request, Response, body::Incoming};

// Custom audit middleware
pub struct AuditMiddleware;

impl Middleware for AuditMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        let method = req.method().to_string();
        let path = req.uri().path().to_string();
        let trace_id = ctx.trace_id.clone();

        Box::pin(async move {
            let response = next.run(req).await;
            let status = response.status().as_u16();

            tracing::info!(
                trace_id = %trace_id,
                method = %method,
                path = %path,
                status = status,
                "audit log"
            );

            response
        })
    }
}

#[get("/")]
async fn index() -> &'static str {
    "API is running!"
}

#[post("/items")]
async fn create_item(body: Json<serde_json::Value>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::CREATED, Json(body.into_inner()))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .middleware(TraceIdMiddleware::new())
        .with_cors(CorsConfig::with_origins(vec![
            "https://app.example.com".to_string(),
        ]))
        .with_rate_limit(RateLimitConfig::per_minute(60))
        .with_compression(CompressionConfig::default())
        .middleware(AuditMiddleware)
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```
