use rapina::database::DatabaseConfig;
use rapina::middleware::RequestLogMiddleware;
use rapina::prelude::*;

mod entity;
mod migrations;
mod todos;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_tracing(TracingConfig::new())
        .openapi("Todo API", "1.0.0")
        .middleware(RequestLogMiddleware::new())
        .with_database(DatabaseConfig::new("sqlite://todos.db?mode=rwc"))
        .await?
        .run_migrations::<migrations::Migrator>()
        .await?
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
