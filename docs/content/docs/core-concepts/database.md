+++
title = "Database"
description = "Database integration with SeaORM"
weight = 5
date = 2025-02-13
+++

Rapina integrates with [SeaORM](https://www.sea-ql.org/SeaORM/) for database operations. Enable it with a feature flag for your database.

## Setup

Add the database feature to your `Cargo.toml`:

```toml
[dependencies]
rapina = { version = "0.5.0", features = ["postgres"] }
# or "mysql", "sqlite"
```

Configure your application with a database connection:

```rust
use rapina::prelude::*;
use rapina::database::DatabaseConfig;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let db_config = DatabaseConfig::from_env()?;

    Rapina::new()
        .with_database(db_config).await?
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

Set your database URL via environment variable:

```bash
DATABASE_URL=postgres://user:password@localhost:5432/myapp
```

## The Db Extractor

Access the database connection in your handlers with the `Db` extractor:

```rust
use rapina::database::{Db, DbError};
use rapina::sea_orm::{EntityTrait, ActiveModelTrait, Set};

#[get("/posts")]
async fn list_posts(db: Db) -> Result<Json<Vec<PostResponse>>> {
    let posts = Post::find()
        .all(db.conn())
        .await
        .map_err(DbError::from)?;

    Ok(Json(posts.into_iter().map(PostResponse::from).collect()))
}

#[post("/posts")]
async fn create_post(body: Json<CreatePost>, db: Db) -> Result<Json<PostResponse>> {
    let post = post::ActiveModel {
        title: Set(body.title.clone()),
        content: Set(body.content.clone()),
        ..Default::default()
    };

    let post = post.insert(db.conn())
        .await
        .map_err(DbError::from)?;

    Ok(Json(PostResponse::from(post)))
}
```

The `DbError` wrapper converts SeaORM errors into Rapina's error responses automatically.

## Defining Entities

### The schema! Macro

The `schema!` macro generates SeaORM entities from a declarative syntax where types define relationships:

```rust
use rapina::prelude::*;

schema! {
    User {
        #[unique]
        email: String,
        name: String,
        posts: Vec<Post>,        // has_many relationship
    }

    #[table_name = "blog_posts"]
    Post {
        title: String,
        content: Text,           // TEXT column type
        author: User,            // belongs_to (generates author_id)
        comments: Vec<Comment>,
    }

    Comment {
        content: Text,
        post: Post,              // belongs_to
        author: Option<User>,    // optional belongs_to
    }
}
```

This generates complete SeaORM entity modules. Each entity automatically includes:

- `id: i32` (primary key)
- `created_at: DateTimeUtc`
- `updated_at: DateTimeUtc`

### Generated Code

For each entity, the macro generates:

- A module (e.g., `user`, `post`)
- `Model` struct with all fields
- `Entity` type for queries
- `ActiveModel` for inserts/updates
- `Relation` enum with relationship definitions
- `Related<T>` trait implementations

Use them in your code:

```rust
use schema::{user, post, User, Post};

// Query
let users = User::find().all(db.conn()).await?;

// Insert
let new_post = post::ActiveModel {
    title: Set("Hello".to_string()),
    content: Set("World".to_string()),
    author_id: Set(1),
    ..Default::default()
};
let post = new_post.insert(db.conn()).await?;

// Update
let mut post: post::ActiveModel = post.into();
post.title = Set("Updated".to_string());
let post = post.update(db.conn()).await?;

// Delete
Post::delete_by_id(1).exec(db.conn()).await?;
```

### Supported Types

| Schema Type | Rust Type | Column Type |
|-------------|-----------|-------------|
| `String` | `String` | VARCHAR |
| `Text` | `String` | TEXT |
| `i32` | `i32` | INTEGER |
| `i64` | `i64` | BIGINT |
| `f32` | `f32` | FLOAT |
| `f64` | `f64` | DOUBLE |
| `bool` | `bool` | BOOLEAN |
| `Uuid` | `Uuid` | UUID |
| `DateTime` | `DateTimeUtc` | TIMESTAMPTZ |
| `Date` | `Date` | DATE |
| `Decimal` | `Decimal` | DECIMAL |
| `Json` | `Json` | JSON |
| `Option<T>` | `Option<T>` | nullable |

### Relationships

Relationships are inferred from types:

| Syntax | Relationship | Generated |
|--------|--------------|-----------|
| `posts: Vec<Post>` | has_many | Relation enum variant |
| `author: User` | belongs_to | `author_id: i32` column |
| `author: Option<User>` | optional belongs_to | `author_id: Option<i32>` |

### Attributes

#### Entity Attributes

| Attribute | Description |
|-----------|-------------|
| `#[table_name = "name"]` | Override the auto-generated table name |
| `#[timestamps(created_at)]` | Only include `created_at` timestamp |
| `#[timestamps(updated_at)]` | Only include `updated_at` timestamp |
| `#[timestamps(none)]` | No automatic timestamps |

```rust
#[table_name = "people"]
Person {
    name: String,
}

#[timestamps(none)]
AuditLog {
    action: String,
    timestamp: DateTime,  // manage your own timestamp
}
```

#### Field Attributes

| Attribute | Description |
|-----------|-------------|
| `#[unique]` | Mark field as unique |
| `#[index]` | Create an index on this column |
| `#[column = "name"]` | Custom column name in database |

```rust
User {
    #[unique]
    email: String,

    #[index]
    username: String,

    #[column = "full_name"]
    name: String,
}
```

## Database Schema

Your database schema should match the generated entities. Example for PostgreSQL:

```sql
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    email VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE posts (
    id SERIAL PRIMARY KEY,
    author_id INTEGER NOT NULL REFERENCES users(id),
    title VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE comments (
    id SERIAL PRIMARY KEY,
    post_id INTEGER NOT NULL REFERENCES posts(id),
    author_id INTEGER REFERENCES users(id),  -- nullable for optional
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

## Error Handling

Database errors are automatically converted to appropriate HTTP responses:

```rust
#[get("/posts/:id")]
async fn get_post(id: Path<i32>, db: Db) -> Result<Json<PostResponse>> {
    let post = Post::find_by_id(id.into_inner())
        .one(db.conn())
        .await
        .map_err(DbError::from)?  // Converts to 500
        .ok_or_else(|| Error::not_found("post not found"))?;  // 404

    Ok(Json(PostResponse::from(post)))
}
```
