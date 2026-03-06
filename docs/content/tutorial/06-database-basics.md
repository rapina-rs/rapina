+++
title = "Database Basics"
template = "tutorial.html"
weight = 6

[extra]
chapter = 6
prev = "/tutorial/05-state-and-config/"
doc = "/docs/core-concepts/database/"

code = """use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct TodoResponse {
    id: u64,
    title: String,
    done: bool,
}

#[public]
#[get("/todos")]
async fn list_todos() -> Json<Vec<TodoResponse>> {
    Json(vec![])
}"""

testcases = """[
  {
    "title": "Add Db extractor",
    "description": "Use Db as a handler parameter to get a database connection",
    "pattern": "fn\\\\s+\\\\w+\\\\s*\\\\([^)]*Db\\\\b"
  },
  {
    "title": "Define the schema",
    "description": "Use the schema! macro to define your table structure",
    "pattern": "schema!\\\\s*\\\\{"
  },
  {
    "title": "Query the database",
    "description": "Use db.query or a select statement to fetch todos",
    "pattern": "db\\\\.",
    "response": {
      "method": "GET",
      "path": "/todos",
      "status": 200,
      "body": [
        { "id": 1, "title": "Learn Rapina", "done": false },
        { "id": 2, "title": "Build an API", "done": true }
      ]
    }
  }
]"""
+++

# Database Basics

Rapina provides the `Db` extractor for database access. It gives you a connection from the pool, ready to query.

Define your table schema with the `schema!` macro:

```rust
schema! {
    table todos {
        id: u64,
        title: String,
        done: bool,
    }
}
```

Then use the `Db` extractor in your handler:

```rust
async fn handler(db: Db) -> Json<Vec<T>> {
    let todos = db.query("SELECT id, title, done FROM todos")
        .fetch_all()
        .await?;
    Json(todos)
}
```

The connection is automatically returned to the pool when the handler completes.

## Assignment

1. Add `db: Db` as a handler parameter
2. Define a `todos` table schema using the `schema!` macro with fields `id`, `title`, and `done`
3. Use `db.query(...)` to fetch todos and return them

This is the final chapter — you've learned the core concepts of Rapina. From here, explore the [full documentation](/docs/introduction/what-is-rapina/) to go deeper.

{% answer() %}
```rust
use rapina::prelude::*;

schema! {
    table todos {
        id: u64,
        title: String,
        done: bool,
    }
}

#[derive(Serialize, JsonSchema)]
struct TodoResponse {
    id: u64,
    title: String,
    done: bool,
}

#[public]
#[get("/todos")]
async fn list_todos(db: Db) -> Json<Vec<TodoResponse>> {
    let todos = db.query("SELECT id, title, done FROM todos")
        .fetch_all()
        .await
        .unwrap();
    Json(todos)
}
```
{% end %}
