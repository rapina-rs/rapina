use rapina::cache::CacheConfig;
use rapina::config::load_dotenv;
use rapina::database::DatabaseConfig;
use rapina::middleware::RequestLogMiddleware;
use rapina::prelude::*;
use rapina::schemars;

pub mod entity;
pub mod migrations;
pub mod urls;

#[derive(Clone, Config)]
pub struct AppConfig {
    #[env = "HOST"]
    #[default = "127.0.0.1"]
    pub host: String,

    #[env = "PORT"]
    #[default = "3000"]
    pub port: u16,

    #[env = "CACHE_CAPACITY"]
    #[default = "10000"]
    pub cache_capacity: usize,

    #[env = "RATE_LIMIT_PER_MINUTE"]
    #[default = "60"]
    pub rate_limit_per_minute: u32,
}

#[derive(Serialize, JsonSchema)]
pub struct MessageResponse {
    message: String,
}

#[derive(Serialize, JsonSchema)]
pub struct HealthResponse {
    status: String,
    version: String,
}

#[get("/")]
#[public]
pub async fn hello() -> Json<MessageResponse> {
    Json(MessageResponse {
        message: "Hello from Rapina!".to_string(),
    })
}

#[get("/health")]
#[public]
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

pub async fn build_app() -> std::io::Result<(Rapina, String)> {
    load_dotenv();

    let config = AppConfig::from_env().expect("Failed to load config");
    let addr = format!("{}:{}", config.host, config.port);
    let db_config = DatabaseConfig::from_env()
        .unwrap_or_else(|_| DatabaseConfig::new("sqlite://urls.db?mode=rwc"));

    let app = Rapina::new()
        .with_tracing(TracingConfig::new())
        .with_rate_limit(RateLimitConfig::per_minute(config.rate_limit_per_minute))
        .with_cache(CacheConfig::in_memory(config.cache_capacity))
        .await?
        .middleware(RequestLogMiddleware::new())
        .with_database(db_config)
        .await?
        .run_migrations::<migrations::Migrator>()
        .await?
        .state(config)
        .discover();

    Ok((app, addr))
}
