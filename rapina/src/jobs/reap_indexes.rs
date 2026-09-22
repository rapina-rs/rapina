//! SeaORM migration adding the indexes that serve lease reaping.
//!
//! The reaper scans for `status = 'running' AND locked_until <= now` every
//! poll cycle, and nothing prunes completed rows, so without an index that
//! scan is a full table pass on an ever-growing table. The claimable index
//! from [`crate::jobs::create_rapina_jobs`] is partial on
//! `status = 'pending'` and cannot serve it.
//!
//! Register it in the project's migration list after `create_rapina_jobs`:
//!
//! ```rust,ignore
//! use rapina::jobs::{create_rapina_jobs, reap_indexes};
//!
//! rapina::migrations! {
//!     create_rapina_jobs,
//!     reap_indexes,
//!     m20260315_000001_create_users,
//! }
//! ```

use crate::jobs::RapinaJobs;
use crate::migration::prelude::*;

/// Migration that indexes the reaper's scan predicate.
///
/// Manual [`MigrationName`](sea_orm_migration::MigrationName) so it stays in
/// the zero-timestamp series, sorting after the table creation but before
/// user migrations. A new name is what reaches deployments that already
/// applied `create_rapina_jobs`: `run_pending` applies it by name.
pub struct Migration;

impl sea_orm_migration::MigrationName for Migration {
    fn name(&self) -> &str {
        "m00000000_000001_add_reaper_indexes"
    }
}

#[async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // PostgreSQL: partial index covering exactly the reaper's predicate.
        // MySQL:      composite index (no partial indexes in MySQL).
        // SQLite:     skipped (single-writer; the claimable index skips it too).
        let db = manager.get_connection();
        match db.get_database_backend() {
            sea_orm::DbBackend::Postgres => {
                let sql = format!(
                    "CREATE INDEX IF NOT EXISTS {} \
                     ON {} ({}) \
                     WHERE {} = 'running'",
                    RapinaJobs::reapable_index(),
                    RapinaJobs::table_name(),
                    RapinaJobs::locked_until(),
                    RapinaJobs::status(),
                );
                db.execute_unprepared(&sql).await?;
            }
            sea_orm::DbBackend::MySql => {
                let sql = format!(
                    "CREATE INDEX {} \
                     ON {} ({}, {})",
                    RapinaJobs::reapable_index(),
                    RapinaJobs::table_name(),
                    RapinaJobs::status(),
                    RapinaJobs::locked_until(),
                );
                db.execute_unprepared(&sql).await?;
            }
            sea_orm::DbBackend::Sqlite => {}
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        match db.get_database_backend() {
            sea_orm::DbBackend::Postgres => {
                let sql = format!("DROP INDEX IF EXISTS {}", RapinaJobs::reapable_index());
                db.execute_unprepared(&sql).await?;
            }
            sea_orm::DbBackend::MySql => {
                let sql = format!(
                    "DROP INDEX {} ON {}",
                    RapinaJobs::reapable_index(),
                    RapinaJobs::table_name(),
                );
                db.execute_unprepared(&sql).await?;
            }
            sea_orm::DbBackend::Sqlite => {
                // no index was created in `up`
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_name_uses_zero_timestamp_series() {
        let m = Migration;
        let name = sea_orm_migration::MigrationName::name(&m);
        assert_eq!(name, "m00000000_000001_add_reaper_indexes");
    }

    #[test]
    fn migration_name_sorts_after_table_creation_and_before_user_migrations() {
        let table =
            sea_orm_migration::MigrationName::name(&crate::jobs::create_rapina_jobs::Migration);
        let framework = sea_orm_migration::MigrationName::name(&Migration);
        let user = "m20260315_000001_create_users";
        assert!(
            table < framework,
            "index migration must run after the table exists"
        );
        assert!(
            framework < user,
            "framework migration must sort before user migrations"
        );
    }
}
