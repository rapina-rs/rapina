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
