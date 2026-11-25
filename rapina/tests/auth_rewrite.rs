//! Publicness is resolved from the request as it reaches the auth middleware,
//! so auth and the router agree after earlier middleware rewrites it.

use http::{Method, StatusCode};
use hyper::{Request, Response, body::Incoming};
use rapina::middleware::{BoxFuture, Middleware, Next};
use rapina::prelude::*;
use rapina::response::BoxBody;
use rapina::testing::TestClient;

mod users {
    use rapina::prelude::*;

    #[public]
    #[get("/users/:id")]
    pub async fn users_show() -> &'static str {
        "user"
    }

    #[get("/users/me")]
    pub async fn users_me() -> &'static str {
        "me"
    }

    #[post("/users/:id")]
    pub async fn users_update() -> &'static str {
        "updated"
    }

    #[public]
    #[get("/users", group = "/api")]
    pub async fn api_users() -> &'static str {
        "api users"
    }
}

const SECRET: &str = "test-secret-auth-rewrite";

fn app() -> Rapina {
    Rapina::new()
        .with_introspection(false)
        .with_auth(AuthConfig::new(SECRET, 3600))
        .discover()
}

struct MethodOverride;

impl Middleware for MethodOverride {
    fn handle<'a>(
        &'a self,
        mut req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            if req.headers().contains_key("x-method-override") {
                *req.method_mut() = Method::GET;
                req.headers_mut().remove("x-method-override");
            }
            next.run(req).await
        })
    }
}

struct StripV1;

impl Middleware for StripV1 {
    fn handle<'a>(
        &'a self,
        mut req: Request<Incoming>,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Response<BoxBody>> {
        Box::pin(async move {
            if let Some(rest) = req.uri().path().strip_prefix("/v1") {
                *req.uri_mut() = rest.parse().unwrap();
            }
            next.run(req).await
        })
    }
}

#[tokio::test]
async fn rewritten_method_resolves_again() {
    let client = TestClient::new(app().middleware(MethodOverride)).await;

    let resp = client
        .post("/users/me")
        .header("x-method-override", "GET")
        .send()
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let token = AuthConfig::new(SECRET, 3600).create_token("u1").unwrap();
    let resp = client
        .post("/users/me")
        .header("x-method-override", "GET")
        .header("authorization", &format!("Bearer {token}"))
        .send()
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "me");
}

#[tokio::test]
async fn rewritten_path_resolves_again() {
    let client = TestClient::new(app().middleware(StripV1)).await;

    let resp = client.get("/v1/users/42").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "user");
}

#[tokio::test]
#[should_panic(expected = "did you mean \"GET /users/:id\"")]
async fn public_route_hints_cover_shape_and_method_mistakes() {
    let app = Rapina::new()
        .with_introspection(false)
        .with_auth(AuthConfig::new(SECRET, 3600))
        .public_route("GET", "/users/123")
        .public_route("POST", "/login")
        .router(
            Router::new()
                .route(Method::GET, "/users/:id", |_, _, _| async { "user" })
                .route(Method::GET, "/login", |_, _, _| async { "login" }),
        );
    TestClient::new(app).await;
}

#[tokio::test]
#[should_panic(expected = "GET /health")]
async fn public_route_method_case_mismatch_names_the_route() {
    let app = Rapina::new()
        .with_introspection(false)
        .with_auth(AuthConfig::new(SECRET, 3600))
        .public_route("get", "/health")
        .router(Router::new().route(Method::GET, "/health", |_, _, _| async { "health" }));
    TestClient::new(app).await;
}

#[tokio::test]
#[should_panic(expected = "did you mean \"GET /users/me\"")]
async fn public_route_hint_prefers_the_exact_pattern() {
    let app = Rapina::new()
        .with_introspection(false)
        .with_auth(AuthConfig::new(SECRET, 3600))
        .public_route("get", "/users/me")
        .router(
            Router::new()
                .route(Method::GET, "/users/:id", |_, _, _| async { "user" })
                .route(Method::GET, "/users/me", |_, _, _| async { "me" }),
        );
    TestClient::new(app).await;
}

#[tokio::test]
async fn public_route_without_auth_enabled_does_not_panic() {
    let app = Rapina::new()
        .with_introspection(false)
        .public_route("GET", "/nope")
        .router(Router::new().route(Method::GET, "/x", |_, _, _| async { "x" }));
    let client = TestClient::new(app).await;

    let resp = client.get("/x").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn group_route_descriptor_matches_registration() {
    let client = TestClient::new(app()).await;

    let resp = client.get("/api/users").send().await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.text(), "api users");
}
