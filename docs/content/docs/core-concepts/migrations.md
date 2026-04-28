+++
title = "Migrations"
description = "Schema migrations with SeaORM"
weight = 5
date = 2026-03-05
+++

Migrations are versioned schema changes written in Rust. Instead of running raw SQL against your database, you define each change as a struct with an `up` method (apply) and a `down` method (rollback). Rapina tracks which migrations have already run, so when your app starts it only applies the new ones.

This page walks through the full workflow: generating a migration, writing the schema change, and wiring it into your app.

## Prerequisites

Your `Cargo.toml` needs the `database` feature and a database driver:

```toml
[dependencies]
rapina = { version = "0.11.0", features = ["sqlite"] }
```

Replace `sqlite` with `postgres` or `mysql` depending on your database. You also need a database connection configured in your app — see the [Database](/docs/core-concepts/database/) page if you haven't set that up yet.

## Generating a Migration

Run the CLI from your project root:

```bash
rapina migrate new create_users
```

This creates `src/migrations/` if it doesn't exist, generates a timestamped migration file, and updates `mod.rs` to register it:

```
  ✓ Created src/migrations/
  ✓ Created src/migrations/m20260305_143022_create_users.rs
  ✓ Updated src/migrations/mod.rs
```

The name must be lowercase with underscores only — no hyphens, no uppercase. The timestamp prefix is added automatically.

## Writing the Migration

Open the generated file. It starts as a skeleton with `todo!()` placeholders:

```rust
use rapina::sea_orm_migration;
use rapina::migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        todo!("Write your migration here")
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        todo!("Write your rollback here")
    }
}
```

Replace the `todo!()` calls with your schema changes. Here's a complete migration that creates a `users` table:

```rust
use rapina::sea_orm_migration;
use rapina::migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Email,
    Name,
}

#[async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .col(
                        ColumnDef::new(Users::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Users::Email).string().not_null())
                    .col(ColumnDef::new(Users::Name).string().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await
    }
}
```

The `DeriveIden` enum defines your table and column names. The first variant is always `Table` (the table name), and the rest are columns. SeaORM converts variant names to snake_case — `Email` becomes `email`, `CreatedAt` becomes `created_at`.

The `up` method creates the table. The `down` method drops it. Always write both so migrations are reversible.

## The mod.rs File

When you run `rapina migrate new`, the CLI also creates or updates `src/migrations/mod.rs`. This file declares your migration modules and registers them with the `migrations!` macro:

```rust
mod m20260305_143022_create_users;

rapina::migrations! {
    m20260305_143022_create_users,
}
```

The macro generates a `Migrator` struct that knows about all your migrations. As you add more, the CLI appends them:

```rust
mod m20260305_143022_create_users;
mod m20260306_091500_add_posts;

rapina::migrations! {
    m20260305_143022_create_users,
    m20260306_091500_add_posts,
}
```

Order matters — migrations run top to bottom.

## Running Migrations

### CLI commands (recommended for production)

Rapina ships a dedicated migration binary that you run separately from the web server.

**First-time setup:** run `rapina migrate init` once from your project root. Use this when starting a new project or when adding migrations to an existing project that was created before the migrate binary existed:

```bash
rapina migrate init
```

This creates `src/bin/rapina_migrate.rs` — a small binary that connects to your database and runs the migration commands. You only need to run `init` once; after that, use the commands below to apply and inspect migrations without starting the web server:

```bash
rapina migrate up          # apply all pending migrations
rapina migrate down        # roll back 1 migration
rapina migrate down --steps 3  # roll back 3 migrations
rapina migrate status      # show applied and pending migrations
rapina migrate fresh       # drop all tables and re-run all migrations (destructive)
rapina migrate reset       # roll back all migrations then re-apply them
```

These commands shell out to `cargo run --bin rapina_migrate` under the hood. They work from any subdirectory in your project — Rapina walks up from the current directory to locate the project root (identified by `Cargo.toml`).

**`src/bin/rapina_migrate.rs` layout note:** the generated file uses `#[path = "../migrations/mod.rs"]` to locate your migrations module. This assumes the default layout where `src/bin/` and `src/migrations/` are siblings. If you have customised your directory structure, update the `#[path]` attribute in that file accordingly.

### `.run_migrations()` in the app builder

You can also chain `.run_migrations()` after `.with_database()` so pending migrations run automatically before the server starts listening:

```rust
use rapina::prelude::*;
use rapina::database::DatabaseConfig;

mod migrations;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_database(DatabaseConfig::new("sqlite://app.db?mode=rwc"))
        .await?
        .run_migrations::<migrations::Migrator>()
        .await?
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

The turbofish `::<migrations::Migrator>` points to the struct generated by the `migrations!` macro. If you forget to call `.with_database()` first, `run_migrations` returns an error.

**Multi-replica warning:** `.run_migrations()` runs on every server startup. On a multi-replica deploy this means multiple instances can race to apply the same migration simultaneously. For production use `rapina migrate up` instead — run it as a one-off step (e.g. in a deploy hook or init container) before starting the server replicas.

Migrations that have already been applied are skipped. Only new ones run.

## Adding More Migrations

Each schema change gets its own migration. Never edit a migration that has already been applied to a database — create a new one instead:

```bash
rapina migrate new add_bio_to_users
```

The CLI appends the new module to `mod.rs` automatically. Open the generated file, write your `ALTER TABLE` logic in `up`, and the reverse in `down`.
