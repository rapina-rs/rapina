#![cfg(feature = "sqlite")]

use rapina::migration::prelude::*;
use rapina::sea_orm::Database;

mod test_migration {
    use super::*;

    #[derive(DeriveMigrationName)]
    pub struct Migration;

    #[async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(TestTable::Table)
                        .col(
                            ColumnDef::new(TestTable::Id)
                                .integer()
                                .not_null()
                                .auto_increment()
                                .primary_key(),
                        )
                        .col(ColumnDef::new(TestTable::Name).string().not_null())
                        .to_owned(),
                )
                .await
        }

        async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .drop_table(Table::drop().table(TestTable::Table).to_owned())
                .await
        }
    }

    #[derive(DeriveIden)]
    enum TestTable {
        Table,
        Id,
        Name,
    }
}

rapina::migrations! {
    test_migration,
}

#[tokio::test]
async fn test_run_pending_migrations() {
    let conn = Database::connect("sqlite::memory:").await.unwrap();
    rapina::migration::run_pending::<Migrator>(&conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_migration_status() {
    let conn = Database::connect("sqlite::memory:").await.unwrap();
    rapina::migration::status::<Migrator>(&conn).await.unwrap();
}

#[tokio::test]
async fn test_migration_rollback() {
    let conn = Database::connect("sqlite::memory:").await.unwrap();
    rapina::migration::run_pending::<Migrator>(&conn)
        .await
        .unwrap();
    rapina::migration::rollback::<Migrator>(&conn, Some(1))
        .await
        .unwrap();
}

mod parse_args_tests {
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_parse_up() {
        let cmd = rapina::migration::parse_args(&s(&["up"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Up);
    }

    #[test]
    fn test_parse_down_default_steps() {
        let cmd = rapina::migration::parse_args(&s(&["down"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Down { steps: 1 });
    }

    #[test]
    fn test_parse_down_with_steps() {
        let cmd = rapina::migration::parse_args(&s(&["down", "--steps", "3"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Down { steps: 3 });
    }

    #[test]
    fn test_parse_status() {
        let cmd = rapina::migration::parse_args(&s(&["status"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Status);
    }

    #[test]
    fn test_parse_fresh() {
        let cmd = rapina::migration::parse_args(&s(&["fresh"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Fresh);
    }

    #[test]
    fn test_parse_reset() {
        let cmd = rapina::migration::parse_args(&s(&["reset"])).unwrap();
        assert_eq!(cmd, rapina::migration::MigrateCommand::Reset);
    }

    #[test]
    fn test_parse_unknown_subcommand() {
        let err = rapina::migration::parse_args(&s(&["migrate"])).unwrap_err();
        assert!(err.contains("Unknown"));
    }

    #[test]
    fn test_parse_empty_args() {
        let err = rapina::migration::parse_args(&[]).unwrap_err();
        assert!(err.contains("Usage"));
    }

    #[test]
    fn test_parse_down_invalid_steps() {
        let err = rapina::migration::parse_args(&s(&["down", "--steps", "abc"])).unwrap_err();
        assert!(err.contains("Invalid steps"));
    }

    #[test]
    fn test_parse_down_missing_steps_value() {
        let err = rapina::migration::parse_args(&s(&["down", "--steps"])).unwrap_err();
        assert!(err.contains("--steps requires"));
    }
}

// Tests verifying the MigratorTrait dispatch methods (fresh, refresh, etc.) that
// run_cli<M> delegates to. run_cli itself requires process-level env manipulation
// (DATABASE_URL + argv) so it is tested via CLI integration rather than here.
mod migrator_trait_tests {
    use rapina::migration::prelude::*;
    use rapina::sea_orm::Database;

    mod run_cli_migration {
        use super::*;

        #[derive(DeriveMigrationName)]
        pub struct Migration;

        #[async_trait]
        impl MigrationTrait for Migration {
            async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
                manager
                    .create_table(
                        Table::create()
                            .table(RunCliTable::Table)
                            .col(
                                ColumnDef::new(RunCliTable::Id)
                                    .integer()
                                    .not_null()
                                    .auto_increment()
                                    .primary_key(),
                            )
                            .col(ColumnDef::new(RunCliTable::Name).string().not_null())
                            .to_owned(),
                    )
                    .await
            }

            async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
                manager
                    .drop_table(Table::drop().table(RunCliTable::Table).to_owned())
                    .await
            }
        }

        #[derive(DeriveIden)]
        enum RunCliTable {
            Table,
            Id,
            Name,
        }
    }

    rapina::migrations! {
        run_cli_migration,
    }

    #[tokio::test]
    async fn test_dispatch_up() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&conn, None).await.unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_fresh() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&conn, None).await.unwrap();
        Migrator::fresh(&conn).await.unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_refresh() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&conn, None).await.unwrap();
        Migrator::refresh(&conn).await.unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_down() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&conn, None).await.unwrap();
        Migrator::down(&conn, Some(1)).await.unwrap();
    }

    #[tokio::test]
    async fn test_dispatch_status() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::status(&conn).await.unwrap();
    }
}
