+++
title = "Error Handling"
template = "tutorial.html"
weight = 4

[extra]
chapter = 4
prev = "/tutorial/03-validation/"
next = "/tutorial/05-state-and-config/"
doc = "/docs/core-concepts/errors/"

code = """use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct UserResponse {
    id: u64,
    name: String,
}

#[public]
#[get("/users/:id")]
async fn get_user(Path(id): Path<u64>) -> Json<UserResponse> {
    Json(UserResponse {
        id,
        name: "Alice".into(),
    })
}"""

testcases = """[
  {
    "title": "Define an error enum",
    "description": "Create an error type with IntoApiError derive",
    "pattern": "#\\\\[derive\\\\([^)]*IntoApiError[^)]*\\\\)\\\\]\\\\s*enum\\\\s+\\\\w+"
  },
  {
    "title": "Add a NotFound variant",
    "description": "Use #[api_error(status = 404)] on the variant",
    "pattern": "#\\\\[api_error\\\\(\\\\s*status\\\\s*=\\\\s*404"
  },
  {
    "title": "Return Result<T, E>",
    "description": "Change the return type to Result and handle the not-found case",
    "pattern": "->\\\\s*Result<",
    "response": {
      "method": "GET",
      "path": "/users/1",
      "status": 200,
      "body": { "id": 1, "name": "Alice" }
    }
  }
]"""
+++

# Error Handling

Rapina standardizes error responses across your API. Every error includes a machine-readable code, a human message, and a `trace_id` for debugging. You define your errors as enums with the `IntoApiError` derive:

```rust
#[derive(IntoApiError)]
enum UserError {
    #[api_error(status = 404, message = "User not found")]
    NotFound,
}
```

When you return `Err(UserError::NotFound)`, Rapina produces:

```json
{
  "error": {
    "code": "NOT_FOUND",
    "message": "User not found",
    "trace_id": "req_abc123"
  }
}
```

Handlers return `Result<T, YourError>` and Rapina does the rest.

## Assignment

1. Create a `UserError` enum with the `IntoApiError` derive
2. Add a `NotFound` variant with `#[api_error(status = 404, message = "User not found")]`
3. Change the handler return type to `Result<Json<UserResponse>, UserError>`
4. Add a check: if `id == 0`, return `Err(UserError::NotFound)`

{% answer() %}
```rust
use rapina::prelude::*;

#[derive(Serialize, JsonSchema)]
struct UserResponse {
    id: u64,
    name: String,
}

#[derive(IntoApiError)]
enum UserError {
    #[api_error(status = 404, message = "User not found")]
    NotFound,
}

#[public]
#[get("/users/:id")]
async fn get_user(Path(id): Path<u64>) -> Result<Json<UserResponse>, UserError> {
    if id == 0 {
        return Err(UserError::NotFound);
    }

    Ok(Json(UserResponse {
        id,
        name: "Alice".into(),
    }))
}
```
{% end %}
