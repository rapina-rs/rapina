//! Migration: create urlss

use rapina::sea_orm_migration;
use rapina::migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Urlss::Table)
                    .col(
                        ColumnDef::new(Urlss::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Urlss::ShortCode).string().not_null())
                    .col(ColumnDef::new(Urlss::LongUrl).text().not_null())
                    .col(ColumnDef::new(Urlss::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Urlss::ExpiresAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Urlss::ClickCount).big_integer().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Urlss::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Urlss {
    Table,
    Id,
    ShortCode,
    LongUrl,
    CreatedAt,
    ExpiresAt,
    ClickCount,
}
