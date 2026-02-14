use rapina::prelude::*;
use rapina::database::DatabaseConfig;
use rapina::middleware::RequestLogMiddleware;

mod entity;
mod migrations;
mod todos;

use todos::handlers::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router = Router::new()
        .get("/todos", list_todos)
        .get("/todos/:id", get_todo)
        .post("/todos", create_todo)
        .put("/todos/:id", update_todo)
        .delete("/todos/:id", delete_todo);

    Rapina::new()
        .with_tracing(TracingConfig::new())
        .openapi("Todo API", "1.0.0")
        .middleware(RequestLogMiddleware::new())
        .with_database(DatabaseConfig::new("sqlite://todos.db?mode=rwc"))
        .await?
        .run_migrations::<migrations::Migrator>()
        .await?
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
