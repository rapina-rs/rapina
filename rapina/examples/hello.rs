use rapina::prelude::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> String {
    format!("ID: {}", *id)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_health_check(true)
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
