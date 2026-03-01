+++
title = "Pagination"
description = "Built-in pagination for list endpoints"
weight = 6
date = 2025-02-27
+++

Every list endpoint needs the same boilerplate: LIMIT/OFFSET, a count query, and response metadata. Rapina handles this with a `Paginate` extractor and a `Paginated<T>` response wrapper.

## Quick Start

```rust
use rapina::prelude::*;
use rapina::database::Db;

#[get("/users")]
async fn list_users(db: Db, page: Paginate) -> Result<Paginated<user::Model>> {
    page.exec(User::find(), db.conn()).await
}
```

That's it. The extractor reads `?page=1&per_page=20` from the query string, `exec` runs fetch and count concurrently, and `Paginated<T>` serializes the response with metadata.

## The Paginate Extractor

`Paginate` implements `FromRequestParts` and parses two optional query parameters:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `page` | 1 | Page number (1-indexed) |
| `per_page` | 20 | Items per page |

Returns **422 Validation Error** when:
- `page` < 1
- `per_page` < 1
- `per_page` exceeds the configured maximum (default: 100)

```rust
#[get("/posts")]
async fn list_posts(db: Db, page: Paginate) -> Result<Paginated<post::Model>> {
    let select = Post::find()
        .filter(post::Column::Published.eq(true))
        .order_by_desc(post::Column::CreatedAt);

    page.exec(select, db.conn()).await
}
```

You can apply any SeaORM filters, ordering, or joins before passing the `Select` to `exec`.

## Response Shape

`Paginated<T>` implements `IntoResponse` directly, so you don't need to wrap it in `Json<>`. The response body looks like:

```json
{
  "data": [{ "id": 1, "name": "Alice" }, { "id": 2, "name": "Bob" }],
  "page": 1,
  "per_page": 20,
  "total": 42,
  "total_pages": 3,
  "has_prev": false,
  "has_next": true
}
```

`Paginated<T>` also derives `JsonSchema`, so it shows up correctly in OpenAPI output.

## Configuration

By default, `Paginate` uses `per_page=20` with a maximum of `100`. Override these by registering a `PaginationConfig` in your app state:

```rust
use rapina::prelude::*;

Rapina::new()
    .state(PaginationConfig {
        default_per_page: 25,
        max_per_page: 50,
    })
    // ...
```

If no config is registered, the hardcoded defaults apply. No setup required for the common case.

## Examples

### Basic list endpoint

The simplest case — paginate an entire table:

```rust
use rapina::prelude::*;
use rapina::database::Db;
use entity::user::{self, Entity as User};

#[get("/users")]
async fn list_users(db: Db, page: Paginate) -> Result<Paginated<user::Model>> {
    page.exec(User::find(), db.conn()).await
}
```

```
GET /users              → page 1, 20 items
GET /users?page=3       → page 3, 20 items
GET /users?per_page=50  → page 1, 50 items
```

### Filtering and ordering

Build your query however you want, then hand it to `exec`:

```rust
use rapina::sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

#[get("/posts")]
async fn list_posts(db: Db, page: Paginate) -> Result<Paginated<post::Model>> {
    let select = Post::find()
        .filter(post::Column::Published.eq(true))
        .order_by_desc(post::Column::CreatedAt);

    page.exec(select, db.conn()).await
}
```

### Combining with other extractors

`Paginate` is a `FromRequestParts` extractor, so it composes with everything else:

```rust
#[derive(Deserialize)]
struct UserFilter {
    role: Option<String>,
    active: Option<bool>,
}

#[get("/users")]
async fn list_users(
    db: Db,
    page: Paginate,
    query: Query<UserFilter>,
) -> Result<Paginated<user::Model>> {
    let mut select = User::find();

    if let Some(role) = &query.0.role {
        select = select.filter(user::Column::Role.eq(role.clone()));
    }
    if let Some(active) = query.0.active {
        select = select.filter(user::Column::Active.eq(active));
    }

    page.exec(select, db.conn()).await
}
```

```
GET /users?role=admin&page=2&per_page=10
```

### Mapping to a response DTO

Use `.map()` to transform models into response types. Pagination metadata carries over automatically:

```rust
#[derive(Serialize, JsonSchema)]
struct UserResponse {
    id: i32,
    name: String,
    email: String,
}

impl From<user::Model> for UserResponse {
    fn from(m: user::Model) -> Self {
        Self { id: m.id, name: m.name, email: m.email }
    }
}

#[get("/users")]
async fn list_users(db: Db, page: Paginate) -> Result<Paginated<UserResponse>> {
    Ok(page.exec(User::find(), db.conn()).await?.map(UserResponse::from))
}
```

Works with closures too:

```rust
#[get("/users")]
async fn list_users(db: Db, page: Paginate) -> Result<Paginated<String>> {
    Ok(page.exec(User::find(), db.conn()).await?.map(|u| u.name))
}
```

### Scoped to a parent resource

Pagination works the same with relationship queries:

```rust
#[get("/users/:id/posts")]
async fn list_user_posts(
    id: Path<i32>,
    db: Db,
    page: Paginate,
) -> Result<Paginated<post::Model>> {
    let select = Post::find()
        .filter(post::Column::AuthorId.eq(id.into_inner()))
        .order_by_desc(post::Column::CreatedAt);

    page.exec(select, db.conn()).await
}
```

### Custom per_page limits

For endpoints with heavier payloads, register a global config:

```rust
#[tokio::main]
async fn main() -> std::io::Result<()> {
    let db_config = DatabaseConfig::from_env()?;

    Rapina::new()
        .with_database(db_config).await?
        .state(PaginationConfig {
            default_per_page: 25,
            max_per_page: 50,
        })
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
```

Any request with `?per_page=51` now returns a 422.

## Performance

`exec` runs the data fetch and count queries **concurrently** using `tokio::join!`, not sequentially. Two queries hit the database in parallel, so latency is the cost of whichever query is slower, not both combined.

The `Select<E>` is cloned before splitting into fetch and count paginators. SeaORM query builders are cheap to clone (they're just AST nodes, not connections).
