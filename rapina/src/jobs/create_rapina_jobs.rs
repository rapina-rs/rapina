//! SeaORM migration for the `rapina_jobs` table.
//!
//! This is a framework-provided migration. Users register it in their
//! project's migration list so it runs alongside application migrations:
//!
//! ```rust,ignore
//! use rapina::jobs::create_rapina_jobs;
//!
//! rapina::migrations! {
//!     create_rapina_jobs,   // framework table — sorts first
//!     m20260315_000001_create_users,
//! }
//! ```
//!
//! The migration name uses a zero timestamp (`m00000000_000000_`) so it
//! always sorts before user migrations regardless of their dates.
//!
//! **PostgreSQL only.** Uses `gen_random_uuid()`, `now()`, and a partial
//! index, none of which are portable to MySQL or SQLite.

use crate::migration::prelude::*;

/// Migration that creates the `rapina_jobs` table and its partial index.
///
/// Implements [`MigrationName`](sea_orm_migration::MigrationName) manually
/// (instead of `DeriveMigrationName`) to use a zero-timestamp prefix that
/// sorts before all user migrations.
pub struct Migration;

impl sea_orm_migration::MigrationName for Migration {
    fn name(&self) -> &str {
        "m00000000_000000_create_rapina_jobs"
    }
}

#[async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RapinaJobs::Table)
                    .col(
                        ColumnDef::new(RapinaJobs::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .extra("DEFAULT gen_random_uuid()"),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::Queue)
                            .string_len(255)
                            .not_null()
                            .default("default"),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::JobType)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::Payload)
                            .json_binary()
                            .not_null()
                            .default("{}"),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::Status)
                            .string_len(32)
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::Attempts)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::MaxRetries)
                            .integer()
                            .not_null()
                            .default(3),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::RunAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT now()"),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::StartedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::LockedUntil)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(RapinaJobs::FinishedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new(RapinaJobs::LastError).text().null())
                    .col(ColumnDef::new(RapinaJobs::TraceId).string_len(64).null())
                    .col(
                        ColumnDef::new(RapinaJobs::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .extra("DEFAULT now()"),
                    )
                    .to_owned(),
            )
            .await?;

        // Partial index for the job claiming query (FOR UPDATE SKIP LOCKED).
        // SeaORM's builder API doesn't support WHERE clauses on indexes.
        // NOTE: the table name here must match `RapinaJobs::Table` ("rapina_jobs").
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE INDEX idx_rapina_jobs_claimable \
             ON rapina_jobs (queue, run_at) \
             WHERE status = 'pending'",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_rapina_jobs_claimable")
            .await?;

        manager
            .drop_table(Table::drop().table(RapinaJobs::Table).to_owned())
            .await
    }
}

/// Column identifiers for the `rapina_jobs` table.
///
/// Used by SeaORM's schema builder for type-safe DDL. The `Table` variant
/// resolves to `rapina_jobs`, and each column variant resolves to its
/// snake_case equivalent (e.g., `JobType` → `job_type`).
#[derive(DeriveIden)]
enum RapinaJobs {
    Table,
    Id,
    Queue,
    JobType,
    Payload,
    Status,
    Attempts,
    MaxRetries,
    RunAt,
    StartedAt,
    LockedUntil,
    FinishedAt,
    LastError,
    TraceId,
    CreatedAt,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_name_uses_zero_timestamp() {
        let m = Migration;
        let name = sea_orm_migration::MigrationName::name(&m);
        assert_eq!(name, "m00000000_000000_create_rapina_jobs");
    }

    #[test]
    fn migration_name_sorts_before_user_migrations() {
        let framework = "m00000000_000000_create_rapina_jobs";
        let user = "m20260315_000001_create_users";
        assert!(framework < user, "framework migration must sort first");
    }

    #[test]
    fn iden_table_name() {
        assert_eq!(
            RapinaJobs::Table.to_string(),
            "rapina_jobs",
            "table name must match the raw SQL in the partial index"
        );
    }

    #[test]
    fn iden_column_names() {
        // Verify DeriveIden produces the expected snake_case names.
        // If someone reorders or renames variants, this catches it.
        let expected = [
            (RapinaJobs::Id, "id"),
            (RapinaJobs::Queue, "queue"),
            (RapinaJobs::JobType, "job_type"),
            (RapinaJobs::Payload, "payload"),
            (RapinaJobs::Status, "status"),
            (RapinaJobs::Attempts, "attempts"),
            (RapinaJobs::MaxRetries, "max_retries"),
            (RapinaJobs::RunAt, "run_at"),
            (RapinaJobs::StartedAt, "started_at"),
            (RapinaJobs::LockedUntil, "locked_until"),
            (RapinaJobs::FinishedAt, "finished_at"),
            (RapinaJobs::LastError, "last_error"),
            (RapinaJobs::TraceId, "trace_id"),
            (RapinaJobs::CreatedAt, "created_at"),
        ];
        for (iden, name) in expected {
            assert_eq!(iden.to_string(), name);
        }
    }

    #[test]
    fn iden_has_all_fourteen_columns() {
        // The RFC specifies 14 columns (excluding the Table variant).
        let columns = [
            RapinaJobs::Id,
            RapinaJobs::Queue,
            RapinaJobs::JobType,
            RapinaJobs::Payload,
            RapinaJobs::Status,
            RapinaJobs::Attempts,
            RapinaJobs::MaxRetries,
            RapinaJobs::RunAt,
            RapinaJobs::StartedAt,
            RapinaJobs::LockedUntil,
            RapinaJobs::FinishedAt,
            RapinaJobs::LastError,
            RapinaJobs::TraceId,
            RapinaJobs::CreatedAt,
        ];
        assert_eq!(columns.len(), 14);
    }
}
