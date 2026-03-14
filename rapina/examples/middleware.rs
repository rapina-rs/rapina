//! Example demonstrating built-in middleware and writing a custom one.
//!
//! Run with: `cargo run --example middleware`
//!
//! Endpoints:
//! - GET /              — Hello world
//! - GET /data          — Returns JSON (large enough to trigger compression)
//! - GET /slow          — Simulates a slow response (2s delay)
//!
//! Try these:
//!
//! ```bash
//! # Basic request — observe x-trace-id and x-request-duration-ms headers
//! curl -v http://localhost:3000/
//!
//! # Compressed response — send Accept-Encoding to get gzip
//! curl -v -H 'Accept-Encoding: gzip' http://localhost:3000/data | gunzip
//!
//! # CORS preflight
//! curl -v -X OPTIONS http://localhost:3000/ \
//!   -H 'Origin: https://example.com' \
//!   -H 'Access-Control-Request-Method: GET'
//!
//! # Rate limiting — send many requests quickly
//! for i in $(seq 1 15); do curl -s -o /dev/null -w "%{http_code}\n" http://localhost:3000/; done
//! ```

use std::time::Duration;

use rapina::middleware::{
    BoxFuture, CompressionConfig, CorsConfig, RequestLogConfig, TimeoutMiddleware,
    TraceIdMiddleware,
};
use rapina::prelude::*;
use rapina::response::BoxBody;

// =============================================================================
// Custom middleware: RequestTimer
// =============================================================================

/// A custom middleware that measures request duration and adds it as a
/// response header (`x-request-duration-ms`).
///
/// This demonstrates how to implement the `Middleware` trait:
/// 1. Define a struct (with any config fields you need)
/// 2. Implement `Middleware` — call `next.run(req).await` and wrap the result
struct RequestTimerMiddleware;

impl Middleware for RequestTimerMiddleware {
    fn handle<'a>(
        &'a self,
        req: hyper::Request<hyper::body::Incoming>,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, hyper::Response<BoxBody>> {
        Box::pin(async move {
            // Run the rest of the middleware chain + handler
            let mut response = next.run(req).await;

            // Use the request context's built-in timer
            let duration_ms = ctx.elapsed().as_millis();
            response.headers_mut().insert(
                "x-request-duration-ms",
                http::HeaderValue::from_str(&duration_ms.to_string()).unwrap(),
            );

            response
        })
    }
}

// =============================================================================
// Handlers
// =============================================================================

#[get("/")]
async fn hello() -> &'static str {
    "Hello from the middleware example!"
}

/// Returns a large-ish JSON response so compression kicks in
/// (default min size is 1024 bytes).
#[get("/data")]
async fn data() -> Json<serde_json::Value> {
    let items: Vec<serde_json::Value> = (1..=50)
        .map(|i| {
            serde_json::json!({
                "id": i,
                "name": format!("Item {}", i),
                "description": format!("This is item number {} in our dataset", i),
            })
        })
        .collect();

    Json(serde_json::json!({ "items": items }))
}

/// Simulates a slow endpoint. With the 5s timeout middleware configured below,
/// this 2s delay will succeed — but anything over 5s would return a timeout error.
#[get("/slow")]
async fn slow() -> &'static str {
    tokio::time::sleep(Duration::from_secs(2)).await;
    "Done (took 2 seconds)"
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // -------------------------------------------------------------------------
    // Middleware ordering
    // -------------------------------------------------------------------------
    //
    // Middleware executes in the order it is registered — first added is the
    // outermost layer. A typical production ordering:
    //
    //   1. TraceId        — assign a trace ID early so all layers can use it
    //   2. RequestLog     — log the request (uses the trace ID from step 1)
    //   3. RequestTimer   — our custom middleware; measures total duration
    //   4. CORS           — handle preflight requests before other checks
    //   5. RateLimit      — reject excessive traffic before doing real work
    //   6. Compression    — compress responses on the way out
    //   7. Timeout        — cap how long a handler can run
    //
    // The request flows inward (1 → 7 → handler) and the response flows
    // back outward (handler → 7 → 1).

    let cors = CorsConfig::with_origins(vec![
        "https://example.com".to_string(),
        "https://app.example.com".to_string(),
    ]);

    let rate_limit = RateLimitConfig::per_minute(10);

    let compression = CompressionConfig::default();

    let timeout = TimeoutMiddleware::new(Duration::from_secs(5));

    println!();
    println!("  Rapina Middleware Example");
    println!("  ------------------------");
    println!();
    println!("  Server: http://127.0.0.1:3000");
    println!();
    println!("  Endpoints:");
    println!("    GET /       — hello world");
    println!("    GET /data   — large JSON (try with Accept-Encoding: gzip)");
    println!("    GET /slow   — 2s delay (timeout set to 5s)");
    println!();
    println!("  Middleware stack (outermost first):");
    println!("    1. TraceId");
    println!("    2. RequestLog");
    println!("    3. RequestTimer (custom)");
    println!("    4. CORS (origins: example.com, app.example.com)");
    println!("    5. RateLimit (10 req/min per IP)");
    println!("    6. Compression (gzip/deflate)");
    println!("    7. Timeout (5s)");
    println!();

    Rapina::new()
        // 1. Trace ID — reads or generates x-trace-id
        .middleware(TraceIdMiddleware::new())
        // 2. Request logging — headers, query, body size, with redaction
        .with_request_log(RequestLogConfig::verbose())
        // 3. Custom middleware — adds x-request-duration-ms header
        .middleware(RequestTimerMiddleware)
        // 4. CORS — allow specific origins
        .with_cors(cors)
        // 5. Rate limiting — 10 requests per minute per IP
        .with_rate_limit(rate_limit)
        // 6. Compression — gzip/deflate for responses over 1KB
        .with_compression(compression)
        // 7. Timeout — fail requests that take longer than 5 seconds
        .middleware(timeout)
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
