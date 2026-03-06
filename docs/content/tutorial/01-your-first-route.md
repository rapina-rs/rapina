+++
title = "Your First Route"
template = "tutorial.html"
weight = 1

[extra]
chapter = 1
prev = "/tutorial/"
next = "/tutorial/02-protected-routes/"
doc = "/docs/core-concepts/routing/"

code = """use rapina::prelude::*;

#[public]
#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}"""

testcases = """[
  {
    "title": "Route path is /hello",
    "description": "Change the route macro to #[get(\\"/hello\\")]",
    "pattern": "#\\\\[get\\\\(\\\\s*\\"\\\\s*/hello\\\\s*\\"\\\\s*\\\\)\\\\]"
  },
  {
    "title": "Define a response struct",
    "description": "Create a struct with Serialize and JsonSchema derives",
    "pattern": "#\\\\[derive\\\\([^)]*Serialize[^)]*\\\\)\\\\]\\\\s*struct\\\\s+\\\\w+"
  },
  {
    "title": "Return Json<T>",
    "description": "The handler should return a Json<T> response",
    "pattern": "->\\\\s*Json<",
    "response": {
      "method": "GET",
      "path": "/hello",
      "status": 200,
      "body": { "name": "World" }
    }
  }
]"""
+++

# Your First Route

Every Rapina handler is an async function annotated with a route macro. Here's the simplest one:

```rust
#[public]
#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}
```

The `#[public]` attribute makes this endpoint accessible without authentication — by default, all Rapina routes require JWT auth. The `#[get("/")]` macro registers it as a GET route at the root path.

## Assignment

Modify the code to:

1. Change the route path to `/hello`
2. Create a response struct with `Serialize` and `JsonSchema` derives
3. Return a `Json<T>` response with a `name` field set to `"World"`

You'll need a struct like `HelloResponse` with a `name: String` field, and the handler should return `Json(HelloResponse { ... })`.

{% answer() %}
```rust
use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct HelloResponse {
    name: String,
}

#[public]
#[get("/hello")]
async fn hello() -> Json<HelloResponse> {
    Json(HelloResponse {
        name: "World".into(),
    })
}
```
{% end %}
