//! Database integration for Rapina applications.
//!
//! This module provides first-class SeaORM integration with:
//! - Environment-aware configuration (development, production, test)
//! - Connection pool management
//! - Automatic error conversion (no `.map_err()` needed)
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use rapina::prelude::*;
//! use rapina::database::{DatabaseConfig, Db};
//!
//! #[get("/users/:id")]
//! async fn get_user(id: Path<i32>, db: Db) -> Result<Json<User>> {
//!     let user = UserEntity::find_by_id(id.into_inner())
//!         .one(db.conn())
//!         .await?  // No .map_err() needed!
//!         .ok_or_else(|| Error::not_found("user not found"))?;
//!     Ok(Json(user.into()))
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     let db_config = DatabaseConfig::from_env()?;
//!
//!     Rapina::new()
//!         .with_database(db_config).await?
//!         .router(router)
//!         .listen("127.0.0.1:3000")
//!         .await
//! }
//! ```
//!
//! # Environment Configuration
//!
//! The database configuration is environment-aware:
//!
//! ```bash
//! # Required
//! DATABASE_URL=postgres://user:pass@localhost/myapp
//!
//! # Optional
//! DATABASE_MAX_CONNECTIONS=100  # default: 10
//! DATABASE_MIN_CONNECTIONS=5    # default: 1
//! DATABASE_CONNECT_TIMEOUT=30   # seconds, default: 30
//! DATABASE_IDLE_TIMEOUT=600     # seconds, default: 600
//! ```

use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use std::time::Duration;

use crate::error::{Error, IntoApiError};

/// Database configuration with environment-aware defaults.
///
/// Use `DatabaseConfig::from_env()` to load from environment variables,
/// or build manually for testing.
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// Database connection URL (e.g., postgres://user:pass@host/db)
    pub url: String,
    /// Maximum number of connections in the pool (default: 10)
    pub max_connections: u32,
    /// Minimum number of connections to keep open (default: 1)
    pub min_connections: u32,
    /// Connection timeout in seconds (default: 30)
    pub connect_timeout: u64,
    /// Idle connection timeout in seconds (default: 600)
    pub idle_timeout: u64,
    /// Enable SQL query logging (default: true in debug, false in release)
    pub sqlx_logging: bool,
}

impl DatabaseConfig {
    /// Creates a new database configuration with the given URL and defaults.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            max_connections: 10,
            min_connections: 1,
            connect_timeout: 30,
            idle_timeout: 600,
            sqlx_logging: cfg!(debug_assertions),
        }
    }

    /// Loads configuration from environment variables.
    ///
    /// Required:
    /// - `DATABASE_URL`: The database connection string
    ///
    /// Optional:
    /// - `DATABASE_MAX_CONNECTIONS`: Max pool size (default: 10)
    /// - `DATABASE_MIN_CONNECTIONS`: Min pool size (default: 1)
    /// - `DATABASE_CONNECT_TIMEOUT`: Connection timeout in seconds (default: 30)
    /// - `DATABASE_IDLE_TIMEOUT`: Idle timeout in seconds (default: 600)
    /// - `DATABASE_LOGGING`: Enable SQL logging (default: true in debug)
    pub fn from_env() -> Result<Self, std::io::Error> {
        let url = std::env::var("DATABASE_URL").map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "DATABASE_URL environment variable not set",
            )
        })?;

        let max_connections = std::env::var("DATABASE_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let min_connections = std::env::var("DATABASE_MIN_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);

        let connect_timeout = std::env::var("DATABASE_CONNECT_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let idle_timeout = std::env::var("DATABASE_IDLE_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);

        let sqlx_logging = std::env::var("DATABASE_LOGGING")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(cfg!(debug_assertions));

        Ok(Self {
            url,
            max_connections,
            min_connections,
            connect_timeout,
            idle_timeout,
            sqlx_logging,
        })
    }

    /// Sets the maximum number of connections in the pool.
    pub fn max_connections(mut self, n: u32) -> Self {
        self.max_connections = n;
        self
    }

    /// Sets the minimum number of connections in the pool.
    pub fn min_connections(mut self, n: u32) -> Self {
        self.min_connections = n;
        self
    }

    /// Sets the connection timeout in seconds.
    pub fn connect_timeout(mut self, secs: u64) -> Self {
        self.connect_timeout = secs;
        self
    }

    /// Sets the idle connection timeout in seconds.
    pub fn idle_timeout(mut self, secs: u64) -> Self {
        self.idle_timeout = secs;
        self
    }

    /// Enables or disables SQL query logging.
    pub fn sqlx_logging(mut self, enabled: bool) -> Self {
        self.sqlx_logging = enabled;
        self
    }

    /// Connects to the database and returns a connection pool.
    pub async fn connect(&self) -> Result<DatabaseConnection, DbError> {
        let mut opts = ConnectOptions::new(&self.url);
        opts.max_connections(self.max_connections)
            .min_connections(self.min_connections)
            .connect_timeout(Duration::from_secs(self.connect_timeout))
            .idle_timeout(Duration::from_secs(self.idle_timeout))
            .sqlx_logging(self.sqlx_logging);

        Database::connect(opts).await.map_err(DbError)
    }
}

/// Wrapper around SeaORM's `DbErr` for Rapina error integration.
///
/// This type implements `IntoApiError`, allowing you to use `?` directly
/// on database operations without manual error mapping.
#[derive(Debug)]
pub struct DbError(pub sea_orm::DbErr);

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl IntoApiError for DbError {
    fn into_api_error(self) -> Error {
        use sea_orm::DbErr;

        match &self.0 {
            DbErr::RecordNotFound(msg) => Error::not_found(msg.clone()),
            DbErr::RecordNotInserted => Error::internal("failed to insert record"),
            DbErr::RecordNotUpdated => Error::internal("failed to update record"),
            DbErr::Custom(msg) => Error::internal(msg.clone()),
            DbErr::Query(err) => {
                tracing::error!(error = %err, "database query error");
                Error::internal("database query failed")
            }
            DbErr::Conn(err) => {
                tracing::error!(error = %err, "database connection error");
                Error::internal("database connection failed")
            }
            DbErr::Exec(err) => {
                tracing::error!(error = %err, "database execution error");
                Error::internal("database operation failed")
            }
            _ => {
                tracing::error!(error = %self.0, "database error");
                Error::internal("database error")
            }
        }
    }
}

impl From<sea_orm::DbErr> for DbError {
    fn from(err: sea_orm::DbErr) -> Self {
        DbError(err)
    }
}

/// Database connection extractor for handlers.
///
/// Use this to access the database connection pool in your handlers.
///
/// # Example
///
/// ```rust,ignore
/// use rapina::prelude::*;
/// use rapina::database::Db;
///
/// #[get("/users")]
/// async fn list_users(db: Db) -> Result<Json<Vec<User>>> {
///     let users = UserEntity::find()
///         .all(db.conn())
///         .await?;
///     Ok(Json(users.into_iter().map(Into::into).collect()))
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Db(DatabaseConnection);

impl Db {
    /// Creates a new Db wrapper around a connection.
    pub fn new(conn: DatabaseConnection) -> Self {
        Self(conn)
    }

    /// Returns a reference to the underlying database connection.
    ///
    /// Use this when calling SeaORM methods that take `&DatabaseConnection`.
    pub fn conn(&self) -> &DatabaseConnection {
        &self.0
    }

    /// Consumes the wrapper and returns the underlying connection.
    pub fn into_inner(self) -> DatabaseConnection {
        self.0
    }
}

impl AsRef<DatabaseConnection> for Db {
    fn as_ref(&self) -> &DatabaseConnection {
        &self.0
    }
}

impl std::ops::Deref for Db {
    type Target = DatabaseConnection;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_config_new() {
        let config = DatabaseConfig::new("postgres://localhost/test");
        assert_eq!(config.url, "postgres://localhost/test");
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 1);
    }

    #[test]
    fn test_database_config_builder() {
        let config = DatabaseConfig::new("postgres://localhost/test")
            .max_connections(50)
            .min_connections(5)
            .connect_timeout(60)
            .idle_timeout(300)
            .sqlx_logging(false);

        assert_eq!(config.max_connections, 50);
        assert_eq!(config.min_connections, 5);
        assert_eq!(config.connect_timeout, 60);
        assert_eq!(config.idle_timeout, 300);
        assert!(!config.sqlx_logging);
    }

    #[test]
    fn test_db_error_not_found() {
        let err = DbError(sea_orm::DbErr::RecordNotFound("user".to_string()));
        let api_err = err.into_api_error();
        assert_eq!(api_err.status, 404);
        assert_eq!(api_err.code, "NOT_FOUND");
    }

    #[test]
    fn test_db_error_custom() {
        let err = DbError(sea_orm::DbErr::Custom("something went wrong".to_string()));
        let api_err = err.into_api_error();
        assert_eq!(api_err.status, 500);
        assert_eq!(api_err.message, "something went wrong");
    }
}
