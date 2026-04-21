use rapina::database::DatabaseConfig;
use rapina::middleware::RequestLogMiddleware;
use rapina::prelude::*;

mod entity;
mod migrations;
mod todos;

#[derive(Config, Clone)]
pub struct AppConfig {
    #[env = "HOST"]
    #[default = "0.0.0.0"]
    host: String,

    #[env = "PORT"]
    #[default = "3000"]
    port: u32,

    #[env = "MAX_TODOS"]
    #[default = "5"]
    max_todos: u32,

    #[env = "DATABASE_URL"]
    #[default = "sqlite://todos.db?mode=rwc"]
    db_url: String,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = AppConfig::from_env()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;

    let db_url = config.db_url.clone();
    let addr = format!("{}:{}", config.host, config.port);

    Rapina::new()
        .with_tracing(TracingConfig::new())
        .state(config)
        .openapi("Todo API", "1.0.0")
        .middleware(RequestLogMiddleware::new())
        .with_database(DatabaseConfig::new(db_url))
        .await?
        .run_migrations::<migrations::Migrator>()
        .await?
        .discover()
        .listen(&addr)
        .await
}
