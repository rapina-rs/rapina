use rapina::prelude::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

#[get("/health")]
async fn health() -> StatusCode {
    StatusCode::OK
}

#[get("/users/:id")]
async fn get_user(id: Path<u64>) -> String {
    format!("ID: {}", *id)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new().discover().listen("127.0.0.1:3000").await
}
