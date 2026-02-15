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
