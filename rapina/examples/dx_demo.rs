use rapina_dx::*;
use rapina::prelude::*;
use rapina::middleware::RequestLogMiddleware;

#[get("/health")]
async fn health() -> &'static str {
    "ok"
}

#[post("/login")]
async fn login() -> &'static str {
    "token"
}

#[get("/me")]
async fn get_me() -> &'static str {
    "user info"
}

#[tokio::main]
async fn main() {
    app()
        .middleware(RequestLogMiddleware::new())
        .get("/health", health)
            .public()
            .tag("Operation")
            .description("Check if the service is alive")
        .group("/api", |api| {
            api.group("/auth", |auth| {
                auth.post("/login", login)
                    .public()
                    .tag("Auth")
                    .description("Authenticate user and get token")
            })
            .group("/user", |user| {
                user.get("/me", get_me)
                    .tag("User")
                    .description("Get information about the current user")
            })
        })
        .listen(3000)
        .await
        .unwrap();
}
