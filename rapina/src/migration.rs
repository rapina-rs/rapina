//! Database migration support for Rapina applications.
//!
//! Wraps SeaORM's migration system with convenient re-exports
//! and a `migrations!` macro for easy registration.
//!
//! # Quick Start
//!
//! ```rust,ignore
//!  // src/migrations/m20260213_000001_create_users.rs
//! use rapina::migration::prelude::*;
//!
//! #[derive(DeriveMigrationName)]
//! pub struct Migration;
//!
//! #[async_trait]
//! impl MigrationTrait for Migration {
//!     async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
//!         manager.create_table(
//!             Table::create()
//!                 .table(Users::Table)
//!                 .col(ColumnDef::new(Users::Id).integer().not_null().auto_increment().primary_key())
//!                 .col(ColumnDef::new(Users::Email).string().not_null().unique_key())
//!                 .to_owned()
//!         ).await
//!     }
//!
//!     async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
//!         manager.drop_table(Table::drop().table(Users::Table).to_owned()).await
//!     }
//! }
//!
//! #[derive(DeriveIden)]
//! enum Users {
//!     Table,
//!     Id,
//!     Email,
//! }
//! ```
//!
//! ```rust,ignore
//! // src/migrations/mod.rs
//! mod m20260213_000001_create_users;
//!
//! rapina::migrations! {
//!     m20260213_000001_create_users,
//! }
//! ```

/// Re-exports for writing migrations.
///
/// ```rust,ignore
/// use rapina::migration::prelude::*;
/// ```
pub mod prelude {
    pub use async_trait::async_trait;
    pub use sea_orm_migration::prelude::*;
}

pub use sea_orm::DbErr;
pub use sea_orm_migration::MigrationTrait;
pub use sea_orm_migration::MigratorTrait;
pub use sea_orm_migration::SchemaManager;
pub use sea_orm_migration::prelude::{DeriveIden, DeriveMigrationName};

/// Subcommands understood by [`run_cli`].
#[derive(Debug, PartialEq)]
pub enum MigrateCommand {
    Up,
    Down { steps: u32 },
    Status,
    Fresh,
    Reset,
}

/// Parse CLI args (everything after the binary name) into a [`MigrateCommand`].
///
/// Expected forms:
/// - `up`
/// - `down [--steps N]`
/// - `status`
/// - `fresh`
/// - `reset`
pub fn parse_args(args: &[String]) -> Result<MigrateCommand, String> {
    match args.first().map(|s| s.as_str()) {
        Some("up") => Ok(MigrateCommand::Up),
        Some("down") => {
            let steps = parse_steps(&args[1..])?;
            Ok(MigrateCommand::Down { steps })
        }
        Some("status") => Ok(MigrateCommand::Status),
        Some("fresh") => Ok(MigrateCommand::Fresh),
        Some("reset") => Ok(MigrateCommand::Reset),
        Some(other) => Err(format!(
            "Unknown subcommand: '{}'. Valid: up | down [--steps N] | status | fresh | reset",
            other
        )),
        None => Err(
            "No subcommand given. Usage: rapina_migrate <up|down|status|fresh|reset>".to_string(),
        ),
    }
}

fn parse_steps(args: &[String]) -> Result<u32, String> {
    if args.is_empty() {
        return Ok(1);
    }
    match args[0].as_str() {
        "--steps" => {
            if args.len() < 2 {
                return Err("--steps requires a value".to_string());
            }
            args[1]
                .parse::<u32>()
                .map_err(|_| format!("Invalid steps value: '{}'", args[1]))
        }
        other => Err(format!("Unexpected argument: '{}'", other)),
    }
}

/// Generates a `Migrator` struct implementing `MigrationTrait`
///
/// ```rust,ignore
/// rapina::migrations! {
///     m20260213_000001_create_users,
///     m20260214_000001_create_posts,
/// }
/// ```
#[macro_export]
macro_rules! migrations {
    ($($module:ident ),* $(,)?) => {
        pub struct Migrator;

        #[$crate::async_trait::async_trait]
        impl $crate::sea_orm_migration::MigratorTrait for Migrator {
            fn migrations() -> Vec<Box<dyn $crate::sea_orm_migration::MigrationTrait>> {
        vec![
        $(Box::new($module::Migration), )*
        ]
        }
        }
    }
}

/// Applies all pending migrations.
pub async fn run_pending<M: MigratorTrait>(
    conn: &sea_orm::DatabaseConnection,
) -> Result<(), DbErr> {
    tracing::info!("Running pending database migrations...");
    M::up(conn, None).await?;
    tracing::info!("All migrations applied successfully");
    Ok(())
}

/// Rolls back migrations. Defaults to 1 step if None.
pub async fn rollback<M: MigratorTrait>(
    conn: &sea_orm::DatabaseConnection,
    steps: Option<u32>,
) -> Result<(), DbErr> {
    let steps = steps.unwrap_or(1);
    tracing::info!(steps, "Rolling back migrations...");
    M::down(conn, Some(steps)).await?;
    tracing::info!("Rollback complete");
    Ok(())
}

/// Prints migration status.
pub async fn status<M: MigratorTrait>(conn: &sea_orm::DatabaseConnection) -> Result<(), DbErr> {
    M::status(conn).await
}

/// Entry point for the `rapina_migrate` binary.
///
/// Reads `DATABASE_URL` from the environment, parses `std::env::args()`,
/// connects to the database, and dispatches the requested migration command.
///
/// # Usage (in `src/bin/rapina_migrate.rs`)
///
/// ```rust,ignore
/// #[path = "../migrations/mod.rs"]
/// mod migrations;
///
/// #[tokio::main]
/// async fn main() {
///     rapina::migration::run_cli::<migrations::Migrator>().await;
/// }
/// ```
pub async fn run_cli<M: MigratorTrait>() {
    // Load .env before reading DATABASE_URL — the rapina-cli parent process loads .env
    // itself but the spawned `rapina_migrate` child process starts fresh and won't
    // inherit values that were only loaded from the file (not exported in the shell).
    dotenvy::dotenv().ok();

    let raw_args: Vec<String> = std::env::args().skip(1).collect();

    let db_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("Error: DATABASE_URL environment variable is not set.");
            eprintln!("       Set it in your .env file or export it before running.");
            std::process::exit(1);
        }
    };

    let cmd = match parse_args(&raw_args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let conn = match sea_orm::Database::connect(&db_url).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: Could not connect to database: {e}");
            std::process::exit(1);
        }
    };

    let result: Result<(), sea_orm::DbErr> = match cmd {
        MigrateCommand::Up => {
            let r = M::up(&conn, None).await;
            if r.is_ok() {
                println!("Migrations applied successfully.");
            }
            r
        }
        MigrateCommand::Down { steps } => {
            let r = M::down(&conn, Some(steps)).await;
            if r.is_ok() {
                println!("Rolled back {steps} migration(s).");
            }
            r
        }
        MigrateCommand::Status => M::status(&conn).await,
        MigrateCommand::Fresh => {
            let r = M::fresh(&conn).await;
            if r.is_ok() {
                println!("Database cleared and migrations re-applied (fresh).");
            }
            r
        }
        MigrateCommand::Reset => {
            let r = M::refresh(&conn).await;
            if r.is_ok() {
                println!("Migrations reset (down all + up all).");
            }
            r
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
