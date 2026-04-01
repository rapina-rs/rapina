+++
title = "Adding a Database with SeaORM"
description = "Step-by-step guide to integrating SeaORM into your Rapina project — from dependency setup to running your first query"
date = 2026-03-25

[taxonomies]
categories = ["tutorials"]
tags = ["database", "seaorm", "postgres", "sqlite"]

[extra]
author = "Ricardo Uemura"
+++

Rapina ships with first-class [SeaORM](https://www.sea-ql.org/SeaORM/) support behind a feature flag. This tutorial walks you through the full setup: enabling the feature, defining your first entity, migrating the schema, and querying data from a handler.

## 1. Enable the database feature

Rapina's database integration is opt-in. Add the feature flag for your database driver in `Cargo.toml`:

```toml
[dependencies]
rapina = { version = "0.11", features = ["postgres"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Replace `postgres` with `mysql` or `sqlite` depending on your target database. The `postgres` feature pulls in `sea-orm` with the `sqlx-postgres` backend and runtime support for Tokio with `rustls`.

## 2. Set the database URL

Rapina reads your connection string from the `DATABASE_URL` environment variable. Add it to your `.env` file (or export it in your shell):

```bash
DATABASE_URL=postgres://user:password@localhost:5432/myapp
```

For SQLite you can use a file path:

```bash
DATABASE_URL=sqlite://app.db?mode=rwc
```

Additional pool settings are optional and have sensible defaults:

```bash
DATABASE_MAX_CONNECTIONS=10   # default
DATABASE_MIN_CONNECTIONS=1    # default
DATABASE_CONNECT_TIMEOUT=30   # seconds
```

## 3. Connect the database to your app

Call `.with_database()` on the builder before `.listen()`. `DatabaseConfig::from_env()` reads all the settings above automatically:

```rust
use rapina::prelude::*;
use rapina::database::DatabaseConfig;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let db_config = DatabaseConfig::from_env()?;

    Rapina::new()
        .with_database(db_config).await?
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

If you prefer an inline URL (useful for tests), you can skip `from_env()`:

```rust
Rapina::new()
    .with_database(DatabaseConfig::new("sqlite://app.db?mode=rwc"))
    .await?
```

## 4. Define your entity with `schema!`

The `schema!` macro generates a complete SeaORM entity from a concise declaration. Types in the body determine the column type and — when another entity is referenced — the relationship:

```rust
use rapina::prelude::*;

schema! {
    Post {
        title: String,
        body: Text,
        published: bool,
    }
}
```

The macro expands into a `post` module containing `Model`, `Entity`, `ActiveModel`, `Relation`, and the `Related` trait implementations. Every entity gets `id: i32`, `created_at`, and `updated_at` for free — no extra fields needed.

### Supported field types

| Schema type | Column type |
|-------------|-------------|
| `String` | VARCHAR |
| `Text` | TEXT |
| `i32` / `i64` | INTEGER / BIGINT |
| `f64` | DOUBLE |
| `bool` | BOOLEAN |
| `Uuid` | UUID |
| `Option<T>` | nullable |

### Relationships

Reference another entity to declare relationships:

```rust
schema! {
    Author {
        #[unique]
        email: String,
        name: String,
        posts: Vec<Post>,    // has_many
    }

    Post {
        title: String,
        body: Text,
        author: Author,      // belongs_to — generates author_id column
    }
}
```

## 5. Create a migration

Generate a migration file with the Rapina CLI:

```bash
rapina migrate new create_posts
```

This creates `src/migrations/m<timestamp>_create_posts.rs` and registers it in `src/migrations/mod.rs`. Open the generated file and fill in `up` and `down`:

```rust
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
                    .table(Posts::Table)
                    .col(
                        ColumnDef::new(Posts::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Posts::Title).string().not_null())
                    .col(ColumnDef::new(Posts::Body).text().not_null())
                    .col(ColumnDef::new(Posts::Published).boolean().not_null())
                    .col(ColumnDef::new(Posts::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Posts::UpdatedAt).timestamp_with_time_zone().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Posts::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Id,
    Title,
    Body,
    Published,
    CreatedAt,
    UpdatedAt,
}
```

### Run migrations on startup

Chain `.run_migrations()` after `.with_database()`. Pending migrations are applied before the server starts listening, and already-applied ones are skipped:

```rust
use rapina::prelude::*;
use rapina::database::DatabaseConfig;

mod migrations;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_database(DatabaseConfig::from_env()?).await?
        .run_migrations::<migrations::Migrator>()
        .await?
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

## 6. Query the database from a handler

Use the `Db` extractor to get a connection from the pool. Pass `db.conn()` to any SeaORM method:

```rust
use rapina::database::{Db, DbError};
use rapina::prelude::*;
use rapina::sea_orm::EntityTrait;
use rapina::schemars;

#[derive(Serialize, JsonSchema)]
struct PostResponse {
    id: i32,
    title: String,
    published: bool,
}

#[get("/posts")]
async fn list_posts(db: Db) -> Result<Json<Vec<PostResponse>>> {
    let posts = Post::find()
        .all(db.conn())
        .await
        .map_err(DbError)?;

    let response = posts
        .into_iter()
        .map(|p| PostResponse {
            id: p.id,
            title: p.title,
            published: p.published,
        })
        .collect();

    Ok(Json(response))
}
```

`DbError` converts SeaORM errors into the right HTTP response automatically — a `RecordNotFound` becomes a 404, connection errors become a 500, and so on.

### Fetch a single record

```rust
use rapina::database::{Db, DbError};
use rapina::prelude::*;
use rapina::sea_orm::EntityTrait;

#[get("/posts/:id")]
async fn get_post(db: Db, id: Path<i32>) -> Result<Json<PostResponse>> {
    let post = Post::find_by_id(*id)
        .one(db.conn())
        .await
        .map_err(DbError)?
        .ok_or_else(|| Error::not_found("post not found"))?;

    Ok(Json(PostResponse {
        id: post.id,
        title: post.title,
        published: post.published,
    }))
}
```

### Insert a record

```rust
use rapina::database::{Db, DbError};
use rapina::prelude::*;
use rapina::schemars;
use rapina::sea_orm::{ActiveModelTrait, Set};

#[derive(Deserialize, JsonSchema, Validate)]
struct CreatePost {
    #[validate(length(min = 1))]
    title: String,
    body: String,
}

#[post("/posts")]
async fn create_post(
    payload: Validated<Json<CreatePost>>,
    db: Db,
) -> Result<Json<PostResponse>> {
    let new_post = post::ActiveModel {
        title: Set(payload.title.clone()),
        body: Set(payload.body.clone()),
        published: Set(false),
        ..Default::default()
    };

    let post = new_post.insert(db.conn()).await.map_err(DbError)?;

    Ok(Json(PostResponse {
        id: post.id,
        title: post.title,
        published: post.published,
    }))
}
```

## Putting it all together

Here's the complete `main.rs` for a minimal posts API with a connected database:

```rust
use rapina::database::{DatabaseConfig, Db, DbError};
use rapina::prelude::*;
use rapina::schemars;
use rapina::sea_orm::EntityTrait;

mod migrations;

schema! {
    Post {
        title: String,
        body: Text,
        published: bool,
    }
}

#[derive(Serialize, JsonSchema)]
struct PostResponse {
    id: i32,
    title: String,
    published: bool,
}

#[get("/posts")]
async fn list_posts(db: Db) -> Result<Json<Vec<PostResponse>>> {
    let posts = Post::find()
        .all(db.conn())
        .await
        .map_err(DbError)?;

    Ok(Json(
        posts
            .into_iter()
            .map(|p| PostResponse { id: p.id, title: p.title, published: p.published })
            .collect(),
    ))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_database(DatabaseConfig::from_env()?).await?
        .run_migrations::<migrations::Migrator>()
        .await?
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```

Start the server, then curl the endpoint:

```bash
curl http://localhost:3000/posts
# []
```

## What's next

- Explore filtering and ordering with SeaORM's query builder in the [Database docs](/docs/core-concepts/database/)
- Learn how to manage schema changes over time in the [Migrations docs](/docs/core-concepts/migrations/)
- Try the [interactive tutorial](/tutorial/06-database-basics/) to practice using `Db` in the browser
