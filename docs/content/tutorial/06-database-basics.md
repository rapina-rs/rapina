+++
title = "Database Basics"
template = "tutorial.html"
weight = 6

[extra]
chapter = 6
prev = "/tutorial/05-state-and-config/"
doc = "/docs/core-concepts/database/"

code = '''use rapina::prelude::*;
use rapina::schemars;

#[derive(Serialize, JsonSchema)]
struct TodoResponse {
    id: i32,
    title: String,
    done: bool,
}

#[public]
#[get("/todos")]
async fn list_todos() -> Json<Vec<TodoResponse>> {
    Json(vec![])
}'''

testcases = '''[
  {
    "title": "Add Db extractor",
    "description": "Use Db as a handler parameter to access the shared connection pool",
    "pattern": "fn\\s+\\w+\\s*\\([^)]*\\bDb\\b"
  },
  {
    "title": "Define the schema",
    "description": "Declare a Todo entity with title and done fields; id is generated automatically",
    "pattern": "schema!\\s*\\{\\s*(?:#\\[[^\\]]*\\]\\s*)*Todo\\s*\\{"
  },
  {
    "title": "Query through SeaORM",
    "description": "Fetch entities with todo::Entity::find() and .all(db.conn())",
    "pattern": "todo::Entity::find\\(\\).*?\\.all\\(db\\.conn\\(\\)\\)"
  },
  {
    "title": "Propagate database errors",
    "description": "Return Result and convert SeaORM errors through DbError instead of unwrapping",
    "pattern": "->\\s*Result<.*?\\.map_err\\(DbError::from\\)\\?",
    "response": {
      "method": "GET",
      "path": "/todos",
      "status": 200,
      "body": [
        {
          "id": 1,
          "title": "Learn Rapina",
          "done": false
        },
        {
          "id": 2,
          "title": "Build an API",
          "done": true
        }
      ]
    }
  }
]'''
+++

# Database Basics

Rapina's database integration uses SeaORM. The `Db` extractor gives handlers
access to the configured connection pool; `db.conn()` supplies the connection
to SeaORM queries. There is no `db.query()` builder.

## Local setup

Enable the SQLite backend in your existing project's `Cargo.toml`:

```toml
rapina = { version = "0.13.1", features = ["sqlite"] }
```

Keep the `tokio` and `serde` dependencies from the earlier chapters.
For this exercise, install the SQLite CLI and run `sqlite3 todos.db` in your
project directory, then execute:

```sql
CREATE TABLE todos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    done BOOLEAN NOT NULL
);
INSERT INTO todos (title, done) VALUES
    ('Learn Rapina', 0),
    ('Build an API', 1);
```

Exit SQLite with `.quit`. Use a fresh database for these statements; running the
inserts again adds more rows. For production schema changes, use
[migrations](/docs/core-concepts/migrations/).

## Entities and queries

`schema!` declares an entity such as `Todo { title: String, done: bool }`,
not a SQL `table todos` block. It generates the `todo` module and an `id: i32`
primary key automatically. `#[table_name = "todos"]` maps it to our table;
`#[timestamps(none)]` omits the default timestamp fields because our table does
not have those columns. The macro describes the mapping; it does not create
the database table.

Import `EntityTrait` and query with `todo::Entity::find().all(db.conn())`.
`QueryOrder` lets us order by ID so the output is deterministic. Convert the
returned models into `TodoResponse` values before wrapping them in `Json`.

Handlers that can fail return `Result<Json<...>>`. Convert SeaORM errors with
`.map_err(DbError::from)?`; Rapina turns the wrapper into an API error rather
than panicking on a database failure. Import `Db`, `DbError`, and
`DatabaseConfig` from `rapina::database` explicitly.

The answer below includes startup code that registers the SQLite connection
with `.with_database(...).await?`. Without it, the `Db` extractor cannot find a
connection. Save the answer as `src/main.rs`, then run `cargo run` and request
`GET http://127.0.0.1:3000/todos`.

The browser editor checks code patterns and shows a **simulated** response;
it does not compile Rust or run SQLite. Run the local project to verify the
actual query and error handling.

## Assignment

1. Add `db: Db` as a handler parameter and the explicit database imports.
2. Declare the `Todo` entity mapped to `todos`, without timestamp columns.
3. Query with SeaORM, order by ID, and map the models into `TodoResponse`.
4. Return `Result` and propagate database failures through `DbError`.
5. Configure the SQLite connection at startup for the local application.

This is the final chapter. Explore the [full documentation](/docs/introduction/what-is-rapina/)
to go deeper.

{% answer() %}
```rust
use rapina::database::{DatabaseConfig, Db, DbError};
use rapina::prelude::*;
use rapina::schemars;
use rapina::sea_orm::{EntityTrait, QueryOrder};

schema! {
    #[table_name = "todos"]
    #[timestamps(none)]
    Todo {
        title: String,
        done: bool,
    }
}

#[derive(Serialize, JsonSchema)]
struct TodoResponse {
    id: i32,
    title: String,
    done: bool,
}

#[public]
#[get("/todos")]
async fn list_todos(db: Db) -> Result<Json<Vec<TodoResponse>>> {
    let todos = todo::Entity::find()
        .order_by_asc(todo::Column::Id)
        .all(db.conn())
        .await
        .map_err(DbError::from)?;

    Ok(Json(
        todos
            .into_iter()
            .map(|todo| TodoResponse {
                id: todo.id,
                title: todo.title,
                done: todo.done,
            })
            .collect(),
    ))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .with_database(DatabaseConfig::new("sqlite://todos.db?mode=rwc"))
        .await?
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
```
{% end %}
