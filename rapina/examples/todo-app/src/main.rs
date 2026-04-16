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
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = AppConfig::from_env().expect("Missing Config");
    let addr = format!("{}:{}", config.host, config.port);

    Rapina::new()
        .with_tracing(TracingConfig::new())
        .state(config)
        .openapi("Todo API", "1.0.0")
        .middleware(RequestLogMiddleware::new())
        .with_database(DatabaseConfig::new("sqlite://todos.db?mode=rwc"))
        .await?
        .run_migrations::<migrations::Migrator>()
        .await?
        .discover()
        .listen(&addr)
        .await
}
