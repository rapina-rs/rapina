use rapina::prelude::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

#[get("")]
async fn list_users() -> String {
    "list_users() of users_router, this would typically be located in users.rs".to_string()
}

#[get("/:id")]
async fn get_user(id: Path<u64>) -> String {
    format!("ID: {}", *id)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let users_router = Router::new().get("", list_users).get("/:id", get_user);

    let router = Router::new()
        .get("/", hello)
        .group("/api/users", users_router);

    Rapina::new().router(router).listen("127.0.0.1:3000").await
}
