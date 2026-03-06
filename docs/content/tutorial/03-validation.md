+++
title = "Validation"
template = "tutorial.html"
weight = 3

[extra]
chapter = 3
prev = "/tutorial/02-protected-routes/"
next = "/tutorial/04-error-handling/"
doc = "/docs/core-concepts/validation/"

code = """use rapina::prelude::*;

#[derive(Deserialize, JsonSchema)]
struct CreateUserRequest {
    name: String,
    email: String,
}

#[derive(Serialize, JsonSchema)]
struct CreateUserResponse {
    id: u64,
    name: String,
}

#[public]
#[post("/users")]
async fn create_user(
    Json(body): Json<CreateUserRequest>,
) -> Json<CreateUserResponse> {
    Json(CreateUserResponse {
        id: 1,
        name: body.name,
    })
}"""

testcases = """[
  {
    "title": "Add validation derives",
    "description": "Add Validate derive to the request struct",
    "pattern": "#\\\\[derive\\\\([^)]*Validate[^)]*\\\\)\\\\]\\\\s*struct\\\\s+CreateUserRequest"
  },
  {
    "title": "Add validation rules",
    "description": "Use #[validate] attributes on struct fields (e.g. length, email)",
    "pattern": "#\\\\[validate\\\\("
  },
  {
    "title": "Use Validated<Json<T>>",
    "description": "Wrap the extractor with Validated for automatic validation",
    "pattern": "Validated<\\\\s*Json<",
    "response": {
      "method": "POST",
      "path": "/users",
      "status": 201,
      "body": { "id": 1, "name": "Alice" }
    }
  }
]"""
+++

# Validation

Rapina uses the `Validated<T>` extractor to validate incoming requests. If validation fails, it automatically returns a `422 Unprocessable Entity` response with details about what went wrong — your handler never runs.

Validation rules are declared on your request struct using the `validator` crate's derive macros:

```rust
#[derive(Deserialize, Validate, JsonSchema)]
struct CreateUserRequest {
    #[validate(length(min = 1, max = 100))]
    name: String,
    #[validate(email)]
    email: String,
}
```

Then wrap your extractor with `Validated`:

```rust
async fn handler(Validated(Json(body)): Validated<Json<CreateUserRequest>>)
```

## Assignment

1. Add the `Validate` derive to `CreateUserRequest`
2. Add `#[validate(length(min = 1))]` to `name` and `#[validate(email)]` to `email`
3. Change the extractor from `Json(body)` to `Validated(Json(body))` with `Validated<Json<CreateUserRequest>>`

{% answer() %}
```rust
use rapina::prelude::*;

#[derive(Deserialize, Validate, JsonSchema)]
struct CreateUserRequest {
    #[validate(length(min = 1, max = 100))]
    name: String,
    #[validate(email)]
    email: String,
}

#[derive(Serialize, JsonSchema)]
struct CreateUserResponse {
    id: u64,
    name: String,
}

#[public]
#[post("/users")]
async fn create_user(
    Validated(Json(body)): Validated<Json<CreateUserRequest>>,
) -> Json<CreateUserResponse> {
    Json(CreateUserResponse {
        id: 1,
        name: body.name,
    })
}
```
{% end %}
