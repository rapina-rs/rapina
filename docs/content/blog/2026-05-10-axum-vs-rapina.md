+++
title = "Axum vs Rapina"
description = "Comparing Axum and Rapina from someone who ships on both."
date = 2026-05-10
slug = "axum-vs-rapina"

[taxonomies]
categories = ["essays"]
tags = ["axum", "comparison", "framework-design"]

[extra]
author = "Antonio Souza"
+++

I maintain Rapina. I also shipped on Axum. This post is the comparison I'd write for a friend asking which one to pick.

Axum is the right call for a lot of teams. Rapina is the right call for a different set of teams.

## Where they come from

Axum is the HTTP layer of the tokio project. It's built on hyper and tower, maintained by the same people who maintain the runtime your code runs on, and has been stable in production for years. The design philosophy is small, composable, unopinionated. Axum gives you a router, an extractor system, and integration with tower middleware. Everything else (auth, validation, OpenAPI, error envelopes) is left to the ecosystem.

Rapina is younger. Started in January 2026, currently at 0.11.0. It's also built on hyper, but not on tower. The shape is borrowed from FastAPI: convention over configuration, secure by default, batteries included. Where Axum hands you a kit, Rapina hands you a finished house with the option to remodel rooms.

Both are fast. Both are async. Both run on tokio. The choice between them is almost never about performance. It's about which tradeoffs you want to live with.

## How a route looks

Axum:

```rust
use axum::{routing::get, Router, extract::Path};

async fn get_user(Path(id): Path<u64>) -> String {
    format!("user {id}")
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/users/{id}", get(get_user));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

Rapina:

```rust
use rapina::prelude::*;

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> String {
    format!("user {}", id.into_inner())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

The proc macro is the obvious difference. Axum deliberately rejected attribute-based routing because it makes static analysis harder and ties handler signatures to the framework. Rapina embraces it because the convention pays for itself: routes get registered automatically, OpenAPI generation gets the path verbatim from the macro, and the file becomes self-documenting.

If you want every route declaration visible in one `Router::new()` call, Axum is closer to your taste. If you want routes attached to the handlers that own them and discovered automatically, Rapina is.

## The defaults question

Axum has no opinion about how you handle errors, what your error responses look like, whether requests get a trace ID, whether bodies are size-limited, whether timeouts apply, whether CORS is on, whether rate limiting exists. You wire up what you need from the tower ecosystem and the wider Rust crates index. The benefit is you get exactly what you choose, no more. The cost is that "exactly what you choose" needs to be chosen, and most teams underchoose at first and pay for it later.

Rapina starts with a stack. Trace IDs by default. Structured error envelopes with a `trace_id` field by default. Health endpoints by default. Body limits and request logging available with one builder call. The pitch is that most APIs need the same set of decisions, and Rapina makes those decisions for you.

```rust
Rapina::new()
    .with_cors(CorsConfig::permissive())
    .with_rate_limit(RateLimitConfig::per_minute(60))
    .with_request_log(RequestLogConfig::default())
    .discover()
    .listen("0.0.0.0:3000")
    .await
```

If your team has the senior bandwidth to assemble a stack from tower-http, governor, validator, utoipa, and friends, Axum gives you a cleaner result with less framework-shaped magic. If your team would rather inherit a sane stack and ship the actual product, Rapina is faster.

## Validation, errors, and the 422 problem

Axum's `Json<T>` extractor returns 400 on parse failure with a body that varies by type. There's no built-in way to enforce semantic validation (string length, regex, range checks) on top of deserialization. The standard answer is to combine `Json<T>` with the `validator` crate manually, return your own error type, and convert it to an `IntoResponse`.

Rapina ships `Validated<T>` as a first-class extractor:

```rust
#[derive(Deserialize, Validate, JsonSchema)]
struct CreateUser {
    #[validate(email)]
    email: String,
    #[validate(length(min = 8))]
    password: String,
}

#[post("/users")]
async fn create_user(payload: Validated<Json<CreateUser>>) -> StatusCode {
    let Validated(Json(_user)) = payload;
    StatusCode::CREATED
}
```

A failing payload returns a 422 with an envelope that lists field-level errors and a `trace_id`. The same shape is used everywhere in the framework, so clients only need to learn one error format. Axum reaches the same destination, but you pick the path.

Clients hitting an API that returns 400 for both "this isn't JSON" and "this is JSON but the email is missing an @" have to special-case responses. The 400/422 distinction is small but compounds: it makes client code easier to write and easier to test.

## OpenAPI and introspection

Axum has no built-in OpenAPI story. The dominant solution is `utoipa` plus a derive macro on each handler. It works, and once it's set up you barely notice it, but the setup is a real cost on day one and the macros are verbose.

Rapina derives OpenAPI from your handler signatures and the `JsonSchema` derive on your DTOs. The `#[get]` macro carries the path. The `Json<T>` and `Validated<T>` extractors carry the body shape. The return type carries the response shape. Together they produce a spec without you writing one.

Keeping an OpenAPI spec in sync with handler code by hand is a known source of drift, and the moment you have one client team consuming a spec from one server team, drift becomes a bug source.

There's also `rapina routes`, which prints the registered routes table from the CLI without starting the server. Useful for code review, useful for `rapina doctor` checks, useful for AI tools that want a structural view of an API.

## Middleware: tower vs Rapina-native

Axum's middleware story is tower's middleware story. Tower is the abstraction that powers half the Rust HTTP ecosystem, and once you understand it, you can compose middleware across hyper clients, axum servers, tonic gRPC servers, and anything else built on tower. The flip side is that tower has a steep learning curve. Writing a custom tower middleware involves three traits, a service factory, and at least one `Pin<Box<dyn Future>>` if you're not careful.

```rust
let app = Router::new()
    .route("/users", get(list_users))
    .layer(TraceLayer::new_for_http())
    .layer(TimeoutLayer::new(Duration::from_secs(10)));
```

Rapina's middleware is a single trait with one method. You implement `handle`, wrap the async body in `Box::pin(async move { ... })`, and call `next.run(req)` when ready to pass control downstream. No service factory, no `poll_ready` ritual, no two-trait split.

```rust
use rapina::middleware::{BoxFuture, Middleware, Next};
use rapina::context::RequestContext;
use rapina::response::BoxBody;
use hyper::body::Incoming;
use hyper::{Request, Response};

struct LogMiddleware;

impl Middleware for LogMiddleware {
    fn handle<'a>(
        &'a self,
        req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            tracing::info!(path = %req.uri().path(), "request");
            next.run(req).await
        })
    }
}
```

The cost: you give up access to the rest of the tower ecosystem unless you wrap things explicitly. Rapina exposes a `TowerLayerMiddleware` adapter for the cases that matter, but you're not getting tower-http for free.

If your team writes a lot of custom middleware, Rapina's API is friendlier. If your team mostly composes existing tower layers, Axum's interop is the bigger win.

## Auth posture

Axum has no built-in auth. The community has crates (`axum-login`, `tower-sessions`, JWT helpers), and the typical pattern is to write a custom extractor that pulls the user out of headers and rejects unauthenticated requests with 401.

Rapina inverts the default: every route requires JWT auth unless you mark it `#[public]`. Configuring auth is one builder call:

```rust
Rapina::new()
    .with_auth(AuthConfig::from_env().expect("JWT_SECRET must be set"))
    .discover()
    .listen("0.0.0.0:3000")
    .await
```

```rust
#[public]
#[get("/health")]
async fn health() -> &'static str {
    "ok"
}

#[get("/me")]
async fn me(user: CurrentUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "id": user.id }))
}
```

The cost of this default is you have to remember `#[public]` for endpoints that should be open. The benefit is you can't accidentally ship an unauthenticated `/admin` route because you forgot to attach the auth middleware.

For Axum, the same posture is achievable, but it's something you have to assemble. If you forget the layer on a sub-router, that sub-router is open. The framework can't tell you about it.

## The ecosystem question

Axum has years of production deployments at scale. There's a blog post for most problems you'll hit. There are examples of integrating axum with sea-orm, diesel, mongodb, and most of the other common stacks. Most Rust libraries that expose an HTTP integration ship an axum example. When you Google an axum problem, you usually find an answer.

Rapina is younger. The community is smaller. The Discord is active and the maintainers respond quickly, but you're not going to find seven blog posts about how to integrate Rapina with your specific weird use case. You'll often be the first person solving it.

## When Axum is the right call

You're building a library or SDK that exposes HTTP and you don't want to drag opinionated defaults into your users' code.

You need deep tower integration: custom layers, gRPC services on the same stack, hyper-level integration with another tokio-based system.

You want to assemble exactly the stack you need and you have the senior bandwidth to do it.

Your team already knows axum well and rewriting in something new costs more than it saves.

You're building something unusual: a proxy, a gateway, a custom protocol bridge, an L7 load balancer. Rapina's opinions get in the way of unusual shapes.

You care about ecosystem maturity above almost everything else, because the project will outlive your tenure on it.

## When Rapina is the right call

You're building a typical SaaS API. CRUD over JSON, auth, rate limits, error envelopes, OpenAPI for your frontend or partners, deployed to one of the obvious clouds.

Your team is small and you'd rather inherit a sane stack than assemble one. Convention over configuration is a productivity win when there's no architect on the team to make the configuration calls.

You want auth secure by default. Forgetting to mark a route open gets you a 401 on day one. Forgetting to mark one protected in Axum ships an open endpoint that no test will catch.

You're coming from FastAPI, NestJS, or Rails and the ergonomics gap to Axum feels too wide. Rapina narrows it considerably.

You're using AI-assisted coding heavily (Cursor, Claude Code, Copilot). In my own work I've noticed LLMs make fewer mistakes when the framework has strong conventions. Fewer valid paths means fewer chances to invent a wrong one.

You want OpenAPI generation that just works without a separate macro on every handler.

## Closing

If you want full control over the stack, Axum. If you want to ship a typical SaaS API quickly, Rapina. Both projects are healthy. The choice isn't a moral one.

The worst outcome is the team that picks one because they read a blog post that pitched it, then spends the next year fighting the choice instead of using the saved time to build the thing that matters.
