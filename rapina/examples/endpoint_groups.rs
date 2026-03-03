use rapina::prelude::*;

// ── Auto-discovered grouped routes ──────────────────────────────────────────

#[get("/users", group = "/api")]
async fn list_users() -> String {
    "list_users via discovery at /api/users".to_string()
}

#[get("/users/:id", group = "/api")]
async fn get_user(id: Path<u64>) -> String {
    format!("user {} via discovery at /api/users/:id", *id)
}

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new().discover().listen("127.0.0.1:3000").await
}
